//! The hosted glue and its cursor on written sessions, as tgw_hosted_glue.cpp and tgw_cursor.cpp behave: `ready` once
//! both the load and the Open came, the load's two halves, a stop reported once, controls it does not take, and the
//! cursor at the pointer following its targets.

mod common;

use std::sync::{Arc, Mutex};

use common::ends::LogBackend;
use twidget_document::Process;
use twidget_ends::{Channel, Host, PlatformEnd};
use twidget_helper::cursor::{CursorSource, CursorStyles};
use twidget_helper::{HostedOptions, Link, Platform, PlatformObserver, make_hosted_renderer, wire};
use twidget_json::Json;
use twidget_remote::{ClaimAnswer, Frame, FrameRequest, Message, Paint, Pointer, Selection, State};

struct NoHost;

impl Host for NoHost {
    fn frame(&mut self, _frame: Frame) {}
    fn state(&mut self, _state: &State) {}
    fn messages(&mut self, _messages: Vec<Json>) {}
    fn pointer(&mut self, _pointer: &Pointer) {}
    fn selection(&mut self, _selection: &Selection) {}
    fn claim(&mut self, _answer: &ClaimAnswer) {}
    fn refused(&mut self, _reason: &str) {}
}

struct NoChannel;

impl Channel for NoChannel {
    fn send(&mut self, _message: Vec<u8>) {}
}

struct Telegram {
    sent: Mutex<Vec<Vec<u8>>>,
    end: Mutex<PlatformEnd<LogBackend, NoHost, NoChannel>>,
    observer: Mutex<Option<PlatformObserver>>,
}

impl Platform for Telegram {
    fn send(&self, message: &[u8]) {
        self.sent.lock().unwrap().push(message.to_vec());
    }
    fn request(&self, message: &[u8]) -> Option<Vec<u8>> {
        let mut reply = Vec::new();
        self.end.lock().unwrap().answer(message, &mut reply).then_some(reply)
    }
    fn set_on_message(&self, observer: PlatformObserver) {
        *self.observer.lock().unwrap() = Some(observer);
    }
}

impl Telegram {
    fn deliver(&self, message: &[u8]) {
        let observer = self.observer.lock().unwrap().clone().expect("the glue's handler");
        observer(message);
    }

    fn controls(&self) -> Vec<wire::Kind> {
        self.sent.lock().unwrap().iter().filter_map(|m| wire::kind_of(m)).collect()
    }
}

fn options() -> HostedOptions {
    HostedOptions { process: Arc::new(Process::default()), clock: Arc::new(|| 1000.0) }
}

#[test]
fn a_widget_loads_and_stops_once() {
    let telegram = Arc::new(Telegram {
        sent: Mutex::new(Vec::new()),
        end: Mutex::new(PlatformEnd::new(LogBackend::new(), NoHost, NoChannel, wire::embedding_caps())),
        observer: Mutex::new(None),
    });
    let renderer = make_hosted_renderer(b"light", 300.0, Some(Arc::clone(&telegram) as Arc<dyn Platform>), &options()).expect("a glue");
    let host = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let heard = Arc::clone(&host);
    renderer.connect(Link::new(move |json: &[u8]| heard.lock().unwrap().push(json.to_vec()), |_: &[u8], _: &[u8]| {}));
    renderer.load();
    assert!(host.lock().unwrap().is_empty(), "no ready before the Open");
    telegram.deliver(&wire::encode_open(2.0));
    assert_eq!(host.lock().unwrap().first().map(Vec::as_slice), Some(&br#"{"type":"ready"}"#[..]));
    renderer.send(br#"{"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},{"op":"text","id":5,"data":"hi"},{"op":"ins","parent":4,"id":5,"before":null}]}"#);
    assert_eq!(telegram.controls(), vec![wire::Kind::Content], "content first");
    renderer.send(br#"{"type":"query","qid":-1,"query":{"q":"viewport"}}"#);
    assert_eq!(telegram.controls(), vec![wire::Kind::Content, wire::Kind::Loaded], "then loaded, at the post-load query");
    let mut frame = Vec::new();
    assert!(FrameRequest { seq: 1, tick: true, paint: Paint::IfDirty }.encode(&wire::embedding_caps(), &mut frame));
    telegram.deliver(&frame);
    telegram.deliver(&wire::encode_mirror());
    telegram.deliver(&wire::encode_open(2.0));
    let stops: Vec<Vec<u8>> = telegram.sent.lock().unwrap().iter().filter_map(|m| wire::decode_stopped(m).map(<[u8]>::to_vec)).collect();
    assert_eq!(stops, vec![b"a control the widget helper does not take".to_vec()], "one stop, the first reason");
}

struct Source(Vec<u8>, Vec<u8>);

impl CursorSource for Source {
    fn builds(&self) -> i32 {
        1
    }
    fn document(&mut self) -> (&[u8], &[u8], f64, i32) {
        (&self.0, &self.1, 300.0, 200)
    }
}

#[test]
fn the_cursor_follows_its_targets() {
    let html =
        br#"<html><head></head><body><div class="b" data-twid="5"><span data-twid="6">x</span></div><p data-twid="7">y</p></body></html>"#;
    let master = [twidget_renderer::master_css_for(&[]), b".b:hover span{cursor:pointer} p:active{cursor:pointer}".to_vec()].concat();
    let mut source = Source(html.to_vec(), master);
    let mut styles = CursorStyles::new(1.0, Arc::new(Process::default()));
    assert!(!styles.pointer(&mut source, 6, 0, 0), "nothing hovered");
    assert!(styles.pointer(&mut source, 6, 6, 0), "its ancestor .b hovers with it");
    assert!(!styles.pointer(&mut source, 7, 7, 0), "not active");
    assert!(styles.pointer(&mut source, 7, 7, 7), "active");
    assert!(!styles.pointer(&mut source, 6, 7, 7), "the hover moved away");
    assert!(!styles.pointer(&mut source, 9, 7, 7), "no element has id 9");
}
