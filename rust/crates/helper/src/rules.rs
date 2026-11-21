//! tdesktop's `tgw/tgw_renderer_rules.h`: what the hosted glue and Telegram's in-process renderer decide alike
//! about a widget's renderer messages: the load's two halves, held context menus, the pointer's hover and active
//! targets, local queries and the settle before a paint, the device ratio's bounds, and the canvas fonts' allowance.

use std::collections::HashMap;

use twidget_json::Json;
use twidget_renderer::BuildStats;

pub const READY_MESSAGE: &[u8] = b"{\"type\":\"ready\"}";
pub const MIN_RATIO: f64 = 0.25;
pub const MAX_RATIO: f64 = 64.0;
pub const TOUCH_DOWN: i32 = 0;
pub const TOUCH_MOVE: i32 = 1;
pub const TOUCH_UP: i32 = 2;
pub const TOUCH_CANCEL: i32 = 3;
pub const HOVER_MOVE: i32 = 0;
pub const HOVER_LEAVE: i32 = 1;
pub const MOUSE_POINTER: i32 = 0;
pub const FINGER_POINTER: i32 = 1;
pub const PRIMARY_BUTTON: i32 = 0;
pub const SECONDARY_BUTTON: i32 = 2;
pub const LOCAL_QUERY_ID: i32 = 0;
/// Mirrors the private `twidget::WidgetHost::kRenderAckQid`, its post-load query.
pub const LOAD_ACK_QUERY_ID: i32 = -1;

pub const MAX_NEW_CANVAS_FONTS: i32 = 16;
pub const MAX_CANVAS_FONTS: i32 = 32;
pub const MAX_CANVAS_FONT_PARSES: i32 = 64;
pub const MAX_CANVAS_FONT_REQUESTS: i32 = 64;
pub const MAX_CANVAS_FONT_SPELLINGS: usize = 64;
pub const MAX_CANVAS_FONT_SPELLING_SIZE: usize = 512;
pub const CANVAS_FONT_LIMIT_KIND: &[u8] = b"canvas-font-limit";

/// What one renderer message changed of the load: the first content drawn, the load complete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadChange {
    pub content: bool,
    pub loaded: bool,
}

/// The elements the pointer hovers and holds active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoverTargets {
    pub hover: i64,
    pub active: i64,
}

/// The canvas font spellings a widget used, each parsed once: at most `MAX_CANVAS_FONT_PARSES` new ones between
/// refills, each at most `MAX_CANVAS_FONT_SPELLING_SIZE` bytes, and at most `MAX_CANVAS_FONT_SPELLINGS` kept (all
/// dropped when one more comes).
#[derive(Clone, Debug)]
pub struct CanvasSpellings<Entry> {
    entries: HashMap<Vec<u8>, Entry>,
    parses_left: i32,
}

impl<Entry> Default for CanvasSpellings<Entry> {
    fn default() -> CanvasSpellings<Entry> {
        CanvasSpellings { entries: HashMap::new(), parses_left: MAX_CANVAS_FONT_PARSES }
    }
}

impl<Entry> CanvasSpellings<Entry> {
    /// The entry of `css`, parsed by `parse` when it is new; None for a spelling too long and, once the parses
    /// ran out, for a new one.
    pub fn find(&mut self, css: &[u8], parse: impl FnOnce(&[u8]) -> Entry) -> Option<&mut Entry> {
        if css.len() > MAX_CANVAS_FONT_SPELLING_SIZE {
            return None;
        }
        if self.entries.contains_key(css) {
            return self.entries.get_mut(css);
        } else if self.parses_left <= 0 {
            return None;
        }
        self.parses_left -= 1;
        let entry = parse(css);
        if self.entries.len() >= MAX_CANVAS_FONT_SPELLINGS {
            // Its entries own no fonts, so clearing it mid-message is safe.
            self.entries.clear();
        }
        Some(self.entries.entry(css.to_vec()).or_insert(entry))
    }

    pub fn refill(&mut self) {
        self.parses_left = MAX_CANVAS_FONT_PARSES;
    }

    /// The spellings kept.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The device ratio within `MIN_RATIO` and `MAX_RATIO`, `MIN_RATIO` for NaN.
pub fn device_ratio(ratio: f64) -> f64 {
    // std::clamp: the bounds only when the value is outside them, which f64::clamp does for a number.
    if ratio.is_nan() { MIN_RATIO } else { ratio.clamp(MIN_RATIO, MAX_RATIO) }
}

/// A press moved past the touch slop.
pub fn outside_slop(dx: f64, dy: f64) -> bool {
    dx.hypot(dy) > twidget_gesture::TOUCH_SLOP_PX
}

/// The documents the renderer parsed anew (its builds that did not patch).
pub fn parsed_documents(stats: &BuildStats) -> i32 {
    stats.builds.wrapping_sub(stats.patched)
}

/// Whom the pointer hovers and holds active: while a press is held, the pressed element if the document is the
/// one it was pressed in, else nothing.
pub fn pointer_targets(hit: i64, press_held: bool, synced: bool, press_hit: i64, active: bool) -> HoverTargets {
    let hover = if press_held { if synced { press_hit } else { 0 } } else { hit };
    HoverTargets { hover, active: if synced && active && hover == press_hit { hover } else { 0 } }
}

/// `OfType(type)(message)`: the message's `type` is `ty`.
pub fn of_type(message: &Json, ty: &[u8]) -> bool {
    message.get("type").as_str() == ty
}

/// The engine's frame message at `clock_seconds` (`twlite::Renderer::clock()`, the steady clock's seconds).
pub fn frame_message(clock_seconds: f64) -> Json {
    let mut result = Json::object();
    result.set("type", "frame");
    result.set("time", clock_seconds * 1000.0);
    result
}

pub fn local_query(query: Json) -> Json {
    let mut result = Json::object();
    result.set("type", "query");
    result.set("qid", LOCAL_QUERY_ID);
    result.set("query", query);
    result
}

pub fn viewport_query() -> Json {
    let mut query = Json::object();
    query.set("q", "viewport");
    local_query(query)
}

pub fn kept_before_ready(message: &Json) -> bool {
    of_type(message, b"size")
}

pub fn draws_content(message: &Json) -> bool {
    of_type(message, b"ops") && !message.get("ops").arr().is_empty()
}

pub fn is_load_ack(message: &Json) -> bool {
    let qid = message.get("qid");
    of_type(message, b"query") && qid.is_number() && qid.as_int(0) == LOAD_ACK_QUERY_ID
}

/// The load after `message`: content once a batch drew some, loaded once the post-load query came too.
pub fn advance_load(message: &Json, has_content: &mut bool, acked: &mut bool, loaded: &mut bool) -> LoadChange {
    let mut result = LoadChange::default();
    if draws_content(message) {
        result.content = !*has_content;
        *has_content = true;
    } else if is_load_ack(message) {
        *acked = true;
    }
    if *acked && *has_content && !*loaded {
        *loaded = true;
        result.loaded = true;
    }
    result
}

pub fn is_context_menu(message: &Json, pointer_type: &[u8]) -> bool {
    let event = message.get("ev");
    of_type(message, b"event") && event.get("type").as_str() == b"contextmenu" && event.get("pointerType").as_str() == pointer_type
}

pub fn is_click(message: &Json) -> bool {
    of_type(message, b"event") && message.get("ev").get("type").as_str() == b"click"
}

/// The `contextmenu` events of `pointer_type` taken out of `out`, in their order; the rest stay in theirs.
pub fn take_context_menus(out: &mut Vec<Json>, pointer_type: &[u8]) -> Vec<Json> {
    let (kept, menus): (Vec<Json>, Vec<Json>) = std::mem::take(out).into_iter().partition(|m| !is_context_menu(m, pointer_type));
    *out = kept;
    menus
}

pub fn append(to: &mut Vec<Json>, from: Vec<Json>) {
    to.extend(from);
}

/// What `receive` answers the viewport query with, its `result`s dropped: the batch a frame waits for built, its
/// reports.
pub fn take_settled(receive: impl FnOnce(&Json) -> Vec<Json>) -> Vec<Json> {
    let mut out = receive(&viewport_query());
    out.retain(|m| !of_type(m, b"result"));
    out
}

pub fn canvas_font_limit_text() -> String {
    format!(
        "canvas text exceeded the allowance of {MAX_NEW_CANVAS_FONTS} new fonts per frame, {MAX_CANVAS_FONTS} per widget, \
         {MAX_CANVAS_FONT_PARSES} font strings per frame of at most {MAX_CANVAS_FONT_SPELLING_SIZE} bytes; text in other fonts is skipped"
    )
}
