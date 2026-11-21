//! Scripted sessions through the Debug tap, as `oracle/helper_tap_oracle.cpp` runs them through the C++ one: a
//! scripted platform that logs what it is sent and answers requests ("A:" and the request, none for one starting
//! with 'X'), a scripted inner renderer that logs every call and runs the step's next action list (a message or a
//! diagnostic to the host, a platform send or request, a panic), and a host that logs what it gets and calls the
//! renderer again for `@again:` and `@theme:` messages. Every step answers its transcript.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use twidget_helper::{Link, Platform, PlatformObserver, Renderer, tap_helper_renderer};

use super::Out;

const LOGGED_TEXT_BYTES: usize = 1 << 20;

#[derive(Clone, Debug)]
pub struct Action {
    pub kind: u8,
    pub first: Vec<u8>,
    pub second: Vec<u8>,
}

/// One record of a session: `B`egin, a call (`L`, `S`, `T`, `W`, `D`), a `P`latform message or the `E`nd, with
/// the action lists the inner renderer's calls take in turn.
#[derive(Clone, Debug)]
pub enum Step {
    Begin { platform: bool, renderer: bool, scheme: Vec<u8> },
    Call { op: u8, text: Vec<u8>, width: f64, actions: Vec<Vec<Action>> },
}

impl Step {
    pub fn record(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            Step::Begin { platform, renderer, scheme } => {
                out.u8(u8::from(*platform) | (u8::from(*renderer) << 1)).str(scheme);
                twidget_testkit::framed(b'B', &out.0)
            }
            Step::Call { op, text, width, actions } => {
                if matches!(op, b'S' | b'T' | b'P') {
                    out.str(text);
                } else if *op == b'W' {
                    out.f64(*width);
                }
                out.u32(actions.len() as u32);
                for list in actions {
                    out.u32(list.len() as u32);
                    for a in list {
                        out.u8(a.kind).str(&a.first).str(&a.second);
                    }
                }
                twidget_testkit::framed(*op, &out.0)
            }
        }
    }
}

#[derive(Default)]
struct Shared {
    log: Mutex<Out>,
    pending: Mutex<VecDeque<Vec<Action>>>,
}

impl Shared {
    fn log(&self, f: impl FnOnce(&mut Out)) {
        f(&mut self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn text(&self, text: &[u8]) {
        self.log(|o| {
            if text.len() > LOGGED_TEXT_BYTES {
                let mut hash = 14695981039346656037u64;
                for &c in text {
                    hash = (hash ^ u64::from(c)).wrapping_mul(1099511628211);
                }
                o.u8(b'#').u64(text.len() as u64).u64(hash);
            } else {
                o.u8(b'=').str(text);
            }
        });
    }
}

struct ScriptedPlatform {
    shared: Arc<Shared>,
    on_message: Mutex<Option<PlatformObserver>>,
}

impl Platform for ScriptedPlatform {
    fn send(&self, message: &[u8]) {
        self.shared.log(|o| {
            o.u8(b'p');
        });
        self.shared.text(message);
    }

    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        self.shared.log(|o| {
            o.u8(b'q');
        });
        self.shared.text(message);
        let answer = if message.first() == Some(&b'X') { None } else { Some([&b"A:"[..], message].concat()) };
        self.shared.log(|o| {
            o.u8(b'a').flag(answer.is_some()).str(answer.as_deref().unwrap_or_default());
        });
        answer
    }

    fn set_on_message(&self, observer: PlatformObserver) {
        *self.on_message.lock().unwrap() = Some(observer);
    }
}

struct InnerCore {
    shared: Arc<Shared>,
    platform: Option<Arc<dyn Platform>>,
    link: Mutex<Option<Link>>,
}

impl InnerCore {
    fn call(&self, kind: u8, text: &[u8]) {
        self.shared.log(|o| {
            o.u8(b'i').u8(kind);
        });
        self.shared.text(text);
        self.act();
    }

    fn act(&self) {
        let Some(actions) = self.shared.pending.lock().unwrap().pop_front() else { return };
        for action in actions {
            let link = self.link.lock().unwrap().clone();
            match action.kind {
                0 => {
                    if let Some(link) = link {
                        (link.message)(&action.first);
                    }
                }
                1 => {
                    if let Some(link) = link {
                        (link.diag)(&action.first, &action.second);
                    }
                }
                2 => {
                    if let Some(p) = &self.platform {
                        p.send(&action.first);
                    }
                }
                3 => {
                    if let Some(p) = &self.platform {
                        let answer = p.request(&action.first);
                        self.shared.log(|o| {
                            o.u8(b'A').flag(answer.is_some());
                        });
                    }
                }
                _ => panic!("scripted"),
            }
        }
    }
}

struct ScriptedInner {
    core: Arc<InnerCore>,
}

impl ScriptedInner {
    fn new(platform: Option<Arc<dyn Platform>>, shared: Arc<Shared>) -> ScriptedInner {
        let core = Arc::new(InnerCore { shared, platform, link: Mutex::new(None) });
        if let Some(p) = &core.platform {
            let weak = Arc::downgrade(&core);
            p.set_on_message(Arc::new(move |message: &[u8]| {
                if let Some(core) = weak.upgrade() {
                    core.call(b'P', message);
                }
            }));
        }
        ScriptedInner { core }
    }
}

impl Drop for ScriptedInner {
    fn drop(&mut self) {
        self.core.shared.log(|o| {
            o.u8(b'z');
        });
    }
}

impl Renderer for ScriptedInner {
    fn connect(&self, link: Link) {
        *self.core.link.lock().unwrap() = Some(link);
    }
    fn load(&self) {
        self.core.call(b'L', b"");
    }
    fn send(&self, json: &[u8]) {
        self.core.call(b'S', json);
    }
    fn set_theme(&self, theme: &[u8]) {
        self.core.call(b'T', theme);
    }
    fn set_width(&self, width: f64) {
        self.core.shared.log(|o| {
            o.u8(b'i').u8(b'W').f64(width);
        });
        self.core.act();
    }
    fn destroy(&self) {
        self.core.call(b'D', b"");
    }
}

/// One session's driver, as the oracle's globals hold one.
#[derive(Default)]
pub struct Session {
    shared: Arc<Shared>,
    reads: Arc<AtomicI64>,
    platform: Option<Arc<ScriptedPlatform>>,
    held: Option<Arc<dyn Renderer>>,
}

fn thin<T: ?Sized>(p: &Arc<T>) -> *const u8 {
    Arc::as_ptr(p) as *const u8
}

impl Session {
    /// The step's transcript.
    pub fn run(&mut self, step: &Step) -> Vec<u8> {
        *self.shared.log.lock().unwrap() = Out::default();
        match step {
            Step::Begin { platform, renderer, scheme } => self.begin(*platform, *renderer, scheme),
            Step::Call { op, text, width, actions } => self.call(*op, text, *width, actions),
        }
        std::mem::take(&mut self.shared.log.lock().unwrap().0)
    }

    fn begin(&mut self, platform: bool, renderer: bool, scheme: &[u8]) {
        self.reads.store(0, Ordering::SeqCst);
        self.held = None;
        self.platform = platform.then(|| Arc::new(ScriptedPlatform { shared: Arc::clone(&self.shared), on_message: Mutex::new(None) }));
        let original: Option<Arc<dyn Platform>> = self.platform.clone().map(|p| p as Arc<dyn Platform>);
        let reads = Arc::clone(&self.reads);
        let clock = Arc::new(move || 1_000_000 + 7 * (reads.fetch_add(1, Ordering::SeqCst) + 1));
        let shared = Arc::clone(&self.shared);
        let build = |tapped: Option<Arc<dyn Platform>>| -> Option<Box<dyn Renderer>> {
            let same = match (&tapped, &original) {
                (Some(a), Some(b)) => thin(a) == thin(b),
                (None, None) => true,
                _ => false,
            };
            shared.log(|o| {
                o.u8(b'b').flag(tapped.is_some()).flag(same);
            });
            renderer.then(|| Box::new(ScriptedInner::new(tapped, Arc::clone(&shared))) as Box<dyn Renderer>)
        };
        let made = tap_helper_renderer(original.clone(), scheme, clock, &build);
        self.shared.log(|o| {
            o.u8(b'r').flag(made.is_some());
        });
        self.held = made.map(Arc::from);
        if let Some(held) = &self.held {
            let wrapper: Weak<dyn Renderer> = Arc::downgrade(held);
            let (messages, diags) = (Arc::clone(&self.shared), Arc::clone(&self.shared));
            held.connect(Link::new(
                move |json: &[u8]| {
                    messages.log(|o| {
                        o.u8(b'm');
                    });
                    messages.text(json);
                    let strong = wrapper.upgrade();
                    if let (Some(rest), Some(w)) = (json.strip_prefix(b"@again:"), &strong) {
                        w.send(rest);
                    } else if let (Some(rest), Some(w)) = (json.strip_prefix(b"@theme:"), &strong) {
                        w.set_theme(rest);
                    }
                },
                move |kind: &[u8], detail: &[u8]| {
                    diags.log(|o| {
                        o.u8(b'g').str(kind).str(detail);
                    });
                },
            ));
        }
    }

    fn call(&mut self, op: u8, text: &[u8], width: f64, actions: &[Vec<Action>]) {
        *self.shared.pending.lock().unwrap() = actions.iter().cloned().collect();
        let held = self.held.clone();
        let platform = self.platform.clone();
        let mut end = false;
        let result = catch_unwind(AssertUnwindSafe(|| match op {
            b'P' => {
                let observer = platform.as_ref().and_then(|p| p.on_message.lock().unwrap().clone());
                if let Some(observer) = observer {
                    observer(text);
                }
            }
            b'E' => end = true,
            _ => {
                if let Some(held) = &held {
                    match op {
                        b'L' => held.load(),
                        b'S' => held.send(text),
                        b'T' => held.set_theme(text),
                        b'W' => held.set_width(width),
                        b'D' => held.destroy(),
                        _ => {}
                    }
                }
            }
        }));
        drop(held);
        if end {
            self.held = None;
        }
        if result.is_err() {
            self.shared.log(|o| {
                o.u8(b'x');
            });
        }
        self.shared.pending.lock().unwrap().clear();
    }
}
