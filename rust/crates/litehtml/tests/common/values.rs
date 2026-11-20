//! Generators of property values (stage L5), shared by the declaration and cascade comparisons: every name
//! style.cpp reads and values of every form each takes.

#![allow(dead_code)]

use super::keywords;
use twidget_testkit::Lcg;

/// Every property name style.cpp reads: its switch's, its shorthands, the names it renames, the raw-declared
/// ones and their twins, and a few it does not know.
pub const PROPERTIES: &[&str] = &[
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

// ---- Generators -------------------------------------------------------------------------------------------

pub fn pick_str<'a>(random: &mut Lcg, from: &[&'a str]) -> &'a str {
    from[random.below(from.len())]
}

pub fn number(random: &mut Lcg) -> String {
    match random.below(10) {
        0 => "0".into(),
        1 => format!("-{}", random.below(50)),
        2 => format!("{}.{}", random.below(20), random.below(100)),
        3 => format!(".{}", random.below(1000)),
        4 => pick_str(random, &["1e3", "2E-2", "+4", "1000", "1001", "65536", "3000000000", "0.00001", "1e40", "0e999", "-0"]).into(),
        _ => format!("{}", random.below(400)),
    }
}

pub fn unit(random: &mut Lcg) -> &'static str {
    if random.chance(5) {
        return pick_str(random, &["fr", "deg", "rad", "turn", "grad", "s", "ms", "x", "Q", "PX", "dpi", "foo", "svh", "dvw", "lvmax"]);
    }
    pick_str(
        random,
        &["px", "px", "px", "px", "em", "rem", "%", "%", "%", "vw", "vh", "vmin", "vmax", "pt", "pc", "cm", "mm", "in", "ch", "ex"],
    )
}

pub fn length(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => "0".into(),
        1 => number(random),
        2 => math(random, 0),
        _ => format!("{}{}", number(random), unit(random)),
    }
}

pub fn color(random: &mut Lcg) -> String {
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

pub fn color_simple(random: &mut Lcg) -> String {
    pick_str(
        random,
        &["red", "#123", "rgb(1 2 3)", "black", "#000000", "white", "transparent", "currentcolor", "hsl(120 50% 50%)", "#8080807f"],
    )
    .into()
}

pub fn position(random: &mut Lcg) -> String {
    let side = |random: &mut Lcg| -> String {
        if random.chance(2) { pick_str(random, &["left", "right", "top", "bottom", "center"]).into() } else { length(random) }
    };
    if random.chance(3) { side(random) } else { format!("{} {}", side(random), side(random)) }
}

pub fn gradient(random: &mut Lcg) -> String {
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

pub fn image(random: &mut Lcg) -> String {
    match random.below(5) {
        0 => "none".into(),
        1 => pick_str(random, &["url(a.png)", "url( 'b c.png' )", "url(\"x\")", "src(\"y\")", "url()"]).into(),
        _ => gradient(random),
    }
}

/// Math the stylesheet folds, or keeps for layout, or refuses.
pub fn math(random: &mut Lcg, depth: u32) -> String {
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

pub fn keyword(random: &mut Lcg) -> String {
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

pub fn function(random: &mut Lcg) -> String {
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

pub fn filter_function(random: &mut Lcg) -> String {
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
pub fn piece(random: &mut Lcg) -> String {
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

pub fn shuffled_join(random: &mut Lcg, mut parts: Vec<String>) -> String {
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

pub fn pick_some(random: &mut Lcg, parts: Vec<String>) -> Vec<String> {
    parts.into_iter().filter(|_| random.below(3) != 0).collect()
}

/// A value for the property: one in its grammar most of the time, any components now and then.
pub fn any_value(random: &mut Lcg, property: &str) -> String {
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
pub fn keyword_table(property: &str) -> Option<&'static [&'static str]> {
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
pub fn decorated(random: &mut Lcg, value: String) -> String {
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
