//! `el_svg.cpp`'s paint lookup, as the cascade leaves it for getComputedStyle (`svg_paint_color`): an SVG property
//! from the element's style or presentation attribute (its `var()`s substituted, its math folded), inherited from
//! the nearest ancestor that gives it; and a paint from it. Outside drawing no `<use>` hosts are open, so the
//! ancestors are the element's own. The shapes, paths and drawing of `el_svg.cpp` are stage L14's.

use crate::StringId;
use crate::css_effects::{fold_math, has_foldable_math};
use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::css_tokenizer::{CV_FUNCTION, CssTokenVector, URL};
use crate::document::{Document, StyleCx};
use crate::document_container::resolver;
use crate::element::ElementId;
use crate::html::lowcase;
use crate::html_tag::ElementVars;
use crate::string_id::{_fill_, _stroke_};
use crate::style::{Value, has_var, subst_vars_};
use crate::web_color::{WebColor, parse_color};

impl Document {
    /// `svg_property(el, name, inherited)`: the tokens the element's style or its attribute of the name gives
    /// (none when its `var()`s fail), else, when inherited, the nearest ancestor's.
    pub fn svg_property(&self, cx: &StyleCx, el: ElementId, name: StringId, inherited: bool) -> CssTokenVector {
        let mut cur = Some(el);
        while let Some(e) = cur {
            if self.kind(e).is_html_tag() {
                if let Value::Tokens(tokens) = &self.el(e).style.get_property(name).value {
                    return tokens.clone();
                }
                if let Some(attr) = self.get_attr(e, &cx.names.s(name)) {
                    let mut tokens = normalize(attr, f_componentize | f_remove_whitespace, None);
                    if has_var(&tokens)
                        && (!subst_vars_(cx.names, name, &mut tokens, &ElementVars { doc: self, cx, start: Some(e) }) || has_var(&tokens))
                    {
                        return Vec::new();
                    }
                    if has_foldable_math(&tokens) {
                        fold_math(cx.names, &mut tokens, Some(cx.container), 0);
                    }
                    return tokens;
                }
            }
            if !inherited {
                break;
            }
            cur = self.parent(e);
        }
        Vec::new()
    }

    /// `svg_paint(el, name, def, def_on, out)`: the paint's colour and whether it paints: the default when nothing
    /// gives it, none for `none`, the element's colour for `currentcolor`, the first colour past any url.
    pub fn svg_paint(&self, cx: &StyleCx, el: ElementId, name: StringId, def: WebColor, def_on: bool) -> (bool, WebColor) {
        let tokens = self.svg_property(cx, el, name, true);
        if tokens.is_empty() {
            return (def_on, def);
        }
        let mut out = WebColor::default();
        for tok in &tokens {
            let ident = tok.ident();
            if ident == b"none" {
                return (false, out);
            }
            if ident == b"currentcolor" {
                out = self.el(el).css.m_color;
                return (true, out);
            }
            if tok.ty == URL || (tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"url") {
                continue;
            }
            if tok.ty != URL
                && tok.ty != CV_FUNCTION
                && let Some(color) = parse_color(cx.names, tok, resolver(Some(cx.container)))
            {
                return (true, color);
            }
            if tok.ty == CV_FUNCTION
                && lowcase(tok.name()) != b"url"
                && let Some(color) = parse_color(cx.names, tok, resolver(Some(cx.container)))
            {
                return (true, color);
            }
        }
        (false, out)
    }

    /// `svg_paint_color(el, fill, out, container)`: the fill (black and painting by default) or the stroke (black
    /// and not painting by default).
    pub fn svg_paint_color(&self, cx: &StyleCx, el: ElementId, fill: bool) -> (bool, WebColor) {
        self.svg_paint(cx, el, if fill { _fill_ } else { _stroke_ }, WebColor::BLACK, fill)
    }
}
