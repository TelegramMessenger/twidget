//! The cursor styles against tdesktop's (`oracle/cursor_oracle`: tgw_cursor.cpp over a seam renderer whose build count,
//! HTML, master CSS and viewport the cases set): widget-sandbox's corpus widgets and generated documents as the
//! shadow tree writes them, each through a session of pointer calls at their widget ids (and none, and ids no element
//! has) with hover and active targets moving, builds that keep or patch the HTML, viewports and master CSS that change:
//! every answer, the styles' state and every indexed element's computed cursor alike. Skipped without
//! `TWIDGET_ORACLE_DIR` or without tdesktop's sources.

mod common;

use std::sync::Arc;

use common::docs::{Shadow, pick, widgets};
use common::*;
use twidget_document::Process;
use twidget_helper::cursor::{CursorSource, CursorStyles};

/// Rules whose cursor follows the pointer's targets, on top of the renderer's master CSS.
const CURSOR_RULES: &str = "\n.c0:hover{cursor:pointer} .c1:active{cursor:pointer} .c2:hover>*{cursor:pointer} \
     .c3:hover .c4{cursor:pointer} :active>.c5{cursor:pointer} div:hover>span{cursor:pointer} a:hover{cursor:text} \
     button:active{cursor:default} [class~=c6]:hover{cursor:pointer}\n";

struct Source {
    builds: i32,
    html: Vec<u8>,
    master: Vec<u8>,
    width: f64,
    height: i32,
}

impl CursorSource for Source {
    fn builds(&self) -> i32 {
        self.builds
    }
    fn document(&mut self) -> (&[u8], &[u8], f64, i32) {
        (&self.html, &self.master, self.width, self.height)
    }
}

fn answer(styles: &mut CursorStyles, source: &mut Source, hit: i64, hover: i64, active: i64) -> Vec<u8> {
    let mut out = Out::default();
    out.flag(styles.pointer(source, hit, hover, active));
    let s = styles.state();
    out.i64(s.hover).i64(s.active).i32(s.builds).f64(s.width).i32(s.height).flag(s.document);
    out.u64(s.html_bytes as u64).u64(s.master_bytes as u64);
    out.u32(s.elements.len() as u32);
    for (id, cursor) in &s.elements {
        out.i64(*id).str(cursor);
    }
    out.0
}

/// Pointer calls, how many elements they indexed and how many answered `pointer`.
#[derive(Default)]
struct Tally {
    calls: usize,
    pointers: usize,
    indexed: usize,
    patches: usize,
    rebuilds: usize,
}

fn session(c: &mut Comparison, process: &Arc<Process>, name: &str, batch: &str, last: i64, r: &mut Lcg, tally: &mut Tally) {
    let mut shadow = Shadow::new(batch, last);
    let masters =
        [twidget_renderer::master_css_for(&[]), [twidget_renderer::master_css_for(&[]), CURSOR_RULES.as_bytes().to_vec()].concat()];
    let dpr = *pick(r, &[1.0, 2.0, 1.5]);
    let mut styles = CursorStyles::new(dpr, Arc::clone(process));
    c.case(&framed(b'N', &Out::default().f64(dpr).0), (1, Vec::new()), format!("{name}: new"));
    let mut source = Source { builds: 1, html: shadow.html(), master: masters[1].clone(), width: 320.0, height: 600 };
    for step in 0..14 {
        match r.below(10) {
            0..=2 => {
                let patch = shadow.patch(r);
                shadow.apply(&patch);
                source.html = shadow.html();
                source.builds += 1;
                tally.patches += 1;
            }
            3 => source.builds += 1,
            4 => {
                source.width = *pick(r, &[320.0, 300.5, 412.0]);
                source.builds += 1;
                tally.rebuilds += 1;
            }
            5 => {
                source.height = *pick(r, &[600, 50, 240]);
                source.builds += 1;
            }
            6 => {
                source.master = masters[r.below(2)].clone();
                source.builds += 1;
            }
            _ => {}
        }
        let mut id = || -> i64 {
            match r.below(8) {
                0 => 0,
                1 => 999_999,
                _ if shadow.ids.is_empty() => 4,
                _ => shadow.ids[r.below(shadow.ids.len())],
            }
        };
        let (hit, hover, active) = (id(), id(), id());
        let mut record = Out::default();
        record.i32(source.builds).str(&source.html).str(&source.master).f64(source.width).i32(source.height);
        record.i64(hit).i64(hover).i64(active);
        let rust = answer(&mut styles, &mut source, hit, hover, active);
        tally.calls += 1;
        tally.pointers += usize::from(rust[0] == 1);
        tally.indexed += styles.state().elements.len();
        c.case(&framed(b'p', &record.0), (1, rust), format!("{name}: step {step} hit {hit} hover {hover} active {active}"));
    }
}

fn compare(dirs: &[&str], generated: usize, seed: u64, what: &str) {
    let Some(oracle) = helper_oracle("cursor_oracle") else { return };
    let process = Arc::new(Process::default());
    let mut r = Lcg::new(seed);
    let mut c = Comparison::new();
    let mut tally = Tally::default();
    let docs = widgets(dirs, generated, seed + 1);
    for (name, batch, last) in &docs {
        session(&mut c, &process, name, batch, *last, &mut r, &mut tally);
    }
    c.check(&oracle);
    eprintln!(
        "cursor, {what}: {} documents, {} pointer calls alike ({} answered pointer, {} elements' cursors, {} patches, {} rebuilds)",
        docs.len(),
        tally.calls,
        tally.pointers,
        tally.indexed,
        tally.patches,
        tally.rebuilds
    );
}

#[test]
fn corpus_alike() {
    compare(&["corpus"], 0, 0x4355_0001, "the corpus");
}

#[test]
fn generated_alike() {
    compare(&[], 1200, 0x4355_0002, "generated documents");
}
