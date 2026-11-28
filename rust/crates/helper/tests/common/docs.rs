//! Widget documents for the cursor and the glue: widget-sandbox's corpus widgets and the input stage's generated
//! documents as validator-like ops (twidget_events' test inputs), their HTML as the renderer's shadow tree writes it,
//! and seeded patches of them (attributes, classes, inline styles, texts, removals and insertions).

use twidget_json::Json;
use twidget_shadow::{Chrome, Document, DocumentHost, StyleOverrides};
use twidget_testkit::Lcg;
use twidget_wire::{CanvasOp, FontMetrics};

pub use super::events::inputs::{corpus_files, document, generated_html, html_to_ops, pick, widget_sandbox};

struct NoHost;

impl DocumentHost for NoHost {
    fn canvas_run(&mut self, _canvas: i32, _width: i32, _height: i32, _ops: &[CanvasOp]) {}
    fn canvas_release(&mut self, _canvas: i32) {}
    fn canvas_font(&mut self, _css: &[u8], _metrics: &mut FontMetrics) -> i32 {
        0
    }
    fn canvas_text_width(&mut self, _font: i32, _text: &[u8]) -> f32 {
        0.0
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
}

/// A widget's tree as the renderer keeps it, and the ids it holds.
pub struct Shadow {
    doc: Document,
    pub ids: Vec<i64>,
    pub next: i64,
}

impl Shadow {
    /// The tree of a batch of ops (a JSON array), its ids up to `last`.
    pub fn new(batch: &str, last: i64) -> Shadow {
        let mut s = Shadow { doc: Document::new(false), ids: Vec::new(), next: last + 1 };
        s.apply(batch);
        s
    }

    pub fn apply(&mut self, batch: &str) {
        let ops = Json::parse(batch.as_bytes()).expect("a batch");
        let mut out = Vec::new();
        self.doc.receive_ops(ops.arr(), &mut NoHost, &mut out);
        self.ids = (2..self.next).filter(|&id| self.doc.node(id).is_some()).collect();
    }

    pub fn html(&self) -> Vec<u8> {
        let overrides = StyleOverrides::new();
        self.doc.serialize(&Chrome { overrides: &overrides, editing: 0 })
    }

    /// A batch that changes the tree a little: classes, attributes, inline styles, texts, a removal, an insertion.
    pub fn patch(&mut self, r: &mut Lcg) -> String {
        let mut ops = Vec::new();
        for _ in 0..1 + r.below(4) {
            let id = if self.ids.is_empty() { 4 } else { self.ids[r.below(self.ids.len())] };
            match r.below(8) {
                0 | 1 => ops.push(format!(r#"{{"op":"attr","id":{id},"name":"class","value":"c{} c{}"}}"#, r.below(8), r.below(8))),
                2 => ops
                    .push(format!(r#"{{"op":"style","id":{id},"name":"cursor","value":"{}"}}"#, pick(r, &["pointer", "text", "auto", ""]))),
                3 => ops.push(format!(r#"{{"op":"data","id":{id},"data":"{}"}}"#, pick(r, &["changed", "x", ""]))),
                4 if id > 4 => ops.push(format!(r#"{{"op":"rm","id":{id}}}"#)),
                5 => {
                    let new = self.next;
                    self.next += 1;
                    let tag = pick(r, &["div", "span", "a", "button"]);
                    ops.push(format!(r#"{{"op":"el","id":{new},"tag":"{tag}","attrs":[["class","c{}"]]}}"#, r.below(8)));
                    let parent = if r.chance(3) { 4 } else { id };
                    ops.push(format!(r#"{{"op":"ins","parent":{parent},"id":{new},"before":null}}"#));
                }
                6 => ops.push(format!(r##"{{"op":"attr","id":{id},"name":"href","value":"#"}}"##)),
                _ => ops.push(format!(r#"{{"op":"rattr","id":{id},"name":"class"}}"#)),
            }
        }
        format!("[{}]", ops.join(","))
    }
}

/// The documents: every corpus widget under `dirs`, then `generated` generated ones; each as its batch and last id.
pub fn widgets(dirs: &[&str], generated: usize, seed: u64) -> Vec<(String, String, i64)> {
    let mut out = Vec::new();
    for path in corpus_files(dirs) {
        let source = std::fs::read(&path).expect("a corpus file");
        let (ops, last) = html_to_ops(&source, 4);
        out.push((path.display().to_string(), document(&ops), last));
    }
    let mut r = Lcg::new(seed);
    for i in 0..generated {
        let (ops, last) = html_to_ops(generated_html(&mut r).as_bytes(), 4);
        out.push((format!("generated {i}"), document(&ops), last));
    }
    out
}
