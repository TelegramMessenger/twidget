//! `gradient.cpp`, as far as the ported stages need it: angles.

use crate::Px;
use crate::css_tokenizer::{CssToken, DIMENSION, NUMBER, PERCENTAGE};
use crate::html::lowcase;
use crate::string_id::{_deg_, _grad_, _rad_, _turn_};
use crate::{Names, StringId};

#[allow(clippy::approx_constant, clippy::excessive_precision)]
const PI: f32 = 3.14159265;

/// `parse_angle(tok, angle, percents_allowed)` (https://drafts.csswg.org/css-values-4/#angles): degrees, or
/// None. A unitless zero is 0 and a percentage, where allowed, a share of 360; the unit is looked up through
/// the name table, which keeps it (`_id`), as the C++ does.
pub fn parse_angle(names: &Names, tok: &CssToken, percents_allowed: bool) -> Option<f32> {
    // The unit identifier may be omitted if the <angle> is zero.
    if tok.ty == NUMBER && Px(tok.n.number) == Px(0.0) {
        return Some(0.0);
    }

    // <angle-percentage> in conic gradient
    if tok.ty == PERCENTAGE && percents_allowed {
        return Some(tok.n.number * 360.0 / 100.0);
    }

    if tok.ty == DIMENSION {
        let number = tok.n.number;
        return match names.id(&lowcase(tok.unit())) {
            _deg_ => Some(number),
            _grad_ => Some((number / 400.0) * 360.0),
            _rad_ => Some((number / (2.0 * PI)) * 360.0),
            _turn_ => Some(number * 360.0),
            StringId(_) => None,
        };
    }

    None
}
