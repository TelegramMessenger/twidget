//! tdesktop's `test/test_widget_helper_tap.{h,cpp}`: the helper half of the validated-ops tap for widgets hosted in
//! the widget helper. The C++ compiles it in the Debug helper and takes the identity (`build(platform)`) in Release;
//! here both are functions and the embedder chooses ([`tap_helper_renderer`], [`untapped_helper_renderer`]).
//!
//! The renderer `build` returns is wrapped in a [`ForwardingRenderer`] and gets a platform that forwards `send()`
//! and `request()` unchanged and times each `request()`; the platform's `onMessage` is chained to the one `build`
//! set: an exact `Mirror` or `MirrorWork` control is swallowed and arms the tap, every other message reaches the
//! chained handler (through weak references, so a control after the wrapper is gone does nothing). Armed, the tap
//! first sends one `Mirrored` record of way `b` with the factory's scheme, then, when a `set_theme()` changed it
//! before the arming, one of way `t`; from then on way `h` for every served-host → renderer message but `ops`, way
//! `r` for every renderer → host message and way `t` for every `set_theme()`, each before it is forwarded. Armed by
//! `MirrorWork` it also sends one way `w` [`HelperWorkRecord`] after each outermost renderer call and each outermost
//! platform message it chained. A record past the platform limit is skipped. Every call runs on the served host's
//! glue thread.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use twidget_remote::{FrameRequest, Kind as RemoteKind, Message, Paint, kind_of as remote_kind_of};

use crate::forward::{ForwardingRenderer, HelperWorkKind, HelperWorkRecord, RendererCall, RendererObserver, encode_helper_work};
use crate::seams::{Platform, PlatformObserver, Renderer};
use crate::wire;

/// The helper's steady clock in microseconds since its epoch, the clock Telegram's process reads too.
pub type SteadyMicros = Arc<dyn Fn() -> i64 + Send + Sync>;

/// What `build` gets and returns: the renderer for the platform given (none when there is no platform or it made
/// none).
pub type HelperRendererBuild<'a> = &'a dyn Fn(Option<Arc<dyn Platform>>) -> Option<Box<dyn Renderer>>;

/// The served host's own prefix of every validated batch it sends.
const OPS_PREFIX: &[u8] = b"{\"type\":\"ops\",";

/// Shared by the wrapper's observer, the timed platform and the chained platform handler, read and written on the
/// glue thread alone.
struct TapState {
    platform: Arc<dyn Platform>,
    factory_scheme: Vec<u8>,
    scheme: Vec<u8>,
    /// The outermost operation being timed, while `depth` is above 0.
    work: Option<HelperWorkRecord>,
    depth: i32,
    armed: bool,
    timing: bool,
}

struct Tap {
    state: Mutex<TapState>,
    clock: SteadyMicros,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn is_ops(json: &[u8]) -> bool {
    json.starts_with(OPS_PREFIX)
}

/// A record of `way` to the platform, while armed; one past the limit is skipped.
fn mirror(state: &TapState, way: u8, text: &[u8]) {
    if !state.armed {
        return;
    }
    if let Some(bytes) = wire::encode_mirrored(way, text) {
        state.platform.send(&bytes);
    }
}

impl Tap {
    fn arm(&self, timing: bool) {
        let mut state = lock(&self.state);
        state.timing = state.timing || timing;
        if std::mem::replace(&mut state.armed, true) {
            return;
        }
        let built = state.factory_scheme.clone();
        mirror(&state, wire::MIRRORED_BUILT, &built);
        if state.scheme != state.factory_scheme {
            let scheme = state.scheme.clone();
            mirror(&state, wire::MIRRORED_THEME, &scheme);
        }
    }

    fn begin_work(&self, kind: HelperWorkKind, seq: u64) {
        let mut state = lock(&self.state);
        let depth = state.depth;
        state.depth += 1;
        if depth != 0 || !state.timing {
            return;
        }
        state.work = Some(HelperWorkRecord { started: (self.clock)(), seq, kind, ..HelperWorkRecord::default() });
    }

    /// An operation that panicked sends no record: no send while unwinding.
    fn end_work(&self) {
        let mut state = lock(&self.state);
        state.depth -= 1;
        if state.depth != 0 {
            return;
        }
        let Some(mut record) = state.work.take() else { return };
        record.finished = (self.clock)();
        if !std::thread::panicking() {
            mirror(&state, wire::MIRRORED_WORK, &encode_helper_work(&record));
        }
    }
}

/// One outermost operation of the glue thread, timed when the tap times.
struct WorkScope<'a> {
    tap: &'a Tap,
}

impl<'a> WorkScope<'a> {
    fn new(tap: &'a Tap, kind: HelperWorkKind, seq: u64) -> WorkScope<'a> {
        tap.begin_work(kind, seq);
        WorkScope { tap }
    }
}

impl Drop for WorkScope<'_> {
    fn drop(&mut self) {
        self.tap.end_work();
    }
}

/// What `build` gets for the platform: the tap's platform, with each `request()` timed into the operation being
/// timed.
struct TimedPlatform {
    tap: Arc<Tap>,
    on_message: Mutex<Option<PlatformObserver>>,
}

impl Platform for TimedPlatform {
    fn send(&self, message: &[u8]) {
        let kind = remote_kind_of(message);
        let mut state = lock(&self.tap.state);
        if let Some(work) = &mut state.work
            && (kind == Some(RemoteKind::Frame) || kind == Some(RemoteKind::FrameDelta))
        {
            work.frame_bytes = work.frame_bytes.wrapping_add(message.len() as u32);
        }
        let platform = Arc::clone(&state.platform);
        drop(state);
        platform.send(message);
    }

    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        let state = lock(&self.tap.state);
        let platform = Arc::clone(&state.platform);
        if state.work.is_none() {
            drop(state);
            return platform.request(message);
        }
        drop(state);
        let started = (self.tap.clock)();
        let result = platform.request(message);
        let ended = (self.tap.clock)();
        let mut state = lock(&self.tap.state);
        if let Some(work) = &mut state.work {
            work.waited = work.waited.wrapping_add(ended.wrapping_sub(started));
            work.waits = work.waits.wrapping_add(1);
        }
        result
    }

    fn set_on_message(&self, observer: PlatformObserver) {
        *lock(&self.on_message) = Some(observer);
    }
}

struct HelperObserver {
    tap: Arc<Tap>,
}

impl RendererObserver for HelperObserver {
    fn calling(&self, call: RendererCall, text: &[u8]) {
        match call {
            RendererCall::Send => {
                if !is_ops(text) {
                    mirror(&lock(&self.tap.state), wire::MIRRORED_HOST, text);
                }
                self.tap.begin_work(HelperWorkKind::Message, 0);
            }
            RendererCall::SetTheme => {
                {
                    let mut state = lock(&self.tap.state);
                    state.scheme = text.to_vec();
                    mirror(&state, wire::MIRRORED_THEME, text);
                }
                self.tap.begin_work(HelperWorkKind::Theme, 0);
            }
            _ => {}
        }
    }

    fn called(&self, call: RendererCall) {
        if call == RendererCall::Send || call == RendererCall::SetTheme {
            self.tap.end_work();
        }
    }

    fn received(&self, json: &[u8]) {
        mirror(&lock(&self.tap.state), wire::MIRRORED_RENDERER, json);
    }
}

/// The kind of operation a platform message starts, and a FrameRequest's seq.
fn platform_kind(message: &[u8], seq: &mut u64) -> HelperWorkKind {
    if wire::is_control(message) {
        return HelperWorkKind::Control;
    } else if remote_kind_of(message) != Some(RemoteKind::FrameRequest) {
        return HelperWorkKind::Platform;
    }
    let Some(request) = FrameRequest::decode(message, &wire::embedding_caps()) else { return HelperWorkKind::Platform };
    *seq = request.seq;
    if request.paint != Paint::Skip {
        HelperWorkKind::Paint
    } else if request.tick {
        HelperWorkKind::Tick
    } else {
        HelperWorkKind::Platform
    }
}

fn chained(weak: &Weak<Tap>, timed: &Weak<TimedPlatform>, message: &[u8]) {
    let state = weak.upgrade();
    if let Some(state) = &state
        && wire::is_exactly(message, wire::Kind::Mirror)
    {
        state.arm(false);
        return;
    } else if let Some(state) = &state
        && wire::is_exactly(message, wire::Kind::MirrorWork)
    {
        state.arm(true);
        return;
    }
    let Some(platform) = timed.upgrade() else { return };
    let Some(on_message) = lock(&platform.on_message).clone() else { return };
    let Some(state) = state else {
        on_message(message);
        return;
    };
    let mut seq = 0;
    let kind = platform_kind(message, &mut seq);
    let _scope = WorkScope::new(&state, kind, seq);
    on_message(message);
}

/// The renderer the helper's hosted factory returns for one widget in a Debug helper: `build`'s, wrapped and on a
/// timed platform as the module says, its records stamped by `clock`. Without a platform, `build(None)` itself.
pub fn tap_helper_renderer(
    platform: Option<Arc<dyn Platform>>,
    scheme: &[u8],
    clock: SteadyMicros,
    build: HelperRendererBuild<'_>,
) -> Option<Box<dyn Renderer>> {
    let Some(platform) = platform else { return build(None) };
    let state = Arc::new(Tap {
        state: Mutex::new(TapState {
            platform: Arc::clone(&platform),
            factory_scheme: scheme.to_vec(),
            scheme: scheme.to_vec(),
            work: None,
            depth: 0,
            armed: false,
            timing: false,
        }),
        clock,
    });
    let timed = Arc::new(TimedPlatform { tap: Arc::clone(&state), on_message: Mutex::new(None) });
    let renderer = build(Some(Arc::clone(&timed) as Arc<dyn Platform>))?;
    let (weak, timed) = (Arc::downgrade(&state), Arc::downgrade(&timed));
    platform.set_on_message(Arc::new(move |message: &[u8]| chained(&weak, &timed, message)));
    Some(Box::new(ForwardingRenderer::new(renderer, Box::new(HelperObserver { tap: state }))))
}

/// The renderer a Release helper's hosted factory returns: `build(platform)` itself.
pub fn untapped_helper_renderer(platform: Option<Arc<dyn Platform>>, build: HelperRendererBuild<'_>) -> Option<Box<dyn Renderer>> {
    build(platform)
}
