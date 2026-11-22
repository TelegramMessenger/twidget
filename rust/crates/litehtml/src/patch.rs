//! `document.cpp`'s patching half (stage L19): a document `createUpdatableFromString` made, patched in place to new
//! HTML (`update_from_string`: the template's holes when the new text differs only inside them, else a structural
//! diff of Gumbo's trees), to new inline styles (`update_styles`) or to new attribute values and sole texts
//! (`update_holes`), each refused, leaving the document as it was, when the change cannot be patched; then the
//! restyle and the render tree kept or made again (`restyle_after_patch`), paint-only patches and the regions they
//! repaint, and what the layout cache needs to relayout incrementally (stage L18: the marked items whose places a
//! patch forgets, the boxes before a patch and the regions a relayout dirtied).
//!
//! The element side is here too: `html_tag.cpp`'s class flips, match memos, retags (`reset_attributes`,
//! `forget_matched_styles`, `apply_matching`), the inline-property patch, the background-only restyle and the
//! shallow width check; `element.cpp`'s `replace_children`, `el_text::set_text`, `el_image::update_src`.
//!
//! What the C++ keeps in `unordered_*` containers keyed by pointers is keyed by id; only membership is read. The
//! `std::sort` of the template's holes is the tree's order on ties (see [`crate::document::Document::template_ties`]).
//! A `::before` or `::after` box gives its generated content back when the C++ object would die: when neither the
//! tree, a live render item nor the patch's own references hold it ([`Document::release_dead_generated`]).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::Px;
use crate::css_length::{CssLength, f_length_percentage, f_positive};
use crate::css_parser::{f_componentize, normalize};
use crate::css_properties::{CssProperties, Get};
use crate::css_selector::{AttrSelectType, CssCombinator, CssSelector, DocumentMode};
use crate::css_values::*;
use crate::document::{Document, HtmlHole, StyleCx, max_build_depth, max_build_elements};
use crate::element::{ElementId, ElementKind, ElementState, StringMap, UsedSelector};
use crate::gumbo::*;
use crate::html::{WHITESPACE, lowcase, split_string, t_strncasecmp};
use crate::html_tag::ElementVars;
use crate::render_item::*;
use crate::string_id::*;
use crate::style::{Style, Value, raw_declaration_id};
use crate::types::{Position, Size};
use crate::utf8_strings::is_valid_utf8;
use crate::{Names, StringId};

/// The `LH_NO_*` switches patching reads, all on unless the variable is set. The C++ reads `LH_NO_TEMPLATE`,
/// `LH_NO_STRUCTURE`, `LH_NO_BACKGROUND_PATCH`, `LH_NO_SHALLOW_WIDTH_PATCH`, `LH_NO_PAINT_ONLY` and
/// `LH_NO_PATCH_REGIONS` on every call, the others once per process; a process never changes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchSwitches {
    /// Holes are patched without a parse: not `LH_NO_TEMPLATE`.
    pub template: bool,
    /// `update_structure` runs: not `LH_NO_STRUCTURE`.
    pub structure: bool,
    /// `class` values are holes that may change: not `LH_NO_CLASS_HOLES`.
    pub class_holes: bool,
    /// `flip_class` swaps back to a class's saved styles: not `LH_NO_CLASS_FLIP`.
    pub class_flip: bool,
    /// `apply_matching` replays a memo of the rules a leaf matched: not `LH_NO_MATCH_MEMO`.
    pub match_memo: bool,
    /// A change of `background-color` alone restyles the background only: not `LH_NO_BACKGROUND_PATCH`.
    pub background_patch: bool,
    /// A change of `transform`, `opacity` or `background-color` alone is patched into the style: not
    /// `LH_NO_PROPERTY_PATCH`.
    pub property_patch: bool,
    /// A colour change of a text's owner repaints the owner and its texts: not `LH_NO_COLOR_REGIONS`.
    pub color_regions: bool,
    /// A text changed in place repaints its words: not `LH_NO_TEXT_REGIONS`.
    pub text_regions: bool,
    /// A text whose words and spaces keep their shape is changed in place: not `LH_NO_INPLACE_TEXT`.
    pub inplace_text: bool,
    /// A `width` change of a box whose children do not inherit it restyles the box alone: not
    /// `LH_NO_SHALLOW_WIDTH_PATCH`.
    pub shallow_width_patch: bool,
    /// The render tree is kept when a patch cannot change it: not `LH_NO_KEEP_TREE`.
    pub keep_tree: bool,
    /// A patch that cannot move a box is paint-only: not `LH_NO_PAINT_ONLY`.
    pub paint_only: bool,
    /// A paint-only patch tells the elements it repaints: not `LH_NO_PATCH_REGIONS`.
    pub patch_regions: bool,
    /// A text changed in place that keeps its size is paint-only: not `LH_NO_SAME_SIZE_TEXT`.
    pub same_size_text: bool,
}

impl Default for PatchSwitches {
    fn default() -> PatchSwitches {
        PatchSwitches {
            template: true,
            structure: true,
            class_holes: true,
            class_flip: true,
            match_memo: true,
            background_patch: true,
            property_patch: true,
            color_regions: true,
            text_regions: true,
            inplace_text: true,
            shallow_width_patch: true,
            keep_tree: true,
            paint_only: true,
            patch_regions: true,
            same_size_text: true,
        }
    }
}

/// What the document keeps for patching: `m_master_styles`, the lazily built lookups of the holes by owner, the
/// last patch's paint-only answer and regions, the selector scope and the names selectors read of ancestors,
/// whether a selector reads `style`, the match epoch, the hovered and active elements (set by hit testing, stage
/// L16), and the layout cache's patch state (`m_layout_marked`, `m_layout_origins`, `m_restyle_regions`,
/// `m_restyle_confined`, `m_layout_before`).
#[derive(Clone, Debug)]
pub struct DocPatch {
    /// `m_master_styles`: the master sheet `createUpdatableFromString` was given (its counters count).
    pub master_styles: Vec<u8>,
    /// `m_style_holes`: each owner's first `style` hole.
    pub style_holes: BTreeMap<ElementId, usize>,
    /// `m_owner_holes`: each owner's holes, an attribute's by its element, a text's by its parent.
    pub owner_holes: BTreeMap<ElementId, Vec<usize>>,
    /// `m_paint_only_patch`, `m_patch_confined`, `m_patch_regions`.
    pub paint_only_patch: bool,
    pub patch_confined: bool,
    pub patch_regions: Vec<ElementId>,
    /// `m_selector_scope`: -1 until looked at, else bit 0 attributes, 1 positions, 2 counters or `:empty`.
    pub selector_scope: i32,
    /// `m_left_classes`, `m_left_ids`, `m_left_attrs`: what selectors read of other elements than their subject.
    pub left_classes: BTreeSet<StringId>,
    pub left_ids: BTreeSet<StringId>,
    pub left_attrs: BTreeSet<StringId>,
    /// `m_style_attributes_selectable`: -1 until looked at.
    pub style_attributes_selectable: Cell<i32>,
    /// `m_match_epoch`.
    pub match_epoch: u64,
    /// `m_over_element`, `m_active_element`.
    pub over_element: Option<ElementId>,
    pub active_element: Option<ElementId>,
    /// `m_layout_marked`: the items whose layout was marked dirty since the last layout or tree.
    pub layout_marked: Vec<RenderId>,
    /// `m_layout_origins`: the elements whose layout was marked since the patch began.
    pub layout_origins: BTreeSet<ElementId>,
    /// `m_restyle_regions`, `m_restyle_confined`.
    pub restyle_regions: Vec<ElementId>,
    pub restyle_confined: bool,
    /// `m_layout_before`, `m_layout_before_valid`: every item's box in document coordinates before the patch.
    pub layout_before: Vec<(RenderId, Position)>,
    pub layout_before_valid: bool,
}

impl Default for DocPatch {
    fn default() -> DocPatch {
        DocPatch {
            master_styles: Vec::new(),
            style_holes: BTreeMap::new(),
            owner_holes: BTreeMap::new(),
            paint_only_patch: false,
            patch_confined: false,
            patch_regions: Vec::new(),
            selector_scope: -1,
            left_classes: BTreeSet::new(),
            left_ids: BTreeSet::new(),
            left_attrs: BTreeSet::new(),
            style_attributes_selectable: Cell::new(-1),
            match_epoch: 1,
            over_element: None,
            active_element: None,
            layout_marked: Vec::new(),
            layout_origins: BTreeSet::new(),
            restyle_regions: Vec::new(),
            restyle_confined: true,
            layout_before: Vec::new(),
            layout_before_valid: false,
        }
    }
}

/// `html_tag::match_memo`: the rules a leaf matched in an epoch with these attributes, per sheet.
#[derive(Clone, Debug, Default)]
pub struct MatchMemo {
    pub epoch: u64,
    pub attrs: StringMap,
    pub rules: [Vec<Arc<CssSelector>>; 3],
}

/// `html_tag::flip_state`: what an element's styles were under a class value, to swap back to.
#[derive(Clone, Debug, Default)]
pub struct FlipState {
    pub value: Vec<u8>,
    pub epoch: u64,
    pub parent_id: i32,
    pub others: StringMap,
    pub str_classes: Vec<Vec<u8>>,
    pub classes: Vec<StringId>,
    pub st: Style,
    pub st_base: Style,
    pub has_base: bool,
    pub used: Vec<UsedSelector>,
    pub css: CssProperties,
    pub kids: Vec<CssProperties>,
    pub regions: BTreeMap<Vec<u8>, i32>,
}

/// What an element keeps for patching: `m_match_memos` (at most 2) and `m_flips` (at most 3).
#[derive(Clone, Debug, Default)]
pub struct ElementPatch {
    pub match_memos: Vec<MatchMemo>,
    pub flips: Vec<FlipState>,
}

/// `document::property_patch`: the properties a style change touches (`transform`, `opacity`, `background-color`)
/// and their declarations, and whether every one repaints inside the box only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyPatch {
    pub names: Vec<StringId>,
    pub text: Vec<u8>,
    pub box_: bool,
}

impl Default for PropertyPatch {
    fn default() -> PropertyPatch {
        PropertyPatch { names: Vec::new(), text: Vec::new(), box_: true }
    }
}

/// `document::hole_change`: an attribute value (`name`) of an element, or the sole text of an element (`text`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HoleChange {
    pub owner: Option<ElementId>,
    pub name: Vec<u8>,
    pub text: bool,
    pub value: Vec<u8>,
}

/// `template_requires_fresh_document`.
pub const template_requires_fresh_document: i32 = -2;

// ---- document.cpp's static helpers --------------------------------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

fn find_first_of(hay: &[u8], set: &[u8]) -> bool {
    hay.iter().any(|c| set.contains(c))
}

/// `find_first_not_of(set) == npos`.
fn all_of(hay: &[u8], set: &[u8]) -> bool {
    hay.iter().all(|c| set.contains(c))
}

/// `split_style_declarations(text)`: the declarations of a style text by `;` outside quotes and parentheses, each
/// its name (trimmed, lower-case unless custom) and its value as written; a piece without a colon that is not
/// blank has an empty value. An unclosed quote takes the rest.
pub fn split_style_declarations(text: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = 0u8;
    for i in 0..=text.len() {
        let c = if i < text.len() { text[i] } else { b';' };
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
        } else if c == b';' && depth <= 0 {
            let decl = &text[start..i];
            start = i + 1;
            let Some(colon) = decl.iter().position(|&c| c == b':') else {
                if !all_of(decl, b" \t\n") {
                    out.push((decl.to_vec(), Vec::new()));
                }
                continue;
            };
            let a = decl.iter().position(|c| !b" \t\n".contains(c));
            let mut name = match a {
                Some(a) if a < colon => {
                    let b = decl[..colon].iter().rposition(|c| !b" \t\n".contains(c)).unwrap_or(a);
                    decl[a..=b].to_vec()
                }
                _ => Vec::new(),
            };
            if !name.starts_with(b"--") {
                name.make_ascii_lowercase();
            }
            out.push((name, decl[colon + 1..].to_vec()));
        }
    }
    out
}

/// `paint_only_property(name)`.
pub fn paint_only_property(name: &[u8]) -> bool {
    const PAINT: &[&[u8]] = &[
        b"color",
        b"background",
        b"background-color",
        b"background-image",
        b"background-position",
        b"background-repeat",
        b"opacity",
        b"transform",
        b"transform-origin",
        b"box-shadow",
        b"text-shadow",
        b"filter",
        b"outline",
        b"outline-color",
        b"outline-width",
        b"outline-style",
        b"outline-offset",
        b"visibility",
        b"fill",
        b"fill-opacity",
        b"stroke",
        b"stroke-opacity",
        b"stroke-width",
        b"stroke-dasharray",
        b"stroke-dashoffset",
        b"stop-color",
        b"stop-opacity",
        b"border-color",
        b"border-top-color",
        b"border-right-color",
        b"border-bottom-color",
        b"border-left-color",
        b"text-decoration-color",
        b"caret-color",
        b"cursor",
        b"-lh-anim",
    ];
    PAINT.contains(&name)
}

/// `style_change_is_background_color(before, after)`: the two texts differ only in `background-color` (and
/// `-lh-anim`), with no escapes, braces, comments or counters.
pub fn style_change_is_background_color(switches: &PatchSwitches, before: &[u8], after: &[u8]) -> bool {
    if !switches.background_patch
        || find_first_of(before, b"\\{}")
        || find_first_of(after, b"\\{}")
        || find(before, b"/*")
        || find(after, b"/*")
        || find(before, b"counter")
        || find(after, b"counter")
    {
        return false;
    }
    let mut a = split_style_declarations(before);
    let mut b = split_style_declarations(after);
    let keep = |entry: &(Vec<u8>, Vec<u8>)| entry.0 != b"background-color" && entry.0 != b"-lh-anim";
    a.retain(keep);
    b.retain(keep);
    a == b
}

/// `style_change_property_patch(before, after, patch)`: the change only sets `transform`, `opacity` or
/// `background-color`, every name plain and once, none removed; `patch` gets them in the new text's order.
pub fn style_change_property_patch(switches: &PatchSwitches, before: &[u8], after: &[u8], patch: &mut PropertyPatch) -> bool {
    if !switches.property_patch {
        return false;
    }
    for text in [before, after] {
        if find_first_of(text, b"\\{}!\"'")
            || find(text, b"/*")
            || find(text, b"var(")
            || find(text, b"--")
            || find(text, b"counter")
            || find(text, b"-lh-")
            || find(text, b"url(")
        {
            return false;
        }
    }
    let a = split_style_declarations(before);
    let b = split_style_declarations(after);
    let collect = |list: &[(Vec<u8>, Vec<u8>)], out: &mut BTreeMap<Vec<u8>, Vec<u8>>| -> bool {
        for d in list {
            if d.0.is_empty()
                || d.0 == b"all"
                || d.0 == b"background"
                || !all_of(&d.0, b"abcdefghijklmnopqrstuvwxyz-")
                || out.contains_key(&d.0)
            {
                return false;
            }
            out.insert(d.0.clone(), d.1.clone());
        }
        true
    };
    let mut left = BTreeMap::new();
    let mut right = BTreeMap::new();
    if !collect(&a, &mut left) || !collect(&b, &mut right) {
        return false;
    }
    for name in left.keys() {
        if !right.contains_key(name) {
            return false;
        }
    }
    for d in &b {
        if left.get(&d.0).is_some_and(|v| *v == d.1) {
            continue;
        }
        let id = if d.0 == b"transform" {
            _transform_
        } else if d.0 == b"opacity" {
            _opacity_
        } else if d.0 == b"background-color" {
            _background_color_
        } else {
            return false;
        };
        if id != _background_color_ {
            patch.box_ = false;
        }
        patch.names.push(id);
        patch.text.extend_from_slice(&d.0);
        patch.text.push(b':');
        patch.text.extend_from_slice(&d.1);
        patch.text.push(b';');
    }
    !patch.names.is_empty()
}

/// `style_change_is_width_only(before, after)`: both texts plain, each with one `width` of a finite px or %
/// length, the rest alike in order.
pub fn style_change_is_width_only(before: &[u8], after: &[u8]) -> bool {
    let mut declarations: [BTreeMap<Vec<u8>, Vec<u8>>; 2] = Default::default();
    let mut unchanged: [Vec<(Vec<u8>, Vec<u8>)>; 2] = Default::default();
    for (index, text) in [before, after].into_iter().enumerate() {
        let lower = lowcase(text);
        if find_first_of(text, b"\\{}!\"'")
            || find(text, b"/*")
            || find(&lower, b"var(")
            || find(&lower, b"--")
            || find(&lower, b"counter")
            || find(&lower, b"-lh-")
            || find(&lower, b"url(")
        {
            return false;
        }
        let values = &mut declarations[index];
        for entry in split_style_declarations(text) {
            if entry.0.is_empty() || entry.0 == b"all" || !all_of(&entry.0, b"abcdefghijklmnopqrstuvwxyz-") || values.contains_key(&entry.0)
            {
                return false;
            }
            values.insert(entry.0.clone(), entry.1.clone());
            if entry.0 != b"width" {
                unchanged[index].push(entry);
            }
        }
        let Some(width) = values.get(&b"width"[..]) else { return false };
        let tokens = normalize(width, f_componentize, None);
        let mut length = CssLength::default();
        if tokens.len() != 1
            || !length.from_token(&tokens[0], f_length_percentage | f_positive, CssValues::default())
            || length.is_predefined()
            || !length.val().is_finite()
            || (length.units() != CssUnits::css_units_px && length.units() != CssUnits::css_units_percentage)
        {
            return false;
        }
        values.remove(&b"width"[..]);
    }
    unchanged[0] == unchanged[1]
}

/// `style_change_is_paint_only(before, after)`: every declaration that differs paints only.
pub fn style_change_is_paint_only(before: &[u8], after: &[u8]) -> bool {
    let (left, right) = declaration_maps(before, after);
    for (name, value) in &left {
        if right.get(name) != Some(value) && !paint_only_property(name) {
            return false;
        }
    }
    for name in right.keys() {
        if !left.contains_key(name) && !paint_only_property(name) {
            return false;
        }
    }
    true
}

/// Declarations by name, the last of a name kept.
type DeclarationMap = BTreeMap<Vec<u8>, Vec<u8>>;

fn declaration_maps(before: &[u8], after: &[u8]) -> (DeclarationMap, DeclarationMap) {
    let mut left = BTreeMap::new();
    let mut right = BTreeMap::new();
    for d in split_style_declarations(before) {
        left.insert(d.0, d.1);
    }
    for d in split_style_declarations(after) {
        right.insert(d.0, d.1);
    }
    (left, right)
}

/// `style_change_region(before, after)`: 1 when what changed repaints inside the element's border box, 2 inside
/// the outermost `<svg>` around it, 3 a colour its texts take, 0 elsewhere.
pub fn style_change_region(before: &[u8], after: &[u8]) -> i32 {
    const BOX: &[&[u8]] = &[
        b"background",
        b"background-color",
        b"background-image",
        b"background-position",
        b"background-repeat",
        b"border-color",
        b"border-top-color",
        b"border-right-color",
        b"border-bottom-color",
        b"border-left-color",
        b"caret-color",
        b"cursor",
        b"-lh-anim",
    ];
    const SVG: &[&[u8]] = &[
        b"fill",
        b"fill-opacity",
        b"stroke",
        b"stroke-opacity",
        b"stroke-width",
        b"stroke-dasharray",
        b"stroke-dashoffset",
        b"stop-color",
        b"stop-opacity",
    ];
    let (left, right) = declaration_maps(before, after);
    let mut region = 1;
    let mut note = |name: &[u8]| {
        if BOX.contains(&name) {
            return;
        }
        if name == b"color" || name == b"text-decoration-color" {
            if region == 1 {
                region = 3;
            }
            return;
        }
        region = if SVG.contains(&name) && region > 0 { 2 } else { 0 };
    };
    for (name, value) in &left {
        if right.get(name) != Some(value) {
            note(name);
        }
    }
    for name in right.keys() {
        if !left.contains_key(name) {
            note(name);
        }
    }
    region
}

/// `declared_region(name)`: 1 for what paints inside the box, 3 for a text colour, 0 otherwise.
pub fn declared_region(name: &[u8]) -> i32 {
    const BOX: &[&[u8]] = &[
        b"border-color",
        b"border-top-color",
        b"border-right-color",
        b"border-bottom-color",
        b"border-left-color",
        b"caret-color",
        b"cursor",
    ];
    if name.starts_with(b"background") || BOX.contains(&name) {
        return 1;
    }
    if name == b"color" || name == b"text-decoration-color" {
        return 3;
    }
    0
}

/// `style_change_is_local(before, after)`: no counters, and no custom property among the declarations between
/// the common head and tail.
pub fn style_change_is_local(before: &[u8], after: &[u8]) -> bool {
    if find(before, b"counter") || find(after, b"counter") {
        return false;
    }
    let a = split_style_declarations(before);
    let b = split_style_declarations(after);
    let sensitive = |d: &(Vec<u8>, Vec<u8>)| d.0.starts_with(b"--");
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    let (mut ea, mut eb) = (a.len(), b.len());
    while ea > i && eb > i && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
    }
    !a[i..ea].iter().any(sensitive) && !b[i..eb].iter().any(sensitive)
}

/// `table_part(display)`.
fn table_part(display: i32) -> bool {
    display == display_table_cell
        || display == display_table_row
        || display == display_table_column
        || display == display_table_column_group
        || display == display_table_row_group
        || display == display_table_header_group
        || display == display_table_footer_group
        || display == display_table_caption
}

/// `tree_neutral_change(before, after, inline_box, table_cached)`: every declaration that differs is one the
/// render tree does not depend on (but margins, padding and offsets of an inline, sizes, margins and padding of a
/// table part).
pub fn tree_neutral_change(before: &[u8], after: &[u8], inline_box: bool, table_cached: bool) -> bool {
    const NEUTRAL: &[&[u8]] = &[
        b"width",
        b"height",
        b"min-width",
        b"min-height",
        b"max-width",
        b"max-height",
        b"left",
        b"top",
        b"right",
        b"bottom",
        b"margin",
        b"margin-left",
        b"margin-right",
        b"margin-top",
        b"margin-bottom",
        b"padding",
        b"padding-left",
        b"padding-right",
        b"padding-top",
        b"padding-bottom",
        b"border-radius",
        b"border-top-left-radius",
        b"border-top-right-radius",
        b"border-bottom-left-radius",
        b"border-bottom-right-radius",
        b"border-color",
        b"border-top-color",
        b"border-right-color",
        b"border-bottom-color",
        b"border-left-color",
        b"color",
        b"background",
        b"background-color",
        b"background-image",
        b"background-position",
        b"background-size",
        b"opacity",
        b"transform",
        b"transform-origin",
        b"box-shadow",
        b"text-shadow",
        b"outline-color",
        b"filter",
        b"fill",
        b"stroke",
        b"stroke-width",
        b"stroke-dasharray",
        b"stroke-dashoffset",
        b"fill-opacity",
        b"stroke-opacity",
        b"z-index",
        b"text-align",
        b"vertical-align",
        b"letter-spacing",
        b"flex-grow",
        b"flex-shrink",
        b"flex-basis",
        b"gap",
        b"row-gap",
        b"column-gap",
        b"justify-content",
        b"align-items",
        b"align-self",
    ];
    let parse = |text: &[u8], out: &mut BTreeMap<Vec<u8>, Vec<u8>>| -> bool {
        if find_first_of(text, b"\"'\\{}") || find(text, b"/*") || find(text, b"url(") {
            return false;
        }
        let mut start = 0;
        while start <= text.len() {
            let end = text[start..].iter().position(|&c| c == b';').map(|k| start + k);
            let part = &text[start..end.unwrap_or(text.len())];
            if let Some(colon) = part.iter().position(|&c| c == b':') {
                let name = &part[..colon];
                let first = name.iter().position(|c| !b" \t\n".contains(c));
                let name = match first {
                    Some(first) => {
                        let last = name.iter().rposition(|c| !b" \t\n".contains(c)).unwrap_or(first);
                        name[first..=last].to_ascii_lowercase()
                    }
                    None => Vec::new(),
                };
                out.insert(name, part[colon + 1..].to_vec());
            } else if !all_of(part, b" \t\n") {
                return false;
            }
            match end {
                None => break,
                Some(end) => start = end + 1,
            }
        }
        true
    };
    let mut a = BTreeMap::new();
    let mut b = BTreeMap::new();
    if !parse(before, &mut a) || !parse(after, &mut b) {
        return false;
    }
    let allowed = |name: &[u8]| -> bool {
        if inline_box
            && (name.starts_with(b"margin")
                || name.starts_with(b"padding")
                || name == b"left"
                || name == b"top"
                || name == b"right"
                || name == b"bottom")
        {
            return false;
        }
        if table_cached
            && (name == b"width"
                || name == b"height"
                || name.starts_with(b"min-")
                || name.starts_with(b"max-")
                || name.starts_with(b"padding")
                || name.starts_with(b"margin"))
        {
            return false;
        }
        NEUTRAL.contains(&name)
    };
    for (name, value) in &a {
        if b.get(name) != Some(value) && !allowed(name) {
            return false;
        }
    }
    for name in b.keys() {
        if !a.contains_key(name) && !allowed(name) {
            return false;
        }
    }
    true
}

/// `mentions_counters(text)`: `counter-`, `counter(` or `counters` in any case.
pub fn mentions_counters(text: &[u8]) -> bool {
    let mut i = 0;
    while i + 7 <= text.len() {
        if (text[i] | 0x20) != b'c' || t_strncasecmp(&text[i..], b"counter", 7) != 0 {
            i += 1;
            continue;
        }
        let next = i + 7;
        if next < text.len() && (text[next] == b'-' || text[next] == b'(' || (text[next] | 0x20) == b's') {
            return true;
        }
        i += 1;
    }
    false
}

/// `restylable_in_place(el)`: an element of exactly these classes.
fn restylable_in_place(kind: ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::html_tag | ElementKind::el_div | ElementKind::el_para | ElementKind::el_td | ElementKind::el_tr | ElementKind::el_body
    )
}

/// `collect_ancestor_names(selector, self, names)`: the classes, ids and attributes a selector reads of elements
/// other than its subject (`:checked` reads `checked` and `selected`, `:disabled` and `:enabled` read `disabled`);
/// a nested list's are read of others when the compound holding it is not the subject.
fn collect_ancestor_names(names: &Names, selector: &CssSelector, self_: bool, left: &mut DocPatch) {
    let mut sel = Some(selector);
    let mut first = true;
    while let Some(s) = sel {
        let ancestor = !self_ || !first;
        first = false;
        for attr in &s.m_right.m_attrs {
            if ancestor {
                if attr.ty == AttrSelectType::select_class {
                    left.left_classes.insert(attr.name);
                } else if attr.ty == AttrSelectType::select_id {
                    left.left_ids.insert(attr.name);
                } else if attr.ty == AttrSelectType::select_attr {
                    left.left_attrs.insert(attr.name);
                } else if attr.ty == AttrSelectType::select_pseudo_class && attr.name == _checked_ {
                    left.left_attrs.insert(names.id(b"checked"));
                    left.left_attrs.insert(names.id(b"selected"));
                } else if attr.ty == AttrSelectType::select_pseudo_class && (attr.name == _disabled_ || attr.name == _enabled_) {
                    left.left_attrs.insert(names.id(b"disabled"));
                }
            }
            for inner in attr.selector_list.iter() {
                collect_ancestor_names(names, inner, !ancestor, left);
            }
        }
        sel = s.m_left.as_deref();
    }
}

/// `scan_selector(selector, attributes, positions, emptiness)`.
fn scan_selector(selector: &CssSelector, attributes: &mut bool, positions: &mut bool, emptiness: &mut bool) {
    let mut sel = Some(selector);
    while let Some(s) = sel {
        if s.m_left.is_some()
            && (s.m_combinator == CssCombinator::combinator_adjacent_sibling || s.m_combinator == CssCombinator::combinator_general_sibling)
        {
            *attributes = true;
            *positions = true;
        }
        for attr in &s.m_right.m_attrs {
            if attr.ty == AttrSelectType::select_pseudo_class {
                let n = attr.name;
                if n == _only_child_
                    || n == _only_of_type_
                    || n == _first_child_
                    || n == _first_of_type_
                    || n == _last_child_
                    || n == _last_of_type_
                    || n == _nth_child_
                    || n == _nth_of_type_
                    || n == _nth_last_child_
                    || n == _nth_last_of_type_
                {
                    *positions = true;
                    if !attr.selector_list.is_empty() {
                        *attributes = true;
                    }
                } else if n == _empty_ {
                    *attributes = true;
                    *emptiness = true;
                }
            }
            for inner in attr.selector_list.iter() {
                scan_selector(inner, attributes, positions, emptiness);
            }
        }
        sel = s.m_left.as_deref();
    }
}

/// Whether the selector reads the `style` attribute (in any case), nested lists included.
fn reads_style(names: &Names, selector: &CssSelector) -> bool {
    let mut sel = Some(selector);
    while let Some(s) = sel {
        for attr in &s.m_right.m_attrs {
            if attr.ty == AttrSelectType::select_attr && crate::html::t_strcasecmp(&names.s(attr.name), b"style") == 0 {
                return true;
            }
            if attr.selector_list.iter().any(|inner| reads_style(names, inner)) {
                return true;
            }
        }
        sel = s.m_left.as_deref();
    }
    false
}

/// `unsafe_style(value)`: counters, `var()` or a custom property.
fn unsafe_style(value: &[u8]) -> bool {
    find(value, b"counter") || find(value, b"var(") || find(value, b"--")
}

/// `background::is_empty()`: no colour and no image that draws.
fn background_is_empty(bg: &crate::css_properties::Background) -> bool {
    if bg.m_color.alpha != 0 {
        return false;
    }
    bg.m_image.iter().all(|img| img.is_empty())
}

// ---- Gumbo's trees as update_structure reads them -------------------------------------------------------------

/// `same_text(a, b)` of two C strings.
fn same_text(a: &[u8], b: &[u8]) -> bool {
    c_str(a) == c_str(b)
}

fn is_element_node(node: &GumboNode) -> bool {
    node.ty == GUMBO_NODE_ELEMENT || node.ty == GUMBO_NODE_TEMPLATE
}

/// A node's children (a document's or an element's), empty for others.
fn node_children(out: &GumboOutput, node: NodeId) -> &[NodeId] {
    out.node(node).children().map_or(&[][..], |c| &c[..])
}

/// `element_name(node)`: the tag's normalized name, else the original tag's name (cut from its text when it is
/// still `<…>`).
fn element_name<'a>(out: &GumboOutput, text: &'a [u8], node: NodeId) -> &'a [u8] {
    let e = out.node(node).element();
    let known = gumbo_normalized_tagname(e.tag);
    if !known.is_empty() {
        return known;
    }
    let mut piece = e.original_tag;
    if let Some(at) = piece.data
        && piece.length >= 2
        && text.get(at) == Some(&b'<')
        && text.get(at + piece.length - 1) == Some(&b'>')
    {
        gumbo_tag_from_original_text(text, &mut piece);
    }
    piece.bytes(text)
}

/// `same_element_kind(a, b)`: elements of the same tag and namespace (an unknown tag by name).
fn same_element_kind(ao: &GumboOutput, at: &[u8], a: NodeId, bo: &GumboOutput, bt: &[u8], b: NodeId) -> bool {
    let (na, nb) = (ao.node(a), bo.node(b));
    if na.ty != nb.ty || !is_element_node(na) {
        return false;
    }
    let (ea, eb) = (na.element(), nb.element());
    if ea.tag != eb.tag || ea.tag_namespace != eb.tag_namespace {
        return false;
    }
    ea.tag != GUMBO_TAG_UNKNOWN || element_name(ao, at, a) == element_name(bo, bt, b)
}

/// `node_key(node)`: an element's `data-lh-id` (its first attribute, else any in any case), or empty.
fn node_key(out: &GumboOutput, node: NodeId) -> &[u8] {
    let n = out.node(node);
    if !is_element_node(n) {
        return &[];
    }
    let attrs = &n.element().attributes;
    if let Some(first) = attrs.first()
        && strcmp(&first.name, b"data-lh-id")
    {
        return c_str(&first.value);
    }
    gumbo_get_attribute(attrs, b"data-lh-id").map_or(&[][..], |a| c_str(&a.value))
}

/// `raw_text_element(node)`.
fn raw_text_element(node: &GumboNode) -> bool {
    if !is_element_node(node) {
        return false;
    }
    let tag = node.element().tag;
    tag == GUMBO_TAG_STYLE
        || tag == GUMBO_TAG_SCRIPT
        || tag == GUMBO_TAG_TITLE
        || tag == GUMBO_TAG_TEXTAREA
        || tag == GUMBO_TAG_NOSCRIPT
        || tag == GUMBO_TAG_XMP
        || tag == GUMBO_TAG_IFRAME
        || tag == GUMBO_TAG_NOEMBED
        || tag == GUMBO_TAG_NOFRAMES
        || tag == GUMBO_TAG_PLAINTEXT
}

/// `has_document_level_element(node)`: the node or one below it is an element of the document's own (html, head,
/// body, style, link, base, meta, title, script, frameset) or a select's.
fn has_document_level_element(out: &GumboOutput, node: NodeId) -> bool {
    let mut pending = vec![node];
    while let Some(id) = pending.pop() {
        let n = out.node(id);
        if is_element_node(n) {
            let tag = n.element().tag;
            if tag == GUMBO_TAG_HTML
                || tag == GUMBO_TAG_HEAD
                || tag == GUMBO_TAG_BODY
                || tag == GUMBO_TAG_STYLE
                || tag == GUMBO_TAG_LINK
                || tag == GUMBO_TAG_BASE
                || tag == GUMBO_TAG_META
                || tag == GUMBO_TAG_TITLE
                || tag == GUMBO_TAG_SCRIPT
                || tag == GUMBO_TAG_FRAMESET
                || tag == GUMBO_TAG_SELECT
                || tag == GUMBO_TAG_OPTION
                || tag == GUMBO_TAG_OPTGROUP
                || tag == GUMBO_TAG_DATALIST
            {
                return true;
            }
        }
        pending.extend(node_children(out, id).iter().rev());
    }
    false
}

/// `count_elements(node, 0, elements, deepest)`: the element nodes, and the deepest's level (the first 0).
fn count_elements(out: &GumboOutput, node: NodeId) -> (i32, i32) {
    let (mut elements, mut deepest) = (0i32, 0i32);
    let mut pending = vec![(node, 0i32)];
    while let Some((id, mut depth)) = pending.pop() {
        if is_element_node(out.node(id)) {
            elements += 1;
            deepest = deepest.max(depth);
            depth += 1;
        }
        for &child in node_children(out, id) {
            pending.push((child, depth));
        }
    }
    (elements, deepest)
}

/// `same_subtree(a, b)`: the same node types, element kinds, attribute names and values, texts and children.
fn same_subtree(ao: &GumboOutput, at: &[u8], a: NodeId, bo: &GumboOutput, bt: &[u8], b: NodeId) -> bool {
    let mut pending = vec![(a, b)];
    while let Some((a, b)) = pending.pop() {
        let (na, nb) = (ao.node(a), bo.node(b));
        if na.ty != nb.ty {
            return false;
        }
        if is_element_node(na) {
            if !same_element_kind(ao, at, a, bo, bt, b) {
                return false;
            }
            let (x, y) = (&na.element().attributes, &nb.element().attributes);
            if x.len() != y.len() {
                return false;
            }
            for (p, q) in x.iter().zip(y) {
                if !same_text(&p.name, &q.name) || !same_text(&p.value, &q.value) {
                    return false;
                }
            }
        } else if na.ty != GUMBO_NODE_DOCUMENT {
            if !same_text(&na.text().text, &nb.text().text) {
                return false;
            }
            continue;
        }
        let (ca, cb) = (node_children(ao, a), node_children(bo, b));
        if ca.len() != cb.len() {
            return false;
        }
        for (&x, &y) in ca.iter().zip(cb) {
            pending.push((x, y));
        }
    }
    true
}

const diff_kept: i32 = 0;
const diff_replace: i32 = 1;
const diff_failed: i32 = 2;

/// `update_structure`'s `rebuild`: a parent's children made again, some kept.
#[derive(Debug)]
struct Rebuild {
    parent: ElementId,
    node: NodeId,
    keep: Vec<Vec<ElementId>>,
    create: Vec<u8>,
}

/// What `update_structure`'s diff gathers.
#[derive(Debug, Default)]
struct DiffOut {
    styles: Vec<(ElementId, Vec<u8>)>,
    retags: Vec<(ElementId, StringMap)>,
    rebuilds: Vec<Rebuild>,
    kept: Vec<(NodeId, ElementId)>,
    kept_text: Vec<(NodeId, Vec<ElementId>)>,
    retexts: Vec<(Vec<ElementId>, Vec<Vec<u8>>)>,
}

/// `update_structure`'s diff of the kept tree (`old`) against a new parse (`new`): its lambdas, over the document
/// as it is.
struct Diff<'a, 'c> {
    doc: &'a Document,
    cx: &'a StyleCx<'c>,
    old: &'a GumboOutput,
    old_text: &'a [u8],
    new: &'a GumboOutput,
    new_text: &'a [u8],
    attribute_scope: bool,
    position_scope: bool,
    out: DiffOut,
}

impl Diff<'_, '_> {
    fn holes_of(&self, node: NodeId) -> Option<&Vec<usize>> {
        if !self.doc.template_ok {
            return None;
        }
        self.doc.node_holes.get(&node)
    }

    /// `hole_in(holes, name)`: the attribute's hole (`Some(name)`) or the text's (`None`).
    fn hole_in(&self, holes: Option<&Vec<usize>>, name: Option<&[u8]>) -> Option<&HtmlHole> {
        for &index in holes? {
            let hole = &self.doc.holes[index];
            let matched = match name {
                Some(name) => !hole.text && c_str(&hole.name) == c_str(name),
                None => hole.text,
            };
            if matched {
                return Some(hole);
            }
        }
        None
    }

    fn mapped_element(&self, node: NodeId) -> Option<ElementId> {
        match self.doc.node_elements.get(&node) {
            Some(els) if els.len() == 1 => Some(els[0]),
            _ => None,
        }
    }

    fn current_text(&self, node: NodeId) -> Vec<ElementId> {
        if let Some(hole) = self.hole_in(self.holes_of(node), None) {
            return hole.text_elements.clone();
        }
        self.doc.node_elements.get(&node).cloned().unwrap_or_default()
    }

    fn same_text_node(&self, a: NodeId, b: NodeId) -> bool {
        if let Some(hole) = self.hole_in(self.holes_of(a), None) {
            let t = self.new.node(b).text().original_text;
            return t.data.is_some() && t.bytes(self.new_text) == &hole.value[..];
        }
        same_text(&self.old.node(a).text().text, &self.new.node(b).text().text)
    }

    fn same_kind(&self, a: NodeId, b: NodeId) -> bool {
        same_element_kind(self.old, self.old_text, a, self.new, self.new_text, b)
    }

    fn retag(&mut self, b: NodeId, el: ElementId, in_select: bool) -> bool {
        let e = self.new.node(b).element();
        let mut attrs = StringMap::new();
        for attr in &e.attributes {
            attrs.insert(c_str(&attr.name).to_vec(), c_str(&attr.value).to_vec());
        }
        let name = element_name(self.new, self.new_text, b).to_vec();
        if !self.doc.retag_allowed(self.cx, el, name, e.tag_namespace == GUMBO_NAMESPACE_SVG, &mut attrs, in_select || self.attribute_scope)
        {
            return false;
        }
        self.out.retags.push((el, attrs));
        true
    }

    fn diff_element(&mut self, a: NodeId, b: NodeId, el: ElementId, in_select: bool) -> i32 {
        let (x, y) = (&self.old.node(a).element().attributes, &self.new.node(b).element().attributes);
        let holes = self.holes_of(a);
        let mut style: Option<usize> = None;
        let mut renamed = x.len() != y.len();
        let mut i = 0;
        while i < x.len() && !renamed {
            let (p, q) = (&x[i], &y[i]);
            i += 1;
            if !same_text(&p.name, &q.name) {
                renamed = true;
                break;
            }
            let hole = self.hole_in(holes, Some(&p.name));
            let before: &[u8] = match hole {
                Some(hole) => &hole.value,
                None => c_str(&p.value),
            };
            let same = match hole {
                Some(hole) => {
                    let v = q.original_value;
                    v.data.is_some_and(|at| {
                        v.length >= 2
                            && self.new_text.get(at) == Some(&hole.close)
                            && self.new_text.get(at + v.length - 1) == Some(&hole.close)
                            && &self.new_text[at + 1..at + v.length - 1] == before
                    })
                }
                None => before == c_str(&q.value),
            };
            if same {
                continue;
            }
            if !strcmp(&p.name, b"style") {
                renamed = true;
                break;
            }
            if unsafe_style(before) || unsafe_style(c_str(&q.value)) {
                return diff_replace;
            }
            if self.doc.style_attributes_selectable(self.cx.names)
                || (self.doc.has_generated_children(el) && style_change_is_width_only(before, c_str(&q.value)))
            {
                return diff_failed;
            }
            style = Some(i - 1);
        }
        if raw_text_element(self.old.node(a)) {
            if renamed || style.is_some() || !same_subtree(self.old, self.old_text, a, self.new, self.new_text, b) {
                return diff_replace;
            }
            self.out.kept.push((b, el));
            return diff_kept;
        }
        if renamed {
            style = None;
            if !self.retag(b, el, in_select) {
                return diff_replace;
            }
        }
        let select = self.old.node(a).element().tag == GUMBO_TAG_SELECT;
        if !self.diff_children(a, b, Some(el), in_select || select) {
            return diff_failed;
        }
        if let Some(k) = style {
            let value = c_str(&self.new.node(b).element().attributes[k].value).to_vec();
            self.out.styles.push((el, value));
        }
        self.out.kept.push((b, el));
        diff_kept
    }

    fn diff_children(&mut self, a: NodeId, b: NodeId, parent: Option<ElementId>, in_select: bool) -> bool {
        let (ca, cb) = (node_children(self.old, a), node_children(self.new, b));
        let mut aligned = ca.len() == cb.len();
        let mut i = 0;
        while aligned && i < ca.len() {
            let (x, y) = (ca[i], cb[i]);
            i += 1;
            let (nx, ny) = (self.old.node(x), self.new.node(y));
            if nx.ty != ny.ty || (is_element_node(nx) && (!self.same_kind(x, y) || node_key(self.old, x) != node_key(self.new, y))) {
                aligned = false;
            }
        }
        let marks = (
            self.out.styles.len(),
            self.out.retags.len(),
            self.out.rebuilds.len(),
            self.out.kept.len(),
            self.out.kept_text.len(),
            self.out.retexts.len(),
        );
        let mut plan = Rebuild { parent: ElementId(0), node: b, keep: Vec::new(), create: Vec::new() };
        if aligned {
            let mut replaced = false;
            for i in 0..cb.len() {
                let (x, y) = (ca[i], cb[i]);
                let mut create;
                let nx = self.old.node(x);
                if is_element_node(nx) {
                    match self.mapped_element(x) {
                        None => {
                            create = !same_subtree(self.old, self.old_text, x, self.new, self.new_text, y);
                            replaced = replaced || create;
                        }
                        Some(el) => {
                            let result = self.diff_element(x, y, el, in_select);
                            if result == diff_failed {
                                return false;
                            }
                            create = result == diff_replace;
                            replaced = replaced || create;
                        }
                    }
                } else if nx.ty == GUMBO_NODE_TEXT {
                    create = !self.same_text_node(x, y);
                    let mut words = Vec::new();
                    if create
                        && !in_select
                        && self.doc.retext_in_place(self.cx, &self.current_text(x), Some(c_str(&self.new.node(y).text().text)), &mut words)
                    {
                        create = false;
                        let current = self.current_text(x);
                        self.out.retexts.push((current, words));
                    }
                    if !create {
                        let current = self.current_text(x);
                        self.out.kept_text.push((y, current));
                    }
                } else {
                    create = !same_text(&nx.text().text, &self.new.node(y).text().text);
                }
                if create && plan.create.is_empty() {
                    plan.create = vec![0; cb.len()];
                }
                if create {
                    plan.create[i] = 1;
                }
            }
            if plan.create.is_empty() {
                return true;
            }
            plan.keep.resize(cb.len(), Vec::new());
            let everything = replaced && self.attribute_scope;
            if everything {
                self.out.styles.truncate(marks.0);
                self.out.retags.truncate(marks.1);
                self.out.rebuilds.truncate(marks.2);
                self.out.kept.truncate(marks.3);
                self.out.kept_text.truncate(marks.4);
                self.out.retexts.truncate(marks.5);
            }
            for i in 0..cb.len() {
                let x = ca[i];
                let nx = self.old.node(x);
                if everything || (nx.ty != GUMBO_NODE_TEXT && !is_element_node(nx)) {
                    plan.create[i] = 1;
                }
                if plan.create[i] != 0 {
                    continue;
                }
                if nx.ty == GUMBO_NODE_TEXT {
                    plan.keep[i] = self.current_text(x);
                } else if let Some(el) = self.mapped_element(x) {
                    plan.keep[i] = vec![el];
                }
            }
        } else {
            plan.keep.resize(cb.len(), Vec::new());
            plan.create = vec![1; cb.len()];
            let mut keyed: BTreeMap<&[u8], NodeId> = BTreeMap::new();
            let mut duplicates: BTreeSet<&[u8]> = BTreeSet::new();
            let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
            if !self.position_scope {
                for &x in ca {
                    let key = node_key(self.old, x);
                    if !key.is_empty() {
                        if keyed.contains_key(key) {
                            duplicates.insert(key);
                        } else {
                            keyed.insert(key, x);
                        }
                    }
                }
                for &y in cb {
                    let key = node_key(self.new, y);
                    if !key.is_empty() && !seen.insert(key) {
                        duplicates.insert(key);
                    }
                }
            }
            let mut i = 0;
            while i < cb.len() && !self.position_scope {
                let y = cb[i];
                i += 1;
                let key = node_key(self.new, y);
                if key.is_empty() || duplicates.contains(key) {
                    continue;
                }
                let Some(&matched) = keyed.get(key) else { continue };
                if !self.same_kind(matched, y) {
                    continue;
                }
                let Some(el) = self.mapped_element(matched) else { continue };
                let result = self.diff_element(matched, y, el, in_select);
                if result == diff_failed {
                    return false;
                }
                if result == diff_kept {
                    plan.create[i - 1] = 0;
                    plan.keep[i - 1] = vec![el];
                }
            }
        }
        let na = self.old.node(a);
        let Some(parent) = parent else { return false };
        if in_select || (is_element_node(na) && (na.element().tag == GUMBO_TAG_HTML || na.element().tag == GUMBO_TAG_HEAD)) {
            return false;
        }
        let mut reused = BTreeSet::new();
        for i in 0..cb.len() {
            if plan.create[i] != 0 && has_document_level_element(self.new, cb[i]) {
                return false;
            }
            reused.extend(plan.keep[i].iter().copied());
        }
        for &x in ca {
            if !is_element_node(self.old.node(x)) {
                continue;
            }
            let el = self.mapped_element(x);
            if el.is_none_or(|el| !reused.contains(&el)) && has_document_level_element(self.old, x) {
                return false;
            }
        }
        plan.parent = parent;
        plan.node = b;
        self.out.rebuilds.push(plan);
        true
    }
}

// ---- html_tag.cpp and element.cpp: what patching asks of elements ----------------------------------------------

impl Document {
    /// `note_style_change()`.
    fn note_style_change(&mut self) {
        self.layout.style_epoch += 1;
    }

    /// `has_generated_children(owner)`: a `::before` or `::after` among its children.
    pub fn has_generated_children(&self, owner: ElementId) -> bool {
        self.children(owner).iter().any(|&c| {
            let tag = self.tag(c);
            tag == __tag_before_ || tag == __tag_after_
        })
    }

    /// `restore_base_style()`: the style before the `style` attribute was added, when there is one.
    pub fn restore_base_style(&mut self, id: ElementId) {
        let el = self.el_mut(id);
        if el.has_style_base {
            el.style = el.style_base.clone();
            el.has_style_base = false;
        }
    }

    /// `reset_attributes(attrs)`: every attribute replaced, as if the element were made with `attrs`.
    pub fn reset_attributes(&mut self, names: &Names, id: ElementId, attrs: &StringMap) {
        let el = self.el_mut(id);
        el.attrs.clear();
        el.str_classes.clear();
        el.classes.clear();
        el.id = empty_id;
        for (name, value) in attrs {
            self.set_attr(names, id, name, value);
        }
    }

    /// `forget_matched_styles()`: the `::before` and `::after` boxes let go, the style, counters, base style and
    /// matched selectors dropped.
    pub fn forget_matched_styles(&mut self, id: ElementId) {
        let children = std::mem::take(&mut self.el_mut(id).children);
        let mut kept = Vec::with_capacity(children.len());
        for child in children {
            let tag = self.tag(child);
            if tag == __tag_before_ || tag == __tag_after_ {
                self.el_mut(child).parent = None;
            } else {
                kept.push(child);
            }
        }
        let el = self.el_mut(id);
        el.children = kept;
        el.style.clear();
        el.counter_changes = 0;
        el.style_base = Style::default();
        el.has_style_base = false;
        el.used_styles.clear();
    }

    /// The declarations `parse_attributes` recorded for one element added to its style (and the master sheet applied
    /// to a form control's box it made), as [`Document::flush_declarations`] does for every element.
    fn flush_element_declarations(&mut self, cx: &StyleCx, id: ElementId) {
        let el = self.el_mut(id);
        let pending = el.declarations[el.declarations_applied..].to_vec();
        el.declarations_applied = el.declarations.len();
        for d in pending {
            let container = if d.with_container { Some(cx.container) } else { None };
            let style = &mut self.el_mut(id).style;
            match &d.value {
                crate::element::AttributeValue::Text(text) => {
                    style.add_property_text(cx.names, d.name, text, &d.baseurl, d.important, container)
                }
                crate::element::AttributeValue::Tokens(tokens) => {
                    style.add_property(cx.names, d.name, tokens, &d.baseurl, d.important, container)
                }
            }
        }
    }

    /// `parse_attributes()` of an element whose children are out, its declarations added to its style at once as the
    /// C++ adds them.
    fn parse_own_attributes(&mut self, cx: &StyleCx, id: ElementId) {
        self.parse_attributes(&cx.env(), id);
        self.flush_element_declarations(cx, id);
    }

    /// `apply_matching(sheets, epoch)`: the master sheet, the attributes, the document's and the user's sheets
    /// matched against the element; a memo of an epoch with the same attributes replays its rules instead. A leaf
    /// that matched from nothing and made no box keeps a memo (the last 2).
    pub fn apply_matching(&mut self, cx: &StyleCx, id: ElementId, epoch: u64) {
        let disabled = !cx.switches.patch.match_memo;
        if !disabled {
            let memo = self.el(id).patch.match_memos.iter().find(|m| m.epoch == epoch && m.attrs == self.el(id).attrs).cloned();
            if let Some(memo) = memo {
                for k in 0..3 {
                    if k == 1 {
                        self.parse_own_attributes(cx, id);
                    }
                    for sel in &memo.rules[k] {
                        self.apply_selector(cx, id, sel);
                    }
                }
                return;
            }
        }
        let sheets = [self.master_css.selectors().to_vec(), self.styles.selectors().to_vec(), self.user_css.selectors().to_vec()];
        let mut bounds = [0usize; 4];
        let start = self.el(id).used_styles.len();
        for k in 0..3 {
            if k == 1 {
                self.parse_own_attributes(cx, id);
            }
            bounds[k] = self.el(id).used_styles.len();
            self.apply_stylesheet(cx, id, &sheets[k]);
        }
        bounds[3] = self.el(id).used_styles.len();
        if disabled || start != 0 || !self.el(id).children.is_empty() {
            return;
        }
        let mut memo = MatchMemo { epoch, attrs: self.el(id).attrs.clone(), rules: Default::default() };
        for k in 0..3 {
            for i in bounds[k]..bounds[k + 1] {
                memo.rules[k].push(Arc::clone(&self.el(id).used_styles[i].selector));
            }
        }
        let memos = &mut self.el_mut(id).patch.match_memos;
        if memos.len() >= 2 {
            memos.remove(0);
        }
        memos.push(memo);
    }

    /// `flip_class(from, to, epoch, region)`: an element whose children are all text swaps its styles (and its
    /// texts') for those saved under the class value `to` when that was flipped from `from` in this epoch under the
    /// same parent style and other attributes; it saves the current ones under `from` either way (the last 3 values).
    pub fn flip_class(&mut self, cx: &StyleCx, id: ElementId, from: &[u8], to: &[u8], epoch: u64, region: &mut i32) -> bool {
        if !cx.switches.patch.class_flip {
            return false;
        }
        if self.children(id).iter().any(|&c| !self.is_text(c)) {
            return false;
        }
        let parent_id = self.parent(id).map_or(0, |p| self.el(p).css.m_shared_id);
        let same_others = |doc: &Document, others: &StringMap| -> bool {
            let mut a = others.iter();
            for (k, v) in &doc.el(id).attrs {
                if k == b"class" {
                    continue;
                }
                match a.next() {
                    Some((ak, av)) if ak == k && av == v => {}
                    _ => return false,
                }
            }
            a.next().is_none()
        };
        let mut flips = std::mem::take(&mut self.el_mut(id).patch.flips);
        let saved = match flips.iter().rposition(|s| s.value == from) {
            Some(saved) => saved,
            None => {
                if flips.len() >= 3 {
                    flips.remove(0);
                }
                flips.push(FlipState { value: from.to_vec(), parent_id: -1, ..FlipState::default() });
                flips.len() - 1
            }
        };
        if flips[saved].epoch != 0 && flips[saved].epoch != epoch {
            flips[saved].regions.clear();
        }
        flips[saved].epoch = epoch;
        flips[saved].parent_id = parent_id;
        if !same_others(self, &flips[saved].others) {
            let mut others = self.el(id).attrs.clone();
            others.remove(&b"class"[..]);
            flips[saved].others = others;
        }
        let children = self.children(id).to_vec();
        let target = flips.iter().rposition(|state| {
            state.value == to
                && state.epoch == epoch
                && state.parent_id == parent_id
                && state.kids.len() == children.len()
                && state.regions.contains_key(from)
                && same_others(self, &state.others)
        });
        let Some(target) = target else {
            let el = self.el(id);
            let state = &mut flips[saved];
            state.str_classes = el.str_classes.clone();
            state.classes = el.classes.clone();
            state.st = el.style.clone();
            state.st_base = el.style_base.clone();
            state.has_base = el.has_style_base;
            state.used = el.used_styles.clone();
            state.css = el.css.clone();
            state.kids = children.iter().map(|&c| self.el(c).css.clone()).collect();
            self.el_mut(id).patch.flips = flips;
            return false;
        };
        *region = flips[target].regions[from];
        let el = &mut self.elements[id.0 as usize];
        let (s, t) = if saved < target {
            let (lo, hi) = flips.split_at_mut(target);
            (&mut lo[saved], &mut hi[0])
        } else {
            let (lo, hi) = flips.split_at_mut(saved);
            (&mut hi[0], &mut lo[target])
        };
        s.str_classes = std::mem::replace(&mut el.str_classes, std::mem::take(&mut t.str_classes));
        s.classes = std::mem::replace(&mut el.classes, std::mem::take(&mut t.classes));
        s.st = std::mem::replace(&mut el.style, std::mem::take(&mut t.st));
        s.st_base = std::mem::replace(&mut el.style_base, std::mem::take(&mut t.st_base));
        s.used = std::mem::replace(&mut el.used_styles, std::mem::take(&mut t.used));
        s.css = std::mem::replace(&mut el.css, std::mem::take(&mut t.css));
        s.has_base = el.has_style_base;
        el.has_style_base = t.has_base;
        std::mem::swap(&mut s.kids, &mut t.kids);
        for (i, &child) in children.iter().enumerate() {
            std::mem::swap(&mut s.kids[i], &mut self.elements[child.0 as usize].css);
        }
        t.kids.clear();
        self.el_mut(id).attrs.insert(b"class".to_vec(), to.to_vec());
        self.el_mut(id).patch.flips = flips;
        true
    }

    /// `note_flip(from, to, region)`: the region of a flip between the two values, both ways.
    pub fn note_flip(&mut self, id: ElementId, from: &[u8], to: &[u8], region: i32) {
        let flips = &mut self.el_mut(id).patch.flips;
        let mut known = false;
        for state in flips.iter_mut() {
            if state.value == to {
                state.regions.insert(from.to_vec(), region);
                known = true;
            }
        }
        if !known {
            if flips.len() >= 3 {
                flips.remove(0);
            }
            let mut state = FlipState { value: to.to_vec(), parent_id: -1, ..FlipState::default() };
            state.regions.insert(from.to_vec(), region);
            flips.push(state);
        }
        for state in flips.iter_mut() {
            if state.value == from {
                state.regions.insert(to.to_vec(), region);
            }
        }
    }

    /// `forget_flips()`.
    pub fn forget_flips(&mut self, id: ElementId) {
        self.el_mut(id).patch.flips.clear();
    }

    /// `background_color_is_local()`: not anonymous, and no child tag inherits `background-color`.
    pub fn background_color_is_local(&self, id: ElementId) -> bool {
        if self.el(id).anonymous_style {
            return false;
        }
        for &child in self.children(id) {
            if self.kind(child).is_html_tag() && self.el(child).style.get_property(_background_color_).value == Value::Inherit {
                return false;
            }
        }
        true
    }

    /// `compute_background_style()`: the `style` attribute added again (the base kept), `var()`s substituted, and
    /// only the background colour computed, under a new share id.
    pub fn compute_background_style(&mut self, cx: &StyleCx, id: ElementId) {
        self.note_style_change();
        if let Some(style) = self.get_attr(id, b"style").map(<[u8]>::to_vec) {
            let el = self.el_mut(id);
            if !el.has_style_base {
                el.style_base = el.style.clone();
                el.has_style_base = true;
            }
            el.style.add_text(cx.names, cx.cache, &style, b"", Some(cx.container));
        }
        let mut style = std::mem::take(&mut self.el_mut(id).style);
        style.subst_vars(cx.names, &ElementVars { doc: self, cx, start: self.parent(id) });
        self.el_mut(id).style = style;
        let mut css = std::mem::take(&mut self.el_mut(id).css);
        css.compute_background_color(&Get { doc: self, cx, id });
        css.m_shared_id = self.next_share_id();
        self.el_mut(id).css = css;
    }

    /// `patch_inline_properties(names, patch)`: the patched declarations replace the properties named in the
    /// style (over the base), and only those are computed again; false (nothing changed) when the element or a child
    /// tag could see it otherwise. A paint group or transform that comes or goes restyles the element fully.
    pub fn patch_inline_properties(&mut self, cx: &StyleCx, id: ElementId, names: &[StringId], patch: &[u8]) -> bool {
        {
            let el = self.el(id);
            if el.anonymous_style || !el.has_style_base || el.style.has_var_property() || el.style_base.has_var_property() {
                return false;
            }
        }
        for &child in self.children(id) {
            if self.kind(child).is_html_tag() {
                for &name in names {
                    if self.el(child).style.get_property(name).value == Value::Inherit {
                        return false;
                    }
                }
            }
        }
        let mut scratch = Style::default();
        let mut fast = Vec::new();
        if names.len() == 1
            && names[0] == _transform_
            && patch.len() > 11
            && patch.starts_with(b"transform:")
            && patch.last() == Some(&b';')
            && crate::css_effects::fast_transform_tokens(&patch[10..patch.len() - 1], &mut fast)
        {
            if cx.container.sanitize_declaration(b"transform", &fast) {
                scratch.add_property_owned(cx.names, _transform_, fast, b"", false, Some(cx.container));
            }
        } else {
            scratch.add(cx.names, &normalize(patch, f_componentize, None), b"", Some(cx.container));
        }
        let mut ids = Vec::new();
        for &name in names {
            ids.push(name);
            let raw = raw_declaration_id(cx.names, name);
            if raw != empty_id {
                ids.push(raw);
            }
        }
        if scratch.has_var_property() || !scratch.only_declares(|id| ids.contains(&id)) {
            return false;
        }
        let group = self.el(id).css.is_paint_group();
        let transform = self.el(id).css.m_transform.is_empty();
        self.note_style_change();
        let mut style = std::mem::take(&mut self.el_mut(id).style);
        style.replace_properties(&ids, &self.el(id).style_base, &mut scratch);
        self.el_mut(id).style = style;
        let mut css = std::mem::take(&mut self.el_mut(id).css);
        for &name in names {
            let g = Get { doc: self, cx, id };
            if name == _transform_ {
                css.compute_transform(&g);
            } else if name == _opacity_ {
                css.compute_opacity(&g);
            } else {
                css.compute_background_color(&g);
            }
        }
        self.el_mut(id).css = css;
        if self.el(id).css.is_paint_group() != group || self.el(id).css.m_transform.is_empty() != transform {
            self.restore_base_style(id);
            self.compute_styles(cx, id, true);
            return true;
        }
        let shared = self.next_share_id();
        self.el_mut(id).css.m_shared_id = shared;
        true
    }

    /// `can_restyle_width_shallow()`: a plain element with a base style and a `width` length, no pseudo-class,
    /// `var()` or custom property, whose children neither are boxes of its own nor inherit `width`.
    pub fn can_restyle_width_shallow(&self, names: &Names, id: ElementId) -> bool {
        let el = self.el(id);
        if el.anonymous_style
            || !el.has_style_base
            || !el.pseudo_classes.is_empty()
            || el.style.has_var_property()
            || el.style_base.has_var_property()
            || el.style.declares_custom_property(names)
            || el.style_base.declares_custom_property(names)
        {
            return false;
        }
        match &el.style.get_property(_width_).value {
            Value::Length(width) if !width.is_predefined() => {}
            _ => return false,
        }
        for &child in &el.children {
            let tag = self.tag(child);
            if tag == __tag_before_ || tag == __tag_after_ {
                return false;
            }
            if self.kind(child).is_html_tag() {
                let c = self.el(child);
                if c.anonymous_style
                    || c.style.get_property(_width_).value == Value::Inherit
                    || c.style.has_var_property()
                    || c.style_base.has_var_property()
                {
                    return false;
                }
            }
        }
        true
    }

    /// `replace_children(old, replacement)`: the run of children `old` (in order) replaced by `replacement`, which
    /// get this parent (`old` keep theirs); false when the run is not there.
    pub fn replace_children(&mut self, parent: ElementId, old: &[ElementId], replacement: &[ElementId]) -> bool {
        let Some(&front) = old.first() else { return false };
        let children = &self.el(parent).children;
        let Some(first) = children.iter().position(|&c| c == front) else { return false };
        let mut it = first;
        for &el in old {
            if it == children.len() || children[it] != el {
                return false;
            }
            it += 1;
        }
        for &el in replacement {
            self.el_mut(el).parent = Some(parent);
        }
        self.el_mut(parent).children.splice(first..it, replacement.iter().copied());
        true
    }

    /// `el_text::set_text(text)`: the text, its measure and breaker let go, its layout marked, its computed style
    /// fresh but for inline text.
    pub fn set_text(&mut self, cx: &StyleCx, id: ElementId, text: &[u8]) {
        let el = self.el_mut(id);
        el.text = text.to_vec();
        let state = &mut el.text_state;
        state.transformed_text.clear();
        state.use_transformed = false;
        state.draw_spaces = true;
        state.size = Cell::new((Px(0.0), Px(0.0)));
        state.measured_font = 0;
        state.width_owed = Cell::new(false);
        let breaker = std::mem::take(&mut state.breaker);
        if breaker != 0 {
            cx.container.drop_text_breaker(breaker);
        }
        self.mark_element_layout_dirty(cx, id);
        let css = &mut self.el_mut(id).css;
        *css = CssProperties::default();
        css.m_display = StyleDisplay::display_inline_text.index();
    }

    /// `el_image::update_src()`.
    pub fn update_src(&mut self, id: ElementId) {
        let src = self.get_attr(id, b"src").unwrap_or(b"").to_vec();
        self.el_mut(id).state = ElementState::Image { src, loaded: false };
    }

    /// `push_text_owner_region(owner, regions)`: an owner (not a list item) whose children are all text repaints
    /// itself and its words.
    fn push_text_owner_region(&self, cx: &StyleCx, owner: Option<ElementId>, regions: &mut Vec<ElementId>) -> bool {
        let Some(owner) = owner else { return false };
        if !cx.switches.patch.color_regions || self.children(owner).is_empty() || self.el(owner).css.m_display == display_list_item {
            return false;
        }
        if self.children(owner).iter().any(|&c| !self.is_text(c)) {
            return false;
        }
        regions.push(owner);
        for &child in self.children(owner) {
            if self.kind(child) == ElementKind::el_text {
                regions.push(child);
            }
        }
        true
    }

    /// `outermost_svg(el)`: the outermost `<svg>` at or above the element (none in a document without one).
    pub fn outermost_svg(&self, el: Option<ElementId>) -> Option<ElementId> {
        let el = el?;
        if !self.has_svg {
            return None;
        }
        let mut found = None;
        let mut p = Some(el);
        while let Some(e) = p {
            if self.kind(e) == ElementKind::el_svg {
                found = Some(e);
            }
            p = self.parent(e);
        }
        found
    }

    /// `shallow_width_owner(owner)`: a plain block, inline-block, flex or grid box that may restyle its width alone.
    fn shallow_width_owner(&self, names: &Names, owner: ElementId) -> bool {
        if !matches!(self.kind(owner), ElementKind::html_tag | ElementKind::el_div | ElementKind::el_para | ElementKind::el_body) {
            return false;
        }
        let d = self.el(owner).css.m_display;
        if d == display_block
            || d == display_inline_block
            || d == display_flex
            || d == display_inline_flex
            || d == display_grid
            || d == display_inline_grid
        {
            return self.can_restyle_width_shallow(names, owner);
        }
        false
    }
}

// ---- document.cpp: patching ---------------------------------------------------------------------------------

impl Document {
    /// `last_patch_paint_only()`.
    pub fn last_patch_paint_only(&self) -> bool {
        self.patch.paint_only_patch
    }

    /// `last_patch_regions(out)`: after a paint-only patch that is confined, the elements whose border boxes hold
    /// every pixel it can change.
    pub fn last_patch_regions(&self) -> Option<Vec<ElementId>> {
        if !self.patch.paint_only_patch || !self.patch.patch_confined {
            return None;
        }
        Some(self.patch.patch_regions.clone())
    }

    /// `template_html(out)`: the HTML the document stands for, the literals with every hole's value.
    pub fn template_html(&self) -> Option<Vec<u8>> {
        if !self.template_ok || self.literals.len() != self.holes.len() + 1 {
            return None;
        }
        let mut out = Vec::new();
        for (literal, hole) in self.literals.iter().zip(&self.holes) {
            out.extend_from_slice(literal);
            out.extend_from_slice(&hole.value);
        }
        out.extend_from_slice(self.literals.last().expect("a literal"));
        Some(out)
    }

    /// `selector_scope()`: what the sheets' selectors read beyond their subject (bit 0 attributes, 1 positions, 2
    /// counters or `:empty`), the names they read of others gathered on the way; looked at once.
    pub fn selector_scope(&mut self, cx: &StyleCx) -> i32 {
        if self.patch.selector_scope >= 0 {
            return self.patch.selector_scope;
        }
        let (mut attributes, mut positions, mut emptiness) = (false, false, false);
        let mut left = std::mem::take(&mut self.patch);
        for sheet in [&self.master_css, &self.styles, &self.user_css] {
            for sel in sheet.selectors() {
                scan_selector(sel, &mut attributes, &mut positions, &mut emptiness);
                let declares = sel.m_style.as_ref().is_some_and(|rule| rule.style.declares_custom_property(cx.names));
                collect_ancestor_names(cx.names, sel, !declares, &mut left);
            }
        }
        let mut counters = mentions_counters(&left.master_styles);
        for sheet in &self.css {
            counters = counters || mentions_counters(&sheet.text);
        }
        left.selector_scope = i32::from(attributes) | if positions { 2 } else { 0 } | if counters || emptiness { 4 } else { 0 };
        self.patch = left;
        self.patch.selector_scope
    }

    /// `style_attributes_selectable()`: a selector of the three sheets reads the `style` attribute; looked at once.
    pub fn style_attributes_selectable(&self, names: &Names) -> bool {
        let cached = self.patch.style_attributes_selectable.get();
        if cached >= 0 {
            return cached != 0;
        }
        for sheet in [&self.master_css, &self.styles, &self.user_css] {
            for sel in sheet.selectors() {
                if reads_style(names, sel) {
                    self.patch.style_attributes_selectable.set(1);
                    return true;
                }
            }
        }
        self.patch.style_attributes_selectable.set(0);
        false
    }

    /// `retag_allowed(el, name, svg, attrs, blocked)`: a plain element (not in a select, nor where attribute
    /// selectors reach) keeping its tag through the container's sanitizer may take the new attributes when no
    /// selector reads, of another element, an attribute, id or class that changes, and its style changes no counter,
    /// `var()` or custom property. `attrs` is left as the sanitizer made it.
    pub fn retag_allowed(&self, cx: &StyleCx, el: ElementId, mut name: Vec<u8>, svg: bool, attrs: &mut StringMap, blocked: bool) -> bool {
        if !self.kind(el).is_html_tag() || blocked || !restylable_in_place(self.kind(el)) {
            return false;
        }
        let tag_name = self.get_tag_name(cx.names, el);
        if name.is_empty()
            || lowcase(&name) != tag_name
            || !cx.container.sanitize_element(&mut name, svg, attrs)
            || lowcase(&name) != tag_name
        {
            return false;
        }
        let before = &self.el(el).attrs;
        let old_style = before.get(&b"style"[..]).cloned().unwrap_or_default();
        let new_style = attrs.get(&b"style"[..]).cloned().unwrap_or_default();
        if old_style != new_style && (unsafe_style(&old_style) || unsafe_style(&new_style)) {
            return false;
        }
        let quirks = self.mode == DocumentMode::quirks_mode;
        let normalized = |value: Option<&Vec<u8>>| -> Vec<u8> {
            let text = value.cloned().unwrap_or_default();
            if quirks { lowcase(&text) } else { text }
        };
        let names = cx.names;
        let left = &self.patch;
        let reaches_others = |attr: &[u8], old: Option<&Vec<u8>>, next: Option<&Vec<u8>>| -> bool {
            if left.left_attrs.contains(&names.id(attr)) {
                return true;
            }
            if attr == b"id" {
                return left.left_ids.contains(&names.id(&normalized(old))) || left.left_ids.contains(&names.id(&normalized(next)));
            }
            if attr != b"class" {
                return false;
            }
            let mut a = BTreeSet::new();
            let mut c = BTreeSet::new();
            for cls in split_string(&normalized(old), WHITESPACE, b"", b"") {
                a.insert(names.id(&cls));
            }
            for cls in split_string(&normalized(next), WHITESPACE, b"", b"") {
                c.insert(names.id(&cls));
            }
            for cls in &a {
                if !c.contains(cls) && left.left_classes.contains(cls) {
                    return true;
                }
            }
            for cls in &c {
                if !a.contains(cls) && left.left_classes.contains(cls) {
                    return true;
                }
            }
            false
        };
        for (k, v) in before {
            let now = attrs.get(k);
            if now != Some(v) && reaches_others(k, Some(v), now) {
                return false;
            }
        }
        for (k, v) in attrs.iter() {
            if !before.contains_key(k) && reaches_others(k, None, Some(v)) {
                return false;
            }
        }
        true
    }

    /// `apply_retag(el, attrs, check, region)`: the element takes the attributes and is matched again from nothing
    /// (its `::before` and `::after` made again). With `check`, true when it has no generated box before or after,
    /// its other attributes stayed and the rules that came or went only declare paint-only properties; `region` then
    /// tells where they repaint (1 the box, 3 its texts' colour, 0 elsewhere).
    pub fn apply_retag(&mut self, cx: &StyleCx, el: ElementId, attrs: &StringMap, check: bool, region: Option<&mut i32>) -> bool {
        let mut region = region;
        if let Some(region) = region.as_deref_mut() {
            *region = 0;
        }
        let used_rules = |doc: &Document| -> Vec<(usize, bool, Arc<CssSelector>)> {
            let mut rules: Vec<(usize, bool, Arc<CssSelector>)> =
                doc.el(el).used_styles.iter().map(|u| (Arc::as_ptr(&u.selector) as usize, u.used, Arc::clone(&u.selector))).collect();
            rules.sort_by_key(|r| (r.0, r.1));
            rules
        };
        if !self.kind(el).is_html_tag() {
            return false;
        }
        let neutral = check && !self.has_generated_children(el);
        let mut rules_before = Vec::new();
        let mut attrs_before = StringMap::new();
        if neutral {
            rules_before = used_rules(self);
            attrs_before = self.el(el).attrs.clone();
        }
        self.reset_attributes(cx.names, el, attrs);
        self.forget_matched_styles(el);
        let mut children = std::mem::take(&mut self.el_mut(el).children);
        let epoch = self.patch.match_epoch;
        self.apply_matching(cx, el, epoch);
        let pseudo = std::mem::take(&mut self.el_mut(el).children);
        for child in pseudo {
            if self.tag(child) == __tag_before_ {
                children.insert(0, child);
            } else {
                children.push(child);
            }
        }
        self.el_mut(el).children = children;
        if !neutral || self.has_generated_children(el) {
            return false;
        }
        attrs_before.remove(&b"class"[..]);
        let mut attrs_after = self.el(el).attrs.clone();
        attrs_after.remove(&b"class"[..]);
        if attrs_before != attrs_after {
            return false;
        }
        let rules_after = used_rules(self);
        // std::set_symmetric_difference of the two sorted lists.
        let mut changed = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < rules_before.len() || j < rules_after.len() {
            if j == rules_after.len()
                || (i < rules_before.len() && (rules_before[i].0, rules_before[i].1) < (rules_after[j].0, rules_after[j].1))
            {
                changed.push(&rules_before[i].2);
                i += 1;
            } else if i == rules_before.len() || (rules_after[j].0, rules_after[j].1) < (rules_before[i].0, rules_before[i].1) {
                changed.push(&rules_after[j].2);
                j += 1;
            } else {
                i += 1;
                j += 1;
            }
        }
        let kind = Cell::new(1);
        for rule in changed {
            let Some(style) = &rule.m_style else { return false };
            let only = style.style.only_declares(|name| {
                let text = cx.names.s(name);
                let property: &[u8] = if text.starts_with(b"-lh-raw-") { &text[8..] } else { &text };
                let declared = declared_region(property);
                let k = kind.get();
                // declared == 0 ? 0 : (kind == 0 ? 0 : std::max(kind, declared))
                kind.set(if declared == 0 || k == 0 { 0 } else { k.max(declared) });
                paint_only_property(property)
            });
            if !only {
                return false;
            }
        }
        if let Some(region) = region {
            *region = kind.get();
        }
        true
    }

    /// `class_change_allowed(hole, value, attrs)`: a class hole of an element in the tree, not in a select, may take
    /// the value when no selector reads attributes, positions, counters or `:empty` and the retag is allowed;
    /// `attrs` gets the attributes it would have.
    fn class_change_allowed(&mut self, cx: &StyleCx, hole: usize, value: &[u8], attrs: &mut StringMap) -> bool {
        let Some(owner) = self.holes[hole].owner else { return false };
        if !cx.switches.patch.class_holes || self.parent(owner).is_none() || (self.selector_scope(cx) & 5) != 0 {
            return false;
        }
        let mut p = self.parent(owner);
        while let Some(e) = p {
            if self.tag(e) == _select_ {
                return false;
            }
            p = self.parent(e);
        }
        if !self.kind(owner).is_html_tag() {
            return false;
        }
        *attrs = self.el(owner).attrs.clone();
        attrs.insert(b"class".to_vec(), value.to_vec());
        let name = self.get_tag_name(cx.names, owner);
        self.retag_allowed(cx, owner, name, false, attrs, false)
    }

    /// `retext_in_place(current, text, words)`: the text splits into words and spaces of the same kinds, the
    /// spaces alike, as the elements it made; `words` gets the new pieces.
    pub fn retext_in_place(&self, cx: &StyleCx, current: &[ElementId], text: Option<&[u8]>, words: &mut Vec<Vec<u8>>) -> bool {
        if !cx.switches.patch.inplace_text || current.is_empty() {
            return false;
        }
        let Some(text) = text else { return false };
        let Some(parent) = self.parent(current[0]) else { return false };
        let mut parts: Vec<(bool, Vec<u8>)> = Vec::new();
        cx.container.split_text(c_str(text), &mut |piece, space| parts.push((space, piece.to_vec())));
        if parts.len() != current.len() {
            return false;
        }
        words.clear();
        for (i, (space_part, piece)) in parts.into_iter().enumerate() {
            let el = current[i];
            let space = self.kind(el) == ElementKind::el_space;
            if (!space && self.kind(el) != ElementKind::el_text) || space != space_part || self.parent(el) != Some(parent) {
                return false;
            }
            if space {
                let mut before = Vec::new();
                self.get_text(el, &mut before);
                if before != piece {
                    return false;
                }
            }
            words.push(piece);
        }
        true
    }

    /// `replace_text_renderers(current, replacement)`: when the owner's children are exactly the replacement and an
    /// inline context of the live tree held exactly the old texts' items, those items are made again from the
    /// replacement and the context's layout marked.
    fn replace_text_renderers(&mut self, cx: &StyleCx, current: &[ElementId], replacement: &[ElementId]) -> bool {
        if current.is_empty() || replacement.is_empty() {
            return false;
        }
        let Some(owner) = self.parent(current[0]) else { return false };
        if self.children(owner) != replacement {
            return false;
        }
        let mut target = None;
        for item in self.live_renders(owner).collect::<Vec<_>>() {
            let it = self.render.item(item);
            if it.kind != RenderKind::render_item_inline_context || it.children.len() != current.len() {
                continue;
            }
            let mut root = item;
            while let Some(parent) = self.render.parent(root) {
                root = parent;
            }
            if Some(root) != self.render.root {
                continue;
            }
            if it.children.iter().zip(current).any(|(&child, &old)| self.render.item(child).element != old) {
                continue;
            }
            target = Some(item);
            break;
        }
        let Some(target) = target else { return false };
        for &child in self.render.children(target) {
            self.patch.layout_marked.retain(|&m| m != child);
        }
        let old = self.render_children_drain(target, 0, self.render.children(target).len());
        for child in old {
            self.release_render(child);
        }
        for &child in replacement {
            let made = self.create_render_item(cx, child, Some(target)).expect("a text makes a render item");
            let item = self.render_init(cx, made);
            self.add_render_child(target, item);
        }
        self.mark_layout_dirty(cx, target);
        self.render.serial += 1;
        true
    }

    /// `forget_positions()`: the places of the items marked since the last layout (with the layout cache; every
    /// item's without), so the next layout places them afresh.
    pub fn forget_positions(&mut self, cx: &StyleCx) {
        let Some(root) = self.render.root else { return };
        if cx.switches.layout_cache {
            for item in std::mem::take(&mut self.patch.layout_marked) {
                // A marked item is alive while the tree is (the C++ keeps raw pointers).
                if self.render.alive(item) {
                    self.render.item_mut(item).pos = Position::default();
                }
            }
            return;
        }
        let mut pending = vec![root];
        while let Some(item) = pending.pop() {
            self.render.item_mut(item).pos = Position::default();
            pending.extend(self.render.children(item).iter().copied());
        }
    }

    /// `begin_layout_patch()`: the patch's layout state cleared, and (with the layout cache) every box taken down.
    pub fn begin_layout_patch(&mut self, cx: &StyleCx) {
        self.patch.layout_origins.clear();
        self.patch.restyle_regions.clear();
        self.patch.restyle_confined = true;
        self.patch.layout_before_valid = false;
        self.layout.after_render = false;
        if self.render.root.is_some() && cx.switches.layout_cache {
            self.snapshot_layout();
        }
    }

    /// The start every entry point shares: the last patch's answers cleared.
    fn begin_patch(&mut self, changed: &mut bool) {
        self.invalidate_paint();
        self.patch.paint_only_patch = false;
        self.patch.patch_confined = false;
        self.patch.patch_regions.clear();
        *changed = false;
    }

    /// `update_from_string(html, changed)`: the document patched to the new HTML in place: through the template
    /// when the text differs only inside its holes, else by the structural diff; false, the document as it was (but
    /// for the patch's layout state), when neither can. `changed` tells whether anything differed.
    pub fn update_from_string(&mut self, cx: &StyleCx, html: &[u8], changed: &mut bool) -> bool {
        let ret = self.update_from_string_inner(cx, html, changed);
        self.release_dead_generated();
        ret
    }

    fn update_from_string_inner(&mut self, cx: &StyleCx, html: &[u8], changed: &mut bool) -> bool {
        self.begin_patch(changed);
        if self.gumbo.is_none() || self.root.is_none() || !self.finalized {
            return false;
        }
        if self.template_ok && cx.switches.patch.template {
            let result = self.update_from_template(cx, html, changed);
            if result == 1 {
                return true;
            }
            if result == template_requires_fresh_document {
                return false;
            }
        }
        self.update_structure(cx, html, changed)
    }

    /// `update_from_template(html, changed)`: 1 patched (or nothing changed), 0 the text does not fit the
    /// template, -1 it fits but needs a rebuild, -2 it needs a new document.
    fn update_from_template(&mut self, cx: &StyleCx, html: &[u8], changed: &mut bool) -> i32 {
        self.invalidate_paint();
        let mut changes = Vec::new();
        let mut pos = 0;
        for i in 0..self.holes.len() {
            let literal = &self.literals[i];
            if html.get(pos..(pos + literal.len()).min(html.len())) != Some(&literal[..]) {
                return 0;
            }
            pos += literal.len();
            let close = if self.holes[i].text { b'<' } else { self.holes[i].close };
            let Some(end) = html[pos..].iter().position(|&c| c == close).map(|k| pos + k) else { return 0 };
            if html[pos..end] != self.holes[i].value[..] {
                changes.push((i, html[pos..end].to_vec()));
            }
            pos = end;
        }
        let last = self.literals.last().expect("a literal");
        if html.len() - pos != last.len() || html[pos..] != last[..] {
            return 0;
        }
        self.apply_template_values(cx, &mut changes, changed)
    }

    /// `update_styles(styles, changed)`: the `style` values of the elements patched through their style holes;
    /// false, the document as it was, when one has none or a value could not come from serialized HTML.
    pub fn update_styles(&mut self, cx: &StyleCx, styles: &[(Option<ElementId>, Vec<u8>)], changed: &mut bool) -> bool {
        let ret = self.update_styles_inner(cx, styles, changed);
        self.release_dead_generated();
        ret
    }

    fn update_styles_inner(&mut self, cx: &StyleCx, styles: &[(Option<ElementId>, Vec<u8>)], changed: &mut bool) -> bool {
        self.begin_patch(changed);
        if self.root.is_none() || !self.finalized || !self.template_ok || !cx.switches.patch.template {
            return false;
        }
        if self.patch.style_holes.is_empty() {
            for (i, hole) in self.holes.iter().enumerate() {
                if !hole.text
                    && !hole.src
                    && !hole.cls
                    && hole.attr.is_empty()
                    && let Some(owner) = hole.owner
                {
                    self.patch.style_holes.entry(owner).or_insert(i);
                }
            }
        }
        let mut changes: Vec<(usize, Vec<u8>)> = Vec::with_capacity(styles.len());
        for (owner, value) in styles {
            let Some(&index) = owner.and_then(|o| self.patch.style_holes.get(&o)) else { return false };
            let hole = &self.holes[index];
            if value.contains(&hole.close) || value.contains(&b'<') {
                return false;
            }
            if *value == hole.value {
                continue;
            }
            if changes.iter().any(|c| c.0 == index) {
                return false;
            }
            changes.push((index, value.clone()));
        }
        changes.sort_by_key(|c| c.0);
        self.apply_template_values(cx, &mut changes, changed) == 1
    }

    /// `update_holes(changes, changed)`: attribute values and sole texts patched through their holes, each the
    /// exact text serialized HTML would carry; false, the document as it was, when a change has no hole of its own.
    pub fn update_holes(&mut self, cx: &StyleCx, changes: &[HoleChange], changed: &mut bool) -> bool {
        let ret = self.update_holes_inner(cx, changes, changed);
        self.release_dead_generated();
        ret
    }

    fn update_holes_inner(&mut self, cx: &StyleCx, changes: &[HoleChange], changed: &mut bool) -> bool {
        self.begin_patch(changed);
        if self.root.is_none() || !self.finalized || !self.template_ok || !cx.switches.patch.template {
            return false;
        }
        if self.patch.owner_holes.is_empty() {
            for (i, hole) in self.holes.iter().enumerate() {
                let owner = if !hole.text { hole.owner } else { hole.text_elements.first().and_then(|&t| self.parent(t)) };
                if let Some(owner) = owner {
                    self.patch.owner_holes.entry(owner).or_default().push(i);
                }
            }
        }
        let mut list: Vec<(usize, Vec<u8>)> = Vec::with_capacity(changes.len());
        let mut seen: Vec<usize> = Vec::with_capacity(changes.len());
        for change in changes {
            let Some(indices) = change.owner.and_then(|o| self.patch.owner_holes.get(&o)) else { return false };
            let mut found = self.holes.len();
            for &i in indices {
                let hole = &self.holes[i];
                if if change.text { !hole.text } else { hole.text || hole.name != change.name } {
                    continue;
                }
                if found != self.holes.len() {
                    return false;
                }
                found = i;
            }
            if found == self.holes.len() {
                return false;
            }
            let hole = &self.holes[found];
            if change.value.contains(&b'<') || (!hole.text && change.value.contains(&hole.close)) {
                return false;
            }
            if hole.text && hole.text_elements.first().is_none_or(|&t| self.parent(t) != change.owner) {
                return false;
            }
            if seen.contains(&found) {
                return false;
            }
            seen.push(found);
            if change.value != hole.value {
                list.push((found, change.value.clone()));
            }
        }
        list.sort_by_key(|c| c.0);
        self.apply_template_values(cx, &mut list, changed) == 1
    }

    /// `apply_template_values(changes, changed)`: the holes' new values into the document: classes flipped or
    /// retagged, texts changed in place or made again, attributes and styles set; then the restyle, whether the patch
    /// is paint-only and the regions it repaints. 1 patched, -1 a value needs a rebuild, -2 a new document.
    fn apply_template_values(&mut self, cx: &StyleCx, changes: &mut [(usize, Vec<u8>)], changed: &mut bool) -> i32 {
        self.budget_enter();
        let ret = self.apply_template_values_scoped(cx, changes, changed);
        self.budget_exit();
        ret
    }

    fn apply_template_values_scoped(&mut self, cx: &StyleCx, changes: &mut [(usize, Vec<u8>)], changed: &mut bool) -> i32 {
        let sw = cx.switches.patch;
        self.begin_layout_patch(cx);
        self.invalidate_paint();
        let n = changes.len();
        let mut dirty: Vec<usize> = Vec::new();
        let mut background_changes = vec![false; n];
        let mut width_changes = vec![false; n];
        let mut property_patches = vec![PropertyPatch::default(); n];
        let mut classes: Vec<(usize, StringMap)> = Vec::new();
        for k in 0..n {
            let i = changes[k].0;
            let v = changes[k].1.clone();
            if i >= self.holes.len() || v == self.holes[i].value {
                continue;
            }
            if find_first_of(&v, b"&\r") || v.contains(&0) || !is_valid_utf8(&v) {
                return -1;
            }
            if self.holes[i].cls {
                let mut attrs = StringMap::new();
                if !self.class_change_allowed(cx, i, &v, &mut attrs) {
                    return -1;
                }
                classes.push((k, attrs));
                continue;
            }
            let hole = &self.holes[i];
            if hole.text {
                if all_of(&v, b" \t\n\x0c") {
                    return -1;
                }
            } else if !hole.src && hole.attr.is_empty() {
                if self.style_attributes_selectable(cx.names) {
                    return template_requires_fresh_document;
                }
                let owner = hole.owner.expect("a style hole has its element");
                let patch = &mut property_patches[k];
                if !style_change_property_patch(&sw, &hole.value, &v, patch) {
                    patch.names.clear();
                    width_changes[k] = style_change_is_width_only(&hole.value, &v);
                    if width_changes[k] && self.has_generated_children(owner) {
                        return template_requires_fresh_document;
                    }
                    let background = style_change_is_background_color(&sw, &hole.value, &v);
                    background_changes[k] = background;
                    if !background && !style_change_is_local(&hole.value, &v) {
                        return -1;
                    }
                }
            }
            if hole.text && hole.text_elements.first().is_none_or(|&t| self.parent(t).is_none()) {
                return -1;
            }
            if hole.text && v[0] == b'\n' {
                let parent = self.parent(hole.text_elements[0]).expect("a text in an element");
                let tag = self.get_tag_name(cx.names, parent);
                if tag == b"pre" || tag == b"listing" || tag == b"textarea" {
                    return -1;
                }
            }
            dirty.push(k);
        }
        if dirty.is_empty() && classes.is_empty() {
            self.patch.paint_only_patch = true;
            self.patch.patch_confined = true;
            self.patch.patch_regions.clear();
            return 1;
        }
        *changed = true;

        let mut restyle: Vec<ElementId> = Vec::new();
        let mut background_only: BTreeSet<ElementId> = BTreeSet::new();
        let mut patched: BTreeMap<ElementId, &PropertyPatch> = BTreeMap::new();
        let mut shallow_width: BTreeSet<ElementId> = BTreeSet::new();
        let mut allow_shallow_width = classes.is_empty() && !self.has_svg && self.render.tabular.is_empty() && sw.shallow_width_patch;
        let mut new_text: Vec<ElementId> = Vec::new();
        let mut retext: Vec<ElementId> = Vec::new();
        let mut words: Vec<Vec<u8>> = Vec::new();
        let mut keep_tree = sw.keep_tree;
        let mut paint_only = sw.paint_only;
        self.patch.patch_regions.clear();
        self.patch.patch_confined = sw.patch_regions;
        for (k, attrs) in &classes {
            let hole_index = changes[*k].0;
            let value = changes[*k].1.clone();
            let mut region = 0;
            let owner = self.holes[hole_index].owner.expect("a class hole has its element");
            if !self.kind(owner).is_html_tag() {
                return -1;
            }
            let from = self.holes[hole_index].value.clone();
            let epoch = self.patch.match_epoch;
            let fast = self.flip_class(cx, owner, &from, &value, epoch, &mut region);
            if !fast {
                if !self.apply_retag(cx, owner, attrs, true, Some(&mut region)) {
                    keep_tree = false;
                    paint_only = false;
                    self.forget_flips(owner);
                } else {
                    self.note_flip(owner, &from, &value, region);
                }
            }
            let mut regions = std::mem::take(&mut self.patch.restyle_regions);
            if region == 1 {
                regions.push(owner);
            } else if region != 3
                || self.outermost_svg(Some(owner)).is_some()
                || !self.push_text_owner_region(cx, Some(owner), &mut regions)
            {
                self.patch.restyle_confined = false;
            }
            self.patch.restyle_regions = regions;
            let mut regions = std::mem::take(&mut self.patch.patch_regions);
            if self.patch.patch_confined && region == 1 {
                regions.push(owner);
            } else if !self.patch.patch_confined
                || region != 3
                || self.outermost_svg(Some(owner)).is_some()
                || !self.push_text_owner_region(cx, Some(owner), &mut regions)
            {
                self.patch.patch_confined = false;
            }
            self.patch.patch_regions = regions;
            self.holes[hole_index].value = value;
            if !fast {
                restyle.push(owner);
            }
        }
        let resize_only = !sw.same_size_text;
        let mut measured: Vec<(ElementId, Size)> = Vec::new();
        for &k in &dirty {
            let value = changes[k].1.clone();
            let hi = changes[k].0;
            let (text, src, attr_empty, owner) =
                (self.holes[hi].text, self.holes[hi].src, self.holes[hi].attr.is_empty(), self.holes[hi].owner);
            let inplace = text && {
                let current = self.holes[hi].text_elements.clone();
                self.retext_in_place(cx, &current, Some(&value), &mut words)
            };
            let background = background_changes[k];
            let patch = if property_patches[k].names.is_empty() { None } else { Some(&property_patches[k]) };
            if if text { !inplace } else { src || !attr_empty || (!width_changes[k] && patch.is_none() && !background) } {
                allow_shallow_width = false;
            }
            if allow_shallow_width && width_changes[k] {
                let owner = owner.expect("a style hole has its element");
                if self.shallow_width_owner(cx.names, owner) {
                    shallow_width.insert(owner);
                }
            }
            let kind_of = |doc: &Document| -> i32 {
                if !attr_empty {
                    return 2;
                }
                if background {
                    1
                } else if let Some(patch) = patch {
                    if patch.box_ { 1 } else { 0 }
                } else {
                    style_change_region(&doc.holes[hi].value, &value)
                }
            };
            if !text && !src {
                let kind = kind_of(self);
                let mut regions = std::mem::take(&mut self.patch.restyle_regions);
                if kind == 1 {
                    regions.push(owner.expect("an attribute hole has its element"));
                } else if kind == 2 && self.outermost_svg(owner).is_some() {
                    regions.push(self.outermost_svg(owner).expect("an svg"));
                } else if kind != 3 || self.outermost_svg(owner).is_some() || !self.push_text_owner_region(cx, owner, &mut regions) {
                    self.patch.restyle_confined = false;
                }
                self.patch.restyle_regions = regions;
            }
            if self.patch.patch_confined {
                let mut region = None;
                let mut words_region = false;
                let mut continue_region = false;
                let mut regions = std::mem::take(&mut self.patch.patch_regions);
                if !text && !src {
                    let kind = kind_of(self);
                    if kind == 1 {
                        region = owner;
                    } else if kind == 2 {
                        region = self.outermost_svg(owner);
                    } else if kind == 3 && self.outermost_svg(owner).is_none() && self.push_text_owner_region(cx, owner, &mut regions) {
                        continue_region = true;
                    }
                } else if inplace && sw.text_regions {
                    region = self.outermost_svg(self.holes[hi].text_elements.first().copied());
                    words_region = region.is_none();
                }
                if continue_region {
                } else if words_region {
                    for &el in &self.holes[hi].text_elements {
                        if self.kind(el) == ElementKind::el_text {
                            regions.push(el);
                        }
                    }
                } else if let Some(region) = region {
                    regions.push(region);
                } else {
                    self.patch.patch_confined = false;
                }
                self.patch.patch_regions = regions;
            }
            if paint_only {
                if src || (text && (!inplace || resize_only)) {
                    paint_only = false;
                } else if !text && !attr_empty {
                    let owner = owner.expect("an attribute hole has its element");
                    let mut inside_svg = false;
                    let mut p = self.parent(owner);
                    while let Some(e) = p {
                        if self.kind(e) == ElementKind::el_svg {
                            inside_svg = true;
                            break;
                        }
                        p = self.parent(e);
                    }
                    paint_only = inside_svg;
                } else if !text {
                    paint_only = background || patch.is_some() || style_change_is_paint_only(&self.holes[hi].value, &value);
                }
            }
            if inplace {
                self.holes[hi].value = value;
                for w in 0..words.len() {
                    let el = self.holes[hi].text_elements[w];
                    let before = self.get_content_size(cx, el, Px(0.0));
                    measured.push((el, before));
                    if self.is_text(el) {
                        let word = words[w].clone();
                        self.set_text(cx, el, &word);
                    }
                    retext.push(el);
                }
                continue;
            }
            if keep_tree
                && (src
                    || (!text && attr_empty && !background && patch.is_none() && {
                        let owner = owner.expect("a style hole has its element");
                        let d = self.el(owner).css.m_display;
                        !tree_neutral_change(&self.holes[hi].value, &value, d == display_inline, table_part(d))
                    }))
            {
                keep_tree = false;
            }
            if background {
                let owner = owner.expect("a style hole has its element");
                if matches!(self.kind(owner), ElementKind::html_tag | ElementKind::el_div | ElementKind::el_td)
                    && self.background_color_is_local(owner)
                {
                    background_only.insert(owner);
                }
            }
            self.holes[hi].value = value.clone();
            if src {
                let owner = owner.expect("a src hole has its element");
                let mut attrs = StringMap::new();
                attrs.insert(b"src".to_vec(), value.clone());
                let mut tag = self.get_tag_name(cx.names, owner);
                let keep = cx.container.sanitize_element(&mut tag, false, &mut attrs) && attrs.contains_key(&b"src"[..]);
                let src_value = if keep { attrs[&b"src"[..]].clone() } else { Vec::new() };
                self.set_attr(cx.names, owner, b"src", &src_value);
                if self.kind(owner) == ElementKind::el_image {
                    self.update_src(owner);
                }
                restyle.push(owner);
                continue;
            }
            if !attr_empty {
                let owner = owner.expect("an attribute hole has its element");
                let attr = self.holes[hi].attr.clone();
                let mut attrs = StringMap::new();
                attrs.insert(attr.clone(), value.clone());
                let mut tag = self.get_tag_name(cx.names, owner);
                let keep = cx.container.sanitize_element(&mut tag, true, &mut attrs) && attrs.contains_key(&attr);
                let attr_value = if keep { attrs[&attr].clone() } else { Vec::new() };
                self.set_attr(cx.names, owner, &attr, &attr_value);
                restyle.push(owner);
                continue;
            }
            if !text {
                let owner = owner.expect("a style hole has its element");
                self.set_attr(cx.names, owner, b"style", &value);
                restyle.push(owner);
                if let Some(patch) = patch
                    && self.outermost_svg(Some(owner)).is_none()
                    && self.kind(owner).is_html_tag()
                {
                    patched.insert(owner, patch);
                }
                continue;
            }
            let mut created: Vec<ElementId> = Vec::new();
            let mut pieces: Vec<(Vec<u8>, bool)> = Vec::new();
            cx.container.split_text(c_str(&value), &mut |piece, space| pieces.push((piece.to_vec(), space)));
            for (piece, space) in pieces {
                created.push(self.new_text(&piece, space));
            }
            let old = self.holes[hi].text_elements.clone();
            let parent = self.parent(old[0]).expect("a text in an element");
            self.replace_children(parent, &old, &created);
            if keep_tree && !self.replace_text_renderers(cx, &old, &created) {
                keep_tree = false;
            }
            self.holes[hi].text_elements = created.clone();
            new_text.extend(created);
        }
        let mut groups: Vec<(bool, bool)> = Vec::new();
        if paint_only {
            for &el in &restyle {
                let css = &self.el(el).css;
                groups.push((css.is_paint_group(), css.m_transform.is_empty()));
            }
        }
        for (k, _) in &classes {
            if let Some(owner) = self.holes[changes[*k].0].owner {
                background_only.remove(&owner);
                patched.remove(&owner);
            }
        }
        let mut seen = BTreeSet::new();
        for &el in &restyle {
            if !seen.insert(el) {
                background_only.remove(&el);
                patched.remove(&el);
                shallow_width.remove(&el);
            }
        }
        for list in [&restyle, &new_text, &retext] {
            for &el in list {
                let mut parent = self.parent(el);
                while let Some(p) = parent {
                    background_only.remove(&p);
                    patched.remove(&p);
                    parent = self.parent(p);
                }
            }
        }
        if !allow_shallow_width || !keep_tree || !new_text.is_empty() {
            shallow_width.clear();
        }
        self.restyle_after_patch(cx, &restyle, &new_text, keep_tree, &retext, &background_only, &patched, &shallow_width);
        let mut k = 0;
        while k < measured.len() && paint_only {
            let after = self.get_content_size(cx, measured[k].0, Px(0.0));
            if after.width != measured[k].1.width || after.height != measured[k].1.height {
                paint_only = false;
            }
            k += 1;
        }
        if paint_only {
            let mut k = 0;
            while k < restyle.len() && paint_only {
                let css = &self.el(restyle[k]).css;
                if css.is_paint_group() != groups[k].0 || css.m_transform.is_empty() != groups[k].1 {
                    paint_only = false;
                }
                k += 1;
            }
        }
        self.patch.paint_only_patch = paint_only && keep_tree;
        if keep_tree && !self.patch.paint_only_patch {
            self.forget_positions(cx);
        }
        if !self.patch.paint_only_patch {
            self.patch.patch_confined = false;
            self.patch.patch_regions.clear();
        }
        1
    }

    /// `restyle_after_patch(restyle, new_text, keep_tree, retext, background_only, patched, shallow_width)`: the
    /// changed elements restyled (each not under another being restyled: a property patch in place, a background
    /// alone, a shallow width without its children, else fully), the positioned boxes kept when no position
    /// changed, then the render tree kept or made again.
    #[allow(clippy::too_many_arguments)]
    fn restyle_after_patch(
        &mut self,
        cx: &StyleCx,
        restyle: &[ElementId],
        new_text: &[ElementId],
        keep_tree: bool,
        retext: &[ElementId],
        background_only: &BTreeSet<ElementId>,
        patched: &BTreeMap<ElementId, &PropertyPatch>,
        shallow_width: &BTreeSet<ElementId>,
    ) {
        self.invalidate_paint();
        self.share_cache.clear();
        let pending: BTreeSet<ElementId> = restyle.iter().chain(new_text).chain(retext).copied().collect();
        let covered = |doc: &Document, el: ElementId| -> bool {
            let mut p = doc.parent(el);
            while let Some(e) = p {
                if pending.contains(&e) && !shallow_width.contains(&e) {
                    return true;
                }
                p = doc.parent(e);
            }
            false
        };
        let mut fast: BTreeMap<ElementId, &PropertyPatch> = BTreeMap::new();
        for &el in restyle {
            match patched.get(&el) {
                Some(&patch) if !covered(self, el) => {
                    fast.insert(el, patch);
                }
                _ => {
                    if self.kind(el).is_html_tag() {
                        self.restore_base_style(el);
                    }
                }
            }
        }
        if !keep_tree || !new_text.is_empty() {
            self.patch.layout_before_valid = false;
        }
        let positioned_fresh = keep_tree
            && new_text.is_empty()
            && self.layout.positioned_epoch == self.layout.style_epoch
            && self.layout.positioned_tree == self.render.serial;
        let mut positions: Vec<(ElementId, i32)> = Vec::new();
        let mut roots: Vec<ElementId> = Vec::new();
        for list in [restyle, new_text, retext] {
            for &el in list {
                if !covered(self, el) {
                    roots.push(el);
                }
            }
        }
        if positioned_fresh {
            for &root in &roots {
                if shallow_width.contains(&root) {
                    positions.push((root, self.el(root).css.m_el_position));
                    continue;
                }
                let mut stack = vec![root];
                while let Some(el) = stack.pop() {
                    positions.push((el, self.el(el).css.m_el_position));
                    stack.extend(self.children(el).iter().copied());
                }
            }
        }
        for &el in &roots {
            let patch = fast.get(&el).copied();
            let local = patch.is_some() || background_only.contains(&el);
            let tag = local && self.kind(el).is_html_tag();
            if tag && let Some(patch) = patch {
                if !self.patch_inline_properties(cx, el, &patch.names, &patch.text) {
                    self.restore_base_style(el);
                    self.compute_styles(cx, el, true);
                }
            } else if tag {
                self.compute_background_style(cx, el);
            } else {
                let recursive = !shallow_width.contains(&el);
                self.compute_styles(cx, el, recursive);
            }
        }
        if positioned_fresh {
            let same = positions.iter().all(|&(el, position)| self.el(el).css.m_el_position == position);
            if same {
                self.layout.positioned_epoch = self.layout.style_epoch;
            }
        }
        if keep_tree && self.render.root.is_some() {
            return;
        }
        self.recreate_render_tree_after_patch(cx);
    }

    /// `update_structure(html, changed)`: the new text parsed and its tree diffed against the kept one: elements of
    /// the same kind kept (their changed `style`, attributes as a retag, texts in place), children lists that differ
    /// made again with the kept elements in their places (by order, or by `data-lh-id`); then the new elements
    /// matched and the changed restyled. False, the document as it was (but for the epochs and the patch's layout
    /// state), when the parse differs in mode or limits, selectors read counters or `:empty`, or a change reaches the
    /// document's own elements, a select, or attributes selectors read.
    fn update_structure(&mut self, cx: &StyleCx, html: &[u8], changed: &mut bool) -> bool {
        self.budget_enter();
        let ret = self.update_structure_scoped(cx, html, changed);
        self.budget_exit();
        ret
    }

    fn update_structure_scoped(&mut self, cx: &StyleCx, html: &[u8], changed: &mut bool) -> bool {
        self.patch.match_epoch += 1;
        self.begin_layout_patch(cx);
        self.invalidate_paint();
        self.note_style_change();
        if self.gumbo.is_none() || self.root.is_none() || !self.finalized || self.build_limited || !cx.switches.patch.structure {
            return false;
        }
        let scope = self.selector_scope(cx);
        if (scope & 4) != 0 || mentions_counters(html) {
            return false;
        }
        let attribute_scope = (scope & 1) != 0;
        let position_scope = (scope & 2) != 0;

        let mut text = html.to_vec();
        let mut output = gumbo_parse_with_options(&kGumboDefaultOptions, &text);
        let old_mode = self.gumbo.as_ref().map(|g| g.node(g.document).document().doc_type_quirks_mode);
        if output.truncated || Some(output.node(output.document).document().doc_type_quirks_mode) != old_mode {
            return false;
        }
        let (total, deepest) = count_elements(&output, output.document);
        if total > max_build_elements || deepest >= max_build_depth {
            return false;
        }

        let old = self.gumbo.take().expect("an updatable document keeps its tree");
        let diffed = {
            let mut diff = Diff {
                doc: self,
                cx,
                old: &old,
                old_text: &self.text,
                new: &output,
                new_text: &text,
                attribute_scope,
                position_scope,
                out: DiffOut::default(),
            };
            let ok = diff.diff_children(old.document, output.document, None, false);
            ok.then_some(diff.out)
        };
        let Some(diff) = diffed else {
            self.gumbo = Some(old);
            return false;
        };
        let DiffOut { styles, retags, rebuilds, kept, kept_text, retexts } = diff;

        let chain = |doc: &Document, el: Option<ElementId>| -> Vec<ElementId> {
            let mut out = Vec::new();
            let mut cur = el;
            while let Some(e) = cur {
                out.push(e);
                cur = doc.parent(e);
            }
            out
        };
        let over_chain = chain(self, self.patch.over_element);
        let active_chain = chain(self, self.patch.active_element);
        // The C++'s vectors of shared_ptrs keep the chains' elements (anonymous ones the tree lets go among them) until
        // it returns.
        for &el in over_chain.iter().chain(&active_chain) {
            self.hold_anonymous(Some(el));
        }

        let mut remapped: BTreeMap<NodeId, Vec<ElementId>> = BTreeMap::new();
        for (node, el) in kept {
            remapped.insert(node, vec![el]);
        }
        for (node, els) in kept_text {
            remapped.insert(node, els);
        }
        // The C++ keeps the old map until it returns; nothing reads it, and the elements it held live in the arena.
        let _old_map = std::mem::replace(&mut self.node_elements, remapped);

        let mut restyle: Vec<ElementId> = Vec::new();
        for (el, style) in &styles {
            self.set_attr(cx.names, *el, b"style", style);
            restyle.push(*el);
        }
        let mut retext: Vec<ElementId> = Vec::new();
        for (els, words) in &retexts {
            for w in 0..words.len() {
                if self.is_text(els[w]) {
                    self.set_text(cx, els[w], &words[w]);
                }
                retext.push(els[w]);
            }
        }
        let mut keep_tree = cx.switches.patch.keep_tree && rebuilds.is_empty() && styles.is_empty();
        let mut fresh: Vec<ElementId> = Vec::new();
        let mut created: Vec<ElementId> = Vec::new();
        // create_node reads the new text: the C++ swaps it in at the end, and nothing reads the old one after the diff.
        std::mem::swap(&mut self.text, &mut text);
        self.track_nodes = true;
        self.build_elements = 0;
        let env = cx.env();
        for plan in &rebuilds {
            let parent = plan.parent;
            let mut depth = 0;
            let mut p = self.parent(parent);
            while let Some(e) = p {
                depth += 1;
                p = self.parent(e);
            }
            self.build_depth = depth + 1;
            let kids = self.children(parent).to_vec();
            let before = kids.first().copied().filter(|&c| self.tag(c) == __tag_before_);
            let after = kids.last().copied().filter(|&c| self.tag(c) == __tag_after_);
            let mut reused = BTreeSet::new();
            let mut children: Vec<ElementId> = Vec::new();
            children.extend(before);
            let nodes = node_children(&output, plan.node).to_vec();
            for (i, &node) in nodes.iter().enumerate() {
                if plan.create[i] == 0 {
                    for &el in &plan.keep[i] {
                        reused.insert(el);
                        children.push(el);
                    }
                    continue;
                }
                let mut made = Vec::new();
                self.create_node(&env, &mut output, node, &mut made, true, true);
                for el in made {
                    children.push(el);
                    if self.kind(el).is_html_tag() {
                        fresh.push(el);
                    }
                    created.push(el);
                }
            }
            children.extend(after);
            for &old_child in &kids {
                if Some(old_child) != before && Some(old_child) != after && !reused.contains(&old_child) {
                    self.el_mut(old_child).parent = None;
                }
            }
            for &el in &children {
                self.el_mut(el).parent = Some(parent);
            }
            self.el_mut(parent).children = children;
        }
        self.track_nodes = false;
        self.build_depth = 0;

        for (el, attrs) in &retags {
            keep_tree = self.apply_retag(cx, *el, attrs, keep_tree, None) && keep_tree;
            restyle.push(*el);
        }

        let any = !styles.is_empty() || !rebuilds.is_empty() || !retags.is_empty() || !retexts.is_empty();
        if any {
            let master = self.master_css.selectors().to_vec();
            for &el in &fresh {
                self.apply_stylesheet(cx, el, &master);
            }
            for &el in &fresh {
                self.parse_attributes(&env, el);
            }
            self.flush_declarations(cx);
            let sheet = self.styles.selectors().to_vec();
            for &el in &fresh {
                self.apply_stylesheet(cx, el, &sheet);
            }
            let user = self.user_css.selectors().to_vec();
            for &el in &fresh {
                self.apply_stylesheet(cx, el, &user);
            }
            self.restyle_after_patch(cx, &restyle, &created, keep_tree, &retext, &BTreeSet::new(), &BTreeMap::new(), &BTreeSet::new());
            if keep_tree {
                self.forget_positions(cx);
            }
            let nearest_attached = |doc: &Document, path: &[ElementId]| -> Option<ElementId> {
                for &candidate in path {
                    let mut el = Some(candidate);
                    while let Some(e) = el {
                        if Some(e) == doc.root {
                            break;
                        }
                        el = doc.parent(e);
                    }
                    if el == doc.root {
                        return Some(candidate);
                    }
                }
                None
            };
            let over = nearest_attached(self, &over_chain);
            self.set_over_element(over);
            let active = nearest_attached(self, &active_chain);
            self.set_active_element(active);
        }

        drop(old);
        self.gumbo = Some(output);
        self.build_template();
        *changed = any;
        for &el in active_chain.iter().chain(&over_chain) {
            self.release_anonymous(el);
        }
        true
    }
}

// ---- document.cpp: the layout cache's patch state (stage L18) -------------------------------------------------

impl Document {
    /// `walk_boxes(root, 0, 0, false, visit)`: every item in tree order with its parent's corner in document
    /// coordinates and whether a positioned box, paint group, transform or scroller is at or above it.
    fn walk_boxes(&self, root: RenderId, visit: &mut dyn FnMut(&Document, RenderId, f32, f32, bool)) {
        let mut pending = vec![(root, 0.0f32, 0.0f32, false)];
        while let Some((item, x, y, mut unsafe_)) = pending.pop() {
            let it = self.render.item(item);
            let css = &self.el(it.element).css;
            unsafe_ = unsafe_
                || css.m_el_position == element_position_absolute
                || css.m_el_position == element_position_fixed
                || css.is_paint_group();
            let pos = it.pos;
            visit(self, item, x, y, unsafe_);
            unsafe_ = unsafe_ || !css.m_transform.is_empty() || css.m_overflow == overflow_scroll || css.m_overflow == overflow_auto;
            let cx = x + pos.x.value();
            let cy = y + pos.y.value();
            for &child in it.children.iter().rev() {
                pending.push((child, cx, cy, unsafe_));
            }
        }
    }

    /// `snapshot_layout()`: every item's box in document coordinates, before a patch.
    pub fn snapshot_layout(&mut self) {
        let mut before = std::mem::take(&mut self.patch.layout_before);
        before.clear();
        if let Some(root) = self.render.root {
            self.walk_boxes(root, &mut |doc, item, x, y, _| {
                let mut b = doc.render.item(item).pos;
                b.x += Px(x);
                b.y += Px(y);
                before.push((item, b));
            });
        }
        self.patch.layout_before = before;
        self.patch.layout_before_valid = true;
    }

    /// `paints_inside_box(item)`: what the item paints stays inside its border box (a text without its owner's
    /// shadows, a box without shadows, outline, list marker or an inline's own background and borders, not an SVG).
    fn paints_inside_box(&self, item: RenderId) -> bool {
        let el = self.render.item(item).element;
        let css = &self.el(el).css;
        if self.is_text(el) {
            return self.parent(el).is_some_and(|p| self.el(p).css.m_text_shadows.is_empty());
        }
        if css.m_display == display_list_item
            || !css.m_box_shadows.is_empty()
            || !css.m_text_shadows.is_empty()
            || (css.m_outline.style != BorderStyle::border_style_none.index()
                && css.m_outline.style != BorderStyle::border_style_hidden.index())
        {
            return false;
        }
        if self.is_inline(el) && !self.is_inline_box(el) {
            let b = &css.m_css_borders;
            let visible = Px(b.left.width.val()) != Px(0.0)
                || Px(b.right.width.val()) != Px(0.0)
                || Px(b.top.width.val()) != Px(0.0)
                || Px(b.bottom.width.val()) != Px(0.0);
            if self.background_shown(el) || visible {
                return false;
            }
        }
        self.kind(el) != ElementKind::el_svg
    }

    /// `get_background() != nullptr` of an element (`html_tag::get_background(false)`): its background unless empty
    /// (the root's own empty one gives its body's), and a body's only when its parent has one of its own.
    fn background_shown(&self, el: ElementId) -> bool {
        if !self.kind(el).is_html_tag() {
            return false;
        }
        let own = |e: ElementId| self.kind(e).is_html_tag() && !background_is_empty(&self.el(e).css.m_bg);
        if background_is_empty(&self.el(el).css.m_bg) {
            if self.parent(el).is_none() {
                for &child in self.children(el) {
                    if self.is_body(child) {
                        return own(child);
                    }
                }
            }
            return false;
        }
        if self.is_body(el)
            && let Some(parent) = self.parent(el)
            && !own(parent)
        {
            return false;
        }
        true
    }

    /// `layout_dirty_regions(out)`: after a patch's relayout, the boxes (before and after, grown by a pixel, a text
    /// by half its height) of the items that moved or whose layout the patch marked, when every such item paints
    /// inside its box outside any positioned box, group, transform or scroller, at most 128 boxes covering at most
    /// half the document; false otherwise, with what `out` held by then (the C++'s out parameter).
    pub fn layout_dirty_regions(&self, out: &mut Vec<Position>) -> bool {
        out.clear();
        let Some(root_render) = self.render.root else { return false };
        if !self.patch.layout_before_valid || !self.layout.after_render {
            return false;
        }
        if !self.patch.restyle_confined {
            return false;
        }
        let mut origins = BTreeSet::new();
        for &el in self.patch.layout_origins.iter().chain(&self.patch.restyle_regions) {
            let mut owner = Some(el);
            while let Some(o) = owner {
                if self.get_render_item(o).is_some() {
                    break;
                }
                owner = self.parent(o);
            }
            let Some(owner) = owner else { return false };
            origins.insert(owner);
        }
        let mut index = 0;
        let mut ok = true;
        let mut area = 0.0f64;
        let add = |doc: &Document, item: RenderId, mut b: Position, out: &mut Vec<Position>, area: &mut f64| {
            let it = doc.render.item(item);
            b.grow(&it.padding);
            b.grow(&it.borders);
            let mut slack = Px(1.0);
            if doc.is_text(it.element) {
                slack = Px(4.0).max(Px((b.height.value() / 2.0).ceil() + 1.0)) + Px(1.0);
            }
            b.x -= slack;
            b.y -= slack;
            b.width += slack * 2;
            b.height += slack * 2;
            if b.width <= Px(0.0) || b.height <= Px(0.0) {
                return;
            }
            *area += f64::from(b.width.value()) * f64::from(b.height.value());
            out.push(b);
        };
        self.walk_boxes(root_render, &mut |doc, item, x, y, unsafe_| {
            if !ok {
                return;
            }
            if index >= doc.patch.layout_before.len() || doc.patch.layout_before[index].0 != item {
                ok = false;
                return;
            }
            let before = doc.patch.layout_before[index].1;
            index += 1;
            let mut b = doc.render.item(item).pos;
            b.x += Px(x);
            b.y += Px(y);
            if before.same(&b) && !origins.contains(&doc.render.item(item).element) {
                return;
            }
            if unsafe_ || !doc.paints_inside_box(item) {
                ok = false;
                return;
            }
            add(doc, item, before, out, &mut area);
            add(doc, item, b, out, &mut area);
            if out.len() > 128 {
                ok = false;
            }
        });
        if !ok || index != self.patch.layout_before.len() {
            return false;
        }
        let root = self.render.item(root_render).pos;
        area <= 0.5 * crate::cxx::max_f64(1.0, f64::from(root.width.value()) * f64::from(root.height.value()))
    }

    /// The `::before` and `::after` boxes the C++ would destroy now give their generated content back: those neither
    /// the tree, a live render item, the Gumbo map, the holes, the patch's regions nor the hovered or active element
    /// hold (with what those hold below them).
    pub fn release_dead_generated(&mut self) {
        let mut live = vec![false; self.elements.len()];
        let mut pending: Vec<ElementId> = Vec::new();
        pending.extend(self.root);
        pending.extend(self.render.live_elements());
        pending.extend(self.node_elements.values().flatten().copied());
        for hole in &self.holes {
            pending.extend(hole.owner);
            pending.extend(hole.text_elements.iter().copied());
        }
        pending.extend(self.patch.patch_regions.iter().copied());
        pending.extend(self.patch.restyle_regions.iter().copied());
        pending.extend(self.patch.over_element);
        pending.extend(self.patch.active_element);
        while let Some(el) = pending.pop() {
            if std::mem::replace(&mut live[el.0 as usize], true) {
                continue;
            }
            pending.extend(self.el(el).children.iter().copied());
        }
        for index in 0..self.elements.len() {
            if live[index] {
                continue;
            }
            if let ElementState::Generated { generations, boxes, bytes } = self.elements[index].state
                && (boxes != 0 || bytes != 0)
            {
                self.release_generated(boxes, bytes);
                self.elements[index].state = ElementState::Generated { generations, boxes: 0, bytes: 0 };
            }
        }
    }
}
