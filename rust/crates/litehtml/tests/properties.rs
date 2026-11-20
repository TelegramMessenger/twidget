//! The Rust `style.cpp` and the value parsers of `css_effects.cpp` against the fork's (`oracle/lhdump`,
//! records `f` and `y`): declaration blocks through `style::add` of text (with the declaration cache) and of
//! tokens, single declarations through `add_property`, every property each leaves with its typed value,
//! `!important`, order and `var()` flag, the name table's size after it; and the values a style keeps as
//! tokens through the parsers the computed style runs on them, with `fold_math` on every value. The
//! declarations: widget-sandbox's corpus (style blocks and attributes), litehtml's master CSS and the
//! renderer's UA layers, generated declarations of every property the fork knows, and random changes of
//! them, through four containers (none, lhdump's, the renderer's sanitizing one, one that resolves colour
//! names). Skipped without `TWIDGET_ORACLE_DIR`.

mod common;

use std::collections::BTreeMap;

use common::*;
use twidget_litehtml::Names;
use twidget_litehtml::css_parser::{f_componentize, f_remove_whitespace, normalize, parse_stylesheet, parse_stylesheet_tokens};
use twidget_litehtml::css_tokenizer::*;
use twidget_litehtml::css_values::CssUnits;
use twidget_litehtml::gradient::ImageType;
use twidget_litehtml::style::{Style, Value};
use twidget_litehtml::stylesheet::RuleType;

/// Every property name style.cpp reads: its switch's, its shorthands, the names it renames, the raw-declared
/// ones and their twins, and a few it does not know.
const PROPERTIES: &[&str] = &[
    "overflow",
    "display",
    "visibility",
    "position",
    "float",
    "clear",
    "appearance",
    "resize",
    "box-sizing",
    "overflow-x",
    "overflow-y",
    "text-align",
    "direction",
    "vertical-align",
    "text-transform",
    "-webkit-font-smoothing",
    "white-space",
    "overflow-wrap",
    "word-break",
    "user-select",
    "font-style",
    "font-variant",
    "text-decoration-style",
    "list-style-type",
    "list-style-position",
    "border-top-style",
    "border-bottom-style",
    "border-left-style",
    "border-right-style",
    "border-collapse",
    "table-layout",
    "outline-style",
    "flex-direction",
    "flex-wrap",
    "justify-content",
    "align-content",
    "caption-side",
    "z-index",
    "text-indent",
    "padding-left",
    "padding-right",
    "padding-top",
    "padding-bottom",
    "left",
    "right",
    "top",
    "bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "margin-bottom",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "line-height",
    "font-size",
    "margin",
    "padding",
    "inset",
    "color",
    "background-color",
    "border-top-color",
    "border-bottom-color",
    "border-left-color",
    "border-right-color",
    "background",
    "background-image",
    "background-position",
    "background-size",
    "background-repeat",
    "background-attachment",
    "background-origin",
    "background-clip",
    "border",
    "border-left",
    "border-right",
    "border-top",
    "border-bottom",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-bottom-width",
    "border-left-width",
    "border-right-width",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-top-right-radius",
    "border-top-left-radius",
    "border-radius-x",
    "border-radius-y",
    "border-radius",
    "border-spacing",
    "list-style-image",
    "list-style",
    "font",
    "font-family",
    "font-weight",
    "text-decoration",
    "text-decoration-thickness",
    "text-decoration-color",
    "text-decoration-line",
    "text-emphasis",
    "text-emphasis-style",
    "font-variant-numeric",
    "font-feature-settings",
    "text-emphasis-color",
    "text-emphasis-position",
    "flex",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "flex-flow",
    "align-items",
    "align-self",
    "order",
    "gap",
    "grid-gap",
    "row-gap",
    "column-gap",
    "grid-row-gap",
    "grid-column-gap",
    "justify-items",
    "justify-self",
    "grid",
    "grid-template",
    "place-items",
    "place-self",
    "place-content",
    "aspect-ratio",
    "opacity",
    "letter-spacing",
    "word-spacing",
    "contain",
    "backface-visibility",
    "color-scheme",
    "accent-color",
    "outline",
    "outline-width",
    "outline-color",
    "outline-offset",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "text-anchor",
    "box-shadow",
    "text-shadow",
    "filter",
    "backdrop-filter",
    "transform",
    "translate",
    "rotate",
    "scale",
    "transform-origin",
    "transform-box",
    "clip-path",
    "grid-template-columns",
    "grid-template-rows",
    "grid-auto-rows",
    "grid-column",
    "grid-row",
    "grid-column-start",
    "grid-column-end",
    "grid-row-start",
    "grid-row-end",
    "counter-increment",
    "counter-reset",
    "content",
    "cursor",
    "word-wrap",
    "-webkit-user-select",
    "-webkit-backdrop-filter",
    "animation",
    "animation-name",
    "animation-duration",
    "animation-timing-function",
    "animation-delay",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "transition",
    "transition-property",
    "transition-duration",
    "transition-timing-function",
    "transition-delay",
    "pointer-events",
    "touch-action",
    "quotes",
    "object-fit",
    "object-position",
    "text-overflow",
    "-lh-raw-opacity",
    "-lh-raw-color",
    "--x",
    "--y-z",
    "--",
    "foo-bar",
    "Width",
];

const KINDS: [&str; 14] = [
    "invalid",
    "inherit",
    "css-wide keyword",
    "int",
    "int vector",
    "length",
    "length vector",
    "float",
    "color",
    "images",
    "string",
    "strings",
    "sizes",
    "tokens",
];

/// One lhdump run of declaration cases, with what they reached.
struct StyleRun {
    lh: Lh,
    c: Comparison,
    counts: BTreeMap<String, usize>,
    properties: BTreeMap<String, BTreeMap<&'static str, usize>>,
}

impl StyleRun {
    fn new() -> StyleRun {
        StyleRun { lh: Lh::new(), c: Comparison::new(), counts: BTreeMap::new(), properties: BTreeMap::new() }
    }

    fn count(&mut self, what: impl Into<String>, n: usize) {
        *self.counts.entry(what.into()).or_default() += n;
    }

    /// An `f` case, answered and tallied now: the Rust side keeps the name table and the cache in the order
    /// lhdump meets the cases.
    fn style(&mut self, container: u8, entry: u8, baseurl: &[u8], name: &[u8], text: &[u8], what: impl Into<String>) {
        let (text, _) = without_overlong_ascii(text);
        let input = style_input(container, entry, baseurl, name, &text);
        let (st, answer) = self.lh.style_case(&input);
        self.tally(&st);
        self.count(
            format!("entry {}", ["add(text)", "add(tokens)", "add_property", "add_property !important"][usize::from(entry.min(3))]),
            1,
        );
        self.c.case(&framed(b'f', &input), answer, what);
    }

    /// A `y` case.
    fn effects(&mut self, which: u8, container: u8, text: &[u8], what: impl Into<String>) {
        let (text, _) = without_overlong_ascii(text);
        let input = effects_input(which, container, &text);
        let answer = self.lh.effects_answer(&input);
        let parser = [
            "box-shadow",
            "filter",
            "transform",
            "transform-origin",
            "clip-path",
            "grid track list",
            "grid placement",
            "fast transform",
            "fold_math",
        ][usize::from(which.min(8))];
        self.count(format!("{parser} {}", if answer.0 == 1 { "parsed" } else { "refused" }), 1);
        self.c.case(&framed(b'y', &input), answer, what);
    }

    /// A declaration every way: `add_property` and `!important`, a block of text and of tokens, the
    /// containers taken in turn.
    fn declaration(&mut self, name: &[u8], value: &[u8], k: usize, what: &str) {
        let container = (k % 4) as u8;
        self.style(container, 2 + u8::from(k.is_multiple_of(5)), b"", name, value, format!("add_property {what}"));
        let text = [name, b": ", value].concat();
        match k % 3 {
            0 => self.style((container + 1) % 4, 0, b"", b"", &text, format!("add(text) {what}")),
            1 => self.style((container + 2) % 4, 1, if k % 7 == 1 { b"base/" } else { b"" }, b"", &text, format!("add(tokens) {what}")),
            _ => {}
        }
    }

    fn tally(&mut self, st: &Style) {
        if st.empty() {
            self.count("styles empty", 1);
        }
        for (&id, p) in st.properties() {
            let name = String::from_utf8_lossy(&self.lh.names.s(id)).into_owned();
            let kind = KINDS[usize::from(p.value.index())];
            *self.properties.entry(name).or_default().entry(kind).or_default() += 1;
            self.count(format!("kind {kind}"), 1);
            if p.m_important {
                self.count("!important kept", 1);
            }
            if p.m_has_var {
                self.count("waiting for var()", 1);
            }
            match &p.value {
                Value::Length(len) => {
                    let form = if len.is_predefined() {
                        "keyword".to_string()
                    } else if len.units() == CssUnits::css_units_calc {
                        "calc".to_string()
                    } else {
                        format!("{:?}", len.units()).replace("css_units_", "")
                    };
                    self.count(format!("length {form}"), 1);
                }
                Value::Images(images) => {
                    for img in images {
                        let form = match img.ty {
                            ImageType::type_none => "none".to_string(),
                            ImageType::type_url => "url".to_string(),
                            ImageType::type_gradient => String::from_utf8_lossy(&self.lh.names.s(img.m_gradient.m_type)).into_owned(),
                        };
                        self.count(format!("image {form}"), 1);
                    }
                }
                Value::CssWideKeyword(kind) => self.count(format!("keyword {kind:?}"), 1),
                Value::Color(c) if c.is_current_color => self.count("color currentcolor", 1),
                _ => {}
            }
        }
    }

    fn check(self, name: &str) {
        let Some(oracle) = Oracle::find("lhdump") else { return };
        let total = self.c.len();
        let tally = self.c.check(&oracle);
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("{name}: {total} answers alike ({} of status 1, {} of 0); {}", tally[1], tally[0], counts.join(", "));
        if self.properties.is_empty() {
            return;
        }
        let properties: Vec<String> = self
            .properties
            .iter()
            .map(|(p, kinds)| format!("{p} [{}]", kinds.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")))
            .collect();
        eprintln!("{name}: {} properties held: {}", properties.len(), properties.join("; "));
    }
}

// ---- Generators -------------------------------------------------------------------------------------------

fn pick_str<'a>(random: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[random.below(from.len())]
}

fn number(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => "0".into(),
        1 => format!("-{}", random.below(50)),
        2 => format!("{}.{}", random.below(20), random.below(100)),
        3 => format!(".{}", random.below(1000)),
        4 => pick_str(random, &["1e3", "2E-2", "+4", "1000", "1001", "65536", "3000000000", "0.00001", "1e40", "0e999", "-0"]).into(),
        _ => format!("{}", random.below(400)),
    }
}

fn unit(random: &mut Lcg) -> &'static str {
    if random.chance(5) {
        return pick_str(random, &["fr", "deg", "rad", "turn", "grad", "s", "ms", "x", "Q", "PX", "dpi", "foo", "svh", "dvw", "lvmax"]);
    }
    pick_str(
        random,
        &["px", "px", "px", "px", "em", "rem", "%", "%", "%", "vw", "vh", "vmin", "vmax", "pt", "pc", "cm", "mm", "in", "ch", "ex"],
    )
}

fn length(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => "0".into(),
        1 => number(random),
        2 => math(random, 0),
        _ => format!("{}{}", number(random), unit(random)),
    }
}

fn color(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => pick_str(
            random,
            &[
                "red",
                "Blue",
                "transparent",
                "currentcolor",
                "currentColor",
                "black",
                "white",
                "rebeccapurple",
                "brand",
                "Accentx",
                "alias",
                "twotok",
                "nope",
            ],
        )
        .into(),
        1 => format!("#{:03x}", random.below(0x1000)),
        2 => format!("#{:06x}", random.next() & 0xff_ffff),
        3 => format!("#{:08x}", random.next()),
        4 => format!("rgb({}, {}, {})", random.below(300), random.below(256), random.below(256)),
        5 => format!("rgba({} {} {} / {})", random.below(256), random.below(256), random.below(256), number(random)),
        6 => format!("hsl({}deg {}% {}%)", random.below(400), random.below(120), random.below(100)),
        7 => format!("hsla({}, {}%, {}%, .{})", random.below(360), random.below(100), random.below(100), random.below(10)),
        8 => format!("color-mix(in srgb, {} {}%, {})", color_simple(random), random.below(120), color_simple(random)),
        _ => color_simple(random),
    }
}

fn color_simple(random: &mut Lcg) -> String {
    pick_str(
        random,
        &["red", "#123", "rgb(1 2 3)", "black", "#000000", "white", "transparent", "currentcolor", "hsl(120 50% 50%)", "#8080807f"],
    )
    .into()
}

fn position(random: &mut Lcg) -> String {
    let side = |random: &mut Lcg| -> String {
        if random.chance(2) { pick_str(random, &["left", "right", "top", "bottom", "center"]).into() } else { length(random) }
    };
    if random.chance(3) { side(random) } else { format!("{} {}", side(random), side(random)) }
}

fn gradient(random: &mut Lcg) -> String {
    let kind = pick_str(
        random,
        &[
            "linear-gradient",
            "repeating-linear-gradient",
            "radial-gradient",
            "repeating-radial-gradient",
            "conic-gradient",
            "repeating-conic-gradient",
            "Linear-Gradient",
        ],
    );
    let mut args = Vec::new();
    if random.chance(2) {
        args.push(match random.below(10) {
            0 => format!("{}deg", number(random)),
            1 => format!("to {}", pick_str(random, &["left", "right", "top", "bottom"])),
            2 => format!(
                "to {} {}",
                pick_str(random, &["left", "right", "top", "bottom"]),
                pick_str(random, &["top", "bottom", "left", "right"])
            ),
            3 => format!(
                "in {}",
                pick_str(random, &["srgb", "oklab", "hsl", "lch longer hue", "oklch increasing hue", "xyz", "hwb shorter"])
            ),
            4 => format!("circle at {}", position(random)),
            5 => format!(
                "{} {}",
                pick_str(random, &["ellipse", "circle", ""]),
                pick_str(random, &["closest-side", "farthest-corner", "closest-corner", "farthest-side"])
            ),
            6 => format!("{} {}", length(random), length(random)),
            7 => format!("from {} at {}", pick_str(random, &["90deg", "0", "0.5turn", "10%"]), position(random)),
            8 => format!("at {}", position(random)),
            _ => format!("{} in srgb", pick_str(random, &["circle 10px", "ellipse 10% 20%", "45deg", "to right", "from 10deg"])),
        });
    }
    let stop_position = |random: &mut Lcg| -> String {
        match random.below(8) {
            0 => length(random),
            1 => format!("{}deg", random.below(400)),
            2 => format!("{}turn", random.below(2)),
            3 => format!("{}px", random.below(200)),
            4 => "0".into(),
            _ => format!("{}%", random.below(101)),
        }
    };
    let stops = if random.chance(8) { 1 } else { 2 + random.below(3) };
    for _ in 0..stops {
        args.push(match random.below(7) {
            0 => format!("{} {}", color(random), stop_position(random)),
            1 => format!("{} {} {}", color(random), stop_position(random), stop_position(random)),
            2 => stop_position(random),
            _ => color(random),
        });
    }
    format!("{kind}({})", args.join(", "))
}

fn image(random: &mut Lcg) -> String {
    match random.below(5) {
        0 => "none".into(),
        1 => pick_str(random, &["url(a.png)", "url( 'b c.png' )", "url(\"x\")", "src(\"y\")", "url()"]).into(),
        _ => gradient(random),
    }
}

/// Math the stylesheet folds, or keeps for layout, or refuses.
fn math(random: &mut Lcg, depth: u32) -> String {
    let leaf = |random: &mut Lcg| -> String {
        match random.below(5) {
            0 => number(random),
            1 => format!("{}%", random.below(100)),
            _ => format!("{}{}", number(random), pick_str(random, &["px", "em", "vw", "rem", "%", "deg"])),
        }
    };
    let inner = |random: &mut Lcg| if depth < 3 && random.chance(3) { math(random, depth + 1) } else { leaf(random) };
    match random.below(7) {
        0 => format!("calc({} {} {})", inner(random), pick_str(random, &["+", "-", "*", "/"]), inner(random)),
        1 => format!("min({}, {})", inner(random), inner(random)),
        2 => format!("max({}, {}, {})", inner(random), inner(random), inner(random)),
        3 => format!("clamp({}, {}, {})", inner(random), inner(random), inner(random)),
        4 => format!("calc(({} + {}) * {})", inner(random), inner(random), number(random)),
        5 => pick_str(
            random,
            &["env(safe-area-inset-top)", "env(keyboard-inset-height)", "env(x, 4px)", "env(y, 1px 2px)", "env(safe-area-max-inset-left)"],
        )
        .into(),
        _ => format!("calc({})", inner(random)),
    }
}

fn keyword(random: &mut Lcg) -> String {
    let table = keywords(random.below(52) as u8);
    if !table.data.is_empty() && random.chance(2) {
        return table.data[random.below(table.data.len())].to_string();
    }
    pick_str(
        random,
        &[
            "auto",
            "none",
            "normal",
            "inherit",
            "initial",
            "unset",
            "revert",
            "span",
            "auto-flow",
            "dense",
            "first",
            "last",
            "safe",
            "unsafe",
            "baseline",
            "inset",
            "from",
            "at",
            "to",
            "in",
            "hue",
            "on",
            "off",
            "light",
            "dark",
            "hidden",
            "visible",
            "strict",
            "content",
            "size",
            "inline-size",
            "layout",
            "paint",
            "style",
            "currentcolor",
            "default",
            "Auto",
            "NONE",
            "fit-content",
            "min-content",
            "max-content",
            "auto-fill",
            "auto-fit",
            "evenodd",
            "nonzero",
            "round",
        ],
    )
    .into()
}

fn function(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => format!("translate({}, {})", length(random), length(random)),
        1 => format!(
            "{}({})",
            pick_str(random, &["rotate", "rotateZ", "rotateX", "rotateY", "skewX", "skewY"]),
            pick_str(random, &["45deg", "0", "1rad", ".5turn", "100grad", "calc(10deg + 5deg)", "1"])
        ),
        2 => format!("{}({})", pick_str(random, &["scale", "scaleX", "scaleY", "scale3d"]), number(random)),
        3 => "matrix(1, 0, 0, 1, 10, 20)".into(),
        4 => format!("rotate3d({}, {}, {}, 30deg)", random.below(2), random.below(2), random.below(2)),
        5 => format!(
            "{}({})",
            pick_str(random, &["blur", "brightness", "contrast", "grayscale", "invert", "opacity", "saturate", "sepia", "hue-rotate"]),
            pick_str(random, &["", "2px", "50%", "1.5", "calc(1px + 2px)", "-1", "90deg", "0", "2"])
        ),
        6 => format!("drop-shadow({} {} {} {})", length(random), length(random), length(random), color(random)),
        7 => format!(
            "var(--{}{})",
            pick_str(random, &["x", "y", "a-b", ""]),
            if random.chance(2) { format!(", {}", length(random)) } else { String::new() }
        ),
        8 => format!(
            "minmax({}, {})",
            pick_str(random, &["10px", "auto", "min-content", "1fr", "20%"]),
            pick_str(random, &["1fr", "max-content", "100px", "auto", "-1fr"])
        ),
        9 => format!(
            "repeat({}, {})",
            pick_str(random, &["3", "auto-fill", "auto-fit", "0", "1001", "2.5"]),
            pick_str(random, &["1fr", "100px 1fr", "[a] 10px", "minmax(10px, 1fr)"])
        ),
        10 => format!("fit-content({})", length(random)),
        _ => pick_str(random, &["perspective(100px)", "translateZ(1px)", "translate3d(1px, 2px, 3px)", "foo(1)", "attr(x)", "counter(c)"])
            .into(),
    }
}

fn filter_function(random: &mut Lcg) -> String {
    let amount = |random: &mut Lcg| {
        pick_str(random, &["", "0", "1", "2", "50%", "150%", ".5", "-1", "1e40", "calc(1 + 1)", "calc(10% * 2)", "1px"]).to_string()
    };
    match random.below(10) {
        0 => format!("blur({})", pick_str(random, &["", "2px", "0", "1em", "-1px", "calc(1px + 1px)", "50%"])),
        1 => format!("hue-rotate({})", pick_str(random, &["", "90deg", "0", "1turn", "calc(45deg * 2)", "1", "2rad"])),
        2 => format!(
            "drop-shadow({})",
            pick_str(
                random,
                &["1px 2px", "1px 2px 3px red", "red 0 0", "0 0 -1px", "1px", "1px 2px 3px 4px", "blue 1px 2px 3px", "1px red 2px"]
            )
        ),
        _ => format!(
            "{}({})",
            pick_str(random, &["brightness", "contrast", "grayscale", "invert", "opacity", "saturate", "sepia"]),
            amount(random)
        ),
    }
}

/// Any one component of a value.
fn piece(random: &mut Lcg) -> String {
    match random.below(12) {
        0 | 1 => keyword(random),
        2 | 3 => length(random),
        4 => number(random),
        5 | 6 => color(random),
        7 => image(random),
        8 => function(random),
        9 => pick_str(random, &["/", ",", "\"abcd\"", "'Helvetica Neue'", "\"liga\"", "url(x)", "[name]", "!", "a b"]).into(),
        10 => math(random, 0),
        _ => format!("{}", random.below(5)),
    }
}

fn shuffled_join(random: &mut Lcg, mut parts: Vec<String>) -> String {
    for i in (1..parts.len()).rev() {
        parts.swap(i, random.below(i + 1));
    }
    parts.join(" ")
}

/// Some of the parts, which are made before the picking borrows the generator.
macro_rules! some_of {
    ($random:expr, [$($e:expr),* $(,)?]) => {{
        let parts: Vec<String> = vec![$($e),*];
        pick_some($random, parts)
    }};
}

/// Some of the parts, joined in any order.
macro_rules! some_shuffled {
    ($random:expr, [$($e:expr),* $(,)?]) => {{
        let parts = some_of!($random, [$($e),*]);
        shuffled_join($random, parts)
    }};
}

fn pick_some(random: &mut Lcg, parts: Vec<String>) -> Vec<String> {
    parts.into_iter().filter(|_| random.below(3) != 0).collect()
}

/// A value for the property: one in its grammar most of the time, any components now and then.
fn any_value(random: &mut Lcg, property: &str) -> String {
    if random.chance(4) {
        let n = 1 + random.below(4);
        return (0..n).map(|_| piece(random)).collect::<Vec<_>>().join(" ");
    }
    let style = || ["none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset", "outset"];
    match property {
        "background" => {
            let layers = 1 + random.below(3);
            let mut out = Vec::new();
            for l in 0..layers {
                let mut parts = vec![
                    image(random),
                    position(random),
                    pick_str(random, &["repeat", "no-repeat", "repeat-x", "space"]).into(),
                    pick_str(random, &["scroll", "fixed", "local"]).into(),
                    pick_str(random, &["border-box", "padding-box", "content-box"]).into(),
                    pick_str(random, &["border-box", "padding-box", "content-box"]).into(),
                ];
                if random.chance(3) {
                    parts[1] = format!(
                        "{} / {}",
                        position(random),
                        pick_str(random, &["cover", "contain", "auto", "10px auto", "50%", "auto 20px", "-1px"])
                    );
                }
                if l == layers - 1 {
                    parts.push(color(random));
                }
                let parts = pick_some(random, parts);
                out.push(shuffled_join(random, parts));
            }
            out.join(", ")
        }
        "background-image" => (0..1 + random.below(3)).map(|_| image(random)).collect::<Vec<_>>().join(", "),
        "background-position" | "object-position" => (0..1 + random.below(2)).map(|_| position(random)).collect::<Vec<_>>().join(", "),
        "background-size" => (0..1 + random.below(2))
            .map(|_| {
                pick_str(random, &["cover", "contain", "auto", "10px", "50% auto", "auto 20%", "1px 2px", "-3px", "auto auto"]).to_string()
            })
            .collect::<Vec<_>>()
            .join(", "),
        "background-repeat" | "background-attachment" | "background-origin" | "background-clip" => (0..1 + random.below(3))
            .map(|_| {
                pick_str(
                    random,
                    &[
                        "repeat",
                        "no-repeat",
                        "repeat-x",
                        "repeat-y",
                        "scroll",
                        "fixed",
                        "local",
                        "border-box",
                        "padding-box",
                        "content-box",
                        "text",
                    ],
                )
                .to_string()
            })
            .collect::<Vec<_>>()
            .join(", "),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" | "outline" => {
            let parts = vec![
                pick_str(random, &["thin", "medium", "thick", "1px", "2.5em", "0", "-1px", "3"]).into(),
                pick_str(random, &style()).into(),
                color(random),
            ];
            let parts = pick_some(random, parts);
            shuffled_join(random, parts)
        }
        "border-width" | "border-style" | "border-color" | "margin" | "padding" | "inset" | "border-radius-x" | "border-radius-y" => {
            let n = 1 + random.below(5);
            (0..n)
                .map(|_| match property {
                    "border-style" => pick_str(random, &style()).to_string(),
                    "border-color" => color(random),
                    "border-width" => pick_str(random, &["thin", "medium", "thick", "1px", "0", "2em", "-1px"]).to_string(),
                    _ => {
                        if random.chance(5) {
                            "auto".into()
                        } else {
                            length(random)
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        "border-radius" => {
            let side = |random: &mut Lcg| (0..1 + random.below(4)).map(|_| length(random)).collect::<Vec<_>>().join(" ");
            if random.chance(2) { side(random) } else { format!("{} / {}", side(random), side(random)) }
        }
        "font" => {
            if random.chance(8) {
                return pick_str(random, &["caption", "menu", "\"icon\"", "status-bar", "Caption"]).into();
            }
            let mut out = some_of!(
                random,
                [
                    pick_str(random, &["italic", "oblique", "normal"]).into(),
                    pick_str(random, &["small-caps", "normal"]).into(),
                    pick_str(random, &["bold", "300", "lighter", "1000", "1001", "normal"]).into(),
                ]
            );
            out.push(pick_str(random, &["12px", "1.5em", "small", "larger", "50%", "0", "-1px", "x"]).into());
            if random.chance(2) {
                out.push(format!("/ {}", pick_str(random, &["1.2", "normal", "20px", "120%", "-1"])));
            }
            out.push(pick_str(random, &["serif", "\"Helvetica Neue\", Arial, sans-serif", "Open Sans", "inherit", "a, , b", ""]).into());
            out.join(" ")
        }
        "font-family" => {
            pick_str(random, &["serif", "\"Helvetica Neue\", Arial, sans-serif", "Open Sans, x", "default", "a, 1", "'x'", "A B C"]).into()
        }
        "font-weight" => pick_str(random, &["bold", "bolder", "100", "1000", "0.5", "1001", "450.5", "normal", "lighter"]).into(),
        "flex" => pick_str(
            random,
            &[
                "1",
                "1 1",
                "1 0 auto",
                "auto 2",
                "none",
                "0",
                "2 3 10%",
                "content",
                "auto",
                "initial",
                "1 2 3",
                "0 0",
                "10px 1",
                "-1",
                "1 1 1 1",
                "0 0 0",
                "fit-content",
            ],
        )
        .into(),
        "flex-flow" => some_shuffled!(
            random,
            [
                pick_str(random, &["row", "column", "row-reverse", "column-reverse"]).into(),
                pick_str(random, &["wrap", "nowrap", "wrap-reverse"]).into(),
            ]
        ),
        "list-style" => some_shuffled!(
            random,
            [
                pick_str(random, &["disc", "decimal", "square", "none"]).into(),
                pick_str(random, &["inside", "outside"]).into(),
                pick_str(random, &["url(a.png)", "none"]).into(),
                "none".into(),
            ]
        ),
        "text-decoration" | "text-emphasis" => some_shuffled!(
            random,
            [
                pick_str(random, &["underline", "overline", "line-through", "none", "blink", "underline overline"]).into(),
                pick_str(random, &["solid", "double", "dotted", "dashed", "wavy"]).into(),
                color(random),
                pick_str(random, &["auto", "from-font", "2px", "10%"]).into(),
                pick_str(random, &["filled", "open", "dot", "'x'", "sesame"]).into(),
            ]
        ),
        "text-decoration-line" | "text-emphasis-position" => some_shuffled!(
            random,
            [
                pick_str(random, &["underline", "overline", "line-through", "none", "over", "under", "left", "right"]).into(),
                pick_str(random, &["underline", "overline", "line-through", "over", "under", "x"]).into(),
            ]
        ),
        "grid" | "grid-template" => pick_str(
            random,
            &[
                "none",
                "auto / 1fr 1fr",
                "auto-flow 10px / 1fr 2fr",
                "auto-flow / repeat(3, 1fr)",
                "auto-flow dense / 1fr",
                "'a' / b",
                "100px 1fr / auto-flow",
                "1fr / 2fr / 3fr",
                "none none",
                "/ 1fr",
                "repeat(2, 10px) / minmax(10px, 1fr)",
            ],
        )
        .into(),
        "grid-template-columns" | "grid-template-rows" | "grid-auto-rows" => {
            let n = 1 + random.below(4);
            (0..n)
                .map(|_| match random.below(4) {
                    0 => function(random),
                    1 => pick_str(random, &["[a]", "1fr", "auto", "min-content", "max-content", "none", "-1fr", "2.5fr"]).into(),
                    _ => length(random),
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        "grid-column" | "grid-row" | "grid-column-start" | "grid-column-end" | "grid-row-start" | "grid-row-end" => pick_str(
            random,
            &["1", "1 / 3", "span 2", "auto", "-1", "2 / span 3", "0", "1.5", "1 / 2 / 3", "span -1", "3000", "auto / auto", "x"],
        )
        .into(),
        "place-items" | "place-self" | "align-items" | "align-self" | "justify-items" | "justify-self" | "place-content"
        | "align-content" | "justify-content" => {
            let n = 1 + random.below(2);
            (0..n)
                .map(|_| {
                    pick_str(
                        random,
                        &[
                            "auto",
                            "normal",
                            "stretch",
                            "baseline",
                            "first",
                            "last",
                            "safe",
                            "unsafe",
                            "center",
                            "start",
                            "end",
                            "self-start",
                            "self-end",
                            "flex-start",
                            "flex-end",
                            "left",
                            "space-between",
                            "space-around",
                            "space-evenly",
                            "1",
                        ],
                    )
                    .to_string()
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
        "aspect-ratio" => pick_str(random, &["16 / 9", "auto 1", "2", "0/1", "auto", "1 / 0", "-1", "4/3 auto", "1 2 3", "1.5"]).into(),
        "overflow" => (0..1 + random.below(3))
            .map(|_| pick_str(random, &["hidden", "visible", "scroll", "auto", "clip", "x"]).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        "contain" => (0..1 + random.below(3))
            .map(|_| pick_str(random, &["none", "strict", "content", "size", "inline-size", "layout", "paint", "style", "x"]).to_string())
            .collect::<Vec<_>>()
            .join(" "),
        "color-scheme" => pick_str(random, &["light", "dark", "light dark", "only dark", "normal", "dark only", "x"]).into(),
        "font-feature-settings" => pick_str(
            random,
            &[
                "\"liga\" 1, \"tnum\" off",
                "normal",
                "\"smcp\"",
                "\"kern\" on",
                "\"abc\" 1",
                "\"liga\" 70000",
                "\"liga\" -1",
                "\"liga\" 1.5",
                "\"x\u{e9}yz\"",
                "\"liga\" 1, \"liga\" 0",
                "\"tnum\" \"liga\"",
            ],
        )
        .into(),
        "font-variant-numeric" => pick_str(
            random,
            &[
                "tabular-nums",
                "normal",
                "lining-nums slashed-zero",
                "ordinal ordinal",
                "diagonal-fractions stacked-fractions oldstyle-nums proportional-nums",
                "x",
            ],
        )
        .into(),
        "box-shadow" | "text-shadow" => (0..1 + random.below(3))
            .map(|_| {
                let parts =
                    some_of!(random, [length(random), length(random), length(random), length(random), color(random), "inset".into()]);
                shuffled_join(random, parts)
            })
            .collect::<Vec<_>>()
            .join(", "),
        "filter" | "backdrop-filter" | "-webkit-backdrop-filter" | "transform" => {
            if random.chance(6) {
                return "none".into();
            }
            let filters = property != "transform";
            (0..1 + random.below(3))
                .map(|_| if filters && random.below(3) != 0 { filter_function(random) } else { function(random) })
                .collect::<Vec<_>>()
                .join(" ")
        }
        "transform-origin" => (0..1 + random.below(3))
            .map(|_| {
                if random.chance(2) { pick_str(random, &["left", "right", "top", "bottom", "center"]).to_string() } else { length(random) }
            })
            .collect::<Vec<_>>()
            .join(" "),
        "clip-path" => pick_str(
            random,
            &[
                "inset(10px)",
                "inset(1px 2px 3px 4px round 5px)",
                "circle(50% at 10px 20%)",
                "ellipse(10px 20px)",
                "polygon(0 0, 100% 0, 50% 100%)",
                "polygon(evenodd, 0 0, 1px 1px, 2px 0)",
                "circle()",
                "inset(1px round -1px)",
                "circle(10px at left top)",
                "polygon(0 0)",
                "none",
            ],
        )
        .into(),
        "counter-increment" | "counter-reset" | "content" | "cursor" | "text-emphasis-style" | "quotes" => pick_str(
            random,
            &[
                "c 1",
                "\"a\" attr(x) \"b\"",
                "none",
                "pointer",
                "url(a.cur) 1 2, auto",
                "counter(c, decimal)",
                "open-quote",
                "'x' 'y'",
                "a  b",
            ],
        )
        .into(),
        "--x" | "--y-z" | "--" => {
            pick_str(random, &["", "1px", "{a}", "!", "a;b", "red)", "var(--y)", " spaced  value ", "[]", "rgb(1 2 3)", "calc(1px + 2px)"])
                .into()
        }
        "opacity" | "fill-opacity" | "stroke-opacity" | "flex-grow" | "flex-shrink" | "order" | "z-index" => {
            pick_str(random, &["0.5", "50%", "2", "-1", "1e3", "0e999", "1.5", "auto", "2147483648", "-3000000000", "x", "10px"]).into()
        }
        "border-spacing"
        | "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-left-radius"
        | "border-bottom-right-radius"
        | "gap"
        | "grid-gap" => {
            (0..1 + random.below(3)).map(|_| if random.chance(6) { "normal".into() } else { length(random) }).collect::<Vec<_>>().join(" ")
        }
        _ => {
            if property.contains("color") || property == "fill" || property == "stroke" {
                return if random.chance(5) { pick_str(random, &["auto", "currentcolor", "none"]).into() } else { color(random) };
            }
            if let Some(table) = keyword_table(property)
                && random.below(4) != 0
            {
                let keyword = table[random.below(table.len())];
                return if random.chance(8) { keyword.to_uppercase() } else { keyword.into() };
            }
            if random.chance(2) { keyword(random) } else { length(random) }
        }
    }
}

/// The keywords a property takes (style.cpp's `m_valid_values` and the length properties' keywords), for
/// the generator.
fn keyword_table(property: &str) -> Option<&'static [&'static str]> {
    use twidget_litehtml::css_values::*;
    Some(match property {
        "display" => &style_display_strings,
        "visibility" => &visibility_strings,
        "position" => &element_position_strings,
        "float" => &element_float_strings,
        "clear" => &element_clear_strings,
        "overflow-x" | "overflow-y" => &overflow_strings,
        "appearance" => &appearance_strings,
        "resize" => &resize_strings,
        "box-sizing" => &box_sizing_strings,
        "text-align" => &text_align_strings,
        "direction" => &direction_strings,
        "vertical-align" => &vertical_align_strings,
        "text-transform" => &text_transform_strings,
        "-webkit-font-smoothing" => &font_smoothing_strings,
        "white-space" => &white_space_strings,
        "overflow-wrap" | "word-wrap" => &overflow_wrap_strings,
        "word-break" => &word_break_strings,
        "user-select" | "-webkit-user-select" => &user_select_strings,
        "font-style" => &font_style_strings,
        "font-variant" => &font_variant_strings,
        "text-decoration-style" => &style_text_decoration_style_strings,
        "list-style-type" => &list_style_type_strings,
        "list-style-position" => &list_style_position_strings,
        "border-top-style" | "border-bottom-style" | "border-left-style" | "border-right-style" | "outline-style" => &border_style_strings,
        "border-collapse" => &border_collapse_strings,
        "table-layout" => &table_layout_strings,
        "flex-direction" => &flex_direction_strings,
        "flex-wrap" => &flex_wrap_strings,
        "caption-side" => &caption_side_strings,
        "font-size" => &font_size_strings,
        "flex-basis" => &flex_basis_strings,
        "text-decoration-thickness" => &style_text_decoration_thickness_strings,
        "border-top-width" | "border-bottom-width" | "border-left-width" | "border-right-width" | "outline-width" => &border_width_strings,
        "width" => &["auto", "min-content", "max-content", "fit-content"],
        "height" | "min-width" | "min-height" | "left" | "right" | "top" | "bottom" | "margin-left" | "margin-right" | "margin-top"
        | "margin-bottom" | "z-index" => &["auto"],
        "max-width" | "max-height" => &["none"],
        "line-height" | "letter-spacing" | "word-spacing" | "row-gap" | "column-gap" | "grid-row-gap" | "grid-column-gap" => &["normal"],
        "backface-visibility" => &["hidden", "visible"],
        "accent-color" => &["auto"],
        "list-style-image" => &["none", "url(x.png)", "url('y')"],
        _ => return None,
    })
}

/// Decorations a declaration may carry: `!important`, a CSS-wide keyword, `var()`, viewport math.
fn decorated(random: &mut Lcg, value: String) -> String {
    match random.below(16) {
        0 => format!("{value} !important"),
        1 => format!("{value} !IMPORTANT"),
        2 => pick_str(random, &["inherit", "initial", "unset", "revert", "INHERIT"]).into(),
        3 => format!("var(--x, {value})"),
        4 => format!("calc({value} + 1vw)"),
        5 => format!("{value} ! important"),
        _ => value,
    }
}

// ---- Cases -----------------------------------------------------------------------------------------------

/// The qualified rules' blocks of a sheet at any depth, as text.
fn rule_blocks(rules: &[twidget_litehtml::stylesheet::RawRule], out: &mut Vec<Vec<u8>>) {
    for rule in rules {
        if rule.ty == RuleType::qualified {
            out.push(get_repr(&rule.block.value, 0, -1, false));
        } else if rule.block.ty == CURLY_BLOCK {
            rule_blocks(&parse_stylesheet_tokens(&rule.block.value, false), out);
        }
    }
}

#[test]
fn corpus_declarations_alike() {
    let styles = corpus_styles();
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5001);
    let mut blocks = 0;
    let mut attributes = 0;
    let mut k = 0usize;
    for (i, style) in styles.iter().enumerate() {
        let (style, _) = without_overlong_ascii(style);
        if style.contains(&b'{') {
            let mut texts = Vec::new();
            rule_blocks(&parse_stylesheet(&style, true), &mut texts);
            for text in texts {
                blocks += 1;
                run.style(1, 1, b"", b"", &text, format!("rule block of style {i}"));
                run.style(2, 0, b"", b"", &text, format!("rule block of style {i} as text"));
            }
        } else {
            attributes += 1;
            run.style(2, 0, b"", b"", &style, format!("style attribute {i}"));
            run.style(0, 0, b"", b"", &style, format!("style attribute {i} without a container"));
            run.style(3, 1, b"base/", b"", &style, format!("style attribute {i} with a base url"));
        }
        for (name, value) in declarations(&style) {
            k += 1;
            run.style(
                (k % 4) as u8,
                2 + u8::from(k.is_multiple_of(9)),
                b"",
                &twidget_litehtml::html::lowcase(&name),
                &value,
                format!("declaration {k} of style {i}"),
            );
            if k.is_multiple_of(5) {
                let mut changed = value.clone();
                let edits = mutate(&mut random, &mut changed);
                run.style((k % 3) as u8 + 1, 2, b"", &name, &changed, format!("declaration {k} of style {i} changed{edits}"));
            }
        }
    }
    assert!(blocks > 1000 && attributes > 500, "found {blocks} rule blocks and {attributes} style attributes in widget-sandbox's corpus");
    eprintln!("corpus: {} style texts, {blocks} rule blocks, {attributes} style attributes, {k} declarations", styles.len());
    run.check("corpus declarations");
}

#[test]
fn master_declarations_alike() {
    let master = master_css();
    let extra = ua_extra();
    let ua = ua_stylesheet();
    let resolved =
        twidget_css::unwrap_where(&twidget_css::substitute_variables(&ua, &[(b"--tg-theme-bg-color".to_vec(), b"#fff".to_vec())]));
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5002);
    let mut n = 0;
    for (name, text) in [
        ("master_css", &master),
        ("UA_EXTRA", &extra),
        ("the profile's UA layer", &ua),
        ("the UA layer as the renderer reads it", &resolved),
    ] {
        let mut texts = Vec::new();
        rule_blocks(&parse_stylesheet(text, true), &mut texts);
        for (k, block) in texts.iter().enumerate() {
            n += 1;
            for container in 0..4u8 {
                run.style(container, 1, b"", b"", block, format!("rule {k} of {name} in container {container}"));
            }
            run.style(2, 0, b"", b"", block, format!("rule {k} of {name} as text"));
            if k.is_multiple_of(4) {
                let mut changed = block.clone();
                let edits = mutate(&mut random, &mut changed);
                run.style(1, 1, b"", b"", &changed, format!("rule {k} of {name} changed{edits}"));
            }
        }
    }
    assert!(n > 150, "{n} rules in the master and UA sheets");
    run.check("master declarations");
}

#[test]
fn generated_declarations_alike() {
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5003);
    let mut k = 0;
    for round in 0..40 {
        for &property in PROPERTIES {
            k += 1;
            let value = any_value(&mut random, property);
            let value = decorated(&mut random, value);
            run.declaration(property.as_bytes(), value.as_bytes(), k, &format!("{property}: {value} (round {round})"));
            if k.is_multiple_of(6) {
                let mut changed = value.clone().into_bytes();
                let edits = mutate(&mut random, &mut changed);
                run.style((k % 4) as u8, 2, b"", property.as_bytes(), &changed, format!("{property}: {value} changed{edits}"));
            }
        }
    }
    // The shorthands and lists whose valid values are rare among random ones, again.
    for round in 0..150 {
        for property in [
            "background",
            "background-image",
            "background-position",
            "background-size",
            "font",
            "flex",
            "border",
            "outline",
            "list-style",
            "text-decoration",
            "text-emphasis",
            "grid",
            "place-items",
            "place-content",
            "border-radius",
            "align-self",
            "flex-flow",
            "margin",
            "border-color",
            "font-feature-settings",
        ] {
            k += 1;
            let value = any_value(&mut random, property);
            run.declaration(property.as_bytes(), value.as_bytes(), k, &format!("{property}: {value} (again {round})"));
        }
    }
    // Blocks of several declarations: the cache, `-lh-anim`'s layer, later declarations over earlier ones,
    // `!important` against them.
    for i in 0..3000usize {
        let n = 1 + random.below(5);
        let mut decls = Vec::new();
        if random.chance(4) {
            decls.push("-lh-anim: 1".to_string());
        }
        for _ in 0..n {
            let property = pick_str(&mut random, PROPERTIES);
            let value = any_value(&mut random, property);
            let value = decorated(&mut random, value);
            decls.push(format!("{property}: {value}"));
        }
        if random.chance(5) {
            decls.insert(random.below(decls.len()), "-lh-anim:x".into());
        }
        let text = decls.join(pick_str(&mut random, &["; ", ";", " ;\n", ";;"]));
        let entry = (i % 2) as u8;
        run.style((i % 4) as u8, entry, if i.is_multiple_of(11) { b"b/" } else { b"" }, b"", text.as_bytes(), format!("block {i}: {text}"));
    }
    // The same declarations again: answers from the cache, keyed by a container's presence alone.
    let mut again = Lcg::new(0x11_5003);
    for i in 0..400 {
        let property = pick_str(&mut again, PROPERTIES);
        let value = any_value(&mut again, property);
        let text = format!("{property}: {value}");
        run.style((i % 4) as u8, 0, b"", b"", text.as_bytes(), format!("again {i}: {text}"));
        run.style(((i + 1) % 4) as u8, 0, b"", b"", text.as_bytes(), format!("again in another container {i}: {text}"));
    }
    // Edges of the parsers, each through every entry.
    for (i, text) in [
        "-lh-anim:1; -lh-raw-opacity: 5; opacity: .5",
        "-lh-anim: 1;transform: none;-lh-raw-transform: x",
        "-LH-ANIM:1; opacity: 1",
        "-lh-animation: 1; color: red",
        "font: 12px",
        "font: \"caption\"",
        "font: Caption",
        "font: italic bold 12px/30px Georgia, serif",
        "font: normal normal normal 1px a",
        "flex: content",
        "flex: unheard",
        "flex: initial",
        "flex: 0",
        "flex: 1 0",
        "flex: 0 0 0",
        "text-decoration: 3px from-font",
        "text-decoration: auto",
        "text-decoration: underline red wavy 2px blink",
        "text-emphasis: 5 red blue 'x'",
        "rotate: 1turn",
        "order: 2147483647",
        "order: -2147483649",
        "background: url(a.png) 10px 20px / cover no-repeat fixed content-box padding-box red",
        "background: red, blue",
        "background: center / 10px",
        "background: left top / 10px 20px, linear-gradient(red, blue)",
        "background: top / x",
        "background-position: top left, 10px bottom",
        "border: 1px solid red; border-color: blue; border-top: thick",
        "margin: 1px 2px 3px",
        "border-radius: 1px 2px / 3px",
        "inset: auto 1px",
        "place-content: normal",
        "place-content: center stretch",
        "place-items: baseline first",
        "align-self: safe center",
        "align-items: last baseline",
        "aspect-ratio: auto 4 / 3",
        "contain: none strict",
        "color-scheme: dark only",
        "accent-color: auto",
        "list-style: none none",
        "list-style: none disc",
        "list-style: inside url(x) none",
        "grid: auto-flow 10px / 1fr",
        "grid-template: none",
        "gap: 1px normal",
        "grid-gap: 10%",
        "width: 10vw",
        "width: calc(100% - 1vw)",
        "font-feature-settings: \"liga\" on, \"tnum\" 2",
        "font-variant-numeric: ordinal ordinal",
        "color: currentcolor",
        "color: inherit",
        "--x: inherit",
        "--x: initial",
        "--: 1",
        "--a: ;",
        "-webkit-backdrop-filter: blur(2px)",
        "word-wrap: anywhere",
        "-webkit-user-select: none",
        "overflow: hidden auto",
        "overflow: hidden auto scroll",
        "background-color: brand",
        "background-color: alias",
        "background-color: twotok",
        "color: Accentx",
        "border-spacing: 1px -1px",
        "border-top-left-radius: 1px 2px 3px",
        "z-index: 1.5",
    ]
    .iter()
    .enumerate()
    {
        for entry in 0..2u8 {
            run.style((i % 4) as u8, entry, b"", b"", text.as_bytes(), format!("edge {text:?} entry {entry}"));
        }
        if let Some((name, value)) = text.split_once(':') {
            run.style(
                ((i + 1) % 4) as u8,
                2,
                b"",
                name.trim().as_bytes(),
                value.split(';').next().unwrap_or("").as_bytes(),
                format!("edge add_property {text:?}"),
            );
        }
    }
    run.check("generated declarations");
}

#[test]
fn effects_alike() {
    let mut run = StyleRun::new();
    let mut random = Lcg::new(0x11_5004);
    // Every value the corpus gives these properties, and generated ones.
    let parsers: [(u8, &[&str]); 7] = [
        (0, &["box-shadow", "text-shadow"]),
        (1, &["filter", "backdrop-filter", "-webkit-backdrop-filter"]),
        (2, &["transform"]),
        (3, &["transform-origin"]),
        (4, &["clip-path"]),
        (5, &["grid-template-columns", "grid-template-rows", "grid-auto-rows"]),
        (6, &["grid-column", "grid-row", "grid-column-start", "grid-column-end", "grid-row-start", "grid-row-end"]),
    ];
    let mut k = 0;
    for style in corpus_styles() {
        let (style, _) = without_overlong_ascii(&style);
        for (name, value) in declarations(&style) {
            k += 1;
            let name = String::from_utf8_lossy(&twidget_litehtml::html::lowcase(&name)).into_owned();
            for (which, properties) in &parsers {
                if properties.contains(&name.as_str()) {
                    run.effects(*which, (k % 4) as u8, &value, format!("{name}: {} (corpus)", String::from_utf8_lossy(&value)));
                    if *which == 2 {
                        run.effects(7, 0, &value, format!("fast {name}: {} (corpus)", String::from_utf8_lossy(&value)));
                    }
                }
            }
            run.effects(8, (k % 4) as u8, &value, format!("fold {name}: {} (corpus)", String::from_utf8_lossy(&value)));
        }
    }
    for round in 0..600 {
        for (which, properties) in &parsers {
            let property = properties[round % properties.len()];
            let mut value = any_value(&mut random, property).into_bytes();
            if round.is_multiple_of(7) {
                mutate(&mut random, &mut value);
            }
            run.effects(*which, (round % 4) as u8, &value, format!("{property}: {} (generated {round})", String::from_utf8_lossy(&value)));
            if *which == 2 {
                run.effects(7, 0, &value, format!("fast transform: {} (generated {round})", String::from_utf8_lossy(&value)));
            }
        }
        let value = (0..1 + random.below(3))
            .map(|_| if random.chance(2) { math(&mut random, 0) } else { color(&mut random) })
            .collect::<Vec<_>>()
            .join(" ");
        run.effects(8, (round % 4) as u8, value.as_bytes(), format!("fold {value}"));
    }
    // Folding's edges: ranges, units, depth, mixes.
    for text in [
        "calc(1e30px * 1e30)",
        "calc(1px / 0)",
        "calc(1px * 2px)",
        "calc(2 / 1px)",
        "min(1px, 2em)",
        "min(1px, 0, 3px)",
        "clamp(1px, 2px)",
        "max()",
        "calc(((((((((((((((((((1px)))))))))))))))))))",
        "calc(1px + 0em)",
        "calc(0 + 5%)",
        "min(1px,2px,3px,4px,5px,6px,7px,8px,9px,10px,11px,12px,13px,14px,15px,16px,17px,18px,19px,20px,21px,22px,23px,24px,25px,26px,27px,28px,29px,30px,31px,32px,33px)",
        "color-mix(in srgb, black, white)",
        "color-mix(in srgb, #000 30%, red)",
        "color-mix(in srgb, red 0%, blue 0%)",
        "color-mix(in srgb, red 60%, blue 60%)",
        "color-mix(in srgb, red 20%, blue 20%)",
        "color-mix(in srgb, transparent, red)",
        "color-mix(in oklab, red, blue)",
        "color-mix(in srgb, currentcolor, red)",
        "env(safe-area-inset-top)",
        "env(foo, 1px 2px)",
        "env(foo, 3px)",
        "env()",
        "f(env(keyboard-inset-bottom))",
        "10vw 5svh 2dvmin 1lvmax",
        "calc(100vw - 10px)",
        "var(--x, calc(1px + 1px))",
        "1e40px",
        "calc(1e38 * 10)",
    ] {
        for container in 0..3u8 {
            run.effects(8, container, text.as_bytes(), format!("fold {text:?} in container {container}"));
        }
    }
    // The fast path's own edges.
    for text in [
        "none",
        " none ",
        "nonex",
        "none(",
        "translate(10px, 20%) rotate(45deg)",
        "scale(1.5)",
        "matrix(1,0,0,1,0,0)",
        "rotate(1e3deg)",
        "translateX(+1.5px)",
        "skewx(.5rad)",
        "rotate(10)",
        "scale(1 2 3 4 5 6 7)",
        "translate(1px 2px",
        "translate()",
        "rotate(1.px)",
        "scale(99999999999)",
        "TRANSLATE(1PX)",
        "rotate(1turn)x",
        "translate(10px)translate(1px)",
        "foo(1)",
        "",
    ] {
        run.effects(7, 0, text.as_bytes(), format!("fast transform {text:?}"));
    }
    run.check("effects");
}

/// What the comparison found, kept as the port's own checks.
#[test]
fn the_forks_quirks_hold() {
    let names = Names::new();
    let cache = twidget_litehtml::style::DeclarationCache::default();
    let add = |text: &str, container: Option<&dyn twidget_litehtml::document_container::DocumentContainer>| {
        let mut st = Style::default();
        st.add_text(&names, &cache, text.as_bytes(), b"", container);
        st
    };
    let id = |name: &str| names.find_id(name.as_bytes()).expect("a known name");
    let value = |st: &Style, name: &str| st.get_property(id(name)).value.clone();

    // A keyword after a length keeps the length's unit (`from_token` sets only the index).
    let st = add("flex: content", None);
    match value(&st, "flex-basis") {
        Value::Length(len) => assert!(len.is_predefined() && len.predef() == 1 && len.units() == CssUnits::css_units_px),
        other => panic!("{other:?}"),
    }
    // `color-mix()` refuses opaque black: it compares equal to currentcolor.
    let mut tokens = normalize(b"color-mix(in srgb, black 50%, white)", f_componentize | f_remove_whitespace, None);
    assert!(!twidget_litehtml::css_effects::fold_math(&names, &mut tokens, None, 0));
    // `font` takes a size without a family, the family empty.
    let st = add("font: 12px", None);
    assert_eq!(value(&st, "font-family"), Value::String(Vec::new()));
    // A malformed rotate3d() is skipped, not refused.
    let mut functions = Vec::new();
    let tokens = normalize(b"rotate3d(1) translate(1px)", f_componentize | f_remove_whitespace, None);
    assert!(twidget_litehtml::css_effects::parse_transform(&names, &tokens, &mut functions));
    assert_eq!(functions.len(), 1);
    // The cache keys by whether there is a container, not which: the renderer's sanitizing is not consulted
    // for a declaration another container left in the cache.
    let st = add("display: list-item", Some(&DeclContainer::Plain));
    assert!(!st.empty());
    let st = add("display: list-item", Some(&DeclContainer::Renderer));
    assert!(!st.empty(), "the cached declaration is replayed");
    let mut fresh = Style::default();
    fresh.add(&names, &normalize(b"display: list-item", f_componentize, None), b"", Some(&DeclContainer::Renderer));
    assert!(fresh.empty(), "the renderer drops list-item when it is asked");
    // Every property name a style meets is interned for good, and so is a lone flex keyword.
    let before = names.usage().names;
    add("made-up-property: 1; flex: unheard", None);
    assert_eq!(names.usage().names, before + 2);
    assert!(names.find_id(b"unheard").is_some() && names.find_id(b"made-up-property").is_some());
    // Past `-lh-anim`, a style text drops a raw name declared outright; a block of tokens keeps it.
    let raw = id("-lh-raw-opacity");
    assert!(add("-lh-anim:1; -lh-raw-opacity: 5", None).get_property(raw).value == Value::Invalid);
    let mut tokens_path = Style::default();
    tokens_path.add(&names, &normalize(b"-lh-anim:1; -lh-raw-opacity: 5", f_componentize, None), b"", None);
    assert!(matches!(tokens_path.get_property(raw).value, Value::Tokens(_)));
    // `auto` in text-decoration is taken as its colour, before its thickness.
    let st = add("text-decoration: auto", None);
    assert!(matches!(value(&st, "text-decoration-color"), Value::Color(c) if c.is_current_color));
    assert_eq!(value(&st, "text-decoration-thickness"), Value::Invalid);
    // Math folds in doubles and narrows to float unchecked: past float's range it is infinite.
    let mut tokens = normalize(b"calc(1e30px * 1e30)", f_componentize | f_remove_whitespace, None);
    assert!(twidget_litehtml::css_effects::fold_math(&names, &mut tokens, None, 0));
    assert!(tokens[0].ty == DIMENSION && tokens[0].n.number == f32::INFINITY);
}

#[test]
fn declaration_cache_limits_alike() {
    let mut run = StyleRun::new();
    let list_item = b"display: list-item";
    // A container's verdict stays in the cache for another: the renderer's sanitizing is not asked again.
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's container");
    run.style(2, 0, b"", b"", list_item, "list-item in the renderer's, from the cache");
    // Past 512 bytes, or with a viewport length, a declaration is not cached.
    let long = [&b"display:"[..], &[b' '; 600], b"list-item"].concat();
    run.style(1, 0, b"", b"", &long, "a long list-item in lhdump's container");
    run.style(2, 0, b"", b"", &long, "a long list-item in the renderer's");
    for container in [1u8, 2, 1, 2] {
        run.style(
            container,
            0,
            b"",
            b"",
            b"width: 10vw; height: calc(5vh + 1px); left: 1vmax",
            format!("viewport lengths in container {container}"),
        );
    }
    // Past 1 MB of keys the cache is emptied whole, and the renderer is asked.
    for batch in 0..45 {
        let text: Vec<String> = (0..50).map(|j| format!("--b{}: {}", batch * 50 + j, "a".repeat(480))).collect();
        run.style(1, 0, b"", b"", text.join(";").as_bytes(), format!("long fillers {batch}"));
    }
    run.style(2, 0, b"", b"", list_item, "list-item in the renderer's after the cache was emptied");
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's, from the cache: dropped");
    // Past 50000 declarations likewise.
    for batch in 0..101 {
        let text: Vec<String> = (0..500).map(|j| format!("--e{}:1", batch * 500 + j)).collect();
        run.style(1, 0, b"", b"", text.join(";").as_bytes(), format!("fillers {batch}"));
    }
    run.style(1, 0, b"", b"", list_item, "list-item in lhdump's after the cache was emptied again");
    assert!(run.lh.cache.len() < 50000);
    run.check("declaration cache limits");
}

/// `subst_vars` waits for the cascade's elements to be compared; this checks the port on its own.
#[test]
fn variables_are_substituted() {
    use twidget_litehtml::style::VarElement;
    struct Vars<'a>(&'a Names, Vec<(&'static str, &'static str)>);
    impl VarElement for Vars<'_> {
        fn get_custom_property(&self, name: twidget_litehtml::StringId, result: &mut CssTokenVector, _invalid: &mut bool) -> bool {
            for (n, v) in &self.1 {
                if self.0.id(n.as_bytes()) == name {
                    *result = normalize(v.as_bytes(), f_componentize | f_remove_whitespace, None);
                    return true;
                }
            }
            false
        }
        fn container(&self) -> Option<&dyn twidget_litehtml::document_container::DocumentContainer> {
            None
        }
    }
    let names = Names::new();
    let cache = twidget_litehtml::style::DeclarationCache::default();
    let mut st = Style::default();
    st.add_text(
        &names,
        &cache,
        b"color: var(--a); margin: var(--m, 1px 2px); width: var(--nope); border: var(--b); border-color: var(--a)",
        b"",
        None,
    );
    assert!(st.has_var_property());
    st.subst_vars(&names, &Vars(&names, vec![("--a", "red"), ("--b", "1px solid blue")]));
    let get = |name: &str| st.get_property(names.id(name.as_bytes())).value.clone();
    assert!(matches!(get("color"), Value::Color(c) if c.red == 255 && c.alpha == 255));
    assert!(matches!(get("margin-left"), Value::Length(len) if len.val() == 2.0));
    // A missing variable without fallback leaves the declaration as it was, still waiting for var().
    assert!(matches!(get("width"), Value::Tokens(_)) && st.get_property(names.id(b"width")).m_has_var);
    // border-color after border: its longhands keep the later declaration.
    assert!(matches!(get("border-top-color"), Value::Color(c) if c.red == 255));
    assert!(matches!(get("border-top-width"), Value::Length(len) if len.val() == 1.0));
}
