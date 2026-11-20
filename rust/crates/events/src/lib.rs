//! TwidgetCore's input (`core/src/renderer.cpp`, its "events", "focus" and "text selection" parts): the
//! host's pointers, hover, wheel and keys in, the widget's events out (with the listeners the sandbox
//! named, the presses it prevented and the coalesced moves of a frame), the gesture claim and the
//! arbiter's verdicts (widget-sandbox's gesture policy, `twidget_gesture`), pointer capture, the
//! `click` default action on a `<details>`, `<img>` load and error, `ResizeObserver`'s reports, focus
//! and the platform's editor over a text control, and text selection under `user-select: text`.
//!
//! The seam with the rest of the renderer: in the C++ all of this is private to `Renderer`. The tree is
//! `twidget_shadow`'s [`Document`], read and changed as the C++ reads and changes its nodes. What
//! layout and style answer (hit testing, boxes, the cascade's `touch-action` and `user-select`, the
//! mouse state behind `:hover` and `:active`, the cursor, the text as laid out, an edited control's
//! style) and what the rest of the renderer does (builds, restyles, the clock, the queue of events it
//! made itself) goes through [`EventHost`], in the order the C++ asks it. A build the input asks for
//! runs in the host; when it rebuilt, the editor follows the document here, as the C++'s `rebuild`
//! ends; when it froze the renderer, the input it held goes as `freeze` lets it go.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};

use twidget_css as css;
use twidget_gesture::{
    BubbleAction, Gesture, GestureArbiter, GestureEvent, GestureVerdict, PointerKind, TOUCH_SLOP_PX, WidgetGestureClaim,
};
use twidget_json::Json;
use twidget_shadow::{Document, DocumentHost, NodeKind, NodeRef};
use twidget_wire::{Affine, CanvasOp, Cmd, Color, Command, FontMetrics, Rect};

/// Sent whether or not the widget listens. The one-off animation events are among them because a page's
/// first build starts its animations before its script says what it listens for; animationiteration
/// repeats, and stays opt-in.
pub const ALWAYS_FORWARD: [&[u8]; 7] = [b"click", b"load", b"error", b"toggle", b"animationstart", b"animationend", b"transitionend"];

/// Every event the widget may get (profile 1, `tables.events` in widget-sandbox's profile.json): no keys,
/// focus, input, touch, wheel or scroll; dblclick and contextmenu only as the gesture policy decides.
pub const PROFILE_EVENTS: [&[u8]; 25] = [
    b"click",
    b"pointerdown",
    b"pointerup",
    b"pointermove",
    b"pointerenter",
    b"pointerleave",
    b"pointerover",
    b"pointerout",
    b"pointercancel",
    b"dblclick",
    b"contextmenu",
    b"mousedown",
    b"mouseup",
    b"mousemove",
    b"mouseenter",
    b"mouseleave",
    b"mouseover",
    b"mouseout",
    b"load",
    b"error",
    b"toggle",
    b"animationstart",
    b"animationend",
    b"animationiteration",
    b"transitionend",
];

/// A script's focus() takes the keyboard only this soon after the user's input (as Swift's scriptFocusWindow).
pub const SCRIPT_FOCUS_S: f64 = 1.0;

/// Past this a selectable run's carets are spread evenly over its width rather than measured: a press
/// or a move must not shape a long unbroken run once per probe.
pub const MAX_MEASURED_RUN: usize = 4096;

/// Move-type events: only the latest one per type and pointer goes out, once per frame.
fn coalesced(t: &[u8]) -> bool {
    t == b"pointermove" || t == b"mousemove" || t == b"touchmove" || t == b"wheel"
}

/// The bytes a C string holds: up to the first NUL, as `c_str()` hands them on.
fn c_str(s: &[u8]) -> &[u8] {
    s.iter().position(|&c| c == 0).map_or(s, |n| &s[..n])
}

/// `std::to_string(id) + suffix`, a key of the prevented presses.
fn key_of(id: i64, suffix: &[u8]) -> Vec<u8> {
    let mut k = id.to_string().into_bytes();
    k.extend_from_slice(suffix);
    k
}

/// A pointer from its press on: where it went down and is now, what it pressed, what captured it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Touch {
    pub x0: f64,
    pub y0: f64,
    pub x: f64,
    pub y: f64,
    pub target: i64,
    pub target_rect: Rect,
    pub moved: bool,
    pub primary: bool,
    pub pointer: i32,
    pub mouse: bool,
    pub modifiers: i32,
    pub button: i32,
    /// The widget took a hold: the release is no click.
    pub held: bool,
    /// setPointerCapture: pointer events go here instead.
    pub capture: i64,
    /// Where the widget last heard the pointer was, for movementX/Y.
    pub sent_x: f64,
    pub sent_y: f64,
    /// A real finger or pen, not a mouse playing one: the chat may still scroll its press.
    pub finger: bool,
}

/// What an input event carries besides its type and target.
#[derive(Clone, Copy, Debug)]
struct Input {
    x: f64,
    y: f64,
    pointer: i32,
    primary: bool,
    mouse: bool,
    button: i32,
    buttons: i32,
    modifiers: i32,
    detail: i32,
    related: i64,
    movement_x: f64,
    movement_y: f64,
}

impl Default for Input {
    fn default() -> Input {
        Input {
            x: 0.0,
            y: 0.0,
            pointer: 0,
            primary: true,
            mouse: false,
            button: 0,
            buttons: 0,
            modifiers: 0,
            detail: 0,
            related: 0,
            movement_x: 0.0,
            movement_y: 0.0,
        }
    }
}

/// The text control being edited (the Swift renderer's WidgetEditorSpec): a platform covers it with its
/// own editor, which takes the keyboard and the input method.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorSpec {
    pub id: i64,
    /// Changes whenever anything below does; edit() names the one it answers.
    pub generation: u64,
    /// A textarea.
    pub multiline: bool,
    /// The content box, document coordinates.
    pub frame: Rect,
    /// A clipping ancestor's padding box.
    pub has_clip: bool,
    pub clip: Rect,
    /// The backend's font, and what it was made from.
    pub font: i32,
    pub font_family: Vec<u8>,
    pub font_size: f32,
    pub font_weight: i32,
    pub italic: bool,
    pub color: Color,
    /// 0 start, 1 center, 2 end.
    pub align: i32,
    pub text: Vec<u8>,
    pub placeholder: Vec<u8>,
    /// text, search, number, email, url, tel; empty for a textarea.
    pub input_type: Vec<u8>,
    pub max_length: i32,
    pub read_only: bool,
    /// What a script's select() or setSelectionRange() asked for, in UTF-16 code units as the script
    /// counts them; -1 when it asked for nothing.
    pub selection_start: i32,
    pub selection_end: i32,
}

impl Default for EditorSpec {
    fn default() -> EditorSpec {
        EditorSpec {
            id: 0,
            generation: 0,
            multiline: false,
            frame: Rect::default(),
            has_clip: false,
            clip: Rect::default(),
            font: 0,
            font_family: Vec::new(),
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            color: Color::default(),
            align: 0,
            text: Vec::new(),
            placeholder: Vec::new(),
            input_type: Vec::new(),
            max_length: -1,
            read_only: false,
            selection_start: -1,
            selection_end: -1,
        }
    }
}

/// Where and in what font an element shows its text, as layout and the cascade have it
/// (`Container::editor_style`): the fields it writes into the editor's spec.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorStyle {
    pub frame: Rect,
    pub font_family: Vec<u8>,
    pub font_size: f32,
    pub font_weight: i32,
    pub italic: bool,
    pub color: Color,
    pub align: i32,
    pub font: i32,
    pub has_clip: bool,
    pub clip: Rect,
}

/// A place in the text: an element (its `data-twid`) and a byte offset into the text of the runs it owns,
/// those whose nearest element with an id it is. It outlives a patch or a layout that keeps the element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPoint {
    pub owner: i64,
    pub offset: usize,
}

/// A piece of text as laid out, in document order: a word, a space or a fragment of one on its line.
#[derive(Clone, Debug, Default)]
pub struct TextRun {
    pub owner: i64,
    /// In the owner's text.
    pub offset: usize,
    /// As drawn.
    pub text: Vec<u8>,
    /// Document coordinates; empty for a `<br>`.
    pub rect: Rect,
    /// litehtml's font handle.
    pub font: u64,
    /// The block container it flows in.
    pub block: i32,
    pub selectable: bool,
    pub line_break: bool,
    /// Measured so far, by byte count.
    pub advances: HashMap<usize, f32>,
    /// Where the last paint drew it: the groups' transform over the box, in the promoted layer `layer`
    /// (0, the document). A run not drawn yet is taken where layout put it.
    pub shown: Affine,
    pub layer: i64,
    pub drawn: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: TextPoint,
    pub focus: TextPoint,
    /// The mouse dragging it out, while it does.
    pub pointer: i32,
}

impl Default for Selection {
    fn default() -> Selection {
        Selection { anchor: TextPoint::default(), focus: TextPoint::default(), pointer: -1 }
    }
}

/// What the input asks of the rest of the renderer: layout's and the cascade's answers, the builds it
/// wants, the clock. Called where and in the order the C++ makes these calls.
pub trait EventHost {
    /// The renderer's clock, in ms.
    fn now_ms(&mut self) -> f64;
    /// The viewport's width in CSS px (what an RTL client's arbiter measures from).
    fn width(&mut self) -> f64;
    /// `settle`: builds a document a coalesced batch left stale and reports its size, into `out` or, for
    /// None, the renderer's pending messages. True when it rebuilt.
    fn settle(&mut self, doc: &mut Document, out: Option<&mut Vec<Json>>) -> bool;
    /// `rebuild(patch)`: the document again from the tree, or patched from the batch's log.
    fn rebuild(&mut self, doc: &mut Document, patch: bool);
    /// `report_size`: a `size` message when the document's size moved.
    fn report_size(&mut self, out: &mut Vec<Json>);
    /// `restyled_by_input`: the pointer changed :hover or :active: a repaint, and the transitions the new
    /// styles start begin now.
    fn restyled_by_input(&mut self, doc: &mut Document);
    /// `selection_changed`: the selection's highlight must be painted again.
    fn selection_changed(&mut self);
    /// The element pressed last (`press_id_`, `press_y_`, `press_at_`), for the resize diff.
    fn pressed(&mut self, _id: i64, _y: f64, _at: f64) {}
    /// `widget_id_at`: the widget id of the element at (x, y) or of its nearest ancestor with one, 0 for
    /// none; with `placement`, that element's border box goes there when there is one.
    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64;
    /// `box_of`: the border box of element `id`, empty when it is not laid out.
    fn box_of(&mut self, id: i64) -> Rect;
    /// Whether element `id` is in the document as built.
    fn has_element(&mut self, id: i64) -> bool;
    /// The `touch-action` element `id` computes, None when it is not in the document.
    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>>;
    /// Whether `user-select` lets a press on element `id` select: the nearest element up from it whose
    /// value is not `auto` says `text`.
    fn user_select_text(&mut self, id: i64) -> bool;
    /// The pointer state behind :hover and :active (`Container::mouse`), `action` 0 down, 1 move, 2 up,
    /// 3 cancel; true when something must repaint.
    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool;
    /// The pointer left the view: :hover ends. True when something must repaint.
    fn leave(&mut self) -> bool;
    /// The CSS cursor where the mouse last was, as the document computes it; empty for none.
    fn cursor(&mut self) -> Vec<u8>;
    /// Where and in what font element `id` shows its text; None when it is not laid out and visible.
    fn editor_style(&mut self, id: i64) -> Option<EditorStyle>;
    /// The container's image for an `<img>` source: (image id, width, height), None when it fails.
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)>;
    /// An asynchronously decoded image got its size; true when a known image changed.
    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool;
    /// The size of element `id`'s placement, 0 × 0 when it is not in the document.
    fn content_size(&mut self, id: i64) -> (f32, f32);
    /// The messages the renderer made itself and holds (its animation events, its paint reports), moved to `out`.
    fn take_pending(&mut self, out: &mut Vec<Json>);
    /// What the text runs are laid out from: the document, its paint generation, its layouts, its style epoch.
    fn runs_key(&mut self) -> [u64; 4];
    /// The document's text as laid out, in document order (the walk of `text_runs`).
    fn text_runs(&mut self) -> Vec<TextRun>;
    /// The width of `text` in litehtml font `font`.
    fn text_width(&mut self, text: &[u8], font: u64) -> f32;
    /// What a litehtml font is for the backend: its font, its descent and its letter and word spacing.
    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)>;
    /// Where promoted layer `id`'s track shows it at `now` (seconds), over its parents', and with what
    /// opacity; None when it is not shown.
    fn layer_shown(&mut self, id: i64, now: f64) -> Option<(Affine, f32)>;
    /// What the selectable text is known from: the document and its style epoch.
    fn selectable_key(&mut self) -> [u64; 2];
    /// Whether any displayed element says `user-select: text`.
    fn any_selectable(&mut self) -> bool;
}

/// The renderer's state before a call into its host that may build: what to follow after it.
#[derive(Clone, Copy)]
struct Before {
    frozen: bool,
    paint_stopped: bool,
}

impl Before {
    fn of(doc: &Document) -> Before {
        Before { frozen: doc.frozen(), paint_stopped: doc.paint_stopped }
    }
}

/// The edit's `prop` op through the tree: what it asks of the renderer is the editor's.
struct EditOp<'a> {
    editing: &'a mut EditorSpec,
    generation: &'a mut u64,
}

impl DocumentHost for EditOp<'_> {
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
    fn value_set(&mut self, id: i64, value: &[u8]) {
        value_set(self.editing, self.generation, id, value);
    }
}

fn value_set(editing: &mut EditorSpec, generation: &mut u64, id: i64, value: &[u8]) {
    if editing.id == id && editing.text != value {
        editing.text = value.to_vec();
        editing.selection_start = -1;
        editing.selection_end = -1;
        *generation += 1;
        editing.generation = *generation;
    }
}

/// The renderer's input state.
#[derive(Debug)]
pub struct Events {
    /// By pointer, the order a capture call walks them in.
    touches: BTreeMap<i32, Touch>,
    hover: i64,
    hover_x: f64,
    hover_y: f64,
    click_count: i32,
    /// Who gets each primary press (widget-sandbox's gesture policy): the widget's own dblclick and contextmenu.
    arbiter: GestureArbiter,
    arbiter_pointer: i32,
    /// `arbitrate`: the renderer plays the bubble (`bubble`), a mouse's main button as a finger (`bubble_touch`).
    bubble: bool,
    bubble_touch: bool,
    rtl: bool,
    /// Key "type:pointer", latest event.
    pending_moves: Vec<(Vec<u8>, Json)>,
    interest: HashSet<Vec<u8>>,
    prevent: HashSet<Vec<u8>>,
    editing: EditorSpec,
    editing_initial: Vec<u8>,
    editor_generation: u64,
    selection: Selection,
    runs_key: [u64; 4],
    runs: Vec<TextRun>,
    selectable_key: [u64; 2],
    selectable: bool,
    /// observeResize: id → content box last reported.
    resize_watch: BTreeMap<i64, (f32, f32)>,
    /// `<img>` elements whose image the backend is still decoding, by image id.
    waiting_images: HashMap<i32, Vec<i64>>,
    /// The user's last input, in seconds: ops batches build at once after it, and a script's focus() takes the keyboard.
    pub last_input: f64,
}

impl Default for Events {
    fn default() -> Events {
        Events::new()
    }
}

impl Events {
    pub fn new() -> Events {
        Events {
            touches: BTreeMap::new(),
            hover: 0,
            hover_x: 0.0,
            hover_y: 0.0,
            click_count: 0,
            arbiter: GestureArbiter::new(),
            arbiter_pointer: -1,
            bubble: false,
            bubble_touch: false,
            rtl: false,
            pending_moves: Vec::new(),
            interest: HashSet::new(),
            prevent: HashSet::new(),
            editing: EditorSpec::default(),
            editing_initial: Vec::new(),
            editor_generation: 0,
            selection: Selection::default(),
            runs_key: [0; 4],
            runs: Vec::new(),
            selectable_key: [0; 2],
            selectable: false,
            resize_watch: BTreeMap::new(),
            waiting_images: HashMap::new(),
            last_input: -1e9,
        }
    }

    // ── state, for the renderer and its tests ──

    pub fn touches(&self) -> &BTreeMap<i32, Touch> {
        &self.touches
    }
    pub fn hover_state(&self) -> (i64, f64, f64) {
        (self.hover, self.hover_x, self.hover_y)
    }
    pub fn click_count(&self) -> i32 {
        self.click_count
    }
    pub fn arbiter_pointer(&self) -> i32 {
        self.arbiter_pointer
    }
    pub fn arbitration(&self) -> (bool, bool, bool) {
        (self.bubble, self.bubble_touch, self.rtl)
    }
    pub fn pending_moves(&self) -> &[(Vec<u8>, Json)] {
        &self.pending_moves
    }
    pub fn interest(&self) -> &HashSet<Vec<u8>> {
        &self.interest
    }
    pub fn prevented(&self) -> &HashSet<Vec<u8>> {
        &self.prevent
    }
    pub fn editing_initial(&self) -> &[u8] {
        &self.editing_initial
    }
    pub fn editor_generation(&self) -> u64 {
        self.editor_generation
    }
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }
    pub fn runs_key(&self) -> [u64; 4] {
        self.runs_key
    }
    pub fn selectable_cache(&self) -> ([u64; 2], bool) {
        (self.selectable_key, self.selectable)
    }
    pub fn resize_watch(&self) -> &BTreeMap<i64, (f32, f32)> {
        &self.resize_watch
    }
    pub fn waiting_images(&self) -> &HashMap<i32, Vec<i64>> {
        &self.waiting_images
    }
    /// Something waits for the next frame on the input's side: a coalesced move.
    pub fn has_pending_moves(&self) -> bool {
        !self.pending_moves.is_empty()
    }

    /// The node of the control the platform's editor shows, or 0: the writer draws its text transparent.
    pub fn editing_id(&self) -> i64 {
        self.editing.id
    }

    /// Null when nothing is being edited.
    pub fn editor(&self) -> Option<&EditorSpec> {
        (self.editing.id != 0).then_some(&self.editing)
    }

    /// The JSON of editor(): null, or the fields of the spec by name.
    pub fn editor_json(&self) -> Vec<u8> {
        if self.editing.id == 0 {
            return b"null".to_vec();
        }
        let e = &self.editing;
        let rect = |r: &Rect| {
            let mut a = Json::array();
            a.push(f64::from(r.x)).push(f64::from(r.y)).push(f64::from(r.w)).push(f64::from(r.h));
            a
        };
        let mut j = Json::object();
        j.set("id", e.id as f64).set("generation", e.generation as f64).set("multiline", e.multiline).set("frame", rect(&e.frame));
        if e.has_clip {
            j.set("clip", rect(&e.clip));
        }
        j.set("font", e.font)
            .set("fontFamily", e.font_family.clone())
            .set("fontSize", f64::from(e.font_size))
            .set("fontWeight", e.font_weight)
            .set("italic", e.italic);
        let color = (u32::from(e.color.a) << 24) | (u32::from(e.color.r) << 16) | (u32::from(e.color.g) << 8) | u32::from(e.color.b);
        j.set("color", f64::from(color));
        j.set("align", e.align)
            .set("text", e.text.clone())
            .set("placeholder", e.placeholder.clone())
            .set("inputType", e.input_type.clone());
        j.set("maxLength", e.max_length)
            .set("readOnly", e.read_only)
            .set("selectionStart", e.selection_start)
            .set("selectionEnd", e.selection_end);
        j.dump()
    }

    // ── the host's messages ──

    /// The `listen`, `preventDefault` and `arbitrate` messages, which only the input reads.
    pub fn receive(&mut self, msg: &Json) {
        match msg.get("type").as_str() {
            b"listen" => {
                self.interest.clear();
                for t in msg.get("types").arr() {
                    if t.is_string() {
                        self.interest.insert(t.as_str().to_vec());
                    }
                }
            }
            b"preventDefault" => {
                self.prevent.clear();
                for k in msg.get("keys").arr() {
                    if k.is_string() {
                        self.prevent.insert(k.as_str().to_vec());
                    }
                }
            }
            b"arbitrate" => {
                self.bubble = msg.get("bubble").as_bool(false);
                self.bubble_touch = self.bubble && msg.get("touch").as_bool(false);
                self.rtl = msg.get("rtl").as_bool(false);
            }
            _ => {}
        }
    }

    /// The renderer froze: it lets go of its touches, coalesced moves and hover.
    pub fn on_frozen(&mut self) {
        self.touches.clear();
        self.pending_moves.clear();
        self.hover = 0;
    }

    /// The renderer stopped past its paint limit: the coalesced moves go with the frame.
    pub fn drop_moves(&mut self) {
        self.pending_moves.clear();
    }

    /// A node left the tree for good: the renderer stops observing its size.
    pub fn node_forgotten(&mut self, id: i64) {
        self.resize_watch.remove(&id);
    }

    /// A `prop` op set node `id`'s value: the editor takes it when it edits that node.
    pub fn value_set(&mut self, id: i64, value: &[u8]) {
        value_set(&mut self.editing, &mut self.editor_generation, id, value);
    }

    /// A `call` op the tree does not answer itself: pointer capture, resize observers, focus, blur, selection.
    pub fn call(&mut self, doc: &mut Document, host: &mut dyn EventHost, id: i64, method: &[u8], args: &Json, out: &mut Vec<Json>) {
        match method {
            b"setPointerCapture" | b"releasePointerCapture" => {
                // The pointer id as event_json gives it out: the mouse is 1, a finger its pointer + 2.
                let pointer_id = i64::from(args.at(0).as_int(0));
                let pointers: Vec<i32> = self.touches.keys().copied().collect();
                for pointer in pointers {
                    let Some(t) = self.touches.get(&pointer) else { continue };
                    let (mouse, capture) = (t.mouse, t.capture);
                    if i64::from(if mouse { 1 } else { pointer.wrapping_add(2) }) != pointer_id {
                        continue;
                    }
                    if method == b"setPointerCapture" && host.has_element(id) {
                        self.set_capture(doc, host, Some(pointer), None, id, out);
                    } else if capture == id {
                        self.set_capture(doc, host, Some(pointer), None, 0, out);
                    }
                }
            }
            // A first report follows observe(), as in browsers.
            b"observeResize" => {
                self.resize_watch.entry(id).or_insert((-1.0, -1.0));
            }
            b"unobserveResize" => {
                self.resize_watch.remove(&id);
            }
            b"focus" => {
                if host.now_ms() / 1000.0 - self.last_input < SCRIPT_FOCUS_S && self.text_control(doc, id) == id {
                    self.focus(doc, host, id, out);
                }
            }
            b"blur" => {
                if self.editing.id == id {
                    self.blur(doc, host, out);
                }
            }
            b"select" | b"setSelectionRange" if self.editing.id == id => {
                let mut length = 0i32;
                for &c in &self.editing.text {
                    length += if (c & 0xC0) != 0x80 { if (c & 0xF8) == 0xF0 { 2 } else { 1 } } else { 0 };
                }
                let start = if method == b"select" { 0 } else { args.at(0).as_int(0).clamp(0, length) };
                let end = if method == b"select" { length } else { args.at(1).as_int(start).clamp(start, length) };
                self.editing.selection_start = start;
                self.editing.selection_end = end;
                self.editor_generation += 1;
                self.editing.generation = self.editor_generation;
            }
            // scroll*: nothing to do without scrolling
            _ => {}
        }
    }

    // ── what a host call that may build leaves behind ──

    fn after(&mut self, doc: &mut Document, host: &mut dyn EventHost, before: Before, built: bool) {
        if built {
            self.refresh_editor(doc, host);
        }
        if !before.frozen && doc.frozen() {
            self.on_frozen();
        }
        if !before.paint_stopped && doc.paint_stopped {
            self.pending_moves.clear();
        }
    }

    fn settle(&mut self, doc: &mut Document, host: &mut dyn EventHost, out: Option<&mut Vec<Json>>) {
        let before = Before::of(doc);
        let built = host.settle(doc, out);
        self.after(doc, host, before, built);
    }

    fn rebuild(&mut self, doc: &mut Document, host: &mut dyn EventHost, patch: bool) {
        let before = Before::of(doc);
        host.rebuild(doc, patch);
        self.after(doc, host, before, true);
    }

    fn restyled_by_input(&mut self, doc: &mut Document, host: &mut dyn EventHost) {
        let before = Before::of(doc);
        host.restyled_by_input(doc);
        self.after(doc, host, before, false);
    }

    // ── events ──

    /// What the widget claims of a press on `target`: the `touch-action` of it and its ancestors, and its
    /// preventDefault on pointerdown or touchstart.
    fn gesture_claim(&self, doc: &Document, host: &mut dyn EventHost, target: i64) -> WidgetGestureClaim {
        let prevented = self.prevent.contains(&key_of(target, b":pointerdown")) || self.prevent.contains(&key_of(target, b":touchstart"));
        let mut values: Vec<Vec<u8>> = Vec::new();
        // Up to the widget's <html> itself: touch-action on an ancestor covers its descendants, as in CSS.
        let mut n = if target != 0 { doc.node(target) } else { Some(doc.html()) };
        while let Some(r) = n {
            let node = doc.get(r);
            // From any stylesheet: the fork keeps the declaration as written, which is all this needs.
            if node.id != 0
                && let Some(value) = host.touch_action(node.id)
            {
                values.push(value);
            }
            n = node.parent;
        }
        twidget_gesture::claim_of(&values, prevented)
    }

    /// What a press at (x, y) that starts no selection would claim, without pressing; nothing while frozen.
    pub fn claim_at(&mut self, doc: &mut Document, host: &mut dyn EventHost, x: f64, y: f64) -> WidgetGestureClaim {
        self.settle(doc, host, None);
        if doc.frozen() {
            return WidgetGestureClaim::default();
        }
        let target = host.hit(x, y, None);
        self.gesture_claim(doc, host, target)
    }

    /// The CSS cursor where the mouse last was: "pointer", "text", … as the document computes it, "auto"
    /// when it says nothing, after the mouse left, or while frozen.
    pub fn cursor(&self, doc: &Document, host: &mut dyn EventHost) -> Vec<u8> {
        let c = host.cursor();
        if c.is_empty() || doc.frozen() { b"auto".to_vec() } else { css::trim(&css::lower(&c)) }
    }

    /// The arbiter's x: from the client's leading edge.
    fn arbiter_x(&self, host: &mut dyn EventHost, x: f64) -> f64 {
        if self.rtl { host.width() - x } else { x }
    }

    /// When the arbiter's hold falls due, in ms; none while frozen or without a press.
    pub fn hold_deadline(&self, doc: &Document) -> Option<f64> {
        if self.arbiter_pointer >= 0 && !doc.frozen() { self.arbiter.hold_deadline() } else { None }
    }

    /// A frame: a hold that fell due settles the press.
    pub fn check_hold(&mut self, doc: &mut Document, host: &mut dyn EventHost, out: &mut Vec<Json>) {
        let Some(at) = self.hold_deadline(doc) else { return };
        if host.now_ms() < at {
            return;
        }
        let now = host.now_ms();
        if let Some(v) = self.arbiter.tick(now) {
            let pointer = self.arbiter_pointer;
            self.settle_gesture(doc, host, &v, pointer, out);
        }
    }

    /// Acts on a verdict once the event that produced it has gone to the widget.
    fn settle_gesture(&mut self, doc: &mut Document, host: &mut dyn EventHost, v: &GestureVerdict, pointer: i32, out: &mut Vec<Json>) {
        let Some(t) = self.touches.get_mut(&pointer) else { return };
        if v.event == Some(GestureEvent::ContextMenu) {
            t.held = v.gesture == Gesture::Hold;
            let t = t.clone();
            let target = if t.capture != 0 { t.capture } else { t.target };
            if target != 0 {
                let rect = if t.capture != 0 { host.box_of(target) } else { t.target_rect };
                self.emit_event(doc, b"contextmenu", target, &rect, &t, 0, out);
            }
        }
        if v.cancel && self.bubble {
            self.bubble_takes(doc, host, pointer, v, out);
        }
    }

    /// Playing the bubble: the press ends for the widget with pointercancel, and the host hears what the bubble does.
    fn bubble_takes(&mut self, doc: &mut Document, host: &mut dyn EventHost, pointer: i32, v: &GestureVerdict, out: &mut Vec<Json>) {
        // Nothing more of this pointer reaches the widget: its later moves and its release find no touch.
        let Some(mut done) = self.touches.remove(&pointer) else { return };
        if pointer == self.arbiter_pointer {
            self.arbiter.cancel();
            self.arbiter_pointer = -1;
        }
        if self.selection.pointer == pointer {
            self.selection.pointer = -1;
        }
        if done.button == 0 && host.mouse(3, done.x, done.y, false) {
            self.restyled_by_input(doc, host);
        }
        if done.target != 0 {
            let target = if done.capture != 0 { done.capture } else { done.target };
            let rect = if done.capture != 0 { host.box_of(done.capture) } else { done.target_rect };
            self.emit_event(doc, b"pointercancel", target, &rect, &done, 3, out);
            let (target, rect) = (done.target, done.target_rect);
            self.emit_event(doc, b"touchcancel", target, &rect, &done, 3, out);
        }
        self.set_capture(doc, host, None, Some(&mut done), 0, out);
        let mut m = Json::object();
        m.set("type", "bubble")
            .set("action", v.action.unwrap_or(BubbleAction::Menu).name())
            .set("gesture", v.gesture.name())
            .set("x", done.x)
            .set("y", done.y)
            .set("edge", v.edge);
        out.push(m);
    }

    fn event_json(&self, doc: &Document, kind: &[u8], target: i64, target_rect: &Rect, i: &Input) -> Json {
        let mut ev = Json::object();
        ev.set("type", kind).set("targetId", target as f64);
        ev.set("clientX", i.x).set("clientY", i.y).set("pageX", i.x).set("pageY", i.y);
        ev.set("offsetX", i.x - f64::from(target_rect.x)).set("offsetY", i.y - f64::from(target_rect.y));
        ev.set("movementX", i.movement_x).set("movementY", i.movement_y);
        ev.set("button", i.button).set("buttons", i.buttons).set("detail", i.detail).set("modifiers", i.modifiers);
        if i.related != 0 {
            ev.set("relatedId", i.related as f64);
        }
        let contains = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        if kind.starts_with(b"pointer")
            || contains(kind, b"pointercapture")
            || kind == b"click"
            || kind == b"dblclick"
            || kind == b"contextmenu"
        {
            ev.set("pointerId", f64::from(if i.mouse { 1 } else { i.pointer.wrapping_add(2) }))
                .set("pointerType", if i.mouse { "mouse" } else { "touch" })
                .set("isPrimary", i.primary)
                .set("pressure", if i.buttons != 0 { 0.5 } else { 0.0 });
        }
        // The sandbox runs a click's default action (a link, a label, a checkbox) unless a listener prevents it.
        if kind == b"click" {
            ev.set("defaultActions", true);
        }
        // Form control state travels with every event (the sandbox mirror relies on it).
        if let Some(r) = doc.node(target) {
            let n = doc.get(r);
            if n.tag == b"input" || n.tag == b"textarea" || n.tag == b"select" || n.tag == b"button" {
                let mut checkbox = false;
                for (name, value) in &n.attrs {
                    if name == b"type" {
                        let v = css::lower(value);
                        checkbox = v == b"checkbox" || v == b"radio";
                    }
                }
                if checkbox {
                    let checked = n.attrs.iter().any(|(name, _)| name == b"checked");
                    ev.set("checked", checked);
                }
                if n.tag == b"textarea" {
                    ev.set("value", n.data.clone());
                } else {
                    for (name, value) in &n.attrs {
                        if name == b"value" {
                            ev.set("value", value.clone());
                        }
                    }
                }
            }
        }
        ev
    }

    /// Every event comes from real input (no script runs in this document): trusted, like the WebView
    /// renderer's isTrusted, which the hosts require before they count a user gesture; a finger's press is not.
    pub fn event_message(ev: Json, trusted: bool) -> Json {
        let mut m = Json::object();
        m.set("type", "event").set("ev", ev).set("trusted", trusted);
        m
    }

    /// Sends `ev` if the sandbox listens for its type; move-type events wait for the next tick, the
    /// latest one per type and pointer.
    fn dispatch(&mut self, doc: &Document, mut ev: Json, out: &mut Vec<Json>, trusted: bool) {
        if doc.frozen() {
            return;
        }
        let kind = ev.get("type").as_str().to_vec();
        if !self.listened(&kind) {
            return;
        }
        if coalesced(&kind) {
            let mut key = kind.clone();
            key.push(b':');
            key.extend_from_slice((ev.get("pointerId").as_number(0.0) as i64).to_string().as_bytes());
            for (k, pending) in &mut self.pending_moves {
                if *k != key {
                    continue;
                }
                // Wheel deltas add up over the frame; a move only keeps its latest position.
                if kind == b"wheel" {
                    let dx = ev.get("deltaX").as_number(0.0) + pending.get("deltaX").as_number(0.0);
                    ev.set("deltaX", dx);
                    let dy = ev.get("deltaY").as_number(0.0) + pending.get("deltaY").as_number(0.0);
                    ev.set("deltaY", dy);
                } else {
                    let mx = ev.get("movementX").as_number(0.0) + pending.get("movementX").as_number(0.0);
                    ev.set("movementX", mx);
                    let my = ev.get("movementY").as_number(0.0) + pending.get("movementY").as_number(0.0);
                    ev.set("movementY", my);
                }
                *pending = ev;
                return;
            }
            self.pending_moves.push((key, ev));
            return;
        }
        // Anything else keeps its order relative to the moves before it.
        self.flush_moves(out);
        out.push(Events::event_message(ev, trusted));
    }

    /// The coalesced moves of the frame go out.
    pub fn flush_moves(&mut self, out: &mut Vec<Json>) {
        for (_, ev) in self.pending_moves.drain(..) {
            out.push(Events::event_message(ev, true));
        }
    }

    /// After a message: the resize reports, then what the renderer held back (its own events, the violations).
    pub fn flush_pending(&mut self, doc: &mut Document, host: &mut dyn EventHost, out: &mut Vec<Json>) {
        self.report_resizes(doc, host, out);
        host.take_pending(out);
        for v in std::mem::take(&mut doc.pending_violations) {
            let mut m = Json::object();
            m.set("type", "violation").set("detail", v);
            out.push(m);
        }
    }

    /// `resize` for the ResizeObserver targets whose content box changed since the last one.
    fn report_resizes(&mut self, doc: &Document, host: &mut dyn EventHost, out: &mut Vec<Json>) {
        // A layout still to come reports on the frame that builds it.
        if self.resize_watch.is_empty() || doc.frozen() || doc.stale {
            return;
        }
        let mut entries = Json::array();
        for (&id, last) in &mut self.resize_watch {
            let size = host.content_size(id);
            if size == *last {
                continue;
            }
            *last = size;
            let mut e = Json::object();
            e.set("id", id as f64).set("width", f64::from(size.0)).set("height", f64::from(size.1));
            entries.push(e);
        }
        if !entries.arr().is_empty() {
            let mut m = Json::object();
            m.set("type", "resize").set("entries", entries);
            out.push(m);
        }
    }

    /// A click's default action on a <details>' summary: open or close it, and send `toggle`.
    fn toggle_details(&mut self, doc: &mut Document, host: &mut dyn EventHost, clicked: i64, out: &mut Vec<Json>) {
        let mut summary = doc.node(clicked);
        while let Some(r) = summary {
            let n = doc.get(r);
            if n.tag == b"summary" {
                break;
            }
            if self.prevent.contains(&key_of(n.id, b":click")) {
                return;
            }
            summary = n.parent;
        }
        let Some(summary) = summary else { return };
        if self.prevent.contains(&key_of(doc.get(summary).id, b":click")) {
            return;
        }
        let Some(details) = doc.get(summary).parent else { return };
        if doc.get(details).tag != b"details" {
            return;
        }
        // Only its first summary opens it.
        for &c in &doc.get(details).children {
            let child = doc.get(c);
            if child.kind == NodeKind::Element && child.tag == b"summary" {
                if c != summary {
                    return;
                }
                break;
            }
        }
        let open = doc.get(details).attrs.iter().position(|(name, _)| name == b"open");
        let opening = open.is_none();
        match open {
            None => doc.get_mut(details).attrs.push((b"open".to_vec(), Vec::new())),
            Some(i) => {
                doc.get_mut(details).attrs.remove(i);
            }
        }
        if doc.attached(Some(details)) {
            doc.patch_log.valid = false;
        }
        doc.stale = true;
        // The sandbox mirrors `open` from the event (widget-sandbox forms.ts).
        let id = doc.get(details).id;
        let rect = host.box_of(id);
        let mut ev = self.event_json(doc, b"toggle", id, &rect, &Input::default());
        ev.set("open", opening);
        self.dispatch(doc, ev, out, true);
    }

    /// The sandbox wants events of `kind` (it listens, or every widget gets them).
    pub fn listened(&self, kind: &[u8]) -> bool {
        if !PROFILE_EVENTS.contains(&kind) {
            return false;
        }
        if ALWAYS_FORWARD.contains(&kind) {
            return true;
        }
        self.interest.contains(kind)
    }

    fn send_event(&mut self, doc: &Document, host: &mut dyn EventHost, kind: &[u8], target: i64, i: &Input, out: &mut Vec<Json>) {
        if self.listened(kind) {
            let rect = host.box_of(target);
            let ev = self.event_json(doc, kind, target, &rect, i);
            self.dispatch(doc, ev, out, true);
        }
    }

    fn emit_event(&mut self, doc: &Document, kind: &[u8], target: i64, target_rect: &Rect, t: &Touch, action: i32, out: &mut Vec<Json>) {
        if t.mouse && kind.starts_with(b"touch") {
            return;
        }
        // An event nothing listens for is dropped before it is built.
        if !self.listened(kind) {
            return;
        }
        let mut i = Input {
            x: t.x,
            y: t.y,
            pointer: t.pointer,
            primary: t.primary,
            mouse: t.mouse,
            modifiers: t.modifiers,
            button: if action == 1 { 0 } else { t.button },
            buttons: if action == 0 || action == 1 { if t.button == 2 { 2 } else { 1 } } else { 0 },
            detail: if kind == b"click" || kind == b"dblclick" { self.click_count } else { 0 },
            ..Input::default()
        };
        if action == 1 {
            i.movement_x = t.x - t.sent_x;
            i.movement_y = t.y - t.sent_y;
        }
        // A finger's or a pen's press may still become the chat's scroll, which ends it in pointercancel: its press is no
        // gesture, its release is (widget-sandbox v0.3.0, a finger's press counts from its release).
        let trusted = !(t.finger && (kind == b"pointerdown" || kind == b"mousedown"));
        let ev = self.event_json(doc, kind, target, target_rect, &i);
        self.dispatch(doc, ev, out, trusted);
    }

    /// The capture of the touch of `pointer` (or of `local`, a touch already out of the map) becomes `id`.
    fn set_capture(
        &mut self,
        doc: &Document,
        host: &mut dyn EventHost,
        pointer: Option<i32>,
        local: Option<&mut Touch>,
        id: i64,
        out: &mut Vec<Json>,
    ) {
        let t = match (local, pointer) {
            (Some(t), _) => t,
            (None, Some(p)) => match self.touches.get_mut(&p) {
                Some(t) => t,
                None => return,
            },
            (None, None) => return,
        };
        if t.capture == id {
            return;
        }
        let previous = t.capture;
        t.capture = id;
        let t = t.clone();
        if previous != 0 {
            let rect = host.box_of(previous);
            self.emit_event(doc, b"lostpointercapture", previous, &rect, &t, 2, out);
        }
        if id != 0 {
            let rect = host.box_of(id);
            self.emit_event(doc, b"gotpointercapture", id, &rect, &t, 1, out);
        }
    }

    fn is_ancestor(doc: &Document, ancestor: i64, id: i64) -> bool {
        if ancestor == 0 {
            return false;
        }
        let mut p = doc.node(id).and_then(|n| doc.get(n).parent);
        while let Some(r) = p {
            if doc.get(r).id == ancestor {
                return true;
            }
            p = doc.get(r).parent;
        }
        false
    }

    fn chain(doc: &Document, id: i64) -> Vec<i64> {
        let mut out = Vec::new();
        let mut n = doc.node(id);
        while let Some(r) = n {
            let node = doc.get(r);
            if node.id != 0 {
                out.push(node.id);
            }
            n = node.parent;
        }
        out
    }

    /// A pointer from the host view, in CSS px. `action`: 0 down, 1 move, 2 up, 3 cancel. A mouse sends no
    /// touch events; `modifiers` is bit0 alt, bit1 ctrl, bit2 meta, bit3 shift. `button` 2 is a mouse's
    /// secondary button: the widget's `contextmenu` when its claim takes holds, else the bubble's menu.
    pub fn touch(
        &mut self,
        doc: &mut Document,
        host: &mut dyn EventHost,
        action: i32,
        x: f64,
        y: f64,
        pointer: i32,
        primary: bool,
        mouse: bool,
        modifiers: i32,
        button: i32,
    ) -> Vec<Json> {
        let mut out = Vec::new();
        self.last_input = host.now_ms() / 1000.0;
        self.settle(doc, host, Some(&mut out));
        if doc.frozen() {
            return out;
        }
        if action == 0 {
            // Playing a phone's bubble, a mouse's main button is a finger, for the widget too.
            let finger = !mouse || (self.bubble_touch && button != 2);
            let mut t = Touch {
                x0: x,
                y0: y,
                x,
                y,
                target: 0,
                target_rect: Rect::default(),
                moved: false,
                primary,
                pointer,
                mouse: !finger,
                modifiers,
                button: 0,
                held: false,
                capture: 0,
                sent_x: x,
                sent_y: y,
                finger: false,
            };
            t.button = button;
            t.finger = !mouse;
            t.target = host.hit(x, y, Some(&mut t.target_rect));
            self.touches.insert(pointer, t.clone());
            let mut selecting = false;
            if primary {
                let at = host.now_ms() / 1000.0;
                host.pressed(t.target, y, at);
                // A press drops the selection; a mouse's on selectable text starts the next one there.
                let had = self.selection.anchor != self.selection.focus;
                self.selection = Selection::default();
                if t.mouse && button == 0 && t.target != 0 && self.starts_selection(doc, host, t.target) {
                    self.text_runs(host);
                    if let Some(at) = self.point_at(host, x, y) {
                        self.selection.anchor = at;
                        self.selection.focus = at;
                        self.selection.pointer = pointer;
                        selecting = true;
                    }
                }
                if had {
                    host.selection_changed();
                }
            }
            let mut verdict = None;
            if primary {
                // A drag that selects claims what a claim can: horizontal drags (a mouse never pans the chat anyway).
                let claim =
                    if selecting { WidgetGestureClaim { x: true, y: false, hold: false } } else { self.gesture_claim(doc, host, t.target) };
                let mut m = Json::object();
                m.set("type", "gesture").set("wants", claim.y).set("x", claim.x).set("y", claim.y).set("hold", claim.hold);
                out.push(m);
                self.arbiter_pointer = pointer;
                let now = host.now_ms();
                let ax = self.arbiter_x(host, x);
                verdict = Some(self.arbiter.down(now, ax, y, claim, if t.mouse { PointerKind::Mouse } else { PointerKind::Touch }, button));
            }
            if button == 0 && host.mouse(0, x, y, false) {
                self.restyled_by_input(doc, host);
            }
            if t.target != 0 {
                let (target, rect) = (t.target, t.target_rect);
                self.emit_event(doc, b"pointerdown", target, &rect, &t, 0, &mut out);
                self.emit_event(doc, b"mousedown", target, &rect, &t, 0, &mut out);
                self.emit_event(doc, b"touchstart", target, &rect, &t, 0, &mut out);
            }
            if let Some(v) = verdict {
                self.settle_gesture(doc, host, &v, pointer, &mut out);
            }
            self.flush_pending(doc, host, &mut out);
            return out;
        }
        let Some(t) = self.touches.get_mut(&pointer) else { return out };
        t.x = x;
        t.y = y;
        t.modifiers = modifiers;
        let mut verdict = None;
        if pointer == self.arbiter_pointer {
            if action == 1 {
                let now = host.now_ms();
                let ax = self.arbiter_x(host, x);
                verdict = self.arbiter.move_to(now, ax, y);
            } else {
                if action == 2 {
                    let now = host.now_ms();
                    let ax = self.arbiter_x(host, x);
                    verdict = self.arbiter.up(now, ax, y);
                } else {
                    self.arbiter.cancel();
                }
                self.arbiter_pointer = -1;
            }
        }
        if action == 1 && self.selection.pointer == pointer {
            self.text_runs(host);
            if let Some(at) = self.point_at(host, x, y)
                && at != self.selection.focus
            {
                self.selection.focus = at;
                host.selection_changed();
            }
        }
        if action == 1 {
            let t = &self.touches[&pointer];
            // A finger jitters a few px during a tap: moves inside the policy's slop are not forwarded.
            if !t.moved && (x - t.x0).hypot(y - t.y0) <= TOUCH_SLOP_PX {
                if let Some(v) = verdict {
                    self.settle_gesture(doc, host, &v, pointer, &mut out);
                }
                return out;
            }
            if let Some(t) = self.touches.get_mut(&pointer) {
                t.moved = true;
            }
            if host.mouse(1, x, y, false) {
                self.restyled_by_input(doc, host);
                // A layout past its budget freezes the renderer, and freeze() drops every touch: `t` with them.
                if !self.touches.contains_key(&pointer) {
                    self.flush_pending(doc, host, &mut out);
                    return out;
                }
            }
            let t = self.touches[&pointer].clone();
            if t.target != 0 {
                // A finger's events stay with the element it pressed; a mouse's go to the element under
                // it. Either way a capture takes the pointer events.
                let mut rect = t.target_rect;
                let under = if t.mouse { host.hit(x, y, Some(&mut rect)) } else { t.target };
                if t.capture != 0 && !host.has_element(t.capture) {
                    self.set_capture(doc, host, Some(pointer), None, 0, &mut out);
                }
                let t = self.touches[&pointer].clone();
                if t.capture != 0 {
                    let r = host.box_of(t.capture);
                    self.emit_event(doc, b"pointermove", t.capture, &r, &t, 1, &mut out);
                } else if under != 0 {
                    self.emit_event(doc, b"pointermove", under, &rect, &t, 1, &mut out);
                }
                if under != 0 {
                    self.emit_event(doc, b"mousemove", under, &rect, &t, 1, &mut out);
                }
                let (target, target_rect) = (t.target, t.target_rect);
                self.emit_event(doc, b"touchmove", target, &target_rect, &t, 1, &mut out);
            }
            if let Some(t) = self.touches.get_mut(&pointer) {
                t.sent_x = x;
                t.sent_y = y;
            }
            if let Some(v) = verdict {
                self.settle_gesture(doc, host, &v, pointer, &mut out);
            }
            return out;
        }
        // Playing the bubble, a press it takes at the release (a drag past the slop there) ends with pointercancel.
        if let Some(v) = verdict
            && v.cancel
            && self.bubble
        {
            self.bubble_takes(doc, host, pointer, &v, &mut out);
            self.flush_pending(doc, host, &mut out);
            return out;
        }
        let Some(mut done) = self.touches.remove(&pointer) else { return out };
        if self.selection.pointer == pointer {
            self.selection.pointer = -1;
        }
        if done.button == 0 && host.mouse(action, x, y, done.mouse && action == 2) {
            self.restyled_by_input(doc, host);
        }
        if action == 2 {
            let mut up_rect = Rect::default();
            let up_target = host.hit(x, y, Some(&mut up_rect));
            if done.target != 0 {
                let under = if done.mouse { up_target } else { done.target };
                let under_rect = if done.mouse { up_rect } else { done.target_rect };
                if done.capture != 0 && !host.has_element(done.capture) {
                    self.set_capture(doc, host, None, Some(&mut done), 0, &mut out);
                }
                if done.capture != 0 {
                    let r = host.box_of(done.capture);
                    self.emit_event(doc, b"pointerup", done.capture, &r, &done, 2, &mut out);
                } else if under != 0 {
                    self.emit_event(doc, b"pointerup", under, &under_rect, &done, 2, &mut out);
                }
                if under != 0 {
                    self.emit_event(doc, b"mouseup", under, &under_rect, &done, 2, &mut out);
                }
                let (target, target_rect) = (done.target, done.target_rect);
                self.emit_event(doc, b"touchend", target, &target_rect, &done, 2, &mut out);
            }
            // A click: the press and the release land on the same element, or one inside the other
            // (the click goes to the outer one). A finger must also have stayed inside the slop.
            let mut click = 0;
            if done.target != 0 && (done.mouse || !done.moved) {
                if up_target == done.target || Events::is_ancestor(doc, done.target, up_target) {
                    click = done.target;
                } else if Events::is_ancestor(doc, up_target, done.target) {
                    click = up_target;
                }
            }
            // A secondary button never clicks, nor does a hold the widget took (it had its contextmenu).
            if done.primary && click != 0 && done.button == 0 && !done.held {
                let twice = verdict.is_some_and(|v| v.event == Some(GestureEvent::DblClick));
                self.click_count = if twice { 2 } else { 1 };
                let rect = if click == done.target { done.target_rect } else { up_rect };
                self.emit_event(doc, b"click", click, &rect, &done, 2, &mut out);
                if twice {
                    self.emit_event(doc, b"dblclick", click, &rect, &done, 2, &mut out);
                }
                self.toggle_details(doc, host, click, &mut out);
                // A tap on a text control edits it; one anywhere else ends the editing.
                let control = self.text_control(doc, click);
                if control != 0 {
                    self.focus(doc, host, control, &mut out);
                } else if self.editing.id != 0 {
                    self.blur(doc, host, &mut out);
                }
                if doc.stale {
                    self.rebuild(doc, host, true);
                    host.report_size(&mut out);
                }
            }
        } else if done.target != 0 {
            let target = if done.capture != 0 { done.capture } else { done.target };
            let rect = if done.capture != 0 { host.box_of(done.capture) } else { done.target_rect };
            self.emit_event(doc, b"pointercancel", target, &rect, &done, 3, &mut out);
            let (target, rect) = (done.target, done.target_rect);
            self.emit_event(doc, b"touchcancel", target, &rect, &done, 3, &mut out);
        }
        self.set_capture(doc, host, None, Some(&mut done), 0, &mut out);
        self.flush_pending(doc, host, &mut out);
        out
    }

    /// A mouse over the view with no button down (`action` 0), or leaving it (1): hover state and the
    /// over/out/enter/leave events.
    pub fn hover(&mut self, doc: &mut Document, host: &mut dyn EventHost, action: i32, x: f64, y: f64, modifiers: i32) -> Vec<Json> {
        let mut out = Vec::new();
        self.last_input = host.now_ms() / 1000.0;
        self.settle(doc, host, Some(&mut out));
        if doc.frozen() {
            return out;
        }
        let mut i = Input { mouse: true, pointer: 1, modifiers, ..Input::default() };
        let previous = self.hover;
        let mut target = 0;
        if action == 0 {
            i.movement_x = x - self.hover_x;
            i.movement_y = y - self.hover_y;
            self.hover_x = x;
            self.hover_y = y;
            i.x = x;
            i.y = y;
            if host.mouse(1, x, y, false) {
                self.restyled_by_input(doc, host);
            }
            target = host.hit(x, y, None);
        } else {
            i.x = self.hover_x;
            i.y = self.hover_y;
            if host.leave() {
                self.restyled_by_input(doc, host);
            }
        }
        if target != previous {
            self.hover = target;
            // out/over bubble and name the other element; leave/enter go to each ancestor the pointer
            // actually left or entered, innermost first for leave and outermost first for enter.
            let before = Events::chain(doc, previous);
            let after = Events::chain(doc, target);
            if previous != 0 {
                i.related = target;
                self.send_event(doc, host, b"pointerout", previous, &i, &mut out);
                self.send_event(doc, host, b"mouseout", previous, &i, &mut out);
                for &id in &before {
                    if after.contains(&id) {
                        continue;
                    }
                    self.send_event(doc, host, b"pointerleave", id, &i, &mut out);
                    self.send_event(doc, host, b"mouseleave", id, &i, &mut out);
                }
            }
            if target != 0 {
                i.related = previous;
                self.send_event(doc, host, b"pointerover", target, &i, &mut out);
                self.send_event(doc, host, b"mouseover", target, &i, &mut out);
                for &id in after.iter().rev() {
                    if before.contains(&id) {
                        continue;
                    }
                    self.send_event(doc, host, b"pointerenter", id, &i, &mut out);
                    self.send_event(doc, host, b"mouseenter", id, &i, &mut out);
                }
            }
            i.related = 0;
        }
        if target != 0 {
            self.send_event(doc, host, b"pointermove", target, &i, &mut out);
            self.send_event(doc, host, b"mousemove", target, &i, &mut out);
        }
        self.flush_pending(doc, host, &mut out);
        out
    }

    /// Profile 1 gives a widget no wheel and no keyboard (widget-sandbox docs/profile-v1.md, "Input"): both
    /// stay entry points so that hosts need not change, and do nothing.
    pub fn wheel(&mut self, _x: f64, _y: f64, _dx: f64, _dy: f64, _mode: i32, _modifiers: i32) -> Vec<Json> {
        Vec::new()
    }

    pub fn key(&mut self, _kind: &[u8], _key: &[u8], _code: &[u8], _repeat: bool, _modifiers: i32) -> Vec<Json> {
        Vec::new()
    }

    // ── images ──

    /// `load` (with naturalWidth/Height) or `error` for each <img> whose src changed.
    pub fn load_images(&mut self, doc: &mut Document, host: &mut dyn EventHost, out: &mut Vec<Json>) {
        for id in std::mem::take(&mut doc.pending_images) {
            let Some(r) = doc.node(id) else { continue };
            let n = doc.get(r);
            if n.tag != b"img" {
                continue;
            }
            let mut src = None;
            for (name, value) in &n.attrs {
                if name == b"src" {
                    src = Some(value.clone());
                }
            }
            let Some(src) = src.filter(|s| !s.is_empty()) else { continue };
            // Loaded like the document will load it: a detached <img> (new Image()) loads too, as in a browser.
            match host.image_of(&src) {
                None => self.image_event(doc, b"error", id, 0.0, 0.0, out),
                Some((_, w, h)) if w > 0.0 || h > 0.0 => self.image_event(doc, b"load", id, w, h, out),
                Some((image, _, _)) => self.waiting_images.entry(image).or_default().push(id),
            }
        }
    }

    fn image_event(&mut self, doc: &Document, kind: &[u8], id: i64, w: f32, h: f32, out: &mut Vec<Json>) {
        let mut ev = Json::object();
        ev.set("type", kind).set("targetId", id as f64);
        if kind == b"load" {
            ev.set("naturalWidth", f64::from(w)).set("naturalHeight", f64::from(h));
        }
        self.dispatch(doc, ev, out, true);
    }

    /// A backend that decodes images asynchronously reports the size of image `id` here; the document is
    /// rebuilt with it.
    pub fn image_ready(&mut self, doc: &mut Document, host: &mut dyn EventHost, id: i32, w: f32, h: f32) -> Vec<Json> {
        let mut out = Vec::new();
        if let Some(nodes) = self.waiting_images.remove(&id) {
            for node in nodes {
                self.image_event(doc, if w > 0.0 || h > 0.0 { b"load" } else { b"error" }, node, w, h, &mut out);
            }
        }
        if !host.image_ready(id, w, h) {
            return out;
        }
        self.rebuild(doc, host, false);
        host.report_size(&mut out);
        out
    }

    // ── focus ──

    /// The text control `id` is, or holds within a few levels (a tap on its inner text), or 0.
    fn text_control(&self, doc: &Document, id: i64) -> i64 {
        let mut n = doc.node(id);
        let mut depth = 0;
        while let Some(r) = n {
            if depth >= 6 {
                break;
            }
            let node = doc.get(r);
            if node.kind != NodeKind::Element || node.svg || (node.tag != b"input" && node.tag != b"textarea") {
                n = node.parent;
                depth += 1;
                continue;
            }
            let mut kind = Vec::new();
            for (name, value) in &node.attrs {
                if name == b"disabled" {
                    return 0;
                }
                if name == b"type" {
                    kind = css::trim(&css::lower(value));
                }
            }
            let editable = node.tag == b"textarea"
                || kind.is_empty()
                || kind == b"text"
                || kind == b"search"
                || kind == b"number"
                || kind == b"email"
                || kind == b"url"
                || kind == b"tel";
            return if editable && doc.attached(Some(r)) { node.id } else { 0 };
        }
        0
    }

    fn control_value(doc: &Document, r: NodeRef) -> Vec<u8> {
        let n = doc.get(r);
        if n.tag == b"textarea" {
            return n.data.clone();
        }
        for (name, value) in &n.attrs {
            if name == b"value" {
                return value.clone();
            }
        }
        Vec::new()
    }

    fn form_event(&mut self, doc: &Document, kind: &[u8], id: i64, out: &mut Vec<Json>, input_type: Option<&[u8]>) {
        if doc.frozen() || !self.listened(kind) {
            return;
        }
        let mut ev = Json::object();
        ev.set("type", kind).set("targetId", id as f64);
        if let Some(r) = doc.node(id) {
            ev.set("value", Events::control_value(doc, r));
        }
        if let Some(t) = input_type {
            ev.set("inputType", t);
        }
        self.dispatch(doc, ev, out, true);
    }

    /// The control's style changes when it is edited: its text goes transparent and its placeholder comes
    /// and goes, which only a build takes.
    fn restyle_control(doc: &mut Document, r: NodeRef) {
        doc.patch_log.valid = false;
        doc.note_attribute(r, b"style");
        doc.document_changed = true;
        doc.stale = true;
    }

    fn take_style(editing: &mut EditorSpec, style: EditorStyle) {
        editing.frame = style.frame;
        editing.font_family = style.font_family;
        editing.font_size = style.font_size;
        editing.font_weight = style.font_weight;
        editing.italic = style.italic;
        editing.color = style.color;
        editing.align = style.align;
        editing.font = style.font;
        editing.has_clip = style.has_clip;
        editing.clip = style.clip;
    }

    fn focus(&mut self, doc: &mut Document, host: &mut dyn EventHost, id: i64, out: &mut Vec<Json>) {
        if doc.frozen() {
            return;
        }
        if self.editing.id == id {
            // Focused again: the platform shows its editor again.
            self.editor_generation += 1;
            self.editing.generation = self.editor_generation;
            return;
        }
        if self.editing.id != 0 {
            self.blur(doc, host, out);
        }
        let Some(r) = doc.node(id) else { return };
        self.editing = EditorSpec::default();
        self.editing.id = id;
        self.editor_generation += 1;
        self.editing.generation = self.editor_generation;
        self.editing.multiline = doc.get(r).tag == b"textarea";
        self.editing.text = Events::control_value(doc, r);
        self.editing_initial = self.editing.text.clone();
        for (name, value) in &doc.get(r).attrs {
            if name == b"placeholder" {
                self.editing.placeholder = value.clone();
            } else if name == b"maxlength" {
                self.editing.max_length = (-1).max(twidget_cstd::atoi(c_str(value)));
            } else if name == b"readonly" {
                self.editing.read_only = true;
            } else if name == b"type" && !self.editing.multiline {
                self.editing.input_type = css::trim(&css::lower(value));
            }
        }
        if !self.editing.multiline && self.editing.input_type.is_empty() {
            self.editing.input_type = b"text".to_vec();
        }
        if let Some(style) = host.editor_style(id) {
            Events::take_style(&mut self.editing, style);
        }
        // The control's text goes transparent in the document: its style attribute changes.
        Events::restyle_control(doc, r);
        self.form_event(doc, b"focus", id, out, None);
        self.form_event(doc, b"focusin", id, out, None);
    }

    fn blur(&mut self, doc: &mut Document, _host: &mut dyn EventHost, out: &mut Vec<Json>) {
        let id = self.editing.id;
        if id == 0 {
            return;
        }
        self.editing = EditorSpec::default();
        self.editor_generation += 1;
        self.editing.generation = self.editor_generation;
        let Some(r) = doc.node(id) else { return };
        Events::restyle_control(doc, r);
        if Events::control_value(doc, r) != self.editing_initial {
            self.form_event(doc, b"change", id, out, None);
        }
        self.form_event(doc, b"blur", id, out, None);
        self.form_event(doc, b"focusout", id, out, None);
    }

    /// Brings the spec up to the document after a build: geometry and font; drops it when its control
    /// left the document.
    pub fn refresh_editor(&mut self, doc: &Document, host: &mut dyn EventHost) {
        if self.editing.id == 0 {
            return;
        }
        let r = doc.node(self.editing.id);
        if !r.is_some_and(|r| doc.attached(Some(r))) {
            self.editing = EditorSpec::default();
            self.editor_generation += 1;
            self.editing.generation = self.editor_generation;
            return;
        }
        let before = self.editing.clone();
        if let Some(style) = host.editor_style(self.editing.id) {
            Events::take_style(&mut self.editing, style);
        }
        // The document draws the edited text transparent: the editor keeps the colour it had.
        self.editing.color = before.color;
        let bits = |r: &Rect| [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()];
        let e = &self.editing;
        let moved = bits(&before.frame) != bits(&e.frame)
            || before.has_clip != e.has_clip
            || bits(&before.clip) != bits(&e.clip)
            || before.font != e.font
            || before.font_size != e.font_size
            || before.align != e.align
            || before.color != e.color;
        if moved {
            self.editor_generation += 1;
            self.editing.generation = self.editor_generation;
        }
    }

    /// The platform's editor changed the text of `generation`'s control: the value goes into the document
    /// and an input event to the widget. A stale generation is ignored.
    pub fn edit(&mut self, doc: &mut Document, host: &mut dyn EventHost, generation: u64, text: &[u8], input_type: &[u8]) -> Vec<Json> {
        let mut out = Vec::new();
        self.last_input = host.now_ms() / 1000.0;
        if doc.frozen() || self.editing.id == 0 || generation != self.editing.generation {
            return out;
        }
        let Some(r) = doc.node(self.editing.id) else { return out };
        let id = doc.get(r).id;
        self.editing.text = text.to_vec();
        self.editing.selection_start = -1;
        self.editing.selection_end = -1;
        // The value goes in as a script setting it would; the text stays hidden under the editor.
        let mut op = Json::object();
        op.set("op", "prop").set("id", id as f64).set("name", "value").set("value", text);
        doc.document_changed = false;
        let before = Before::of(doc);
        doc.apply_op(&op, &mut EditOp { editing: &mut self.editing, generation: &mut self.editor_generation }, &mut out);
        self.after(doc, host, before, false);
        self.editing.generation = generation;
        if doc.document_changed {
            self.rebuild(doc, host, true);
            host.report_size(&mut out);
        }
        self.form_event(doc, b"input", id, &mut out, Some(input_type));
        self.flush_pending(doc, host, &mut out);
        out
    }

    /// The platform's editor let go (the user dismissed it, or it lost the keyboard): change if the value
    /// differs from when editing began, then blur and focusout.
    pub fn end_editing(&mut self, doc: &mut Document, host: &mut dyn EventHost) -> Vec<Json> {
        let mut out = Vec::new();
        self.blur(doc, host, &mut out);
        if doc.stale {
            self.rebuild(doc, host, true);
            host.report_size(&mut out);
        }
        self.flush_pending(doc, host, &mut out);
        out
    }

    // ── text selection ──

    /// The text runs, built again only when the layout, the styles or the document changed. A restyle
    /// that moved nothing (a press's :active) keeps where the last paint drew each run, which is what
    /// shows until the next one, and the carets measured in it.
    pub fn text_runs(&mut self, host: &mut dyn EventHost) {
        let key = host.runs_key();
        if key == self.runs_key {
            return;
        }
        self.runs_key = key;
        let mut before = std::mem::take(&mut self.runs);
        self.runs = host.text_runs();
        if before.len() == self.runs.len() {
            let bits = |r: &Rect| [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()];
            for (r, old) in self.runs.iter_mut().zip(before.iter_mut()) {
                if r.owner != old.owner
                    || r.offset != old.offset
                    || r.font != old.font
                    || r.text != old.text
                    || bits(&r.rect) != bits(&old.rect)
                {
                    continue;
                }
                r.shown = old.shown;
                r.layer = old.layer;
                r.drawn = old.drawn;
                r.advances = std::mem::take(&mut old.advances);
            }
        }
    }

    /// Where run `i` shows now: the groups' transform around it, after its layer's track (`layers` caches
    /// those for a pass); None when its layer is not shown. Identity for a run no paint drew yet.
    fn run_shown(&self, host: &mut dyn EventHost, i: usize, now: f64, layers: &mut HashMap<i64, Option<Affine>>) -> Option<Affine> {
        let r = &self.runs[i];
        if !r.drawn {
            return Some(Affine::default());
        }
        if r.layer == 0 {
            return Some(r.shown);
        }
        let shown = *layers.entry(r.layer).or_insert_with(|| host.layer_shown(r.layer, now).map(|(m, _)| m));
        shown.map(|m| m * r.shown)
    }

    /// The text under `user-select: text` as laid out, for a host that selects it with its own UI (JSON).
    pub fn selectable_text_json(&mut self, host: &mut dyn EventHost) -> Vec<u8> {
        let mut out = Json::array();
        let mut layers = HashMap::new();
        let now = host.now_ms() / 1000.0;
        self.text_runs(host);
        for i in 0..self.runs.len() {
            let r = &self.runs[i];
            if !r.selectable {
                continue;
            }
            let (mut font, mut descent, mut letter, mut word) = (0, 0.0f32, 0.0f32, 0.0f32);
            if !r.line_break {
                match host.font_info(r.font) {
                    Some(info) => (font, descent, letter, word) = info,
                    None => continue,
                }
            }
            let mut run = Json::array();
            run.push(r.block).push(f64::from(r.rect.x)).push(f64::from(r.rect.y)).push(f64::from(r.rect.w)).push(f64::from(r.rect.h));
            // Where the run is drawn: draw_text puts the baseline `descent` above the bottom of its box.
            run.push(f64::from(if r.line_break { 0.0f32 } else { r.rect.y + r.rect.h - descent }))
                .push(font)
                .push(f64::from(letter))
                .push(f64::from(word))
                .push(r.text.clone());
            // Where it shows: the transforms around it as last painted, after its layer's track as it is now.
            // A run whose layer is hidden keeps its place in the text, at its layout position.
            let m = self.run_shown(host, i, now, &mut layers).unwrap_or_default();
            let mut matrix = Json::array();
            matrix
                .push(f64::from(m.a))
                .push(f64::from(m.b))
                .push(f64::from(m.c))
                .push(f64::from(m.d))
                .push(f64::from(m.e))
                .push(f64::from(m.f));
            run.push(matrix);
            out.push(run);
        }
        out.dump()
    }

    /// The run `p` falls in and the offset into it; None when its owner has no text now.
    fn locate(&self, p: &TextPoint) -> Option<(usize, usize)> {
        let mut found = None;
        for (i, r) in self.runs.iter().enumerate() {
            if r.owner != p.owner || p.offset < r.offset || p.offset > r.offset.wrapping_add(r.text.len()) {
                continue;
            }
            let offset = p.offset - r.offset;
            found = Some((i, offset));
            // The end of a run is also the start of the owner's next one: that one takes it.
            if offset < r.text.len() {
                return found;
            }
        }
        found
    }

    /// How far into run `i` its first `bytes` reach, in CSS px.
    fn run_advance(&mut self, host: &mut dyn EventHost, i: usize, bytes: usize) -> f32 {
        let run = &self.runs[i];
        if run.line_break || bytes == 0 {
            return 0.0;
        }
        if bytes >= run.text.len() {
            return run.rect.w;
        }
        if run.text.len() > MAX_MEASURED_RUN {
            return run.rect.w * bytes as f32 / run.text.len() as f32;
        }
        if let Some(&w) = run.advances.get(&bytes) {
            return w;
        }
        let w = host.text_width(c_str(&run.text[..bytes]), run.font);
        self.runs[i].advances.insert(bytes, w);
        w
    }

    /// The place nearest the point: the run on the closest line, the closest character boundary in it.
    fn point_at(&mut self, host: &mut dyn EventHost, x: f64, y: f64) -> Option<TextPoint> {
        let mut best: Option<usize> = None;
        let (mut best_dy, mut best_dx, mut best_x) = (0.0f64, 0.0f64, 0.0f64);
        // The point in each run's own coordinates: back through the layer's track as it shows now, then the
        // groups' transforms around the run.
        let mut layers = HashMap::new();
        let now = host.now_ms() / 1000.0;
        for i in 0..self.runs.len() {
            if self.runs[i].line_break {
                continue;
            }
            let (mut rx, mut ry) = (x, y);
            let Some(m) = self.run_shown(host, i, now, &mut layers) else { continue };
            if m != Affine::default() {
                let Some(inverse) = m.invert() else { continue };
                rx = f64::from(inverse.apply_x(x as f32, y as f32));
                ry = f64::from(inverse.apply_y(x as f32, y as f32));
            }
            let b = &self.runs[i].rect;
            let dy = if ry < f64::from(b.y) {
                f64::from(b.y) - ry
            } else if ry > f64::from(b.y + b.h) {
                ry - f64::from(b.y + b.h)
            } else {
                0.0
            };
            let dx = if rx < f64::from(b.x) {
                f64::from(b.x) - rx
            } else if rx > f64::from(b.x + b.w) {
                rx - f64::from(b.x + b.w)
            } else {
                0.0
            };
            if best.is_none() || dy < best_dy || (dy == best_dy && dx < best_dx) {
                best = Some(i);
                best_dy = dy;
                best_dx = dx;
                best_x = rx;
            }
        }
        let best = best?;
        let size = self.runs[best].text.len();
        let inside_sequence = |runs: &[TextRun], i: usize| i < size && (runs[best].text[i] & 0xC0) == 0x80;
        let target = best_x - f64::from(self.runs[best].rect.x);
        let mut bytes: usize;
        if size > MAX_MEASURED_RUN {
            // Evenly spread, as run_advance spreads them.
            let w = self.runs[best].rect.w;
            let share = if w > 0.0 { cpp_clamp(target / f64::from(w), 0.0, 1.0) } else { 0.0 };
            bytes = lround(share * size as f64) as usize;
            while inside_sequence(&self.runs, bytes) {
                bytes = bytes.wrapping_sub(1);
            }
        } else {
            // The character boundaries' advances grow along the run: the first one at or past the point, or
            // the one before it when that is closer.
            let mut stops = vec![0usize];
            for i in 1..=size {
                if !inside_sequence(&self.runs, i) {
                    stops.push(i);
                }
            }
            let (mut lo, mut hi) = (0usize, stops.len() - 1);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if f64::from(self.run_advance(host, best, stops[mid])) < target {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            bytes = stops[lo];
            if lo > 0 {
                let before = f64::from(self.run_advance(host, best, stops[lo - 1]));
                let at = f64::from(self.run_advance(host, best, stops[lo]));
                if target - before <= at - target {
                    bytes = stops[lo - 1];
                }
            }
        }
        Some(TextPoint { owner: self.runs[best].owner, offset: self.runs[best].offset.wrapping_add(bytes) })
    }

    /// The selection's ends in document order, as runs and offsets; None when there is no selected text.
    fn selection_range(&self) -> Option<(usize, usize, usize, usize)> {
        if self.selection.anchor == self.selection.focus {
            return None;
        }
        let (mut ar, mut ao) = self.locate(&self.selection.anchor)?;
        let (mut fr, mut fo) = self.locate(&self.selection.focus)?;
        if (ar, ao) > (fr, fo) {
            std::mem::swap(&mut ar, &mut fr);
            std::mem::swap(&mut ao, &mut fo);
        }
        Some((ar, ao, fr, fo))
    }

    /// Whether a mouse press on `target` starts a selection: `user-select` lets it, and no listener on the
    /// way prevents the press.
    fn starts_selection(&self, doc: &Document, host: &mut dyn EventHost, target: i64) -> bool {
        // A listener that prevents the press keeps it, as preventDefault on mousedown does in a browser.
        let mut n = doc.node(target);
        while let Some(r) = n {
            let node = doc.get(r);
            if self.prevent.contains(&key_of(node.id, b":pointerdown")) || self.prevent.contains(&key_of(node.id, b":mousedown")) {
                return false;
            }
            n = node.parent;
        }
        host.user_select_text(target)
    }

    /// The selected text, with a line break between blocks and at `<br>`; empty without a selection.
    pub fn selected_text(&mut self, host: &mut dyn EventHost) -> Vec<u8> {
        self.text_runs(host);
        let Some((first, first_offset, last, last_offset)) = self.selection_range() else { return Vec::new() };
        let space = |c: u8| c == b' ' || c == b'\n' || c == b'\t';
        let mut out: Vec<u8> = Vec::new();
        let mut previous: Option<usize> = None;
        for i in first..=last {
            let r = &self.runs[i];
            let from = if i == first { first_offset } else { 0 };
            let to = if i == last { last_offset } else { r.text.len() };
            if !r.selectable || from >= to {
                continue;
            }
            if let Some(p) = previous
                && let Some(&back) = out.last()
                && back != b'\n'
                && !r.line_break
            {
                let p = &self.runs[p];
                // Blocks apart by a line break; the lines of one block by a space, where a wrap ate the one between.
                if r.block != p.block {
                    out.push(b'\n');
                } else if r.rect.y >= p.rect.y + p.rect.h / 2.0 && !space(back) && !space(r.text[from]) {
                    out.push(b' ');
                }
            }
            out.extend_from_slice(&r.text[from..to]);
            previous = Some(i);
        }
        out
    }

    pub fn has_selection(&mut self, host: &mut dyn EventHost) -> bool {
        !self.selected_text(host).is_empty()
    }

    /// False when nothing is selected, without laying the text out; when true, has_selection() may still be false.
    pub fn may_have_selection(&self) -> bool {
        self.selection.anchor != self.selection.focus
    }

    /// Drops the selection; true when there was one, and the next paint draws no highlight.
    pub fn clear_selection(&mut self, host: &mut dyn EventHost) -> bool {
        let had = self.selection.anchor != self.selection.focus;
        self.selection = Selection::default();
        if had {
            host.selection_changed();
        }
        had
    }

    /// Whether any displayed element's text can be selected: a paint of a document without any does no more.
    pub fn selectable_text(&mut self, host: &mut dyn EventHost) -> bool {
        let key = host.selectable_key();
        if key == self.selectable_key {
            return self.selectable;
        }
        self.selectable_key = key;
        self.selectable = host.any_selectable();
        self.selectable
    }

    /// A paint begins: it finds the runs again, in the document or in a layer.
    pub fn paint_begins(&mut self, host: &mut dyn EventHost) {
        if self.selectable_text(host) {
            self.text_runs(host);
            for r in &mut self.runs {
                r.drawn = false;
            }
        }
    }

    /// Records where `list` (the document's, or promoted layer `layer`'s content) draws the selectable runs,
    /// and with `tint` puts the selection's tint after each selected one, inside its transforms and clips.
    /// `theme` is the root's variables, whose link colour the tint takes.
    pub fn paint_selection(
        &mut self,
        host: &mut dyn EventHost,
        theme: &[(Vec<u8>, Vec<u8>)],
        list: &mut Vec<Command>,
        layer: i64,
        tint: bool,
    ) {
        if !self.selectable_text(host) {
            return;
        }
        self.text_runs(host);
        // A run's Text command starts where its box does, in the coordinates of the groups around it.
        // A box laid out at 1e30 px has no row a long can hold: clamped, so that the loop over rows below ends.
        let row_of = |y: f32| -> i64 { if y.is_finite() { y.clamp(-1e9, 1e9).floor() as i32 as i64 } else { 0 } };
        let mut rows: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, r) in self.runs.iter().enumerate() {
            if r.selectable && !r.line_break {
                rows.entry(row_of(r.rect.y)).or_default().push(i);
            }
        }
        if rows.is_empty() {
            return;
        }
        let run_at = |runs: &[TextRun], drawn: &Rect| -> Option<usize> {
            let (below, above) = (row_of(drawn.y - 0.05), row_of(drawn.y + 0.05));
            for row in below..=above {
                let Some(list) = rows.get(&row) else { continue };
                for &i in list {
                    let r = &runs[i];
                    if (r.rect.x - drawn.x).abs() < 0.05 && (r.rect.y - drawn.y).abs() < 0.05 {
                        return Some(i);
                    }
                }
            }
            None
        };
        let range = self.selection_range();
        let tint = tint && range.is_some();
        let (first, first_offset, last, last_offset) = range.unwrap_or((0, 0, 0, 0));
        // Over the text, in the theme's link colour: a tint the text shows through, on every painter.
        let mut color = Color::rgba(51, 144, 236, 255);
        if tint {
            for (name, value) in theme {
                if name == b"--tg-theme-link-color"
                    && let Some(c) = css::parse_color(value)
                {
                    color = c;
                }
            }
            color.a = lround(f64::from(color.a) * 0.3) as u8;
        }
        let mut frames = vec![Affine::default()];
        let mut tints: Vec<(usize, Command)> = Vec::new();
        let mut placed: HashSet<usize> = HashSet::new();
        for (i, c) in list.iter().enumerate() {
            if c.kind == Cmd::GroupPush {
                let m = c.matrix;
                let top = *frames.last().unwrap_or(&Affine::default());
                frames.push(top * Affine { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] });
                continue;
            }
            if c.kind == Cmd::GroupPop {
                if frames.len() > 1 {
                    frames.pop();
                }
                continue;
            }
            if c.kind != Cmd::Text {
                continue;
            }
            let Some(k) = run_at(&self.runs, &c.rect) else { continue };
            // Once: a run drawn again (a text shadow) has its tint after the first.
            if !placed.insert(k) {
                continue;
            }
            let r = &mut self.runs[k];
            r.shown = *frames.last().unwrap_or(&Affine::default());
            r.layer = layer;
            r.drawn = true;
            if !tint || k < first || k > last {
                continue;
            }
            let from = if k == first { first_offset } else { 0 };
            let to = if k == last { last_offset } else { r.text.len() };
            if from >= to {
                continue;
            }
            let x0 = self.run_advance(host, k, from);
            let x1 = self.run_advance(host, k, to);
            if x1 <= x0 {
                continue;
            }
            let r = &self.runs[k];
            let t =
                Command { kind: Cmd::FillRect, rect: Rect::new(r.rect.x + x0, r.rect.y, x1 - x0, r.rect.h), color, ..Command::default() };
            tints.push((i + 1, t));
        }
        if tints.is_empty() {
            return;
        }
        let mut out = Vec::with_capacity(list.len() + tints.len());
        let mut next = tints.into_iter().peekable();
        let old = std::mem::take(list);
        let n = old.len();
        let mut items = old.into_iter();
        for i in 0..=n {
            while let Some((at, _)) = next.peek() {
                if *at != i {
                    break;
                }
                if let Some((_, t)) = next.next() {
                    out.push(t);
                }
            }
            if i < n
                && let Some(c) = items.next()
            {
                out.push(c);
            }
        }
        *list = out;
    }
}

/// `std::clamp(v, lo, hi)`: `v < lo ? lo : hi < v ? hi : v`, a NaN through.
fn cpp_clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `std::lround`: half away from zero.
fn lround(v: f64) -> i64 {
    v.round() as i64
}
