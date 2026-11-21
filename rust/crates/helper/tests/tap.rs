//! The Debug tap as test_widget_helper_tap.h describes it, on a written session: nothing before the arming, the
//! arming controls swallowed, the built and theme records, `ops` never mirrored, renderer messages mirrored before the
//! host gets them, and one work record after each outermost operation once `MirrorWork` came.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use twidget_helper::wire;
use twidget_helper::{HelperWorkKind, Link, Platform, PlatformObserver, Renderer, decode_helper_work, tap_helper_renderer};

#[derive(Default)]
struct Recorder {
    sent: Mutex<Vec<Vec<u8>>>,
    observer: Mutex<Option<PlatformObserver>>,
}

impl Platform for Recorder {
    fn send(&self, message: &[u8]) {
        self.sent.lock().unwrap().push(message.to_vec());
    }
    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        Some(message.to_vec())
    }
    fn set_on_message(&self, observer: PlatformObserver) {
        *self.observer.lock().unwrap() = Some(observer);
    }
}

/// Keeps its platform, as the glue does: the tap's chained handler holds it weakly.
struct Inner {
    seen: Arc<Mutex<Vec<String>>>,
    link: Mutex<Option<Link>>,
    _platform: Arc<dyn Platform>,
}

impl Renderer for Inner {
    fn connect(&self, link: Link) {
        *self.link.lock().unwrap() = Some(link);
    }
    fn load(&self) {
        let link = self.link.lock().unwrap().clone();
        (link.expect("connected").message)(br#"{"type":"ready"}"#);
    }
    fn send(&self, json: &[u8]) {
        self.seen.lock().unwrap().push(String::from_utf8_lossy(json).into_owned());
    }
    fn set_theme(&self, theme: &[u8]) {
        self.seen.lock().unwrap().push(format!("theme {}", String::from_utf8_lossy(theme)));
    }
    fn set_width(&self, _width: f64) {}
    fn destroy(&self) {}
}

fn records(platform: &Recorder) -> Vec<(u8, Vec<u8>)> {
    let sent = std::mem::take(&mut *platform.sent.lock().unwrap());
    sent.iter().map(|m| wire::decode_mirrored(m).map(|r| (r.way, r.text.to_vec())).expect("only mirrored records")).collect()
}

#[test]
fn a_written_session() {
    let platform = Arc::new(Recorder::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let clock = Arc::new(AtomicI64::new(100));
    let ticks = Arc::clone(&clock);
    let inner_seen = Arc::clone(&seen);
    let build = move |tapped: Option<Arc<dyn Platform>>| -> Option<Box<dyn Renderer>> {
        let tapped = tapped.expect("a platform");
        let seen = Arc::clone(&inner_seen);
        tapped.set_on_message(Arc::new(move |m: &[u8]| seen.lock().unwrap().push(format!("platform {}", m.len()))));
        Some(Box::new(Inner { seen: Arc::clone(&inner_seen), link: Mutex::new(None), _platform: tapped }))
    };
    let renderer = tap_helper_renderer(
        Some(Arc::clone(&platform) as Arc<dyn Platform>),
        b"light",
        Arc::new(move || ticks.fetch_add(10, Ordering::SeqCst)),
        &build,
    )
    .expect("a renderer");
    let host = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&host);
    renderer.connect(Link::new(move |json: &[u8]| heard.lock().unwrap().push(json.to_vec()), |_: &[u8], _: &[u8]| {}));
    let deliver = |message: &[u8]| (platform.observer.lock().unwrap().clone().expect("chained"))(message);

    renderer.set_theme(b"dark");
    renderer.send(br#"{"type":"query"}"#);
    assert!(records(&platform).is_empty(), "nothing before the arming");
    deliver(&wire::encode_mirror());
    assert_eq!(records(&platform), vec![(b'b', b"light".to_vec()), (b't', b"dark".to_vec())], "the built scheme, then the changed one");
    renderer.send(br#"{"type":"ops","ops":[]}"#);
    renderer.send(br#"{"type":"theme"}"#);
    assert_eq!(records(&platform), vec![(b'h', br#"{"type":"theme"}"#.to_vec())], "ops are the host's to report");
    renderer.load();
    assert_eq!(records(&platform), vec![(b'r', br#"{"type":"ready"}"#.to_vec())]);
    assert_eq!(host.lock().unwrap().len(), 1, "then the host");
    deliver(&wire::encode_mirror_work());
    deliver(b"\x01\x01");
    renderer.send(br#"{"type":"query"}"#);
    let work = records(&platform);
    assert_eq!(work.len(), 3, "a record for the platform message, then the host's message and its record");
    let first = decode_helper_work(&work[0].1).expect("a work record");
    assert_eq!((work[0].0, first.kind, first.finished - first.started), (b'w', HelperWorkKind::Platform, 10));
    let last = decode_helper_work(&work[2].1).expect("a work record");
    assert_eq!(last.kind, HelperWorkKind::Message);
    let seen = seen.lock().unwrap();
    assert!(seen.contains(&"platform 2".to_string()) && !seen.iter().any(|s| s == "platform 0"), "controls swallowed, the rest forwarded");
}
