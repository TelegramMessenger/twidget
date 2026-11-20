//! `document.cpp`, as far as the ported stages need it: lengths in pixels (`to_pixels`, `cvt_units`).
//! What they read of the document (its media features, its root's font size, its container) is a [`Units`]
//! until the document itself is ported.

use crate::Px;
use crate::css_length::{CssLength, calc_evaluate, calc_resolve_units, clamp_length};
use crate::css_values::CssUnits;
use crate::cxx;
use crate::document_container::DocumentContainer;
use crate::types::{FontMetrics, MediaFeatures};

/// What `document::to_pixels` reads of its document.
#[derive(Clone, Copy)]
pub struct Units<'a> {
    /// `m_container`.
    pub container: &'a dyn DocumentContainer,
    /// `m_media`: the media features the document last had from its container.
    pub media: MediaFeatures,
    /// `m_root->css().get_font_size()`.
    pub root_font_size: Px,
}

impl std::fmt::Debug for Units<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Units").field("media", &self.media).field("root_font_size", &self.root_font_size).finish()
    }
}

impl Units<'_> {
    /// `document::to_pixels(val, metrics, size)`: a length in px against a font and, for %, a size; capped
    /// at ±2^25 px; 0 for a keyword.
    pub fn to_pixels(&self, val: &CssLength, metrics: &FontMetrics, size: Px) -> Px {
        if val.is_predefined() {
            return Px(0.0);
        }
        let ret = match val.units() {
            CssUnits::css_units_percentage => val.calc_percent(size),
            CssUnits::css_units_calc => {
                let convert = |value: f32, unit: CssUnits| self.to_pixels(&CssLength::new(value, unit), metrics, size).value();
                calc_evaluate(val.calc().map(|c| &**c), size, Some(&convert))
            }
            CssUnits::css_units_em => metrics.font_size * val.val(),

            // https://drafts.csswg.org/css-values-4/#absolute-lengths
            CssUnits::css_units_pt => self.container.pt_to_px(val.val()),
            // 1in = 72pt
            CssUnits::css_units_in => self.container.pt_to_px(val.val() * 72.0),
            // 1pc = (1/6)in = 12pt
            CssUnits::css_units_pc => self.container.pt_to_px(val.val() * 12.0),
            // 1cm = (1/2.54)in = (72/2.54)pt
            CssUnits::css_units_cm => self.container.pt_to_px(val.val() * 72.0 / 2.54),
            CssUnits::css_units_mm => self.container.pt_to_px(val.val() * 72.0 / 2.54 / 10.0),

            CssUnits::css_units_vw => Px(self.media.width.value() * val.val() / 100.0),
            CssUnits::css_units_vh => Px(self.media.height.value() * val.val() / 100.0),
            CssUnits::css_units_vmin => Px(cxx::min_f32(self.media.height.value(), self.media.width.value()) * val.val() / 100.0),
            CssUnits::css_units_vmax => Px(cxx::max_f32(self.media.height.value(), self.media.width.value()) * val.val() / 100.0),
            CssUnits::css_units_rem => Px(self.root_font_size.value() * val.val()),
            CssUnits::css_units_ex => Px(metrics.x_height.value() * val.val()),
            CssUnits::css_units_ch => Px(metrics.ch_width.value() * val.val()),
            _ => Px(val.val()),
        };
        Px(clamp_length(ret.value()))
    }

    /// `document::cvt_units(val, metrics, size)`: the length in px; a % stays one, and a calc keeps only
    /// its % leaves (or becomes px when it has none).
    pub fn cvt_units(&self, val: &mut CssLength, metrics: &FontMetrics, size: Px) {
        if val.is_predefined() {
            return;
        }
        if val.units() == CssUnits::css_units_calc {
            // Every leaf but % in px; with no % left, the length is a plain one.
            let mut px = 0.0;
            let convert = |value: f32, unit: CssUnits| self.to_pixels(&CssLength::new(value, unit), metrics, size).value();
            let expression = calc_resolve_units(val.calc(), &convert, &mut px);
            match expression {
                None => val.set_value(clamp_length(px), CssUnits::css_units_px),
                Some(expression) => val.set_calc(Some(expression)),
            }
        } else if val.units() != CssUnits::css_units_percentage {
            let px = self.to_pixels(val, metrics, size).value();
            val.set_value(px, CssUnits::css_units_px);
        }
    }
}
