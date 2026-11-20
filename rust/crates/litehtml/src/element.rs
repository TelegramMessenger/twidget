//! `element.cpp`, and the element classes as a document is built: `element.h`'s tree of `shared_ptr`s is an
//! arena here, [`crate::document::Document::elements`], each element an [`Element`] linked by [`ElementId`], its
//! class an [`ElementKind`]. The classes whose files hold nothing but construction and a flag (`el_body`,
//! `el_break`, `el_cdata`, `el_comment`, `el_space`, `el_text`) live here, as do the classes' `appendChild`s
//! (`el_script`'s, `el_style`'s, `el_table`'s) and the `parse_attributes` dispatch; the others have a module each.
//! What a class does in layout and painting (`el_text`'s measuring, `el_image`'s drawing) is later stages'.
//!
//! The cascade's state of an element is here too: `m_style` and `m_style_base`, the selectors it matched
//! (`m_used_styles`), its counters (`element.cpp`'s counter functions) and its computed `m_css`, with what
//! `el_text`, the form controls and `el_image` keep of their computed style. An element records the declarations
//! its attributes add ([`Element::declarations`]) as `parse_attributes` makes them, and the cascade adds them to its
//! style ([`Document::flush_declarations`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::Px;
use crate::css_properties::CssProperties;
use crate::css_selector::CssSelector;
use crate::css_tokenizer::CssToken;
use crate::css_values::StyleDisplay;
use crate::document::{Document, Env};
use crate::html::{is_number, join_string, trim};
use crate::string_id::*;
use crate::style::Style;
use crate::web_color::WebColor;
use crate::{Names, StringId};

/// `string_map`: names to values, in byte order.
pub type StringMap = BTreeMap<Vec<u8>, Vec<u8>>;

/// An element: its index in [`Document::elements`], what an `element::ptr` is in the C++.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(pub u32);

/// The element classes. `html_tag` and every class made from it come first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ElementKind {
    html_tag,
    el_anchor,
    el_base,
    el_body,
    el_break,
    el_div,
    el_font,
    el_image,
    el_link,
    el_para,
    el_table,
    el_td,
    el_title,
    el_tr,
    el_svg,
    el_input,
    el_select,
    el_button,
    el_textarea,
    el_fieldset,
    el_meter,
    el_before,
    el_after,
    el_script,
    el_style,
    el_text,
    el_space,
    el_comment,
    el_cdata,
}

impl ElementKind {
    /// Whether the class is `html_tag` or made from it (`dynamic_cast<html_tag*>` succeeds).
    pub fn is_html_tag(self) -> bool {
        (self as u8) <= ElementKind::el_after as u8
    }
}

/// A value `parse_attributes` adds to `m_style`: `add_property(name, text, …)` or `add_property(name, tokens, …)`.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    Text(Vec<u8>),
    Tokens(Vec<CssToken>),
}

/// One `m_style.add_property(name, value, baseurl, important, container)` of `parse_attributes`, in the order the
/// C++ makes them; `with_container` is whether the document's container was passed.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeDeclaration {
    pub name: StringId,
    pub value: AttributeValue,
    pub baseurl: Vec<u8>,
    pub important: bool,
    pub with_container: bool,
}

/// What a class keeps of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ElementState {
    #[default]
    None,
    /// `el_image`: `m_src` and `m_loaded`.
    Image { src: Vec<u8>, loaded: bool },
    /// `el_input`: `m_type`, `m_checked`, `m_fraction` (a range's place) and `m_swatch` (a colour's).
    Input { ty: Vec<u8>, checked: bool, fraction: f32, swatch: WebColor },
    /// `el_before_after_base`: how many rules gave it content, and the boxes and bytes it took of the document's.
    Generated { generations: i32, boxes: i32, bytes: usize },
    /// `el_svg`: what its paints keep (stage L14).
    Svg(crate::el_svg::SvgPaintState),
}

/// `used_selector`: a selector that matched the element when a sheet was applied, and whether its style is in the
/// element's (a pseudo-class may not hold now; a `::before` or `::after` one styles that box).
#[derive(Clone, Debug)]
pub struct UsedSelector {
    pub selector: Arc<CssSelector>,
    pub used: bool,
}

/// What `el_text` keeps of its computed style and measure.
#[derive(Clone, Debug, PartialEq)]
pub struct TextState {
    /// `m_transformed_text` and `m_use_transformed`.
    pub transformed_text: Vec<u8>,
    pub use_transformed: bool,
    /// `m_draw_spaces`.
    pub draw_spaces: bool,
    /// `m_size`: width and height.
    pub size: (Px, Px),
    /// `m_measured_font` and `m_measured_transform`: what `m_size.width` was measured in.
    pub measured_font: u64,
    pub measured_transform: i32,
    /// `m_breaker`: the container's handle of the text's breaker, 0 for none.
    pub breaker: u64,
}

impl Default for TextState {
    fn default() -> TextState {
        TextState {
            transformed_text: Vec::new(),
            use_transformed: false,
            draw_spaces: true,
            size: (Px(0.0), Px(0.0)),
            measured_font: 0,
            measured_transform: -1,
            breaker: 0,
        }
    }
}

/// `form_look`: what a form control's computed style decides about how it is drawn, and what its last paint asked
/// the container (whether controls are active, the accent colour).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FormLook {
    pub dark: bool,
    pub disabled: bool,
    pub native: bool,
    pub active: bool,
    pub accent: WebColor,
}

/// An element of any class: what `element` holds, what `html_tag` adds, and the class's own.
#[derive(Clone, Debug)]
pub struct Element {
    pub kind: ElementKind,
    /// `m_parent`: set by `appendChild` of a parent that keeps the child.
    pub parent: Option<ElementId>,
    /// `m_children`.
    pub children: Vec<ElementId>,
    /// `html_tag::m_tag` (`empty_id` until `set_tagName`).
    pub tag: StringId,
    /// `m_id`.
    pub id: StringId,
    /// `m_str_classes`: the `class` attribute's words, lower-case in quirks mode.
    pub str_classes: Vec<Vec<u8>>,
    /// `m_classes`: the same as names.
    pub classes: Vec<StringId>,
    /// `m_attrs`: every attribute, its name lower-case, its value as written.
    pub attrs: StringMap,
    /// `m_pseudo_classes`, in the order they were set.
    pub pseudo_classes: Vec<StringId>,
    /// What `parse_attributes` added to `m_style`, for the cascade to add in this order.
    pub declarations: Vec<AttributeDeclaration>,
    /// `apply_stylesheet(master_css())` was asked of this element when its parent's attributes were parsed: a
    /// form control's generated box. The cascade (L5) applies the master sheet to it then.
    pub master_sheet: bool,
    /// `m_text` of `el_text`, `el_space`, `el_comment`, `el_cdata` and `el_script`.
    pub text: Vec<u8>,
    /// `el_style`'s own `m_children`, which hides `element::m_children`: its text, with no parent set.
    pub own_children: Vec<ElementId>,
    pub state: ElementState,
    /// How many of `declarations` the style has.
    pub declarations_applied: usize,
    /// The master sheet was applied as `master_sheet` asks.
    pub master_applied: bool,
    /// `m_css`: the computed style; before the cascade, what the class's constructor sets (its display).
    pub css: CssProperties,
    /// `m_style`, `m_style_base` and `m_has_style_base`: the cascaded declarations, and what they were before the
    /// `style` attribute was added.
    pub style: Style,
    pub style_base: Style,
    pub has_style_base: bool,
    /// `m_counter_changes`: how many times `counter-increment` changed the counters.
    pub counter_changes: i32,
    /// `m_anonymous_style`.
    pub anonymous_style: bool,
    /// `m_used_styles`.
    pub used_styles: Vec<UsedSelector>,
    /// `m_counter_values`.
    pub counter_values: BTreeMap<StringId, i32>,
    /// `el_text`'s computed state.
    pub text_state: TextState,
    /// A form control's `m_look`, and `el_select`'s `m_arrow`.
    pub look: FormLook,
    pub arrow: bool,
    /// `m_renders`: the element's render items, weak references in the order `add_render` added them.
    pub renders: Vec<crate::render_item::RenderId>,
    /// An anonymous element's owners: its render items, and the anonymous elements whose parent it is.
    pub box_holds: u32,
    /// `html_tag`'s `m_match_memos` and `m_flips` (stage L19).
    pub patch: crate::patch::ElementPatch,
}

impl Element {
    pub(crate) fn new(kind: ElementKind) -> Element {
        Element {
            kind,
            parent: None,
            children: Vec::new(),
            tag: empty_id,
            id: empty_id,
            str_classes: Vec::new(),
            classes: Vec::new(),
            attrs: StringMap::new(),
            pseudo_classes: Vec::new(),
            declarations: Vec::new(),
            master_sheet: false,
            text: Vec::new(),
            own_children: Vec::new(),
            state: ElementState::None,
            declarations_applied: 0,
            master_applied: false,
            css: CssProperties::default(),
            style: Style::default(),
            style_base: Style::default(),
            has_style_base: false,
            counter_changes: 0,
            anonymous_style: false,
            used_styles: Vec::new(),
            counter_values: BTreeMap::new(),
            text_state: TextState::default(),
            look: FormLook::default(),
            arrow: false,
            renders: Vec::new(),
            box_holds: 0,
            patch: crate::patch::ElementPatch::default(),
        }
    }
}

impl Document {
    /// `std::make_shared<kind>(doc)`: a new element, its constructor's effects included (`el_text`'s display,
    /// `el_image`'s and `el_svg`'s, `el_svg` marking the document, a generated box's tag).
    pub fn new_element(&mut self, kind: ElementKind) -> ElementId {
        let mut el = Element::new(kind);
        match kind {
            ElementKind::el_text | ElementKind::el_space => el.css.m_display = StyleDisplay::display_inline_text.index(),
            ElementKind::el_image => {
                el.css.m_display = StyleDisplay::display_inline_block.index();
                el.state = ElementState::Image { src: Vec::new(), loaded: false };
            }
            ElementKind::el_svg => {
                self.has_svg = true;
                el.css.m_display = StyleDisplay::display_inline_block.index();
                el.state = ElementState::Svg(crate::el_svg::SvgPaintState::default());
            }
            ElementKind::el_input => {
                el.state = ElementState::Input { ty: Vec::new(), checked: false, fraction: 0.5, swatch: WebColor::BLACK }
            }
            ElementKind::el_before | ElementKind::el_after => {
                el.tag = if kind == ElementKind::el_before { __tag_before_ } else { __tag_after_ };
                el.state = ElementState::Generated { generations: 0, boxes: 0, bytes: 0 };
            }
            _ => {}
        }
        let id = ElementId(self.elements.len() as u32);
        self.elements.push(el);
        id
    }

    /// `el_text(text, doc)` or `el_space(text, doc)`: the text up to its first NUL, as a `const char*` gives it.
    pub fn new_text(&mut self, text: &[u8], space: bool) -> ElementId {
        let id = self.new_element(if space { ElementKind::el_space } else { ElementKind::el_text });
        self.elements[id.0 as usize].text = crate::gumbo::c_str(text).to_vec();
        id
    }

    pub fn el(&self, id: ElementId) -> &Element {
        &self.elements[id.0 as usize]
    }

    pub fn el_mut(&mut self, id: ElementId) -> &mut Element {
        &mut self.elements[id.0 as usize]
    }

    pub fn kind(&self, id: ElementId) -> ElementKind {
        self.el(id).kind
    }

    /// `parent()`.
    pub fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.el(id).parent
    }

    /// `children()`: `element::m_children`, which an `el_style` leaves empty.
    pub fn children(&self, id: ElementId) -> &[ElementId] {
        &self.el(id).children
    }

    /// `tag()`: an `html_tag`'s `m_tag`, `script` and `style` for those classes, `empty_id` for the others.
    pub fn tag(&self, id: ElementId) -> StringId {
        let el = self.el(id);
        match el.kind {
            ElementKind::el_script => _script_,
            ElementKind::el_style => _style_,
            kind if kind.is_html_tag() => el.tag,
            _ => empty_id,
        }
    }

    /// `get_tagName()`: an `html_tag`'s tag name, `script` and `style` for those classes, `""` for the others.
    pub fn get_tag_name(&self, names: &Names, id: ElementId) -> Vec<u8> {
        let el = self.el(id);
        match el.kind {
            ElementKind::el_script => b"script".to_vec(),
            ElementKind::el_style => b"style".to_vec(),
            kind if kind.is_html_tag() => names.s(el.tag).to_vec(),
            _ => Vec::new(),
        }
    }

    /// `is_text()`: `el_text` and `el_space`.
    pub fn is_text(&self, id: ElementId) -> bool {
        matches!(self.kind(id), ElementKind::el_text | ElementKind::el_space)
    }

    /// `is_space()`.
    pub fn is_space(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_space
    }

    /// `is_comment()`.
    pub fn is_comment(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_comment
    }

    /// `is_body()`.
    pub fn is_body(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_body
    }

    /// `is_break()`: an `el_break`, and an `el_space` of a newline in a `pre`, `pre-line` or `pre-wrap` white-space.
    pub fn is_break(&self, id: ElementId) -> bool {
        match self.kind(id) {
            ElementKind::el_break => true,
            ElementKind::el_space => self.el_space_is_break(id),
            _ => false,
        }
    }

    /// `is_white_space()`: an `el_space` in a `normal`, `nowrap` or `pre-line` white-space.
    pub fn is_white_space(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_space && self.el_space_is_white_space(id)
    }

    /// `is_replaced()`: `el_image` and `el_svg`.
    pub fn is_replaced(&self, id: ElementId) -> bool {
        matches!(self.kind(id), ElementKind::el_image | ElementKind::el_svg)
    }

    /// `get_text(text)`: text, spaces, comments and CDATA give their own; an `html_tag` its children's; a script
    /// and a style nothing.
    pub fn get_text(&self, id: ElementId, text: &mut Vec<u8>) {
        let el = self.el(id);
        match el.kind {
            ElementKind::el_text | ElementKind::el_space | ElementKind::el_comment | ElementKind::el_cdata => {
                text.extend_from_slice(&el.text)
            }
            ElementKind::el_script | ElementKind::el_style => {}
            _ => {
                for &child in &el.children {
                    self.get_text(child, text);
                }
            }
        }
    }

    /// `set_data(data)`: `el_comment` and `el_cdata` add the text up to its first NUL; others ignore it.
    pub fn set_data(&mut self, id: ElementId, data: &[u8]) {
        if matches!(self.kind(id), ElementKind::el_comment | ElementKind::el_cdata) {
            self.el_mut(id).text.extend_from_slice(crate::gumbo::c_str(data));
        }
    }

    /// `appendChild(el)`, as the parent's class has it: an `html_tag` keeps any child; an `el_table` only
    /// `tbody`, `thead`, `tfoot` and `caption`; an `el_script` takes the child's text and drops it; an `el_style`
    /// keeps text in its own list without making itself the parent; text, comments and CDATA keep nothing.
    pub fn append_child(&mut self, parent: ElementId, el: ElementId) -> bool {
        match self.kind(parent) {
            ElementKind::el_table => {
                let tag = self.tag(el);
                if tag == _tbody_ || tag == _thead_ || tag == _tfoot_ || tag == _caption_ {
                    return self.html_tag_append_child(parent, el);
                }
                false
            }
            ElementKind::el_script => {
                let mut text = std::mem::take(&mut self.el_mut(parent).text);
                self.get_text(el, &mut text);
                self.el_mut(parent).text = text;
                true
            }
            ElementKind::el_style => {
                if self.is_text(el) {
                    self.el_mut(parent).own_children.push(el);
                    return true;
                }
                false
            }
            kind if kind.is_html_tag() => self.html_tag_append_child(parent, el),
            _ => false,
        }
    }

    /// `removeChild(el)`: an `html_tag` lets go of a child it is the parent of.
    pub fn remove_child(&mut self, parent: ElementId, el: ElementId) -> bool {
        if !self.kind(parent).is_html_tag() {
            return false;
        }
        if self.parent(el) == Some(parent) {
            self.el_mut(el).parent = None;
            self.el_mut(parent).children.retain(|&c| c != el);
            return true;
        }
        false
    }

    /// `_add_before_after(type, style)`: a new `::before` (`type` 0, the first child) or `::after` (the last).
    pub fn add_before_after(&mut self, id: ElementId, ty: i32) -> ElementId {
        let el = if ty == 0 {
            let el = self.new_element(ElementKind::el_before);
            self.el_mut(id).children.insert(0, el);
            el
        } else {
            let el = self.new_element(ElementKind::el_after);
            self.el_mut(id).children.push(el);
            el
        };
        self.el_mut(el).parent = Some(id);
        el
    }

    /// `parse_attributes()`, as each class has it; `finalize_from_external_root` asks it of the root after the
    /// master sheet.
    pub fn parse_attributes(&mut self, env: &Env, id: ElementId) {
        match self.kind(id) {
            ElementKind::el_base => self.el_base_parse_attributes(env, id),
            ElementKind::el_div => self.el_div_parse_attributes(env, id),
            ElementKind::el_para => self.el_para_parse_attributes(env, id),
            ElementKind::el_font => self.el_font_parse_attributes(env, id),
            ElementKind::el_image => self.el_image_parse_attributes(env, id),
            ElementKind::el_link => self.el_link_parse_attributes(env, id),
            ElementKind::el_table => self.el_table_parse_attributes(env, id),
            ElementKind::el_td => self.el_td_parse_attributes(env, id),
            ElementKind::el_tr => self.el_tr_parse_attributes(env, id),
            ElementKind::el_title => self.el_title_parse_attributes(env, id),
            ElementKind::el_svg => self.el_svg_parse_attributes(env, id),
            ElementKind::el_input => self.el_input_parse_attributes(env, id),
            ElementKind::el_select => self.el_select_parse_attributes(env, id),
            ElementKind::el_textarea => self.el_textarea_parse_attributes(env, id),
            ElementKind::el_style => self.el_style_parse_attributes(id),
            ElementKind::el_script | ElementKind::el_text | ElementKind::el_space | ElementKind::el_comment | ElementKind::el_cdata => {}
            _ => self.html_tag_parse_attributes(env, id),
        }
    }
}

// ---- Counters (element.cpp) -----------------------------------------------------------------------------

impl Document {
    /// `find_counter(counter_name_id)`: the element holding the counter: the element, else each previous sibling
    /// from the nearest, then the same from the parent up. An element its parent does not list has every child
    /// of the parent before it.
    pub fn find_counter(&self, id: ElementId, name: StringId) -> Option<ElementId> {
        let mut current = Some(id);
        while let Some(cur) = current {
            if self.el(cur).counter_values.contains_key(&name) {
                return Some(cur);
            }
            // on each level, search previous siblings too
            let el_parent = self.parent(cur);
            if let Some(parent) = el_parent {
                let siblings = self.children(parent);
                let at = siblings.iter().position(|&s| s == cur).unwrap_or(siblings.len());
                for &sibling in siblings[..at].iter().rev() {
                    if self.el(sibling).counter_values.contains_key(&name) {
                        return Some(sibling);
                    }
                }
            }
            current = el_parent;
        }
        None
    }

    /// `get_counter_value(counter_name)`: the counter's value, "0" when there is none.
    pub fn get_counter_value(&self, names: &Names, id: ElementId, counter_name: &[u8]) -> Vec<u8> {
        match self.find_counter(id, names.id(counter_name)) {
            Some(holder) => self.el(holder).counter_values[&names.id(counter_name)].to_string().into_bytes(),
            None => b"0".to_vec(),
        }
    }

    /// `get_counters_value(parameters)`: a name and a separator: the counter's values from the outermost element
    /// that has it to this one, joined; "0" (and the counter made here) when none has it.
    pub fn get_counters_value(&mut self, names: &Names, id: ElementId, parameters: &[Vec<u8>]) -> Vec<u8> {
        let mut result = Vec::new();
        if parameters.len() >= 2 {
            let counter_name_id = names.id(&parameters[0]);
            let delims = trim(&parameters[1], b"\"'");

            let mut values = Vec::new();
            let mut current = Some(id);
            while let Some(cur) = current {
                if let Some(value) = self.el(cur).counter_values.get(&counter_name_id) {
                    values.push(value.to_string().into_bytes());
                }
                current = self.parent(cur);
            }
            if values.is_empty() {
                // if no counter is found, instance one with value '0'
                self.el_mut(id).counter_values.insert(counter_name_id, 0);
                result = b"0".to_vec();
            } else {
                values.reverse();
                result = join_string(&values, &delims);
            }
        }
        result
    }

    /// `parse_counter_tokens(tokens, default_value, handler)`: names, each with the number after it or the default,
    /// at most 16.
    pub fn parse_counter_tokens(names: &Names, tokens: &[Vec<u8>], default_value: i32) -> Vec<(StringId, i32)> {
        // Each name is looked up through all the previous siblings: 16 keep 2000 siblings cheap.
        const max_names: i32 = 16;
        let mut out = Vec::new();
        let mut pos = 0;
        let mut count = 0;
        while pos < tokens.len() && {
            let ok = count < max_names;
            count += 1;
            ok
        } {
            let name = &tokens[pos];
            let mut value = default_value;
            if pos + 1 < tokens.len() && is_number(&tokens[pos + 1], false) {
                value = twidget_cstd::atoi(crate::gumbo::c_str(&tokens[pos + 1]));
                pos += 2;
            } else {
                pos += 1;
            }
            out.push((names.id(name), value));
        }
        out
    }

    /// `increment_counter(counter_name_id, increment)`: the counter where `find_counter` finds it, saturating, or a
    /// new one here.
    pub fn increment_counter(&mut self, id: ElementId, name: StringId, increment: i32) {
        match self.find_counter(id, name) {
            Some(holder) => {
                let value = self.el_mut(holder).counter_values.get_mut(&name).expect("the counter found");
                *value = (i64::from(*value) + i64::from(increment)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            }
            None => {
                // if counter is not found, initialize one on this element
                self.el_mut(id).counter_values.insert(name, increment);
            }
        }
    }

    /// `reset_counter(counter_name_id, value)`.
    pub fn reset_counter(&mut self, id: ElementId, name: StringId, value: i32) {
        self.el_mut(id).counter_values.insert(name, value);
    }
}
