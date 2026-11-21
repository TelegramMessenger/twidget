//! Scripted sessions through the hosted glue, as `oracle/hosted_glue_oracle.cpp` runs them through the C++ one: a
//! platform that logs every message and request and answers the requests from a platform end over the log backend
//! (twidget_ends' tests), a host that logs the renderer's messages and diagnostics and, at `ready`, sends the messages
//! the served host kept for it (inside that call, as the served host does), and a step clock both the renderer and
//! the frame message read. Every record answers its transcript.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use twidget_document::Process;
use twidget_ends::{Channel, Host, PlatformEnd};
use twidget_helper::{HostedOptions, Link, Platform, PlatformObserver, Renderer, make_hosted_renderer, prepare_hosted_renderers, wire};
use twidget_json::Json;
use twidget_remote::{ClaimAnswer, Frame, Pointer, Selection, State};

use super::Out;
use super::ends::LogBackend;

const LOGGED_BYTES: usize = 4096;

/// One record of a session.
#[derive(Clone, Debug)]
pub enum GlueStep {
    Begin { clock: f64, scheme: Vec<u8>, width: f64, platform: bool, images: bool, ready: Vec<Vec<u8>> },
    Call { op: u8, clock: f64, text: Vec<u8>, width: f64 },
}

impl GlueStep {
    pub fn record(&self) -> Vec<u8> {
        let mut out = Out::default();
        match self {
            GlueStep::Begin { clock, scheme, width, platform, images, ready } => {
                out.f64(*clock).str(scheme).f64(*width).u8(u8::from(*platform) | (u8::from(*images) << 1));
                out.u32(ready.len() as u32);
                for m in ready {
                    out.str(m);
                }
                twidget_testkit::framed(b'B', &out.0)
            }
            GlueStep::Call { op, clock, text, width } => {
                out.f64(*clock);
                if matches!(op, b'S' | b'T' | b'P') {
                    out.str(text);
                } else if *op == b'W' {
                    out.f64(*width);
                }
                twidget_testkit::framed(*op, &out.0)
            }
        }
    }
}

#[derive(Default)]
struct Shared {
    log: Mutex<Out>,
    now: AtomicU64,
    ready: Mutex<Vec<Vec<u8>>>,
}

impl Shared {
    fn log(&self, f: impl FnOnce(&mut Out)) {
        f(&mut self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    }

    fn bytes(&self, tag: u8, bytes: &[u8]) {
        self.log(|o| {
            o.u8(tag);
        });
        self.logged(bytes);
    }

    /// `LogBytes`: short bytes as they are, longer ones by length and FNV-1a.
    fn logged(&self, bytes: &[u8]) {
        self.log(|o| {
            if bytes.len() > LOGGED_BYTES {
                let mut hash = 14695981039346656037u64;
                for &c in bytes {
                    hash = (hash ^ u64::from(c)).wrapping_mul(1099511628211);
                }
                o.u8(b'#').u64(bytes.len() as u64).u64(hash);
            } else {
                o.u8(b'=').str(bytes);
            }
        });
    }
}

struct LogHost(Arc<Shared>);

impl Host for LogHost {
    fn frame(&mut self, _frame: Frame) {}
    fn state(&mut self, _state: &State) {}
    fn messages(&mut self, _messages: Vec<Json>) {}
    fn pointer(&mut self, _pointer: &Pointer) {}
    fn selection(&mut self, _selection: &Selection) {}
    fn claim(&mut self, _answer: &ClaimAnswer) {}
    fn refused(&mut self, reason: &str) {
        self.0.log(|o| {
            o.u8(b'!').str(reason.as_bytes());
        });
    }
}

struct NoChannel;

impl Channel for NoChannel {
    fn send(&mut self, _message: Vec<u8>) {}
}

struct ScriptedPlatform {
    shared: Arc<Shared>,
    end: Mutex<PlatformEnd<LogBackend, LogHost, NoChannel>>,
    on_message: Mutex<Option<PlatformObserver>>,
}

impl Platform for ScriptedPlatform {
    fn send(&self, message: &[u8]) {
        self.shared.bytes(b'p', message);
    }

    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        self.shared.bytes(b'q', message);
        let mut reply = Vec::new();
        let answered = self.end.lock().unwrap().answer(message, &mut reply);
        self.shared.log(|o| {
            o.u8(b'a').flag(answered);
        });
        self.shared.logged(&reply);
        answered.then_some(reply)
    }

    fn set_on_message(&self, observer: PlatformObserver) {
        *self.on_message.lock().unwrap() = Some(observer);
    }
}

/// One run's driver, as the oracle's globals hold one; every session of a run shares litehtml's process state.
pub struct GlueSession {
    shared: Arc<Shared>,
    options: HostedOptions,
    platform: Option<Arc<ScriptedPlatform>>,
    held: Option<Arc<dyn Renderer>>,
}

impl Default for GlueSession {
    fn default() -> GlueSession {
        prepare_hosted_renderers();
        let shared = Arc::new(Shared::default());
        let now = Arc::clone(&shared);
        let options = HostedOptions {
            process: Arc::new(Process::default()),
            clock: Arc::new(move || f64::from_bits(now.now.load(Ordering::SeqCst))),
        };
        GlueSession { shared, options, platform: None, held: None }
    }
}

impl GlueSession {
    pub fn run(&mut self, step: &GlueStep) -> Vec<u8> {
        *self.shared.log.lock().unwrap() = Out::default();
        match step {
            GlueStep::Begin { clock, scheme, width, platform, images, ready } => {
                self.shared.now.store(clock.to_bits(), Ordering::SeqCst);
                self.begin(scheme, *width, *platform, *images, ready);
            }
            GlueStep::Call { op, clock, text, width } => {
                self.shared.now.store(clock.to_bits(), Ordering::SeqCst);
                self.call(*op, text, *width);
            }
        }
        std::mem::take(&mut self.shared.log.lock().unwrap().0)
    }

    fn begin(&mut self, scheme: &[u8], width: f64, platform: bool, images: bool, ready: &[Vec<u8>]) {
        *self.shared.ready.lock().unwrap() = ready.to_vec();
        self.held = None;
        self.platform = platform.then(|| {
            let mut backend = LogBackend::new();
            backend.images_load = images;
            Arc::new(ScriptedPlatform {
                shared: Arc::clone(&self.shared),
                end: Mutex::new(PlatformEnd::new(backend, LogHost(Arc::clone(&self.shared)), NoChannel, wire::embedding_caps())),
                on_message: Mutex::new(None),
            })
        });
        let given = self.platform.clone().map(|p| p as Arc<dyn Platform>);
        let made = make_hosted_renderer(scheme, width, given, &self.options);
        self.shared.log(|o| {
            o.u8(b'r').flag(made.is_some());
        });
        self.held = made.map(Arc::from);
        let Some(held) = &self.held else { return };
        let weak: Weak<dyn Renderer> = Arc::downgrade(held);
        let (messages, diags) = (Arc::clone(&self.shared), Arc::clone(&self.shared));
        held.connect(Link::new(
            move |json: &[u8]| {
                messages.bytes(b'm', json);
                if let Some(strong) = weak.upgrade()
                    && json == br#"{"type":"ready"}"#
                {
                    let queued = std::mem::take(&mut *messages.ready.lock().unwrap());
                    for message in queued {
                        strong.send(&message);
                    }
                }
            },
            move |kind: &[u8], detail: &[u8]| {
                diags.log(|o| {
                    o.u8(b'g').str(kind).str(detail);
                });
            },
        ));
    }

    fn call(&mut self, op: u8, text: &[u8], width: f64) {
        let held = self.held.clone();
        match op {
            b'P' => {
                let observer = self.platform.as_ref().and_then(|p| p.on_message.lock().unwrap().clone());
                if let Some(observer) = observer {
                    observer(text);
                }
            }
            b'E' => self.held = None,
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
        }
    }
}
