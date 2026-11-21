//! core_tests.cpp's check of the names each renderer is charged, which needs the renderer's real container: two
//! renderers on `twidget_document` engines sharing one process's name table, the second on a thread of its own, are
//! each charged the names they add themselves, also while one waits in a text measurement as the other adds a name.
//!
//! The C++ moves the second renderer to a new thread for one batch; a Rust renderer stays on the thread it was made on,
//! so the second lives on its own thread throughout, and the first's measurement runs its batch there and waits.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use twidget_document::{DocumentEngine, Process};
use twidget_ends::Backend;
use twidget_json::Json;
use twidget_renderer::Renderer;
use twidget_wire::FontMetrics;

/// A Backend answering every font with the next id and every text with 8 px a byte.
#[derive(Clone, Default)]
struct Plain {
    next: Rc<Cell<i32>>,
}

impl Backend for Plain {
    fn create_font(&mut self, _families: &[u8], size: f32, _weight: i32, _italic: bool, _features: &[u8], out: &mut FontMetrics) -> i32 {
        out.ascent = size * 0.8;
        out.descent = size * 0.2;
        out.height = size;
        out.x_height = size * 0.5;
        self.next.set(self.next.get() + 1);
        self.next.get()
    }
    fn create_canvas_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.create_font(families, size, weight, italic, b"", out)
    }
    fn delete_font(&mut self, _font: i32) {}
    fn text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }
    fn load_image(&mut self, _id: i32, _url: &[u8], _w: &mut f32, _h: &mut f32) -> bool {
        false
    }
}

/// What the second renderer's thread is told to do; it answers each with its charges, names and bytes.
enum Command {
    Receive(String),
    Tick,
    Charges,
    Quit,
}

/// The way to the second renderer, which the first's Backend takes once, from inside its next text measurement.
struct Gate {
    to: Sender<Command>,
    from: Receiver<(u64, u64)>,
    next: RefCell<Option<String>>,
    ran: Cell<bool>,
}

impl Gate {
    fn tell(&self, command: Command) -> (u64, u64) {
        self.to.send(command).expect("the second renderer's thread lives");
        self.from.recv_timeout(Duration::from_secs(120)).expect("the second renderer's thread answers")
    }
}

/// A Backend that, given a batch, runs it on the second renderer and waits, from inside the next text measurement
/// (core_tests.cpp's `GateBackend`).
#[derive(Clone)]
struct GateBackend {
    plain: Plain,
    gate: Rc<Gate>,
}

impl Backend for GateBackend {
    fn create_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        self.plain.create_font(families, size, weight, italic, features, out)
    }
    fn create_canvas_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.plain.create_canvas_font(families, size, weight, italic, out)
    }
    fn delete_font(&mut self, font: i32) {
        self.plain.delete_font(font)
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        let batch = self.gate.next.borrow_mut().take();
        if let Some(batch) = batch {
            self.gate.tell(Command::Receive(batch));
            self.gate.tell(Command::Tick);
            self.gate.ran.set(true);
        }
        self.plain.text_width(font, text)
    }
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        self.plain.load_image(id, url, w, h)
    }
}

fn renderer<B: Backend + Clone>(process: &Arc<Process>, backend: B) -> Renderer<DocumentEngine<B>, B> {
    Renderer::new(300.0, 1.0, false, DocumentEngine::new(Arc::clone(process), backend.clone()), backend, None)
}

fn parse(s: &str) -> Json {
    Json::parse(s).unwrap_or_else(|| panic!("bad json: {s}"))
}

const PAGE: &str = r#"{"type":"ops","ops":[
    {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
    {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
    {"op":"el","id":10,"tag":"div","attrs":[["class","warm"]]},{"op":"ins","parent":4,"id":10,"before":null},
    {"op":"text","id":11,"data":"warm words"},{"op":"ins","parent":10,"id":11,"before":null}]}"#;

fn text_of(text: &str) -> String {
    format!(r#"{{"type":"ops","ops":[{{"op":"data","id":11,"data":"{text}"}}]}}"#)
}

fn class_of(name: &str) -> String {
    format!(r#"{{"type":"ops","ops":[{{"op":"attr","id":10,"name":"class","value":"{name}"}}]}}"#)
}

fn shown(c: &[u64; 4]) -> String {
    format!("A {} names, {} bytes; B {} names, {} bytes", c[0], c[1], c[2], c[3])
}

/// Each renderer is charged the names it adds itself, also while another one adds names on another thread.
#[test]
fn names_by_thread() {
    let process = Arc::new(Process::default());
    let (to, commands) = channel::<Command>();
    let (answers, from) = channel::<(u64, u64)>();
    let theirs = Arc::clone(&process);
    let second = thread::spawn(move || {
        let mut b = renderer(&theirs, Plain::default());
        for command in commands {
            match command {
                Command::Receive(json) => {
                    b.receive(&parse(&json));
                }
                Command::Tick => {
                    b.tick();
                }
                Command::Charges => {}
                Command::Quit => break,
            }
            if answers.send((b.stats().names, b.stats().name_bytes)).is_err() {
                break;
            }
        }
    });
    let gate = Rc::new(Gate { to, from, next: RefCell::new(None), ran: Cell::new(false) });
    let mut a = renderer(&process, GateBackend { plain: Plain::default(), gate: Rc::clone(&gate) });
    a.receive(&parse(PAGE));
    a.tick();
    gate.tell(Command::Receive(PAGE.to_string()));
    gate.tell(Command::Tick);
    // A class changed once before: the C++ checks this after others that add the names a class change reads ("class")
    // to its process's one table, which this process makes new.
    gate.tell(Command::Receive(class_of("tw-warm-class")));
    gate.tell(Command::Tick);
    let charges = |a: &Renderer<DocumentEngine<GateBackend>, GateBackend>| {
        let (names, bytes) = gate.tell(Command::Charges);
        [a.stats().names, a.stats().name_bytes, names, bytes]
    };
    let since = |now: [u64; 4], start: [u64; 4]| [now[0] - start[0], now[1] - start[1], now[2] - start[2], now[3] - start[3]];

    // A lays out a text it has not measured on the page it has, B gives its element a class no one has used.
    let (first, second_class) = ("tw-fresh-class-1", "tw-fresh-class-2");
    let start = charges(&a);
    a.receive(&parse(&text_of("serial words")));
    a.tick();
    gate.tell(Command::Receive(class_of(first)));
    gate.tell(Command::Tick);
    let serial = since(charges(&a), start);
    assert_eq!(
        serial,
        [0, 0, 1, first.len() as u64],
        "of two renderers taking turns only the one that adds a class is charged its name and bytes ({})",
        shown(&serial)
    );
    let start = charges(&a);
    *gate.next.borrow_mut() = Some(class_of(second_class));
    a.receive(&parse(&text_of("overlapping words")));
    a.tick();
    let overlapped = since(charges(&a), start);
    assert!(
        gate.ran.get() && second_class.len() == first.len() && overlapped == serial,
        "a renderer waiting in a text measurement while another adds a class on a second thread is not charged for it ({})",
        shown(&overlapped)
    );
    gate.to.send(Command::Quit).expect("the second renderer's thread lives");
    second.join().expect("the second renderer's thread ends");
}
