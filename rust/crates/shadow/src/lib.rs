//! TwidgetCore's shadow tree (`core/src/renderer.cpp`, its "shadow tree", "ops" and "serialisation"
//! parts): litehtml has no DOM mutation API, so the widget's validated render ops build the renderer's
//! own tree of element, text and comment nodes, and every build writes that tree as the HTML the fork
//! parses. This crate is that tree with what the ops keep beside it (the canvases they make, the
//! budgets of text, sheets and bytes, the in-place patch log, the theme variables on the root and the
//! frozen chrome) and its writer.
//!
//! The seam with the rest of the renderer: in the C++ all of this is private to `Renderer`. What the
//! ops ask of the renderer besides the tree (the backend's canvas calls and fonts, the container's
//! images, the editor, pointer capture, resize observers) goes through [`DocumentHost`] in the order
//! the C++ makes those calls, and what the renderer adds to the tree when it writes it (animated
//! values, the control being edited) comes in as [`Chrome`]. Text is bytes, as the C++ `std::string`s
//! hold it.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use twidget_canvas::{Canvas, CanvasHost, CanvasSource};
use twidget_css as css;
use twidget_json::Json;
use twidget_wire::{CanvasOp, FontMetrics, OpKind};

pub use twidget_animations::{StyleOverrides, StyleValues};

/// What ops may hand over, several times what the validator sends (10,000 ops a batch, 48 K of text,
/// 400 rules): more is refused here too.
pub const MAX_BATCH_OPS: usize = 200000;
pub const MAX_TEXT_BYTES: usize = 4 << 20;
pub const MAX_TEXT_PIECES: usize = 100000;
pub const MAX_SHEET_BLOCKS: usize = 16384;
pub const MAX_NODE_ATTRS: usize = 4096;
pub const MAX_TREE_BYTES: usize = 64 << 20;
pub const MAX_CANVASES: usize = 64;
/// The levels of a tree the ops build, attached or not (the validator caps the attached one): every
/// walk recurses over them.
pub const MAX_TREE_DEPTH: usize = 1024;
/// The nodes a renderer holds, attached or not: the validator's own bound (widget-sandbox
/// validator.ts, MAX_NODES).
pub const MAX_NODES: usize = 50000;
/// litehtml::document::max_build_depth: the fork builds no element deeper, and nothing deeper is written.
pub const MAX_BUILD_DEPTH: i32 = 128;

const VOID_TAGS: [&[u8]; 14] =
    [b"area", b"base", b"br", b"col", b"embed", b"hr", b"img", b"input", b"link", b"meta", b"param", b"source", b"track", b"wbr"];

pub const FROZEN_BADGE_CSS: &str = "[data-tw-frozen=fatal] body{opacity:.85}[data-tw-badge]{position:fixed;right:6px;bottom:6px;z-index:2147483647;\
font:11px/1.3 -apple-system,system-ui,sans-serif;color:#fff;background:rgba(60,60,60,.75);padding:2px 7px;\
border-radius:9px}";

fn is_void(tag: &[u8]) -> bool {
    VOID_TAGS.contains(&tag)
}

/// An event handler attribute, which the renderer never takes.
pub fn starts_with_on(name: &[u8]) -> bool {
    name.len() >= 2 && (name[0] == b'o' || name[0] == b'O') && (name[1] == b'n' || name[1] == b'N')
}

pub fn escape_text(s: &[u8], out: &mut Vec<u8>) {
    for &c in s {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(c),
        }
    }
}

pub fn escape_attr(s: &[u8], out: &mut Vec<u8>) {
    for &c in s {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(c),
        }
    }
}

/// A node id from an op or a query; 0, which names no node, for a number that is no int.
pub fn node_id(v: &Json) -> i64 {
    let d = v.as_number(0.0);
    if d > -2147483649.0 && d < 2147483648.0 { d as i64 } else { 0 }
}

fn text_pieces(text: &[u8]) -> usize {
    let mut pieces = 0usize;
    let mut last = -1;
    for &c in text {
        let space = i32::from(c <= 0x20);
        pieces += usize::from(space != last) + usize::from(c >= 0xE0);
        last = space;
    }
    pieces
}

fn sheet_blocks(css: &[u8]) -> usize {
    css.iter().filter(|&&c| c == b'{').count()
}

/// The `style` attribute is the inline style itself, as in the DOM: setting it replaces what the style
/// ops set, and a style op that removes a property removes it from what the attribute set.
fn set_inline_style(styles: &mut Vec<(Vec<u8>, Vec<u8>)>, text: &[u8]) {
    styles.clear();
    for d in css::parse_declarations(text) {
        if styles.len() >= MAX_NODE_ATTRS {
            break;
        }
        let value = if d.important { [&d.value[..], b" !important"].concat() } else { d.value };
        styles.push((d.name, value));
    }
}

/// Rewrites `prefers-color-scheme` media queries into width queries litehtml can evaluate
/// (`(min-width:0px)` = always, `(min-width:999999px)` = never), since litehtml knows no colour-scheme
/// feature.
pub fn rewrite_color_scheme(css_text: &[u8], dark: bool) -> Vec<u8> {
    const KEY: &[u8] = b"prefers-color-scheme";
    let mut out = Vec::with_capacity(css_text.len());
    let mut pos = 0;
    let mut i = 0;
    // The media feature's name as a whole identifier, outside strings, comments and URLs.
    while i + KEY.len() <= css_text.len() {
        let skip = css::skip_opaque(css_text, i);
        if skip != i {
            i = skip;
            continue;
        }
        if (i > 0 && css::ident_char(css_text[i - 1]))
            || !css::iequals(&css_text[i..i + KEY.len()], KEY)
            || (i + KEY.len() < css_text.len() && css::ident_char(css_text[i + KEY.len()]))
        {
            i += 1;
            continue;
        }
        out.extend_from_slice(&css_text[pos..i]);
        let mut j = i + KEY.len();
        while j < css_text.len() && matches!(css_text[j], b' ' | b'\t' | b'\n' | b':') {
            j += 1;
        }
        let mut k = j;
        while k < css_text.len() && css_text[k].is_ascii_alphabetic() {
            k += 1;
        }
        let value = css::lower(&css_text[j..k]);
        // bare `(prefers-color-scheme)` is true
        let matches = if value.is_empty() { true } else { (value == b"dark") == dark };
        out.extend_from_slice(if matches { b"min-width:0px" } else { b"min-width:999999px" });
        pos = k;
        i = k;
    }
    out.extend_from_slice(&css_text[pos.min(css_text.len())..]);
    out
}

/// One node of the tree, as the C++ `Renderer::Node` holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeKind {
    #[default]
    Element,
    Text,
    Comment,
}

/// A node's place in the document's arena: the C++'s `Node*`. A removed node's place may hold a later one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeRef(u32);

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub kind: NodeKind,
    /// The widget's node id, 0 until adopted.
    pub id: i64,
    pub tag: Vec<u8>,
    pub svg: bool,
    pub attrs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Inline style properties set by `style` ops and the style attribute.
    pub styles: Vec<(Vec<u8>, Vec<u8>)>,
    /// Text or comment data; the sheet text of a <style>; a <textarea>'s value.
    pub data: Vec<u8>,
    pub parent: Option<NodeRef>,
    pub children: Vec<NodeRef>,
}

/// What a batch changed that the fork can patch into the live document in place (the Swift renderer's
/// patch log): attributes of attached elements, and attached elements whose sole text changed. By node
/// id, as the C++ keeps it and hands the changes on: a node keeps its id while the log is valid.
#[derive(Clone, Debug)]
pub struct PatchLog {
    pub valid: bool,
    /// Each element's place and the names of its attributes that changed.
    pub attributes: BTreeMap<i64, (NodeRef, BTreeSet<Vec<u8>>)>,
    pub texts: BTreeMap<i64, NodeRef>,
}

impl Default for PatchLog {
    fn default() -> PatchLog {
        PatchLog { valid: true, attributes: BTreeMap::new(), texts: BTreeMap::new() }
    }
}

/// What the renderer adds to the tree when it writes it.
#[derive(Clone, Copy, Debug)]
pub struct Chrome<'a> {
    /// The animations' values per node id, written after the node's own style.
    pub overrides: &'a StyleOverrides,
    /// The node of the control the platform's editor shows, or 0.
    pub editing: i64,
}

/// What the ops ask of the renderer besides its tree, called where and in the order the C++ makes these
/// calls.
pub trait DocumentHost {
    /// The backend's `canvas_run`: a canvas made (its reset), resized, or drawn.
    fn canvas_run(&mut self, canvas: i32, width: i32, height: i32, ops: &[CanvasOp]);
    /// A canvas whose node was removed: the renderer forgets what it drew and the backend releases it.
    fn canvas_release(&mut self, canvas: i32);
    /// Before a canvas runs its commands: the renderer lets go of the canvas fonts it held past 64.
    fn canvas_run_begins(&mut self) {}
    /// After a run that drew something, its ops given to the backend: what the renderer keeps of a drawing canvas.
    fn canvas_drew(&mut self, _canvas: i32) {}
    /// The renderer's canvas font for a CSS `font` shorthand, `metrics` filled, or 0.
    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32;
    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32;
    /// The container's image for a `data:` source: (image id, width, height), or None.
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)>;
    /// A node left the tree for good: the renderer stops observing its size.
    fn node_forgotten(&mut self, _id: i64) {}
    /// A `prop` op set node `id`'s value: the editor takes it when it edits that node.
    fn value_set(&mut self, _id: i64, _value: &[u8]) {}
    /// A `call` op the tree does not answer itself: pointer capture, resize observers, focus, blur, selection.
    fn call(&mut self, _document: &mut Document, _id: i64, _method: &[u8], _args: &Json, _out: &mut Vec<Json>) {}
    /// The renderer was frozen: it lets go of its touches, coalesced moves and hover.
    fn frozen(&mut self) {}
    /// The violations the container made while a canvas ran (an `<img>` source it refused), for the document's
    /// list where the C++ container pushes them.
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

const HTML: NodeRef = NodeRef(0);
const HEAD: NodeRef = NodeRef(1);
const BODY: NodeRef = NodeRef(2);

fn violation(out: &mut Vec<Json>, detail: impl Into<Json>) {
    let mut m = Json::object();
    m.set("type", "violation").set("detail", detail);
    out.push(m);
}

/// The renderer's canvas host while one canvas of the document runs: the other canvases from the
/// registry, the running one from what it was when it was taken out of it (a run never changes its size).
struct Canvases<'a> {
    host: &'a mut dyn DocumentHost,
    canvases: &'a BTreeMap<i64, Canvas>,
    running: Option<(i64, i32, i32, i32)>,
    arena: &'a [Option<Node>],
    ids: &'a HashMap<i64, NodeRef>,
    aliases: &'a HashMap<i64, NodeRef>,
}

impl CanvasHost for Canvases<'_> {
    fn canvas_font(&mut self, css: &[u8], metrics: &mut FontMetrics) -> i32 {
        self.host.canvas_font(css, metrics)
    }

    fn canvas_text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.host.canvas_text_width(font, text)
    }

    fn canvas_source(&mut self, id: i64) -> Option<CanvasSource> {
        if let Some(c) = self.canvases.get(&id) {
            return Some(CanvasSource { image: 0, canvas: c.id(), w: c.width() as f32, h: c.height() as f32 });
        }
        if let Some((node, canvas, w, h)) = self.running
            && node == id
        {
            return Some(CanvasSource { image: 0, canvas, w: w as f32, h: h as f32 });
        }
        let r = self.ids.get(&id).or_else(|| self.aliases.get(&id))?;
        let n = self.arena[r.0 as usize].as_ref()?;
        if n.tag != b"img" {
            return None;
        }
        for (k, v) in &n.attrs {
            if k == b"src" && v.starts_with(b"data:") {
                let (image, w, h) = self.host.image_of(v)?;
                return Some(CanvasSource { image, canvas: 0, w, h });
            }
        }
        None
    }

    fn canvas_allowance(&mut self, id: i32) -> i64 {
        let used: i64 = self.canvases.values().filter(|c| c.id() != id).map(Canvas::pixels).sum();
        Canvas::MAX_PIXELS - used
    }

    fn canvas_path_floats(&mut self, id: i32) -> usize {
        self.canvases.values().filter(|c| c.id() != id).map(Canvas::path_floats).sum()
    }
}

/// The renderer's tree of the widget's nodes and what its ops keep beside it.
#[derive(Debug)]
pub struct Document {
    arena: Vec<Option<Node>>,
    free: Vec<u32>,
    /// Widget nodes by id. html/head/body are fixed nodes reachable through `aliases` once adopted.
    ids: HashMap<i64, NodeRef>,
    aliases: HashMap<i64, NodeRef>,
    /// By node id; a canvas's own id is its node's.
    canvases: BTreeMap<i64, Canvas>,
    tree_bytes: usize,
    text_bytes: usize,
    text_pieces: usize,
    sheet_blocks: usize,
    /// <img> elements whose src changed, for the renderer to load.
    pub pending_images: Vec<i64>,
    /// An op since the renderer cleared it changed what the document writes.
    pub document_changed: bool,
    pub patch_log: PatchLog,
    dark: bool,
    theme_received: bool,
    host_theme_vars: css::Variables,
    safe_area_vars: css::Variables,
    theme_vars: css::Variables,
    frozen: bool,
    frozen_fatal: bool,
    frozen_reason: Vec<u8>,
    /// Past the tree limit, or stopped by the renderer past its layout or name limit: nothing more is
    /// applied, and the document is written as the badge alone.
    layout_stopped: bool,
    /// Stopped by the renderer past its paint limit: its ops are no longer applied.
    pub paint_stopped: bool,
    /// Violations the renderer reports after a message, as the C++'s `pending_violations_`.
    pub pending_violations: Vec<Vec<u8>>,
    /// The renderer must build before it shows the document again.
    pub stale: bool,
}

impl Document {
    /// The tree of a new renderer: <html> holding <head> and <body>, unadopted, and the theme of its scheme.
    pub fn new(dark: bool) -> Document {
        let make = |tag: &[u8]| Some(Node { tag: tag.to_vec(), ..Node::default() });
        let mut d = Document {
            arena: vec![make(b"html"), make(b"head"), make(b"body")],
            free: Vec::new(),
            ids: HashMap::new(),
            aliases: HashMap::new(),
            canvases: BTreeMap::new(),
            tree_bytes: 0,
            text_bytes: 0,
            text_pieces: 0,
            sheet_blocks: 0,
            pending_images: Vec::new(),
            document_changed: false,
            patch_log: PatchLog::default(),
            dark,
            theme_received: false,
            host_theme_vars: Vec::new(),
            safe_area_vars: Vec::new(),
            theme_vars: Vec::new(),
            frozen: false,
            frozen_fatal: false,
            frozen_reason: Vec::new(),
            layout_stopped: false,
            paint_stopped: false,
            pending_violations: Vec::new(),
            stale: false,
        };
        d.get_mut(HEAD).parent = Some(HTML);
        d.get_mut(BODY).parent = Some(HTML);
        d.get_mut(HTML).children = vec![HEAD, BODY];
        d.safe_area_vars = css::content_safe_area_variables(&Json::Null);
        let mut scheme = Json::object();
        scheme.set("colorScheme", if dark { "dark" } else { "light" });
        d.apply_theme(&scheme);
        d
    }

    // ── nodes ──

    pub fn html(&self) -> NodeRef {
        HTML
    }

    pub fn head(&self) -> NodeRef {
        HEAD
    }

    pub fn body(&self) -> NodeRef {
        BODY
    }

    /// The node `id` names: a widget node, or a root adopted under it.
    pub fn node(&self, id: i64) -> Option<NodeRef> {
        self.ids.get(&id).or_else(|| self.aliases.get(&id)).copied()
    }

    pub fn get(&self, r: NodeRef) -> &Node {
        self.arena[r.0 as usize].as_ref().expect("a live node")
    }

    pub fn get_mut(&mut self, r: NodeRef) -> &mut Node {
        self.arena[r.0 as usize].as_mut().expect("a live node")
    }

    /// The widget's nodes, the roots aside.
    pub fn node_count(&self) -> usize {
        self.ids.len()
    }

    fn is_root(r: NodeRef) -> bool {
        r == HTML || r == HEAD || r == BODY
    }

    fn alloc(&mut self, n: Node) -> NodeRef {
        match self.free.pop() {
            Some(slot) => {
                self.arena[slot as usize] = Some(n);
                NodeRef(slot)
            }
            None => {
                self.arena.push(Some(n));
                NodeRef((self.arena.len() - 1) as u32)
            }
        }
    }

    /// Inserts `n` under `id` in place of the node there (whose place goes free), as `nodes_[id] = n`.
    fn register(&mut self, id: i64, n: Node) {
        let r = self.alloc(n);
        if let Some(old) = self.ids.insert(id, r) {
            self.release(old);
        }
    }

    fn release(&mut self, r: NodeRef) {
        if !Document::is_root(r) {
            self.arena[r.0 as usize] = None;
            self.free.push(r.0);
        }
    }

    pub fn canvas(&self, id: i64) -> Option<&Canvas> {
        self.canvases.get(&id)
    }

    /// The canvases by node id.
    pub fn canvases(&self) -> &BTreeMap<i64, Canvas> {
        &self.canvases
    }

    /// The renderer's `trim`: every canvas drops what it makes again on demand.
    pub fn trim_canvases(&mut self) {
        for canvas in self.canvases.values_mut() {
            canvas.trim();
        }
    }

    pub fn tree_bytes(&self) -> usize {
        self.tree_bytes
    }

    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }

    pub fn text_pieces(&self) -> usize {
        self.text_pieces
    }

    pub fn sheet_blocks(&self) -> usize {
        self.sheet_blocks
    }

    // ── the renderer's state the writer shows ──

    pub fn dark(&self) -> bool {
        self.dark
    }

    pub fn theme_variables(&self) -> &css::Variables {
        &self.theme_vars
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// What the frozen badge says.
    pub fn frozen_reason(&self) -> &[u8] {
        &self.frozen_reason
    }

    pub fn layout_stopped(&self) -> bool {
        self.layout_stopped
    }

    /// Frozen for good (the badge says so), rather than paused.
    pub fn frozen_fatal(&self) -> bool {
        self.frozen_fatal
    }

    /// The host's `theme` message, or the scheme's defaults; false when the variables did not change.
    pub fn apply_theme(&mut self, theme: &Json) -> bool {
        let vars = css::theme_variables(theme);
        if vars == self.host_theme_vars {
            return false;
        }
        self.host_theme_vars = vars;
        self.apply_root_variables();
        true
    }

    /// The host's `theme` message: its variables from then on, whatever the scheme.
    pub fn receive_theme(&mut self, theme: &Json) -> bool {
        self.theme_received = true;
        self.apply_theme(theme)
    }

    pub fn apply_safe_area(&mut self, inset: &Json) -> bool {
        let vars = css::content_safe_area_variables(inset);
        if vars == self.safe_area_vars {
            return false;
        }
        self.safe_area_vars = vars;
        self.apply_root_variables();
        true
    }

    fn apply_root_variables(&mut self) {
        self.theme_vars = self.host_theme_vars.clone();
        self.theme_vars.extend(self.safe_area_vars.iter().cloned());
    }

    /// The renderer's `set_theme`: false when the scheme is the same; until a `theme` message, the
    /// scheme's default variables.
    pub fn set_dark(&mut self, dark: bool) -> bool {
        if dark == self.dark {
            return false;
        }
        self.dark = dark;
        if !self.theme_received {
            let mut scheme = Json::object();
            scheme.set("colorScheme", if dark { "dark" } else { "light" });
            self.apply_theme(&scheme);
        }
        true
    }

    pub fn freeze(&mut self, reason: &[u8], fatal: bool, host: &mut dyn DocumentHost) {
        if self.frozen {
            return;
        }
        self.frozen = true;
        self.frozen_reason = reason.to_vec();
        self.frozen_fatal = fatal;
        host.frozen();
    }

    /// The renderer's stop past its layout or name limit (and the tree's own past the tree limit): a
    /// violation, the widget frozen for good, and the document the badge alone.
    pub fn stop(&mut self, violation: &[u8], reason: &[u8], host: &mut dyn DocumentHost) {
        if self.layout_stopped {
            return;
        }
        self.layout_stopped = true;
        self.pending_violations.push(violation.to_vec());
        self.freeze(reason, true, host);
        self.stale = true;
    }

    // ── the patch log ──

    pub fn attached(&self, mut n: Option<NodeRef>) -> bool {
        while let Some(r) = n {
            if r == HTML {
                return true;
            }
            n = self.get(r).parent;
        }
        false
    }

    pub fn note_attribute(&mut self, n: NodeRef, name: &[u8]) {
        if !self.patch_log.valid || !self.attached(Some(n)) {
            return;
        }
        // The attributes the Swift renderer patches; anything else, or a text node, builds.
        let node = self.get(n);
        if node.kind != NodeKind::Element || !(name == b"class" || name == b"style" || name == b"src" || node.svg) {
            self.patch_log.valid = false;
            return;
        }
        let id = node.id;
        self.patch_log.attributes.entry(id).or_insert_with(|| (n, BTreeSet::new())).1.insert(name.to_vec());
    }

    fn note_children(&mut self, parent: Option<NodeRef>, child: NodeRef) {
        let Some(parent) = parent else { return };
        if !self.patch_log.valid || !self.attached(Some(parent)) {
            return;
        }
        if self.get(child).kind != NodeKind::Text {
            self.patch_log.valid = false;
            return;
        }
        let id = self.get(parent).id;
        self.patch_log.texts.insert(id, parent);
    }

    /// The log of the ops since the last build, a fresh one in its place.
    pub fn take_patch_log(&mut self) -> PatchLog {
        std::mem::take(&mut self.patch_log)
    }

    // ── ops ──

    /// The `ops` message as the renderer applies it before it decides on a build: refused whole past the
    /// paint or layout limit or past MAX_BATCH_OPS, otherwise every op in order (apply_ops).
    pub fn receive_ops(&mut self, ops: &[Json], host: &mut dyn DocumentHost, out: &mut Vec<Json>) {
        if self.paint_stopped || self.layout_stopped {
            return;
        }
        if ops.len() > MAX_BATCH_OPS {
            violation(out, format!("ops limit: {} operations in one batch", ops.len()));
            return;
        }
        self.document_changed = false;
        self.apply_ops(ops, host, out);
    }

    /// A batch's ops in order, but for a canvas drawn from another one later in the batch: that one's
    /// node and drawing come first, as the script's calls ran.
    pub fn apply_ops(&mut self, ops: &[Json], host: &mut dyn DocumentHost, out: &mut Vec<Json>) {
        // The sandbox sends a canvas's commands as one op per canvas, the first canvas used first.
        let mut drawn: HashMap<i64, usize> = HashMap::new();
        for (i, op) in ops.iter().enumerate() {
            if op.get("op").as_str() == b"canvas" {
                drawn.entry(node_id(op.get("id"))).or_insert(i);
            }
        }
        if drawn.len() < 2 {
            for op in ops {
                self.apply_op(op, host, out);
            }
            return;
        }
        let mut done = vec![false; ops.len()];
        for i in 0..ops.len() {
            self.apply_in_order(ops, &drawn, &mut done, i, 0, host, out);
        }
    }

    fn apply_in_order(
        &mut self,
        ops: &[Json],
        drawn: &HashMap<i64, usize>,
        done: &mut [bool],
        i: usize,
        depth: usize,
        host: &mut dyn DocumentHost,
        out: &mut Vec<Json>,
    ) {
        if done[i] {
            return;
        }
        done[i] = true;
        // A chain of sources is no longer than the canvases a renderer holds; past that the ops apply in their own order.
        if depth < MAX_CANVASES && ops[i].get("op").as_str() == b"canvas" {
            for cmd in ops[i].get("cmds").arr() {
                if cmd.at(1).as_str() != b"drawImage" {
                    continue;
                }
                let Some(source) = cmd.at(2).find("n") else { continue };
                let reference = node_id(source);
                let Some(&j) = drawn.get(&reference) else { continue };
                if j <= i || done[j] {
                    continue;
                }
                // The source's node and its size (a canvas made by script arrives just before its drawing);
                // where it is inserted stays in its place.
                for k in i + 1..j {
                    let kind = ops[k].get("op").as_str();
                    if !done[k] && node_id(ops[k].get("id")) == reference && (kind == b"el" || kind == b"attr") {
                        self.apply_in_order(ops, drawn, done, k, depth + 1, host, out);
                    }
                }
                self.apply_in_order(ops, drawn, done, j, depth + 1, host, out);
            }
        }
        self.apply_op(&ops[i], host, out);
    }

    pub fn apply_op(&mut self, op: &Json, host: &mut dyn DocumentHost, out: &mut Vec<Json>) {
        let kind = op.get("op").as_str();
        let id = node_id(op.get("id"));
        let text = |k: &str| op.get(k).as_str();
        if !self.hold(op, host) {
            return;
        }
        // What changes the serialised document; `call` marks it itself when it toggles an attribute.
        if kind != b"canvas" && kind != b"call" && !(kind == b"prop" && (text("name") == b"scrollTop" || text("name") == b"scrollLeft")) {
            self.document_changed = true;
        }
        if id == 0 && (kind == b"el" || kind == b"text" || kind == b"comment") {
            return;
        }
        if kind == b"el" {
            let adopt_name = text("adopt");
            if !adopt_name.is_empty() {
                let n = match adopt_name {
                    b"html" => Some(HTML),
                    b"head" => Some(HEAD),
                    b"body" => Some(BODY),
                    _ => None,
                };
                // A live node keeps its id, as below: its parent and its children point at it.
                let Some(n) = n else { return };
                if self.node(id).is_some_and(|live| live != n) {
                    return;
                }
                self.patch_log.valid = false;
                self.adopt(id, n, op.get("attrs"));
                return;
            }
            // the validator never reuses a live id; keep the first
            if self.node(id).is_some() || self.ids.len() >= MAX_NODES {
                return;
            }
            if !css::is_tag_name(text("tag")) {
                violation(out, "element name");
                return;
            }
            let mut n =
                Node { kind: NodeKind::Element, id, tag: css::lower(text("tag")), svg: op.get("ns").as_str() == b"svg", ..Node::default() };
            for pair in op.get("attrs").arr() {
                if !pair.at(0).is_string() || !pair.at(1).is_string() {
                    continue;
                }
                let (name, value) = (pair.at(0).as_str(), pair.at(1).as_str());
                if starts_with_on(name) {
                    continue;
                }
                if name == b"style" {
                    set_inline_style(&mut n.styles, value);
                } else if n.attrs.len() < MAX_NODE_ATTRS {
                    n.attrs.push((name.to_vec(), value.to_vec()));
                }
            }
            if n.tag == b"img" && !n.svg {
                self.pending_images.push(id);
            }
            self.register(id, n);
            return;
        }
        if kind == b"text" || kind == b"comment" {
            if self.node(id).is_some() || self.ids.len() >= MAX_NODES {
                return;
            }
            if kind == b"text" && !self.take_text(b"", text("data")) {
                violation(out, "text limit");
                return;
            }
            let n = Node {
                kind: if kind == b"text" { NodeKind::Text } else { NodeKind::Comment },
                id,
                data: text("data").to_vec(),
                ..Node::default()
            };
            self.register(id, n);
            return;
        }
        let Some(n) = self.node(id) else { return };
        let is_root = Document::is_root(n);
        match kind {
            b"attr" => {
                let name = text("name");
                if starts_with_on(name) || !css::is_attribute_name(name) {
                    violation(out, [&b"attribute "[..], &name[..name.len().min(64)]].concat());
                    return;
                }
                let node = self.get(n);
                if name != b"style" && node.attrs.len() >= MAX_NODE_ATTRS && !node.attrs.iter().any(|(a, _)| a == name) {
                    violation(out, "attribute count");
                    return;
                }
                self.note_attribute(n, &css::lower(name));
                let value = text("value");
                let node = self.get_mut(n);
                if name == b"style" {
                    set_inline_style(&mut node.styles, value);
                    return;
                }
                let mut found = false;
                for (a, v) in &mut node.attrs {
                    if a == name {
                        *v = value.to_vec();
                        found = true;
                    }
                }
                if !found {
                    node.attrs.push((name.to_vec(), value.to_vec()));
                }
                if node.tag == b"img" && !node.svg && name == b"src" {
                    let nid = node.id;
                    self.pending_images.push(nid);
                }
                // Setting a canvas's width or height clears its bitmap, as in a browser.
                let node = self.get(n);
                if node.tag == b"canvas" && (name == b"width" || name == b"height") {
                    let nid = node.id;
                    if let Some(mut canvas) = self.canvases.remove(&nid) {
                        let (mut w, mut h) = (canvas.width(), canvas.height());
                        let size = twidget_cstd::atoi(value).max(0);
                        if name == b"width" {
                            w = size;
                        } else {
                            h = size;
                        }
                        let mut ops = Vec::new();
                        {
                            let mut adapter = Canvases {
                                host: &mut *host,
                                canvases: &self.canvases,
                                running: Some((nid, canvas.id(), canvas.width(), canvas.height())),
                                arena: &self.arena,
                                ids: &self.ids,
                                aliases: &self.aliases,
                            };
                            canvas.resize(&mut adapter, w, h, &mut ops);
                        }
                        host.canvas_run(canvas.id(), canvas.width(), canvas.height(), &ops);
                        self.canvases.insert(nid, canvas);
                    }
                }
            }
            b"rattr" => {
                if self.attached(Some(n)) {
                    self.patch_log.valid = false;
                }
                let name = text("name");
                let node = self.get_mut(n);
                if name == b"style" {
                    node.styles.clear();
                }
                node.attrs.retain(|(a, _)| a != name);
            }
            b"data" => {
                let node = self.get(n);
                if node.kind == NodeKind::Element {
                    return;
                }
                if node.kind == NodeKind::Text {
                    let old = node.data.clone();
                    if !self.take_text(&old, text("data")) {
                        violation(out, "text limit");
                        return;
                    }
                }
                let parent = self.get(n).parent;
                self.note_children(parent, n);
                self.get_mut(n).data = text("data").to_vec();
            }
            b"ins" => {
                if is_root {
                    return;
                }
                let parent = node_id(op.get("parent"));
                let old_parent = self.get(n).parent;
                self.note_children(old_parent, n);
                self.detach(n);
                if parent == 0 {
                    return;
                }
                let Some(p) = self.node(parent) else { return };
                if self.get(p).kind != NodeKind::Element {
                    return;
                }
                self.note_children(Some(p), n);
                // no cycles: the validator forbids them, but a hostile batch must not hang the renderer
                let mut level = 1usize;
                let mut a = Some(p);
                while let Some(r) = a {
                    if r == n {
                        return;
                    }
                    a = self.get(r).parent;
                    level += 1;
                }
                let mut height = 0usize;
                let mut below: Vec<(NodeRef, usize)> = vec![(n, 1)];
                while let Some((at, levels)) = below.pop() {
                    height = height.max(levels);
                    for &c in &self.get(at).children {
                        below.push((c, levels + 1));
                    }
                }
                if level + height - 1 > MAX_TREE_DEPTH {
                    violation(out, "tree depth");
                    return;
                }
                self.get_mut(n).parent = Some(p);
                if let Some(before) = op.find("before")
                    && before.is_number()
                    && let Some(b) = self.node(node_id(before))
                    && self.get(b).parent == Some(p)
                {
                    let children = &mut self.get_mut(p).children;
                    let at = children.iter().position(|&c| c == b).unwrap_or(children.len());
                    children.insert(at, n);
                    return;
                }
                self.get_mut(p).children.push(n);
            }
            b"rm" => {
                if is_root {
                    return;
                }
                let parent = self.get(n).parent;
                self.note_children(parent, n);
                self.detach(n);
                self.forget(n, host);
            }
            b"style" => {
                let raw = text("name");
                let name = if raw.starts_with(b"--") { raw.to_vec() } else { css::lower(raw) };
                if !css::is_property_name(&name) {
                    violation(out, "property name");
                    return;
                }
                // The host's theme variables on the root are not the widget's to change (it can shadow them on its own elements).
                if n == HTML && css::is_theme_variable(&name) {
                    return;
                }
                self.note_attribute(n, b"style");
                let mut value = text("value").to_vec();
                if op.get("important").as_bool(false) && !value.is_empty() {
                    value.extend_from_slice(b" !important");
                }
                let node = self.get_mut(n);
                let at = node.styles.iter().position(|(s, _)| *s == name);
                if value.is_empty() {
                    if let Some(at) = at {
                        node.styles.remove(at);
                    }
                } else if let Some(at) = at {
                    node.styles[at].1 = value;
                } else if node.styles.len() < MAX_NODE_ATTRS {
                    node.styles.push((name, value));
                } else {
                    violation(out, "declaration count");
                }
            }
            b"css" => {
                if self.attached(Some(n)) {
                    self.patch_log.valid = false;
                }
                if self.get(n).tag == b"style" {
                    let sheet = text("text");
                    let blocks = self.sheet_blocks.wrapping_sub(sheet_blocks(&self.get(n).data)).wrapping_add(sheet_blocks(sheet));
                    if blocks > MAX_SHEET_BLOCKS && blocks > self.sheet_blocks {
                        violation(out, "stylesheet limit");
                        return;
                    }
                    self.sheet_blocks = blocks;
                    self.get_mut(n).data = sheet.to_vec();
                }
            }
            b"prop" => self.prop(n, op, host, out),
            b"call" => {
                let method = text("method");
                match method {
                    b"showModal" | b"show" | b"showPopover" => {
                        if self.get(n).attrs.iter().any(|(a, _)| a == b"open") {
                            return;
                        }
                        self.get_mut(n).attrs.push((b"open".to_vec(), Vec::new()));
                        self.document_changed = true;
                        if self.attached(Some(n)) {
                            self.patch_log.valid = false;
                        }
                    }
                    b"close" | b"hidePopover" => {
                        self.get_mut(n).attrs.retain(|(a, _)| a != b"open");
                        self.document_changed = true;
                        if self.attached(Some(n)) {
                            self.patch_log.valid = false;
                        }
                    }
                    _ => {
                        let nid = self.get(n).id;
                        host.call(self, nid, method, op.get("args"), out);
                    }
                }
            }
            b"canvas" => {
                let node = self.get(n);
                if node.tag == b"canvas" && !node.svg {
                    let nid = node.id;
                    // The document shows a canvas through its src, which names its size: a new or resized
                    // bitmap changes the document.
                    let (w, h) = self.canvases.get(&nid).map_or((-1, -1), |c| (c.width(), c.height()));
                    if self.canvas_for(n, host) {
                        self.run_canvas(nid, op.get("cmds"), host);
                        let c = &self.canvases[&nid];
                        if c.width() != w || c.height() != h {
                            self.document_changed = true;
                            self.note_attribute(n, b"src");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn prop(&mut self, n: NodeRef, op: &Json, host: &mut dyn DocumentHost, out: &mut Vec<Json>) {
        let name = op.get("name").as_str();
        let value = op.get("value");
        fn set_attr(node: &mut Node, a: &[u8], v: &[u8]) {
            for (k, x) in &mut node.attrs {
                if k == a {
                    *x = v.to_vec();
                    return;
                }
            }
            node.attrs.push((a.to_vec(), v.to_vec()));
        }
        fn remove_attr(node: &mut Node, a: &[u8]) {
            node.attrs.retain(|(k, _)| k != a);
        }
        match name {
            b"value" => {
                let v: Vec<u8> = if value.is_string() {
                    value.as_str().to_vec()
                } else if value.is_null() {
                    Vec::new()
                } else {
                    value.dump()
                };
                if !value.is_string() {
                    self.tree_bytes = self.tree_bytes.wrapping_add(v.len());
                }
                let textarea = self.get(n).tag == b"textarea";
                if textarea {
                    let old = self.get(n).data.clone();
                    if !self.take_text(&old, &v) {
                        violation(out, "text limit");
                        return;
                    }
                    // A textarea's value is its text, a hole when it is the only content.
                    if !self.get(n).children.is_empty() {
                        self.patch_log.valid = false;
                    } else if self.patch_log.valid && self.attached(Some(n)) {
                        let id = self.get(n).id;
                        self.patch_log.texts.insert(id, n);
                    }
                    let children = std::mem::take(&mut self.get_mut(n).children);
                    self.get_mut(n).data = v.clone();
                    for c in children {
                        self.get_mut(c).parent = None;
                    }
                } else {
                    self.note_attribute(n, b"value");
                    set_attr(self.get_mut(n), b"value", &v);
                }
                let nid = self.get(n).id;
                host.value_set(nid, &v);
            }
            b"checked" | b"selected" | b"indeterminate" => {
                if self.attached(Some(n)) {
                    self.patch_log.valid = false;
                }
                if value.as_bool(false) {
                    set_attr(self.get_mut(n), name, b"");
                } else {
                    remove_attr(self.get_mut(n), name);
                }
            }
            b"selectedIndex" => {
                if self.attached(Some(n)) {
                    self.patch_log.valid = false;
                }
                let index = value.as_int(-1);
                let mut i = 0;
                let children = self.get(n).children.clone();
                for c in children {
                    let child = self.get_mut(c);
                    if child.tag != b"option" {
                        continue;
                    }
                    child.attrs.retain(|(k, _)| k != b"selected");
                    if i == index {
                        child.attrs.push((b"selected".to_vec(), Vec::new()));
                    }
                    i += 1;
                }
            }
            // scrollTop/scrollLeft: litehtml has no scrolling boxes to move
            _ => {}
        }
    }

    fn adopt(&mut self, id: i64, n: NodeRef, attrs: &Json) {
        let old = self.get(n).id;
        if old != 0 && old != id {
            self.aliases.remove(&old);
        }
        self.get_mut(n).id = id;
        // a non-owning alias: the fixed nodes live in the arena's first places
        if let Some(r) = self.ids.remove(&id) {
            self.release(r);
        }
        self.aliases.insert(id, n);
        let node = self.get_mut(n);
        for pair in attrs.arr() {
            if !pair.at(0).is_string() || !pair.at(1).is_string() || starts_with_on(pair.at(0).as_str()) {
                continue;
            }
            let (name, value) = (pair.at(0).as_str(), pair.at(1).as_str());
            if name == b"style" {
                set_inline_style(&mut node.styles, value);
                continue;
            }
            let mut found = false;
            for (a, v) in &mut node.attrs {
                if a == name {
                    *v = value.to_vec();
                    found = true;
                }
            }
            if !found && node.attrs.len() < MAX_NODE_ATTRS {
                node.attrs.push((name.to_vec(), value.to_vec()));
            }
        }
    }

    fn detach(&mut self, n: NodeRef) {
        let Some(p) = self.get(n).parent else { return };
        self.get_mut(p).children.retain(|&c| c != n);
        self.get_mut(n).parent = None;
    }

    fn take_text(&mut self, old: &[u8], fresh: &[u8]) -> bool {
        let bytes = self.text_bytes.wrapping_sub(old.len()).wrapping_add(fresh.len());
        let pieces = self.text_pieces.wrapping_sub(text_pieces(old)).wrapping_add(text_pieces(fresh));
        if (bytes > MAX_TEXT_BYTES && bytes > self.text_bytes) || (pieces > MAX_TEXT_PIECES && pieces > self.text_pieces) {
            return false;
        }
        self.text_bytes = bytes;
        self.text_pieces = pieces;
        true
    }

    fn hold(&mut self, op: &Json, host: &mut dyn DocumentHost) -> bool {
        if self.layout_stopped {
            return false;
        }
        let mut bytes = 0usize;
        let mut count = |value: &Json| {
            if value.is_string() {
                bytes += value.as_str().len();
            }
        };
        count(op.get("data"));
        count(op.get("text"));
        count(op.get("name"));
        count(op.get("value"));
        for pair in op.get("attrs").arr() {
            count(pair.at(0));
            count(pair.at(1));
        }
        if bytes > MAX_TREE_BYTES - self.tree_bytes.min(MAX_TREE_BYTES) {
            // The count only grows with what ops bring: what the nodes hold now, before the widget is stopped for it.
            let mut held = 0usize;
            let add = |n: &Node, held: &mut usize| {
                *held += n.data.len();
                for (name, value) in &n.attrs {
                    *held += name.len() + value.len();
                }
                for (name, value) in &n.styles {
                    *held += name.len() + value.len();
                }
            };
            for r in self.ids.values() {
                add(self.get(*r), &mut held);
            }
            add(self.get(HTML), &mut held);
            add(self.get(HEAD), &mut held);
            add(self.get(BODY), &mut held);
            self.tree_bytes = held;
            if held > MAX_TREE_BYTES / 4 * 3 || bytes > MAX_TREE_BYTES / 4 {
                let detail = format!("tree limit: {held} bytes of text, attributes and styles, {bytes} more in one op");
                self.stop(detail.as_bytes(), b"Document limit exceeded", host);
                return false;
            }
        }
        self.tree_bytes += bytes;
        true
    }

    fn forget(&mut self, n: NodeRef, host: &mut dyn DocumentHost) {
        let id = self.get(n).id;
        self.forget_canvas(id, host);
        host.node_forgotten(id);
        let node = self.get(n);
        if node.kind == NodeKind::Text || node.tag == b"textarea" {
            let old = node.data.clone();
            self.take_text(&old, b"");
        }
        let node = self.get(n);
        if node.tag == b"style" {
            self.sheet_blocks = self.sheet_blocks.wrapping_sub(sheet_blocks(&node.data));
        }
        let children = std::mem::take(&mut self.get_mut(n).children);
        for c in children {
            self.get_mut(c).parent = None;
            self.forget(c, host);
        }
        // deletes n
        if let Some(r) = self.ids.remove(&id) {
            self.release(r);
        }
    }

    // ── canvases ──

    /// The canvas of node `n`, made at the size its attributes name; false past MAX_CANVASES.
    fn canvas_for(&mut self, n: NodeRef, host: &mut dyn DocumentHost) -> bool {
        let id = self.get(n).id;
        if self.canvases.contains_key(&id) {
            return true;
        }
        if self.canvases.len() >= MAX_CANVASES {
            return false;
        }
        let size = |name: &[u8], fallback: i32| {
            for (k, v) in &self.get(n).attrs {
                let t = css::trim(v);
                if k == name && t.first().is_some_and(u8::is_ascii_digit) {
                    return twidget_cstd::atoi(&t);
                }
            }
            fallback
        };
        let (w, h) = (size(b"width", 300), size(b"height", 150));
        let canvas = {
            let mut adapter = Canvases {
                host: &mut *host,
                canvases: &self.canvases,
                running: None,
                arena: &self.arena,
                ids: &self.ids,
                aliases: &self.aliases,
            };
            Canvas::new(id as i32, w, h, &mut adapter)
        };
        let (cid, cw, ch) = (canvas.id(), canvas.width(), canvas.height());
        self.canvases.insert(id, canvas);
        host.canvas_run(cid, cw, ch, &[CanvasOp { kind: OpKind::Reset, ..CanvasOp::default() }]);
        true
    }

    fn run_canvas(&mut self, id: i64, cmds: &Json, host: &mut dyn DocumentHost) {
        // Fonts are held across one batch at most, so a widget cycling through font strings cannot grow this.
        host.canvas_run_begins();
        let Some(mut canvas) = self.canvases.remove(&id) else { return };
        let mut ops = Vec::new();
        {
            let mut adapter = Canvases {
                host: &mut *host,
                canvases: &self.canvases,
                running: Some((id, canvas.id(), canvas.width(), canvas.height())),
                arena: &self.arena,
                ids: &self.ids,
                aliases: &self.aliases,
            };
            canvas.run(&mut adapter, cmds, &mut ops);
        }
        self.pending_violations.extend(host.take_violations());
        let (cid, w, h) = (canvas.id(), canvas.width(), canvas.height());
        self.canvases.insert(id, canvas);
        if ops.is_empty() {
            return;
        }
        host.canvas_run(cid, w, h, &ops);
        host.canvas_drew(cid);
    }

    fn forget_canvas(&mut self, id: i64, host: &mut dyn DocumentHost) {
        if let Some(canvas) = self.canvases.remove(&id) {
            host.canvas_release(canvas.id());
        }
    }

    // ── serialisation ──

    /// The document as the HTML the fork parses.
    pub fn serialize(&self, chrome: &Chrome) -> Vec<u8> {
        let mut out = Vec::with_capacity(4096);
        out.extend_from_slice(b"<!DOCTYPE html>");
        if self.layout_stopped {
            // Past the layout budget nothing of the widget's is laid out again, its stylesheets neither: the badge alone.
            out.extend_from_slice(b"<html data-tw-frozen=\"fatal\"><head><style>");
            out.extend_from_slice(FROZEN_BADGE_CSS.as_bytes());
            out.extend_from_slice(b"</style></head><body><div data-tw-badge=\"fatal\">");
            escape_text(&self.frozen_reason, &mut out);
            out.extend_from_slice(b"</div></body></html>");
            return out;
        }
        // the frozen chrome: html attribute, badge style in head, badge in body (renderer elements, never in the widget's table)
        let html = self.get(HTML);
        out.extend_from_slice(b"<html");
        if html.id != 0 {
            out.extend_from_slice(format!(" data-twid=\"{}\"", html.id).as_bytes());
        }
        if self.frozen {
            out.extend_from_slice(if self.frozen_fatal { b" data-tw-frozen=\"fatal\"" } else { b" data-tw-frozen=\"paused\"" });
        }
        let mut styled = !html.styles.is_empty() || !self.theme_vars.is_empty() || chrome.overrides.contains_key(&html.id);
        for (name, value) in &html.attrs {
            if name == b"style" {
                styled = true;
                continue;
            }
            if css::iequals(name, b"data-twid") || css::iequals(name, b"data-lh-id") || css::iequals(name, b"style") {
                continue;
            }
            if !css::is_attribute_name(name) {
                continue;
            }
            out.push(b' ');
            out.extend_from_slice(name);
            out.extend_from_slice(b"=\"");
            escape_attr(value, &mut out);
            out.push(b'"');
        }
        if styled {
            out.extend_from_slice(b" style=\"");
            self.write_style(HTML, chrome, &mut out);
            out.push(b'"');
        }
        out.push(b'>');
        for &c in &html.children {
            if c == BODY && self.frozen {
                // badge before the body's own children would shift them; append it inside instead
                let mut body = Vec::new();
                self.serialize_node(c, chrome, &mut body, 2);
                if let Some(close) = body.windows(7).rposition(|w| w == b"</body>") {
                    let mut badge = Vec::new();
                    badge.extend_from_slice(if self.frozen_fatal {
                        b"<div data-tw-badge=\"fatal\">"
                    } else {
                        b"<div data-tw-badge=\"paused\">"
                    });
                    escape_text(&self.frozen_reason, &mut badge);
                    badge.extend_from_slice(b"</div>");
                    body.splice(close..close, badge);
                }
                out.extend_from_slice(&body);
            } else if c == HEAD && self.frozen {
                let mut head = Vec::new();
                self.serialize_node(c, chrome, &mut head, 2);
                if let Some(close) = head.windows(7).rposition(|w| w == b"</head>") {
                    let style = [&b"<style>"[..], FROZEN_BADGE_CSS.as_bytes(), b"</style>"].concat();
                    head.splice(close..close, style);
                }
                out.extend_from_slice(&head);
            } else {
                self.serialize_node(c, chrome, &mut out, 2);
            }
        }
        out.extend_from_slice(b"</html>");
        out
    }

    fn serialize_node(&self, r: NodeRef, chrome: &Chrome, out: &mut Vec<u8>, level: i32) {
        let n = self.get(r);
        match n.kind {
            NodeKind::Comment => return,
            NodeKind::Text => {
                escape_text(&n.data, out);
                return;
            }
            NodeKind::Element => {}
        }
        // never forwarded by the validator; belt and braces
        if n.tag == b"script" || n.tag == b"template" {
            return;
        }
        if level > MAX_BUILD_DEPTH {
            return;
        }
        if !css::is_tag_name(&n.tag) {
            return;
        }
        out.push(b'<');
        out.extend_from_slice(&n.tag);
        if n.id != 0 {
            out.extend_from_slice(b" data-twid=\"");
            out.extend_from_slice(n.id.to_string().as_bytes());
            out.push(b'"');
        }
        // The fork lays a <canvas> out as an image: its source is the backend's bitmap.
        let canvas = if n.tag == b"canvas" && !n.svg { self.canvases.get(&n.id) } else { None };
        if let Some(c) = canvas {
            out.extend_from_slice(format!(" src=\"tw-canvas:{}/{}x{}\"", c.id(), c.width(), c.height()).as_bytes());
        }
        let edited = n.id != 0 && n.id == chrome.editing;
        let mut styled = !n.styles.is_empty() || chrome.overrides.contains_key(&n.id) || edited;
        for (name, value) in &n.attrs {
            if name == b"style" {
                styled = true;
                continue;
            }
            // The parser folds the case of names and keeps the first of two: the node's id, the fork's key and the style are not the widget's to write.
            if css::iequals(name, b"data-twid") || css::iequals(name, b"data-lh-id") || css::iequals(name, b"style") {
                continue;
            }
            // A canvas's width and height are its natural size, which `src` names: on the fork's image they would be its style.
            if (canvas.is_some() && (name == b"src" || name == b"width" || name == b"height")) || !css::is_attribute_name(name) {
                continue;
            }
            // The platform's editor shows the placeholder of the control being edited.
            if edited && name == b"placeholder" {
                continue;
            }
            out.push(b' ');
            out.extend_from_slice(name);
            out.extend_from_slice(b"=\"");
            escape_attr(value, out);
            out.push(b'"');
        }
        if styled {
            out.extend_from_slice(b" style=\"");
            self.write_style(r, chrome, out);
            out.push(b'"');
        }
        out.push(b'>');
        if !n.svg && is_void(&n.tag) {
            return;
        }
        if n.tag == b"style" {
            // A sheet is raw text to the parser in HTML content only (not in svg, nor in a select): no `<` of it may open a tag.
            let sheet = css::resolve_light_dark(&rewrite_color_scheme(&n.data, self.dark), self.dark, 0);
            let mut i = 0;
            while i < sheet.len() {
                let end = css::skip_opaque(&sheet, i);
                if end == i {
                    if sheet[i..].starts_with(b"<!--") {
                        i += 4;
                        continue;
                    }
                    out.push(sheet[i]);
                    i += 1;
                    if out.last() == Some(&b'<') && i < sheet.len() && sheet[i] != b'=' {
                        out.push(b' ');
                    }
                }
                while i < end {
                    let escaped = sheet[i] == b'\\' && i + 1 < end;
                    if escaped && sheet[i + 1] != b'<' {
                        out.push(sheet[i]);
                        i += 1;
                    } else if escaped {
                        i += 1;
                    }
                    if sheet[i] == b'<' {
                        out.extend_from_slice(b"\\3c ");
                    } else {
                        out.push(sheet[i]);
                    }
                    i += 1;
                }
            }
        } else if n.tag == b"textarea" {
            // The parser drops a newline right after <textarea>: a value that starts with one needs a second.
            if n.data.first() == Some(&b'\n') {
                out.push(b'\n');
            }
            escape_text(&n.data, out);
            for &c in &n.children {
                self.serialize_node(c, chrome, out, level + 1);
            }
        } else {
            for &c in &n.children {
                self.serialize_node(c, chrome, out, level + 1);
            }
        }
        out.extend_from_slice(b"</");
        out.extend_from_slice(&n.tag);
        out.push(b'>');
    }

    /// The style attribute of `r` as written: its own declarations, the theme on the root, then the
    /// animated values after the marker. Escaped once; the same text goes to the fork's style holes.
    pub fn write_style(&self, r: NodeRef, chrome: &Chrome, out: &mut Vec<u8>) {
        let n = self.get(r);
        let mut plain: Vec<u8> = Vec::new();
        let dark = self.dark;
        let mut declare = |name: &[u8], value: &[u8]| {
            if plain.last().is_some_and(|&c| c != b';') {
                plain.push(b';');
            }
            plain.extend_from_slice(name);
            plain.push(b':');
            plain.extend_from_slice(&css::resolve_light_dark(value, dark, 0));
        };
        for (name, value) in &n.styles {
            if css::is_property_name(name) {
                declare(name, value);
            }
        }
        // Last and important: neither the widget's stylesheets nor its own style on the root replace them.
        if r == HTML {
            for (name, value) in &self.theme_vars {
                declare(name, &[&value[..], b" !important"].concat());
            }
        }
        let animated = if n.id != 0 { chrome.overrides.get(&n.id) } else { None };
        let edited = n.id != 0 && n.id == chrome.editing;
        if animated.is_some() || edited {
            // The fork records nothing after this marker as declared, so animated values never feed back
            // into the next scan as the element's own.
            declare(b"-lh-anim", b"1");
            if let Some(values) = animated {
                for (name, value) in values {
                    if css::is_property_name(name) {
                        declare(name, value);
                    }
                }
            }
            // The platform's editor shows the text of the control being edited.
            if edited {
                declare(b"color", b"transparent");
            }
        }
        escape_attr(&plain, out);
    }
}
