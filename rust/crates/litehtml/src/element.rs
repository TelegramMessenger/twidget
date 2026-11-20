//! `element.cpp`, and the element classes as a document is built: `element.h`'s tree of `shared_ptr`s is an
//! arena here, [`crate::document::Document::elements`], each element an [`Element`] linked by [`ElementId`], its
//! class an [`ElementKind`]. The classes whose files hold nothing but construction and a flag (`el_body`,
//! `el_break`, `el_cdata`, `el_comment`, `el_space`, `el_text`) live here, as do the classes' `appendChild`s
//! (`el_script`'s, `el_style`'s, `el_table`'s) and the `parse_attributes` dispatch; the others have a module each.
//! What a class does in layout and painting (`el_text`'s measuring, `el_image`'s drawing) is later stages'.
//!
//! What the cascade computes (`m_css`, `m_style`'s declarations from style sheets) is stage L5's: an element
//! keeps the declarations its attributes add ([`Element::declarations`]) for the cascade to add to its style, and
//! the display its class starts with. A `::before` or `::after` box is made here ([`Document::add_before_after`]);
//! the content its style gives it (`el_before_after_base::add_style`) is the cascade's and stage L6's.

use std::collections::BTreeMap;

use crate::css_tokenizer::CssToken;
use crate::css_values::StyleDisplay;
use crate::document::{Document, Env};
use crate::string_id::*;
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
    /// `css().get_display()` as the class's constructor leaves it.
    pub display: StyleDisplay,
    pub state: ElementState,
}

impl Element {
    fn new(kind: ElementKind) -> Element {
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
            display: StyleDisplay::display_inline,
            state: ElementState::None,
        }
    }
}

impl Document {
    /// `std::make_shared<kind>(doc)`: a new element, its constructor's effects included (`el_text`'s display,
    /// `el_image`'s and `el_svg`'s, `el_svg` marking the document, a generated box's tag).
    pub fn new_element(&mut self, kind: ElementKind) -> ElementId {
        let mut el = Element::new(kind);
        match kind {
            ElementKind::el_text | ElementKind::el_space => el.display = StyleDisplay::display_inline_text,
            ElementKind::el_image => {
                el.display = StyleDisplay::display_inline_block;
                el.state = ElementState::Image { src: Vec::new(), loaded: false };
            }
            ElementKind::el_svg => {
                self.has_svg = true;
                el.display = StyleDisplay::display_inline_block;
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

    /// `is_break()` before the cascade: an `el_break`. (An `el_space` is one in a `pre` white-space, which
    /// stage L5 computes.)
    pub fn is_break(&self, id: ElementId) -> bool {
        self.kind(id) == ElementKind::el_break
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
