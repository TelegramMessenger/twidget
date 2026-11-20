//! Painting, ported (stage L13): documents cascaded, built and laid out as the renderer builds them, then painted
//! through `oracle/lhdump` (`Q`) and the port with the renderer's settings: the viewport as the clip, scrolled views,
//! culling, the paint trace, a snap scale, the elements the renderer skips, paints without background images, paints
//! bare or leaves unclipped, and active controls with an accent. Every container call is compared in order with its
//! arguments (floats as bits): texts, list markers, images, solid fills, the three gradients with their stops,
//! borders, clips, clip paths, groups with their matrices and bounds, filters and backdrops, box shadows, paths, the
//! controls' questions; then what painting keeps (every item's ink and drawn ink by paint generation, each control's
//! look). The documents: widget-sandbox's corpus light and dark at several widths with a media change and a `:hover`
//! restyle, every tag, written shapes for each branch (backgrounds of every layer kind, repeat, size, position,
//! attachment, clip and origin; every border style and radius; shadows; gradients with hints and repeats; opacity,
//! filters, `backdrop-filter`, transforms and their origins, clip paths, overflow clips with radii, z-index stacking,
//! fixed boxes, form controls, list markers, text decorations, right to left, collapsed table borders), generated
//! documents, the `LH_NO_*` switches, and a deep document on a small stack. SVG (stage L14) is compared in every case
//! and on its own: the corpus's SVGs, written SVG shapes for each branch, generated SVG documents, the group cache's
//! and late cache's switches, the container's cache answering as one (`draw_cached` with its keys), each `<svg>`'s
//! `paint_shapes`, and a deep SVG on a small stack. A later stage's paint (an `<img>`'s image, L15) is the port's
//! seam, standing for the C++'s call there, and counted.

mod common;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::paint::*;
use common::*;
use twidget_litehtml::document::Switches;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// `Q` cases through lhdump and the port, checked in batches of about 24 MB; settings (`E`) open every batch.
struct Run {
    name: &'static str,
    lh: Lh,
    switches: Switches,
    settings: Vec<u8>,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    compared: usize,
    skipped_calls: usize,
    tally: PaintTally,
    oracle: Option<Oracle>,
}

impl Run {
    fn new(name: &'static str) -> Run {
        Run {
            name,
            lh: Lh::new(),
            switches: Switches::default(),
            settings: Vec::new(),
            batch: Vec::new(),
            bytes: 0,
            cases: 0,
            compared: 0,
            skipped_calls: 0,
            tally: PaintTally::default(),
            oracle: Oracle::find("lhdump"),
        }
    }

    /// A run with a switch set on both sides.
    fn with_switch(name: &'static str, switch: &str) -> Run {
        let mut run = Run::new(name);
        run.settings = framed(b'E', format!("{switch}=1").as_bytes());
        match switch {
            "LH_NO_LAYOUT_CACHE" => run.switches.layout_cache = false,
            "LH_NO_POSITIONED_REUSE" => run.switches.positioned_reuse = false,
            "LH_NO_FIXED_LAYOUT_REUSE" => run.switches.fixed_layout_reuse = false,
            "LH_LAYOUT_CACHE_CHECK" => run.switches.layout_cache_check = true,
            "LH_NO_COLUMN_REUSE" => run.switches.column_reuse = false,
            "LH_NO_TRANSFORMED_CULLING" => run.switches.transformed_culling = false,
            "LH_NO_SVG_GROUP_CACHE" => run.switches.svg_group_cache = false,
            "LH_SVG_LATE_CACHE" => run.switches.svg_late_cache = true,
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, input: Vec<u8>, what: impl Into<String>) {
        let rust = paint_answer(&self.lh, self.switches, &input, &mut self.tally);
        self.cases += 1;
        self.compared += 1;
        self.bytes += input.len() + rust.1.len();
        if self.oracle.is_some() {
            self.batch.push((input, rust, what.into()));
        }
        if self.bytes > 24 << 20 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        self.bytes = 0;
        let Some(oracle) = &self.oracle else {
            self.lh = Lh::new();
            return;
        };
        if batch.is_empty() {
            return;
        }
        let mut input = self.settings.clone();
        for (case, _, _) in &batch {
            input.extend(framed(b'Q', case));
        }
        let answers = oracle.run(input, batch.len());
        // Every batch is a process of its own: the next one starts with a fresh name table and declaration cache.
        self.lh = Lh::new();
        let mut differences = Vec::new();
        for ((case, rust, what), cpp) in batch.iter().zip(&answers) {
            let (cpp_body, _) = split_answer(&cpp.1);
            let (rust_body, _) = split_answer(&rust.1);
            if cpp.0 != rust.0 || cpp_body != rust_body {
                differences.push(explain(self.switches, case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_paint_calls(&cpp.1, &rust.1, &mut self.skipped_calls) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(3000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> PaintTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "painted (no oracle)" };
        eprintln!(
            "{}: {} cases, {} {compared} ({} C++ calls of later stages' paint skipped); {}",
            self.name,
            self.cases,
            self.compared,
            self.skipped_calls,
            self.tally.report()
        );
        std::mem::take(&mut self.tally)
    }
}

/// Where two answers' bodies part: the field the Rust answer was writing there.
fn explain(switches: Switches, case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        paint_answer(&Lh::new(), switches, case, &mut PaintTally::default());
        take_marks()
    };
    let item = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(16).min(b.len())..(at + 64).min(b.len())];
        hex(&piece[..piece.len().min(48)])
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, {field} (item or element {item}):\n  C++  {}\n  Rust {}\n  in {}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        near(&rust.1),
        what.chars().take(3000).collect::<String>()
    )
}

// ---- Steps ---------------------------------------------------------------------------------------------------

fn paint(step: PaintStep) -> Step {
    Step::Paint(Box::new(step))
}

/// The paints of a case laid out at `width` in a `height` viewport: the viewport as the renderer paints it, a view
/// scrolled down and culled, the trace, a snapped and offset paint with active controls, and one with what the
/// renderer skips; `k` picks the elements.
fn paint_steps(width: f32, height: f32, k: u32) -> Vec<Step> {
    let mut scrolled = PaintStep::viewport(width, height);
    scrolled.y = -(height * 0.75).floor();
    scrolled.cull = true;
    scrolled.hdc = 3;
    let mut traced = PaintStep::viewport(width, height);
    traced.trace = true;
    traced.cull = true;
    let mut snapped = PaintStep::viewport(width, height);
    snapped.snap = 2.0;
    snapped.x = 0.25;
    snapped.y = 0.75;
    snapped.active = true;
    snapped.accent = [10, 200, 30, if k.is_multiple_of(2) { 255 } else { 0 }];
    let mut skipping = PaintStep::viewport(width, height * 2.0);
    skipping.skip = vec![k + 3, k * 7 + 11];
    skipping.skip_backgrounds = vec![k + 1, k + 2, k * 3 + 5];
    skipping.skip_scroll = vec![k * 5 + 4];
    skipping.bare = (k * 11 + 6) as i32;
    skipping.bare_shadows = k % 2 == 1;
    skipping.unclipped = (k * 13 + 2) as i32;
    skipping.cull = k.is_multiple_of(3);
    let mut unclipped = PaintStep::viewport(width, height);
    unclipped.clip = None;
    vec![
        Step::Layout(width, None),
        paint(PaintStep::viewport(width, height)),
        Step::Dump,
        paint(scrolled),
        paint(traced),
        Step::Dump,
        paint(snapped),
        paint(skipping),
        paint(unclipped),
        Step::Dump,
    ]
}

// ---- The comparison ------------------------------------------------------------------------------------------

#[test]
fn the_corpus_paints_alike() {
    let mut run = Run::new("corpus");
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    let light = themed_master(false);
    let dark = themed_master(true);
    for (k, (name, text)) in pages.iter().enumerate() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, height, other) = if dark_theme { (412.0, 915.0, (800.0, 600.0)) } else { (375.0, 667.0, (1024.0, 768.0)) };
            let mut steps = paint_steps(width, height, k as u32);
            if k % 3 == 0 {
                let t = k as u32 * 7;
                steps.extend([
                    Step::Media(other.0, other.1),
                    Step::Layout(other.0, None),
                    paint(PaintStep::viewport(other.0, other.1)),
                    Step::Dump,
                ]);
                steps.extend([Step::Hover(t), Step::Layout(other.0, None), paint(PaintStep::viewport(other.0, other.1)), Step::Dump]);
            }
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(paint_record(width, height, 2.0, master, b"", &html, &steps), format!("{name} ({theme})"));
        }
    }
    let tally = run.finish();
    assert_eq!(tally.documents, 824);
}

/// The corpus again at narrower and wider viewports: light at 150 and 768 px, dark at 1440 and 240 px, each with a
/// media change between.
#[test]
fn the_corpus_paints_alike_at_more_widths() {
    let mut run = Run::new("corpus at more widths");
    let pages = corpus_pages();
    let light = themed_master(false);
    let dark = themed_master(true);
    for (k, (name, text)) in pages.iter().enumerate() {
        let snap = snapshot(text);
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(&snap, dark_theme, vars);
            let (width, other) = if dark_theme { (1440.0, 240.0) } else { (150.0, 768.0) };
            let mut culled = PaintStep::viewport(other, 900.0);
            culled.cull = true;
            culled.y = -300.0;
            culled.hdc = 9 + k as u32;
            let steps = vec![
                Step::Layout(width, None),
                paint(PaintStep::viewport(width, 900.0)),
                Step::Dump,
                Step::Media(other, 900.0),
                Step::Layout(other, None),
                paint(PaintStep::viewport(other, 900.0)),
                paint(culled),
                Step::Dump,
            ];
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(paint_record(width, 900.0, 2.0, master, b"", &html, &steps), format!("{name} ({theme}, {width} and {other} px)"));
        }
    }
    let tally = run.finish();
    assert_eq!(tally.documents, 824);
}

/// Every tag, the table and form ones too, under the master sheet and the UA layers.
#[test]
fn every_tag_paints_alike() {
    let mut run = Run::new("every tag");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    for quirks in [false, true] {
        let html = every_tag_document(quirks);
        let what = if quirks { "every tag in quirks mode" } else { "every tag" };
        let steps = paint_steps(375.0, 1200.0, 5);
        run.case(paint_record(375.0, 667.0, 1.0, &master, b"", &html, &steps), format!("{what}, master CSS"));
        run.case(paint_record(375.0, 667.0, 2.0, &light, b"", &html, &steps), format!("{what}, light UA layers"));
        run.case(paint_record(412.0, 915.0, 3.0, &dark, b"", &themed_html(&html, true, &vars), &steps), format!("{what}, dark UA layers"));
        run.case(paint_record(375.0, 667.0, 2.0, b"", b"", &html, &steps), format!("{what}, no sheet"));
    }
    let tally = run.finish();
    assert_eq!(tally.documents, 8);
}

// ---- Written shapes ------------------------------------------------------------------------------------------

/// Shapes for each branch of painting, written out.
fn written_paint_documents() -> Vec<(&'static str, String)> {
    let page = |body: &str| format!("<!DOCTYPE html><html><head></head><body>{body}</body></html>");
    let styled = |css: &str, body: &str| format!("<!DOCTYPE html><html><head><style>{css}</style></head><body>{body}</body></html>");
    let boxes = |decls: &[&str]| -> String {
        decls.iter().map(|d| format!("<div style=\"width:120px;height:60px;margin:6px;{d}\">x</div>")).collect::<String>()
    };
    let mut docs = vec![
        // Backgrounds: every layer kind, repeat, size, position, attachment, clip and origin.
        (
            "background colours",
            page(&boxes(&["background:red", "background:rgba(0,0,255,.5)", "background:transparent", "background-color:#0000"])),
        ),
        (
            "background images repeated",
            page(&boxes(&[
                "background:url(a.png)",
                "background:url(bb.png) repeat-x",
                "background:url(photo.png) repeat-y",
                "background:url(x9.png) no-repeat",
                "background:url(a.png) space",
                "background:url(a.png) round",
            ])),
        ),
        (
            "background sizes",
            page(&boxes(&[
                "background:url(photo.png) no-repeat;background-size:cover",
                "background:url(photo.png) no-repeat;background-size:contain",
                "background:url(photo.png);background-size:50% auto",
                "background:url(photo.png);background-size:auto 30px",
                "background:url(photo.png);background-size:40px 20px",
                "background:url(photo.png);background-size:auto",
                "background:linear-gradient(red,blue);background-size:30px 20px",
                "background:linear-gradient(red,blue) no-repeat;background-size:50%",
                "background:url(photo.png) no-repeat;background-size:cover;height:400px;width:20px",
                "background:url(photo.png) no-repeat;background-size:contain;height:5px;width:200px",
            ])),
        ),
        (
            "background positions",
            page(&boxes(&[
                "background:url(bb.png) no-repeat right bottom",
                "background:url(bb.png) no-repeat center",
                "background:url(bb.png) no-repeat 25% 75%",
                "background:url(bb.png) no-repeat 10px -5px",
                "background:url(bb.png) no-repeat right 10px top 4px",
                "background:radial-gradient(red,blue) no-repeat 100% 0 / 40px 40px",
            ])),
        ),
        (
            "background attachment, clip and origin",
            page(&boxes(&[
                "background:url(a.png) fixed",
                "background:url(a.png);background-attachment:local",
                "border:5px dashed red;padding:7px;background:yellow;background-clip:padding-box",
                "border:5px dashed red;padding:7px;background:yellow;background-clip:content-box",
                "border:5px dashed red;padding:7px;background:url(a.png) no-repeat;background-origin:border-box",
                "border:5px dashed red;padding:7px;background:url(a.png) no-repeat;background-origin:content-box",
                "border:5px dashed red;padding:7px;background:url(a.png),linear-gradient(red,blue);background-clip:content-box,padding-box;background-origin:content-box",
            ])),
        ),
        (
            "background layers",
            page(&boxes(&[
                "background:url(a.png) no-repeat, url(bb.png) repeat-x, linear-gradient(red,blue), green",
                "background-image:url(a.png),url(bb.png),url(photo.png);background-repeat:no-repeat,repeat;background-position:0 0,50% 50%;background-size:10px,auto,cover",
                "background-image:none,url(a.png);background-color:pink",
                "background-image:linear-gradient(red,blue),radial-gradient(white,black),conic-gradient(red,blue)",
            ])),
        ),
        ("body background propagated to the root", styled("body{background:linear-gradient(red,blue) fixed;margin:10px}", "<p>a</p>")),
        ("root and body backgrounds", styled("html{background:#eee}body{background:url(a.png);border:2px solid}", "<p>a</p>")),
        ("body background with the root's transparent", styled("html{background:transparent}body{background-color:teal}", "<p>a</p>")),
        // Gradients of each kind, with hints and repeats.
        (
            "linear gradients",
            page(&boxes(&[
                "background:linear-gradient(red,blue)",
                "background:linear-gradient(0deg,red,blue)",
                "background:linear-gradient(90deg,red,blue)",
                "background:linear-gradient(180deg,red,blue)",
                "background:linear-gradient(270deg,red,blue)",
                "background:linear-gradient(45deg,red,blue)",
                "background:linear-gradient(-30deg,red,blue)",
                "background:linear-gradient(400deg,red,blue)",
                "background:linear-gradient(1turn,red,blue)",
                "background:linear-gradient(to right,red,blue)",
                "background:linear-gradient(to left top,red,blue)",
                "background:linear-gradient(to bottom right,red,blue)",
                "background:linear-gradient(to left bottom,red,blue)",
                "background:linear-gradient(red 10%,green 30%,blue 90%)",
                "background:linear-gradient(red,30%,blue)",
                "background:linear-gradient(red 0 20px,blue 20px 40px)",
                "background:linear-gradient(red 40%,blue 20%,green)",
                "background:linear-gradient(red,green,blue,yellow,black)",
                "background:linear-gradient(transparent,red,transparent,blue,transparent)",
                "background:linear-gradient(in oklab,red,blue)",
            ])),
        ),
        (
            "repeating linear gradients",
            page(&boxes(&[
                "background:repeating-linear-gradient(red,blue 20px)",
                "background:repeating-linear-gradient(45deg,red 0 5px,blue 5px 10px)",
                "background:repeating-linear-gradient(red 50%,blue 50%)",
                "background:repeating-linear-gradient(red 0,blue 0.001px)",
                "background:repeating-linear-gradient(red 120%,blue 150%)",
                "background:repeating-linear-gradient(red -150%,blue -120%)",
                "background:repeating-linear-gradient(red,transparent 10px,blue 20px)",
                "background:repeating-linear-gradient(red 1e20%,blue 1e20%)",
                "background:repeating-linear-gradient(red 0,blue 1px)",
                "background:repeating-linear-gradient(red 0e999%,blue 10px)",
            ])),
        ),
        (
            "radial gradients",
            page(&boxes(&[
                "background:radial-gradient(red,blue)",
                "background:radial-gradient(circle,red,blue)",
                "background:radial-gradient(circle closest-side,red,blue)",
                "background:radial-gradient(circle farthest-side,red,blue)",
                "background:radial-gradient(circle closest-corner,red,blue)",
                "background:radial-gradient(circle farthest-corner,red,blue)",
                "background:radial-gradient(ellipse closest-side,red,blue)",
                "background:radial-gradient(ellipse farthest-side,red,blue)",
                "background:radial-gradient(ellipse closest-corner,red,blue)",
                "background:radial-gradient(closest-corner at 10% 20%,red,blue)",
                "background:radial-gradient(farthest-corner at left top,red,blue)",
                "background:radial-gradient(at right bottom,red,blue)",
                "background:radial-gradient(at 30px 10px,red,blue)",
                "background:radial-gradient(circle 20px at center,red,blue)",
                "background:radial-gradient(40px 10px at 50% 50%,red,blue)",
                "background:radial-gradient(50% 25%,red,blue)",
                "background:radial-gradient(circle closest-side at 0 0,red,blue)",
                "background:radial-gradient(red,30%,blue 80%)",
                "background:repeating-radial-gradient(red,blue 10px)",
                "background:repeating-radial-gradient(circle,red 0 4px,transparent 4px 8px)",
            ])),
        ),
        (
            "conic gradients",
            page(&boxes(&[
                "background:conic-gradient(red,blue)",
                "background:conic-gradient(from 45deg,red,blue)",
                "background:conic-gradient(at 20% 70%,red,green,blue)",
                "background:conic-gradient(red 0deg,blue 90deg,green 180deg)",
                "background:conic-gradient(red 25%,blue 0 50%,green 0)",
                "background:conic-gradient(red,45deg,blue)",
                "background:conic-gradient(from 90deg at left top,red,blue)",
                "background:repeating-conic-gradient(red 0 10deg,blue 10deg 20deg)",
                "background:repeating-conic-gradient(red 0,blue 0.01deg)",
                "background:conic-gradient(in hsl longer hue,red,blue)",
            ])),
        ),
        // Every border style and radius.
        (
            "border styles",
            page(&boxes(&[
                "border:3px none red",
                "border:3px hidden red",
                "border:3px dotted red",
                "border:3px dashed red",
                "border:3px solid red",
                "border:6px double red",
                "border:6px groove red",
                "border:6px ridge red",
                "border:6px inset red",
                "border:6px outset red",
                "border-width:1px 2px 3px 4px;border-style:solid dotted dashed double;border-color:red green blue yellow",
                "border-left:4px solid;border-top:0 solid",
                "border:thin solid;border-right:thick dashed",
                "border:medium solid currentcolor;color:purple",
            ])),
        ),
        (
            "border radii",
            page(&boxes(&[
                "border:2px solid;border-radius:10px",
                "border:2px solid;border-radius:50%",
                "border:2px solid;border-radius:10px 20px 30px 40px",
                "border:2px solid;border-radius:20px/10px",
                "border:2px solid;border-radius:100px",
                "border:2px solid;border-radius:0 60px",
                "border:8px solid;border-radius:6px",
                "border:2px solid;border-top-left-radius:30% 60%",
                "background:red;border-radius:10px",
                "background:url(a.png);border-radius:12px;border:3px solid",
                "border:0 solid;border-radius:10px;background:blue",
            ])),
        ),
        // Shadows, inset and outer, with spread and blur.
        (
            "box shadows",
            page(&boxes(&[
                "box-shadow:2px 3px red",
                "box-shadow:2px 3px 4px red",
                "box-shadow:2px 3px 4px 5px red",
                "box-shadow:-2px -3px 4px -5px red",
                "box-shadow:inset 2px 3px 4px 5px red",
                "box-shadow:inset 0 0 10px blue,0 0 3px green,inset 1px 1px yellow",
                "box-shadow:0 0 10px red;border-radius:12px",
                "box-shadow:inset 0 0 0 4px red;border:3px solid;border-radius:15px",
                "box-shadow:0 0 -4px red",
                "box-shadow:30px 30px 0 currentcolor;color:green",
                "box-shadow:200px 0 10px red",
            ])),
        ),
        (
            "inline box shadows and borders across lines",
            page(
                "<p style=\"width:90px\"><span style=\"box-shadow:2px 2px 3px red;border:2px solid;border-radius:6px;background:yellow;padding:2px\">a span that runs over several lines of text</span></p>",
            ),
        ),
        (
            "inline backgrounds",
            page(
                "<p style=\"width:100px\"><span style=\"background:linear-gradient(red,blue);border-left:5px solid;border-right:3px dotted\">words words words words words</span> <a style=\"background:url(a.png)\">link text here</a></p>",
            ),
        ),
        // Outlines.
        (
            "outlines",
            page(&boxes(&[
                "outline:2px solid red",
                "outline:3px dashed blue;outline-offset:4px",
                "outline:2px dotted;outline-offset:-6px",
                "outline:4px double;border-radius:10px",
                "outline:0 solid",
                "outline:2px none red",
                "outline:5px groove;outline-offset:2px;border-radius:5px 0",
            ])),
        ),
        // Opacity, filters and backdrop-filter.
        (
            "opacity",
            page(
                "<div style=\"opacity:.5;background:red\">a<div style=\"opacity:.3;box-shadow:0 0 20px\">b</div></div><div style=\"opacity:0\">gone</div><span style=\"opacity:.7\">inline</span><div style=\"opacity:1\">c</div>",
            ),
        ),
        (
            "filters",
            page(&boxes(&[
                "filter:blur(3px)",
                "filter:grayscale(1) brightness(1.2)",
                "filter:drop-shadow(2px 3px 4px red)",
                "filter:contrast(2) hue-rotate(90deg) invert(1) saturate(3) sepia(.5) opacity(.4)",
                "filter:blur(2px);opacity:.5;transform:rotate(10deg)",
                "filter:blur(0)",
            ])),
        ),
        (
            "backdrop filters",
            page(&boxes(&[
                "backdrop-filter:blur(4px)",
                "backdrop-filter:blur(4px);border-radius:20px",
                "backdrop-filter:grayscale(1);filter:blur(1px)",
                "backdrop-filter:blur(2px);border-radius:50%;background:rgba(255,255,255,.3)",
            ])),
        ),
        // Transforms and their origins.
        (
            "transforms",
            page(&boxes(&[
                "transform:translate(10px,5px)",
                "transform:translate(50%,-20%)",
                "transform:rotate(30deg)",
                "transform:rotate(30deg);transform-origin:0 0",
                "transform:scale(1.5,.5);transform-origin:right bottom",
                "transform:skew(10deg,5deg)",
                "transform:matrix(1,0.2,0.3,1,10,20)",
                "transform:rotateX(60deg)",
                "transform:rotateY(45deg);backface-visibility:hidden",
                "transform:translateX(20px) rotate(45deg) scale(2)",
                "transform:scale(0)",
                "transform:rotate(90deg);transform-origin:10px 20%",
                "transform:scale(1e30)",
            ])),
        ),
        (
            "transformed and culled content",
            page(
                "<div style=\"transform:rotate(45deg);width:300px\"><p>a</p><p style=\"margin-top:900px\">far</p></div><div style=\"transform:scale(.5);margin-top:200px\"><p>b</p><p style=\"margin-top:1200px\">farther</p></div>",
            ),
        ),
        // Clip paths.
        (
            "clip paths",
            page(&boxes(&[
                "clip-path:inset(10px)",
                "clip-path:inset(5px 10% 15px 20px round 8px)",
                "clip-path:inset(60% 60%)",
                "clip-path:circle(40%)",
                "clip-path:circle(20px at 10px 10px)",
                "clip-path:ellipse(30% 40% at right top)",
                "clip-path:polygon(0 0,100% 0,50% 100%)",
                "clip-path:polygon(evenodd,0 0,100% 0,0 100%,100% 100%)",
                "clip-path:circle(closest-side)",
            ])),
        ),
        ("a clip path on the root", styled("html{clip-path:inset(5px)}body{background:red}", "<p>a</p>")),
        // Overflow clips with radii.
        (
            "overflow clips",
            page(&boxes(&[
                "overflow:hidden;border:3px solid;border-radius:12px",
                "overflow:auto;border-radius:50%",
                "overflow:scroll;border:10px solid;border-radius:5px",
                "overflow:clip",
                "contain:paint;border-radius:8px",
                "overflow:hidden;display:inline-block",
                "overflow:visible",
            ])),
        ),
        ("an inline that clips", page("<p><span style=\"overflow:hidden;border-radius:4px\">an inline with overflow</span></p>")),
        (
            "a list item that clips",
            page("<ul><li style=\"overflow:hidden;border-radius:10px;padding:4px;border:2px solid\">item</li></ul>"),
        ),
        // z-index stacking with negative layers.
        (
            "z-index stacking",
            page(
                "<div style=\"position:relative;z-index:0\"><div style=\"position:absolute;z-index:-2;background:red;width:50px;height:50px\">n2</div><div style=\"position:absolute;z-index:-1;background:green;left:10px;width:50px;height:50px\">n1</div><div style=\"position:relative;z-index:3;background:blue\">p3</div><div style=\"position:relative;background:yellow\">p0</div><div style=\"position:absolute;z-index:1;top:5px;background:pink\">p1</div><div style=\"float:left;background:gray\">float</div><span style=\"display:inline-block;background:orange\">ib</span></div>",
            ),
        ),
        (
            "z-index inside paint groups",
            page(
                "<div style=\"opacity:.9\"><div style=\"position:relative;z-index:-1;background:red\">neg</div><div style=\"position:absolute;z-index:2\">two</div><div><div style=\"position:relative;z-index:1\">nested one</div></div><div style=\"transform:scale(1)\"><div style=\"position:relative;z-index:-5\">own group</div></div></div>",
            ),
        ),
        (
            "positioned groups in each pass",
            page(
                "<div style=\"position:relative;z-index:1;opacity:.5\">positioned group</div><div style=\"float:left;transform:rotate(5deg)\">float group</div><span style=\"filter:blur(1px)\">inline group</span><div style=\"opacity:.4\">block group</div><div style=\"position:sticky;top:0\">sticky group</div>",
            ),
        ),
        // Fixed boxes.
        (
            "fixed boxes",
            page(
                "<div style=\"position:fixed;top:10px;left:10px;background:red;width:40px;height:40px\">f</div><div style=\"overflow:hidden;height:50px\"><div style=\"position:fixed;bottom:0;background:blue\">fixed in a clip</div></div><div style=\"opacity:.5\"><div style=\"position:fixed;right:0\">fixed in a group</div></div><div style=\"position:fixed;top:0;opacity:.5\">fixed group</div><div style=\"height:2000px\"></div>",
            ),
        ),
        (
            "fixed groups inside clips",
            page(
                "<div style=\"overflow:hidden;height:30px;border-radius:5px\"><div style=\"position:fixed;transform:translate(5px);top:0\">a</div><div style=\"position:fixed;top:20px\">b<div style=\"position:fixed;top:40px\">c</div></div></div>",
            ),
        ),
        // Form controls.
        (
            "form controls",
            page(
                "<input><input type=\"search\" value=\"q\"><input type=\"number\" value=\"5\"><input type=\"date\" value=\"2024-02-03\"><input type=\"button\" value=\"b\"><input type=\"submit\"><input type=\"reset\" disabled><input type=\"checkbox\"><input type=\"checkbox\" checked><input type=\"checkbox\" checked disabled><input type=\"radio\"><input type=\"radio\" checked><input type=\"radio\" checked disabled><input type=\"checkbox\" disabled><input type=\"range\"><input type=\"range\" value=\"20\" disabled><input type=\"range\" min=\"5\" max=\"5\"><input type=\"color\" value=\"#ff8800\"><input type=\"color\"><select><option>one</option></select><select disabled><option selected>two</option></select><button>press</button><button disabled>no</button><textarea>text</textarea><textarea style=\"resize:none\"></textarea><textarea placeholder=\"hint\" rows=\"4\"></textarea>",
            ),
        ),
        (
            "form controls styled",
            page(
                "<input style=\"border:2px solid red\"><input type=\"checkbox\" checked style=\"accent-color:purple\"><input type=\"range\" style=\"accent-color:green;width:300px\"><input style=\"appearance:none\"><select style=\"appearance:none\"><option>x</option></select><select style=\"background:yellow\"><option>y</option></select><button style=\"border-radius:9px\">r</button><input type=\"number\" style=\"height:12px\"><input type=\"number\" style=\"appearance:none\"><input type=\"search\" style=\"height:40px\"><select style=\"width:20px\"><option>z</option></select><input type=\"checkbox\" style=\"width:30px;height:10px\"><input type=\"range\" style=\"width:10px\">",
            ),
        ),
        (
            "meters and progress bars",
            page(
                "<meter value=\"0.5\"></meter><meter value=\"2\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><meter value=\"5\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"5\"></meter><meter value=\"9\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"1\"></meter><meter value=\"x\"></meter><meter style=\"appearance:none\" value=\"1\"></meter><meter style=\"width:2px\" value=\"1\"></meter><progress></progress><progress value=\"30\" max=\"100\"></progress><progress value=\"3\" max=\"-1\"></progress><progress value=\"200\" max=\"100\"></progress><progress value=\"0\"></progress>",
            ),
        ),
        (
            "fieldsets",
            page(
                "<fieldset><legend>Title</legend>body</fieldset><fieldset style=\"border:4px dashed red;border-radius:8px;background:yellow\"><legend style=\"padding:0 10px\">Big legend<br>two lines</legend>x</fieldset><fieldset><legend style=\"float:left\">floated</legend>y</fieldset><fieldset>no legend</fieldset><fieldset style=\"border:0\"><legend>bare</legend></fieldset><fieldset style=\"background:url(a.png)\"> <!-- c --> <legend>after a comment</legend></fieldset>",
            ),
        ),
        // List markers of each type.
        (
            "list markers",
            styled(
                "li{margin:2px}",
                "<ul><li>disc</li><li style=\"list-style-type:circle\">circle</li><li style=\"list-style-type:square\">square</li><li style=\"list-style-type:none\">none</li></ul><ol><li>decimal</li><li style=\"list-style-type:decimal-leading-zero\">dlz</li><li style=\"list-style-type:lower-alpha\">la</li><li style=\"list-style-type:upper-alpha\">ua</li><li style=\"list-style-type:lower-latin\">ll</li><li style=\"list-style-type:upper-latin\">ul</li><li style=\"list-style-type:lower-roman\">lr</li><li style=\"list-style-type:upper-roman\">ur</li><li style=\"list-style-type:lower-greek\">lg</li><li style=\"list-style-type:armenian\">arm</li><li style=\"list-style-type:georgian\">geo</li><li style=\"list-style-type:hebrew\">heb</li><li style=\"list-style-type:cjk-ideographic\">cjk</li><li style=\"list-style-type:hiragana\">h</li></ol><ol start=\"9\"><li>nine</li><li>ten</li></ol>",
            ),
        ),
        (
            "list markers inside, images and positions",
            styled(
                "ul{list-style-position:inside}",
                "<ul><li>in</li><li style=\"list-style-image:url(m.png)\">image</li><li style=\"list-style-image:url(bb.png);list-style-position:outside\">image outside</li><li style=\"list-style:upper-roman inside\">roman</li></ul><ol style=\"list-style-position:inside\"><li>1</li></ol><div style=\"display:list-item;list-style:square\">div item</div><ol><li style=\"font-size:0\">no font</li><li style=\"line-height:40px\">tall</li></ol>",
            ),
        ),
        // Text decorations, ellipses, right to left.
        (
            "text decorations",
            page(
                "<p><u>under</u> <s>strike</s> <span style=\"text-decoration:overline wavy red\">over</span> <span style=\"text-decoration:underline dotted;text-decoration-thickness:3px\">thick</span> <a href=\"#\">link</a></p><p style=\"text-shadow:2px 2px 3px red\">shadowed text</p><p style=\"white-space:pre;text-decoration:underline\">a   b</p>",
            ),
        ),
        (
            "ellipses",
            page(
                "<div style=\"width:80px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis\">a long line of text that is cut</div><div style=\"width:5px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis\">tiny</div><div style=\"width:60px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis;direction:rtl\">\u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd} words words</div>",
            ),
        ),
        (
            "right to left",
            page(
                "<div dir=\"rtl\" style=\"direction:rtl\"><p>\u{5e9}\u{5dc}\u{5d5}\u{5dd} abc <span style=\"background:yellow;border:1px solid\">\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629}</span></p><ol><li>one</li><li style=\"list-style-position:inside\">two</li></ol><ul><li>x</li></ul><input type=\"checkbox\" checked></div>",
            ),
        ),
        // Collapsed table borders.
        (
            "collapsed table borders",
            page(
                "<table style=\"border-collapse:collapse;border:3px solid red\"><tr><td style=\"border:1px solid\">a</td><td style=\"border:5px double blue\">b</td></tr><tr><td style=\"border:2px dashed\">c</td><td style=\"border:hidden\">d</td></tr></table><table style=\"border-collapse:collapse\"><caption style=\"background:yellow\">cap</caption><tr style=\"background:pink\"><td style=\"border:4px groove\" rowspan=\"2\">e</td><td style=\"border:1px solid;visibility:hidden\">f</td></tr><tr><td>g</td></tr></table>",
            ),
        ),
        (
            "separate table borders",
            page(
                "<table style=\"border:2px outset;border-spacing:4px;background:#eee\"><caption>top</caption><tr style=\"background:linear-gradient(red,blue)\"><td style=\"border:2px inset;box-shadow:0 0 3px\">a</td><td style=\"background:url(a.png)\">b</td></tr></table><table style=\"display:inline-table;border:1px solid\"><tr><td>c</td></tr><caption style=\"caption-side:bottom\">bottom</caption></table>",
            ),
        ),
        // Images and SVG: their boxes are this stage's, what they draw inside later ones'.
        (
            "images and svg boxes",
            page(
                "<img src=\"a.png\" style=\"border:3px solid;border-radius:5px;box-shadow:0 0 4px\"><img src=\"\" width=\"0\"><svg width=\"40\" height=\"20\" style=\"background:yellow;border:1px solid\"><rect width=\"10\" height=\"10\"/></svg><svg width=\"0\" height=\"0\"></svg>",
            ),
        ),
        // Visibility, display and skipped content.
        (
            "visibility",
            page(
                "<div style=\"visibility:hidden;background:red\">hidden<div style=\"visibility:visible;background:blue\">visible child</div></div><div style=\"display:none;background:red\">none</div><p>after</p>",
            ),
        ),
        (
            "text and spaces",
            page("<p style=\"white-space:pre-wrap\">a  b\n  c</p><p style=\"font-size:0\">zero</p><p>\u{4e2d}\u{6587} caf\u{e9}</p>"),
        ),
        (
            "inline blocks and floats",
            page(
                "<p>text <span style=\"display:inline-block;border:2px solid;background:yellow\">ib <b>b</b></span> <span style=\"display:inline-flex;background:pink\">if</span> more</p><div style=\"float:right;background:green;box-shadow:2px 2px\">float<div style=\"position:relative\">rel</div></div>",
            ),
        ),
        (
            "native form controls",
            styled(
                "input,select,button,textarea,meter,progress{appearance:auto}",
                "<input><input type=\"search\" value=\"q\"><input type=\"number\" value=\"5\"><input type=\"number\" style=\"height:8px\"><input type=\"number\" style=\"height:15px\"><input type=\"date\"><input type=\"button\" value=\"b\"><input type=\"submit\"><input type=\"reset\" disabled><input type=\"checkbox\"><input type=\"checkbox\" checked><input type=\"checkbox\" checked disabled><input type=\"checkbox\" disabled><input type=\"radio\"><input type=\"radio\" checked><input type=\"radio\" checked disabled><input type=\"range\"><input type=\"range\" value=\"20\" disabled><input type=\"range\" value=\"80\" style=\"accent-color:red\"><input type=\"range\" style=\"width:12px\"><input type=\"color\" value=\"#ff8800\"><input type=\"color\"><input type=\"hidden\"><input type=\"file\"><select><option>one</option></select><select disabled style=\"width:20px\"><option selected>two</option></select><select style=\"border:1px solid red\"><option>three</option></select><button>press</button><button disabled>no</button><textarea>text</textarea><textarea style=\"resize:none\"></textarea><textarea style=\"width:10px;height:10px\"></textarea><meter value=\"0.5\"></meter><meter value=\"2\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><meter value=\"5\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"5\"></meter><meter value=\"9\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"1\"></meter><meter value=\"1\" style=\"width:2px\"></meter><meter value=\"x\" min=\"5\" max=\"1\"></meter><progress></progress><progress value=\"30\" max=\"100\"></progress><progress value=\"3\" max=\"-1\"></progress><progress value=\"0\"></progress><input type=\"checkbox\" checked style=\"accent-color:transparent\"><input style=\"width:0\">",
            ),
        ),
        (
            "an invisible root with a clip path",
            styled("html{clip-path:inset(1px);visibility:hidden}body{background:red}", "<p style=\"visibility:visible\">a</p>"),
        ),
        (
            "radial gradients on an edge",
            page(&boxes(&[
                "background:radial-gradient(ellipse closest-corner at 50% 0,red,blue)",
                "background:radial-gradient(ellipse farthest-corner at 0 50%,red,blue)",
                "background:radial-gradient(ellipse closest-corner at 0 0,red,blue)",
                "background:radial-gradient(ellipse closest-side at 100% 100%,red,blue)",
            ])),
        ),
        (
            "repeating gradients starting inside the box",
            page(&boxes(&[
                "background:repeating-linear-gradient(red 30%,blue 50%)",
                "background:repeating-linear-gradient(90deg,red 10px,transparent 15px,blue 30px)",
                "background:repeating-radial-gradient(red 40%,blue 45%)",
                "background:repeating-conic-gradient(red 90deg,blue 120deg)",
                "background:repeating-linear-gradient(red 0.5%,blue 0.52%)",
                "background:repeating-linear-gradient(red 0,blue 0.0245%)",
            ])),
        ),
        (
            "an ellipsis inside a word",
            page(
                "<div style=\"width:100px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis\">abcdefghijklmnopqrstuvwxyz</div><div style=\"width:30px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis\"><b>bold</b>words</div>",
            ),
        ),
        (
            "tables with hidden parts and tall rows",
            page(
                "<table style=\"border-collapse:collapse;border:2px solid\"><caption style=\"visibility:hidden\">hidden caption</caption><tr style=\"visibility:hidden\"><td style=\"border:3px solid\">a</td></tr><tr><td style=\"border:1px dashed\">b</td></tr></table><table><caption style=\"background:yellow\">top</caption><tr><td style=\"height:1500px;background:pink\">tall</td></tr></table>",
            ),
        ),
        (
            "native fields, buttons and selects (the UA's sentinel colours)",
            styled(
                "input,select,button,textarea{appearance:auto;border:1px solid rgba(1,2,3,.0045);background-color:rgba(3,2,1,.0045);color:rgba(2,3,1,.0045);border-radius:0}meter,progress{appearance:auto}",
                "<input value=\"v\"><input type=\"search\"><input type=\"number\" value=\"3\"><input type=\"number\" style=\"height:14px\"><input type=\"button\" value=\"b\"><input type=\"submit\" disabled><button>press</button><button disabled>off</button><select style=\"border-radius:5px\"><option>s</option></select><select disabled style=\"border-radius:5px;width:24px\"><option>t</option></select><textarea>text</textarea><textarea disabled style=\"resize:none\"></textarea><meter value=\"5\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><meter value=\"1\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"5\"></meter><meter value=\"1\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><progress value=\"2\" max=\"4\"></progress><input type=\"checkbox\" checked><input type=\"radio\"><input type=\"range\" value=\"10\"><input type=\"color\" value=\"#0f0\">",
            ),
        ),
        (
            "native controls in a dark scheme",
            styled(
                ":root{color-scheme:dark}input,select,button,textarea{appearance:auto;border:1px solid rgba(1,2,3,.0045);background-color:rgba(3,2,1,.0045);color:rgba(2,3,1,.0045);border-radius:0}meter,progress{appearance:auto}",
                "<input value=\"v\"><input type=\"search\"><input type=\"number\" value=\"3\"><input type=\"number\" style=\"height:14px\"><input type=\"button\" value=\"b\"><input type=\"submit\" disabled><button>press</button><button disabled>off</button><select style=\"border-radius:5px\"><option>s</option></select><select disabled style=\"border-radius:5px;width:24px\"><option>t</option></select><textarea>text</textarea><textarea disabled style=\"resize:none\"></textarea><meter value=\"5\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><meter value=\"1\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"5\"></meter><meter value=\"1\" min=\"0\" max=\"10\" low=\"3\" high=\"7\" optimum=\"9\"></meter><progress value=\"2\" max=\"4\"></progress><input type=\"checkbox\" checked><input type=\"radio\"><input type=\"range\" value=\"10\"><input type=\"color\" value=\"#0f0\">",
            ),
        ),
        (
            "grids",
            page(
                "<div style=\"display:grid;grid-template-columns:1fr 2fr;gap:4px;background:#eee;border:2px solid\"><div style=\"background:red;opacity:.5\">a</div><div style=\"position:relative;z-index:-1;background:blue\">b</div><div style=\"grid-column:span 2;box-shadow:0 0 3px\">c</div></div><span style=\"display:inline-grid;grid-template-columns:20px 20px;border:1px dashed\"><b>x</b><i>y</i></span>",
            ),
        ),
        (
            "scroll views",
            page(
                "<div style=\"overflow:auto;height:40px;border:1px solid\"><p>a</p><p>b</p><p>c</p><p>d</p></div><div style=\"overflow:scroll;width:50px\"><div style=\"width:300px\">wide</div></div>",
            ),
        ),
    ];
    // Many layers and groups nested, for the stacks.
    let mut nested = String::new();
    for k in 0..12 {
        nested += &format!(
            "<div style=\"{}\">{k} ",
            match k % 6 {
                0 => "opacity:.9",
                1 => "overflow:hidden;border-radius:4px",
                2 => "transform:translate(1px,1px)",
                3 => "position:relative;z-index:1",
                4 => "filter:blur(1px)",
                _ => "clip-path:inset(1px)",
            }
        );
    }
    nested += &"</div>".repeat(12);
    docs.push(("groups and clips nested twelve deep", page(&nested)));
    docs
}

/// The steps of a written shape: the paints of `paint_steps`, then after a `:hover` restyle and after a media change.
fn shape_steps(width: f32, k: u32) -> Vec<Step> {
    let mut steps = paint_steps(width, 667.0, k);
    steps.extend([Step::Hover(k + 2), Step::Layout(width, None), paint(PaintStep::viewport(width, 667.0)), Step::Dump]);
    steps.extend([Step::Media(900.0, 700.0), Step::Layout(900.0, None), paint(PaintStep::viewport(900.0, 700.0)), Step::Dump]);
    steps
}

#[test]
fn written_paint_shapes_paint_alike() {
    let mut run = Run::new("written paint shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let docs = written_paint_documents();
    for (k, (what, html)) in docs.iter().enumerate() {
        for (sheet, theme) in [(&master, "master"), (&light, "light"), (&dark, "dark")] {
            let html = if theme == "dark" { themed_html(html.as_bytes(), true, &vars) } else { html.clone().into_bytes() };
            for width in [375.0, 160.0] {
                run.case(
                    paint_record(375.0, 667.0, 2.0, sheet, b"", &html, &shape_steps(width, k as u32)),
                    format!("{what} ({theme}, {width} px)"),
                );
            }
        }
    }
    let tally = run.finish();
    assert_eq!(tally.documents, docs.len() * 6);
    for what in [
        "linear gradients",
        "radial gradients",
        "conic gradients",
        "box shadows",
        "inset box shadows",
        "clip paths",
        "filtered paints",
        "backdrops",
        "transformed groups",
        "groups with opacity",
        "clips suspended",
        "paths drawn",
        "list markers drawn",
        "list marker images",
    ] {
        assert!(tally.counts.contains_key(what), "no {what}");
    }
    // A hidden side computes to no width, which no box draws (border_style_hidden is 1).
    for style in (0..10).filter(|&s| s != 1) {
        assert!(tally.counts.contains_key(&format!("border sides of style {style}")), "no border style {style}");
    }
}

// ---- Generated documents -------------------------------------------------------------------------------------

fn pick<'a>(random: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[random.below(from.len())]
}

fn color(random: &mut Lcg) -> &'static str {
    pick(random, &["red", "blue", "rgba(0,128,0,.5)", "transparent", "#123", "currentcolor", "#fff8", "black"])
}

/// A declaration painting reads: backgrounds of every kind, borders, radii, shadows, outlines, opacity, filters,
/// transforms, clip paths, overflow, positions and z-indexes, lists, visibility.
fn paint_declaration(random: &mut Lcg) -> String {
    let len = |random: &mut Lcg| pick(random, &["0", "2px", "5px", "10%", "1em", "-3px", "50%", "20px"]).to_string();
    match random.below(30) {
        0 | 1 => format!("background:{}", color(random)),
        2 => format!(
            "background:url({}.png) {} {}",
            pick(random, &["a", "bb", "photo"]),
            pick(random, &["repeat", "no-repeat", "repeat-x", "repeat-y", "space"]),
            pick(random, &["", "center", "right bottom", "10px 20%", "fixed"])
        ),
        3 => format!("background-size:{}", pick(random, &["cover", "contain", "50%", "10px 30px", "auto 20%"])),
        4 => format!(
            "background-clip:{};background-origin:{}",
            pick(random, &["border-box", "padding-box", "content-box"]),
            pick(random, &["border-box", "padding-box", "content-box"])
        ),
        5 => format!(
            "background-image:{}-gradient({}{},{} {},{})",
            pick(random, &["linear", "repeating-linear", "radial", "repeating-radial", "conic", "repeating-conic"]),
            pick(random, &["", "45deg,", "to right,", "circle,", "from 30deg,", "closest-side,", "at 10% 90%,"]),
            color(random),
            color(random),
            pick(random, &["30%", "10px", "", "50%", "90deg"]),
            color(random)
        ),
        6 | 7 => format!(
            "border:{} {} {}",
            pick(random, &["1px", "3px", "6px", "thin", "0"]),
            pick(random, &["solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "none", "hidden"]),
            color(random)
        ),
        8 => format!("border-radius:{} {}", len(random), len(random)),
        9 => format!(
            "box-shadow:{}{} {} {} {} {}",
            pick(random, &["", "inset "]),
            len(random),
            len(random),
            pick(random, &["0", "4px", "10px"]),
            pick(random, &["0", "2px", "-2px"]),
            color(random)
        ),
        10 => format!(
            "outline:{} {} {};outline-offset:{}",
            pick(random, &["1px", "3px", "0"]),
            pick(random, &["solid", "dashed", "none"]),
            color(random),
            pick(random, &["0", "3px", "-2px"])
        ),
        11 => format!("opacity:{}", pick(random, &[".5", "0", "1", ".99"])),
        12 => format!("filter:{}", pick(random, &["blur(2px)", "grayscale(1)", "drop-shadow(1px 2px 3px red)", "brightness(2) blur(1px)"])),
        13 => format!("backdrop-filter:{}", pick(random, &["blur(3px)", "invert(1)"])),
        14 | 15 => format!(
            "transform:{}",
            pick(
                random,
                &[
                    "rotate(15deg)",
                    "scale(.8)",
                    "translate(5px,-3px)",
                    "skewX(10deg)",
                    "matrix(1,0,0,1,4,4)",
                    "rotate(180deg) scale(2)",
                    "rotateY(80deg)"
                ]
            )
        ),
        16 => format!("transform-origin:{}", pick(random, &["0 0", "100% 50%", "10px 20px", "center"])),
        17 => format!(
            "clip-path:{}",
            pick(random, &["inset(2px round 3px)", "circle(30%)", "ellipse(40% 20%)", "polygon(0 0,100% 20%,50% 100%)"])
        ),
        18 => format!("overflow:{}", pick(random, &["hidden", "auto", "visible", "clip", "scroll"])),
        19 => format!(
            "position:{};z-index:{}",
            pick(random, &["relative", "absolute", "fixed", "sticky"]),
            pick(random, &["-1", "0", "2", "auto", "-3"])
        ),
        20 => format!("top:{};left:{}", len(random), len(random)),
        21 => format!("display:{}", pick(random, &["block", "inline-block", "inline", "list-item", "flex", "none", "grid", "inline-grid"])),
        22 => format!(
            "list-style:{}",
            pick(
                random,
                &["disc", "circle", "square", "decimal", "lower-roman", "upper-alpha", "none", "inside lower-greek", "url(m.png)"]
            )
        ),
        23 => format!("visibility:{}", pick(random, &["hidden", "visible"])),
        24 => format!("float:{}", pick(random, &["left", "right"])),
        25 => format!("width:{};height:{}", pick(random, &["50px", "auto", "80%", "200px"]), pick(random, &["auto", "30px", "100px"])),
        26 => format!("padding:{}", len(random)),
        27 => format!("text-decoration:{}", pick(random, &["underline", "line-through", "overline dotted"])),
        28 => format!("color:{}", color(random)),
        _ => format!("contain:{}", pick(random, &["paint", "layout", "none"])),
    }
}

/// A generated document of boxes, inlines, lists, tables and controls with paint declarations.
fn paint_document(random: &mut Lcg) -> Vec<u8> {
    fn element(random: &mut Lcg, depth: u32, out: &mut String) {
        let tags = [
            "div", "div", "div", "p", "span", "span", "b", "a", "ul", "ol", "li", "table", "img", "input", "button", "select", "textarea",
            "meter", "progress", "fieldset", "svg", "section", "u",
        ];
        let tag = pick(random, &tags);
        out.push('<');
        out.push_str(tag);
        match tag {
            "img" => out.push_str(&format!(" src=\"{}.png\"", pick(random, &["a", "bb", ""]))),
            "input" => out.push_str(pick(
                random,
                &[
                    " type=\"checkbox\" checked",
                    " type=\"radio\"",
                    " type=\"range\" value=\"30\"",
                    " type=\"color\"",
                    " type=\"number\"",
                    "",
                    " type=\"search\"",
                ],
            )),
            "meter" => out.push_str(" value=\"0.4\""),
            "progress" => out.push_str(pick(random, &[" value=\"5\" max=\"10\"", ""])),
            "svg" => out.push_str(" width=\"20\" height=\"10\""),
            _ => {}
        }
        if random.chance(4) {
            out.push_str(&format!(" class=\"{}\"", pick(random, &["a", "b", "a b"])));
        }
        if random.chance(2) {
            let decls: Vec<String> = (0..1 + random.below(4)).map(|_| paint_declaration(random)).collect();
            out.push_str(&format!(" style=\"{}\"", decls.join(";")));
        }
        out.push('>');
        match tag {
            "img" | "input" => return,
            "svg" => {
                out.push_str("<rect width=\"5\" height=\"5\"/></svg>");
                return;
            }
            "table" => {
                for r in 0..1 + random.below(3) {
                    out.push_str("<tr>");
                    for c in 0..1 + random.below(3) {
                        let style = if random.chance(2) { format!(" style=\"{}\"", paint_declaration(random)) } else { String::new() };
                        out.push_str(&format!("<td{style}>{r}{c}</td>"));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</table>");
                return;
            }
            "select" => {
                out.push_str("<option>o</option></select>");
                return;
            }
            "fieldset" => out.push_str("<legend>l</legend>"),
            _ => {}
        }
        let n = if depth > 4 { 0 } else { random.below(4) };
        for _ in 0..n {
            if random.chance(3) {
                out.push_str(pick(random, &["words here ", "a ", "\u{5e9}\u{5dc}\u{5d5}\u{5dd} ", "long text that wraps over lines "]));
            } else {
                element(random, depth + 1, out);
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::from("<!DOCTYPE html><html><head><style>");
    for _ in 0..random.below(4) {
        let selector = pick(random, &["div", "span", ".a", ".b", "p", "li", "td", "body", "html", ".a:hover", "div:hover"]);
        let decls: Vec<String> = (0..1 + random.below(3)).map(|_| paint_declaration(random)).collect();
        let rule = format!("{selector}{{{}}}", decls.join(";"));
        if random.chance(4) {
            out.push_str(&format!("@media (min-width:{}px){{{rule}}}", pick(random, &["300", "500"])));
        } else {
            out.push_str(&rule);
        }
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(4) {
        element(random, 0, &mut out);
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

#[test]
fn generated_documents_paint_alike() {
    let mut run = Run::new("generated");
    let master = master_css();
    let mut random = Lcg::new(0x1D_0001);
    for k in 0..2500u32 {
        let html = paint_document(&mut random);
        let width = [320.0, 375.0, 200.0, 480.0][k as usize % 4];
        let mut steps = paint_steps(width, 600.0, k);
        steps.extend([Step::Hover(k), Step::Layout(width, None), paint(PaintStep::viewport(width, 600.0)), Step::Dump]);
        if k % 5 == 0 {
            steps.extend([Step::Media(600.0, 400.0), Step::Layout(600.0, None), paint(PaintStep::viewport(600.0, 400.0)), Step::Dump]);
        }
        run.case(
            paint_record(width, 600.0, 2.0, &master, b"", &html, &steps),
            format!("generated {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    let tally = run.finish();
    assert!(tally.documents > 2400);
}

/// The switches, each on both sides over generated documents and the written shapes: the layout switches and
/// `LH_NO_TRANSFORMED_CULLING`.
#[test]
fn the_switches_paint_alike() {
    let master = master_css();
    let docs = written_paint_documents();
    for switch in [
        "LH_NO_TRANSFORMED_CULLING",
        "LH_NO_LAYOUT_CACHE",
        "LH_NO_POSITIONED_REUSE",
        "LH_NO_FIXED_LAYOUT_REUSE",
        "LH_LAYOUT_CACHE_CHECK",
        "LH_NO_COLUMN_REUSE",
    ] {
        let mut run = Run::with_switch("switch", switch);
        let mut random = Lcg::new(0x1D_0002);
        for k in 0..150u32 {
            let html = paint_document(&mut random);
            run.case(
                paint_record(320.0, 600.0, 2.0, &master, b"", &html, &paint_steps(320.0, 600.0, k)),
                format!("{switch} {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        for (k, (what, html)) in docs.iter().enumerate() {
            run.case(
                paint_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &shape_steps(375.0, k as u32)),
                format!("{switch}: {what}"),
            );
        }
        run.finish();
    }
}

/// 128 levels of paint groups, overflow clips, positioned boxes, inline boxes, floats and list items paint on a
/// thread whose stack is the size the layout's measures asked (1 MiB unoptimized, 512 KiB optimized).
#[test]
fn a_deep_document_paints_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body>");
        let opens = [
            "<div style=\"opacity:.99\">a ",
            "<div style=\"overflow:hidden;border-radius:2px\">b ",
            "<div style=\"position:relative;z-index:1\">c ",
            "<span style=\"display:inline-block\">d ",
            "<div style=\"float:left\">e ",
            "<div style=\"transform:translate(1px)\">f ",
            "<ul><li>g ",
            "<span>h ",
        ];
        let closes = ["</div>", "</div>", "</div>", "</span>", "</div>", "</div>", "</li></ul>", "</span>"];
        for k in 0..124 {
            html += opens[k % 8];
        }
        html += "<div>deep</div>";
        for k in (0..124).rev() {
            html += closes[k % 8];
        }
        html += "</body></html>";
        let lh = Lh::new();
        let mut tally = PaintTally::default();
        let mut culled = PaintStep::viewport(375.0, 667.0);
        culled.cull = true;
        culled.trace = true;
        let steps = [Step::Layout(375.0, None), paint(PaintStep::viewport(375.0, 667.0)), paint(culled), Step::Dump];
        let input = paint_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &steps);
        let answer = paint_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.calls
    });
    let calls = thread.expect("a thread").join().expect("the paint ran");
    eprintln!("a deep document: {calls} paint calls");
    assert!(calls > 200);
}

// ---- SVG (stage L14) -----------------------------------------------------------------------------------------

/// The paints of an SVG case laid out at `width` in a `height` viewport: the viewport with the container's cache
/// answering false, then as a cache (a key drawn, then found), scrolled and culled, traced, offset by a fraction
/// with each `<svg>`'s `paint_shapes`; after a `:hover` restyle (which changes paint keys, and so lets an `<svg>`'s
/// groups through the cache), again; after a media change, and after the hover left. `k` picks the elements.
fn svg_steps(width: f32, height: f32, k: u32) -> Vec<Step> {
    let view = |cache: bool| {
        let mut step = PaintStep::viewport(width, height);
        step.cache = cache;
        step
    };
    let mut scrolled = view(true);
    scrolled.y = -(height * 0.5).floor();
    scrolled.cull = true;
    scrolled.hdc = 3;
    let mut traced = view(false);
    traced.trace = true;
    traced.cull = true;
    let mut offset = view(true);
    offset.x = 0.25;
    offset.y = 10.5;
    offset.snap = 2.0;
    offset.shapes = true;
    let mut unclipped = view(k.is_multiple_of(2));
    unclipped.clip = None;
    unclipped.shapes = true;
    vec![
        Step::Layout(width, None),
        paint(view(false)),
        Step::Dump,
        paint(view(true)),
        paint(view(true)),
        Step::Dump,
        paint(scrolled),
        paint(traced),
        paint(offset),
        Step::Dump,
        Step::Hover(k),
        Step::Layout(width, None),
        paint(view(true)),
        paint(view(true)),
        Step::Dump,
        Step::Media(width * 2.0, height),
        Step::Layout(width * 2.0, None),
        paint(view(true)),
        paint(unclipped),
        Step::Dump,
        Step::Leave(k),
        Step::Layout(width * 2.0, None),
        paint(view(true)),
        Step::Dump,
    ]
}

/// The corpus's SVGs as svg.rs takes them: each page that writes `<svg` as it is, and every `<svg>…</svg>` the corpus
/// writes (in its pages and in its scripts' strings) alone in a page. The renderer's snapshots are not used: they
/// name every element Gumbo does not know (`path`, `rect`, …) `x-unknown`.
fn corpus_svg_documents() -> Vec<(String, Vec<u8>)> {
    let texts = common::svg::corpus_texts();
    let mut documents: Vec<(String, Vec<u8>)> = texts
        .iter()
        .filter(|(name, text)| {
            name.ends_with(".html")
                && !name.contains("render-nested-")
                && text.len() < 300_000
                && text.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
        })
        .cloned()
        .collect();
    for (name, snippet) in common::svg::corpus_svg_snippets(&texts) {
        let mut html = b"<!doctype html><html><body>".to_vec();
        html.extend_from_slice(&snippet);
        html.extend_from_slice(b"</body></html>");
        documents.push((name, html));
    }
    documents
}

/// The written SVG documents that draw thousands of shapes a paint: painted once (and then from the cache) at a width,
/// and not again under every switch.
fn heavy_svg_document(what: &str) -> bool {
    what.starts_with("the use budget") || what.starts_with("an svg of more than")
}

/// What an SVG run must have reached.
fn assert_svg_reached(tally: &PaintTally, what: &[&str]) {
    for name in what {
        assert!(tally.counts.contains_key(*name), "no {name}: {}", tally.report());
    }
}

#[test]
fn the_corpus_svgs_paint_alike() {
    let mut run = Run::new("corpus SVGs");
    let documents = corpus_svg_documents();
    assert!(documents.len() > 250, "the corpus's SVGs are there ({})", documents.len());
    let light = themed_master(false);
    let dark = themed_master(true);
    for (k, (name, html)) in documents.iter().enumerate() {
        for (dark_theme, (master, vars)) in [(false, &light), (true, &dark)] {
            let html = themed_html(html, dark_theme, vars);
            let (width, height) = if dark_theme { ([412.0, 240.0][k % 2], 915.0) } else { ([375.0, 768.0][k % 2], 667.0) };
            let theme = if dark_theme { "dark" } else { "light" };
            run.case(
                paint_record(width, height, 2.0, master, b"", &html, &svg_steps(width, height, k as u32)),
                format!("{name} ({theme}, {width} px)"),
            );
        }
    }
    let tally = run.finish();
    assert_eq!(tally.documents, documents.len() * 2);
    assert_svg_reached(
        &tally,
        &["paths drawn", "isolated paints", "cached paints drawn", "texts drawn", "groups", "paths stroked", "dashed paths"],
    );
}

/// Written SVG documents for each branch of SVG painting.
fn written_svg_documents() -> Vec<(&'static str, String)> {
    // Every document carries rules that a :hover on any element changes (body is its ancestor): its paint keys move.
    let page = |css: &str, body: &str| {
        format!(
            "<!DOCTYPE html><html><head><style>body:hover svg rect{{stroke-width:3}}body:hover text{{fill:teal}}{css}</style></head><body>{body}</body></html>"
        )
    };
    let mut docs = vec![
        (
            "every shape",
            page(
                "",
                "<svg width=\"200\" height=\"130\"><rect x=\"5\" y=\"5\" width=\"30\" height=\"20\"/><rect x=\"40\" y=\"5\" width=\"30\" height=\"20\" rx=\"5\"/><rect x=\"75\" y=\"5\" width=\"30\" height=\"20\" ry=\"8\"/><rect x=\"110\" y=\"5\" width=\"30\" height=\"20\" rx=\"50\" ry=\"2\"/><rect width=\"0\" height=\"5\"/><rect width=\"5\" height=\"-5\"/><rect x=\"150\" width=\"10\" height=\"10\" rx=\"-2\" ry=\"3\"/><circle cx=\"20\" cy=\"50\" r=\"10\"/><circle cx=\"50\" cy=\"50\" r=\"0\"/><circle cx=\"80\" cy=\"50\" r=\"-3\"/><ellipse cx=\"110\" cy=\"50\" rx=\"15\" ry=\"8\"/><ellipse cx=\"140\" cy=\"50\" rx=\"0\" ry=\"8\"/><line x1=\"0\" y1=\"80\" x2=\"100\" y2=\"90\" stroke=\"black\"/><line x1=\"0\" y1=\"80\" x2=\"100\" y2=\"90\"/><polyline points=\"0,100 20,110 40,100 60,115\" fill=\"none\" stroke=\"blue\"/><polygon points=\"100,100 120,110 110,118\"/><polygon points=\"150 100\"/><polygon points=\"150 100 160\"/><polyline points=\"\"/><path d=\"M150 5 L190 5 L170 40 Z\"/></svg>",
            ),
        ),
        (
            "percent lengths",
            page(
                "",
                "<svg width=\"50%\" height=\"100\"><rect x=\"10%\" y=\"10%\" width=\"50%\" height=\"25%\"/><circle cx=\"50%\" cy=\"50%\" r=\"10%\"/><ellipse rx=\"20%\" ry=\"10%\" cx=\"50%\" cy=\"50%\" fill=\"red\"/><line x1=\"0%\" y1=\"0\" x2=\"100%\" y2=\"100%\" stroke=\"red\"/><rect x=\"5px\" y=\"1em\" width=\"3em\" height=\"10pt\"/></svg><svg viewBox=\"0 0 50 20\"><rect width=\"100%\" height=\"50%\" fill=\"blue\"/></svg>",
            ),
        ),
        (
            "path commands",
            page(
                "",
                "<svg width=\"200\" height=\"140\"><path d=\"M10 10 H50 V30 h-20 v10 L5 45 l10 5 Z\" fill=\"red\"/><path d=\"m60 10 c10 0 20 10 20 20 s-10 20 -20 20 C50 40 40 30 60 10z\"/><path d=\"M100 10 Q120 0 140 10 T180 10 q10 10 0 20 t-20 0\" fill=\"none\" stroke=\"green\"/><path d=\"M10 60 A20 10 0 0 1 50 60 a20 10 30 1 0 40 0 A0 5 0 0 0 100 70 A5 5 0 1 1 100 70.001\"/><path d=\"M0,0 1,1 2,2 3,3\" stroke=\"black\"/><path d=\"M 10 80 L 20 90 30 80 Z M 40 80 L 50 90 Z\"/><path d=\"M1e3 1e-2 L.5.5 L-.5-.5\" stroke=\"red\"/><path d=\"M10 100 L20 110 X 30 120\" stroke=\"blue\"/><path d=\"L10 10\"/><path d=\"\"/><path d=\"M10 120 z m 5 5 z\" stroke=\"red\"/><path d=\"M150 100 a10 10 0 1 1 0.01 0 A 10 10 0 01160 110 a 5 5 0 1020 0\"/><path d=\"M150 60 S 160 50 170 60 T 190 60\" fill=\"none\" stroke=\"black\"/></svg>",
            ),
        ),
        (
            "arcs on edges",
            page(
                "",
                "<svg width=\"200\" height=\"100\"><path d=\"M10 10 A0 0 0 0 0 30 30\" stroke=\"red\"/><path d=\"M10 10 A10 10 0 0 0 10 10\" stroke=\"red\"/><path d=\"M40 10 A1 1 0 1 1 80 10\" stroke=\"red\"/><path d=\"M40 50 A-10 -5 45 0 1 80 50\" stroke=\"red\"/><path d=\"M100 10 A1e39 1e39 0 0 0 110 10\" stroke=\"red\"/><path d=\"M100 40 A10 10 720 1 0 140 40\" stroke=\"red\"/><path d=\"M100 70 A10 10 0 2 0 140 70\" stroke=\"red\"/><path d=\"M150 70 A10 1e-30 0 0 0 190 70\" stroke=\"red\"/></svg>",
            ),
        ),
        (
            "transform attributes",
            page(
                "",
                "<svg width=\"200\" height=\"200\"><g transform=\"translate(10,20) rotate(30) scale(2,1)\"><rect width=\"10\" height=\"10\"/></g><rect transform=\"matrix(1 0.5 0 1 50 0)\" width=\"10\" height=\"10\"/><rect transform=\"rotate(45 60 60)\" width=\"10\" height=\"10\" x=\"55\" y=\"55\"/><rect transform=\"skewX(20) skewY(10)\" x=\"100\" width=\"10\" height=\"10\"/><rect transform=\"translate(5)\" y=\"100\" width=\"10\" height=\"10\"/><rect transform=\"scale(0)\" width=\"10\" height=\"10\"/><rect transform=\"SCALE(2)\" x=\"60\" y=\"60\" width=\"5\" height=\"5\"/><rect transform=\"foo(1) translate(10 10)\" width=\"5\" height=\"5\"/><rect transform=\"rotate(10,20)\" width=\"5\" height=\"5\"/><rect transform=\"translate(10 10\" width=\"5\" height=\"5\"/><rect transform=\"matrix(1 0 0 1 0)\" width=\"5\" height=\"5\"/><g transform=\"translate(0 150)\"><g transform=\"scale(.5)\"><circle r=\"20\" cx=\"20\" cy=\"20\"/></g></g></svg>",
            ),
        ),
        (
            "CSS transforms",
            page(
                ".spin{transform:rotate(45deg)}.grow{transform:scale(1.5);transform-origin:10px 10px}.box{transform:translate(10px,20%);transform-box:fill-box;transform-origin:center}",
                "<svg width=\"200\" height=\"120\"><rect style=\"transform:rotate(20deg);transform-origin:center;transform-box:fill-box\" x=\"20\" y=\"20\" width=\"30\" height=\"10\"/><g style=\"transform:translate(10px,20%)\"><rect width=\"10\" height=\"10\"/></g><circle class=\"spin\" cx=\"80\" cy=\"40\" r=\"10\"/><circle class=\"grow\" cx=\"120\" cy=\"40\" r=\"10\"/><rect class=\"box\" x=\"140\" y=\"10\" width=\"20\" height=\"40\"/><rect style=\"transform:rotateY(80deg) skew(10deg,5deg)\" x=\"10\" y=\"80\" width=\"20\" height=\"20\"/><rect style=\"transform:matrix(1,0,0,1,5,5)\" transform=\"scale(3)\" x=\"60\" y=\"80\" width=\"5\" height=\"5\"/></svg>",
            ),
        ),
        (
            "view boxes and aspect ratios",
            page(
                "svg{border:1px solid}",
                "<svg width=\"100\" height=\"50\" viewBox=\"0 0 10 10\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"100\" height=\"50\" viewBox=\"0 0 10 10\" preserveAspectRatio=\"none\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"100\" height=\"50\" viewBox=\"0 0 10 10\" preserveAspectRatio=\"xMinYMin meet\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"100\" height=\"50\" viewBox=\"0 0 10 10\" preserveAspectRatio=\"xMidYMid slice\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"50\" height=\"100\" viewBox=\"0 0 10 10\" preserveAspectRatio=\"xMaxYMax\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"50\" height=\"100\" viewBox=\"-5 -5 10 10\" preserveAspectRatio=\"xMinYMax slice\"><circle r=\"5\"/></svg><svg width=\"50\" height=\"100\" viewBox=\"0 0 10 10\" preserveAspectRatio=\"XMAXYMIN\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"50\" height=\"30\" viewBox=\"0 0 0 10\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg width=\"50\" height=\"30\" viewBox=\"0 0 10\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg><svg viewBox=\"0 0 30 10\"><rect width=\"30\" height=\"10\"/></svg><svg height=\"40\" viewBox=\"0,0,20,10\" preserveAspectRatio=\"foo\"><rect width=\"20\" height=\"10\"/></svg>",
            ),
        ),
        (
            "svgs sized by CSS",
            page(
                "",
                "<svg style=\"width:100px;height:50px\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\"/></svg><svg style=\"display:block;width:50%\"><rect width=\"100%\" height=\"10\"/></svg><svg width=\"0\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg><svg width=\"100\" height=\"0\"><rect width=\"10\" height=\"10\"/></svg><svg><rect width=\"10\" height=\"10\"/></svg><svg width=\"30\" height=\"30\" style=\"padding:5px;border:3px solid red;background:yellow;box-shadow:0 0 4px;border-radius:6px\"><rect width=\"100\" height=\"100\"/></svg>",
            ),
        ),
        (
            "fills",
            page(
                "rect.c{fill:orange}",
                "<svg width=\"300\" height=\"40\" style=\"color:green\"><rect width=\"9\" height=\"9\" fill=\"red\"/><rect x=\"10\" width=\"9\" height=\"9\" fill=\"#0f08\"/><rect x=\"20\" width=\"9\" height=\"9\" fill=\"rgb(1,2,3)\"/><rect x=\"30\" width=\"9\" height=\"9\" fill=\"hsl(120,50%,50%)\"/><rect x=\"40\" width=\"9\" height=\"9\" fill=\"none\"/><rect x=\"50\" width=\"9\" height=\"9\" fill=\"transparent\"/><rect x=\"60\" width=\"9\" height=\"9\" fill=\"currentColor\"/><rect x=\"70\" width=\"9\" height=\"9\" fill=\"inherit\"/><rect x=\"80\" width=\"9\" height=\"9\" fill=\"url(#none) red\"/><rect x=\"90\" width=\"9\" height=\"9\" fill=\"url(#missing)\"/><rect x=\"100\" width=\"9\" height=\"9\" fill=\"url(#missing) none\"/><rect x=\"110\" width=\"9\" height=\"9\" fill=\"\"/><rect x=\"120\" width=\"9\" height=\"9\" fill=\"  \"/><rect x=\"130\" width=\"9\" height=\"9\" fill=\"red blue\"/><rect x=\"140\" width=\"9\" height=\"9\" fill=\"rgba(0,0,0,.5)\"/><rect x=\"150\" width=\"9\" height=\"9\" fill=\"color-mix(in srgb, red, blue)\"/><rect x=\"160\" width=\"9\" height=\"9\" fill=\"light-dark(red, blue)\"/><rect x=\"170\" width=\"9\" height=\"9\" fill=\"calc(1)\"/><rect x=\"180\" width=\"9\" height=\"9\" fill=\"red\" style=\"fill:blue\"/><rect x=\"190\" width=\"9\" height=\"9\" class=\"c\" fill=\"red\"/><g fill=\"purple\"><rect x=\"200\" width=\"9\" height=\"9\"/><g><rect x=\"210\" width=\"9\" height=\"9\"/></g></g><rect x=\"220\" width=\"9\" height=\"9\" fill=\"CurrentColor\" style=\"color:red\"/><rect x=\"230\" width=\"9\" height=\"9\" fill=\"#zzz\"/><rect x=\"240\" width=\"9\" height=\"9\" fill=\"NONE\"/><rect x=\"250\" width=\"9\" height=\"9\" fill=\"rgb(0 0 0 / 30%)\"/><rect x=\"260\" width=\"9\" height=\"9\" fill=\"url(#none)\" stroke=\"red\"/></svg>",
            ),
        ),
        (
            "strokes",
            page(
                "line.k{stroke:blue;stroke-width:4}",
                "<svg width=\"300\" height=\"100\"><line x1=\"5\" y1=\"5\" x2=\"50\" y2=\"5\" stroke=\"red\" stroke-width=\"0\"/><line x1=\"5\" y1=\"15\" x2=\"50\" y2=\"15\" stroke=\"red\" stroke-width=\"2\"/><line x1=\"5\" y1=\"25\" x2=\"50\" y2=\"25\" stroke=\"red\" stroke-width=\"2px\"/><line x1=\"5\" y1=\"35\" x2=\"50\" y2=\"35\" stroke=\"red\" stroke-width=\"50%\"/><line x1=\"5\" y1=\"45\" x2=\"50\" y2=\"45\" stroke=\"red\" stroke-width=\"-1\"/><line x1=\"5\" y1=\"55\" x2=\"50\" y2=\"55\" stroke=\"red\" stroke-width=\"1em\"/><line x1=\"60\" y1=\"5\" x2=\"100\" y2=\"5\" stroke=\"red\" stroke-linecap=\"round\"/><line x1=\"60\" y1=\"15\" x2=\"100\" y2=\"15\" stroke=\"red\" stroke-linecap=\"square\"/><line x1=\"60\" y1=\"25\" x2=\"100\" y2=\"25\" stroke=\"red\" stroke-linecap=\"ROUND\"/><line x1=\"60\" y1=\"35\" x2=\"100\" y2=\"35\" stroke=\"red\" stroke-linecap=\"butt\"/><polyline points=\"110,5 130,30 150,5\" fill=\"none\" stroke=\"black\" stroke-width=\"5\" stroke-linejoin=\"miter\" stroke-miterlimit=\"1\"/><polyline points=\"160,5 180,30 200,5\" fill=\"none\" stroke=\"black\" stroke-width=\"5\" stroke-linejoin=\"round\"/><polyline points=\"210,5 230,30 250,5\" fill=\"none\" stroke=\"black\" stroke-width=\"5\" stroke-linejoin=\"bevel\"/><polyline points=\"110,50 130,80 150,50\" fill=\"none\" stroke=\"black\" stroke-linejoin=\"arcs\" stroke-miterlimit=\"-3\"/><rect x=\"160\" y=\"50\" width=\"20\" height=\"20\" stroke=\"none\"/><rect x=\"190\" y=\"50\" width=\"20\" height=\"20\" stroke=\"green\" stroke-opacity=\".3\"/><line class=\"k\" x1=\"5\" y1=\"80\" x2=\"100\" y2=\"80\"/><g stroke=\"orange\" stroke-width=\"3\"><circle cx=\"250\" cy=\"60\" r=\"10\"/></g></svg>",
            ),
        ),
        (
            "dashes",
            page(
                "",
                "<svg width=\"200\" height=\"150\"><g stroke=\"black\" stroke-width=\"2\"><line x1=\"5\" y1=\"5\" x2=\"190\" y2=\"5\" stroke-dasharray=\"5\"/><line x1=\"5\" y1=\"15\" x2=\"190\" y2=\"15\" stroke-dasharray=\"5 3\"/><line x1=\"5\" y1=\"25\" x2=\"190\" y2=\"25\" stroke-dasharray=\"5,3,2\"/><line x1=\"5\" y1=\"35\" x2=\"190\" y2=\"35\" stroke-dasharray=\"0\"/><line x1=\"5\" y1=\"45\" x2=\"190\" y2=\"45\" stroke-dasharray=\"-5 3\"/><line x1=\"5\" y1=\"55\" x2=\"190\" y2=\"55\" stroke-dasharray=\"5px 2em\"/><line x1=\"5\" y1=\"65\" x2=\"190\" y2=\"65\" stroke-dasharray=\"10%\"/><line x1=\"5\" y1=\"75\" x2=\"190\" y2=\"75\" stroke-dasharray=\"none\"/><line x1=\"5\" y1=\"85\" x2=\"190\" y2=\"85\" stroke-dasharray=\"\"/><line x1=\"5\" y1=\"95\" x2=\"190\" y2=\"95\" stroke-dasharray=\"4 2\" stroke-dashoffset=\"3\"/><line x1=\"5\" y1=\"105\" x2=\"190\" y2=\"105\" stroke-dasharray=\"4 2\" stroke-dashoffset=\"-2\"/><line x1=\"5\" y1=\"115\" x2=\"190\" y2=\"115\" stroke-dasharray=\"4 2\" stroke-dashoffset=\"50%\"/><g stroke-dasharray=\"1 1 1\"><line x1=\"5\" y1=\"125\" x2=\"190\" y2=\"125\"/></g><line x1=\"5\" y1=\"135\" x2=\"190\" y2=\"135\" style=\"stroke-dasharray:6 2\"/></g></svg>",
            ),
        ),
        (
            "fill rules",
            page(
                ".eo{fill-rule:evenodd}",
                "<svg width=\"200\" height=\"60\"><path d=\"M10 10 h40 v40 h-40 z M20 20 h20 v20 h-20 z\" fill-rule=\"evenodd\"/><path d=\"M60 10 h40 v40 h-40 z M70 20 h20 v20 h-20 z\" fill-rule=\"nonzero\"/><path d=\"M110 10 h40 v40 h-40 z M120 20 h20 v20 h-20 z\" fill-rule=\"EVENODD\"/><path class=\"eo\" d=\"M160 10 h30 v30 h-30 z\"/><g fill-rule=\"evenodd\"><path d=\"M0 0 h5 v5 z\"/></g></svg>",
            ),
        ),
        (
            "opacity at each level",
            page(
                ".half{opacity:.5}",
                "<svg width=\"300\" height=\"60\" style=\"opacity:.8\"><g opacity=\".5\"><rect width=\"20\" height=\"20\"/></g><g class=\"half\"><rect x=\"25\" width=\"20\" height=\"20\" opacity=\".5\"/></g><rect x=\"50\" width=\"20\" height=\"20\" opacity=\"2\"/><rect x=\"75\" width=\"20\" height=\"20\" opacity=\"-1\"/><rect x=\"100\" width=\"20\" height=\"20\" opacity=\"50%\"/><rect x=\"125\" width=\"20\" height=\"20\" opacity=\"abc\"/><rect x=\"150\" width=\"20\" height=\"20\" fill-opacity=\".5\"/><rect x=\"175\" width=\"20\" height=\"20\" fill-opacity=\"50%\" stroke=\"red\" stroke-opacity=\"0.25\"/><rect x=\"200\" width=\"20\" height=\"20\" fill-opacity=\"2\"/><rect x=\"225\" width=\"20\" height=\"20\" fill-opacity=\"-1\" stroke=\"red\" stroke-opacity=\"x\"/><g opacity=\".5\"><g opacity=\".5\" fill-opacity=\".5\"><circle cx=\"260\" cy=\"10\" r=\"8\"/></g></g><rect x=\"0\" y=\"30\" width=\"20\" height=\"20\" style=\"opacity:.3\" opacity=\".5\"/><rect x=\"25\" y=\"30\" width=\"20\" height=\"20\" opacity=\"1e39\"/><rect x=\"50\" y=\"30\" width=\"20\" height=\"20\" opacity=\" .7 \"/></svg>",
            ),
        ),
        (
            "gradients",
            page(
                "",
                "<svg width=\"300\" height=\"200\" style=\"color:navy\"><defs><linearGradient id=\"g1\"><stop offset=\"0\" stop-color=\"red\"/><stop offset=\"100%\" stop-color=\"blue\"/></linearGradient><linearGradient id=\"g2\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"><stop offset=\"20%\" stop-color=\"red\" stop-opacity=\".5\"/><stop offset=\"10%\" style=\"stop-color:green\"/><stop offset=\"1.5\" stop-color=\"currentColor\"/><stop stop-color=\"x\"/><stop offset=\"-1\" stop-color=\"#0f0\" style=\"stop-opacity:30%\"/></linearGradient><linearGradient id=\"g3\" gradientUnits=\"userSpaceOnUse\" x1=\"10\" y1=\"0\" x2=\"90\" y2=\"0\" spreadMethod=\"reflect\" gradientTransform=\"rotate(30)\"><stop offset=\".5\" stop-color=\"orange\"/></linearGradient><linearGradient id=\"g4\" gradientUnits=\"userspaceonuse\"><stop offset=\"0\" stop-color=\"red\"/></linearGradient><linearGradient id=\"empty\"></linearGradient><radialGradient id=\"r1\"><stop offset=\"0\" stop-color=\"white\"/><stop offset=\"1\" stop-color=\"black\"/></radialGradient><radialGradient id=\"r2\" cx=\"30%\" cy=\"40%\" r=\"70%\" fx=\"10%\" fy=\"20%\" spreadMethod=\"repeat\"><stop offset=\"0\" stop-color=\"yellow\"/><stop offset=\".5\" stop-color=\"green\" stop-opacity=\"0\"/></radialGradient><radialGradient id=\"r3\" gradientUnits=\"userSpaceOnUse\" cx=\"200\" cy=\"100\" r=\"30\"><stop offset=\"0\" stop-color=\"pink\"/><stop offset=\"1\" stop-color=\"purple\"/></radialGradient><linearGradient id=\"chain\" xlink:href=\"#g1\"></linearGradient></defs><rect width=\"40\" height=\"40\" fill=\"url(#g1)\"/><rect x=\"45\" width=\"40\" height=\"40\" fill=\"url(#g2)\" fill-opacity=\".5\"/><rect x=\"90\" width=\"40\" height=\"40\" fill=\"url(#g3)\"/><rect x=\"135\" width=\"40\" height=\"40\" fill=\"url(#g4)\"/><rect x=\"180\" width=\"40\" height=\"40\" fill=\"url(#empty)\"/><rect x=\"225\" width=\"40\" height=\"40\" fill=\"url(#empty) red\"/><circle cx=\"20\" cy=\"70\" r=\"20\" fill=\"url(#r1)\"/><circle cx=\"70\" cy=\"70\" r=\"20\" fill=\"url(#r2)\"/><circle cx=\"200\" cy=\"100\" r=\"30\" fill=\"url(#r3)\"/><rect x=\"10\" y=\"100\" width=\"60\" height=\"20\" fill=\"none\" stroke=\"url(#g1)\" stroke-width=\"4\" stroke-opacity=\".5\"/><line x1=\"10\" y1=\"130\" x2=\"100\" y2=\"130\" stroke=\"url(#g1)\" stroke-width=\"5\"/><rect x=\"110\" y=\"100\" width=\"30\" height=\"30\" fill=\"url('#g1')\"/><rect x=\"150\" y=\"100\" width=\"30\" height=\"30\" style=\"fill:url(#r1)\"/><rect x=\"10\" y=\"140\" width=\"30\" height=\"30\" fill=\"url(#chain)\"/><rect x=\"50\" y=\"140\" width=\"30\" height=\"30\" fill=\"url(#rect1)\"/><rect id=\"rect1\" x=\"90\" y=\"140\" width=\"30\" height=\"30\" fill=\"url(#g1) red\" stroke=\"url(#r2) blue\"/><text x=\"130\" y=\"160\" fill=\"url(#g1)\">gradient text</text><path d=\"M250 150 L250 190\" stroke=\"url(#g1)\" stroke-width=\"8\"/><circle cx=\"280\" cy=\"170\" r=\"12\" fill=\"none\" stroke=\"url(#r1)\" stroke-width=\"6\"/><circle cx=\"280\" cy=\"130\" r=\"12\" stroke=\"url(#r3)\"/></svg>",
            ),
        ),
        (
            "text and tspans",
            page(
                "text{font-size:14px}.big{font-size:20px}",
                "<svg width=\"300\" height=\"200\"><text x=\"10\" y=\"20\">Hello <tspan fill=\"red\">red</tspan> world</text><text x=\"150\" y=\"20\" text-anchor=\"middle\">middle</text><text x=\"290\" y=\"20\" text-anchor=\"end\">end</text><text x=\"10\" y=\"40\" style=\"text-anchor:middle\">styled</text><text x=\"10\" y=\"60\" dx=\"5\" dy=\"1em\">shifted <tspan dx=\"2em\" dy=\"-0.5em\" class=\"big\">big</tspan><tspan x=\"200\" y=\"60\">placed</tspan> after</text><text x=\"10 20 30\" y=\"100\">list</text><text y=\"120\">  lots   of   space  </text><text x=\"10\" y=\"140\"><tspan>nested <tspan fill=\"blue\">deep <tspan style=\"display:none\">hidden</tspan>end</tspan></tspan></text><text x=\"10\" y=\"160\"></text><text x=\"10\" y=\"160\">   </text><text x=\"10\" y=\"180\" fill=\"none\">no fill</text><text x=\"100\" y=\"180\" fill-opacity=\".5\" fill=\"green\">half</text><g fill=\"orange\" text-anchor=\"end\"><text x=\"290\" y=\"180\">group <a href=\"#x\">link</a> <tspan fill=\"none\">none</tspan>done</text></g><text x=\"10\" y=\"195\" style=\"font-size:0\">zero</text><text x=\"200\" y=\"100\" visibility=\"hidden\">hidden</text><text x=\"200\" y=\"120\" dx=\"x\" dy=\"3px\">bad shift</text></svg>",
            ),
        ),
        (
            "use",
            page(
                "",
                "<svg width=\"300\" height=\"120\"><defs><rect id=\"r\" width=\"10\" height=\"10\"/><symbol id=\"s\" viewBox=\"0 0 10 10\"><circle cx=\"5\" cy=\"5\" r=\"5\"/></symbol><symbol id=\"s0\"><rect width=\"4\" height=\"4\"/></symbol><g id=\"grp\"><rect width=\"5\" height=\"5\"/><use href=\"#r\" x=\"10\"/></g><text id=\"t\" y=\"10\">used</text></defs><use href=\"#r\" x=\"5\" y=\"5\"/><use xlink:href=\"#s\" x=\"20\" width=\"20\" height=\"40\"/><use href=\"#s\" x=\"50\"/><use href=\"#s0\" x=\"80\"/><use href=\"#grp\" x=\"100\"/><use href=\"#missing\"/><use href=\"r\"/><use href=\"#\"/><use href=\"#1a\"/><use href=\"\" xlink:href=\"#r\" x=\"120\"/><use href=\"#self\" id=\"self\"/><use href=\"#r\" x=\"140\" fill=\"red\"/><use href=\"#r\" x=\"160\" style=\"fill:blue\" stroke=\"black\"/><use href=\"#t\" x=\"180\" fill=\"green\" text-anchor=\"middle\"/><use href=\"#r\" x=\"200\" visibility=\"hidden\"/><use href=\"#r\" x=\"220\" opacity=\".5\" transform=\"rotate(10)\"/><use href=\"#r\" x=\"50%\" y=\"50%\"/><use href=\"#s\" x=\"10\" y=\"60\" width=\"50%\" height=\"0\"/><g id=\"c1\"><use href=\"#c2\" x=\"1\"/></g><g id=\"c2\"><use href=\"#c1\" x=\"1\"/></g></svg>",
            ),
        ),
        (
            "use chains deeper than eight",
            page(
                "",
                "<svg width=\"200\" height=\"50\"><defs><rect id=\"u0\" width=\"3\" height=\"3\" fill=\"red\"/><use id=\"u1\" href=\"#u0\" x=\"2\"/><use id=\"u2\" href=\"#u1\" x=\"2\"/><use id=\"u3\" href=\"#u2\" x=\"2\"/><use id=\"u4\" href=\"#u3\" x=\"2\"/><use id=\"u5\" href=\"#u4\" x=\"2\"/><use id=\"u6\" href=\"#u5\" x=\"2\"/><use id=\"u7\" href=\"#u6\" x=\"2\"/><use id=\"u8\" href=\"#u7\" x=\"2\"/><use id=\"u9\" href=\"#u8\" x=\"2\"/><use id=\"u10\" href=\"#u9\" x=\"2\"/></defs><use href=\"#u7\"/><use href=\"#u8\" y=\"10\"/><use href=\"#u9\" y=\"20\"/><use href=\"#u10\" y=\"30\"/></svg>",
            ),
        ),
        (
            "nested svgs",
            page(
                "",
                "<svg width=\"200\" height=\"100\"><svg x=\"10\" y=\"10\" width=\"20\" height=\"20\" viewBox=\"0 0 100 100\"><rect width=\"100\" height=\"100\" fill=\"red\"/><svg><circle cx=\"5\" cy=\"5\" r=\"5\"/></svg></svg><switch><rect x=\"50\" width=\"10\" height=\"10\"/><circle cx=\"80\" cy=\"10\" r=\"5\"/></switch><a href=\"#\"><rect x=\"100\" width=\"10\" height=\"10\"/></a><foreignObject width=\"50\" height=\"50\"><div>html</div></foreignObject><image href=\"a.png\" width=\"10\" height=\"10\"/><unknown><rect y=\"50\" width=\"5\" height=\"5\"/></unknown></svg>",
            ),
        ),
        (
            "clip paths and masks",
            page(
                ".clipped{clip-path:circle(40%)}",
                "<svg width=\"200\" height=\"100\" class=\"clipped\"><defs><clipPath id=\"c\"><rect width=\"10\" height=\"10\"/></clipPath><mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask><filter id=\"f\"><feGaussianBlur stdDeviation=\"2\"/></filter><marker id=\"mk\"><path d=\"M0 0 L5 5\"/></marker><pattern id=\"pt\" width=\"4\" height=\"4\"><rect width=\"2\" height=\"2\"/></pattern></defs><rect clip-path=\"url(#c)\" width=\"50\" height=\"50\"/><g mask=\"url(#m)\"><circle cx=\"70\" cy=\"30\" r=\"20\"/></g><rect x=\"100\" width=\"30\" height=\"30\" filter=\"url(#f)\"/><path d=\"M140 10 L180 40\" stroke=\"black\" marker-start=\"url('#mk')\" marker-end=\"url(#mk )\"/><rect x=\"140\" y=\"50\" width=\"30\" height=\"30\" fill=\"url(#pt)\"/><clipPath><rect width=\"5\" height=\"5\"/></clipPath></svg><svg width=\"50\" height=\"50\"><rect width=\"20\" height=\"20\" clip-path=\"url(#elsewhere)\"/></svg>",
            ),
        ),
        (
            "currentColor and var()",
            page(
                ":root{--root:pink}",
                "<svg width=\"300\" height=\"40\" style=\"color:green;--c:blue;--w:3\"><rect width=\"9\" height=\"9\" fill=\"currentColor\"/><rect x=\"10\" width=\"9\" height=\"9\" fill=\"var(--c)\"/><rect x=\"20\" width=\"9\" height=\"9\" style=\"fill:var(--c)\"/><rect x=\"30\" width=\"9\" height=\"9\" fill=\"var(--missing)\"/><rect x=\"40\" width=\"9\" height=\"9\" fill=\"var(--missing, red)\"/><rect x=\"50\" width=\"9\" height=\"9\" stroke=\"var(--c)\" stroke-width=\"var(--w)\"/><rect x=\"60\" width=\"9\" height=\"9\" fill=\"var(--root)\"/><g style=\"color:red\" fill=\"currentColor\"><rect x=\"70\" width=\"9\" height=\"9\"/><rect x=\"80\" width=\"9\" height=\"9\" style=\"color:orange\"/></g><rect x=\"90\" width=\"9\" height=\"9\" fill=\"var(--c\"/><rect x=\"100\" width=\"9\" height=\"9\" style=\"--c:yellow\" fill=\"var(--c)\"/><text x=\"120\" y=\"20\" fill=\"var(--c)\" style=\"color:red\">var</text><rect x=\"200\" width=\"9\" height=\"9\" fill-opacity=\"var(--o, .5)\"/><rect x=\"210\" width=\"9\" height=\"9\" fill=\"calc(var(--w) * 1px)\"/></svg>",
            ),
        ),
        (
            "NaN and degenerate geometry",
            page(
                "",
                "<svg width=\"100\" height=\"100\"><rect width=\"1e39\" height=\"10\"/><circle r=\"1e39\"/><rect x=\"1e39\" width=\"10\" height=\"10\" rx=\"1e39\"/><path d=\"M 1e39 0 L -1e39 0 L 0 1e39 Z\" stroke=\"red\"/><path d=\"M0 0 A1e39 5 0 0 0 1e39 1e39\" stroke=\"red\"/><g transform=\"scale(1e39)\"><rect width=\"1\" height=\"1\"/></g><g transform=\"rotate(1e39)\"><rect width=\"1\" height=\"1\"/></g><rect width=\"1e-45\" height=\"1e-45\"/><line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"0\" stroke=\"black\"/><polygon points=\"1e39,1e39 -1e39,0 0,0\"/><rect style=\"transform:scale(0)\" width=\"10\" height=\"10\"/><rect x=\"-1e39\" width=\"1e39\" height=\"10\" rx=\"3\" stroke=\"red\"/><ellipse cx=\"-1e39\" rx=\"1e39\" ry=\"5\" fill=\"url(#gn)\"/><linearGradient id=\"gn\"><stop stop-color=\"red\"/></linearGradient></svg><svg width=\"50\" height=\"50\" viewBox=\"0 0 1e39 1\"><rect width=\"10\" height=\"10\"/></svg><svg width=\"50\" height=\"50\" viewBox=\"0 0 1e-45 1e-45\"><rect width=\"10\" height=\"10\"/></svg><svg width=\"50\" height=\"50\" viewBox=\"-1e39 0 10 10\"><rect width=\"10\" height=\"10\"/></svg>",
            ),
        ),
        (
            "visibility and display",
            page(
                ".gone{display:none}.inv{visibility:hidden}",
                "<svg width=\"300\" height=\"40\"><g visibility=\"hidden\"><rect width=\"9\" height=\"9\"/><rect x=\"10\" width=\"9\" height=\"9\" visibility=\"visible\"/></g><g class=\"inv\"><rect x=\"20\" width=\"9\" height=\"9\"/><rect x=\"30\" width=\"9\" height=\"9\" style=\"visibility:visible\"/><rect x=\"40\" width=\"9\" height=\"9\" visibility=\" Visible \"/></g><g display=\"none\"><rect x=\"50\" width=\"9\" height=\"9\"/></g><rect x=\"60\" width=\"9\" height=\"9\" class=\"gone\"/><rect x=\"70\" width=\"9\" height=\"9\" visibility=\"collapse\"/><rect x=\"80\" width=\"9\" height=\"9\" display=\" NONE \"/><rect x=\"90\" width=\"9\" height=\"9\" display=\"inline\"/><rect x=\"100\" width=\"9\" height=\"9\" visibility=\"bogus\" class=\"inv\"/><g visibility=\"hidden\"><g><rect x=\"110\" width=\"9\" height=\"9\"/></g><use href=\"#vr\"/></g><rect id=\"vr\" x=\"120\" width=\"9\" height=\"9\"/><text x=\"130\" y=\"9\" class=\"inv\">x</text><text x=\"140\" y=\"9\" visibility=\"hidden\"><tspan visibility=\"visible\">v</tspan></text></svg>",
            ),
        ),
        (
            "elements never drawn",
            page(
                "",
                "<svg width=\"100\" height=\"40\"><defs><rect width=\"9\" height=\"9\"/></defs><title>t</title><desc>d</desc><symbol><rect width=\"9\" height=\"9\"/></symbol><marker><rect width=\"9\" height=\"9\"/></marker><pattern><rect width=\"9\" height=\"9\"/></pattern><filter><rect width=\"9\" height=\"9\"/></filter><metadata>m</metadata><style>rect{}</style><script>1</script><linearGradient><stop/></linearGradient><mask><rect width=\"9\" height=\"9\"/></mask><!-- a comment --><![CDATA[cdata]]><rect x=\"50\" width=\"9\" height=\"9\"/></svg>",
            ),
        ),
        (
            "paint keys and their references",
            page(
                "",
                "<svg width=\"100\" height=\"40\"><defs><linearGradient id=\"in\"><stop stop-color=\"red\"/></linearGradient></defs><rect width=\"9\" height=\"9\" fill=\"url(#in)\"/><rect x=\"10\" width=\"9\" height=\"9\" style=\"fill:url(#in)\"/><rect x=\"20\" width=\"9\" height=\"9\" filter=\"url(#in) url(#in2)\"/></svg><svg width=\"100\" height=\"40\"><rect width=\"9\" height=\"9\" fill=\"url(#in)\"/></svg><svg width=\"100\" height=\"40\"><rect width=\"9\" height=\"9\" mask=\"url(&quot;#nothing&quot;)\"/><rect x=\"10\" width=\"9\" height=\"9\" style=\"x:url(in)\"/></svg><svg width=\"100\" height=\"40\"><g id=\"inside\"><rect width=\"9\" height=\"9\" marker-mid=\"url(#inside) url(#inside\"/></g></svg><svg width=\"100\" height=\"40\"><use href=\"#inside\"/></svg><svg width=\"100\" height=\"40\"><image href=\"a.png\"/><rect width=\"9\" height=\"9\"/></svg><svg width=\"100\" height=\"40\" fill=\"url(#in)\"><rect width=\"9\" height=\"9\"/></svg>",
            ),
        ),
        ("use budget shared by svgs", {
            let mut fan = String::from("<defs><rect id=\"f0\" width=\"1\" height=\"1\"/>");
            for level in 1..=6 {
                fan += &format!("<g id=\"f{level}\">");
                for k in 0..6 {
                    fan += &format!("<use href=\"#f{}\" x=\"{k}\"/>", level - 1);
                }
                fan += "</g>";
            }
            fan += "</defs>";
            page(
                "",
                &format!(
                    "<svg width=\"100\" height=\"30\">{fan}<use href=\"#f5\"/></svg><svg width=\"100\" height=\"30\"><use href=\"#f4\"/><use href=\"#f2\" y=\"5\"/></svg><svg width=\"100\" height=\"30\"><use href=\"#f6\"/></svg><svg width=\"100\" height=\"30\"><use href=\"#f1\"/></svg>"
                ),
            )
        }),
        (
            "svgs in clips and groups",
            page(
                ".o{opacity:.5}.t{transform:rotate(10deg)}.f{filter:blur(1px)}.h{overflow:hidden;width:30px;height:20px}",
                "<div class=\"h\"><svg width=\"60\" height=\"60\"><rect width=\"60\" height=\"60\"/></svg></div><div class=\"o\"><svg width=\"30\" height=\"30\"><circle cx=\"15\" cy=\"15\" r=\"15\"/></svg></div><div class=\"t\"><svg width=\"30\" height=\"30\"><rect width=\"30\" height=\"30\" fill=\"red\"/></svg></div><div class=\"f\"><svg width=\"30\" height=\"30\"><rect width=\"30\" height=\"30\" fill=\"blue\"/></svg></div><div style=\"margin-top:2000px\"><svg width=\"30\" height=\"30\"><rect width=\"30\" height=\"30\"/></svg></div><p>text <svg width=\"12\" height=\"12\" style=\"vertical-align:middle\"><circle cx=\"6\" cy=\"6\" r=\"6\"/></svg> inline</p><div style=\"position:fixed;top:0;right:0\"><svg width=\"20\" height=\"20\"><rect width=\"20\" height=\"20\"/></svg></div>",
            ),
        ),
    ];
    // The group cache: top-level groups of eight elements and more, through draw_cached once the paint key moves.
    let mut cached_groups = String::from("<svg width=\"300\" height=\"100\">");
    for g in 0..4 {
        cached_groups += &format!("<g{}>", ["", " opacity=\".5\"", " id=\"g2\"", " transform=\"translate(1 1)\""][g]);
        for k in 0..6 {
            cached_groups += &format!("<rect x=\"{}\" y=\"{}\" width=\"8\" height=\"8\" fill=\"#{k}{g}0\"/>", k * 10, g * 20);
        }
        cached_groups += "<text x=\"70\" y=\"10\">group</text></g>";
    }
    cached_groups += "<g><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/></g>";
    cached_groups += "<g><rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\" fill=\"url(#x)\"/> </g>";
    cached_groups += "<g><rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\" fill=\"var(--v)\"/> </g>";
    cached_groups +=
        "<g><rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <use href=\"#g2\"/> </g>";
    cached_groups += "<g><rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\" style=\"fill:var(--v)\"/> </g>";
    cached_groups += "<g><g><rect width=\"1\" height=\"1\"/> <rect width=\"1\" height=\"1\"/></g> <rect width=\"1\" height=\"1\"/> <circle r=\"1\"/> </g></svg>";
    docs.push(("cached groups", page("", &cached_groups)));
    let eight = |extra: &str| format!("<g>{}{extra}</g>", "<rect width=\"1\" height=\"1\"/>".repeat(7));
    docs.push((
        "groups the cache refuses",
        page(
            "rect.u{fill:url(#ga)}",
            &format!(
                "<svg width=\"50\" height=\"20\"><defs><linearGradient id=\"ga\"><stop stop-color=\"red\"/></linearGradient></defs>{}</svg><svg width=\"50\" height=\"20\">{}</svg><svg width=\"50\" height=\"20\">{}</svg><div style=\"fill:url(#gc)\"><svg width=\"50\" height=\"20\"><linearGradient id=\"gc\"><stop stop-color=\"red\"/></linearGradient>{}</svg></div>",
                eight("<rect class=\"u\" width=\"1\" height=\"1\"/>"),
                eight("<rect fill=\"var(--v)\" width=\"1\" height=\"1\"/>"),
                eight("<rect stroke=\"VAR(--v, red)\" width=\"1\" height=\"1\"/>"),
                eight("<rect width=\"1\" height=\"1\"/>")
            ),
        ),
    ));
    docs.push((
        "gradient children that are no stops, and empty shifts",
        page(
            "",
            "<svg width=\"100\" height=\"40\"><linearGradient id=\"gw\"> <stop offset=\"0\" stop-color=\"red\"/> <!-- c --> <rect width=\"9\" height=\"9\"/> <stop offset=\"1\" stop-color=\"blue\"/> </linearGradient><rect width=\"30\" height=\"30\" fill=\"url(#gw)\"/><text x=\"40\" y=\"20\" dx=\"\" dy=\"\">e</text></svg>",
        ),
    ));
    docs.push((
        "an svg of more than 4000 elements",
        page("", &format!("<svg width=\"100\" height=\"40\"><g>{}</g></svg>", "<rect width=\"1\" height=\"1\"/>".repeat(4100))),
    ));
    // The <use> budget spent to its last 2 units by 32 uses of a group of 300 rects (303 each) and 151 of a rect (2
    // each); then a use of the group, of a path of 300 points, or of a text of 300 bytes finds it short.
    for (what, target, last) in [
        ("the use budget short at a group's children", "", "big"),
        ("the use budget short at a path's points", "", "P"),
        ("the use budget short at a text's bytes", "", "T"),
    ] {
        let _ = target;
        let points: String = (0..300).map(|k| format!(" L{k} {}", k % 7)).collect();
        let defs = format!(
            "<defs><g id=\"big\">{}</g><rect id=\"r\" width=\"1\" height=\"1\"/><path id=\"P\" d=\"M0 0{points}\"/><text id=\"T\" y=\"10\">{}</text></defs>",
            "<rect width=\"1\" height=\"1\"/>".repeat(300),
            "a".repeat(300)
        );
        let uses = format!("{}{}<use href=\"#{last}\"/>", "<use href=\"#big\"/>".repeat(32), "<use href=\"#r\"/>".repeat(151));
        docs.push((what, page("", &format!("<svg width=\"100\" height=\"40\">{defs}{uses}</svg>"))));
    }
    docs.push((
        "cached groups under a gradient ancestor",
        page(
            "",
            &format!("<div style=\"fill:url(#nothing)\">{cached_groups}</div><div class=\"x\" fill=\"var(--q)\">{cached_groups}</div>"),
        ),
    ));
    docs
}

#[test]
fn written_svg_shapes_paint_alike() {
    let mut run = Run::new("written SVG shapes");
    let master = master_css();
    let (light, _) = themed_master(false);
    let (dark, vars) = themed_master(true);
    let docs = written_svg_documents();
    for (k, (what, html)) in docs.iter().enumerate() {
        for (sheet, theme) in [(&master, "master"), (&light, "light"), (&dark, "dark")] {
            let html = if theme == "dark" { themed_html(html.as_bytes(), true, &vars) } else { html.clone().into_bytes() };
            for width in [375.0, 180.0] {
                let mut steps = svg_steps(width, 667.0, k as u32);
                if heavy_svg_document(what) {
                    steps.truncate(5);
                }
                run.case(paint_record(375.0, 667.0, 2.0, sheet, b"", &html, &steps), format!("{what} ({theme}, {width} px)"));
            }
        }
    }
    let tally = run.finish();
    assert_eq!(tally.documents, docs.len() * 6);
    assert_svg_reached(
        &tally,
        &[
            "paths drawn",
            "paths filled",
            "paths stroked",
            "paths even-odd",
            "dashed paths",
            "stroke caps 0",
            "stroke caps 1",
            "stroke caps 2",
            "stroke joins 0",
            "stroke joins 1",
            "stroke joins 2",
            "fill gradients of type 1",
            "fill gradients of type 2",
            "stroke gradients of type 1",
            "stroke gradients of type 2",
            "isolated paints",
            "cached paints asked",
            "cached paints drawn",
            "groups with opacity",
            "transformed groups",
            "paths with NaN points",
            "texts drawn",
        ],
    );
}

// ---- Generated SVG documents ---------------------------------------------------------------------------------

const SVG_IDS: [&str; 8] = ["g1", "g2", "r1", "s1", "p1", "c", "x-y", "1a"];

fn svg_color(random: &mut Lcg) -> &'static str {
    pick(
        random,
        &[
            "red",
            "#00f",
            "rgba(0,128,0,.5)",
            "none",
            "currentColor",
            "transparent",
            "url(#g1)",
            "url(#r1) red",
            "url(#missing)",
            "url(#g2) none",
            "var(--c)",
            "inherit",
            "",
            "#fff8",
        ],
    )
}

fn svg_number_text(random: &mut Lcg) -> String {
    match random.below(8) {
        0 => pick(random, &["0", "-1", "1e39", "50%", "2px", "1em", "x", "", ".5", "-0"]).to_string(),
        _ => common::svg::path_number(random),
    }
}

/// Paint properties of an SVG element: names and values.
fn svg_paint_pairs(random: &mut Lcg) -> Vec<(&'static str, String)> {
    (0..random.below(5))
        .map(|_| match random.below(17) {
            0 | 1 => ("fill", svg_color(random).to_string()),
            2 | 3 => ("stroke", svg_color(random).to_string()),
            4 => ("fill-opacity", svg_number_text(random)),
            5 => ("stroke-opacity", svg_number_text(random)),
            6 | 7 => ("stroke-width", svg_number_text(random)),
            8 => ("stroke-linecap", pick(random, &["butt", "round", "square", "ROUND", "x"]).to_string()),
            9 => ("stroke-linejoin", pick(random, &["miter", "round", "bevel", "arcs"]).to_string()),
            10 => ("stroke-miterlimit", svg_number_text(random)),
            11 => (
                "stroke-dasharray",
                (0..random.below(5)).map(|_| svg_number_text(random)).collect::<Vec<_>>().join(pick(random, &[" ", ",", ", "])),
            ),
            12 => ("stroke-dashoffset", svg_number_text(random)),
            13 => ("fill-rule", pick(random, &["evenodd", "nonzero", "EvenOdd"]).to_string()),
            14 => ("opacity", svg_number_text(random)),
            15 => ("visibility", pick(random, &["hidden", "visible", "collapse", " Visible "]).to_string()),
            _ => ("display", pick(random, &["none", "inline", " NONE "]).to_string()),
        })
        .collect()
}

/// Paint attributes of an SVG element, as attributes or in its `style`.
fn svg_paint_attributes(random: &mut Lcg, out: &mut String) {
    let mut style = String::new();
    for (name, value) in svg_paint_pairs(random) {
        if random.chance(4) {
            style += &format!("{name}:{value};");
        } else {
            out.push_str(&format!(" {name}=\"{value}\""));
        }
    }
    if random.chance(6) {
        style += pick(
            random,
            &[
                "color:green;",
                "--c:orange;",
                "opacity:.5;",
                "transform:rotate(10deg);",
                "visibility:hidden;",
                "display:none;",
                "text-anchor:middle;",
            ],
        );
    }
    if !style.is_empty() {
        out.push_str(&format!(" style=\"{style}\""));
    }
}

fn svg_paint_element(random: &mut Lcg, out: &mut String, depth: u32) {
    let tag = pick(
        random,
        &[
            "rect",
            "circle",
            "ellipse",
            "line",
            "polyline",
            "polygon",
            "path",
            "path",
            "g",
            "g",
            "use",
            "use",
            "text",
            "text",
            "symbol",
            "linearGradient",
            "radialGradient",
            "defs",
            "svg",
            "a",
            "switch",
            "image",
            "clipPath",
        ],
    );
    out.push('<');
    out.push_str(tag);
    if random.chance(3) || matches!(tag, "symbol" | "linearGradient" | "radialGradient") {
        out.push_str(&format!(" id=\"{}\"", SVG_IDS[random.below(SVG_IDS.len())]));
    }
    let lengths: &[&str] = match tag {
        "circle" => &["cx", "cy", "r"],
        "ellipse" => &["cx", "cy", "rx", "ry"],
        "rect" => &["x", "y", "width", "height", "rx", "ry"],
        "line" => &["x1", "y1", "x2", "y2"],
        "use" => &["x", "y", "width", "height"],
        "linearGradient" => &["x1", "y1", "x2", "y2"],
        "radialGradient" => &["cx", "cy", "r", "fx", "fy"],
        "text" => &["x", "y", "dx", "dy"],
        _ => &[],
    };
    for name in lengths {
        if !random.chance(4) {
            let value = if random.chance(3) { format!("{}%", random.below(120)) } else { format!("{}", random.below(120) as i64 - 10) };
            out.push_str(&format!(" {name}=\"{value}\""));
        }
    }
    match tag {
        "polyline" | "polygon" => {
            let points: Vec<String> = (0..random.below(9)).map(|_| format!("{}", random.below(100))).collect();
            out.push_str(&format!(" points=\"{}\"", points.join(" ")));
        }
        "path" => {
            let d = common::svg::any_path_data(random);
            out.push_str(&format!(" d=\"{}\"", String::from_utf8_lossy(&d).replace(['\0', '"'], "")));
        }
        "use" => out.push_str(&format!(" href=\"#{}\"", SVG_IDS[random.below(SVG_IDS.len())])),
        "symbol" if random.chance(2) => out.push_str(&format!(" viewBox=\"0 0 {} {}\"", 1 + random.below(50), 1 + random.below(50))),
        "linearGradient" | "radialGradient" if random.chance(2) => {
            out.push_str(&format!(" gradientUnits=\"{}\"", pick(random, &["userSpaceOnUse", "objectBoundingBox", "x"])))
        }
        _ => {}
    }
    if random.chance(4) {
        out.push_str(&format!(" transform=\"{}\"", String::from_utf8_lossy(&common::svg::any_transform_text(random)).replace('"', "")));
    }
    if !matches!(tag, "linearGradient" | "radialGradient") {
        svg_paint_attributes(random, out);
    }
    if random.chance(8) {
        out.push_str(&format!(" class=\"{}\"", pick(random, &["a", "b", "a b"])));
    }
    out.push('>');
    match tag {
        "g" | "symbol" | "defs" | "svg" | "a" | "switch" | "clipPath" if depth < 5 => {
            for _ in 0..random.below(6) {
                svg_paint_element(random, out, depth + 1);
            }
        }
        "linearGradient" | "radialGradient" => {
            for _ in 0..random.below(4) {
                out.push_str(&format!(
                    "<stop offset=\"{}\" stop-color=\"{}\"{}></stop>",
                    svg_number_text(random),
                    pick(random, &["red", "#00f", "currentColor", "x", "rgba(1,2,3,.5)"]),
                    if random.chance(3) { format!(" stop-opacity=\"{}\"", svg_number_text(random)) } else { String::new() }
                ));
            }
        }
        "text" => {
            for _ in 0..1 + random.below(3) {
                if random.chance(2) {
                    out.push_str(pick(random, &["Hi ", "  spaced  out ", "word", "\u{5e9}\u{5dc}\u{5d5}\u{5dd} "]));
                } else {
                    out.push_str("<tspan");
                    for name in ["x", "y", "dx", "dy"] {
                        if random.chance(3) {
                            out.push_str(&format!(" {name}=\"{}{}\"", random.below(60), pick(random, &["", "em", "px"])));
                        }
                    }
                    svg_paint_attributes(random, out);
                    out.push_str(">tspan</tspan>");
                }
            }
        }
        _ => {}
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// A generated document of SVGs for painting: sizes, view boxes and aspect ratios, every shape, groups, `<use>`
/// of shapes, groups and symbols, gradients with stops, texts with `<tspan>`s, nested `<svg>`s, every paint
/// attribute as an attribute or in a style, sheets that style them (with `:hover` rules that move the paint keys).
fn svg_paint_document(random: &mut Lcg) -> Vec<u8> {
    let mut out = String::from("<!doctype html><html><head><style>");
    for _ in 0..random.below(4) {
        let selector =
            pick(random, &["rect", "circle", "g", ".a", ".b", "text", "svg", "body:hover rect", "svg:hover circle", "path", "tspan"]);
        let decls: String = svg_paint_pairs(random).iter().map(|(name, value)| format!("{name}:{value};")).collect();
        out.push_str(&format!("{selector}{{{decls}}}"));
    }
    out.push_str("</style></head><body>");
    for _ in 0..1 + random.below(3) {
        out.push_str("<svg");
        if !random.chance(4) {
            out.push_str(&format!(" width=\"{}\"", pick(random, &["100", "50%", "0", "300", "40", "1e39"])));
        }
        if !random.chance(4) {
            out.push_str(&format!(" height=\"{}\"", pick(random, &["80", "100%", "0", "150", "40"])));
        }
        if random.chance(2) {
            out.push_str(&format!(
                " viewBox=\"{} {} {} {}\"",
                random.below(20) as i64 - 10,
                random.below(20) as i64 - 10,
                random.below(200),
                random.below(200)
            ));
        }
        if random.chance(3) {
            out.push_str(&format!(
                " preserveAspectRatio=\"{}\"",
                pick(random, &["none", "xMinYMin meet", "xMidYMid slice", "xMaxYMax", "xMinYMax slice"])
            ));
        }
        if random.chance(3) {
            out.push_str(&format!(
                " style=\"{}\"",
                pick(
                    random,
                    &[
                        "color:purple",
                        "--c:teal;color:red",
                        "opacity:.5",
                        "display:block;width:120px",
                        "transform:rotate(5deg)",
                        "border:2px solid"
                    ]
                )
            ));
        }
        out.push('>');
        for _ in 0..1 + random.below(8) {
            svg_paint_element(random, &mut out, 0);
        }
        out.push_str("</svg>");
        if random.chance(4) {
            out.push_str("<p>text</p>");
        }
    }
    out.push_str("</body></html>");
    out.into_bytes()
}

#[test]
fn generated_svg_documents_paint_alike() {
    let mut run = Run::new("generated SVG documents");
    let master = master_css();
    let mut random = Lcg::new(0x1D_5001);
    for k in 0..2500u32 {
        let html = if k % 5 == 4 { common::svg::any_svg_document(&mut random) } else { svg_paint_document(&mut random) };
        let width = [320.0, 375.0, 200.0, 480.0][k as usize % 4];
        let steps = if k % 3 == 0 { svg_steps(width, 600.0, k) } else { svg_steps(width, 600.0, k).into_iter().take(10).collect() };
        run.case(
            paint_record(width, 600.0, 2.0, &master, b"", &html, &steps),
            format!("generated SVG {k}: {}", String::from_utf8_lossy(&html)),
        );
    }
    let tally = run.finish();
    assert_eq!(tally.documents, 2500);
    assert_svg_reached(
        &tally,
        &["paths drawn", "texts drawn", "fill gradients of type 1", "fill gradients of type 2", "cached paints drawn", "dashed paths"],
    );
}

/// The switches over SVG documents, each on both sides: the group cache off (`LH_NO_SVG_GROUP_CACHE`), the late cache
/// (`LH_SVG_LATE_CACHE`), `LH_NO_TRANSFORMED_CULLING` and the five layout switches.
#[test]
fn the_switches_paint_svgs_alike() {
    let master = master_css();
    let docs = written_svg_documents();
    for switch in [
        "LH_NO_SVG_GROUP_CACHE",
        "LH_SVG_LATE_CACHE",
        "LH_NO_TRANSFORMED_CULLING",
        "LH_NO_LAYOUT_CACHE",
        "LH_NO_POSITIONED_REUSE",
        "LH_NO_FIXED_LAYOUT_REUSE",
        "LH_LAYOUT_CACHE_CHECK",
        "LH_NO_COLUMN_REUSE",
    ] {
        let mut run = Run::with_switch("SVG switch", switch);
        let mut random = Lcg::new(0x1D_5002);
        for k in 0..150u32 {
            let html = svg_paint_document(&mut random);
            run.case(
                paint_record(320.0, 600.0, 2.0, &master, b"", &html, &svg_steps(320.0, 600.0, k)),
                format!("{switch} {k}: {}", String::from_utf8_lossy(&html)),
            );
        }
        for (k, (what, html)) in docs.iter().enumerate().filter(|(_, (what, _))| !heavy_svg_document(what)) {
            run.case(
                paint_record(375.0, 667.0, 2.0, &master, b"", html.as_bytes(), &svg_steps(375.0, 667.0, k as u32)),
                format!("{switch}: {what}"),
            );
        }
        let tally = run.finish();
        let groups_cached = tally.counts.get("cached paint keys").copied().unwrap_or(0);
        assert!(groups_cached > 0, "{switch}: no cached paint");
    }
}

/// An `<svg>` 124 levels deep (100 groups, a text and 20 `<tspan>`s, then `<use>`s of the outermost group and one
/// within) paints on a thread whose stack is the size the layout's measures asked (1 MiB unoptimized, 512 KiB
/// optimized); the painter takes 112 bytes of stack a level optimized, 304 unoptimized.
#[test]
fn a_deep_svg_paints_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let mut html = String::from("<!DOCTYPE html><html><head></head><body><svg width=\"100\" height=\"100\">");
        for k in 0..100 {
            html += &format!("<g transform=\"translate(1 0)\" opacity=\".99\" id=\"d{k}\">");
        }
        html += "<text x=\"1\" y=\"10\">";
        for _ in 0..20 {
            html += "<tspan dx=\"1\">t";
        }
        html += &"</tspan>".repeat(20);
        html += "</text>";
        html += &"</g>".repeat(100);
        html += "<use href=\"#d0\"/><use href=\"#d3\" x=\"5\"/></svg></body></html>";
        let lh = Lh::new();
        let mut tally = PaintTally::default();
        let mut cached = PaintStep::viewport(375.0, 667.0);
        cached.cache = true;
        cached.shapes = true;
        let steps = [Step::Layout(375.0, None), paint(PaintStep::viewport(375.0, 667.0)), paint(cached), Step::Dump];
        let input = paint_record(375.0, 667.0, 2.0, &master_css(), b"", html.as_bytes(), &steps);
        let answer = paint_answer(&lh, Switches::default(), &input, &mut tally);
        assert_eq!(answer.0, 1);
        tally.counts.get("groups").copied().unwrap_or(0)
    });
    let groups = thread.expect("a thread").join().expect("the paint ran");
    eprintln!("a deep svg: {groups} groups");
    assert!(groups > 100);
}
