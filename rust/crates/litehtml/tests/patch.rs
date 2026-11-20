//! Document patching, ported (stage L19), with the layout cache's incremental relayout after it (stage L18): documents
//! made as the renderer makes them (`createUpdatableFromString`), then patched to new HTML (the template's holes, the
//! structural diff, refusals), to new inline styles and to new attribute values and texts, with media changes,
//! `:hover` flips and layouts between, through `oracle/lhdump` (`b`) and the port: each patch's answer, whether it
//! changed anything, was paint-only and the regions it repaints, the HTML the document stands for, the dirty regions
//! of each relayout, then the DOM, every computed style, the render tree, the layout with its cache and marks and what
//! patching keeps (holes, selector scope, epochs, memos, class flips, generated content), with the container's calls
//! in order; and each relayout against a fresh document of the same HTML (where the two part is reported, not
//! failed). The patches: widget-sandbox's corpus edited (texts, styles, classes, attributes, retags, elements added,
//! moved and removed, structural changes that must refuse, holes and styles patched directly), the 4.1 shadow tree's
//! serialiser and patch log over generated op batches, written edge cases, generated sequences, every patching switch,
//! and a deep document on a small stack.

mod common;

use std::collections::BTreeMap;

use common::cascade::*;
use common::gumbo::{corpus_pages, snapshot};
use common::patch::*;
use common::*;
use twidget_json::Json;
use twidget_litehtml::ElementId;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::gumbo::*;
use twidget_litehtml::patch::HoleChange;

/// A case: its record, the port's answer and what it is.
type Case = (Vec<u8>, (u8, Vec<u8>), String);

/// Steps, each patch with a label for the report.
#[derive(Default)]
struct Script {
    steps: Vec<PatchStep>,
    labels: Vec<String>,
}

impl Script {
    fn step(&mut self, step: PatchStep) {
        self.steps.push(step);
        self.labels.push(String::new());
    }

    /// A patch, a layout at the width, a dump when asked, and the fresh comparison (not while an element is hovered
    /// or active, which a fresh document is not).
    fn patch(&mut self, step: PatchStep, label: &str, width: f32, dump: bool) {
        self.patch_as(step, label, width, dump, true);
    }

    fn patch_as(&mut self, step: PatchStep, label: &str, width: f32, dump: bool, fresh: bool) {
        self.steps.push(step);
        self.labels.push(label.to_string());
        self.step(PatchStep::Layout(width));
        if dump {
            self.step(PatchStep::Dump);
        }
        if fresh {
            self.step(PatchStep::Fresh);
        }
    }
}

/// `b` cases through lhdump and the port, checked in batches of about 16 MB; settings (`E`) open every batch.
struct Run {
    name: &'static str,
    lh: Lh,
    switches: Switches,
    settings: Vec<u8>,
    batch: Vec<Case>,
    bytes: usize,
    cases: usize,
    steps: usize,
    tally: PatchTally,
    /// Where incremental layouts parted from fresh ones, by the patch before: count and the first few.
    fresh: BTreeMap<String, (usize, Vec<String>)>,
    /// The same by why they part (`classify_fresh`).
    fresh_kinds: BTreeMap<String, usize>,
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
            steps: 0,
            tally: PatchTally::default(),
            fresh: BTreeMap::new(),
            fresh_kinds: BTreeMap::new(),
            oracle: Oracle::find("lhdump"),
        }
    }

    /// A run with a switch set on both sides.
    fn with_switch(name: &'static str, switch: &str) -> Run {
        let mut run = Run::new(name);
        run.settings = framed(b'E', format!("{switch}=1").as_bytes());
        let p = &mut run.switches.patch;
        match switch {
            "LH_NO_TEMPLATE" => p.template = false,
            "LH_NO_STRUCTURE" => p.structure = false,
            "LH_NO_CLASS_HOLES" => p.class_holes = false,
            "LH_NO_CLASS_FLIP" => p.class_flip = false,
            "LH_NO_MATCH_MEMO" => p.match_memo = false,
            "LH_NO_BACKGROUND_PATCH" => p.background_patch = false,
            "LH_NO_PROPERTY_PATCH" => p.property_patch = false,
            "LH_NO_COLOR_REGIONS" => p.color_regions = false,
            "LH_NO_TEXT_REGIONS" => p.text_regions = false,
            "LH_NO_INPLACE_TEXT" => p.inplace_text = false,
            "LH_NO_SHALLOW_WIDTH_PATCH" => p.shallow_width_patch = false,
            "LH_NO_KEEP_TREE" => p.keep_tree = false,
            "LH_NO_PAINT_ONLY" => p.paint_only = false,
            "LH_NO_PATCH_REGIONS" => p.patch_regions = false,
            "LH_NO_SAME_SIZE_TEXT" => p.same_size_text = false,
            "LH_NO_LAYOUT_CACHE" => run.switches.layout_cache = false,
            "LH_LAYOUT_CACHE_CHECK" => run.switches.layout_cache_check = true,
            "LH_NO_STYLE_SHARING" => run.switches.style_sharing = false,
            other => panic!("no switch {other}"),
        }
        run
    }

    fn case(&mut self, width: f32, height: f32, sanitizing: bool, master: &[u8], html: &[u8], script: &Script, what: impl Into<String>) {
        let what = what.into();
        let input = patch_record(width, height, 2.0, sanitizing, master, html, &script.steps);
        let mut tally = PatchTally::default();
        let mut parted: Vec<(usize, String)> = Vec::new();
        let mut tied = false;
        let rust = patch_answer_with(&self.lh, self.switches, &input, &mut tally, &mut |step, cx, doc, fresh| {
            // Holes whose spans tie are in std::sort's library order in the C++ and in the tree's here (L3's finding):
            // which clone a patch reaches differs, so such a case is not compared.
            tied = tied || doc.template_ties > 0;
            if let Some(fresh) = fresh
                && geometry(doc) != geometry(fresh)
            {
                let kind = classify_fresh(cx.names, doc, fresh);
                parted.push((step, format!("[{kind}] {}", describe_fresh(cx.names, doc, fresh))));
            }
        });
        for (step, description) in parted {
            if let Some(end) = description.find(']') {
                *self.fresh_kinds.entry(description[1..end].to_string()).or_default() += 1;
            }
            let label = script.labels[..step].iter().rfind(|l| !l.is_empty()).cloned().unwrap_or_else(|| "the document as made".into());
            let entry = self.fresh.entry(label).or_default();
            entry.0 += 1;
            if entry.1.len() < 8 {
                entry.1.push(format!("{description}\n      in {}", what.chars().take(160).collect::<String>()));
            }
        }
        self.cases += 1;
        self.steps += script.steps.len();
        self.tally.merge(tally);
        if tied {
            // The case grew the name table and the declaration cache the C++ process will not have: the next cases
            // start a process of their own.
            self.tally.add("cases left out for tied holes");
            self.flush();
            self.lh = Lh::new();
            return;
        }
        self.bytes += input.len() + rust.1.len();
        if self.oracle.is_some() {
            self.batch.push((input, rust, what));
        }
        if self.bytes > 16 << 20 {
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
            input.extend(framed(b'b', case));
        }
        let answers = oracle.run(input, batch.len());
        // Every batch is a process of its own: the next one starts with a fresh name table and declaration cache.
        self.lh = Lh::new();
        let mut differences = Vec::new();
        for ((case, rust, what), cpp) in batch.iter().zip(&answers) {
            if *cpp == *rust {
                continue;
            }
            let (cpp_body, _) = split_answer(&cpp.1);
            let (rust_body, _) = split_answer(&rust.1);
            if cpp.0 != rust.0 || cpp_body != rust_body {
                differences.push(explain(self.switches, case, &(cpp.0, cpp_body), &(rust.0, rust_body), what));
            } else if let Some(calls) = compare_calls(&cpp.1, &rust.1) {
                differences.push(format!("{calls}\n  in {}", what.chars().take(3000).collect::<String>()));
            }
            if differences.len() >= 3 {
                break;
            }
        }
        assert!(differences.is_empty(), "{}: answers differ, the first:\n{}", self.name, differences.join("\n"));
    }

    fn finish(mut self) -> PatchTally {
        self.flush();
        let compared = if self.oracle.is_some() { "alike with lhdump" } else { "patched (no oracle)" };
        eprintln!("{}: {} cases, {} steps {compared}; {}", self.name, self.cases, self.steps, self.tally.report());
        if !self.fresh_kinds.is_empty() {
            eprintln!("  where relayouts part from fresh documents: {:?}", self.fresh_kinds);
        }
        for (label, (count, examples)) in &self.fresh {
            eprintln!("  {count} relayouts after {label} part from a fresh document's, e.g.");
            for e in examples {
                eprintln!("    {e}");
            }
        }
        self.tally
    }
}

/// Where two answers part: the field the Rust answer was writing there.
fn explain(switches: Switches, case: &[u8], cpp: &(u8, Vec<u8>), rust: &(u8, Vec<u8>), what: &str) -> String {
    let at = cpp.1.iter().zip(&rust.1).position(|(a, b)| a != b).unwrap_or(cpp.1.len().min(rust.1.len()));
    let marks = {
        start_marks();
        patch_answer(&Lh::new(), switches, case, &mut PatchTally::default());
        take_marks()
    };
    let item = marks.iter().rfind(|m| m.0 <= at && m.2 >= 0).map_or(-1, |m| m.2);
    let field = marks.iter().rfind(|m| m.0 <= at).map_or("", |m| m.1);
    let step = marks.iter().rfind(|m| m.0 <= at && m.1 == "step").map_or(-1, |m| m.2);
    let near = |b: &[u8]| {
        let piece = &b[at.saturating_sub(16).min(b.len())..(at + 64).min(b.len())];
        hex(&piece[..piece.len().min(48)])
    };
    format!(
        "status C++ {} / Rust {}, {} / {} bytes, first difference at byte {at}, step {step}, {field} (item or element {item}):\n  C++  {}\n  Rust {}\n  in {}",
        cpp.0,
        rust.0,
        cpp.1.len(),
        rust.1.len(),
        near(&cpp.1),
        near(&rust.1),
        what.chars().take(3000).collect::<String>()
    )
}

// ---- The planner: the port's document kept in step, to aim patches at what is there ---------------------------

/// A hole change as a `b` record names it: the element's place in the walk, text, name, value.
type HoleChangeAt = (u32, bool, Vec<u8>, Vec<u8>);

/// A hole as the planner lists it: its owner's place, text, name, value, class, source.
type HoleAt = (u32, bool, Vec<u8>, Vec<u8>, bool, bool);

struct Planner {
    lh: Lh,
    container: CascadeContainer,
    doc: Document,
    current: Vec<u8>,
    switches: Switches,
}

macro_rules! cx {
    ($p:expr) => {
        StyleCx { names: &$p.lh.names, container: &$p.container, cache: &$p.lh.cache, switches: $p.switches }
    };
}

impl Planner {
    fn new(width: f32, height: f32, sanitizing: bool, master: &[u8], html: &[u8], switches: Switches) -> Planner {
        let lh = Lh::new();
        let container = CascadeContainer::new(width, height, 2.0);
        container.sanitizing.set(sanitizing);
        let doc = {
            let cx = StyleCx { names: &lh.names, container: &container, cache: &lh.cache, switches };
            Document::create_updatable(html, master, &cx)
        };
        Planner { lh, container, doc, current: html.to_vec(), switches }
    }

    fn html(&mut self, html: &[u8]) -> bool {
        let cx = cx!(self);
        let mut changed = false;
        let ok = self.doc.update_from_string(&cx, html, &mut changed);
        if ok {
            self.current = html.to_vec();
        }
        ok
    }

    fn holes(&mut self, list: &[(u32, bool, Vec<u8>, Vec<u8>)]) -> bool {
        let w = ElementIndex::new(&self.doc);
        let changes: Vec<HoleChange> = list
            .iter()
            .map(|(k, text, name, value)| HoleChange { owner: w.pick(*k), name: name.clone(), text: *text, value: value.clone() })
            .collect();
        let cx = cx!(self);
        let mut changed = false;
        let ok = self.doc.update_holes(&cx, &changes, &mut changed);
        if ok && let Some(html) = self.doc.template_html() {
            self.current = html;
        }
        ok
    }

    fn styles(&mut self, list: &[(u32, Vec<u8>)]) -> bool {
        let w = ElementIndex::new(&self.doc);
        let styles: Vec<(Option<ElementId>, Vec<u8>)> = list.iter().map(|(k, v)| (w.pick(*k), v.clone())).collect();
        let cx = cx!(self);
        let mut changed = false;
        let ok = self.doc.update_styles(&cx, &styles, &mut changed);
        if ok && let Some(html) = self.doc.template_html() {
            self.current = html;
        }
        ok
    }

    fn media(&mut self, width: f32, height: f32) {
        self.container.width.set(width);
        self.container.height.set(height);
        let cx = cx!(self);
        self.doc.media_changed(&cx);
    }

    /// Applies the step as the record will, keeping the planner in step.
    fn apply(&mut self, step: &PatchStep) -> bool {
        match step {
            PatchStep::Html(html) => self.html(html),
            PatchStep::Styles(list) => self.styles(list),
            PatchStep::Holes(list) => self.holes(list),
            PatchStep::Media(w, h) => {
                self.media(*w, *h);
                true
            }
            PatchStep::Hover(k) | PatchStep::Active(k) | PatchStep::Leave(k) => {
                let w = ElementIndex::new(&self.doc);
                let tags: Vec<ElementId> = w.nodes.iter().map(|n| n.0).filter(|&id| self.doc.kind(id).is_html_tag()).collect();
                if tags.is_empty() {
                    return false;
                }
                let el = tags[*k as usize % tags.len()];
                match step {
                    PatchStep::Hover(_) => self.doc.on_mouse_over(el),
                    PatchStep::Active(_) => self.doc.on_lbutton_down(el),
                    _ => self.doc.on_mouse_leave(el),
                };
                let root = self.doc.root.expect("a root");
                let cx = cx!(self);
                self.doc.find_styles_changes(&cx, root);
                true
            }
            _ => true,
        }
    }

    /// The elements of the walk with the attribute at the value.
    fn by_attr(&self, name: &[u8], value: &[u8]) -> Option<u32> {
        let w = ElementIndex::new(&self.doc);
        w.nodes.iter().position(|&(el, _)| self.doc.get_attr(el, name) == Some(value)).map(|k| k as u32)
    }

    /// The k-th element of the walk with the tag.
    fn by_tag(&self, tag: &[u8], nth: usize) -> Option<u32> {
        let w = ElementIndex::new(&self.doc);
        w.nodes.iter().enumerate().filter(|&(_, &(el, _))| self.doc.get_tag_name(&self.lh.names, el) == tag).nth(nth).map(|(k, _)| k as u32)
    }

    /// A text hole's owner and its text, or an attribute hole's element, name and value.
    fn holes_list(&self) -> Vec<HoleAt> {
        let w = ElementIndex::new(&self.doc);
        let mut out = Vec::new();
        for hole in &self.doc.holes {
            let owner = if hole.text { hole.text_elements.first().and_then(|&t| self.doc.parent(t)) } else { hole.owner };
            let k = w.of(owner);
            if k < 0 {
                continue;
            }
            out.push((k as u32, hole.text, hole.name.clone(), hole.value.clone(), hole.cls, hole.src));
        }
        out
    }
}

// ---- Edits of HTML text, aimed through the port's Gumbo -------------------------------------------------------

fn pick<'a>(r: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[r.below(from.len())]
}

struct AttrSpan {
    start: usize,
    end: usize,
    name: Vec<u8>,
}

struct ElSpan {
    start: usize,
    end: usize,
    tag_end: usize,
    tag: Vec<u8>,
}

#[derive(Default)]
struct Spans {
    attrs: Vec<AttrSpan>,
    texts: Vec<(usize, usize)>,
    elements: Vec<ElSpan>,
}

fn spans(html: &[u8]) -> Spans {
    let out = gumbo_parse_with_options(&kGumboDefaultOptions, html);
    let mut s = Spans::default();
    let mut pending = vec![out.document];
    while let Some(id) = pending.pop() {
        let node = out.node(id);
        match node.ty {
            GUMBO_NODE_ELEMENT | GUMBO_NODE_TEMPLATE => {
                let e = node.element();
                for attr in &e.attributes {
                    let v = attr.original_value;
                    if let Some(at) = v.data
                        && v.length >= 2
                        && at + v.length <= html.len()
                        && (html[at] == b'"' || html[at] == b'\'')
                    {
                        s.attrs.push(AttrSpan { start: at + 1, end: at + v.length - 1, name: c_str(&attr.name).to_vec() });
                    }
                }
                if let (Some(start), Some(end)) = (e.original_tag.data, e.original_end_tag.data)
                    && e.original_tag.length > 0
                    && e.original_end_tag.length > 0
                    && end + e.original_end_tag.length <= html.len()
                {
                    s.elements.push(ElSpan {
                        start,
                        end: end + e.original_end_tag.length,
                        tag_end: start + e.original_tag.length,
                        tag: gumbo_normalized_tagname(e.tag).to_vec(),
                    });
                }
                pending.extend(e.children.iter().rev());
            }
            GUMBO_NODE_DOCUMENT => pending.extend(node.document().children.iter().rev()),
            GUMBO_NODE_TEXT => {
                let t = node.text().original_text;
                if let Some(at) = t.data
                    && t.length > 0
                    && at + t.length <= html.len()
                {
                    s.texts.push((at, at + t.length));
                }
            }
            _ => {}
        }
    }
    s
}

fn splice(html: &[u8], start: usize, end: usize, with: &[u8]) -> Vec<u8> {
    [&html[..start], with, &html[end..]].concat()
}

/// A new value for a style attribute, as the renderer writes them (`name:value` joined by `;`).
fn style_edit(r: &mut Lcg, old: &[u8]) -> Vec<u8> {
    let decl = match r.below(24) {
        0..=3 => format!("width:{}px", 10 + r.below(90)),
        4 => format!("height:{}px", 5 + r.below(40)),
        5 | 6 => format!("color:{}", pick(r, &["red", "#123456", "rgb(1,2,3)", "blue"])),
        7 | 8 => format!("background-color:{}", pick(r, &["#fff", "rgba(0,0,0,0.5)", "green", "transparent"])),
        9 | 10 => format!("transform:{}", pick(r, &["translateX(4px)", "rotate(10deg)", "none", "scale(1.5)", "translate(2px,3px)"])),
        11 => format!("opacity:{}", pick(r, &["0.5", "1", "0"])),
        12 => format!("display:{}", pick(r, &["none", "block", "flex", "inline-block", "grid"])),
        13 => format!("margin:{}px", r.below(9)),
        14 => format!("padding:{}px {}px", r.below(9), r.below(9)),
        15 => format!("position:{};left:{}px", pick(r, &["relative", "absolute", "static"]), r.below(20)),
        16 => format!("--v:{}px", r.below(20)),
        17 => "height:var(--v,6px)".to_string(),
        18 => format!("border-color:{}", pick(r, &["red", "#00f"])),
        19 => format!("visibility:{}", pick(r, &["hidden", "visible"])),
        20 => format!("font-size:{}px", 8 + r.below(20)),
        21 => format!("background:{}", pick(r, &["red", "none", "linear-gradient(red,blue)"])),
        22 => format!("box-shadow:{}", pick(r, &["0 0 3px red", "none"])),
        _ => "counter-increment:c".to_string(),
    };
    let name = &decl.as_bytes()[..decl.find(':').unwrap_or(0)];
    let mut parts: Vec<Vec<u8>> = old.split(|&c| c == b';').filter(|p| !p.is_empty()).map(<[u8]>::to_vec).collect();
    match r.below(5) {
        0 => parts.clear(),
        1 if !parts.is_empty() => {
            parts.remove(r.below(parts.len()));
        }
        _ => {}
    }
    if let Some(at) = parts.iter().position(|p| p.starts_with(name) && p.get(name.len()) == Some(&b':')) {
        parts[at] = decl.into_bytes();
    } else if !r.chance(8) {
        parts.push(decl.into_bytes());
    }
    parts.join(&b';')
}

/// One edit of the document's text: what it became and its kind.
fn edit(r: &mut Lcg, html: &[u8]) -> (Vec<u8>, &'static str) {
    let s = spans(html);
    for _ in 0..12 {
        match r.below(30) {
            0..=4 if !s.texts.is_empty() => {
                let (a, b) = s.texts[r.below(s.texts.len())];
                let piece: Vec<u8> =
                    html[a..b].iter().map(|&c| if c.is_ascii_lowercase() { b'a' + (c - b'a' + 1) % 26 } else { c }).collect();
                if piece != html[a..b] {
                    return (splice(html, a, b, &piece), "a text changed in place");
                }
            }
            5 | 6 if !s.texts.is_empty() => {
                let (_, b) = s.texts[r.below(s.texts.len())];
                let more = pick(r, &[" and more words", "x", " 12", "ünï", " a"]);
                return (splice(html, b, b, more.as_bytes()), "a text reworded");
            }
            7 if !s.texts.is_empty() => {
                let (a, b) = s.texts[r.below(s.texts.len())];
                return (splice(html, a, b, pick(r, &[" ", "\n", "&amp;x", "a\rb"]).as_bytes()), "a text blanked or escaped");
            }
            8 if !s.texts.is_empty() => {
                let (_, b) = s.texts[r.below(s.texts.len())];
                return (splice(html, b, b, pick(r, &["<b>x</b>", "<br>", "<span class=\"a\">y</span>"]).as_bytes()), "markup in a text");
            }
            9..=13 => {
                let styles: Vec<&AttrSpan> = s.attrs.iter().filter(|a| a.name == b"style").collect();
                if let Some(a) = styles.get(r.below(styles.len().max(1))) {
                    let value = style_edit(r, &html[a.start..a.end]);
                    return (splice(html, a.start, a.end, &value), "a style attribute");
                }
            }
            14..=16 => {
                let classes: Vec<&AttrSpan> = s.attrs.iter().filter(|a| a.name == b"class").collect();
                if let Some(a) = classes.get(r.below(classes.len().max(1))) {
                    let value = pick(r, &["a", "b", "a b", "on", "off", "", "x y z", "hidden", "row", "card", "title"]);
                    return (splice(html, a.start, a.end, value.as_bytes()), "a class attribute");
                }
            }
            17 | 18 => {
                let others: Vec<&AttrSpan> =
                    s.attrs.iter().filter(|a| a.name != b"style" && a.name != b"class" && a.name != b"data-twid").collect();
                if let Some(a) = others.get(r.below(others.len().max(1))) {
                    let value = pick(r, &["1", "x", "20", "red", "M0 0 L5 5", "#abc", "t", "data:image/png;base64,AA"]);
                    return (splice(html, a.start, a.end, value.as_bytes()), "another attribute");
                }
            }
            19 if !s.elements.is_empty() => {
                let e = &s.elements[r.below(s.elements.len())];
                let at = e.start + 1 + e.tag.len().min(e.tag_end - e.start - 1);
                let attr = pick(r, &[" title=\"t\"", " id=\"k\"", " data-x=\"1\"", " hidden", " class=\"a\"", " style=\"color:red\""]);
                return (splice(html, at, at, attr.as_bytes()), "an attribute added");
            }
            20 | 21 => {
                let removable: Vec<&ElSpan> = s.elements.iter().filter(|e| !matches!(&e.tag[..], b"html" | b"head" | b"body")).collect();
                if let Some(e) = removable.get(r.below(removable.len().max(1))) {
                    return (splice(html, e.start, e.end, b""), "an element removed");
                }
            }
            22 => {
                let copies: Vec<&ElSpan> = s.elements.iter().filter(|e| !matches!(&e.tag[..], b"html" | b"head" | b"body")).collect();
                if let Some(e) = copies.get(r.below(copies.len().max(1))) {
                    let copy = html[e.start..e.end].to_vec();
                    return (splice(html, e.end, e.end, &copy), "an element copied");
                }
            }
            23 | 24 => {
                let places: Vec<&ElSpan> = s.elements.iter().filter(|e| !matches!(&e.tag[..], b"html" | b"head")).collect();
                if let Some(e) = places.get(r.below(places.len().max(1))) {
                    let new = pick(
                        r,
                        &[
                            "<div class=\"a\" style=\"height:5px\">new</div>",
                            "<p>p</p>",
                            "<span>s</span>",
                            "<img src=\"n.png\">",
                            "<div><i>deep</i> text</div>",
                        ],
                    );
                    let at = if e.tag == b"body" || r.chance(2) { e.tag_end } else { e.end };
                    return (splice(html, at, at, new.as_bytes()), "an element added");
                }
            }
            25 => {
                let movable: Vec<&ElSpan> = s.elements.iter().filter(|e| !matches!(&e.tag[..], b"html" | b"head" | b"body")).collect();
                if movable.len() >= 2 {
                    let e = movable[r.below(movable.len())];
                    let piece = html[e.start..e.end].to_vec();
                    let cut = splice(html, e.start, e.end, b"");
                    let t = spans(&cut);
                    if let Some(body) = t.elements.iter().find(|x| x.tag == b"body") {
                        return (splice(&cut, body.tag_end, body.tag_end, &piece), "an element moved");
                    }
                }
            }
            26 => {
                if let Some(body) = s.elements.iter().find(|e| e.tag == b"body") {
                    let piece = pick(
                        r,
                        &[
                            "<style>p{margin:1px}</style>",
                            "<select><option>o</option></select>",
                            "<link rel=\"stylesheet\" href=\"a.css\">",
                        ],
                    );
                    return (splice(html, body.tag_end, body.tag_end, piece.as_bytes()), "a document-level element added");
                }
            }
            27 if html.starts_with(b"<!DOCTYPE html>") => {
                return (html[15..].to_vec(), "the doctype dropped");
            }
            28 if !s.texts.is_empty() => {
                let (_, b) = s.texts[r.below(s.texts.len())];
                return (splice(html, b, b, b"counter(x)"), "counters mentioned");
            }
            29 => return (html.to_vec(), "nothing"),
            _ => {}
        }
    }
    (html.to_vec(), "nothing")
}

/// A patch aimed at a hole the planner's document has: through `update_holes` or `update_styles`, the value as
/// serialized HTML carries it, or one it cannot carry.
fn hole_patch(r: &mut Lcg, p: &Planner) -> Option<(PatchStep, &'static str)> {
    let holes = p.holes_list();
    if holes.is_empty() {
        return None;
    }
    let (k, text, name, value, cls, src) = holes[r.below(holes.len())].clone();
    let bad = r.chance(10);
    let new: Vec<u8> = if bad {
        pick(r, &["a&b", "x\ry", "<b>", "\"q\"", " ", "\n lead", "x\u{0}y", "'"]).as_bytes().to_vec()
    } else if text {
        if r.chance(2) {
            value.iter().map(|&c| if c.is_ascii_lowercase() { b'a' + (c - b'a' + 3) % 26 } else { c }).collect()
        } else {
            [&value[..], b" more"].concat()
        }
    } else if cls {
        pick(r, &["a", "b", "on", "off", "", "a b", "card", "row"]).as_bytes().to_vec()
    } else if src {
        pick(r, &["n.png", "javascript:x", "data:image/png;base64,AB", ""]).as_bytes().to_vec()
    } else if name == b"style" {
        style_edit(r, &value)
    } else {
        pick(r, &["10", "25", "red", "M0 0 L9 9 Z", "none", "0.5"]).as_bytes().to_vec()
    };
    if !text && name == b"style" && r.chance(2) {
        let list = vec![(k, new)];
        return Some((PatchStep::Styles(list), if bad { "update_styles with a value HTML cannot carry" } else { "update_styles" }));
    }
    let mut list = vec![(k, text, name.clone(), new)];
    if r.chance(6) {
        // A second change, sometimes of the same hole, sometimes of no hole at all.
        let (k2, text2, name2, value2, _, _) = holes[r.below(holes.len())].clone();
        list.push((if r.chance(5) { u32::MAX } else { k2 }, text2, name2, [&value2[..], b"z"].concat()));
    }
    let label = if bad {
        "update_holes with a value HTML cannot carry"
    } else if text {
        "update_holes of a text"
    } else if cls {
        "update_holes of a class"
    } else if src {
        "update_holes of a source"
    } else if name == b"style" {
        "update_holes of a style"
    } else {
        "update_holes of an SVG attribute"
    };
    Some((PatchStep::Holes(list), label))
}

/// A sequence of patches over a document: edits of its text, holes and styles patched directly, now and then a
/// media change or a `:hover` flip; each followed by a layout and the fresh comparison, every few by a dump.
fn sequence(r: &mut Lcg, p: &mut Planner, width: f32, count: usize, dump_every: usize, script: &mut Script) {
    script.step(PatchStep::Layout(width));
    script.step(PatchStep::Dump);
    let mut hovered: Option<u32> = None;
    for n in 0..count {
        let dump = dump_every != 0 && n % dump_every == dump_every - 1;
        let choice = r.below(20);
        if choice < 12 {
            let (html, label) = edit(r, &p.current.clone());
            let step = PatchStep::Html(html);
            p.apply(&step);
            script.patch_as(step, label, width, dump, hovered.is_none());
        } else if choice < 17 {
            if let Some((step, label)) = hole_patch(r, p) {
                p.apply(&step);
                script.patch_as(step, label, width, dump, hovered.is_none());
            }
        } else if choice == 17 {
            let (w, h) = *[(320.0, 600.0), (800.0, 600.0), (200.0, 900.0)].get(r.below(3)).expect("a size");
            let step = PatchStep::Media(w, h);
            p.apply(&step);
            script.step(step);
        } else if choice == 18 && hovered.is_none() {
            let k = r.next();
            let step = PatchStep::Hover(k);
            p.apply(&step);
            script.step(step);
            hovered = Some(k);
        } else if let Some(k) = hovered {
            // The element hovered, or (after a patch moved it) the one now in its place, and its ancestors.
            let step = PatchStep::Leave(k);
            p.apply(&step);
            script.step(step);
            hovered = None;
        }
    }
    script.step(PatchStep::Layout(width));
    script.step(PatchStep::Dump);
}

// ---- The corpus ------------------------------------------------------------------------------------------------

#[test]
fn the_corpus_is_patched_alike() {
    let mut run = Run::new("corpus");
    let pages = corpus_pages();
    assert!(pages.len() > 300, "widget-sandbox's corpus: {} pages", pages.len());
    let light = themed_master(false);
    let dark = themed_master(true);
    let mut r = Lcg::new(0x5eed_0019);
    for (k, (name, text)) in pages.iter().enumerate() {
        let snap = snapshot(text);
        let dark_theme = k % 3 == 2;
        let (master, vars) = if dark_theme { &dark } else { &light };
        let html = themed_html(&snap, dark_theme, vars);
        let (width, height) = if dark_theme { (412.0, 915.0) } else { (375.0, 667.0) };
        let mut p = Planner::new(width, height, false, master, &html, run.switches);
        let mut script = Script::default();
        sequence(&mut r, &mut p, width, 8, 3, &mut script);
        let theme = if dark_theme { "dark" } else { "light" };
        run.case(width, height, false, master, &html, &script, format!("{name} ({theme})"));
    }
    let tally = run.finish();
    assert!(tally.count("update_from_string patched") > 500);
}

// ---- The renderer's shadow tree and patch log --------------------------------------------------------------

/// A renderer's backend for the shadow tree: canvases drawn into nothing, fonts by size, no images.
#[derive(Default)]
struct Host;

impl twidget_shadow::DocumentHost for Host {
    fn canvas_run(&mut self, _canvas: i32, _width: i32, _height: i32, _ops: &[twidget_wire::CanvasOp]) {}
    fn canvas_release(&mut self, _canvas: i32) {}
    fn canvas_font(&mut self, _css: &[u8], metrics: &mut twidget_wire::FontMetrics) -> i32 {
        metrics.ascent = 10.0;
        metrics.descent = 2.0;
        metrics.height = 12.0;
        1
    }
    fn canvas_text_width(&mut self, _font: i32, text: &[u8]) -> f32 {
        8.0 * text.len() as f32
    }
    fn image_of(&mut self, _src: &[u8]) -> Option<(i32, f32, f32)> {
        None
    }
}

fn quote(s: &str) -> String {
    String::from_utf8_lossy(&twidget_json::quote(s.as_bytes())).into_owned()
}

const ROOTS: &str = r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]}"#;

/// The renderer's patch scripts' page (twidget_renderer's tests): a sheet, boxes with classes and styles holding
/// texts, an inline span, a textarea, a canvas, an SVG, an image, a transformed and a positioned box, a details; and
/// a list, a table and a flex box of this stage's own.
fn shadow_page(r: &mut Lcg) -> Vec<u8> {
    let mut ops = vec![ROOTS.to_string()];
    let sheet = pick(
        r,
        &[
            ".a{color:red;padding:2px}.b{border:1px solid blue;width:40px}.on{background:#eee}.c{font-size:20px;line-height:24px}",
            ".a{height:12px}.b{display:flex}.c{font-size:20px;line-height:24px} @media (max-width:250px){.a{height:30px}}",
            ".q{width:25%;height:10px}.a{min-height:5px}.on::before{content:'*'} .b .c{color:green}",
            ".a{margin:var(--m,3px)} :root{--m:4px} .b{color:var(--tg-theme-text-color,black)}",
            "li:nth-child(2){color:red} .a{color:blue}",
            "[data-x]{color:red} .a{color:blue}",
            "",
        ],
    );
    ops.push(format!("{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}", quote(sheet)));
    for i in 0..8 {
        let (e, t) = (10 + i, 20 + i);
        let class = pick(r, &["a", "b", "a b", "c", "on", "q", ""]);
        let style = pick(
            r,
            &[
                "height:10px",
                "width:50px;height:20px",
                "",
                "position:relative;left:3px",
                "transform:scale(2)",
                "overflow:hidden;height:15px",
                "padding:4px;border:2px solid red",
            ],
        );
        let tag = pick(r, &["div", "div", "p", "span"]);
        ops.push(format!(
            "{{\"op\":\"el\",\"id\":{e},\"tag\":\"{tag}\",\"attrs\":[[\"class\",{}],[\"style\",{}]]}},{{\"op\":\"ins\",\"parent\":4,\"id\":{e},\"before\":null}},{{\"op\":\"text\",\"id\":{t},\"data\":{}}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{t},\"before\":null}}",
            quote(class),
            quote(style),
            quote(pick(r, &["item", "a longer line of text that wraps", "x", "ünï"]))
        ));
    }
    ops.push(r#"{"op":"el","id":30,"tag":"span","attrs":[["style","font-size:10px"]]},{"op":"ins","parent":12,"id":30,"before":null},{"op":"text","id":31,"data":"words words words words words words words"},{"op":"ins","parent":30,"id":31,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":32,"tag":"textarea","attrs":[["style","height:20px;padding:0;border:0"]]},{"op":"ins","parent":4,"id":32,"before":null},{"op":"prop","id":32,"name":"value","value":"\na\nb"}"#.to_string());
    ops.push(r#"{"op":"el","id":33,"tag":"canvas","attrs":[["width","40"],["height","20"]]},{"op":"ins","parent":4,"id":33,"before":null},{"op":"canvas","id":33,"cmds":[["c","fillRect",0,0,5,5]]}"#.to_string());
    ops.push(r#"{"op":"el","id":34,"tag":"svg","ns":"svg","attrs":[["width","100"],["height","50"],["viewBox","0 0 200 100"]]},{"op":"ins","parent":4,"id":34,"before":null},{"op":"el","id":35,"tag":"rect","ns":"svg","attrs":[["x","10"],["y","10"],["width","30"],["height","20"],["fill","red"]]},{"op":"ins","parent":34,"id":35,"before":null},{"op":"el","id":36,"tag":"path","ns":"svg","attrs":[["d","M0 0 L50 0 L50 50 Z"],["stroke","blue"]]},{"op":"ins","parent":34,"id":36,"before":null}"#.to_string());
    ops.push(format!(
        "{{\"op\":\"el\",\"id\":39,\"tag\":\"img\",\"attrs\":[[\"src\",{}]]}},{{\"op\":\"ins\",\"parent\":4,\"id\":39,\"before\":null}}",
        quote(pick(r, &["a.png", "data:image/png;base64,iVBORw0KGgo=", "b.png"]))
    ));
    ops.push(r#"{"op":"el","id":40,"tag":"div","attrs":[["style","width:50px;height:10px;transform:rotate(30deg)"]]},{"op":"ins","parent":4,"id":40,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":41,"tag":"div","attrs":[["style","position:relative;margin-left:20px;padding:5px"]]},{"op":"ins","parent":4,"id":41,"before":null},{"op":"el","id":42,"tag":"div","attrs":[["style","position:absolute;left:7px;width:50px;height:10px"]]},{"op":"ins","parent":41,"id":42,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":50,"tag":"ul","attrs":[]},{"op":"ins","parent":4,"id":50,"before":null},{"op":"el","id":51,"tag":"li","attrs":[]},{"op":"ins","parent":50,"id":51,"before":null},{"op":"text","id":52,"data":"one"},{"op":"ins","parent":51,"id":52,"before":null},{"op":"el","id":53,"tag":"li","attrs":[["class","a"]]},{"op":"ins","parent":50,"id":53,"before":null},{"op":"text","id":54,"data":"two"},{"op":"ins","parent":53,"id":54,"before":null}"#.to_string());
    if r.chance(2) {
        ops.push(r#"{"op":"el","id":60,"tag":"table","attrs":[]},{"op":"ins","parent":4,"id":60,"before":null},{"op":"el","id":61,"tag":"tbody","attrs":[]},{"op":"ins","parent":60,"id":61,"before":null},{"op":"el","id":62,"tag":"tr","attrs":[]},{"op":"ins","parent":61,"id":62,"before":null},{"op":"el","id":63,"tag":"td","attrs":[["style","width:30px"]]},{"op":"ins","parent":62,"id":63,"before":null},{"op":"text","id":64,"data":"cell"},{"op":"ins","parent":63,"id":64,"before":null}"#.to_string());
    }
    if r.chance(2) {
        ops.push(r#"{"op":"el","id":70,"tag":"div","attrs":[["style","display:flex;gap:4px"]]},{"op":"ins","parent":4,"id":70,"before":null},{"op":"el","id":71,"tag":"div","attrs":[["style","flex:1"]]},{"op":"ins","parent":70,"id":71,"before":null},{"op":"text","id":72,"data":"f"},{"op":"ins","parent":71,"id":72,"before":null},{"op":"el","id":73,"tag":"div","attrs":[["style","width:20px"]]},{"op":"ins","parent":70,"id":73,"before":null}"#.to_string());
    }
    format!("[{}]", ops.join(",")).into_bytes()
}

/// A batch of what a busy widget sends (twidget_renderer's `patch_batch`, with structural ops of this stage's own).
fn shadow_batch(r: &mut Lcg) -> Vec<u8> {
    let mut ops = Vec::new();
    for _ in 0..1 + r.below(4) {
        let e = 10 + r.below(8);
        let t = 20 + r.below(8);
        let op = match r.below(30) {
            0..=4 => format!(
                "{{\"op\":\"style\",\"id\":{e},\"name\":{},\"value\":{}}}",
                quote(pick(
                    r,
                    &["transform", "background-color", "opacity", "color", "height", "width", "padding-top", "--m", "font-size", "display"]
                )),
                quote(pick(r, &["translateX(4px)", "rgb(3,4,5)", "0.5", "red", "17px", "33px", "2px", "8px", "none", "block", "12px", ""]))
            ),
            5..=7 => format!(
                "{{\"op\":\"attr\",\"id\":{e},\"name\":\"class\",\"value\":{}}}",
                quote(pick(r, &["a", "a b", "b", "c", "on", "q", ""]))
            ),
            8..=11 => format!(
                "{{\"op\":\"data\",\"id\":{t},\"data\":{}}}",
                quote(pick(r, &["text 1", "text 22", "a longer text that wraps onto a second line here", "&<>", "", "ünï 😀", "item"]))
            ),
            12 => format!(
                "{{\"op\":\"attr\",\"id\":{e},\"name\":\"style\",\"value\":{}}}",
                quote(pick(r, &["height:20px", "", "color:blue;width:10px"]))
            ),
            13 => format!("{{\"op\":\"attr\",\"id\":{e},\"name\":\"title\",\"value\":\"t\"}}"),
            14 => format!(
                "{{\"op\":\"attr\",\"id\":{},\"name\":{},\"value\":{}}}",
                *[35, 36, 34].get(r.below(3)).expect("an id"),
                quote(pick(r, &["fill", "x", "width", "d", "stroke", "viewBox"])),
                quote(pick(r, &["green", "20", "M0 0 L10 10", "0 0 100 100", "5"]))
            ),
            15 => format!(
                "{{\"op\":\"canvas\",\"id\":33,\"cmds\":[[\"s\",\"width\",{}],[\"c\",\"fillRect\",0,0,5,5]]}}",
                *[40, 41, 60].get(r.below(3)).expect("a width")
            ),
            16 => format!(
                "{{\"op\":\"attr\",\"id\":39,\"name\":\"src\",\"value\":{}}}",
                quote(pick(r, &["c.png", "data:image/png;base64,QUJD", "a.png"]))
            ),
            17 => format!("{{\"op\":\"prop\",\"id\":32,\"name\":\"value\",\"value\":{}}}", quote(pick(r, &["x", "\nlead", "", "a\nb\nc"]))),
            18 | 19 => {
                let fresh = 100 + r.below(50);
                format!(
                    "{{\"op\":\"el\",\"id\":{fresh},\"tag\":\"div\",\"attrs\":[[\"style\",\"height:7px\"]]}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{fresh},\"before\":null}}"
                )
            }
            20 => format!("{{\"op\":\"rm\",\"id\":{}}}", 100 + r.below(50)),
            21 => format!(
                "{{\"op\":\"style\",\"id\":{e},\"name\":\"transform\",\"value\":\"translateX({}px)\",\"important\":{}}}",
                r.below(30),
                r.chance(4)
            ),
            22 => format!("{{\"op\":\"ins\",\"parent\":4,\"id\":{e},\"before\":{}}}", 10 + r.below(8)),
            23 => format!("{{\"op\":\"rm\",\"id\":{t}}}"),
            24 => format!(
                "{{\"op\":\"text\",\"id\":{},\"data\":\"n\"}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{},\"before\":null}}",
                200 + r.below(10),
                200 + r.below(10)
            ),
            25 => format!(
                "{{\"op\":\"attr\",\"id\":{},\"name\":\"class\",\"value\":{}}}",
                *[51, 53].get(r.below(2)).expect("an id"),
                quote(pick(r, &["a", "", "b"]))
            ),
            26 => format!(
                "{{\"op\":\"data\",\"id\":{},\"data\":{}}}",
                *[52, 54, 64, 72].get(r.below(4)).expect("an id"),
                quote(pick(r, &["uno", "a longer cell text", "2"]))
            ),
            27 => format!(
                "{{\"op\":\"style\",\"id\":{},\"name\":\"width\",\"value\":{}}}",
                *[63, 71, 73, 42].get(r.below(4)).expect("an id"),
                quote(pick(r, &["40px", "10%", "auto"]))
            ),
            28 => format!("{{\"op\":\"attr\",\"id\":{e},\"name\":\"data-x\",\"value\":\"1\"}}"),
            _ => "{\"op\":\"style\",\"id\":2,\"name\":\"--tg-theme-bg-color\",\"value\":\"#123456\"}".to_string(),
        };
        ops.push(op);
    }
    format!("[{}]", ops.join(",")).into_bytes()
}

fn apply_ops(doc: &mut twidget_shadow::Document, batch: &[u8]) {
    let json = Json::parse(batch).expect("a batch parses");
    let mut out = Vec::new();
    doc.receive_ops(json.arr(), &mut Host, &mut out);
}

/// The renderer's `patch_document`: the patch log's changes as `update_holes` takes them, each owner the element the
/// renderer indexed by `data-twid` in the planner's document; None where the renderer builds instead.
fn log_changes(doc: &twidget_shadow::Document, log: &twidget_shadow::PatchLog, p: &Planner) -> Option<Vec<HoleChangeAt>> {
    use twidget_shadow::{NodeKind, escape_attr, escape_text};
    if !log.valid || (log.attributes.is_empty() && log.texts.is_empty()) {
        return None;
    }
    let chrome = twidget_shadow::Chrome { overrides: &Default::default(), editing: 0 };
    let index = |id: i64| p.by_attr(b"data-twid", id.to_string().as_bytes());
    let mut changes = Vec::new();
    for (r, names) in log.attributes.values() {
        let n = doc.get(*r);
        let k = index(n.id)?;
        for name in names {
            let mut value = Vec::new();
            if name == b"style" {
                if n.styles.is_empty() {
                    return None;
                }
                doc.write_style(*r, &chrome, &mut value);
            } else if name == b"src" && n.tag == b"canvas" && !n.svg {
                let c = doc.canvas(n.id)?;
                value = format!("tw-canvas:{}/{}x{}", c.id(), c.width(), c.height()).into_bytes();
            } else {
                let (_, v) = n.attrs.iter().find(|(key, _)| key == name)?;
                if !twidget_css::is_attribute_name(name) {
                    return None;
                }
                escape_attr(v, &mut value);
            }
            changes.push((k, false, name.clone(), value));
        }
    }
    for r in log.texts.values() {
        let n = doc.get(*r);
        let k = index(n.id)?;
        if n.tag == b"style" || n.tag == b"script" {
            return None;
        }
        let mut value = Vec::new();
        if n.tag == b"textarea" {
            if !n.children.is_empty() || n.data.is_empty() {
                return None;
            }
            if n.data[0] == b'\n' {
                value.push(b'\n');
            }
            escape_text(&n.data, &mut value);
        } else {
            if n.children.len() != 1 {
                return None;
            }
            let child = doc.get(n.children[0]);
            if child.kind != NodeKind::Text || child.data.is_empty() {
                return None;
            }
            escape_text(&child.data, &mut value);
        }
        changes.push((k, true, Vec::new(), value));
    }
    Some(changes)
}

/// A widget's session as the renderer patches it: the page, then batches, each into the document through the patch
/// log's holes when it can (`update_holes`), else as the serialised HTML (`update_from_string`).
fn shadow_script(r: &mut Lcg, master: &[u8], width: f32, batches: usize) -> (Vec<u8>, Script) {
    let mut sd = twidget_shadow::Document::new(false);
    apply_ops(&mut sd, &shadow_page(r));
    let chrome_overrides = Default::default();
    let chrome = twidget_shadow::Chrome { overrides: &chrome_overrides, editing: 0 };
    let html = sd.serialize(&chrome);
    sd.take_patch_log();
    let mut p = Planner::new(width, 667.0, false, master, &html, Switches::default());
    let mut script = Script::default();
    script.step(PatchStep::Layout(width));
    script.step(PatchStep::Dump);
    for n in 0..batches {
        apply_ops(&mut sd, &shadow_batch(r));
        let log = sd.take_patch_log();
        let new_html = sd.serialize(&chrome);
        let dump = n % 4 == 3;
        let mut patched = false;
        if let Some(changes) = log_changes(&sd, &log, &p) {
            let step = PatchStep::Holes(changes);
            patched = p.apply(&step) && p.current == new_html;
            script.patch(step, "the renderer's patch log", width, false);
        }
        if !patched {
            let step = PatchStep::Html(new_html);
            p.apply(&step);
            script.patch(step, "the renderer's serialised HTML", width, dump);
        } else if dump {
            script.step(PatchStep::Dump);
        }
        if r.chance(8) {
            let step = PatchStep::Media(*[300.0f32, 240.0, 600.0].get(r.below(3)).expect("a width"), 667.0);
            p.apply(&step);
            script.step(step);
        }
    }
    (html, script)
}

#[test]
fn the_renderers_batches_are_patched_alike() {
    let mut run = Run::new("shadow tree batches");
    let (light, _) = themed_master(false);
    let mut r = Lcg::new(0x5eed_4100);
    for k in 0..400 {
        let width = *[300.0f32, 240.0, 412.5].get(k % 3).expect("a width");
        let (html, script) = shadow_script(&mut r, &light, width, 10);
        run.case(width, 667.0, false, &light, &html, &script, format!("shadow session {k}"));
    }
    let tally = run.finish();
    assert!(tally.count("update_holes patched") > 200 && tally.count("update_from_string patched") > 200);
}

// ---- Written edge cases ----------------------------------------------------------------------------------------

/// A written case being scripted: its sheet, the planner and the steps.
struct W<'a> {
    sheet: &'a str,
    p: &'a mut Planner,
    s: &'a mut Script,
    width: f32,
    hovered: bool,
}

fn page(sheet: &str, body: &str) -> Vec<u8> {
    format!("<!DOCTYPE html><html><head><style>{sheet}</style></head><body>{body}</body></html>").into_bytes()
}

impl W<'_> {
    fn id(&self, id: &str) -> u32 {
        self.p.by_attr(b"id", id.as_bytes()).unwrap_or(u32::MAX)
    }
    fn tag(&self, tag: &str, nth: usize) -> u32 {
        self.p.by_tag(tag.as_bytes(), nth).unwrap_or(u32::MAX)
    }
    /// The element's place among the tags of the walk, as `h`, `a` and `x` name it.
    fn tag_index(&self, id: &str) -> u32 {
        let k = self.id(id) as usize;
        let ix = ElementIndex::new(&self.p.doc);
        ix.nodes.iter().take(k).filter(|n| self.p.doc.kind(n.0).is_html_tag()).count() as u32
    }
    fn go(&mut self, step: PatchStep, label: &str) {
        self.p.apply(&step);
        self.s.patch_as(step, label, self.width, true, !self.hovered);
    }
    fn body(&mut self, body: &str, label: &str) {
        let html = page(self.sheet, body);
        self.go(PatchStep::Html(html), label);
    }
    fn html(&mut self, html: &str, label: &str) {
        self.go(PatchStep::Html(html.as_bytes().to_vec()), label);
    }
    fn text(&mut self, owner: u32, value: &str, label: &str) {
        self.go(PatchStep::Holes(vec![(owner, true, Vec::new(), value.as_bytes().to_vec())]), label);
    }
    fn attr(&mut self, owner: u32, name: &str, value: &[u8], label: &str) {
        self.go(PatchStep::Holes(vec![(owner, false, name.as_bytes().to_vec(), value.to_vec())]), label);
    }
    fn holes(&mut self, list: Vec<(u32, bool, &str, &[u8])>, label: &str) {
        let list = list.into_iter().map(|(k, t, n, v)| (k, t, n.as_bytes().to_vec(), v.to_vec())).collect();
        self.go(PatchStep::Holes(list), label);
    }
    fn styles(&mut self, list: Vec<(u32, &str)>, label: &str) {
        let list = list.into_iter().map(|(k, v)| (k, v.as_bytes().to_vec())).collect();
        self.go(PatchStep::Styles(list), label);
    }
    fn plain(&mut self, step: PatchStep) {
        match step {
            PatchStep::Hover(_) | PatchStep::Active(_) => self.hovered = true,
            PatchStep::Leave(_) => self.hovered = false,
            _ => {}
        }
        self.p.apply(&step);
        self.s.step(step);
    }
}

/// A written case: what it is, its sheet, its body, whether the container sanitizes, and its script.
type Written = (&'static str, &'static str, &'static str, bool, fn(&mut W));

fn written_cases() -> Vec<Written> {
    vec![
        (
            "values HTML cannot carry",
            ".x{color:red}",
            r#"<div id="a" class="x" style="color:red">text one</div><pre id="p">pre text</pre><textarea id="t">area</textarea><div id="n">no style</div>"#,
            false,
            |w| {
                let a = w.id("a");
                for v in ["a&b", "x\ry", "x\u{0}y", "<b>", "   ", "\nnew"] {
                    w.text(a, v, "a text HTML cannot carry");
                }
                w.go(PatchStep::Holes(vec![(a, true, Vec::new(), b"bad \xff byte".to_vec())]), "a text that is not UTF-8");
                let p = w.id("p");
                w.text(p, "\npre", "a pre's text after a newline");
                let t = w.id("t");
                w.text(t, "\nx", "a textarea's text after a newline");
                w.attr(a, "style", b"color:\"blue\"", "a style with its quote");
                w.attr(a, "class", b"x\"", "a class with its quote");
                w.attr(a, "style", b"color:red", "a style as it was");
                w.holes(vec![(a, false, "style", b"color:blue"), (a, false, "style", b"color:green")], "one hole twice");
                w.holes(vec![(u32::MAX, false, "style", b"color:blue")], "no owner");
                w.holes(vec![(a, false, "title", b"t")], "no hole of the name");
                let body = w.tag("body", 0);
                w.holes(vec![(body, true, "", b"text")], "a text whose owner is not its parent");
                let n = w.id("n");
                w.styles(vec![(n, "color:red")], "update_styles of an element without a style");
                w.styles(vec![(a, "color:blue"), (a, "color:green")], "update_styles of one element twice");
                w.styles(vec![(a, "color:\"x\"")], "update_styles with the quote");
                w.styles(vec![(a, "color:red")], "update_styles as it was");
                w.styles(vec![(a, "color:blue;padding:2px")], "update_styles");
                w.text(a, "text two", "a text in place");
                w.text(a, "text three words", "a text reworded");
            },
        ),
        (
            "class flips",
            ".on{background:yellow}.off{color:gray}.big{font-size:30px}.dot::before{content:'*'}",
            r#"<div id="a" class="off">label</div><div id="b" class="on"><span>inner</span></div><p id="c" class="off">x y</p>"#,
            false,
            |w| {
                let (a, b, c) = (w.id("a"), w.id("b"), w.id("c"));
                for value in ["on", "off", "on", "off", "big", "off", "on"] {
                    w.attr(a, "class", value.as_bytes(), "a class flipped");
                }
                w.attr(b, "class", b"off", "a class of an element with element children");
                w.attr(c, "class", b"dot", "a class that makes a ::before");
                w.attr(c, "class", b"off", "a class that drops a ::before");
                w.attr(c, "class", b"on", "a class flipped");
                w.plain(PatchStep::Media(800.0, 600.0));
                w.attr(c, "class", b"off", "a class flipped after a media change");
                w.body(
                    r#"<div id="a" class="big">label</div><div id="b" class="on"><span>inner</span></div><p id="c" class="off">x y</p>"#,
                    "a class through the HTML",
                );
            },
        ),
        (
            "classes ancestor selectors read",
            ".x .y{color:red} .x{padding:1px}",
            r#"<div id="a" class="x"><span class="y">t</span></div><div id="b" class="q">u</div>"#,
            false,
            |w| {
                let (a, b) = (w.id("a"), w.id("b"));
                w.attr(a, "class", b"z", "a class an ancestor selector reads");
                w.attr(b, "class", b"x", "a class becoming one an ancestor selector reads");
                w.attr(b, "class", b"r", "a class no selector reads");
                w.body(
                    r#"<div id="a" class="z"><span class="y">t</span></div><div id="b" class="r">u</div>"#,
                    "a class an ancestor selector reads, through the HTML",
                );
                w.body(
                    r#"<div id="a" class="z" title="t"><span class="y">t</span></div><div id="b" class="r">u</div>"#,
                    "an attribute added",
                );
            },
        ),
        (
            "attribute selectors",
            "[data-k]{color:red} .a{color:blue}",
            r#"<div id="a" class="a" data-k="1">t</div><div id="b"><p>p</p><p>q</p></div>"#,
            false,
            |w| {
                let a = w.id("a");
                w.attr(a, "class", b"b", "a class under attribute selectors");
                w.body(r#"<div id="a" class="a">t</div><div id="b"><p>p</p><p>q</p></div>"#, "an attribute selectors read removed");
                w.body(
                    r#"<div id="a" class="a">t</div><div id="b"><p>p</p><i>new</i><p>q</p></div>"#,
                    "an element added where attribute selectors read",
                );
                w.body(
                    r#"<div id="a" class="a">t</div><div id="b"><p data-k="2">p</p><p>q</p></div>"#,
                    "everything made again under attribute selectors",
                );
            },
        ),
        (
            "sibling and position selectors",
            "p+p{color:red} li:nth-child(odd){color:blue} .k{margin:1px}",
            r#"<div><p id="a" class="k">one</p><p>two</p></div><ul><li data-lh-id="x">x</li><li data-lh-id="y">y</li><li data-lh-id="z">z</li></ul>"#,
            false,
            |w| {
                let a = w.id("a");
                w.attr(a, "class", b"j", "a class under sibling selectors");
                w.body(r#"<div><p id="a" class="k">one</p><p>two</p></div><ul><li data-lh-id="y">y</li><li data-lh-id="x">x</li><li data-lh-id="z">z</li></ul>"#, "keyed children reordered under position selectors");
                w.body(
                    r#"<div><p id="a" class="k">one</p><p>two</p></div><ul><li data-lh-id="y">y</li><li data-lh-id="z">z</li></ul>"#,
                    "a keyed child removed under position selectors",
                );
            },
        ),
        (
            "keyed children",
            ".k{color:green}",
            r#"<ul id="u"><li data-lh-id="x" class="k">x</li><li data-lh-id="y">y</li><li data-lh-id="z" style="color:red">z</li><li>plain</li></ul>"#,
            false,
            |w| {
                w.body(r#"<ul id="u"><li data-lh-id="z" style="color:red">z</li><li data-lh-id="x" class="k">x</li><li data-lh-id="y">y</li><li>plain</li></ul>"#, "keyed children reordered");
                w.body(r#"<ul id="u"><li data-lh-id="z" style="color:blue">z</li><li data-lh-id="x" class="k">x2</li><li data-lh-id="w">w</li><li>plain</li></ul>"#, "keyed children changed and added");
                w.body(
                    r#"<ul id="u"><li data-lh-id="z">z</li><li data-lh-id="z">z</li><li>plain</li></ul>"#,
                    "keyed children with a key twice",
                );
                w.body(
                    r#"<ul id="u"><li data-lh-id="z">z</li><li data-lh-id="q"><b>b</b></li><li>plain</li></ul>"#,
                    "a keyed child of another kind",
                );
                w.body(
                    r#"<ul id="u"><b data-lh-id="z">z</b><li data-lh-id="q"><b>b</b></li><li>plain</li></ul>"#,
                    "a keyed child becoming another tag",
                );
            },
        ),
        ("empty and counters", "div:empty{height:3px} .a{color:red}", r#"<div id="a" class="a">t</div><div></div>"#, false, |w| {
            let a = w.id("a");
            w.attr(a, "class", b"b", "a class under :empty");
            w.text(a, "u", "a text under :empty");
            w.body(r#"<div id="a" class="a">t</div><div></div><p>new</p>"#, "an element added under :empty");
        }),
        (
            "counters in the sheet",
            "ol{counter-reset:c} li{counter-increment:c} li::before{content:counter(c) '. '}",
            r#"<ol><li id="a">one</li><li>two</li></ol>"#,
            false,
            |w| {
                let a = w.id("a");
                w.text(a, "uno", "a text where counters count");
                w.body(r#"<ol><li id="a">one</li><li>two</li><li>three</li></ol>"#, "an item added where counters count");
            },
        ),
        (
            "a select",
            ".o{color:red}",
            r#"<select id="s"><option id="o" class="o">x</option><option>y</option></select><div id="d">d</div>"#,
            false,
            |w| {
                let o = w.id("o");
                w.attr(o, "class", b"p", "a class in a select");
                w.body(
                    r#"<select id="s"><option id="o" class="o">x</option><option>y</option><option>z</option></select><div id="d">d</div>"#,
                    "an option added",
                );
                w.body(
                    r#"<select id="s"><option id="o" class="o">x2</option><option>y</option></select><div id="d">d</div>"#,
                    "an option's text",
                );
            },
        ),
        (
            "style attributes selected",
            "[style]{outline:1px solid red} .a{color:red}",
            r#"<div id="a" class="a" style="width:20px">t</div>"#,
            false,
            |w| {
                let a = w.id("a");
                w.attr(a, "style", b"width:30px", "a style where [style] selects");
                w.styles(vec![(a, "width:40px")], "update_styles where [style] selects");
                w.body(r#"<div id="a" class="a" style="width:50px">t</div>"#, "a style through the HTML where [style] selects");
                w.attr(a, "class", b"b", "a class where [style] selects");
            },
        ),
        (
            ":hover across patches",
            ".h{width:20px}.h:hover{color:red;width:30px}.h:active{background:blue}",
            r#"<div id="a" class="h" style="height:10px">hover me</div><div id="b">other</div>"#,
            false,
            |w| {
                let a = w.id("a");
                let h = w.tag_index("a");
                w.plain(PatchStep::Hover(h));
                w.text(a, "hovered me", "a text of a hovered element");
                w.attr(a, "style", b"height:20px", "a style of a hovered element");
                w.attr(a, "class", b"h k", "a class of a hovered element");
                w.plain(PatchStep::Active(h));
                w.body(
                    r#"<div id="a" class="h k" style="height:20px">hovered me</div><p>new</p><div id="b">other</div>"#,
                    "an element added while hovered",
                );
                w.plain(PatchStep::Leave(h));
                w.text(a, "left", "a text after leaving");
            },
        ),
        (
            "backgrounds alone",
            "td{padding:1px}",
            r#"<div id="a" style="background-color:red;width:50px">x<span style="background-color:inherit">y</span></div><div id="b" style="background-color:red">z</div><table><tr><td id="c" style="background-color:red">c</td></tr></table><p id="d" style="background-color:red;-lh-anim:1">p</p>"#,
            false,
            |w| {
                let (a, b, c, d) = (w.id("a"), w.id("b"), w.id("c"), w.id("d"));
                w.attr(a, "style", b"background-color:blue;width:50px", "a background a child inherits");
                w.attr(b, "style", b"background-color:blue", "a background alone");
                w.attr(c, "style", b"background-color:blue", "a cell's background");
                w.attr(d, "style", b"background-color:blue;-lh-anim:1", "a paragraph's background");
                w.attr(b, "style", b"background-color:green;color:red", "a background with a colour");
                w.attr(b, "style", b"background-color:green;color:red;/*c*/", "a background with a comment");
            },
        ),
        (
            "properties patched in place",
            ".g{transform:rotate(1deg)}",
            r#"<div id="a" style="transform:translateX(1px);opacity:0.5;background-color:red;width:10px">x<span>s</span></div><div id="b" style="width:20px">y</div><svg width="20" height="20"><rect id="r" width="5" height="5" style="opacity:0.5"/></svg>"#,
            false,
            |w| {
                let (a, b, r) = (w.id("a"), w.id("b"), w.id("r"));
                w.attr(a, "style", b"transform:translateX(2px);opacity:0.5;background-color:red;width:10px", "a transform alone");
                w.attr(a, "style", b"transform:translateX(2px);opacity:0.75;background-color:red;width:10px", "an opacity alone");
                w.attr(a, "style", b"transform:translateX(2px);opacity:0.75;background-color:blue;width:10px", "a background colour alone");
                w.attr(a, "style", b"transform:none;opacity:1;background-color:blue;width:10px", "a paint group gone");
                w.attr(b, "style", b"width:20px;transform:scale(2)", "a transform added");
                w.attr(b, "style", b"width:20px;transform:translate(var(--x,2px))", "a transform with var()");
                w.attr(b, "style", b"width:20px;transform:scale(2) !important", "an important transform");
                w.attr(r, "style", b"opacity:0.25", "an opacity in an SVG");
                w.styles(vec![(a, "transform:rotate(3deg);opacity:0.75;background-color:blue;width:10px")], "update_styles of a transform");
            },
        ),
        (
            "widths alone",
            ".g::before{content:'x'}",
            r#"<div id="a" style="width:100px"><p>t</p><div style="width:inherit">i</div></div><div id="f" style="display:flex;width:200px"><div>x</div></div><div id="g" style="display:grid;width:80px"><span>y</span></div><div id="h" class="g" style="width:50px">h</div><span id="i" style="width:10px">i</span><div id="j" style="width:40px"><p>plain</p></div>"#,
            false,
            |w| {
                let (a, f, g, h, i, j) = (w.id("a"), w.id("f"), w.id("g"), w.id("h"), w.id("i"), w.id("j"));
                w.attr(a, "style", b"width:120px", "a width a child inherits");
                w.attr(f, "style", b"display:flex;width:150px", "a flex box's width");
                w.attr(g, "style", b"display:grid;width:60%", "a grid's width in %");
                w.attr(h, "style", b"width:70px", "a width beside a ::before");
                w.attr(i, "style", b"width:20px", "an inline's width");
                w.attr(j, "style", b"width:60px", "a block's width");
                w.attr(f, "style", b"display:flex;width:var(--w,90px)", "a width with var()");
                w.holes(
                    vec![(f, false, "style", b"display:flex;width:160px"), (g, false, "style", b"display:grid;width:70px")],
                    "two widths",
                );
            },
        ),
        (
            "tables patched",
            "caption{height:12px}",
            r#"<table id="t" style="width:200px"><caption>cap</caption><tr id="r" class="a"><td id="c1" style="width:50px">a</td><td id="c2">b</td></tr></table><div id="d" style="display:table-cell">loose</div>"#,
            false,
            |w| {
                let (t, c1, c2, r, d) = (w.id("t"), w.id("c1"), w.id("c2"), w.id("r"), w.id("d"));
                w.attr(c1, "style", b"width:80px", "a cell's width");
                w.text(c2, "bee", "a cell's text in place");
                w.text(c2, "a longer cell", "a cell's text reworded");
                w.attr(t, "style", b"width:220px", "a table's width");
                w.attr(r, "class", b"b", "a row's class");
                w.body(r#"<table id="t" style="width:220px"><caption>cap</caption><tr id="r" class="b"><td id="c1" style="width:80px">a</td><td id="c2">a longer cell</td></tr><tr><td>new</td><td>row</td></tr></table><div id="d" style="display:table-cell">loose</div>"#, "a row added");
                w.body(r#"<table id="t" style="width:220px"><tr id="r" class="b"><td id="c1" style="width:80px">a</td><td id="c2">a longer cell</td></tr></table><div id="d" style="display:table-cell">loose</div>"#, "a caption removed");
                w.attr(d, "style", b"display:table-cell;padding:3px", "a loose cell's padding");
            },
        ),
        (
            "flex, grid and positioned boxes patched",
            "",
            r#"<div id="f" style="display:flex;flex-wrap:wrap;gap:2px"><div id="f1" style="flex:1;min-width:30px">one</div><div id="f2" style="width:40px">two</div></div><div id="g" style="display:grid;grid-template-columns:1fr 2fr"><span id="g1">a</span><span>b</span></div><div style="position:relative;height:40px"><div id="p" style="position:absolute;left:3px;top:4px;width:10px">abs</div></div><div id="s" style="position:sticky;top:0">sticky</div>"#,
            false,
            |w| {
                let (f, f1, f2, g, g1, p, s) = (w.id("f"), w.id("f1"), w.id("f2"), w.id("g"), w.id("g1"), w.id("p"), w.id("s"));
                w.attr(f1, "style", b"flex:2;min-width:30px", "a flex item's factor");
                w.attr(f2, "style", b"width:60px", "a flex item's width");
                w.attr(f, "style", b"display:flex;flex-wrap:wrap;gap:6px", "a flex box's gap");
                w.attr(g, "style", b"display:grid;grid-template-columns:2fr 1fr", "a grid's columns");
                w.text(g1, "a longer grid item", "a grid item's text");
                w.attr(p, "style", b"position:absolute;left:8px;top:4px;width:10px", "an absolute box's offset");
                w.attr(p, "style", b"position:static;left:8px;top:4px;width:10px", "a box no longer positioned");
                w.attr(p, "style", b"position:fixed;left:8px;top:4px;width:10px", "a fixed box");
                w.attr(s, "style", b"position:sticky;top:5px", "a sticky box's offset");
            },
        ),
        (
            "template ties and refusals",
            ".x{color:red}",
            r#"<p><b class="">bold<div id="d">in a div</div></b></p><div id="a" class=x>unquoted</div>"#,
            false,
            |w| {
                w.body(
                    r#"<p><b class="">bold<div id="d">in a div 2</div></b></p><div id="a" class=x>unquoted</div>"#,
                    "a text beside tied holes",
                );
                w.body(
                    r#"<p><b class="x">bold<div id="d">in a div 2</div></b></p><div id="a" class=x>unquoted</div>"#,
                    "a class of a cloned element",
                );
                w.body(
                    r#"<p><b class="x">bold<div id="d">in a div 2</div></b></p><div id="a" class=y>unquoted</div>"#,
                    "an unquoted class",
                );
            },
        ),
        ("the document's own elements", ".a{color:red}", r#"<div id="a" class="a">t</div>"#, false, |w| {
            let head = r#"<!DOCTYPE html><html><head><style>.a{color:red}</style>"#;
            w.html(&format!(r#"{head}<title>t</title></head><body><div id="a" class="a">t</div></body></html>"#), "a title added");
            w.html(r#"<!DOCTYPE html><html lang="en"><head><style>.a{color:red}</style></head><body><div id="a" class="a">t</div></body></html>"#, "the html element's attribute");
            w.html(
                r#"<!DOCTYPE html><html><head><style>.a{color:blue}</style></head><body><div id="a" class="a">t</div></body></html>"#,
                "the sheet changed",
            );
            w.html(&format!(r#"{head}</head><body class="b"><div id="a" class="a">t</div></body></html>"#), "the body's class added");
            w.html(&format!(r#"{head}</head><body class="c"><div id="a" class="a">t</div></body></html>"#), "the body's class");
            w.html(
                r#"<html><head><style>.a{color:red}</style></head><body class="c"><div id="a" class="a">t</div></body></html>"#,
                "quirks mode",
            );
            w.html(&format!(r#"{head}</head><body class="c"><div id="a" class="a">t</div><!-- c --></body></html>"#), "a comment added");
            w.html(&format!(r#"{head}</head><body class="c"><div id="a" class="a">t</div><!-- d --></body></html>"#), "a comment changed");
            w.html(&format!(r#"{head}</head><body class="c"><div id="a" class="a">t</div> <!-- d --></body></html>"#), "whitespace added");
            w.html(
                &format!(r#"{head}</head><body class="c"><div id="a" class="a">counter(x)</div></body></html>"#),
                "counters in the HTML",
            );
            w.html(
                &format!(r#"{head}</head><body class="c"><textarea id="t">area</textarea><div id="a" class="a">t</div></body></html>"#),
                "a textarea added",
            );
            w.html(
                &format!(r#"{head}</head><body class="c"><textarea id="t">area 2</textarea><div id="a" class="a">t</div></body></html>"#),
                "a textarea's text",
            );
            w.html(
                &format!(
                    r#"{head}</head><body class="c"><textarea id="t" rows="3">area 2</textarea><div id="a" class="a">t</div></body></html>"#
                ),
                "a textarea's attribute",
            );
        }),
        ("limits", "", r#"<div id="a">t</div>"#, false, |w| {
            let deep = format!("{}x{}", "<div>".repeat(130), "</div>".repeat(130));
            w.body(&deep, "too deep");
            let many = "<i>i</i>".repeat(20010);
            w.body(&many, "too many elements");
            w.body(r#"<div id="a">t</div><div>u</div>"#, "an element added after the limits");
        }),
        (
            "generated content given back",
            ".g::before{content:'generated text here'} .g::after{content:'after'} .h{color:red}",
            r#"<div id="a" class="g">a</div><div id="b" class="g">b</div><div id="c"><span class="g">c</span></div>"#,
            false,
            |w| {
                let a = w.id("a");
                w.attr(a, "class", b"h", "a class that drops generated boxes");
                w.attr(a, "class", b"g", "a class that makes generated boxes");
                w.body(r#"<div id="a" class="g">a</div><div id="c"></div>"#, "generated boxes removed with their elements");
                w.body(
                    r#"<div id="a" class="g">a</div><div id="c"><span class="g">c</span><span class="g">d</span></div>"#,
                    "generated boxes added",
                );
                let a = w.id("a");
                w.attr(a, "class", b"", "a class emptied");
            },
        ),
        (
            "a sanitizing container",
            ".a{color:red}",
            r#"<blink id="b">x</blink><img id="i" src="a.png"><div id="d" class="a" onclick="x()">d</div>"#,
            true,
            |w| {
                let (i, d) = (w.id("i"), w.id("d"));
                w.attr(i, "src", b"javascript:alert(1)", "a source the sanitizer drops");
                w.attr(i, "src", b"b.png", "a source");
                w.attr(d, "class", b"b", "a class of an element the sanitizer changed");
                w.body(
                    r#"<blink id="b" title="t">x</blink><img id="i" src="b.png"><div id="d" class="b" onclick="y()">d</div>"#,
                    "attributes the sanitizer drops",
                );
                w.body(
                    r#"<blink id="b" title="t">x</blink><img id="i" src="b.png"><div id="d" class="b">d</div><object>o</object>"#,
                    "an element the sanitizer refuses",
                );
            },
        ),
        (
            "SVG attributes",
            "",
            r#"<svg width="100" height="50"><rect id="r" x="1" y="2" width="10" height="5" fill="red"/><circle id="c" cx="20" cy="20" r="5" stroke="blue"/></svg><div id="d">d</div>"#,
            false,
            |w| {
                let (r, c) = (w.id("r"), w.id("c"));
                w.attr(r, "x", b"5", "an SVG x");
                w.attr(r, "fill", b"green", "an SVG fill");
                w.attr(c, "r", b"8", "an SVG radius");
                w.holes(vec![(r, false, "width", b"20"), (c, false, "stroke", b"red")], "two SVG attributes");
                w.attr(r, "transform", b"rotate(5)", "an SVG transform with no hole");
            },
        ),
        (
            "an SVG attribute a selector names",
            "[fill]{opacity:0.5}",
            r#"<svg width="100" height="50"><rect id="r" x="1" fill="red" width="10" height="5"/></svg>"#,
            false,
            |w| {
                let r = w.id("r");
                w.attr(r, "fill", b"green", "an SVG fill a selector names");
                w.attr(r, "x", b"3", "an SVG x beside it");
            },
        ),
        (
            "texts in place",
            "",
            r#"<div id="a">abc def</div><div id="b" style="font-size:20px">x</div><pre id="p">a
b</pre>"#,
            false,
            |w| {
                let (a, b, p) = (w.id("a"), w.id("b"), w.id("p"));
                w.text(a, "xyz def", "a text of the same size");
                w.text(a, "abcd def", "a text of another size");
                w.text(a, "abcd  def", "a text with another space");
                w.text(b, "y", "a large text");
                w.text(p, "c\nd", "a pre's text");
                w.plain(PatchStep::Media(320.0, 600.0));
                w.text(a, "back again", "a text after a media change");
            },
        ),
        ("retags", ".k{color:red}", r#"<div id="a" title="x" style="color:red">t</div><p id="b" class="k" lang="en">p</p>"#, false, |w| {
            w.body(r#"<div id="a" title="y" style="color:red">t</div><p id="b" class="k" lang="en">p</p>"#, "an attribute retagged");
            w.body(
                r#"<div id="a" title="y" style="color:var(--c,red)">t</div><p id="b" class="k" lang="en">p</p>"#,
                "a style with var() beside a retag",
            );
            w.body(r#"<div id="a" style="color:blue">t</div><p id="b" class="k" lang="fr">p</p>"#, "attributes removed and changed");
            w.body(r#"<div id="a" style="color:blue">t</div><p id="b" class="j" lang="fr" hidden>p</p>"#, "a class and an attribute");
            w.body(r#"<span id="a" style="color:blue">t</span><p id="b" class="j" lang="fr" hidden>p</p>"#, "a tag changed");
        }),
        ("nothing changed", "", r#"<div id="a" style="color:red">t</div>"#, false, |w| {
            let html = String::from_utf8(page("", r#"<div id="a" style="color:red">t</div>"#)).expect("UTF-8");
            w.html(&html, "the same HTML");
            w.html(&html.replace("<div id", " <div id"), "whitespace before an element");
        }),
        ("images", "img{border:1px solid}", r#"<img id="i" src="a.png"><canvas id="c" src="tw-canvas:1/10x10"></canvas>"#, false, |w| {
            w.body(r#"<img id="i" src="bb.png"><canvas id="c" src="tw-canvas:1/10x10"></canvas>"#, "an image's source");
            let c = w.id("c");
            w.attr(c, "src", b"tw-canvas:1/20x10", "a canvas's source");
        }),
    ]
}

#[test]
fn written_patches_are_alike() {
    let mut run = Run::new("written patches");
    let master = master_css();
    let (light, _) = themed_master(false);
    for (what, sheet, body, sanitizing, script_of) in written_cases() {
        for (master, theme) in [(&master, "master CSS"), (&light, "the renderer's UA")] {
            let html = page(sheet, body);
            let mut p = Planner::new(375.0, 667.0, sanitizing, master, &html, run.switches);
            let mut s = Script::default();
            s.step(PatchStep::Layout(375.0));
            s.step(PatchStep::Dump);
            script_of(&mut W { sheet, p: &mut p, s: &mut s, width: 375.0, hovered: false });
            run.case(375.0, 667.0, sanitizing, master, &html, &s, format!("{what} ({theme})"));
        }
    }
    let tally = run.finish();
    assert!(tally.count("update_holes refused") > 20 && tally.count("paint-only patches") > 10);
}

// ---- Generated sequences ---------------------------------------------------------------------------------------

/// A generated sheet: rules of every scope patching looks at (attribute, sibling, position, `:empty`, counter and
/// `[style]` selectors, ancestor classes, generated content, `:hover`, custom properties) and layout's kinds.
fn generated_sheet(r: &mut Lcg) -> String {
    const RULES: [&str; 26] = [
        ".a{color:red}",
        ".b{padding:3px}",
        ".on{background:#eee}",
        ".off{color:gray}",
        ".card .row{margin:2px}",
        "[data-x]{color:blue}",
        "p+p{margin-top:4px}",
        "li:nth-child(2n){color:green}",
        "div:empty{height:2px}",
        ".c::before{content:'c'}",
        ".off::after{content:'.'}",
        ".a:hover{color:blue;width:40px}",
        "ol{counter-reset:k} li::before{content:counter(k)}",
        "[style]{outline:0}",
        ".row{display:flex;gap:2px}",
        ".card{display:grid;grid-template-columns:1fr 1fr}",
        "section{position:relative}",
        ".abs{position:absolute;right:0;top:2px}",
        "table{border-collapse:collapse}",
        "td{padding:2px;border:1px solid}",
        ":root{--m:3px} .b{margin:var(--m)}",
        ".big{font-size:24px}",
        ".hidden{display:none}",
        "span.on{text-decoration:underline}",
        ".x .y{color:purple}",
        "h2{margin:4px 0}",
    ];
    let mut out = String::new();
    for _ in 0..2 + r.below(5) {
        out.push_str(RULES[r.below(RULES.len())]);
        out.push(' ');
    }
    out
}

/// A generated body: nested boxes, inlines, lists, tables, flex and grid boxes, positioned boxes, images and SVG,
/// with classes, styles, ids, keys and texts.
fn generated_body(r: &mut Lcg) -> String {
    fn attrs(r: &mut Lcg, out: &mut String, next: &mut u32) {
        if r.chance(2) {
            out.push_str(&format!(" class=\"{}\"", pick(r, &["a", "b", "c", "on", "off", "card", "row", "a b", "x", "y", "big", ""])));
        }
        if r.chance(2) {
            out.push_str(&format!(" style=\"{}\"", String::from_utf8_lossy(&style_edit(r, b""))));
        }
        if r.chance(3) {
            *next += 1;
            out.push_str(&format!(" id=\"e{next}\""));
        }
        if r.chance(6) {
            out.push_str(&format!(" data-lh-id=\"k{}\"", r.below(6)));
        }
        if r.chance(10) {
            out.push_str(" data-x=\"1\"");
        }
    }
    fn element(r: &mut Lcg, depth: u32, out: &mut String, next: &mut u32) {
        let tag = if depth > 4 {
            pick(r, &["span", "b", "i"])
        } else {
            pick(r, &["div", "div", "p", "span", "b", "section", "ul", "ol", "table", "img", "svg", "h2", "pre", "a", "div"])
        };
        match tag {
            "img" => {
                out.push_str(&format!("<img src=\"{}.png\"", pick(r, &["a", "bb", "c3"])));
                attrs(r, out, next);
                out.push('>');
                return;
            }
            "svg" => {
                out.push_str(&format!(
                    "<svg width=\"40\" height=\"20\"><rect x=\"{}\" y=\"1\" width=\"10\" height=\"5\" fill=\"{}\"/></svg>",
                    r.below(10),
                    pick(r, &["red", "blue"])
                ));
                return;
            }
            "table" => {
                out.push_str("<table");
                attrs(r, out, next);
                out.push('>');
                for _ in 0..1 + r.below(2) {
                    out.push_str("<tr>");
                    for _ in 0..1 + r.below(3) {
                        out.push_str("<td");
                        attrs(r, out, next);
                        out.push_str(&format!(">{}</td>", pick(r, &["cell", "a b", "1"])));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</table>");
                return;
            }
            _ => {}
        }
        out.push('<');
        out.push_str(tag);
        attrs(r, out, next);
        out.push('>');
        let list = tag == "ul" || tag == "ol";
        for _ in 0..r.below(4) {
            if list {
                out.push_str("<li");
                attrs(r, out, next);
                out.push_str(&format!(">{}</li>", pick(r, &["one", "two words", "x"])));
            } else if depth < 6 && r.chance(2) {
                element(r, depth + 1, out, next);
            } else {
                out.push_str(pick(r, &["text", "a few words here", "x", "ünï", "longer text that may wrap at narrow widths", " "]));
            }
        }
        out.push_str(&format!("</{tag}>"));
    }
    let mut out = String::new();
    let mut next = 0;
    for _ in 0..1 + r.below(5) {
        element(r, 0, &mut out, &mut next);
    }
    out
}

fn generated_case(r: &mut Lcg, run: &mut Run, master: &[u8], k: usize, count: usize) {
    let sheet = generated_sheet(r);
    let body = generated_body(r);
    let html = page(&sheet, &body);
    let width = *[375.0f32, 200.0, 600.0].get(k % 3).expect("a width");
    let sanitizing = k % 7 == 3;
    let mut p = Planner::new(width, 667.0, sanitizing, master, &html, run.switches);
    let mut script = Script::default();
    sequence(r, &mut p, width, count, 4, &mut script);
    run.case(width, 667.0, sanitizing, master, &html, &script, format!("generated {k}: {}", String::from_utf8_lossy(&html)));
}

#[test]
fn generated_sequences_are_patched_alike() {
    let mut run = Run::new("generated sequences");
    let master = master_css();
    let (light, _) = themed_master(false);
    let mut r = Lcg::new(0x5eed_1900);
    for k in 0..3000 {
        let master = if k % 2 == 0 { &master } else { &light };
        generated_case(&mut r, &mut run, master, k, 8);
    }
    let tally = run.finish();
    assert!(tally.count("update_from_string patched") > 5000 && tally.count("update_holes patched") > 1000);
}

// ---- The switches ----------------------------------------------------------------------------------------------

#[test]
fn the_patching_switches_are_alike() {
    let master = master_css();
    for switch in [
        "LH_NO_TEMPLATE",
        "LH_NO_STRUCTURE",
        "LH_NO_CLASS_HOLES",
        "LH_NO_CLASS_FLIP",
        "LH_NO_MATCH_MEMO",
        "LH_NO_BACKGROUND_PATCH",
        "LH_NO_PROPERTY_PATCH",
        "LH_NO_COLOR_REGIONS",
        "LH_NO_TEXT_REGIONS",
        "LH_NO_INPLACE_TEXT",
        "LH_NO_SHALLOW_WIDTH_PATCH",
        "LH_NO_KEEP_TREE",
        "LH_NO_PAINT_ONLY",
        "LH_NO_PATCH_REGIONS",
        "LH_NO_SAME_SIZE_TEXT",
        "LH_NO_LAYOUT_CACHE",
        "LH_LAYOUT_CACHE_CHECK",
        "LH_NO_STYLE_SHARING",
    ] {
        let name: &'static str = Box::leak(format!("switch {switch}").into_boxed_str());
        let mut run = Run::with_switch(name, switch);
        let mut r = Lcg::new(switch.bytes().fold(0x5eed_5100u64, |h, c| h.wrapping_mul(31).wrapping_add(u64::from(c))));
        for k in 0..120 {
            generated_case(&mut r, &mut run, &master, k, 6);
        }
        for (what, sheet, body, sanitizing, script_of) in written_cases() {
            let html = page(sheet, body);
            let mut p = Planner::new(375.0, 667.0, sanitizing, &master, &html, run.switches);
            let mut s = Script::default();
            s.step(PatchStep::Layout(375.0));
            script_of(&mut W { sheet, p: &mut p, s: &mut s, width: 375.0, hovered: false });
            run.case(375.0, 667.0, sanitizing, &master, &html, &s, what);
        }
        run.finish();
    }
}

// ---- Deep documents --------------------------------------------------------------------------------------------

/// 124 levels of boxes patched at the bottom (a text, a class, a style, an element added through the structural
/// diff) and at the top (a style every level inherits from): the diff, the restyle and the relayout recurse per
/// level on the helper's thread stacks.
#[test]
fn a_deep_document_is_patched_on_a_small_stack() {
    let stack = if cfg!(debug_assertions) { 1 << 20 } else { 512 << 10 };
    let thread = std::thread::Builder::new().stack_size(stack).spawn(|| {
        let levels = 124;
        let body = |inner: &str, class: &str, color: &str| -> String {
            let mut html = String::new();
            for k in 0..levels {
                html += &format!("<div class=\"l{}\" style=\"padding-left:1px{color}\">", k % 3);
            }
            html += &format!("<span id=\"deep\" class=\"{class}\">deep text</span>{inner}");
            html += &"</div>".repeat(levels);
            html
        };
        let sheet = ".on{color:red} .l1{margin:0}";
        let html = page(sheet, &body("", "off", ""));
        let mut run = Run::new("a deep document");
        let master = master_css();
        let mut p = Planner::new(375.0, 667.0, false, &master, &html, run.switches);
        let mut s = Script::default();
        s.step(PatchStep::Layout(375.0));
        let mut w = W { sheet, p: &mut p, s: &mut s, width: 375.0, hovered: false };
        let deep = w.id("deep");
        w.text(deep, "deep words", "a deep text");
        w.attr(deep, "class", b"on", "a deep class");
        w.body(&body("<b>added</b>", "on", ""), "an element added at the bottom");
        w.body(&body("<b>added</b>", "on", ";color:blue"), "every level's style");
        w.body(&body("<b>added</b><i>more</i>", "off", ";color:green"), "every level's style and an element");
        run.case(375.0, 667.0, false, &master, &html, &s, "124 levels");
        run.finish().count("update_from_string patched")
    });
    let patched = thread.expect("a thread").join().expect("the patches ran");
    assert!(patched >= 2);
}

/// The rarer branches the probes found unreached by the other cases.
fn rare_cases() -> Vec<Written> {
    vec![
        ("a quote in a style", "", r#"<div id="q" style="background-color:red;font-family:'a;b'">q</div>"#, false, |w| {
            let q = w.id("q");
            w.attr(q, "style", b"background-color:blue;font-family:'a;b'", "a background beside a quoted value");
            w.attr(q, "style", b"background-color:blue;font-family:'a;b", "an unclosed quote");
        }),
        (
            "states and lists in selectors",
            "input:checked + label{color:red} fieldset:disabled .d{color:gray} li:nth-child(odd of .a){color:blue} .k{margin:1px}",
            r#"<input type="checkbox" checked><label>l</label><fieldset><p class="d" id="d">d</p></fieldset><ul><li class="a">a</li><li id="b" class="k">b</li></ul>"#,
            false,
            |w| {
                let b = w.id("b");
                w.attr(b, "class", b"j", "a class under state selectors");
                w.body(r#"<input type="checkbox" checked><label>l</label><fieldset><p class="d" id="d">d2</p></fieldset><ul><li class="a">a</li><li id="b" class="j">b</li></ul>"#, "a text under state selectors");
            },
        ),
        (
            "a style read inside a list",
            ":not([style]) > .z{margin:0}",
            r#"<div><p id="z" class="z" style="color:red">z</p></div>"#,
            false,
            |w| {
                let z = w.id("z");
                w.attr(z, "style", b"color:blue", "a style a nested [style] reads");
            },
        ),
        ("a keyed template", "", r#"<div><template data-lh-id="k"></template><p data-lh-id="j">p</p></div>"#, false, |w| {
            w.body(r#"<div><p data-lh-id="j">p</p><div data-lh-id="k">d</div></div>"#, "a keyed template becoming a box");
        }),
        ("elements the sanitizer refused, changed", "", r#"<div><object data="1"><b>x</b></object><p>p</p></div>"#, true, |w| {
            w.body(r#"<div><object data="2"><b>x</b></object><p>p</p></div>"#, "a refused element's attribute");
            w.body(r#"<div><object><b>x</b></object><p>p</p></div>"#, "a refused element's attribute removed");
            w.body(r#"<div><object><i>x</i></object><p>p</p></div>"#, "a refused element's child");
            w.body(r#"<div><object><i>x</i></object><p>p2</p></div>"#, "a text beside a refused element");
        }),
        (
            "a keyed child whose diff fails",
            "[style]{outline:0}",
            r#"<ul><li data-lh-id="a" style="color:red">a</li><li data-lh-id="b">b</li></ul>"#,
            false,
            |w| {
                w.body(
                    r#"<ul><li data-lh-id="b">b</li><li data-lh-id="a" style="color:blue">a</li></ul>"#,
                    "a keyed child's style where [style] selects",
                );
            },
        ),
        ("a document-level element removed", "", r#"<div><select><option>o</option></select><p>x</p></div>"#, false, |w| {
            w.body(r#"<div><p>x</p></div>"#, "a select removed");
        }),
        (
            "attributes of retagged boxes",
            "",
            r#"<div id="g" align="center" title="a">g</div><table><tr><td id="c" bgcolor="red" width="20" title="a">c</td></tr></table>"#,
            false,
            |w| {
                w.body(r#"<div id="g" align="center" title="b">g</div><table><tr><td id="c" bgcolor="red" width="20" title="b">c</td></tr></table>"#, "boxes with presentational attributes retagged");
            },
        ),
        (
            "many class values",
            ".a{color:red}.b{color:blue}.c{color:green}.d{color:gray}.e{color:black}",
            r#"<div id="x" class="a">x</div><p>p</p>"#,
            false,
            |w| {
                let x = w.id("x");
                for value in ["b", "c", "d", "e", "a", "b"] {
                    w.attr(x, "class", value.as_bytes(), "a class among many");
                }
                w.body(r#"<div id="x" class="b">x</div><p>p</p><p>q</p>"#, "an element added between flips");
                let x = w.id("x");
                w.attr(x, "class", b"a", "a class flipped in a later epoch");
                w.attr(x, "class", b"b", "a class flipped back in a later epoch");
            },
        ),
        ("a positioned cell", "", r#"<table><tr><td id="c" style="height:10px">x</td><td>y</td></tr></table>"#, false, |w| {
            let c = w.id("c");
            w.attr(c, "style", b"position:absolute;left:18px", "a cell made absolute");
            w.attr(c, "style", b"position:absolute;left:30px", "an absolute cell's offset");
            w.attr(c, "style", b"height:10px", "a cell no longer absolute");
        }),
        (
            "a transform over an absolute box",
            "",
            r#"<div style="height:30px"></div><div id="h" style="width:100px;margin-left:20px"><div style="position:absolute;left:18px;top:3px">abs</div></div>"#,
            false,
            |w| {
                let h = w.id("h");
                w.attr(h, "style", b"width:100px;margin-left:20px;transform:translateX(0px)", "a transform added over an absolute box");
                w.attr(h, "style", b"width:100px;margin-left:20px", "a transform removed over an absolute box");
                w.attr(h, "style", b"width:100px;margin-left:20px;position:relative", "a box made the holder of an absolute box");
            },
        ),
        (
            "a fieldset's legend",
            ".a{color:red}.b{color:blue}",
            r#"<fieldset id="f" style="border:4px solid;padding:6px"><legend id="l" class="a" style="padding:2px">Legend</legend>content</fieldset>"#,
            false,
            |w| {
                let l = w.id("l");
                w.attr(l, "class", b"b", "a legend's class");
                w.attr(l, "style", b"padding:3px", "a legend's style");
                w.text(l, "Legends", "a legend's text");
            },
        ),
        (
            "flips across epochs",
            ".on{color:red}.off{color:gray}.a{color:blue}.b{color:green}.c{color:black}",
            r#"<div id="x" class="on">x</div><div id="y" class="a">y</div><div><p>p</p></div>"#,
            false,
            |w| {
                let (x, y) = (w.id("x"), w.id("y"));
                w.attr(x, "class", b"off", "a class flipped");
                w.attr(x, "class", b"on", "a class flipped back");
                w.attr(y, "class", b"b", "a class flipped");
                w.attr(y, "class", b"c", "a class flipped");
                w.body(
                    r#"<div id="x" class="on">x</div><div id="y" class="z">y</div><div><p>p</p><p>q</p></div>"#,
                    "a class through the structure",
                );
                let (x, y) = (w.id("x"), w.id("y"));
                w.attr(x, "class", b"off", "a class flipped in a later epoch");
                w.attr(y, "class", b"a", "a class not flipped before");
            },
        ),
        (
            "an attribute an ancestor selector reads",
            ".p[data-k] .c{color:red}",
            r#"<div id="p" class="p" data-k="1" title="x"><span class="c">c</span></div>"#,
            false,
            |w| {
                w.body(r#"<div id="p" class="p" data-k="1" title="y"><span class="c">c</span></div>"#, "an attribute no selector reads");
                w.body(
                    r#"<div id="p" class="p" data-k="2" title="y"><span class="c">c</span></div>"#,
                    "an attribute an ancestor selector reads",
                );
            },
        ),
        ("a script's class", "", r#"<script class="a">var x</script><div id="d">d</div>"#, false, |w| {
            let s = w.tag("script", 0);
            w.attr(s, "class", b"b", "a script's class");
        }),
        (
            "a cell's text after a media change",
            "@media (max-width:500px){td{padding:4px}}",
            r#"<table><tr><td id="c">cell text</td></tr></table>"#,
            false,
            |w| {
                w.plain(PatchStep::Media(800.0, 600.0));
                let c = w.id("c");
                w.text(c, "cell text longer", "a cell's text reworded after a media change");
                w.plain(PatchStep::Media(375.0, 667.0));
                w.text(c, "cell text much longer", "a cell's text reworded after another media change");
            },
        ),
    ]
}

#[test]
fn rarer_patches_are_alike() {
    let mut run = Run::new("rarer patches");
    let master = master_css();
    for (what, sheet, body, sanitizing, script_of) in rare_cases() {
        let html = page(sheet, body);
        let mut p = Planner::new(375.0, 667.0, sanitizing, &master, &html, run.switches);
        let mut s = Script::default();
        s.step(PatchStep::Layout(375.0));
        s.step(PatchStep::Dump);
        script_of(&mut W { sheet, p: &mut p, s: &mut s, width: 375.0, hovered: false });
        run.case(375.0, 667.0, sanitizing, &master, &html, &s, what);
    }
    run.finish();
}
