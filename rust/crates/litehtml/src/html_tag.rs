//! `html_tag.cpp`'s selector matching: `select` of a complex selector, a compound, a pseudo-class and an
//! attribute selector, the ancestor and sibling searches the combinators make, the structural counts of the
//! `:nth-*` pseudo-classes, `select_all` and `select_one`. Until the DOM is ported it works on any tree that
//! answers what `css_selector.cpp` and `html_tag.cpp` ask of an element ([`SelectElements`]).

use std::cell::Cell;

use crate::css_selector::{AttrMatcher, AttrSelectType, CssAttributeSelector, CssCombinator, CssElementSelector, CssSelector};
use crate::html::{WHITESPACE, lowcase, match_at, split_string};
use crate::string_id::*;
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
}

impl<T: SelectElements> std::fmt::Debug for Matcher<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matcher").field("prune", &self.prune).field("budget", &self.budget).finish()
    }
}

impl<'a, T: SelectElements> Matcher<'a, T> {
    pub fn new(tree: &'a T, names: &'a Names) -> Matcher<'a, T> {
        Matcher { tree, names, prune: true, budget: None }
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
    /// the same answers and is not taken here.)
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
        self.charge_children(children.len());
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
