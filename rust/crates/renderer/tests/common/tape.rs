//! The tape `oracle/renderer_probe.h` writes: every answer litehtml's document and the container gave renderer.cpp, in
//! order, with the arguments it was asked with. [`TapeEngine`] answers the port's [`Engine`] from it, checking every
//! question is the C++'s, in its order, with its arguments (floats by their bits, texts by FNV-1a where the tape keeps
//! a digest).
//!
//! Entries (a tag, then little-endian fields; `str` is a u32 length and bytes, `rect` four f32): 'S' u32 a step
//! begins; 'V' str a violation the container pushed, which the next answer brings. The input's questions as
//! events_probe.h writes them ('h' 'b' 'x' 'a' 'u' 'm' 'v' 'c' 'K' 'R' 'w' 'f' 'k' 'j' 'e' 'i' 'I' 'p'; 'L' is the port's). The
//! document's life: 'n' u64 elements; 'W' the layout's words measured; 'r' f32 width, u64 budget; 'E' u8; 's' u64 boxes; 'z' u64 html digest, u32
//! length, u8; 'D' the same, u8 patched, u8 changed; 'M'; 'y' u8; 'C' html digest and length, master digest and
//! length, u8 made; 'X' index; 'H' f64; 'o' u8; 'q' u8; 'F' u64 fonts; 'J' u32 changes (i64 owner, str name, str
//! value, u8 text), u8 ok, u8 changed; 'Z' u32 patches (i64 id, str style), u8 ok, u8 changed; 'N' u64 names, u64
//! bytes; 't' trim; 'd' the names, gates and always asked, u32 elements (i64 id, u32 declarations of str, str), u64
//! generation; 'g' u32 (i64 id, rect). The queries: '1' i64 id, rect; '2' i64 id, u32 rects; '3' i64 id, rect in,
//! rect out; '4' i64 id, rect; '5' i64 id, u8 has, 4 f32; '6' i64 id, f64 left, f64 top, i64 parent; '7' i64 id, u8
//! scroller, 4 f32; '8' i64 id, u8 has, 6 f32; '9' i64 id, str pseudo, u32 animated (str, str), str JSON; '0' i64 id,
//! str name, str value; '!' i64 id, u8 ok, 4 f32; '@' i64 id, u8; '#' i64 id, f32 at, f32 length, f32 x, f32 y;
//! '$' i64 id, u8 ok, 4 f32. Stage 4.5's (a list is u8 the wire takes it, str its wire form; ids are u32 and i64s,
//! a set of elements sorted): 'A' i64 id, u8 background is local; '[' i64 id, u8 movers given, ids, u8 ok, rect
//! when ok; ']' i64 id, u8 ok, when ok u8 has, when has rect and 8 f32 radii; '~' i64 id, u8 ok, when ok tile,
//! area and clip rects, radii, list; '{' i64 id, u32 declarations (str, str); '|' paint_order; '<' i64 id, u64
//! index; '?' i64 id, rect, ignored ids, u8; '&' i64 id, rect, ignored ids, u64 limit, u8 found, ids when found;
//! '+' skipped ids, background ids, list; '*' i64 id, skipped ids, rect bounds, list; '=' list; '^' u64 document,
//! u32 paint generation, u64 layouts; '/' i64 id, f32 x, f32 y, u8 placement, i64 hit, rect when asked and hit;
//! 'Q' a walk of the boxes begins, ';' it ends; '%' i64 key, u8 pseudo, u8 backdrop, u8 filter, u8 blurred, u8
//! dimmed, f32 opacity; '-' set_no_effects.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use twidget_animations::{Declared, StyleValues};
use twidget_events::{EditorStyle, TextRun};
use twidget_json::Json;
use twidget_renderer::{BoxStyle, Clip, Engine, HoleChange, ScrollExtent, TextContent, Tile};
use twidget_wire::{Caps, Color, DisplayList, Radii, Rect, decode_list};

use super::{Tally, fnv64};

#[derive(Clone, Debug)]
pub enum Entry {
    Step(u32),
    Violation(Vec<u8>),
    // a batch the container handed the stub, as the stub's log line ('T')
    Widths(Vec<u8>),
    // the input's
    Hit { x: f64, y: f64, placement: bool, id: i64, rect: Rect },
    Box { id: i64, rect: Rect },
    Exists { id: i64, yes: bool },
    TouchAction { id: i64, value: Option<Vec<u8>> },
    UserSelect { id: i64, yes: bool },
    Mouse { action: i32, x: f64, y: f64, hovers: bool, redraw: bool },
    Leave(bool),
    Cursor(Vec<u8>),
    RunsKey([u64; 4]),
    Runs(Vec<TextRun>),
    Width { text: Vec<u8>, font: u64, w: f32 },
    FontInfo { font: u64, info: Option<(i32, f32, f32, f32)> },
    SelectableKey([u64; 2]),
    Selectable(bool),
    EditorStyle { id: i64, style: Option<EditorStyle> },
    ImageOf { src: Vec<u8>, answer: Option<(i32, f32, f32)> },
    ImageReady { id: i32, w: f32, h: f32, changed: bool },
    Content { id: i64, size: (f32, f32) },
    // the document's life
    Count(u64),
    Words,
    Render { width: f32, budget: u64 },
    Exhausted(bool),
    Spent(u64),
    ViewportUnits { html: (u64, u32), yes: bool },
    Update { html: (u64, u32), ok: bool, changed: bool },
    MediaChanged,
    PaintOnly(bool),
    Create { html: (u64, u32), master: (u64, u32), made: bool },
    Index,
    Height(f64),
    Root(bool),
    ReadsHeight(bool),
    Fonts(u64),
    Holes { changes: Vec<HoleChange>, ok: bool, changed: bool },
    Styles { patches: Vec<(i64, Vec<u8>)>, ok: bool, changed: bool },
    Usage(u64, u64),
    Trim,
    Declarations { names: Vec<Vec<u8>>, gates: Vec<Vec<u8>>, always: Vec<i64>, found: BTreeMap<i64, Declared>, generation: u64 },
    Edges(Vec<(i64, Rect)>),
    // the queries'
    Visual { id: i64, rect: Rect },
    Lines { id: i64, rects: Vec<Rect> },
    Transformed { id: i64, input: Rect, rect: Rect },
    Border { id: i64, rect: Rect },
    Borders { id: i64, borders: Option<[f32; 4]> },
    Offsets { id: i64, left: f64, top: f64, parent: i64 },
    Extent { id: i64, extent: ScrollExtent },
    Textarea { id: i64, content: Option<TextContent> },
    Computed { id: i64, pseudo: Vec<u8>, animated: StyleValues, json: Vec<u8> },
    Custom { id: i64, name: Vec<u8>, value: Vec<u8> },
    SvgText { id: i64, text: Option<[f32; 4]> },
    SvgGeometry { id: i64, yes: bool },
    PathLength { id: i64, at: f32, length: f32, x: f32, y: f32 },
    Bounds { id: i64, bounds: Option<[f32; 4]> },
    // stage 4.5's
    Local { id: i64, yes: bool },
    Candidate { id: i64, movers: Option<Vec<i64>>, rect: Option<Rect> },
    ClipOf { id: i64, clip: Option<Clip> },
    TileOf { id: i64, tile: Option<Tile> },
    Declared { id: i64, values: Declared },
    PaintOrder,
    Order { id: i64, index: u64 },
    Covered { id: i64, area: Rect, ignore: Vec<i64>, yes: bool },
    Obscurers { id: i64, area: Rect, ignore: Vec<i64>, limit: u64, found: Option<Vec<i64>> },
    Drawn { skip: Vec<i64>, backgrounds: Vec<i64>, list: DisplayList },
    LayerDrawn { id: i64, skip: Vec<i64>, bounds: Rect, list: DisplayList },
    Plain(DisplayList),
    PaintKey(u64, u32, u64),
    HitWithin { id: i64, x: f32, y: f32, placement: bool, hit: i64, rect: Rect },
    ScanBegins,
    ScanEnds,
    BoxFacts { key: i64, pseudo: bool, style: BoxStyle },
    NoEffects,
}

impl Entry {
    pub fn name(&self) -> &'static str {
        match self {
            Entry::Step(_) => "a step",
            Entry::Violation(_) => "a violation",
            Entry::Widths(_) => "a widths batch",
            Entry::Hit { .. } => "hit",
            Entry::Box { .. } => "box_of",
            Entry::Exists { .. } => "has_element",
            Entry::TouchAction { .. } => "touch_action",
            Entry::UserSelect { .. } => "user_select_text",
            Entry::Mouse { .. } => "mouse",
            Entry::Leave(_) => "leave",
            Entry::Cursor(_) => "cursor",
            Entry::RunsKey(_) => "runs_key",
            Entry::Runs(_) => "text_runs",
            Entry::Width { .. } => "text_width",
            Entry::FontInfo { .. } => "font_info",
            Entry::SelectableKey(_) => "selectable_key",
            Entry::Selectable(_) => "any_selectable",
            Entry::EditorStyle { .. } => "editor_style",
            Entry::ImageOf { .. } => "image_of",
            Entry::ImageReady { .. } => "image_ready",
            Entry::Content { .. } => "content_size",
            Entry::Count(_) => "element_count",
            Entry::Words => "measure_words",
            Entry::Render { .. } => "render",
            Entry::Exhausted(_) => "layout_exhausted",
            Entry::Spent(_) => "layout_spent",
            Entry::ViewportUnits { .. } => "has_viewport_units",
            Entry::Update { .. } => "update_from_string",
            Entry::MediaChanged => "media_changed",
            Entry::PaintOnly(_) => "last_patch_paint_only",
            Entry::Create { .. } => "create",
            Entry::Index => "index",
            Entry::Height(_) => "height",
            Entry::Root(_) => "has_root",
            Entry::ReadsHeight(_) => "layout_reads_viewport_height",
            Entry::Fonts(_) => "font_count",
            Entry::Holes { .. } => "update_holes",
            Entry::Styles { .. } => "update_styles",
            Entry::Usage(..) => "name_usage",
            Entry::Trim => "trim",
            Entry::Declarations { .. } => "declarations",
            Entry::Edges(_) => "edges",
            Entry::Visual { .. } => "visual_box",
            Entry::Lines { .. } => "inline_boxes",
            Entry::Transformed { .. } => "transformed_box",
            Entry::Border { .. } => "border_box",
            Entry::Borders { .. } => "borders",
            Entry::Offsets { .. } => "offsets",
            Entry::Extent { .. } => "scroll_extent",
            Entry::Textarea { .. } => "textarea_content",
            Entry::Computed { .. } => "computed_style",
            Entry::Custom { .. } => "custom_property",
            Entry::SvgText { .. } => "svg_text_box",
            Entry::SvgGeometry { .. } => "svg_geometry",
            Entry::PathLength { .. } => "svg_path_length",
            Entry::Bounds { .. } => "svg_path_bounds",
            Entry::Local { .. } => "background_is_local",
            Entry::Candidate { .. } => "layer_candidate",
            Entry::ClipOf { .. } => "clip_of",
            Entry::TileOf { .. } => "background_tile",
            Entry::Declared { .. } => "declared",
            Entry::PaintOrder => "paint_order",
            Entry::Order { .. } => "paint_index",
            Entry::Covered { .. } => "covered",
            Entry::Obscurers { .. } => "obscurers",
            Entry::Drawn { .. } => "draw_skipping",
            Entry::LayerDrawn { .. } => "draw_layer",
            Entry::Plain(_) => "draw",
            Entry::PaintKey(..) => "paint_key",
            Entry::HitWithin { .. } => "hit_within",
            Entry::ScanBegins => "boxes",
            Entry::ScanEnds => "the end of the boxes",
            Entry::BoxFacts { .. } => "a box's facts",
            Entry::NoEffects => "set_no_effects",
        }
    }
}

struct Take<'a> {
    s: &'a [u8],
    at: usize,
}

impl Take<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).ok_or("a length past the tape")?;
        let v = self.s.get(self.at..end).ok_or_else(|| format!("cut at {}", self.at))?;
        self.at = end;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.u8()? != 0)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.bytes(N)?.try_into().unwrap_or([0; N]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.arr()?))
    }
    fn str(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        Ok(self.bytes(n)?.to_vec())
    }
    fn rect(&mut self) -> Result<Rect, String> {
        Ok(Rect::new(self.f32()?, self.f32()?, self.f32()?, self.f32()?))
    }
    fn four(&mut self) -> Result<[f32; 4], String> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > self.s.len() {
            return Err(format!("a count of {n} past the tape"));
        }
        Ok(n)
    }
    fn digest(&mut self) -> Result<(u64, u32), String> {
        Ok((self.u64()?, self.u32()?))
    }
    fn ids(&mut self) -> Result<Vec<i64>, String> {
        let n = self.count()?;
        (0..n).map(|_| self.i64()).collect()
    }
    fn radii(&mut self) -> Result<Radii, String> {
        Ok(Radii {
            tlx: self.f32()?,
            tly: self.f32()?,
            trx: self.f32()?,
            try_: self.f32()?,
            brx: self.f32()?,
            bry: self.f32()?,
            blx: self.f32()?,
            bly: self.f32()?,
        })
    }
    fn list(&mut self) -> Result<DisplayList, String> {
        let taken = self.flag()?;
        let bytes = self.str()?;
        if !taken {
            return Err(format!("a display list the wire does not take, at {}", self.at));
        }
        decode_list(&bytes, &Caps::default()).ok_or_else(|| format!("a display list that does not decode, at {}", self.at))
    }

    fn entry(&mut self) -> Result<Entry, String> {
        let tag = self.u8()?;
        Ok(match tag {
            b'S' => Entry::Step(self.u32()?),
            b'V' => Entry::Violation(self.str()?),
            b'T' => {
                let start = self.at - 1;
                let n = self.count()?;
                for _ in 0..n {
                    self.i32()?;
                    self.str()?;
                }
                Entry::Widths(self.s[start..self.at].to_vec())
            }
            b'h' => {
                let (x, y, placement, id) = (self.f64()?, self.f64()?, self.flag()?, self.i64()?);
                let rect = if placement { self.rect()? } else { Rect::default() };
                Entry::Hit { x, y, placement, id, rect }
            }
            b'b' => Entry::Box { id: self.i64()?, rect: self.rect()? },
            b'x' => Entry::Exists { id: self.i64()?, yes: self.flag()? },
            b'a' => {
                let id = self.i64()?;
                let value = if self.flag()? { Some(self.str()?) } else { None };
                Entry::TouchAction { id, value }
            }
            b'u' => Entry::UserSelect { id: self.i64()?, yes: self.flag()? },
            b'm' => Entry::Mouse { action: self.i32()?, x: self.f64()?, y: self.f64()?, hovers: self.flag()?, redraw: self.flag()? },
            b'v' => Entry::Leave(self.flag()?),
            b'c' => Entry::Cursor(self.str()?),
            b'K' => Entry::RunsKey([self.u64()?, self.u64()?, self.u64()?, self.u64()?]),
            b'R' => {
                let n = self.count()?;
                let mut runs = Vec::with_capacity(n);
                for _ in 0..n {
                    runs.push(TextRun {
                        owner: self.i64()?,
                        offset: self.u64()? as usize,
                        text: self.str()?,
                        rect: self.rect()?,
                        font: self.u64()?,
                        block: self.i32()?,
                        selectable: self.flag()?,
                        line_break: self.flag()?,
                        ..TextRun::default()
                    });
                }
                Entry::Runs(runs)
            }
            b'w' => Entry::Width { text: self.str()?, font: self.u64()?, w: self.f32()? },
            b'f' => {
                let font = self.u64()?;
                let info = if self.flag()? { Some((self.i32()?, self.f32()?, self.f32()?, self.f32()?)) } else { None };
                Entry::FontInfo { font, info }
            }
            b'k' => Entry::SelectableKey([self.u64()?, self.u64()?]),
            b'j' => Entry::Selectable(self.flag()?),
            b'e' => {
                let id = self.i64()?;
                let style = if self.flag()? {
                    Some(EditorStyle {
                        frame: self.rect()?,
                        font_family: self.str()?,
                        font_size: self.f32()?,
                        font_weight: self.i32()?,
                        italic: self.flag()?,
                        color: Color { r: self.u8()?, g: self.u8()?, b: self.u8()?, a: self.u8()? },
                        align: self.i32()?,
                        font: self.i32()?,
                        has_clip: self.flag()?,
                        clip: self.rect()?,
                    })
                } else {
                    None
                };
                Entry::EditorStyle { id, style }
            }
            b'i' => {
                let src = self.str()?;
                let answer = if self.flag()? { Some((self.i32()?, self.f32()?, self.f32()?)) } else { None };
                Entry::ImageOf { src, answer }
            }
            b'I' => Entry::ImageReady { id: self.i32()?, w: self.f32()?, h: self.f32()?, changed: self.flag()? },
            b'p' => Entry::Content { id: self.i64()?, size: (self.f32()?, self.f32()?) },
            b'n' => Entry::Count(self.u64()?),
            b'r' => Entry::Render { width: self.f32()?, budget: self.u64()? },
            b'E' => Entry::Exhausted(self.flag()?),
            b's' => Entry::Spent(self.u64()?),
            b'z' => Entry::ViewportUnits { html: self.digest()?, yes: self.flag()? },
            b'D' => Entry::Update { html: self.digest()?, ok: self.flag()?, changed: self.flag()? },
            b'M' => Entry::MediaChanged,
            b'W' => Entry::Words,
            b'y' => Entry::PaintOnly(self.flag()?),
            b'C' => Entry::Create { html: self.digest()?, master: self.digest()?, made: self.flag()? },
            b'X' => Entry::Index,
            b'H' => Entry::Height(self.f64()?),
            b'o' => Entry::Root(self.flag()?),
            b'q' => Entry::ReadsHeight(self.flag()?),
            b'F' => Entry::Fonts(self.u64()?),
            b'J' => {
                let n = self.count()?;
                let mut changes = Vec::with_capacity(n);
                for _ in 0..n {
                    changes.push(HoleChange { owner: self.i64()?, name: self.str()?, value: self.str()?, text: self.flag()? });
                }
                Entry::Holes { changes, ok: self.flag()?, changed: self.flag()? }
            }
            b'Z' => {
                let n = self.count()?;
                let mut patches = Vec::with_capacity(n);
                for _ in 0..n {
                    patches.push((self.i64()?, self.str()?));
                }
                Entry::Styles { patches, ok: self.flag()?, changed: self.flag()? }
            }
            b'N' => Entry::Usage(self.u64()?, self.u64()?),
            b't' => Entry::Trim,
            b'd' => {
                let n = self.count()?;
                let names = (0..n).map(|_| self.str()).collect::<Result<Vec<_>, _>>()?;
                let n = self.count()?;
                let gates = (0..n).map(|_| self.str()).collect::<Result<Vec<_>, _>>()?;
                let n = self.count()?;
                let always = (0..n).map(|_| self.i64()).collect::<Result<Vec<_>, _>>()?;
                let n = self.count()?;
                let mut found = BTreeMap::new();
                for _ in 0..n {
                    let id = self.i64()?;
                    let m = self.count()?;
                    let mut declared = Declared::new();
                    for _ in 0..m {
                        let k = self.str()?;
                        declared.insert(k, self.str()?);
                    }
                    found.insert(id, declared);
                }
                Entry::Declarations { names, gates, always, found, generation: self.u64()? }
            }
            b'g' => {
                let n = self.count()?;
                Entry::Edges((0..n).map(|_| Ok((self.i64()?, self.rect()?))).collect::<Result<Vec<_>, String>>()?)
            }
            b'1' => Entry::Visual { id: self.i64()?, rect: self.rect()? },
            b'2' => {
                let id = self.i64()?;
                let n = self.count()?;
                Entry::Lines { id, rects: (0..n).map(|_| self.rect()).collect::<Result<Vec<_>, _>>()? }
            }
            b'3' => Entry::Transformed { id: self.i64()?, input: self.rect()?, rect: self.rect()? },
            b'4' => Entry::Border { id: self.i64()?, rect: self.rect()? },
            b'5' => {
                let id = self.i64()?;
                let borders = if self.flag()? { Some(self.four()?) } else { None };
                Entry::Borders { id, borders }
            }
            b'6' => Entry::Offsets { id: self.i64()?, left: self.f64()?, top: self.f64()?, parent: self.i64()? },
            b'7' => Entry::Extent {
                id: self.i64()?,
                extent: ScrollExtent {
                    scroller: self.flag()?,
                    pad_right: self.f32()?,
                    pad_bottom: self.f32()?,
                    right: self.f32()?,
                    bottom: self.f32()?,
                },
            },
            b'8' => {
                let id = self.i64()?;
                let content = if self.flag()? {
                    let (width, height) = (self.f32()?, self.f32()?);
                    Some(TextContent { width, height, pad: self.four()? })
                } else {
                    // renderer_probe.h writes the sizes only when the textarea lays out inline content.
                    None
                };
                Entry::Textarea { id, content }
            }
            b'9' => {
                let (id, pseudo) = (self.i64()?, self.str()?);
                let n = self.count()?;
                let animated = (0..n).map(|_| Ok((self.str()?, self.str()?))).collect::<Result<Vec<_>, String>>()?;
                Entry::Computed { id, pseudo, animated, json: self.str()? }
            }
            b'0' => Entry::Custom { id: self.i64()?, name: self.str()?, value: self.str()? },
            b'!' => {
                let id = self.i64()?;
                let text = if self.flag()? { Some(self.four()?) } else { None };
                Entry::SvgText { id, text }
            }
            b'@' => Entry::SvgGeometry { id: self.i64()?, yes: self.flag()? },
            b'#' => Entry::PathLength { id: self.i64()?, at: self.f32()?, length: self.f32()?, x: self.f32()?, y: self.f32()? },
            b'$' => {
                let id = self.i64()?;
                let bounds = if self.flag()? { Some(self.four()?) } else { None };
                Entry::Bounds { id, bounds }
            }
            b'A' => Entry::Local { id: self.i64()?, yes: self.flag()? },
            b'[' => {
                let id = self.i64()?;
                let movers = if self.flag()? { Some(self.ids()?) } else { None };
                let rect = if self.flag()? { Some(self.rect()?) } else { None };
                Entry::Candidate { id, movers, rect }
            }
            b']' => {
                let id = self.i64()?;
                let clip = if self.flag()? {
                    let has = self.flag()?;
                    Some(if has { Clip { has, rect: self.rect()?, radii: self.radii()? } } else { Clip::default() })
                } else {
                    None
                };
                Entry::ClipOf { id, clip }
            }
            b'~' => {
                let id = self.i64()?;
                let tile = if self.flag()? {
                    Some(Tile { tile: self.rect()?, area: self.rect()?, clip: self.rect()?, radii: self.radii()?, content: self.list()? })
                } else {
                    None
                };
                Entry::TileOf { id, tile }
            }
            b'{' => {
                let id = self.i64()?;
                let n = self.count()?;
                let mut values = Declared::new();
                for _ in 0..n {
                    let k = self.str()?;
                    values.insert(k, self.str()?);
                }
                Entry::Declared { id, values }
            }
            b'|' => Entry::PaintOrder,
            b'<' => Entry::Order { id: self.i64()?, index: self.u64()? },
            b'?' => Entry::Covered { id: self.i64()?, area: self.rect()?, ignore: self.ids()?, yes: self.flag()? },
            b'&' => {
                let (id, area, ignore, limit) = (self.i64()?, self.rect()?, self.ids()?, self.u64()?);
                let found = if self.flag()? { Some(self.ids()?) } else { None };
                Entry::Obscurers { id, area, ignore, limit, found }
            }
            b'+' => Entry::Drawn { skip: self.ids()?, backgrounds: self.ids()?, list: self.list()? },
            b'*' => Entry::LayerDrawn { id: self.i64()?, skip: self.ids()?, bounds: self.rect()?, list: self.list()? },
            b'=' => Entry::Plain(self.list()?),
            b'^' => Entry::PaintKey(self.u64()?, self.u32()?, self.u64()?),
            b'/' => {
                let (id, x, y, placement, hit) = (self.i64()?, self.f32()?, self.f32()?, self.flag()?, self.i64()?);
                let rect = if placement && hit != 0 { self.rect()? } else { Rect::default() };
                Entry::HitWithin { id, x, y, placement, hit, rect }
            }
            b'Q' => Entry::ScanBegins,
            b';' => Entry::ScanEnds,
            b'%' => Entry::BoxFacts {
                key: self.i64()?,
                pseudo: self.flag()?,
                style: BoxStyle {
                    backdrop: self.flag()?,
                    filter: self.flag()?,
                    blurred: self.flag()?,
                    dimmed: self.flag()?,
                    opacity: self.f32()?,
                },
            },
            b'-' => Entry::NoEffects,
            other => return Err(format!("an unknown entry {:?} at {}", other as char, self.at - 1)),
        })
    }
}

pub fn parse(tape: &[u8]) -> Result<Vec<Entry>, String> {
    let mut t = Take { s: tape, at: 0 };
    let mut out = Vec::new();
    while t.at < tape.len() {
        out.push(t.entry()?);
    }
    Ok(out)
}

/// The tape both hosts read, and what went wrong.
#[derive(Debug, Default)]
pub struct Shared {
    pub entries: VecDeque<Entry>,
    pub errors: Vec<String>,
    pub tally: Tally,
    /// The violations the container pushed since the renderer last took them.
    pub violations: Vec<Vec<u8>>,
    /// The step running, for the errors.
    pub step: usize,
    /// The step's Backend log, into which the batches the C++ container made go where the tape has them.
    pub log: super::SharedLog,
    /// Those batches since a comparison last took them.
    pub batches: Vec<Vec<u8>>,
}

pub type Tape = Rc<RefCell<Shared>>;

impl Shared {
    pub fn error(&mut self, what: String) {
        if self.errors.len() < 8 {
            let step = self.step;
            self.errors.push(format!("step {step}: {what}"));
        }
    }

    /// The next answer, the violations before it taken in.
    pub fn next(&mut self, asked: &str) -> Option<Entry> {
        loop {
            match self.entries.pop_front() {
                Some(Entry::Violation(v)) => {
                    self.tally.add("container violations");
                    self.violations.push(v);
                }
                Some(Entry::Widths(line)) => {
                    self.tally.add("container batches");
                    self.log.borrow_mut().0.extend_from_slice(&line);
                    self.batches.push(line);
                }
                Some(e @ Entry::Step(_)) => {
                    self.error(format!("the port asked {asked} where the C++ asked nothing more"));
                    self.entries.push_front(e);
                    return None;
                }
                None => {
                    self.error(format!("the port asked {asked} past the tape's end"));
                    return None;
                }
                Some(e) => {
                    self.tally.add(format!("answer {}", e.name()));
                    return Some(e);
                }
            }
        }
    }

    /// The next entry, without taking it (the violations before it are taken in).
    pub fn peek(&mut self) -> Option<&Entry> {
        while let Some(Entry::Violation(_) | Entry::Widths(_)) = self.entries.front() {
            match self.entries.pop_front() {
                Some(Entry::Violation(v)) => {
                    self.tally.add("container violations");
                    self.violations.push(v);
                }
                Some(Entry::Widths(line)) => {
                    self.tally.add("container batches");
                    self.log.borrow_mut().0.extend_from_slice(&line);
                    self.batches.push(line);
                }
                _ => {}
            }
        }
        self.entries.front()
    }
}

fn bits(r: &Rect) -> [u32; 4] {
    [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()]
}

fn digest(bytes: &[u8]) -> (u64, u32) {
    (fnv64(bytes), bytes.len() as u32)
}

/// Asks the tape for the entry `$pattern` binds, else records the difference and answers `$default`.
macro_rules! answer {
    ($tape:expr, $name:literal, $pattern:pat => $body:expr, $default:expr) => {{
        let next = $tape.borrow_mut().next($name);
        match next {
            Some($pattern) => $body,
            Some(other) => {
                $tape.borrow_mut().error(format!("the port asked {} where the C++ asked {}: {other:?}", $name, other.name()));
                $default
            }
            None => $default,
        }
    }};
}

/// litehtml's document as the C++ answered.
#[derive(Debug)]
pub struct TapeEngine {
    pub tape: Tape,
}

impl TapeEngine {
    fn check(&self, ok: bool, what: impl FnOnce() -> String) {
        if !ok {
            self.tape.borrow_mut().error(what());
        }
    }
}

impl Engine for TapeEngine {
    fn has_viewport_units(&mut self, html: &[u8]) -> bool {
        let d = digest(html);
        answer!(self.tape, "has_viewport_units", Entry::ViewportUnits { html, yes } => { self.check(html == d, || "has_viewport_units of another document".into()); yes }, false)
    }
    fn update_from_string(&mut self, html: &[u8]) -> Option<bool> {
        let d = digest(html);
        let (ok, changed) = answer!(self.tape, "update_from_string", Entry::Update { html, ok, changed } => {
            if html != d {
                self.tape.borrow_mut().error(format!("update_from_string of another document: {} bytes in C++, {} in Rust", html.1, d.1));
            }
            self.tape.borrow_mut().tally.add(if ok { "path patched from the HTML" } else { "path refused by update_from_string" });
            (ok, changed)
        }, (false, false));
        ok.then_some(changed)
    }
    fn measure_words(&mut self) {
        answer!(self.tape, "measure_words", Entry::Words => (), ())
    }

    fn media_changed(&mut self) {
        answer!(self.tape, "media_changed", Entry::MediaChanged => (), ())
    }
    fn last_patch_paint_only(&mut self) -> bool {
        answer!(self.tape, "last_patch_paint_only", Entry::PaintOnly(yes) => { if yes { self.tape.borrow_mut().tally.add("paint-only patches seen"); } yes }, false)
    }
    fn create(&mut self, html: &[u8], master: &[u8]) -> bool {
        let (h, m) = (digest(html), digest(master));
        answer!(self.tape, "create", Entry::Create { html, master, made } => {
            if html != h {
                self.tape.borrow_mut().error(format!("create of another document: {} bytes in C++, {} in Rust", html.1, h.1));
            }
            if master != m {
                self.tape.borrow_mut().error(format!("create with another master CSS: {} bytes in C++, {} in Rust", master.1, m.1));
            }
            self.tape.borrow_mut().tally.add("path parsed anew");
            made
        }, false)
    }
    fn drop_document(&mut self) {}
    fn element_count(&mut self) -> u64 {
        answer!(self.tape, "element_count", Entry::Count(n) => n, 0)
    }
    fn render(&mut self, width: f32, budget: u64) {
        answer!(self.tape, "render", Entry::Render { width: w, budget: b } => self.check(w.to_bits() == width.to_bits() && b == budget, || format!("render at {w} within {b} in C++, at {width} within {budget} in Rust")), ())
    }
    fn layout_exhausted(&mut self) -> bool {
        answer!(self.tape, "layout_exhausted", Entry::Exhausted(yes) => yes, false)
    }
    fn layout_spent(&mut self) -> u64 {
        answer!(self.tape, "layout_spent", Entry::Spent(n) => n, 0)
    }
    fn index(&mut self) {
        answer!(self.tape, "index", Entry::Index => (), ())
    }
    fn height(&mut self) -> f64 {
        answer!(self.tape, "height", Entry::Height(h) => h, 0.0)
    }
    fn has_root(&mut self) -> bool {
        answer!(self.tape, "has_root", Entry::Root(yes) => yes, false)
    }
    fn layout_reads_viewport_height(&mut self) -> bool {
        answer!(self.tape, "layout_reads_viewport_height", Entry::ReadsHeight(yes) => yes, false)
    }
    fn font_count(&mut self) -> usize {
        answer!(self.tape, "font_count", Entry::Fonts(n) => n as usize, 0)
    }
    fn update_holes(&mut self, changes: &[HoleChange]) -> Option<bool> {
        let (ok, changed) = answer!(self.tape, "update_holes", Entry::Holes { changes: c, ok, changed } => {
            self.check(c == changes, || format!("update_holes with {c:?} in C++, {changes:?} in Rust"));
            self.tape.borrow_mut().tally.add(if ok { "path in place (holes)" } else { "path refused by update_holes" });
            (ok, changed)
        }, (false, false));
        ok.then_some(changed)
    }
    fn update_styles(&mut self, patches: &[(i64, Vec<u8>)]) -> Option<bool> {
        let (ok, changed) = answer!(self.tape, "update_styles", Entry::Styles { patches: p, ok, changed } => {
            self.check(p == patches, || format!("update_styles with {:?} in C++, {:?} in Rust", show(&p), show(patches)));
            self.tape.borrow_mut().tally.add(if ok { "path restyled (style holes)" } else { "path refused by update_styles" });
            (ok, changed)
        }, (false, false));
        ok.then_some(changed)
    }
    fn name_usage(&mut self) -> (u64, u64) {
        answer!(self.tape, "name_usage", Entry::Usage(n, b) => (n, b), (0, 0))
    }
    fn trim(&mut self) {
        answer!(self.tape, "trim", Entry::Trim => (), ())
    }
    fn set_no_effects(&mut self) {
        answer!(self.tape, "set_no_effects", Entry::NoEffects => (), ())
    }
    fn take_violations(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tape.borrow_mut().violations)
    }
    fn has_element(&mut self, id: i64) -> bool {
        answer!(self.tape, "has_element", Entry::Exists { id: i, yes } => { self.check(i == id, || format!("has_element({i}) in C++, ({id}) in Rust")); yes }, false)
    }
    fn declarations(&mut self, names: &[Vec<u8>], gates: &[Vec<u8>], always: &[i64]) -> (BTreeMap<i64, Declared>, u64) {
        answer!(self.tape, "declarations", Entry::Declarations { names: n, gates: g, always: a, found, generation } => {
            self.check(n == names && g == gates && a == always, || "declarations of other names, gates or elements".into());
            (found, generation)
        }, (BTreeMap::new(), 0))
    }
    fn edges(&mut self) -> Vec<(i64, Rect)> {
        answer!(self.tape, "edges", Entry::Edges(list) => list, Vec::new())
    }
    fn visual_box(&mut self, id: i64) -> Rect {
        answer!(self.tape, "visual_box", Entry::Visual { id: i, rect } => { self.check(i == id, || format!("visual_box({i}) in C++, ({id}) in Rust")); rect }, Rect::default())
    }
    fn inline_boxes(&mut self, id: i64) -> Vec<Rect> {
        answer!(self.tape, "inline_boxes", Entry::Lines { id: i, rects } => { self.check(i == id, || format!("inline_boxes({i}) in C++, ({id}) in Rust")); rects }, Vec::new())
    }
    fn transformed_box(&mut self, id: i64, r: Rect) -> Rect {
        answer!(self.tape, "transformed_box", Entry::Transformed { id: i, input, rect } => {
            self.check(i == id && bits(&input) == bits(&r), || format!("transformed_box({i}, {input:?}) in C++, ({id}, {r:?}) in Rust"));
            rect
        }, Rect::default())
    }
    fn border_box(&mut self, id: i64) -> Rect {
        answer!(self.tape, "border_box", Entry::Border { id: i, rect } => { self.check(i == id, || format!("border_box({i}) in C++, ({id}) in Rust")); rect }, Rect::default())
    }
    fn borders(&mut self, id: i64) -> Option<[f32; 4]> {
        answer!(self.tape, "borders", Entry::Borders { id: i, borders } => { self.check(i == id, || format!("borders({i}) in C++, ({id}) in Rust")); borders }, None)
    }
    fn offsets(&mut self, id: i64) -> (f64, f64, i64) {
        answer!(self.tape, "offsets", Entry::Offsets { id: i, left, top, parent } => { self.check(i == id, || format!("offsets({i}) in C++, ({id}) in Rust")); (left, top, parent) }, (0.0, 0.0, 0))
    }
    fn scroll_extent(&mut self, id: i64) -> ScrollExtent {
        answer!(self.tape, "scroll_extent", Entry::Extent { id: i, extent } => { self.check(i == id, || format!("scroll_extent({i}) in C++, ({id}) in Rust")); extent }, ScrollExtent::default())
    }
    fn textarea_content(&mut self, id: i64) -> Option<TextContent> {
        answer!(self.tape, "textarea_content", Entry::Textarea { id: i, content } => { self.check(i == id, || format!("textarea_content({i}) in C++, ({id}) in Rust")); content }, None)
    }
    fn computed_style(&mut self, id: i64, pseudo: &[u8], animated: &StyleValues) -> Json {
        answer!(self.tape, "computed_style", Entry::Computed { id: i, pseudo: p, animated: a, json } => {
            self.check(i == id && p == pseudo, || format!("computed_style({i}) in C++, ({id}) in Rust"));
            self.check(&a == animated, || format!("computed_style({id}) with animated values {:?} in C++, {:?} in Rust", show_values(&a), show_values(animated)));
            Json::parse(&json).unwrap_or(Json::Null)
        }, Json::Null)
    }
    fn custom_property(&mut self, id: i64, name: &[u8]) -> Vec<u8> {
        answer!(self.tape, "custom_property", Entry::Custom { id: i, name: n, value } => { self.check(i == id && n == name, || format!("custom_property({i}) in C++, ({id}) in Rust")); value }, Vec::new())
    }
    fn svg_text_box(&mut self, id: i64) -> Option<[f32; 4]> {
        answer!(self.tape, "svg_text_box", Entry::SvgText { id: i, text } => { self.check(i == id, || format!("svg_text_box({i}) in C++, ({id}) in Rust")); text }, None)
    }
    fn svg_geometry(&mut self, id: i64) -> bool {
        answer!(self.tape, "svg_geometry", Entry::SvgGeometry { id: i, yes } => { self.check(i == id, || format!("svg_geometry({i}) in C++, ({id}) in Rust")); yes }, false)
    }
    fn svg_path_length(&mut self, id: i64, at: f32) -> (f32, f32, f32) {
        answer!(self.tape, "svg_path_length", Entry::PathLength { id: i, at: a, length, x, y } => {
            self.check(i == id && a.to_bits() == at.to_bits(), || format!("svg_path_length({i}, {a}) in C++, ({id}, {at}) in Rust"));
            (length, x, y)
        }, (0.0, 0.0, 0.0))
    }
    fn svg_path_bounds(&mut self, id: i64) -> Option<[f32; 4]> {
        answer!(self.tape, "svg_path_bounds", Entry::Bounds { id: i, bounds } => { self.check(i == id, || format!("svg_path_bounds({i}) in C++, ({id}) in Rust")); bounds }, None)
    }
    fn hit(&mut self, x: f64, y: f64, placement: Option<&mut Rect>) -> i64 {
        let asked = placement.is_some();
        let (id, rect) = answer!(self.tape, "hit", Entry::Hit { x: hx, y: hy, placement: p, id, rect } => {
            self.check(hx.to_bits() == x.to_bits() && hy.to_bits() == y.to_bits() && p == asked, || format!("hit({hx}, {hy}, {p}) in C++, ({x}, {y}, {asked}) in Rust"));
            (id, rect)
        }, (0, Rect::default()));
        if let Some(out) = placement {
            *out = rect;
        }
        id
    }
    fn box_of(&mut self, id: i64) -> Rect {
        answer!(self.tape, "box_of", Entry::Box { id: i, rect } => { self.check(i == id, || format!("box_of({i}) in C++, ({id}) in Rust")); rect }, Rect::default())
    }
    fn touch_action(&mut self, id: i64) -> Option<Vec<u8>> {
        answer!(self.tape, "touch_action", Entry::TouchAction { id: i, value } => { self.check(i == id, || format!("touch_action({i}) in C++, ({id}) in Rust")); value }, None)
    }
    fn user_select_text(&mut self, id: i64) -> bool {
        answer!(self.tape, "user_select_text", Entry::UserSelect { id: i, yes } => { self.check(i == id, || format!("user_select_text({i}) in C++, ({id}) in Rust")); yes }, false)
    }
    fn mouse(&mut self, action: i32, x: f64, y: f64, hovers: bool) -> bool {
        answer!(self.tape, "mouse", Entry::Mouse { action: a, x: mx, y: my, hovers: h, redraw } => {
            self.check(a == action && mx.to_bits() == x.to_bits() && my.to_bits() == y.to_bits() && h == hovers, || format!("mouse({a}, {mx}, {my}, {h}) in C++, ({action}, {x}, {y}, {hovers}) in Rust"));
            redraw
        }, false)
    }
    fn leave(&mut self) -> bool {
        answer!(self.tape, "leave", Entry::Leave(redraw) => redraw, false)
    }
    fn cursor(&mut self) -> Vec<u8> {
        answer!(self.tape, "cursor", Entry::Cursor(c) => c, Vec::new())
    }
    fn editor_style(&mut self, id: i64, _clip: Rect) -> Option<EditorStyle> {
        answer!(self.tape, "editor_style", Entry::EditorStyle { id: i, style } => { self.check(i == id, || format!("editor_style({i}) in C++, ({id}) in Rust")); style }, None)
    }
    fn image_of(&mut self, src: &[u8]) -> Option<(i32, f32, f32)> {
        answer!(self.tape, "image_of", Entry::ImageOf { src: s, answer } => { self.check(s == src, || "image_of another source".into()); answer }, None)
    }
    fn image_ready(&mut self, id: i32, w: f32, h: f32) -> bool {
        answer!(self.tape, "image_ready", Entry::ImageReady { id: i, w: iw, h: ih, changed } => {
            self.check(i == id && iw.to_bits() == w.to_bits() && ih.to_bits() == h.to_bits(), || format!("image_ready({i}, {iw}, {ih}) in C++, ({id}, {w}, {h}) in Rust"));
            changed
        }, false)
    }
    fn content_size(&mut self, id: i64) -> (f32, f32) {
        answer!(self.tape, "content_size", Entry::Content { id: i, size } => { self.check(i == id, || format!("content_size({i}) in C++, ({id}) in Rust")); size }, (0.0, 0.0))
    }
    fn runs_key(&mut self) -> [u64; 4] {
        answer!(self.tape, "runs_key", Entry::RunsKey(k) => k, [0; 4])
    }
    fn text_runs(&mut self) -> Vec<TextRun> {
        answer!(self.tape, "text_runs", Entry::Runs(r) => r, Vec::new())
    }
    fn text_width(&mut self, text: &[u8], font: u64) -> f32 {
        // The C++ measures the text as a C string: up to a NUL.
        let text = text.iter().position(|&c| c == 0).map_or(text, |n| &text[..n]);
        answer!(self.tape, "text_width", Entry::Width { text: t, font: f, w } => { self.check(t == text && f == font, || "text_width of another text or font".into()); w }, 0.0)
    }
    fn font_info(&mut self, font: u64) -> Option<(i32, f32, f32, f32)> {
        answer!(self.tape, "font_info", Entry::FontInfo { font: f, info } => { self.check(f == font, || format!("font_info({f}) in C++, ({font}) in Rust")); info }, None)
    }
    fn selectable_key(&mut self) -> [u64; 2] {
        answer!(self.tape, "selectable_key", Entry::SelectableKey(k) => k, [0; 2])
    }
    fn any_selectable(&mut self) -> bool {
        answer!(self.tape, "any_selectable", Entry::Selectable(yes) => yes, false)
    }
    fn boxes(&mut self) -> Vec<(i64, bool, BoxStyle)> {
        if !answer!(self.tape, "boxes", Entry::ScanBegins => true, false) {
            return Vec::new();
        }
        let mut out = Vec::new();
        loop {
            let next = self.tape.borrow_mut().next("a box of the walk");
            match next {
                Some(Entry::BoxFacts { key, pseudo, style }) => out.push((key, pseudo, style)),
                Some(Entry::ScanEnds) => break,
                Some(other) => {
                    self.tape.borrow_mut().error(format!("the walk of the boxes ran into {}: {other:?}", other.name()));
                    break;
                }
                None => break,
            }
        }
        self.tape.borrow_mut().tally.add_n("boxes walked", out.len());
        out
    }
    fn box_style(&mut self, id: i64) -> BoxStyle {
        answer!(self.tape, "box_style", Entry::BoxFacts { key, pseudo, style } => {
            self.check(key == id.wrapping_mul(4) && !pseudo, || format!("box_style of key {key} in C++, of element {id} in Rust"));
            style
        }, BoxStyle::default())
    }
    fn background_is_local(&mut self, id: i64) -> bool {
        answer!(self.tape, "background_is_local", Entry::Local { id: i, yes } => { self.check(i == id, || format!("background_is_local({i}) in C++, ({id}) in Rust")); yes }, false)
    }
    fn layer_candidate(&mut self, id: i64, movers: Option<&BTreeSet<i64>>) -> Option<Rect> {
        let asked: Option<Vec<i64>> = movers.map(|m| m.iter().copied().collect());
        answer!(self.tape, "layer_candidate", Entry::Candidate { id: i, movers: m, rect } => {
            self.check(i == id && m == asked, || format!("layer_candidate({i}, {m:?}) in C++, ({id}, {asked:?}) in Rust"));
            rect
        }, None)
    }
    fn clip_of(&mut self, id: i64) -> Option<Clip> {
        answer!(self.tape, "clip_of", Entry::ClipOf { id: i, clip } => { self.check(i == id, || format!("clip_of({i}) in C++, ({id}) in Rust")); clip }, None)
    }
    fn background_tile(&mut self, id: i64) -> Option<Tile> {
        answer!(self.tape, "background_tile", Entry::TileOf { id: i, tile } => { self.check(i == id, || format!("background_tile({i}) in C++, ({id}) in Rust")); tile }, None)
    }
    fn declared(&mut self, id: i64, _names: &[&str]) -> Declared {
        answer!(self.tape, "declared", Entry::Declared { id: i, values } => { self.check(i == id, || format!("declared({i}) in C++, ({id}) in Rust")); values }, Declared::new())
    }
    fn paint_order(&mut self) {
        answer!(self.tape, "paint_order", Entry::PaintOrder => (), ())
    }
    fn paint_index(&mut self, id: i64) -> usize {
        answer!(self.tape, "paint_index", Entry::Order { id: i, index } => { self.check(i == id, || format!("paint_index({i}) in C++, ({id}) in Rust")); index as usize }, 0)
    }
    fn covered(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>) -> bool {
        let asked: Vec<i64> = ignore.iter().copied().collect();
        answer!(self.tape, "covered", Entry::Covered { id: i, area: a, ignore: g, yes } => {
            self.check(i == id && bits(&a) == bits(&area) && g == asked, || format!("covered({i}, {a:?}, {g:?}) in C++, ({id}, {area:?}, {asked:?}) in Rust"));
            yes
        }, false)
    }
    fn obscurers(&mut self, id: i64, area: Rect, ignore: &BTreeSet<i64>, limit: usize) -> Option<Vec<i64>> {
        let asked: Vec<i64> = ignore.iter().copied().collect();
        answer!(self.tape, "obscurers", Entry::Obscurers { id: i, area: a, ignore: g, limit: l, found } => {
            self.check(i == id && bits(&a) == bits(&area) && g == asked && l == limit as u64, || format!("obscurers({i}, {a:?}, {g:?}, {l}) in C++, ({id}, {area:?}, {asked:?}, {limit}) in Rust"));
            found
        }, None)
    }
    fn draw(&mut self) -> DisplayList {
        answer!(self.tape, "draw", Entry::Plain(list) => list, DisplayList::new())
    }
    fn draw_skipping(&mut self, skip: &[i64], backgrounds: &[i64]) -> DisplayList {
        answer!(self.tape, "draw_skipping", Entry::Drawn { skip: s, backgrounds: b, list } => {
            self.check(s == skip && b == backgrounds, || format!("draw without {s:?}, backgrounds {b:?} in C++, {skip:?}, {backgrounds:?} in Rust"));
            list
        }, DisplayList::new())
    }
    fn draw_layer(&mut self, id: i64, skip: &[i64]) -> (DisplayList, Rect) {
        answer!(self.tape, "draw_layer", Entry::LayerDrawn { id: i, skip: s, bounds, list } => {
            self.check(i == id && s == skip, || format!("draw_layer({i}, {s:?}) in C++, ({id}, {skip:?}) in Rust"));
            (list, bounds)
        }, (DisplayList::new(), Rect::default()))
    }
    fn paint_key(&mut self) -> (u64, u32) {
        answer!(self.tape, "paint_key", Entry::PaintKey(doc, generation, _) => (doc, generation), (0, 0))
    }
    fn hit_within(&mut self, id: i64, x: f32, y: f32, placement: Option<&mut Rect>) -> i64 {
        let asked = placement.is_some();
        let (hit, rect) = answer!(self.tape, "hit_within", Entry::HitWithin { id: i, x: hx, y: hy, placement: p, hit, rect } => {
            self.check(i == id && hx.to_bits() == x.to_bits() && hy.to_bits() == y.to_bits() && p == asked, || format!("hit_within({i}, {hx}, {hy}, {p}) in C++, ({id}, {x}, {y}, {asked}) in Rust"));
            (hit, rect)
        }, (0, Rect::default()));
        if hit != 0
            && let Some(out) = placement
        {
            *out = rect;
        }
        hit
    }
}

fn show(patches: &[(i64, Vec<u8>)]) -> Vec<(i64, String)> {
    patches.iter().map(|(id, s)| (*id, String::from_utf8_lossy(s).into_owned())).collect()
}

fn show_values(values: &StyleValues) -> Vec<(String, String)> {
    values.iter().map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned())).collect()
}
