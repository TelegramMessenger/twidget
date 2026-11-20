//! `web_color.h` and `web_color.cpp`: colours, the named colours, `#rgb[a]` and `#rrggbb[aa]`, `rgb()`,
//! `rgba()`, `hsl()` and `hsla()` in their legacy and modern forms (https://drafts.csswg.org/css-color-4/),
//! `currentcolor`, darkening and the hex form.

use crate::Names;
use crate::Px;
use crate::css_length::{CssLength, f_number, f_percentage};
use crate::css_parser::parse_comma_separated_list;
use crate::css_tokenizer::{CV_FUNCTION, CssToken, HASH, NUMBER, PERCENTAGE};
use crate::css_values::{CssUnits, CssValues, split_css_values};
use crate::cxx;
use crate::gradient::parse_angle;
use crate::html::{digit_value, equal_i, is_hex_digit, lowcase};

/// `web_color`. Two colours are equal when their channels are: `is_current_color` takes no part.
#[derive(Clone, Copy, Debug)]
pub struct WebColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
    pub is_current_color: bool,
}

impl Default for WebColor {
    fn default() -> WebColor {
        WebColor { red: 0, green: 0, blue: 0, alpha: 255, is_current_color: false }
    }
}

impl PartialEq for WebColor {
    fn eq(&self, color: &WebColor) -> bool {
        self.red == color.red && self.green == color.green && self.blue == color.blue && self.alpha == color.alpha
    }
}

impl WebColor {
    pub const TRANSPARENT: WebColor = WebColor::rgba(0, 0, 0, 0);
    pub const BLACK: WebColor = WebColor::rgba(0, 0, 0, 255);
    pub const WHITE: WebColor = WebColor::rgba(255, 255, 255, 255);
    /// `web_color(true)`: opaque black that stands for the current colour.
    pub const CURRENT_COLOR: WebColor = WebColor { red: 0, green: 0, blue: 0, alpha: 255, is_current_color: true };

    /// `web_color(r, g, b, a)`.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> WebColor {
        WebColor { red, green, blue, alpha, is_current_color: false }
    }

    /// `darken(fraction)`: each channel less `fraction` of itself, cut at 0 and truncated (a channel whose
    /// result is no `int`, from a NaN or huge fraction, is what the cast gives there); alpha kept.
    pub fn darken(&self, fraction: f64) -> WebColor {
        let v_red = i32::from(self.red);
        let v_blue = i32::from(self.blue);
        let v_green = i32::from(self.green);
        let v_red = cxx::f64_to_int(cxx::max_f64(f64::from(v_red) - (f64::from(v_red) * fraction), 0.0));
        let v_blue = cxx::f64_to_int(cxx::max_f64(f64::from(v_blue) - (f64::from(v_blue) * fraction), 0.0));
        let v_green = cxx::f64_to_int(cxx::max_f64(f64::from(v_green) - (f64::from(v_green) * fraction), 0.0));
        WebColor::rgba(v_red as u8, v_green as u8, v_blue as u8, self.alpha)
    }

    /// `to_string()`: `RRGGBBAA` in upper-case hex, `RRGGBB` for a transparent colour.
    pub fn to_string(&self) -> Vec<u8> {
        let text = if self.alpha != 0 {
            format!("{:02X}{:02X}{:02X}{:02X}", self.red, self.green, self.blue, self.alpha)
        } else {
            format!("{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
        };
        text.into_bytes()
    }
}

/// `g_def_colors`: the named colours and what they stand for, in the C++ order.
pub const DEF_COLORS: [(&str, &str); 149] = [
    ("transparent", "rgba(0, 0, 0, 0)"),
    ("AliceBlue", "#F0F8FF"),
    ("AntiqueWhite", "#FAEBD7"),
    ("Aqua", "#00FFFF"),
    ("Aquamarine", "#7FFFD4"),
    ("Azure", "#F0FFFF"),
    ("Beige", "#F5F5DC"),
    ("Bisque", "#FFE4C4"),
    ("Black", "#000000"),
    ("BlanchedAlmond", "#FFEBCD"),
    ("Blue", "#0000FF"),
    ("BlueViolet", "#8A2BE2"),
    ("Brown", "#A52A2A"),
    ("BurlyWood", "#DEB887"),
    ("CadetBlue", "#5F9EA0"),
    ("Chartreuse", "#7FFF00"),
    ("Chocolate", "#D2691E"),
    ("Coral", "#FF7F50"),
    ("CornflowerBlue", "#6495ED"),
    ("Cornsilk", "#FFF8DC"),
    ("Crimson", "#DC143C"),
    ("Cyan", "#00FFFF"),
    ("DarkBlue", "#00008B"),
    ("DarkCyan", "#008B8B"),
    ("DarkGoldenRod", "#B8860B"),
    ("DarkGray", "#A9A9A9"),
    ("DarkGrey", "#A9A9A9"),
    ("DarkGreen", "#006400"),
    ("DarkKhaki", "#BDB76B"),
    ("DarkMagenta", "#8B008B"),
    ("DarkOliveGreen", "#556B2F"),
    ("Darkorange", "#FF8C00"),
    ("DarkOrchid", "#9932CC"),
    ("DarkRed", "#8B0000"),
    ("DarkSalmon", "#E9967A"),
    ("DarkSeaGreen", "#8FBC8F"),
    ("DarkSlateBlue", "#483D8B"),
    ("DarkSlateGray", "#2F4F4F"),
    ("DarkSlateGrey", "#2F4F4F"),
    ("DarkTurquoise", "#00CED1"),
    ("DarkViolet", "#9400D3"),
    ("DeepPink", "#FF1493"),
    ("DeepSkyBlue", "#00BFFF"),
    ("DimGray", "#696969"),
    ("DimGrey", "#696969"),
    ("DodgerBlue", "#1E90FF"),
    ("FireBrick", "#B22222"),
    ("FloralWhite", "#FFFAF0"),
    ("ForestGreen", "#228B22"),
    ("Fuchsia", "#FF00FF"),
    ("Gainsboro", "#DCDCDC"),
    ("GhostWhite", "#F8F8FF"),
    ("Gold", "#FFD700"),
    ("GoldenRod", "#DAA520"),
    ("Gray", "#808080"),
    ("Grey", "#808080"),
    ("Green", "#008000"),
    ("GreenYellow", "#ADFF2F"),
    ("HoneyDew", "#F0FFF0"),
    ("HotPink", "#FF69B4"),
    ("IndianRed", "#CD5C5C"),
    ("Indigo", "#4B0082"),
    ("Ivory", "#FFFFF0"),
    ("Khaki", "#F0E68C"),
    ("Lavender", "#E6E6FA"),
    ("LavenderBlush", "#FFF0F5"),
    ("LawnGreen", "#7CFC00"),
    ("LemonChiffon", "#FFFACD"),
    ("LightBlue", "#ADD8E6"),
    ("LightCoral", "#F08080"),
    ("LightCyan", "#E0FFFF"),
    ("LightGoldenRodYellow", "#FAFAD2"),
    ("LightGray", "#D3D3D3"),
    ("LightGrey", "#D3D3D3"),
    ("LightGreen", "#90EE90"),
    ("LightPink", "#FFB6C1"),
    ("LightSalmon", "#FFA07A"),
    ("LightSeaGreen", "#20B2AA"),
    ("LightSkyBlue", "#87CEFA"),
    ("LightSlateGray", "#778899"),
    ("LightSlateGrey", "#778899"),
    ("LightSteelBlue", "#B0C4DE"),
    ("LightYellow", "#FFFFE0"),
    ("Lime", "#00FF00"),
    ("LimeGreen", "#32CD32"),
    ("Linen", "#FAF0E6"),
    ("Magenta", "#FF00FF"),
    ("Maroon", "#800000"),
    ("MediumAquaMarine", "#66CDAA"),
    ("MediumBlue", "#0000CD"),
    ("MediumOrchid", "#BA55D3"),
    ("MediumPurple", "#9370DB"),
    ("MediumSeaGreen", "#3CB371"),
    ("MediumSlateBlue", "#7B68EE"),
    ("MediumSpringGreen", "#00FA9A"),
    ("MediumTurquoise", "#48D1CC"),
    ("MediumVioletRed", "#C71585"),
    ("MidnightBlue", "#191970"),
    ("MintCream", "#F5FFFA"),
    ("MistyRose", "#FFE4E1"),
    ("Moccasin", "#FFE4B5"),
    ("NavajoWhite", "#FFDEAD"),
    ("Navy", "#000080"),
    ("OldLace", "#FDF5E6"),
    ("Olive", "#808000"),
    ("OliveDrab", "#6B8E23"),
    ("Orange", "#FFA500"),
    ("OrangeRed", "#FF4500"),
    ("Orchid", "#DA70D6"),
    ("PaleGoldenRod", "#EEE8AA"),
    ("PaleGreen", "#98FB98"),
    ("PaleTurquoise", "#AFEEEE"),
    ("PaleVioletRed", "#DB7093"),
    ("PapayaWhip", "#FFEFD5"),
    ("PeachPuff", "#FFDAB9"),
    ("Peru", "#CD853F"),
    ("Pink", "#FFC0CB"),
    ("Plum", "#DDA0DD"),
    ("PowderBlue", "#B0E0E6"),
    ("Purple", "#800080"),
    ("RebeccaPurple", "#663399"),
    ("Red", "#FF0000"),
    ("RosyBrown", "#BC8F8F"),
    ("RoyalBlue", "#4169E1"),
    ("SaddleBrown", "#8B4513"),
    ("Salmon", "#FA8072"),
    ("SandyBrown", "#F4A460"),
    ("SeaGreen", "#2E8B57"),
    ("SeaShell", "#FFF5EE"),
    ("Sienna", "#A0522D"),
    ("Silver", "#C0C0C0"),
    ("SkyBlue", "#87CEEB"),
    ("SlateBlue", "#6A5ACD"),
    ("SlateGray", "#708090"),
    ("SlateGrey", "#708090"),
    ("Snow", "#FFFAFA"),
    ("SpringGreen", "#00FF7F"),
    ("SteelBlue", "#4682B4"),
    ("Tan", "#D2B48C"),
    ("Teal", "#008080"),
    ("Thistle", "#D8BFD8"),
    ("Tomato", "#FF6347"),
    ("Turquoise", "#40E0D0"),
    ("Violet", "#EE82EE"),
    ("Wheat", "#F5DEB3"),
    ("White", "#FFFFFF"),
    ("WhiteSmoke", "#F5F5F5"),
    ("Yellow", "#FFFF00"),
    ("YellowGreen", "#9ACD32"),
];

/// What the document container answers for a colour name the table does not have
/// (`document_container::resolve_color`, which answers nothing unless a container overrides it).
pub trait ResolveColor {
    fn resolve_color(&self, name: &[u8]) -> Vec<u8>;
}

/// `parse_hash_color(tok, color)`: `<hex-color>` (https://drafts.csswg.org/css-color-4/#typedef-hex-color)
/// of a HASH token.
pub fn parse_hash_color(tok: &CssToken) -> Option<WebColor> {
    if tok.ty != HASH {
        return None;
    }

    let s = tok.str.as_slice();
    let len = s.len();
    if !matches!(len, 3 | 4 | 6 | 8) {
        return None;
    }
    // The C++ passes each byte as a char: one past 0x7f is negative, no hex digit.
    if !s.iter().all(|&ch| is_hex_digit(i32::from(ch))) {
        return None;
    }

    let (r, g, b, a) = if len == 3 || len == 4 {
        ([s[0], s[0]], [s[1], s[1]], [s[2], s[2]], if len == 4 { [s[3], s[3]] } else { [b'f', b'f'] })
    } else {
        ([s[0], s[1]], [s[2], s[3]], [s[4], s[5]], if len == 8 { [s[6], s[7]] } else { [b'f', b'f'] })
    };
    let read_two_hex_digits = |str: [u8; 2]| (16 * digit_value(i32::from(str[0])) + digit_value(i32::from(str[1]))) as u8;

    Some(WebColor::rgba(read_two_hex_digits(r), read_two_hex_digits(g), read_two_hex_digits(b), read_two_hex_digits(a)))
}

/// `clamp(x, min, max)`: `min` for NaN.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn clamp(x: f32, min: f32, max: f32) -> f32 {
    if !(x >= min) {
        return min;
    }
    if x > max {
        return max;
    }
    x
}

const NONE_VAL: [&str; 1] = split_css_values::<1>("none");

/// `parse_modern_syntax(tokens, is_hsl, x, y, z, a)`: `[ <number> | <percentage> | none ]{3}
/// [ / [<alpha-value> | none] ]?`, a `none` made 0 (https://drafts.csswg.org/css-color-4/#missing). The
/// lengths are written as the C++ writes them, also when it then fails.
pub fn parse_modern_syntax(
    names: &Names,
    tokens: &[CssToken],
    is_hsl: bool,
    x: &mut CssLength,
    y: &mut CssLength,
    z: &mut CssLength,
    a: &mut CssLength,
) -> bool {
    let n = tokens.len();
    let none_val = CssValues::from(&NONE_VAL);
    if n != 3 && n != 5 {
        return false;
    }
    if is_hsl {
        // [<hue> | none] [<number> | <percentage> | none]{2} [ / [<alpha-value> | none] ]?
        // <hue> = <number> | <angle>
        if !x.from_token(&tokens[0], f_number, none_val) {
            let Some(hue) = parse_angle(names, &tokens[0], false) else { return false };
            x.set_value(hue, CssUnits::css_units_none);
        }
    } else if !x.from_token(&tokens[0], f_number | f_percentage, none_val) {
        return false;
    }
    if !y.from_token(&tokens[1], f_number | f_percentage, none_val) {
        return false;
    }
    if !z.from_token(&tokens[2], f_number | f_percentage, none_val) {
        return false;
    }
    if n == 5 {
        if tokens[3].ch() != i32::from(b'/') {
            return false;
        }
        // <alpha-value> = <number> | <percentage>
        if !a.from_token(&tokens[4], f_number | f_percentage, none_val) {
            return false;
        }
    }
    // convert nones to zeros
    for t in [x, y, z, a] {
        if t.is_predefined() {
            t.set_value(0.0, CssUnits::css_units_none);
        }
    }
    true
}

/// `calc_percent_and_clamp(val, max)` (https://drafts.csswg.org/css-color-4/#rgb-functions): a % of `max`,
/// clamped to [0, max] and rounded; a 0..1 alpha scaled to 255 first.
pub fn calc_percent_and_clamp(val: &CssLength, max: f32) -> u8 {
    let mut x = val.val();
    if val.units() == CssUnits::css_units_percentage {
        x = (x / 100.0) * max;
    }
    x = clamp(x, 0.0, max);
    cxx::f32_to_byte((if Px(max) == Px(1.0) { x * 255.0 } else { x }).round())
}

/// `parse_rgb_func(tok, color)`: `rgb()` and `rgba()` (https://drafts.csswg.org/css-color-4/#rgb-functions).
pub fn parse_rgb_func(names: &Names, tok: &CssToken) -> Option<WebColor> {
    if tok.ty != CV_FUNCTION || !matches!(&lowcase(tok.name())[..], b"rgb" | b"rgba") {
        return None;
    }

    let list = parse_comma_separated_list(&tok.value);
    let n = list.len();
    if !matches!(n, 1 | 3 | 4) {
        return None;
    }

    let mut r = CssLength::default();
    let mut g = CssLength::default();
    let mut b = CssLength::default();
    let mut a = CssLength::new(1.0, CssUnits::css_units_none);
    // legacy syntax: <percentage>#{3} , <alpha-value>? | <number>#{3} , <alpha-value>?
    if n != 1 {
        if list.iter().any(|item| item.len() != 1) {
            return None;
        }
        let ty = list[0][0].ty;
        if ty != PERCENTAGE && ty != NUMBER {
            return None;
        }
        let options = if ty == PERCENTAGE { f_percentage } else { f_number };
        if !r.from_token(&list[0][0], options, CssValues::default()) {
            return None;
        }
        if !g.from_token(&list[1][0], options, CssValues::default()) {
            return None;
        }
        if !b.from_token(&list[2][0], options, CssValues::default()) {
            return None;
        }
        // <alpha-value> = <number> | <percentage>
        if n == 4 && !a.from_token(&list[3][0], f_number | f_percentage, CssValues::default()) {
            return None;
        }
    }
    // modern syntax:  [ <number> | <percentage> | none ]{3}  [ / [<alpha-value> | none] ]?
    else if !parse_modern_syntax(names, &tok.value, false, &mut r, &mut g, &mut b, &mut a) {
        return None;
    }

    Some(WebColor::rgba(
        calc_percent_and_clamp(&r, 255.0),
        calc_percent_and_clamp(&g, 255.0),
        calc_percent_and_clamp(&b, 255.0),
        calc_percent_and_clamp(&a, 1.0),
    ))
}

/// `hsl_to_rgb(hue, sat, light, r, g, b)` (https://drafts.csswg.org/css-color-4/#hsl-to-rgb), each channel
/// 0..1 for a saturation and lightness of 0..100 (and past it outside).
pub fn hsl_to_rgb(hue: f32, sat: f32, light: f32) -> (f32, f32, f32) {
    let mut hue = hue % 360.0;

    if hue < 0.0 {
        hue += 360.0;
    }

    let sat = sat / 100.0;
    let light = light / 100.0;

    let f = |n: f32| {
        let k = (n + hue / 30.0) % 12.0;
        let a = sat * cxx::min_f32(light, 1.0 - light);
        light - a * cxx::max_f32(-1.0, cxx::min_list_f32(&[k - 3.0, 9.0 - k, 1.0]))
    };

    (f(0.0), f(8.0), f(4.0))
}

/// `parse_hsl_func(tok, color)`: `hsl()` and `hsla()` (https://drafts.csswg.org/css-color-4/#the-hsl-notation).
pub fn parse_hsl_func(names: &Names, tok: &CssToken) -> Option<WebColor> {
    if tok.ty != CV_FUNCTION || !matches!(&lowcase(tok.name())[..], b"hsl" | b"hsla") {
        return None;
    }

    let list = parse_comma_separated_list(&tok.value);
    let n = list.len();
    if !matches!(n, 1 | 3 | 4) {
        return None;
    }

    let mut h = CssLength::default();
    let mut s = CssLength::default();
    let mut l = CssLength::default();
    let mut a = CssLength::new(1.0, CssUnits::css_units_none);
    // legacy syntax: <hue>, <percentage>, <percentage>, <alpha-value>?
    if n != 1 {
        if list.iter().any(|item| item.len() != 1) {
            return None;
        }
        let tok0 = &list[0][0];
        // <hue> = <number> | <angle>; a number is degrees (https://drafts.csswg.org/css-color-4/#typedef-hue)
        let hue = if tok0.ty == NUMBER { tok0.n.number } else { parse_angle(names, tok0, false)? };
        h.set_value(hue, CssUnits::css_units_none);

        if !s.from_token(&list[1][0], f_percentage, CssValues::default()) {
            return None;
        }
        if !l.from_token(&list[2][0], f_percentage, CssValues::default()) {
            return None;
        }
        if n == 4 && !a.from_token(&list[3][0], f_number | f_percentage, CssValues::default()) {
            return None;
        }
    }
    // modern syntax:  [<hue> | none] [<percentage> | <number> | none]{2} [ / [<alpha-value> | none] ]?
    else if !parse_modern_syntax(names, &tok.value, true, &mut h, &mut s, &mut l, &mut a) {
        return None;
    }

    let hue = h.val();
    // no percent calculation needed for sat and lit because 0% ~ 0, 100% ~ 100
    let mut sat = s.val();
    let lit = l.val();
    // For historical reasons, if the saturation is less than 0% it is clamped to 0% at parsed-value time,
    // before being converted to an sRGB color.
    if sat < 0.0 {
        sat = 0.0;
    }

    // Saturation is not clamped at 100 nor lightness at all, as the standard has it: the channels may fall
    // outside [0, 1] before the clamp.
    let (r, g, b) = hsl_to_rgb(hue, sat, lit);

    let r = clamp(r, 0.0, 1.0);
    let g = clamp(g, 0.0, 1.0);
    let b = clamp(b, 0.0, 1.0);

    let channel = |v: f32| cxx::f32_to_byte((v * 255.0).round());
    Some(WebColor::rgba(channel(r), channel(g), channel(b), calc_percent_and_clamp(&a, 1.0)))
}

/// `parse_func_color(tok, color)` (https://drafts.csswg.org/css-color-5/#typedef-color-function).
pub fn parse_func_color(names: &Names, tok: &CssToken) -> Option<WebColor> {
    parse_rgb_func(names, tok).or_else(|| parse_hsl_func(names, tok))
}

/// `resolve_name(name, container)`: what a colour name stands for, the table's first match regardless of
/// ASCII case, else the container's answer, else nothing.
pub fn resolve_name(name: &[u8], container: Option<&dyn ResolveColor>) -> Vec<u8> {
    for (clr_name, rgb) in DEF_COLORS {
        if equal_i(name, clr_name.as_bytes()) {
            return rgb.as_bytes().to_vec();
        }
    }

    if let Some(container) = container {
        return container.resolve_color(name);
    }

    Vec::new()
}
