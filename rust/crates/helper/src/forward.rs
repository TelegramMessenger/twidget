//! tdesktop's `test/test_widget_renderer_forward.h`: what the helper's Debug tap forwards (a renderer wrapped,
//! every call and message seen by an observer) and the helper work record the tap writes and Telegram's hosted
//! tap reads (way `w` of a `Mirrored` record).

use std::sync::{Arc, Mutex, PoisonError};

use crate::seams::{Link, Renderer};
use crate::wire::details;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererCall {
    Load,
    Send,
    SetTheme,
    SetWidth,
    Destroy,
}

/// What one tap records around the renderer it wraps. Every hook runs on the thread of the call or callback it
/// observes and must not call into the renderer; the defaults do nothing.
pub trait RendererObserver: Send + Sync {
    /// Right before a call is forwarded: `text` is the message of Send and the theme of SetTheme, empty for the
    /// other calls.
    fn calling(&self, _call: RendererCall, _text: &[u8]) {}
    /// Right after the forwarded call returned or panicked: every `calling` gets its `called`.
    fn called(&self, _call: RendererCall) {}
    /// A renderer → host message, before the wrapper's `onMessage` gets it.
    fn received(&self, _json: &[u8]) {}
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Forwards every call to `inner` unchanged, on the calling thread, and chains `inner`'s `onMessage` and `onDiag`
/// to its own, which the served host sets once the factory returned, so they are read when invoked, never when
/// installed. Dropping it drops `inner` first and only then `observer`.
pub struct ForwardingRenderer {
    inner: Option<Box<dyn Renderer>>,
    observer: Arc<dyn RendererObserver>,
    outer: Arc<Mutex<Option<Link>>>,
}

impl std::fmt::Debug for ForwardingRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ForwardingRenderer")
    }
}

struct Called<'a> {
    observer: &'a dyn RendererObserver,
    call: RendererCall,
}

impl Drop for Called<'_> {
    fn drop(&mut self) {
        self.observer.called(self.call);
    }
}

impl ForwardingRenderer {
    pub fn new(inner: Box<dyn Renderer>, observer: Box<dyn RendererObserver>) -> ForwardingRenderer {
        let observer: Arc<dyn RendererObserver> = Arc::from(observer);
        let outer: Arc<Mutex<Option<Link>>> = Arc::new(Mutex::new(None));
        let (seen, messages, diags) = (Arc::clone(&observer), Arc::clone(&outer), Arc::clone(&outer));
        inner.connect(Link::new(
            move |json| {
                seen.received(json);
                let link = lock(&messages).clone();
                if let Some(link) = link {
                    (link.message)(json);
                }
            },
            move |kind, detail| {
                let link = lock(&diags).clone();
                if let Some(link) = link {
                    (link.diag)(kind, detail);
                }
            },
        ));
        ForwardingRenderer { inner: Some(inner), observer, outer }
    }

    fn forward(&self, call: RendererCall, text: &[u8], method: impl FnOnce(&dyn Renderer)) {
        self.observer.calling(call, text);
        let _called = Called { observer: &*self.observer, call };
        if let Some(inner) = &self.inner {
            method(&**inner);
        }
    }
}

impl Drop for ForwardingRenderer {
    fn drop(&mut self) {
        self.inner = None;
    }
}

impl Renderer for ForwardingRenderer {
    fn connect(&self, link: Link) {
        *lock(&self.outer) = Some(link);
    }

    fn load(&self) {
        self.forward(RendererCall::Load, b"", |inner| inner.load());
    }

    fn send(&self, json: &[u8]) {
        self.forward(RendererCall::Send, json, |inner| inner.send(json));
    }

    fn set_theme(&self, theme: &[u8]) {
        self.forward(RendererCall::SetTheme, theme, |inner| inner.set_theme(theme));
    }

    fn set_width(&self, width: f64) {
        self.forward(RendererCall::SetWidth, b"", |inner| inner.set_width(width));
    }

    fn destroy(&self) {
        self.forward(RendererCall::Destroy, b"", |inner| inner.destroy());
    }
}

/// What one helper renderer operation was, by the call that started it on the served host's glue thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HelperWorkKind {
    /// A served-host → renderer message: TwidgetCore's `receive()`, a validated batch applied or a layout query
    /// answered, with the style and layout they need.
    #[default]
    Message = 1,
    /// The served host's `setTheme()`: the scheme applied to the renderer.
    Theme = 2,
    /// A FrameRequest that ticks and does not paint: TwidgetCore's `tick()`, its animations and the build they need.
    Tick = 3,
    /// A FrameRequest that paints: the settle, style, layout, the display list build and the Frame's encoding.
    Paint = 4,
    /// Any other TwidgetCore platform message: input, sizes, selection and claims.
    Platform = 5,
    /// A glue control: Open (the renderer end's construction, its theme and first layout) and the press controls.
    Control = 6,
}

impl HelperWorkKind {
    fn from_u8(value: u8) -> Option<HelperWorkKind> {
        Some(match value {
            1 => HelperWorkKind::Message,
            2 => HelperWorkKind::Theme,
            3 => HelperWorkKind::Tick,
            4 => HelperWorkKind::Paint,
            5 => HelperWorkKind::Platform,
            6 => HelperWorkKind::Control,
            _ => return None,
        })
    }
}

/// One way `w` Mirrored record: an outermost renderer operation of one widget, timed on the helper's steady clock
/// (microseconds since its epoch).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HelperWorkRecord {
    pub started: i64,
    pub finished: i64,
    /// Microseconds of [started, finished] spent inside the platform's `request()`, waiting for Telegram's answers,
    /// and how many.
    pub waited: i64,
    /// The FrameRequest's seq for Tick and Paint, else 0.
    pub seq: u64,
    pub waits: u32,
    /// Bytes of the Frame messages the operation sent, 0 for none.
    pub frame_bytes: u32,
    pub kind: HelperWorkKind,
}

pub const HELPER_WORK_RECORD_BYTES: usize = 1 + 8 * 4 + 4 * 2;

/// The longest interval one record may span, in microseconds: an hour.
pub const MAX_HELPER_INTERVAL: i64 = 3600 * 1000 * 1000;

pub fn encode_helper_work(record: &HelperWorkRecord) -> Vec<u8> {
    let mut result = Vec::with_capacity(HELPER_WORK_RECORD_BYTES);
    result.push(record.kind as u8);
    details::put_number(&mut result, record.started as u64, 8);
    details::put_number(&mut result, record.finished as u64, 8);
    details::put_number(&mut result, record.waited as u64, 8);
    details::put_number(&mut result, record.seq, 8);
    details::put_number(&mut result, u64::from(record.waits), 4);
    details::put_number(&mut result, u64::from(record.frame_bytes), 4);
    result
}

/// Exactly `HELPER_WORK_RECORD_BYTES`, a known kind, 0 <= started <= finished with finished - started at most
/// `MAX_HELPER_INTERVAL`, and 0 <= waited <= finished - started; None otherwise.
pub fn decode_helper_work(text: &[u8]) -> Option<HelperWorkRecord> {
    if text.len() != HELPER_WORK_RECORD_BYTES {
        return None;
    }
    let kind = text[0];
    let mut text = &text[1..];
    let mut result = HelperWorkRecord {
        started: details::take_number(&mut text, 8) as i64,
        finished: details::take_number(&mut text, 8) as i64,
        waited: details::take_number(&mut text, 8) as i64,
        seq: details::take_number(&mut text, 8),
        waits: details::take_number(&mut text, 4) as u32,
        frame_bytes: details::take_number(&mut text, 4) as u32,
        kind: HelperWorkKind::Message,
    };
    let known = HelperWorkKind::from_u8(kind);
    if known.is_none() || result.started < 0 || result.finished < result.started {
        return None;
    }
    let interval = result.finished - result.started;
    if interval > MAX_HELPER_INTERVAL || result.waited < 0 || result.waited > interval {
        return None;
    }
    result.kind = known?;
    Some(result)
}
