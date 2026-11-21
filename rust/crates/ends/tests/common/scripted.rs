//! A renderer that does what its script says (the stand-in for renderer.h's Renderer until stage 4, and the
//! oracle's fake renderer, `oracle/ends_seam/renderer.h`, does the same): every call is written to its log,
//! and a call with a reaction queued for it makes the reaction's Backend calls, sets what it sets and returns
//! its messages. A `receive` takes the reaction a message carries in "react", or else the next queued one.
//!
//! A script is `{"ctor": reaction, "drop": reaction, "on": {"method": [reaction, …]}}`; a reaction
//! `{"calls": [call, …], "out": [message, …], "set": {…}, "list": [command, …], "damage": [[x, y, w, h], …]}`,
//! a call one of `["font", families, size, weight, italic, features]`, `["cfont", families, size, weight,
//! italic]`, `["del", k]`, `["width", k, text]` (k: the k-th font made, or the handle k past them), `["widths",
//! [[k, text], …]]` (one `text_widths` call),
//! `["image", id, url]`, `["run", id, w, h, [op, …]]`, `["read", id, x, y, w, h]`, `["release", id]`. Without
//! a "drop" reaction the renderer releases its canvases, then its fonts, as the real one does.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use twidget_ends::{Backend, Renderer, TextInFont};
use twidget_json::{Json, quote};
use twidget_remote::GestureClaim;
use twidget_wire::*;

/// Lines shared between a renderer and whoever reads them after it is gone.
pub type Log = Rc<RefCell<Vec<String>>>;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn f32s(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

pub fn f64s(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

pub fn quoted(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&quote(bytes)).into_owned()
}

pub fn metrics_line(m: &FontMetrics) -> String {
    [m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent]
        .iter()
        .map(|v| f32s(*v))
        .collect::<Vec<_>>()
        .join(",")
}

fn number_f32(j: &Json) -> f32 {
    j.as_number(0.0) as f32
}

fn rect_of(j: &Json) -> Rect {
    Rect::new(number_f32(j.at(0)), number_f32(j.at(1)), number_f32(j.at(2)), number_f32(j.at(3)))
}

fn argb(j: &Json) -> Color {
    let v = j.as_number(0.0) as u32;
    Color::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8)
}

fn rect_geometry(r: &Rect) -> Geometry {
    Geometry {
        ops: vec![PathOp::Move, PathOp::Line, PathOp::Line, PathOp::Close],
        points: vec![r.x, r.y, r.x + r.w, r.y, r.x + r.w, r.y + r.h],
    }
}

/// Payloads by index, so that a script can share one between commands or ops.
#[derive(Default)]
struct Pools {
    paths: HashMap<i32, Arc<Path>>,
    filters: HashMap<i32, Arc<Filters>>,
    clips: HashMap<i32, Arc<Clips>>,
    pixels: HashMap<i32, Arc<Pixels>>,
}

fn build_list(spec: &Json) -> DisplayList {
    let mut pools = Pools::default();
    let mut list = DisplayList::new();
    for item in spec.arr() {
        let mut c = Command {
            kind: Cmd::from_u8((item.get("k").as_int(0).rem_euclid(18)) as u8).expect("a kind"),
            rect: rect_of(item.get("r")),
            color: argb(item.get("c")),
            text: item.get("t").as_str().to_vec(),
            font: item.get("f").as_int(0),
            image: item.get("i").as_int(0),
            canvas: item.get("cv").as_int(0),
            ..Command::default()
        };
        let p = item.get("p").as_int(-1);
        if p >= 0 {
            let path = pools.paths.entry(p).or_insert_with(|| {
                Arc::new(Path {
                    ops: vec![PathOp::Move, PathOp::Line, PathOp::Close],
                    points: vec![p as f32, 0.0, p as f32 + 10.0, 5.0],
                    fill: true,
                    fill_color: c.color,
                    ..Path::default()
                })
            });
            c.path = Some(Arc::clone(path));
        }
        let f = item.get("fl").as_int(-1);
        if f >= 0 {
            let filters = pools
                .filters
                .entry(f)
                .or_insert_with(|| Arc::new(vec![Filter { kind: FilterKind::Blur, amount: f as f32, ..Filter::default() }]));
            c.filter = Some(Arc::clone(filters));
        }
        list.push(c);
    }
    list
}

fn build_ops(spec: &Json) -> Vec<CanvasOp> {
    let mut pools = Pools::default();
    let mut ops = Vec::new();
    for item in spec.arr() {
        let kind = OpKind::from_u8((item.get("k").as_int(1).rem_euclid(7)) as u8).expect("a kind");
        let r = rect_of(item.get("r"));
        let mut op = CanvasOp { kind, ..CanvasOp::default() };
        if matches!(kind, OpKind::Fill | OpKind::Stroke | OpKind::Clear) {
            op.path = rect_geometry(&r);
        }
        op.paint.color = argb(item.get("c"));
        op.text = item.get("t").as_str().to_vec();
        op.font = item.get("f").as_int(0);
        op.x = r.x;
        op.y = r.y;
        let cl = item.get("cl").as_int(-1);
        if cl >= 0 {
            let clips = pools.clips.entry(cl).or_insert_with(|| {
                Arc::new(vec![CanvasClip { path: rect_geometry(&Rect::new(cl as f32, cl as f32, 10.0, 10.0)), evenodd: cl % 2 == 1 }])
            });
            op.clips = Some(Arc::clone(clips));
        }
        let px = item.get("px");
        if px.is_array() {
            let (w, h, n) = (px.at(0).as_int(0), px.at(1).as_int(0), px.at(2).as_int(0));
            let rgba = pools.pixels.entry(n).or_insert_with(|| {
                Arc::new((0..(w.max(0) * h.max(0) * 4) as usize).map(|i| (n as usize).wrapping_mul(31).wrapping_add(i) as u8).collect())
            });
            op.rgba = Some(Arc::clone(rgba));
            op.w = w;
            op.h = h;
            op.dx = r.x;
            op.dy = r.y;
        }
        ops.push(op);
    }
    ops
}

/// The renderer: a Backend it owns, the queued reactions by method, and the state it reports.
pub struct ScriptedRenderer<B: Backend> {
    backend: B,
    queues: HashMap<Vec<u8>, VecDeque<Json>>,
    on_drop: Option<Json>,
    width: f64,
    height: i32,
    dirty: bool,
    wants_tick: bool,
    next_tick_in: f64,
    cursor: Vec<u8>,
    selection: Vec<u8>,
    claim: GestureClaim,
    fonts: Vec<i32>,
    canvases: Vec<i32>,
    pub log: Log,
}

impl<B: Backend> std::fmt::Debug for ScriptedRenderer<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedRenderer").field("fonts", &self.fonts).field("canvases", &self.canvases).finish_non_exhaustive()
    }
}

impl<B: Backend> ScriptedRenderer<B> {
    pub fn new(width: f64, dpr: f64, dark: bool, backend: B, script: &Json, log: Log) -> ScriptedRenderer<B> {
        let mut queues = HashMap::new();
        for (method, list) in script.get("on").obj() {
            queues.insert(method.clone(), list.arr().iter().cloned().collect::<VecDeque<Json>>());
        }
        let on_drop = script.find("drop").cloned();
        let mut r = ScriptedRenderer {
            backend,
            queues,
            on_drop,
            width,
            height: 0,
            dirty: false,
            wants_tick: false,
            next_tick_in: f64::INFINITY,
            cursor: b"auto".to_vec(),
            selection: Vec::new(),
            claim: GestureClaim::default(),
            fonts: Vec::new(),
            canvases: Vec::new(),
            log,
        };
        r.line(format!("new {} {} {}", f64s(width), f64s(dpr), u8::from(dark)));
        if let Some(ctor) = script.find("ctor") {
            r.react(&ctor.clone());
        }
        r
    }

    /// Queues `reaction` for the next call of `method`.
    pub fn push(&mut self, method: &str, reaction: Json) {
        self.queues.entry(method.as_bytes().to_vec()).or_default().push_back(reaction);
    }

    pub fn backend(&mut self) -> &mut B {
        &mut self.backend
    }

    fn line(&self, text: String) {
        self.log.borrow_mut().push(text);
    }

    fn pop(&mut self, method: &str) -> Option<Json> {
        self.queues.get_mut(method.as_bytes()).and_then(VecDeque::pop_front)
    }

    /// The queued reaction of `method`, made; its messages.
    fn queued(&mut self, method: &str) -> Vec<Json> {
        match self.pop(method) {
            Some(r) => self.react(&r),
            None => Vec::new(),
        }
    }

    fn font_at(&self, k: i32) -> i32 {
        usize::try_from(k).ok().and_then(|i| self.fonts.get(i)).copied().unwrap_or(k)
    }

    fn react(&mut self, r: &Json) -> Vec<Json> {
        for call in r.get("calls").arr() {
            self.call(call);
        }
        let set = r.get("set");
        if let Some(v) = set.find("width") {
            self.width = v.as_number(0.0);
        }
        if let Some(v) = set.find("height") {
            self.height = v.as_int(0);
        }
        if let Some(v) = set.find("dirty") {
            self.dirty = v.as_bool(false);
        }
        if let Some(v) = set.find("tick") {
            self.wants_tick = v.as_bool(false);
        }
        if let Some(v) = set.find("next") {
            self.next_tick_in = if v.is_number() { v.as_number(0.0) } else { f64::INFINITY };
        }
        if let Some(v) = set.find("cursor") {
            self.cursor = v.as_str().to_vec();
        }
        if let Some(v) = set.find("selection") {
            self.selection = v.as_str().to_vec();
        }
        if let Some(v) = set.find("claim") {
            self.claim = GestureClaim { x: v.at(0).as_bool(false), y: v.at(1).as_bool(false), hold: v.at(2).as_bool(false) };
        }
        r.get("out").arr().to_vec()
    }

    fn call(&mut self, call: &Json) {
        let name = call.at(0).as_str().to_vec();
        match &name[..] {
            b"font" | b"cfont" => {
                let families = call.at(1).as_str().to_vec();
                let size = number_f32(call.at(2));
                let weight = call.at(3).as_int(0);
                let italic = call.at(4).as_bool(false);
                let mut m = FontMetrics::default();
                let font = if name == b"font" {
                    self.backend.create_font(&families, size, weight, italic, call.at(5).as_str(), &mut m)
                } else {
                    self.backend.create_canvas_font(&families, size, weight, italic, &mut m)
                };
                self.fonts.push(font);
                self.line(format!("  {} {} -> {font} {}", String::from_utf8_lossy(&name), quoted(&families), metrics_line(&m)));
            }
            b"del" => {
                let k = call.at(1).as_int(0);
                let font = self.font_at(k);
                if let Some(slot) = usize::try_from(k).ok().and_then(|i| self.fonts.get_mut(i)) {
                    *slot = 0;
                }
                self.backend.delete_font(font);
                self.line(format!("  del {font}"));
            }
            b"width" => {
                let font = self.font_at(call.at(1).as_int(0));
                let w = self.backend.text_width(font, call.at(2).as_str());
                self.line(format!("  width {font} {} -> {}", quoted(call.at(2).as_str()), f32s(w)));
            }
            b"widths" => {
                let texts: Vec<TextInFont> = call
                    .at(1)
                    .arr()
                    .iter()
                    .map(|t| TextInFont { font: self.font_at(t.at(0).as_int(0)), text: t.at(1).as_str().to_vec() })
                    .collect();
                let widths = self.backend.text_widths(&texts);
                let all: String = widths.iter().map(|w| format!(" {}", f32s(*w))).collect();
                self.line(format!("  widths {} ->{all}", texts.len()));
            }
            b"image" => {
                let (mut w, mut h) = (0.0f32, 0.0f32);
                let id = call.at(1).as_int(0);
                let ok = self.backend.load_image(id, call.at(2).as_str(), &mut w, &mut h);
                self.line(format!("  image {id} -> {} {} {}", u8::from(ok), f32s(w), f32s(h)));
            }
            b"run" => {
                let id = call.at(1).as_int(0);
                if !self.canvases.contains(&id) {
                    self.canvases.push(id);
                }
                let ops = build_ops(call.at(4));
                self.backend.canvas_run(id, call.at(2).as_int(0), call.at(3).as_int(0), &ops);
                self.line(format!("  run {id} {}", ops.len()));
            }
            b"read" => {
                let a: Vec<i32> = (1..6).map(|i| call.at(i).as_int(0)).collect();
                let mut rgba = Vec::new();
                let ok = self.backend.canvas_read(a[0], a[1], a[2], a[3], a[4], &mut rgba);
                // As the renderer takes them: pixels of the region's size, or none.
                let good = ok && rgba.len() as i64 == i64::from(a[3].max(0)) * i64::from(a[4].max(0)) * 4;
                self.line(format!("  read {} -> {} {}", a[0], u8::from(good), if good { hex(&rgba) } else { String::new() }));
            }
            b"release" => {
                let id = call.at(1).as_int(0);
                self.canvases.retain(|c| *c != id);
                self.backend.canvas_release(id);
                self.line(format!("  release {id}"));
            }
            _ => self.line(format!("  unknown {}", String::from_utf8_lossy(&call.dump()))),
        }
    }
}

impl<B: Backend> Drop for ScriptedRenderer<B> {
    fn drop(&mut self) {
        self.line("drop".to_string());
        if let Some(r) = self.on_drop.take() {
            self.react(&r);
            return;
        }
        for id in std::mem::take(&mut self.canvases) {
            self.backend.canvas_release(id);
            self.line(format!("  release {id}"));
        }
        for font in std::mem::take(&mut self.fonts) {
            if font != 0 {
                self.backend.delete_font(font);
                self.line(format!("  del {font}"));
            }
        }
    }
}

fn dumps(messages: &[Json]) -> String {
    messages.iter().map(|m| String::from_utf8_lossy(&m.dump()).into_owned()).collect::<Vec<_>>().join(" ")
}

impl<B: Backend> Renderer for ScriptedRenderer<B> {
    fn receive(&mut self, message: &Json) -> Vec<Json> {
        self.line(format!("receive {}", String::from_utf8_lossy(&message.dump())));
        let out = match message.find("react") {
            Some(r) if r.is_object() => self.react(&r.clone()),
            _ => self.queued("receive"),
        };
        self.line(format!("  -> {}", dumps(&out)));
        out
    }

    fn set_width(&mut self, width: f64, out: &mut Vec<Json>) {
        self.line(format!("set_width {}", f64s(width)));
        self.width = width;
        out.extend(self.queued("set_width"));
    }

    fn set_max_height(&mut self, height: f64, out: &mut Vec<Json>) {
        self.line(format!("set_max_height {}", f64s(height)));
        out.extend(self.queued("set_max_height"));
    }

    fn set_theme(&mut self, dark: bool, out: &mut Vec<Json>) {
        self.line(format!("set_theme {}", u8::from(dark)));
        out.extend(self.queued("set_theme"));
    }

    fn touch(&mut self, action: i32, x: f64, y: f64, pointer: i32, primary: bool, mouse: bool, modifiers: i32, button: i32) -> Vec<Json> {
        self.line(format!(
            "touch {action} {} {} {pointer} {} {} {modifiers} {button}",
            f64s(x),
            f64s(y),
            u8::from(primary),
            u8::from(mouse)
        ));
        self.queued("touch")
    }

    fn hover(&mut self, action: i32, x: f64, y: f64, modifiers: i32) -> Vec<Json> {
        self.line(format!("hover {action} {} {} {modifiers}", f64s(x), f64s(y)));
        self.queued("hover")
    }

    fn cursor(&mut self) -> Vec<u8> {
        self.line("cursor".to_string());
        self.queued("cursor");
        self.cursor.clone()
    }

    fn claim_at(&mut self, x: f64, y: f64) -> GestureClaim {
        self.line(format!("claim_at {} {}", f64s(x), f64s(y)));
        self.queued("claim_at");
        self.claim
    }

    fn clear_selection(&mut self) -> bool {
        self.line("clear_selection".to_string());
        let had = !self.selection.is_empty();
        self.selection.clear();
        self.queued("clear_selection");
        had
    }

    fn may_have_selection(&mut self) -> bool {
        self.line("may_have_selection".to_string());
        self.queued("may_have_selection");
        !self.selection.is_empty()
    }

    fn selected_text(&mut self) -> Vec<u8> {
        self.line("selected_text".to_string());
        self.queued("selected_text");
        self.selection.clone()
    }

    fn tick(&mut self) -> Vec<Json> {
        self.line("tick".to_string());
        self.queued("tick")
    }

    fn dirty(&mut self) -> bool {
        self.line("dirty".to_string());
        self.queued("dirty");
        self.dirty
    }

    fn paint(&mut self) -> DisplayList {
        self.line("paint".to_string());
        self.dirty = false;
        match self.pop("paint") {
            Some(r) => {
                self.react(&r);
                build_list(r.get("list"))
            }
            None => DisplayList::new(),
        }
    }

    fn damage(&mut self, out: &mut Vec<Rect>) -> bool {
        self.line("damage".to_string());
        let Some(r) = self.pop("damage") else { return false };
        self.react(&r);
        let rects = r.get("damage");
        if !rects.is_array() {
            return false;
        }
        out.extend(rects.arr().iter().map(rect_of));
        true
    }

    fn width(&mut self) -> f64 {
        self.line("width".to_string());
        self.queued("width");
        self.width
    }

    fn viewport_height(&mut self) -> i32 {
        self.line("viewport_height".to_string());
        self.queued("viewport_height");
        self.height
    }

    fn wants_tick(&mut self) -> bool {
        self.line("wants_tick".to_string());
        self.queued("wants_tick");
        self.wants_tick
    }

    fn next_tick_in(&mut self) -> f64 {
        self.line("next_tick_in".to_string());
        self.queued("next_tick_in");
        self.next_tick_in
    }
}
