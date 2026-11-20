//! `html_tag.cpp`: what an `html_tag` keeps of its tag and attributes (`set_tagName`, `set_attr` with its classes
//! and id, `set_class`, the pseudo-classes, `appendChild`, `parse_attributes` with the presentational mappings) on
//! the document's elements, its selector matching (`select` of a complex selector, a compound, a pseudo-class
//! and an attribute selector, the ancestor and sibling searches the combinators make, the structural counts of the
//! `:nth-*` pseudo-classes, `select_all` and `select_one`, the selector budget), and its part of the cascade:
//! `apply_stylesheet` and `apply_selector` with the `::before` and `::after` boxes, `add_style` with the counters,
//! `refresh_styles`, `compute_styles` with style sharing, `get_property`'s `revert` (`ua_property`),
//! `get_custom_property` and `raw_declaration`, and `find_styles_changes`. Matching works on any tree that answers
//! what `css_selector.cpp` and `html_tag.cpp` ask of an element ([`SelectElements`]); the document is one.

use std::cell::Cell;
use std::sync::Arc;

use crate::css_properties::Get;
use crate::css_selector::{
    AttrMatcher, AttrSelectType, CssAttributeSelector, CssCombinator, CssElementSelector, CssSelector, DocumentMode,
};
use crate::css_tokenizer::{CssNumberType, CssToken, CssTokenType, CssTokenVector, DIMENSION, PERCENTAGE, get_repr};
use crate::css_values::StyleDisplay;
use crate::document::{Document, Env, StyleCx};
use crate::document_container::DocumentContainer;
use crate::element::{AttributeDeclaration, AttributeValue, ElementId, ElementKind, ElementState, UsedSelector};
use crate::html::{WHITESPACE, join_string, lowcase, match_at, split_string};
use crate::html_microsyntaxes::{
    HtmlDimensionType, html_parse_dimension_value, html_parse_non_negative_integer, html_parse_nonzero_dimension_value,
};
use crate::string_id::*;
use crate::style::{PropertyValue, Style, Value, VarElement, raw_declaration_id};
use crate::{Names, StringId};

/// `select_result`.
pub const select_no_match: i32 = 0x00;
pub const select_match: i32 = 0x01;
pub const select_match_pseudo_class: i32 = 0x02;
pub const select_match_with_before: i32 = 0x10;
pub const select_match_with_after: i32 = 0x20;

/// What matching asks of an element and its document. Only an `html_tag` is matched: any other element
/// (text, a comment, a space) matches nothing and has no children to search.
pub trait SelectElements {
    type Node: Copy + Eq;

    /// `parent()`.
    fn parent(&self, el: Self::Node) -> Option<Self::Node>;
    /// `m_children`, in order.
    fn children(&self, el: Self::Node) -> &[Self::Node];
    /// Whether the element is an `html_tag`.
    fn is_html_tag(&self, el: Self::Node) -> bool;
    /// `tag()`: `empty_id` for an element that is no tag.
    fn tag(&self, el: Self::Node) -> StringId;
    /// `m_id`.
    fn id(&self, el: Self::Node) -> StringId;
    /// `m_classes`.
    fn classes(&self, el: Self::Node) -> &[StringId];
    /// `get_attr(name)`, the name lower-case.
    fn get_attr(&self, el: Self::Node, name: &[u8]) -> Option<&[u8]>;
    /// `is_comment()`.
    fn is_comment(&self, el: Self::Node) -> bool;
    /// `css().get_display() == display_inline_text`: text, and what the cascade made `inline-text`.
    fn is_inline_text(&self, el: Self::Node) -> bool;
    /// `has_pseudo_class(cls)`: `:root`, `:hover`, `:active` as the document set them.
    fn has_pseudo_class(&self, el: Self::Node, cls: StringId) -> bool;
    /// `get_document()->match_lang(lang)`.
    fn match_lang(&self, lang: &[u8]) -> bool;
}

/// `kSelectorStepBudget`: steps a stylesheet's matching may take while a budget is open.
pub const K_SELECTOR_STEP_BUDGET: i64 = 4_000_000;

/// Matching over a tree, with the C++'s process-wide state made explicit.
#[derive(Clone, Copy)]
pub struct Matcher<'a, T: SelectElements> {
    pub tree: &'a T,
    pub names: &'a Names,
    /// `prune_enabled()`: unless `LH_NO_DESCENDANT_PRUNE`, an ancestor or a sibling that matches the right part
    /// of a descendant (or child) selector but not the whole of it ends the search, as none further can.
    pub prune: bool,
    /// The selector budget while a stylesheet is applied or a `selector_budget` lives: the steps taken. Outside
    /// those the C++ counts nothing.
    pub budget: Option<&'a Cell<i64>>,
    /// While a stylesheet is applied (and not `LH_NO_SIBLING_INDEX`), `find_adjacent_sibling` of a parent of more
    /// than 16 children reads an index of them, which gives the same answer and charges no step for the walk.
    pub indexed: bool,
}

impl<T: SelectElements> std::fmt::Debug for Matcher<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matcher").field("prune", &self.prune).field("budget", &self.budget).finish()
    }
}

impl<'a, T: SelectElements> Matcher<'a, T> {
    pub fn new(tree: &'a T, names: &'a Names) -> Matcher<'a, T> {
        Matcher { tree, names, prune: true, budget: None, indexed: false }
    }

    fn is_generated(&self, el: T::Node) -> bool {
        let tag = self.tree.tag(el);
        tag == __tag_before_ || tag == __tag_after_
    }

    /// `charge_children(count)`: a walk over a child list costs one step for every 64 children.
    fn charge_children(&self, count: usize) {
        if let Some(budget) = self.budget {
            budget.set(budget.get() + (count / 64) as i64);
        }
    }

    /// `select(selector_list, apply_pseudo)`: the first selector's answer that matches.
    pub fn select_list(&self, el: T::Node, selector_list: &[std::sync::Arc<CssSelector>], apply_pseudo: bool) -> i32 {
        if !self.tree.is_html_tag(el) {
            return select_no_match;
        }
        for sel in selector_list {
            let result = self.select(el, sel, apply_pseudo);
            if result != 0 {
                return result;
            }
        }
        select_no_match
    }

    /// `select(selector, apply_pseudo)` of a complex selector: the right compound, then the left part through
    /// the combinator. Without `apply_pseudo` pseudo-classes are not checked but marked in the answer.
    pub fn select(&self, el: T::Node, selector: &CssSelector, apply_pseudo: bool) -> i32 {
        if !self.tree.is_html_tag(el) {
            return select_no_match;
        }
        if let Some(budget) = self.budget {
            budget.set(budget.get() + 1);
            if budget.get() > K_SELECTOR_STEP_BUDGET {
                return select_no_match;
            }
        }
        let mut right_res = self.select_element(el, &selector.m_right, apply_pseudo);
        if right_res == select_no_match {
            return select_no_match;
        }
        let el_parent = self.tree.parent(el);
        if let Some(left) = &selector.m_left {
            let Some(el_parent) = el_parent else { return select_no_match };
            match selector.m_combinator {
                CssCombinator::combinator_descendant => {
                    let Some((_, is_pseudo)) = self.find_ancestor(el, left, apply_pseudo) else { return select_no_match };
                    if is_pseudo {
                        right_res |= select_match_pseudo_class;
                    }
                }
                CssCombinator::combinator_child => {
                    let res = self.select(el_parent, left, apply_pseudo);
                    if res == select_no_match {
                        return select_no_match;
                    }
                    if right_res != select_match_pseudo_class {
                        right_res |= res;
                    }
                }
                CssCombinator::combinator_adjacent_sibling => {
                    let Some((_, is_pseudo)) = self.find_adjacent_sibling(el_parent, el, left, apply_pseudo) else {
                        return select_no_match;
                    };
                    if is_pseudo {
                        right_res |= select_match_pseudo_class;
                    }
                }
                CssCombinator::combinator_general_sibling => {
                    let Some((_, is_pseudo)) = self.find_sibling(el_parent, el, left, apply_pseudo) else { return select_no_match };
                    if is_pseudo {
                        right_res |= select_match_pseudo_class;
                    }
                }
            }
        }
        right_res
    }

    /// `select(selector, apply_pseudo)` of a compound: the type, then each part. A generated `::before` or
    /// `::after` box is matched by the bare pseudo-element only, and other pseudo-elements match nothing.
    pub fn select_element(&self, el: T::Node, selector: &CssElementSelector, apply_pseudo: bool) -> i32 {
        if !self.tree.is_html_tag(el) {
            return select_no_match;
        }
        let tag = self.tree.tag(el);
        if selector.m_tag != star_id && selector.m_tag != tag {
            return select_no_match;
        }

        let mut res = select_match;

        for attr in &selector.m_attrs {
            match attr.ty {
                AttrSelectType::select_class => {
                    if !self.tree.classes(el).contains(&attr.name) {
                        return select_no_match;
                    }
                }
                AttrSelectType::select_id => {
                    if attr.name != self.tree.id(el) {
                        return select_no_match;
                    }
                }
                AttrSelectType::select_pseudo_element => {
                    // A generated box has no ::before or ::after of its own: only the bare form styles it.
                    if (tag == __tag_before_ || tag == __tag_after_) && (selector.m_attrs.len() > 1 || selector.m_tag != star_id) {
                        return select_no_match;
                    }
                    if attr.name == _after_ {
                        if selector.m_attrs.len() == 1 && selector.m_tag == star_id && tag != __tag_after_ {
                            return select_no_match;
                        }
                        res |= select_match_with_after;
                    } else if attr.name == _before_ {
                        if selector.m_attrs.len() == 1 && selector.m_tag == star_id && tag != __tag_before_ {
                            return select_no_match;
                        }
                        res |= select_match_with_before;
                    } else {
                        return select_no_match;
                    }
                }
                AttrSelectType::select_pseudo_class => {
                    if apply_pseudo {
                        if self.select_pseudoclass(el, attr) == select_no_match {
                            return select_no_match;
                        }
                    } else {
                        res |= select_match_pseudo_class;
                    }
                }
                AttrSelectType::select_attr => {
                    if self.select_attribute(el, attr) == select_no_match {
                        return select_no_match;
                    }
                }
            }
        }
        res
    }

    /// `select_pseudoclass(sel)`: the structural, logical, language, input and state pseudo-classes.
    #[inline(never)]
    pub fn select_pseudoclass(&self, el: T::Node, sel: &CssAttributeSelector) -> i32 {
        let tree = self.tree;
        let el_parent = tree.parent(el);

        match sel.name {
            _only_child_ | _only_of_type_ => {
                let of_type = sel.name == _only_of_type_;
                if !el_parent.is_some_and(|p| self.is_only_child(p, el, of_type)) {
                    return select_no_match;
                }
            }
            _first_child_ | _first_of_type_ => {
                let of_type = sel.name == _first_of_type_;
                if !el_parent.is_some_and(|p| self.is_nth_child(p, el, 0, 1, of_type, &[])) {
                    return select_no_match;
                }
            }
            _last_child_ | _last_of_type_ => {
                let of_type = sel.name == _last_of_type_;
                if !el_parent.is_some_and(|p| self.is_nth_last_child(p, el, 0, 1, of_type, &[])) {
                    return select_no_match;
                }
            }
            _nth_child_ | _nth_of_type_ | _nth_last_child_ | _nth_last_of_type_ => {
                let Some(el_parent) = el_parent else { return select_no_match };

                let num = sel.a;
                let off = sel.b;
                if num == 0 && off == 0 {
                    return select_no_match;
                }

                let matched = match sel.name {
                    _nth_child_ => self.is_nth_child(el_parent, el, num, off, false, &sel.selector_list),
                    _nth_of_type_ => self.is_nth_child(el_parent, el, num, off, true, &[]),
                    _nth_last_child_ => self.is_nth_last_child(el_parent, el, num, off, false, &sel.selector_list),
                    _ => self.is_nth_last_child(el_parent, el, num, off, true, &[]),
                };
                if !matched {
                    return select_no_match;
                }
            }
            // :where() matches like :is(); they differ in specificity only
            _is_ | _where_ => {
                if self.select_list(el, &sel.selector_list, true) == 0 {
                    return select_no_match;
                }
            }
            _not_ => {
                if self.select_list(el, &sel.selector_list, true) != 0 {
                    return select_no_match;
                }
            }
            _lang_ => {
                // The nearest lang attribute decides, as a language range: "ru" matches "ru" and "ru-RU".
                let mut lang = None;
                let mut cur = Some(el);
                while let Some(e) = cur {
                    if lang.is_some() {
                        break;
                    }
                    lang = if tree.is_html_tag(e) { tree.get_attr(e, b"lang") } else { None };
                    cur = tree.parent(e);
                }
                match lang {
                    None => {
                        if !tree.match_lang(&sel.value) {
                            return select_no_match;
                        }
                    }
                    Some(lang) => {
                        let tag = lowcase(c_str(lang));
                        let range = lowcase(&sel.value);
                        if range.is_empty() || (tag != range && !tag.starts_with(&[&range[..], b"-"].concat())) {
                            return select_no_match;
                        }
                    }
                }
            }
            _empty_ => {
                // Whitespace is content, as in browsers; comments and generated ::before/::after are not.
                for &child in tree.children(el) {
                    let child_tag = tree.tag(child);
                    if !tree.is_comment(child) && child_tag != __tag_before_ && child_tag != __tag_after_ {
                        return select_no_match;
                    }
                }
            }
            _checked_ => {
                let tag = tree.tag(el);
                let checked = tag == _input_ && tree.get_attr(el, b"checked").is_some();
                let selected = tag == _option_ && tree.get_attr(el, b"selected").is_some();
                if !checked && !selected {
                    return select_no_match;
                }
            }
            _disabled_ | _enabled_ => {
                let tag = tree.tag(el);
                if !matches!(tag, _input_ | _button_ | _select_ | _textarea_ | _option_ | _optgroup_ | _fieldset_)
                    || tree.get_attr(el, b"disabled").is_some() != (sel.name == _disabled_)
                {
                    return select_no_match;
                }
            }
            StringId(_) => {
                if !tree.has_pseudo_class(el, sel.name) {
                    return select_no_match;
                }
            }
        }
        select_match
    }

    /// `select_attribute(sel)` (https://www.w3.org/TR/selectors-4/#attribute-selectors); the value as a C
    /// string, lower-case for a caseless match. An empty value matches nothing but with `=` and `|=`.
    #[inline(never)]
    pub fn select_attribute(&self, el: T::Node, sel: &CssAttributeSelector) -> i32 {
        let Some(value) = self.tree.get_attr(el, &self.names.s(sel.name)) else { return select_no_match };
        let value = c_str(value);
        let attr_value = if sel.caseless_match { lowcase(value) } else { value.to_vec() };

        let matched = match sel.matcher {
            AttrMatcher::attribute_exists => true,
            AttrMatcher::attribute_equals => attr_value == sel.value,
            AttrMatcher::attribute_contains_string => !sel.value.is_empty() && find(&attr_value, &sel.value),
            // one of the whitespace-separated words (quoted runs kept together, as split_string has it)
            AttrMatcher::attribute_contains_word => {
                !sel.value.is_empty() && split_string(&attr_value, WHITESPACE, b"", b"\"").contains(&sel.value)
            }
            AttrMatcher::attribute_starts_with_string => !sel.value.is_empty() && match_at(&attr_value, 0, &sel.value),
            // equal, or the value and a hyphen first; nothing special about an empty value
            AttrMatcher::attribute_starts_with_string_hyphen => {
                attr_value == sel.value || match_at(&attr_value, 0, &[&sel.value[..], b"-"].concat())
            }
            AttrMatcher::attribute_ends_with_string => {
                !sel.value.is_empty() && match_at(&attr_value, -(sel.value.len() as i32), &sel.value)
            }
        };
        if matched { select_match } else { select_no_match }
    }

    /// `find_ancestor(selector, apply_pseudo, is_pseudo)`: the nearest ancestor the selector matches, and
    /// whether its answer was a pseudo-class one.
    pub fn find_ancestor(&self, el: T::Node, selector: &CssSelector, apply_pseudo: bool) -> Option<(T::Node, bool)> {
        if !self.tree.is_html_tag(el) {
            return None;
        }
        let el_parent = self.tree.parent(el)?;
        let res = self.select(el_parent, selector, apply_pseudo);
        if res != select_no_match {
            return Some((el_parent, res & select_match_pseudo_class != 0));
        }
        if self.prune
            && selector.m_left.is_some()
            && selector.m_combinator == CssCombinator::combinator_descendant
            && self.select_element(el_parent, &selector.m_right, apply_pseudo) != select_no_match
        {
            return None;
        }
        self.find_ancestor(el_parent, selector, apply_pseudo)
    }

    /// The children `parent` counts for siblings: not text (`inline-text`) and not generated.
    fn counted(&self, child: T::Node) -> bool {
        !self.tree.is_inline_text(child) && !self.is_generated(child)
    }

    /// `find_adjacent_sibling(el, selector, apply_pseudo, is_pseudo)` on `parent`: the counted sibling just
    /// before `el`, when the selector matches it. (The C++'s index of siblings while a sheet is applied gives
    /// the same answers; only its charge is kept.)
    pub fn find_adjacent_sibling(
        &self,
        parent: T::Node,
        el: T::Node,
        selector: &CssSelector,
        apply_pseudo: bool,
    ) -> Option<(T::Node, bool)> {
        if !self.tree.is_html_tag(parent) {
            return None;
        }
        let children = self.tree.children(parent);
        if !(self.indexed && children.len() > 16) {
            self.charge_children(children.len());
        }
        let mut ret = None;
        for &e in children {
            if self.counted(e) {
                if e == el {
                    if let Some(prev) = ret {
                        let res = self.select(prev, selector, apply_pseudo);
                        if res != select_no_match {
                            return Some((prev, res & select_match_pseudo_class != 0));
                        }
                    }
                    return None;
                }
                ret = Some(e);
            }
        }
        None
    }

    /// `find_sibling(el, selector, apply_pseudo, is_pseudo)` on `parent`: the first counted sibling before `el`
    /// the selector matches.
    pub fn find_sibling(&self, parent: T::Node, el: T::Node, selector: &CssSelector, apply_pseudo: bool) -> Option<(T::Node, bool)> {
        if !self.tree.is_html_tag(parent) {
            return None;
        }
        let children = self.tree.children(parent);
        self.charge_children(children.len());
        let mut ret: Option<(T::Node, bool)> = None;
        for &e in children {
            if self.counted(e) {
                if e == el {
                    return ret;
                }
                if ret.is_none() {
                    let res = self.select(e, selector, apply_pseudo);
                    if res == select_no_match
                        && selector.m_left.is_some()
                        && matches!(selector.m_combinator, CssCombinator::combinator_descendant | CssCombinator::combinator_child)
                        && self.prune
                        && self.select_element(e, &selector.m_right, apply_pseudo) != select_no_match
                    {
                        return None;
                    }
                    if res != select_no_match {
                        ret = Some((e, res & select_match_pseudo_class != 0));
                    }
                }
            }
        }
        None
    }

    /// `is_nth_child(el, num, off, of_type, selector_list)` on `parent`: `el`'s place among the counted children
    /// (of its type, or matching the list when there is one) is `num`·k + `off` for some k ⩾ 0.
    pub fn is_nth_child(
        &self,
        parent: T::Node,
        el: T::Node,
        num: i32,
        off: i32,
        of_type: bool,
        selector_list: &[std::sync::Arc<CssSelector>],
    ) -> bool {
        if !self.tree.is_html_tag(parent) {
            return false;
        }
        let children = self.tree.children(parent);
        self.charge_children(children.len());
        self.nth(children.iter().copied(), el, num, off, of_type, selector_list)
    }

    /// `is_nth_last_child(el, num, off, of_type, selector_list)` on `parent`: as [`Matcher::is_nth_child`], from
    /// the end.
    pub fn is_nth_last_child(
        &self,
        parent: T::Node,
        el: T::Node,
        num: i32,
        off: i32,
        of_type: bool,
        selector_list: &[std::sync::Arc<CssSelector>],
    ) -> bool {
        if !self.tree.is_html_tag(parent) {
            return false;
        }
        let children = self.tree.children(parent);
        self.charge_children(children.len());
        self.nth(children.iter().rev().copied(), el, num, off, of_type, selector_list)
    }

    fn nth(
        &self,
        children: impl Iterator<Item = T::Node>,
        el: T::Node,
        num: i32,
        off: i32,
        of_type: bool,
        selector_list: &[std::sync::Arc<CssSelector>],
    ) -> bool {
        let mut idx: i32 = 1;
        let el_tag = self.tree.tag(el);
        for child in children {
            if self.counted(child) {
                if (!of_type && selector_list.is_empty())
                    || (of_type && self.tree.tag(child) == el_tag)
                    || self.select_list(child, selector_list, true) != 0
                {
                    if el == child {
                        let from = i64::from(idx) - i64::from(off);
                        if num != 0 {
                            return from * i64::from(num) >= 0 && from % i64::from(num) == 0;
                        }
                        return idx == off;
                    }
                    idx += 1;
                }
                if el == child {
                    break;
                }
            }
        }
        false
    }

    /// `is_only_child(el, of_type)` on `parent`: at most one counted child (of `el`'s type).
    pub fn is_only_child(&self, parent: T::Node, el: T::Node, of_type: bool) -> bool {
        if !self.tree.is_html_tag(parent) {
            return false;
        }
        let el_tag = self.tree.tag(el);
        let mut child_count = 0;
        for &child in self.tree.children(parent) {
            if self.counted(child) {
                if !of_type || el_tag == self.tree.tag(child) {
                    child_count += 1;
                }
                if child_count > 1 {
                    break;
                }
            }
        }
        child_count <= 1
    }

    /// `select_all(selector, res)`: the element and its descendants the selector matches, in tree order.
    pub fn select_all(&self, el: T::Node, selector: &CssSelector, res: &mut Vec<T::Node>) {
        if !self.tree.is_html_tag(el) {
            return;
        }
        if self.select(el, selector, true) != 0 {
            res.push(el);
        }
        for &child in self.tree.children(el) {
            self.select_all(child, selector, res);
        }
    }

    /// `select_one(selector)`: the first of them.
    pub fn select_one(&self, el: T::Node, selector: &CssSelector) -> Option<T::Node> {
        if !self.tree.is_html_tag(el) {
            return None;
        }
        if self.select(el, selector, true) != 0 {
            return Some(el);
        }
        self.tree.children(el).iter().find_map(|&child| self.select_one(child, selector))
    }
}

/// An attribute as `get_attr` hands it to C: up to its first NUL.
fn c_str(value: &[u8]) -> &[u8] {
    &value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())]
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

// ---- The element --------------------------------------------------------------------------------------------

/// A value as a `const char*` hands it on: up to its first NUL.
fn c_value(value: &[u8]) -> &[u8] {
    c_str(value)
}

impl Document {
    /// `html_tag::appendChild(el)`: the child gets this parent and goes last.
    pub(crate) fn html_tag_append_child(&mut self, parent: ElementId, el: ElementId) -> bool {
        self.el_mut(el).parent = Some(parent);
        self.el_mut(parent).children.push(el);
        true
    }

    /// `set_tagName(tag)`: an `html_tag`'s tag, lower-case; other classes ignore it.
    pub fn set_tag_name(&mut self, names: &Names, id: ElementId, tag: &[u8]) {
        if self.kind(id).is_html_tag() {
            self.el_mut(id).tag = names.id(&lowcase(c_value(tag)));
        }
    }

    /// `set_attr(name, val)`: an `html_tag` keeps every attribute under its lower-case name with its value as
    /// written; `class` also gives its words and `id` the id, lower-case in quirks mode. Other classes ignore it.
    pub fn set_attr(&mut self, names: &Names, id: ElementId, name: &[u8], val: &[u8]) {
        if !self.kind(id).is_html_tag() {
            return;
        }
        let (name, val) = (lowcase(c_value(name)), c_value(val).to_vec());
        let quirks = self.mode == DocumentMode::quirks_mode;
        let el = &mut self.elements[id.0 as usize];
        el.attrs.insert(name.clone(), val.clone());
        if name == b"class" {
            let val = if quirks { lowcase(&val) } else { val };
            el.str_classes = split_string(&val, WHITESPACE, b"", b"");
            el.classes = el.str_classes.iter().map(|cls| names.id(cls)).collect();
        } else if name == b"id" {
            let val = if quirks { lowcase(&val) } else { val };
            el.id = names.id(&val);
        }
    }

    /// `get_attr(name)`: an `html_tag`'s attribute; none for other classes.
    pub fn get_attr(&self, id: ElementId, name: &[u8]) -> Option<&[u8]> {
        let el = self.el(id);
        if !el.kind.is_html_tag() {
            return None;
        }
        el.attrs.get(name).map(|v| &v[..])
    }

    /// `set_class(pclass, add)`: the space-separated classes added to or taken from `m_str_classes`, and the
    /// `class` attribute written again from them when they changed.
    pub fn set_class(&mut self, names: &Names, id: ElementId, pclass: &[u8], add: bool) -> bool {
        if !self.kind(id).is_html_tag() {
            return false;
        }
        let classes = split_string(c_value(pclass), b" ", b"", b"\"");
        let mut changed = false;
        let str_classes = &mut self.el_mut(id).str_classes;
        if add {
            for class in classes {
                if !str_classes.contains(&class) {
                    str_classes.push(class);
                    changed = true;
                }
            }
        } else {
            for class in &classes {
                let before = str_classes.len();
                str_classes.retain(|c| c != class);
                if str_classes.len() != before {
                    changed = true;
                }
            }
        }
        if changed {
            let class_string = join_string(&self.el(id).str_classes, b" ");
            self.set_attr(names, id, b"class", &class_string);
            return true;
        }
        false
    }

    /// `set_pseudo_class(cls, add)`: whether the element's pseudo-classes changed.
    pub fn set_pseudo_class(&mut self, id: ElementId, cls: StringId, add: bool) -> bool {
        if !self.kind(id).is_html_tag() {
            return false;
        }
        let pseudo = &mut self.el_mut(id).pseudo_classes;
        if add {
            if !pseudo.contains(&cls) {
                pseudo.push(cls);
                return true;
            }
        } else if let Some(at) = pseudo.iter().position(|c| *c == cls) {
            pseudo.remove(at);
            return true;
        }
        false
    }

    /// `set_pseudo_class(cls, add)` on the element and every ancestor: whether any changed.
    fn set_pseudo_class_up(&mut self, id: ElementId, cls: StringId, add: bool) -> bool {
        let mut ret = false;
        let mut el = Some(id);
        while let Some(e) = el {
            if self.set_pseudo_class(e, cls, add) {
                ret = true;
            }
            el = self.parent(e);
        }
        ret
    }

    /// `on_mouse_over()`: `:hover` up the chain.
    pub fn on_mouse_over(&mut self, id: ElementId) -> bool {
        self.set_pseudo_class_up(id, _hover_, true)
    }

    /// `on_mouse_leave()`: `:hover` and `:active` off up the chain, element by element.
    pub fn on_mouse_leave(&mut self, id: ElementId) -> bool {
        let mut ret = false;
        let mut el = Some(id);
        while let Some(e) = el {
            if self.set_pseudo_class(e, _hover_, false) {
                ret = true;
            }
            if self.set_pseudo_class(e, _active_, false) {
                ret = true;
            }
            el = self.parent(e);
        }
        ret
    }

    /// `on_lbutton_down()`: `:active` up the chain.
    pub fn on_lbutton_down(&mut self, id: ElementId) -> bool {
        self.set_pseudo_class_up(id, _active_, true)
    }

    /// `m_style.add_property(name, value, "", false, container)` as `parse_attributes` makes it.
    pub(crate) fn add_declaration(&mut self, id: ElementId, name: StringId, value: AttributeValue, with_container: bool) {
        self.el_mut(id).declarations.push(AttributeDeclaration { name, value, baseurl: Vec::new(), important: false, with_container });
    }

    /// `m_style.add_property(name, text)`, no container.
    pub(crate) fn add_text_declaration(&mut self, id: ElementId, name: StringId, text: &[u8]) {
        self.add_declaration(id, name, AttributeValue::Text(c_value(text).to_vec()), false);
    }

    /// `m_style.add_property(name, text, "", false, container)`.
    pub(crate) fn add_text_declaration_with_container(&mut self, id: ElementId, name: StringId, text: &[u8]) {
        self.add_declaration(id, name, AttributeValue::Text(c_value(text).to_vec()), true);
    }

    /// `html_tag::parse_attributes()`: the font attributes of an SVG text element (in an `<svg>`, or one itself)
    /// set the properties they name, a bare `font-size` number in px; then the children's.
    pub fn html_tag_parse_attributes(&mut self, env: &Env, id: ElementId) {
        const SVG_TEXT_TAGS: [&[u8]; 6] = [b"svg", b"g", b"text", b"tspan", b"textpath", b"a"];
        let mut svg_context = false;
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if svg_context {
                break;
            }
            svg_context = lowcase(&self.get_tag_name(env.names, p)) == b"svg";
            cur = self.parent(p);
        }
        let tag = lowcase(&env.names.s(self.el(id).tag));
        if (tag == b"svg" || svg_context) && SVG_TEXT_TAGS.contains(&&tag[..]) {
            for name in [&b"font-family"[..], b"font-weight", b"font-style", b"font-size"] {
                let Some(value) = self.get_attr(id, name) else { continue };
                if value.is_empty() {
                    continue;
                }
                let mut text = value.to_vec();
                if name == b"font-size" && text.iter().all(|c| b"0123456789.".contains(c)) {
                    text.extend_from_slice(b"px");
                }
                let name = env.names.id(name);
                self.add_text_declaration_with_container(id, name, &text);
            }
        }
        for child in self.el(id).children.clone() {
            self.parse_attributes(env, child);
        }
    }

    /// `css_token(type, number, number_type, str)` in a list of one, as the mappings add it.
    fn mapped(&mut self, id: ElementId, prop_name: StringId, ty: CssTokenType, number: f32, number_type: CssNumberType, unit: &[u8]) {
        let mut tok = CssToken::with_number(ty, number, number_type);
        tok.str = unit.to_vec();
        self.add_declaration(id, prop_name, AttributeValue::Tokens(vec![tok]), false);
    }

    /// `map_to_pixel_length_property(prop_name, attr_value)` (https://html.spec.whatwg.org/multipage/rendering.html#maps-to-the-pixel-length-property).
    pub(crate) fn map_to_pixel_length_property(&mut self, id: ElementId, prop_name: StringId, attr_value: &[u8]) {
        if let Some(n) = html_parse_non_negative_integer(attr_value) {
            self.mapped(id, prop_name, DIMENSION, n as f32, CssNumberType::css_number_integer, b"px");
        }
    }

    /// `map_to_pixel_length_property_with_default_value(prop_name, attr_value, default_value)`.
    pub(crate) fn map_to_pixel_length_property_with_default_value(
        &mut self,
        id: ElementId,
        prop_name: StringId,
        attr_value: &[u8],
        default_value: i32,
    ) {
        let n = html_parse_non_negative_integer(attr_value).unwrap_or(default_value);
        self.mapped(id, prop_name, DIMENSION, n as f32, CssNumberType::css_number_integer, b"px");
    }

    /// `map_to_dimension_property(prop_name, attr_value)` (https://html.spec.whatwg.org/multipage/rendering.html#maps-to-the-dimension-property).
    pub(crate) fn map_to_dimension_property(&mut self, id: ElementId, prop_name: StringId, attr_value: &[u8]) {
        if let Some((x, ty)) = html_parse_dimension_value(attr_value) {
            self.dimension(id, prop_name, x, ty);
        }
    }

    /// `map_to_dimension_property_ignoring_zero(prop_name, attr_value)`.
    pub(crate) fn map_to_dimension_property_ignoring_zero(&mut self, id: ElementId, prop_name: StringId, attr_value: &[u8]) {
        if let Some((x, ty)) = html_parse_nonzero_dimension_value(attr_value) {
            self.dimension(id, prop_name, x, ty);
        }
    }

    fn dimension(&mut self, id: ElementId, prop_name: StringId, x: f32, ty: HtmlDimensionType) {
        match ty {
            HtmlDimensionType::html_length => self.mapped(id, prop_name, DIMENSION, x, CssNumberType::css_number_number, b"px"),
            HtmlDimensionType::html_percentage => self.mapped(id, prop_name, PERCENTAGE, x, CssNumberType::css_number_number, b""),
        }
    }
}

/// The document's elements as matching reads them. `tag()` is the class's (`script` and `style` too), the
/// children are `element::m_children` (an `el_style` has none), `inline-text` is the computed display (text's, as
/// no tag computes to it), and the language is the document's (empty until `lang_changed`, which the renderer never
/// calls).
impl SelectElements for Document {
    type Node = ElementId;

    fn parent(&self, el: ElementId) -> Option<ElementId> {
        Document::parent(self, el)
    }
    fn children(&self, el: ElementId) -> &[ElementId] {
        Document::children(self, el)
    }
    fn is_html_tag(&self, el: ElementId) -> bool {
        self.kind(el).is_html_tag()
    }
    fn tag(&self, el: ElementId) -> StringId {
        Document::tag(self, el)
    }
    fn id(&self, el: ElementId) -> StringId {
        let el = self.el(el);
        if el.kind.is_html_tag() { el.id } else { empty_id }
    }
    fn classes(&self, el: ElementId) -> &[StringId] {
        let el = self.el(el);
        if el.kind.is_html_tag() { &el.classes } else { &[] }
    }
    fn get_attr(&self, el: ElementId, name: &[u8]) -> Option<&[u8]> {
        Document::get_attr(self, el, name)
    }
    fn is_comment(&self, el: ElementId) -> bool {
        Document::is_comment(self, el)
    }
    fn is_inline_text(&self, el: ElementId) -> bool {
        self.el(el).css.m_display == crate::css_values::StyleDisplay::display_inline_text.index()
    }
    fn has_pseudo_class(&self, el: ElementId, cls: StringId) -> bool {
        let el = self.el(el);
        el.kind.is_html_tag() && el.pseudo_classes.contains(&cls)
    }
    fn match_lang(&self, lang: &[u8]) -> bool {
        lang == &self.lang[..] || lang == &self.culture[..]
    }
}

// ---- The cascade ----------------------------------------------------------------------------------------------

/// `max_used_styles`: selectors an element keeps once they matched it.
pub const max_used_styles: usize = 256;

/// `get_custom_property` from an element up: what `style::subst_vars` asks past the style it substitutes, and what
/// `subst_vars_` asks of an element.
pub struct ElementVars<'a> {
    pub doc: &'a Document,
    pub cx: &'a StyleCx<'a>,
    /// The element to start from (`None`: nothing is declared).
    pub start: Option<ElementId>,
}

impl std::fmt::Debug for ElementVars<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementVars").field("start", &self.start).finish_non_exhaustive()
    }
}

impl VarElement for ElementVars<'_> {
    fn get_custom_property(&self, name: StringId, result: &mut CssTokenVector, invalid: &mut bool) -> bool {
        let mut cur = self.start;
        while let Some(el) = cur {
            if !self.doc.kind(el).is_html_tag() {
                return false;
            }
            match &self.doc.el(el).style.get_property(name).value {
                Value::Tokens(tokens) => {
                    *result = tokens.clone();
                    return true;
                }
                Value::CssWideKeyword(_) => {
                    // invalid at computed-value time (style::subst_vars)
                    *invalid = true;
                    return false;
                }
                _ => {}
            }
            cur = self.doc.parent(el);
        }
        false
    }

    fn container(&self) -> Option<&dyn DocumentContainer> {
        Some(self.cx.container)
    }
}

impl Document {
    /// Matching as the C++ does it now: the budget counts while a sheet is applied or a `selector_budget` lives,
    /// and the sibling index is read while a sheet is applied.
    pub fn matcher<'a>(&'a self, cx: &'a StyleCx) -> Matcher<'a, Document> {
        let in_scope = self.apply_depth.get() > 0 || self.budget_depth.get() > 0;
        Matcher {
            tree: self,
            names: cx.names,
            prune: cx.switches.descendant_prune,
            budget: if in_scope { Some(&self.selector_steps) } else { None },
            indexed: self.apply_depth.get() > 0 && cx.switches.sibling_index,
        }
    }

    /// `apply_scope`'s constructor: the first one starts the steps at 0 unless a budget is open.
    fn apply_scope_enter(&self) {
        if self.apply_depth.get() == 0 && self.budget_depth.get() == 0 {
            self.selector_steps.set(0);
        }
        self.apply_depth.set(self.apply_depth.get() + 1);
    }

    fn apply_scope_exit(&self) {
        self.apply_depth.set(self.apply_depth.get() - 1);
    }

    /// `selector_budget()`: the first one starts the steps at 0 unless a sheet is being applied.
    pub fn budget_enter(&self) {
        if self.budget_depth.get() == 0 && self.apply_depth.get() == 0 {
            self.selector_steps.set(0);
        }
        self.budget_depth.set(self.budget_depth.get() + 1);
    }

    pub fn budget_exit(&self) {
        self.budget_depth.set(self.budget_depth.get() - 1);
    }

    /// `apply_stylesheet(stylesheet)`: each selector of the sheet, then the children that are not text; an element
    /// that is no tag takes none, an anchor with an `href` is a `:link` first.
    pub fn apply_stylesheet(&mut self, cx: &StyleCx, id: ElementId, selectors: &[Arc<CssSelector>]) {
        if !self.kind(id).is_html_tag() {
            return;
        }
        if self.kind(id) == ElementKind::el_anchor {
            self.el_anchor_apply_stylesheet(id);
        }
        self.apply_scope_enter();
        for sel in selectors {
            self.apply_selector(cx, id, sel);
        }
        let mut i = 0;
        while i < self.el(id).children.len() {
            let child = self.el(id).children[i];
            if self.el(child).css.m_display != StyleDisplay::display_inline_text.index() {
                self.apply_stylesheet(cx, child, selectors);
            }
            i += 1;
        }
        self.apply_scope_exit();
    }

    /// `apply_selector(sel)`: past 256 kept selectors, or a type or first class that differs, nothing; else the
    /// selector's match without pseudo-classes, and when it matches its style is added (to the element, or to its
    /// `::before` or `::after`, made when the rule gives content) where its media hold and its pseudo-classes do, and
    /// the selector kept.
    #[inline(never)]
    pub fn apply_selector(&mut self, cx: &StyleCx, id: ElementId, sel: &Arc<CssSelector>) {
        if self.el(id).used_styles.len() >= max_used_styles {
            return;
        }
        let r = &sel.m_right;
        if r.m_tag != star_id && r.m_tag != self.el(id).tag {
            return;
        }
        if let Some(attr) = r.m_attrs.first()
            && attr.ty == AttrSelectType::select_class
            && !self.el(id).classes.contains(&attr.name)
        {
            return;
        }

        let apply = self.matcher(cx).select(id, sel, false);

        if apply != select_no_match {
            let mut used = false;
            if sel.is_media_valid() {
                let style = &sel.m_style.as_ref().expect("a sheet's selector has its rule's style").style;
                if apply & select_match_pseudo_class != 0 {
                    if self.matcher(cx).select(id, sel, true) != 0 {
                        if apply & (select_match_with_after | select_match_with_before) != 0 {
                            used = self.apply_before_after(cx, id, sel, apply);
                        } else {
                            self.add_style(cx, id, style);
                            used = true;
                        }
                    }
                } else if apply & (select_match_with_after | select_match_with_before) != 0 {
                    used = self.apply_before_after(cx, id, sel, apply);
                } else {
                    self.add_style(cx, id, style);
                    used = true;
                }
            }
            self.el_mut(id).used_styles.push(UsedSelector { selector: Arc::clone(sel), used });
        }
    }

    /// `apply_selector`'s `apply_before_after`: the style goes to the `::after` or `::before` box (made when the
    /// rule is more than a bare pseudo-element and gives content other than `none`, which removes the box), or to
    /// the element itself when it has none; whether it was used.
    fn apply_before_after(&mut self, cx: &StyleCx, id: ElementId, sel: &Arc<CssSelector>, apply: i32) -> bool {
        let style = &sel.m_style.as_ref().expect("a sheet's selector has its rule's style").style;
        let content_none = matches!(&style.get_property(_content_).value, Value::String(s) if s == b"none");
        let create = !content_none && (sel.m_right.m_attrs.len() > 1 || sel.m_right.m_tag != star_id);

        let el = if apply & select_match_with_after != 0 {
            self.get_element_after(id, create)
        } else if apply & select_match_with_before != 0 {
            self.get_element_before(id, create)
        } else {
            return false;
        };
        match el {
            Some(el) => {
                if !content_none {
                    self.add_style(cx, el, style);
                } else if let Some(parent) = self.parent(el) {
                    self.remove_child(parent, el);
                    // The box goes with its last owner, and gives its generated content back.
                    if let ElementState::Generated { boxes, bytes, .. } = self.el(el).state {
                        self.release_generated(boxes, bytes);
                    }
                }
            }
            None => {
                if !content_none {
                    self.add_style(cx, id, style);
                }
            }
        }
        true
    }

    /// `get_element_before(style, create)`: the first child when it is a `::before`, or a new one.
    pub fn get_element_before(&mut self, id: ElementId, create: bool) -> Option<ElementId> {
        if let Some(&front) = self.el(id).children.first()
            && self.tag(front) == __tag_before_
        {
            return Some(front);
        }
        if create {
            return Some(self.add_before_after(id, 0));
        }
        None
    }

    /// `get_element_after(style, create)`: the last child when it is an `::after`, or a new one.
    pub fn get_element_after(&mut self, id: ElementId, create: bool) -> Option<ElementId> {
        if let Some(&back) = self.el(id).children.last()
            && self.tag(back) == __tag_after_
        {
            return Some(back);
        }
        if create {
            return Some(self.add_before_after(id, 1));
        }
        None
    }

    /// `html_tag(parent, style)`, the constructor of the render tree's anonymous boxes: an `html_tag` with no tag and
    /// no attributes, the style text added (no container, no base url), anonymous, its parent `parent` (which does
    /// not list it among its children), and its style computed. It takes a slot an earlier one left when there is one.
    #[inline(never)]
    pub fn new_anonymous_tag(&mut self, cx: &StyleCx, parent: ElementId, style: &[u8]) -> ElementId {
        let id = match self.free_elements.pop() {
            Some(id) => id,
            None => self.new_element(ElementKind::html_tag),
        };
        let mut st = Style::default();
        st.add_text(cx.names, cx.cache, style, b"", None);
        self.html_tag_add_style(cx, id, &st);
        self.el_mut(id).anonymous_style = true;
        self.el_mut(id).parent = Some(parent);
        if self.el(parent).anonymous_style {
            self.el_mut(parent).box_holds += 1;
        }
        self.compute_styles(cx, id, true);
        id
    }

    /// `add_style(style)`, as the element's class has it: a `::before` or `::after` makes its content too.
    pub fn add_style(&mut self, cx: &StyleCx, id: ElementId, style: &Style) {
        match self.kind(id) {
            ElementKind::el_before | ElementKind::el_after => self.before_after_add_style(cx, id, style),
            kind if kind.is_html_tag() => self.html_tag_add_style(cx, id, style),
            _ => {}
        }
    }

    /// `html_tag::add_style(style)`: the style combined into the element's, then its counters reset or (when the
    /// style brings another `counter-increment`) incremented.
    pub fn html_tag_add_style(&mut self, cx: &StyleCx, id: ElementId, style: &Style) {
        let added = style.get_property(_counter_increment_);
        let counted = self.el(id).style.get_property(_counter_increment_);
        let changed = match (&added.value, &counted.value) {
            (Value::Strings(a), Value::Strings(c)) => a != c,
            (Value::Strings(_), _) => true,
            _ => false,
        };
        self.el_mut(id).style.combine(style);
        self.handle_counter_properties(cx, id, changed);
    }

    /// `handle_counter_properties(increment)`: a `counter-reset` resets; else a `counter-increment`, when asked and
    /// at most twice, increments.
    fn handle_counter_properties(&mut self, cx: &StyleCx, id: ElementId, increment: bool) {
        let reset = match &self.el(id).style.get_property(_counter_reset_).value {
            Value::Strings(tokens) => Some(Document::parse_counter_tokens(cx.names, tokens, 0)),
            _ => None,
        };
        if let Some(counters) = reset {
            for (name, value) in counters {
                self.reset_counter(id, name, value);
            }
            return;
        }

        if increment
            && self.el(id).counter_changes < 2
            && let Value::Strings(tokens) = &self.el(id).style.get_property(_counter_increment_).value
        {
            let tokens = tokens.clone();
            self.el_mut(id).counter_changes += 1;
            for (name, value) in Document::parse_counter_tokens(cx.names, &tokens, 1) {
                self.increment_counter(id, name, value);
            }
        }
    }

    /// `refresh_styles()`: the children's first (but text), then the element's style again from the selectors it
    /// kept, as each matches now, under one selector budget; the attributes' declarations are not added again.
    pub fn refresh_styles(&mut self, cx: &StyleCx, id: ElementId) {
        if !self.kind(id).is_html_tag() {
            return;
        }
        self.budget_enter();
        self.el_mut(id).has_style_base = false;
        let mut i = 0;
        while i < self.el(id).children.len() {
            let child = self.el(id).children[i];
            if self.el(child).css.m_display != StyleDisplay::display_inline_text.index() {
                self.refresh_styles(cx, child);
            }
            i += 1;
        }

        self.refresh_own_styles(cx, id);
        self.budget_exit();
    }

    /// `refresh_styles()`'s own part, after the children's: kept out of the recursion's frames.
    #[inline(never)]
    fn refresh_own_styles(&mut self, cx: &StyleCx, id: ElementId) {
        self.el_mut(id).style.clear();
        self.el_mut(id).counter_changes = 0;

        for k in 0..self.el(id).used_styles.len() {
            self.el_mut(id).used_styles[k].used = false;
            let sel = Arc::clone(&self.el(id).used_styles[k].selector);
            if !sel.is_media_valid() {
                continue;
            }
            let apply = self.matcher(cx).select(id, &sel, false);
            if apply == select_no_match {
                continue;
            }
            let style = &sel.m_style.as_ref().expect("a sheet's selector has its rule's style").style;
            if apply & select_match_pseudo_class != 0 {
                if self.matcher(cx).select(id, &sel, true) != 0 {
                    if apply & select_match_with_after != 0 {
                        if let Some(el) = self.get_element_after(id, false) {
                            self.add_style(cx, el, style);
                        }
                    } else if apply & select_match_with_before != 0 {
                        if let Some(el) = self.get_element_before(id, false) {
                            self.add_style(cx, el, style);
                        }
                    } else {
                        self.add_style(cx, id, style);
                        self.el_mut(id).used_styles[k].used = true;
                    }
                }
            } else if apply & select_match_with_after != 0 {
                if let Some(el) = self.get_element_after(id, false) {
                    self.add_style(cx, el, style);
                }
            } else if apply & select_match_with_before != 0 {
                if let Some(el) = self.get_element_before(id, false) {
                    self.add_style(cx, el, style);
                }
            } else {
                self.add_style(cx, id, style);
                self.el_mut(id).used_styles[k].used = true;
            }
        }
    }

    /// `compute_styles(recursive)`, as each class has it.
    pub fn compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        match self.kind(id) {
            ElementKind::el_text | ElementKind::el_space => self.el_text_compute_styles(cx, id),
            ElementKind::el_style => self.el_mut(id).css.m_display = StyleDisplay::display_none.index(),
            ElementKind::el_image => self.el_image_compute_styles(cx, id, recursive),
            ElementKind::el_input => self.el_input_compute_styles(cx, id, recursive),
            ElementKind::el_select => self.el_select_compute_styles(cx, id, recursive),
            ElementKind::el_button => self.el_button_compute_styles(cx, id, recursive),
            ElementKind::el_textarea => self.el_textarea_compute_styles(cx, id, recursive),
            ElementKind::el_meter => self.el_meter_compute_styles(cx, id, recursive),
            ElementKind::el_fieldset => self.el_fieldset_compute_styles(cx, id, recursive),
            kind if kind.is_html_tag() => self.html_tag_compute_styles(cx, id, recursive),
            _ => {}
        }
    }

    /// `html_tag::compute_styles(recursive)`: an element with no pseudo-class, `style` attribute, `var()`,
    /// selectable id or attribute but `class` and `data-*` takes the computed style of an element of the same
    /// tag, parent style and matched selectors when there was one; else its `style` attribute is added (the style
    /// before it kept), its `var()`s substituted and its style computed. Then its children's.
    pub fn html_tag_compute_styles(&mut self, cx: &StyleCx, id: ElementId, recursive: bool) {
        self.compute_own_style(cx, id);

        if recursive {
            let mut i = 0;
            while i < self.el(id).children.len() {
                let child = self.el(id).children[i];
                self.compute_styles(cx, child, true);
                i += 1;
            }
        }
    }

    /// `html_tag::compute_styles`'s own part, before the children's: kept out of the recursion's frames (a
    /// computed style is a few kilobytes).
    #[inline(never)]
    fn compute_own_style(&mut self, cx: &StyleCx, id: ElementId) {
        self.layout.style_epoch += 1;
        self.mark_element_layout_dirty(cx, id);
        let mut eligible = false;
        let mut shared = false;
        let mut share_key = Vec::new();

        {
            let el = self.el(id);
            if cx.switches.style_sharing
                && !el.anonymous_style
                && el.pseudo_classes.is_empty()
                && !el.attrs.contains_key(&b"style"[..])
                && !el.style.has_var_property()
                && (el.id == empty_id || !self.ids_selectable(cx))
            {
                eligible = true;
                for name in el.attrs.keys() {
                    if name != b"class" && !name.starts_with(b"data-") && (name != b"id" || self.ids_selectable(cx)) {
                        eligible = false;
                        break;
                    }
                }
            }
        }

        if eligible {
            let parent_id = self.parent(id).map_or(0, |p| self.el(p).css.m_shared_id);
            share_key = format!("{parent_id}#{}", self.el(id).tag.0).into_bytes();
            for us in &self.el(id).used_styles {
                share_key.push(if us.used { b'|' } else { b'!' });
                share_key.extend(us.selector.place().to_string().into_bytes());
            }
            if let Some(hit) = self.share_cache.get(&share_key) {
                self.el_mut(id).css = hit.clone();
                shared = true;
            }
        }

        if !shared {
            if let Some(style) = self.el(id).attrs.get(&b"style"[..]).cloned() {
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
            css.compute(&Get { doc: self, cx, id });
            css.m_shared_id = self.next_share_id();
            if eligible {
                self.share_cache.insert(share_key, css.clone());
            }
            self.el_mut(id).css = css;
        }
    }

    /// `ua_property(name)`: what the master sheet alone gives the element (its unlayered rules, sorted, the last
    /// that matches and holds winning unless an earlier one is important); invalid when nothing.
    pub fn ua_property(&self, cx: &StyleCx, id: ElementId, name: StringId) -> PropertyValue {
        let none = PropertyValue::default();
        let mut found = &none;
        let matcher = self.matcher(cx);
        for sel in self.master_css.selectors() {
            if sel.m_layered {
                continue;
            }
            let Some(rule) = &sel.m_style else { continue };
            let value = rule.style.get_property(name);
            if matches!(value.value, Value::Invalid) || (found.m_important && !value.m_important) || !sel.is_media_valid() {
                continue;
            }
            let matched = matcher.select(id, sel, true);
            if matched & select_match == 0 || matched & (select_match_with_before | select_match_with_after) != 0 {
                continue;
            }
            found = value;
        }
        found.clone()
    }

    /// `raw_declaration(name)`: the tokens of the property's `-lh-raw-` twin as written, spaced; empty when none.
    pub fn raw_declaration(&self, names: &Names, id: ElementId, name: &[u8]) -> Vec<u8> {
        let raw = raw_declaration_id(names, names.id(name));
        if raw == empty_id || !self.kind(id).is_html_tag() {
            return Vec::new();
        }
        match &self.el(id).style.get_property(raw).value {
            Value::Tokens(tokens) => get_repr(tokens, 0, -1, true),
            _ => Vec::new(),
        }
    }

    /// `requires_styles_update()`: a kept selector whose match changed: it matches plainly now and was not used, or
    /// matches nothing and was.
    #[inline(never)]
    pub fn requires_styles_update(&self, cx: &StyleCx, id: ElementId) -> bool {
        if !self.kind(id).is_html_tag() {
            return false;
        }
        let matcher = self.matcher(cx);
        for used in &self.el(id).used_styles {
            if used.selector.is_media_valid() {
                let res = matcher.select(id, &used.selector, true);
                if (res == select_no_match && used.used) || (res == select_match && !used.used) {
                    return true;
                }
            }
        }
        false
    }

    /// `find_styles_changes(redraw_box)`: under one selector budget, an element (not text) whose selectors match
    /// otherwise now gets its styles again and computed, then its children are looked at. (The boxes it would
    /// have redrawn are stage L6's.)
    pub fn find_styles_changes(&mut self, cx: &StyleCx, id: ElementId) -> bool {
        self.find_styles_changes_with(cx, id, &mut |_| {})
    }

    /// `find_styles_changes(redraw_box)`: the boxes an element whose styles change had (its own and its children's
    /// render items' rendering boxes) told before it is restyled.
    pub fn find_styles_changes_with(&mut self, cx: &StyleCx, id: ElementId, redraw_box: &mut dyn FnMut(crate::types::Position)) -> bool {
        self.budget_enter();
        let ret = self.find_styles_changes_scoped(cx, id, redraw_box);
        self.budget_exit();
        ret
    }

    /// The rendering boxes of the element's render items and its children's, kept out of the walk's frames.
    #[inline(never)]
    fn restyled_boxes(&self, cx: &StyleCx, id: ElementId, redraw_box: &mut dyn FnMut(crate::types::Position)) {
        for el in std::iter::once(id).chain(self.el(id).children.iter().copied()) {
            for ri in self.live_renders(el) {
                self.get_rendering_boxes(cx, ri, redraw_box);
            }
        }
    }

    fn find_styles_changes_scoped(&mut self, cx: &StyleCx, id: ElementId, redraw_box: &mut dyn FnMut(crate::types::Position)) -> bool {
        if self.el(id).css.m_display == StyleDisplay::display_inline_text.index() {
            return false;
        }

        let mut ret = false;

        if self.requires_styles_update(cx, id) {
            self.restyled_boxes(cx, id, redraw_box);
            self.refresh_styles(cx, id);
            self.compute_styles(cx, id, true);
            ret = true;
        }
        let mut i = 0;
        while i < self.el(id).children.len() {
            let child = self.el(id).children[i];
            if self.find_styles_changes_with(cx, child, redraw_box) {
                ret = true;
            }
            i += 1;
        }
        ret
    }
}
