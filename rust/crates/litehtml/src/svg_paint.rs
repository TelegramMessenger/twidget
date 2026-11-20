//! `el_svg.cpp`'s paint lookups: an SVG property from the element's style or presentation attribute (its
//! `var()`s substituted, its math folded), inherited from the nearest ancestor that gives it, where a `<use>`
//! being drawn lends its target its own parents (`svg_property`); and what the painter reads through it: a
//! paint (`svg_paint`, with `svg_paint_color` for getComputedStyle), a number (`svg_number`), a keyword
//! (`svg_ident`), opacity (`with_opacity`), a `url(#id)` (`svg_url_id`) and a gradient's paint with its stops
//! (`svg_gradient_paint`). Outside drawing no `<use>` hosts are open, so the ancestors are the element's own.

use crate::StringId;
use crate::css_effects::{SvgGradient, VectorPath, fold_math, has_foldable_math};
use crate::css_parser::{f_componentize, f_remove_whitespace, normalize};
use crate::css_tokenizer::{CV_FUNCTION, CssToken, CssTokenVector, DIMENSION, NUMBER, PERCENTAGE, URL};
use crate::cxx;
use crate::document::{Document, StyleCx};
use crate::document_container::resolver;
use crate::el_svg::{DocSvg, UseHosts, gradient_coordinate, svg_gradient_geometry, svg_reference_id};
use crate::element::ElementId;
use crate::html::lowcase;
use crate::html_tag::ElementVars;
use crate::string_id::{_fill_, _stroke_};
use crate::style::{Value, has_var, subst_vars_};
use crate::web_color::{WebColor, parse_color};

/// `with_opacity(color, opacity)`: the colour's alpha times the opacity clamped to 0–1 (NaN counts as 1), rounded
/// half away from zero.
pub fn with_opacity(mut color: WebColor, opacity: f32) -> WebColor {
    let opacity = cxx::max_f32(0.0, cxx::min_f32(1.0, opacity));
    color.alpha = (f32::from(color.alpha) * opacity).round() as u8;
    color
}

/// `svg_url_id(tokens)`: the id of the first `url(#id)` among the tokens (a URL token's, or the first value of a
/// `url()` function), empty when there is none.
pub fn svg_url_id(tokens: &[CssToken]) -> Vec<u8> {
    for tok in tokens {
        let url: &[u8] = if tok.ty == URL {
            &tok.str
        } else if tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"url" && !tok.value.is_empty() {
            &tok.value[0].str
        } else {
            b""
        };
        if url.len() > 1 && url[0] == b'#' {
            return url[1..].to_vec();
        }
    }
    Vec::new()
}

impl Document {
    /// `svg_property(el, name, inherited)` with no `<use>` being drawn.
    pub fn svg_property(&self, cx: &StyleCx, el: ElementId, name: StringId, inherited: bool) -> CssTokenVector {
        self.svg_property_in(cx, &UseHosts::new(), el, name, inherited)
    }

    /// `svg_property(el, name, inherited)`: the tokens the element's style or its attribute of the name gives (none
    /// when its `var()`s fail), else, when inherited, the nearest ancestor's through the `<use>`s being drawn.
    pub fn svg_property_in(
        &self,
        cx: &StyleCx,
        hosts: &UseHosts<ElementId>,
        el: ElementId,
        name: StringId,
        inherited: bool,
    ) -> CssTokenVector {
        let tree = DocSvg { doc: self, cx };
        let mut hosts_above = hosts.len();
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
            cur = hosts.svg_parent(&tree, e, &mut hosts_above);
        }
        Vec::new()
    }

    /// `svg_paint(el, name, def, def_on, out)` with no `<use>` being drawn: the paint's colour and whether it
    /// paints, the colour the default one when nothing is found.
    pub fn svg_paint(&self, cx: &StyleCx, el: ElementId, name: StringId, def: WebColor, def_on: bool) -> (bool, WebColor) {
        let mut out = WebColor::default();
        let paints = self.svg_paint_in(cx, &UseHosts::new(), el, name, def, def_on, &mut out, None);
        (paints, out)
    }

    /// `svg_paint(el, name, def, def_on, out, container, gradient)`: whether the paint paints, and its colour in
    /// `out`: the default when nothing gives it, none for `none`, the element's colour for `currentcolor`, the
    /// first colour past any url (each url noted in `gradient`); `out` is left as it was when nothing paints.
    #[allow(clippy::too_many_arguments)]
    pub fn svg_paint_in(
        &self,
        cx: &StyleCx,
        hosts: &UseHosts<ElementId>,
        el: ElementId,
        name: StringId,
        def: WebColor,
        def_on: bool,
        out: &mut WebColor,
        mut gradient: Option<&mut bool>,
    ) -> bool {
        if let Some(g) = gradient.as_deref_mut() {
            *g = false;
        }
        let tokens = self.svg_property_in(cx, hosts, el, name, true);
        if tokens.is_empty() {
            *out = def;
            return def_on;
        }
        for tok in &tokens {
            let ident = tok.ident();
            if ident == b"none" {
                return false;
            }
            if ident == b"currentcolor" {
                *out = self.el(el).css.m_color;
                return true;
            }
            if tok.ty == URL || (tok.ty == CV_FUNCTION && lowcase(tok.name()) == b"url") {
                if let Some(g) = gradient.as_deref_mut() {
                    *g = true;
                }
                continue;
            }
            if tok.ty != URL
                && tok.ty != CV_FUNCTION
                && let Some(color) = parse_color(cx.names, tok, resolver(Some(cx.container)))
            {
                *out = color;
                return true;
            }
            if tok.ty == CV_FUNCTION
                && lowcase(tok.name()) != b"url"
                && let Some(color) = parse_color(cx.names, tok, resolver(Some(cx.container)))
            {
                *out = color;
                return true;
            }
        }
        false
    }

    /// `svg_paint_color(el, fill, out, container)`: the fill (black and painting by default) or the stroke (black
    /// and not painting by default).
    pub fn svg_paint_color(&self, cx: &StyleCx, el: ElementId, fill: bool) -> (bool, WebColor) {
        self.svg_paint(cx, el, if fill { _fill_ } else { _stroke_ }, WebColor::BLACK, fill)
    }

    /// `svg_number(el, name, def, inherited)`: the property's first token as a number (a percentage a fraction),
    /// `def` when it is missing or no number.
    pub fn svg_number_in(
        &self,
        cx: &StyleCx,
        hosts: &UseHosts<ElementId>,
        el: ElementId,
        name: StringId,
        def: f32,
        inherited: bool,
    ) -> f32 {
        let tokens = self.svg_property_in(cx, hosts, el, name, inherited);
        let Some(tok) = tokens.first() else { return def };
        if tok.ty == NUMBER || tok.ty == DIMENSION {
            return tok.n.number;
        }
        if tok.ty == PERCENTAGE {
            return tok.n.number / 100.0;
        }
        def
    }

    /// `svg_ident(el, name)`: the inherited property's first token as a keyword (empty when it is none).
    pub fn svg_ident_in(&self, cx: &StyleCx, hosts: &UseHosts<ElementId>, el: ElementId, name: StringId) -> Vec<u8> {
        let tokens = self.svg_property_in(cx, hosts, el, name, true);
        tokens.first().map(CssToken::ident).unwrap_or_default()
    }

    /// `svg_gradient_paint(el, name, path, out, opacity, container, reference)`: the `linearGradient` or
    /// `radialGradient` the paint's `url(#id)` names (looked up by `reference`), over the box of the path's
    /// points, with its `stop`s: each offset clamped and never below the one before, its `stop-color` (the first
    /// colour among its own tokens, black without one) at its own `stop-opacity` times `opacity`. None without a
    /// reference id, a root, a target of those tags, or a stop.
    #[allow(clippy::too_many_arguments)]
    pub fn svg_gradient_paint(
        &self,
        cx: &StyleCx,
        hosts: &UseHosts<ElementId>,
        el: ElementId,
        name: StringId,
        path: &VectorPath,
        opacity: f32,
        reference: &mut dyn FnMut(&[u8]) -> Option<ElementId>,
    ) -> Option<SvgGradient> {
        let id = svg_url_id(&self.svg_property_in(cx, hosts, el, name, true));
        if !svg_reference_id(&id) {
            return None;
        }
        self.root?;
        let target = reference(&id)?;
        let tree = DocSvg { doc: self, cx };
        let mut g = svg_gradient_geometry(&tree, target, path)?;
        for &child in self.children(target) {
            if lowcase(&self.get_tag_name(cx.names, child)) != b"stop" {
                continue;
            }
            let mut offset = cxx::max_f32(0.0, cxx::min_f32(1.0, gradient_coordinate(&tree, child, b"offset", 0.0)));
            let mut color = WebColor::BLACK;
            let tokens = self.svg_property_in(cx, hosts, child, cx.names.id(b"stop-color"), false);
            for tok in &tokens {
                if let Some(parsed) = parse_color(cx.names, tok, resolver(Some(cx.container))) {
                    color = parsed;
                    break;
                }
            }
            color = with_opacity(color, self.svg_number_in(cx, hosts, child, cx.names.id(b"stop-opacity"), 1.0, false) * opacity);
            if let Some(last) = g.stops.last() {
                offset = cxx::max_f32(offset, last.0);
            }
            g.stops.push((offset, color));
        }
        if g.stops.is_empty() {
            return None;
        }
        Some(g)
    }
}
