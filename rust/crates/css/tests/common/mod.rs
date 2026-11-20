//! What the CSS tests share: the Rust side of every `oracle/css_oracle` operation, a generator of the
//! CSS-ish text the helpers read, and the style text of widget-sandbox's corpus.

#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};

use twidget_css as css;
use twidget_json::Json;
pub use twidget_testkit::{Lcg, framed, hex, mutate};
use twidget_wire::Affine;

#[derive(Default)]
pub struct Out {
    pub s: Vec<u8>,
}

impl Out {
    pub fn u8(&mut self, v: u8) {
        self.s.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.s.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.s.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub fn str(&mut self, t: &[u8]) {
        self.u32(t.len() as u32);
        self.s.extend_from_slice(t);
    }
}

struct In<'a> {
    s: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    fn u32(&mut self) -> u32 {
        let mut v = 0u32;
        for i in 0..4 {
            if self.at < self.s.len() {
                v |= u32::from(self.s[self.at]) << (8 * i);
                self.at += 1;
            }
        }
        v
    }
    fn u8(&mut self) -> u8 {
        let v = self.s.get(self.at).copied().unwrap_or(0);
        self.at = (self.at + 1).min(self.s.len());
        v
    }
    fn f64(&mut self) -> f64 {
        let mut v = 0u64;
        for i in 0..8 {
            if self.at < self.s.len() {
                v |= u64::from(self.s[self.at]) << (8 * i);
                self.at += 1;
            }
        }
        f64::from_bits(v)
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    fn bytes(&mut self, n: usize) -> &'a [u8] {
        let n = n.min(self.s.len() - self.at);
        let out = &self.s[self.at..self.at + n];
        self.at += n;
        out
    }
    fn rest(&mut self) -> &'a [u8] {
        self.bytes(self.s.len() - self.at)
    }
}

fn pairs(o: &mut Out, vars: &css::Variables) {
    o.u32(vars.len() as u32);
    for (k, v) in vars {
        o.str(k);
        o.str(v);
    }
}

fn affine(o: &mut Out, m: &Affine) {
    for f in [m.a, m.b, m.c, m.d, m.e, m.f] {
        o.f32(f);
    }
}

/// What the oracle answers to operation `op` on `input`.
pub fn answer(op: u8, input: &[u8]) -> (u8, Vec<u8>) {
    let mut i = In { s: input, at: 0 };
    let mut o = Out::default();
    match op {
        b's' => {
            let end = input.iter().position(|c| *c == 0).unwrap_or(input.len());
            let (v, n) = twidget_cstd::strtod(&input[..end]);
            o.f64(v);
            o.u32(n as u32);
            (1, o.s)
        }
        b'N' => match css::number(input) {
            None => (0, Vec::new()),
            Some(v) => {
                o.f64(v);
                (1, o.s)
            }
        },
        b't' => (1, css::trim(input)),
        b'l' => (1, css::lower(input)),
        b'Z' => (1, css::normalize_colors(input)),
        b'W' => (1, css::unwrap_where(input)),
        b'i' => {
            let n = i.u32() as usize;
            let a = i.bytes(n);
            (u8::from(css::iequals(a, i.rest())), Vec::new())
        }
        b'S' => {
            let sep = i.u8();
            let parts = css::split_top_level(i.rest(), sep);
            o.u32(parts.len() as u32);
            for p in &parts {
                o.str(p);
            }
            (1, o.s)
        }
        b'D' => {
            let ds = css::parse_declarations(input);
            o.u32(ds.len() as u32);
            for d in &ds {
                o.str(&d.name);
                o.str(&d.value);
                o.u8(u8::from(d.important));
            }
            (1, o.s)
        }
        b'n' => (u8::from(css::is_tag_name(input)), Vec::new()),
        b'a' => (u8::from(css::is_attribute_name(input)), Vec::new()),
        b'p' => (u8::from(css::is_property_name(input)), Vec::new()),
        b'v' => (u8::from(css::is_theme_variable(input)), Vec::new()),
        b'L' => {
            let dark = i.u8() != 0;
            (1, css::resolve_light_dark(i.rest(), dark, 0))
        }
        b'O' => {
            let at = i.u32() as usize;
            o.u32(css::skip_opaque(i.rest(), at) as u32);
            (1, o.s)
        }
        b'T' => match css::parse_time(input) {
            None => (0, Vec::new()),
            Some(v) => {
                o.f64(v);
                (1, o.s)
            }
        },
        b'C' => match css::parse_color(input) {
            None => (0, Vec::new()),
            Some(c) => (1, vec![c.r, c.g, c.b, c.a]),
        },
        b'F' => {
            let d = i.f64();
            let digits = i.u32() as i32;
            (1, css::format_number(d, digits).into_bytes())
        }
        b'f' => {
            let f = css::parse_font(input);
            o.str(&f.family);
            o.f32(f.size);
            o.u32(f.weight as u32);
            o.u8(u8::from(f.italic));
            (1, o.s)
        }
        b'X' => {
            let which = i.u8();
            let w = i.f32();
            let h = i.f32();
            let text = i.rest();
            match which {
                4 => match css::parse_origin(text, w, h) {
                    None => (0, Vec::new()),
                    Some((x, y)) => {
                        o.f32(x);
                        o.f32(y);
                        (1, o.s)
                    }
                },
                5 => match css::parse_opacity(text) {
                    None => (0, Vec::new()),
                    Some(v) => {
                        o.f32(v);
                        (1, o.s)
                    }
                },
                _ => {
                    let m = match which {
                        0 => css::parse_transform(text, w, h),
                        1 => css::parse_translate(text, w, h),
                        2 => css::parse_rotate(text),
                        _ => css::parse_scale(text),
                    };
                    match m {
                        None => (0, Vec::new()),
                        Some(m) => {
                            affine(&mut o, &m);
                            (1, o.s)
                        }
                    }
                }
            }
        }
        b'H' | b'A' => match Json::parse(input) {
            None => (2, Vec::new()),
            Some(json) => {
                pairs(&mut o, &if op == b'H' { css::theme_variables(&json) } else { css::content_safe_area_variables(&json) });
                (1, o.s)
            }
        },
        b'V' => {
            let n = i.u32();
            let mut vars = Vec::new();
            for _ in 0..n {
                let kn = i.u32() as usize;
                let k = i.bytes(kn).to_vec();
                let vn = i.u32() as usize;
                let v = i.bytes(vn).to_vec();
                vars.push((k, v));
            }
            (1, css::substitute_variables(i.rest(), &vars))
        }
        _ => panic!("no operation {op}"),
    }
}

// ---- Text the helpers read --------------------------------------------------------------------

fn pick<'a>(random: &mut Lcg, items: &[&'a str]) -> &'a str {
    items[random.below(items.len())]
}

pub fn any_number(random: &mut Lcg) -> String {
    match random.below(12) {
        0 => pick(random, &["0x1p3", "0X1.8p-2", "0x.8", "0x10", "0x", "0xg", "0x1P+1023", "0x1p-1074", "0x1.fffffffffffff8p1023"]).into(),
        1 => pick(random, &["inf", "-INF", "Infinity", "nan", "NaN(123)", "nan(", "+inf", "infinit"]).into(),
        2 => pick(
            random,
            &[".5", "5.", ".", "-.5e3", "+5", "1e", "1e+", "1e-2", "1E5", "007", "-0", "1e999", "1e-400", "\u{b}5", " 5 ", "5 5"],
        )
        .into(),
        3 => format!("{}", random.next() as f64 / 7.0),
        4 => format!("{}.{}", random.below(1000), random.below(100000)),
        5 => format!("{}e{}", random.below(100), random.below(40) as i32 - 20),
        6 => {
            let mut s = format!("0x{:x}", random.next());
            if random.chance(2) {
                s.push_str(&format!(".{:x}", random.next()));
            }
            if random.chance(2) {
                s.push_str(&format!("p{}", random.below(2200) as i32 - 1100));
            }
            s
        }
        7 => format!("{}", (random.below(2001) as i32 - 1000)),
        8 => format!("-{}.{:03}", random.below(50), random.below(1000)),
        _ => format!("{}", random.below(360)),
    }
}

fn any_unit(random: &mut Lcg) -> &'static str {
    pick(random, &["px", "%", "deg", "rad", "grad", "turn", "s", "ms", "em", "rem", "pt", "vh", "", "", "PX", "Deg"])
}

pub fn any_dimension(random: &mut Lcg) -> String {
    format!("{}{}", any_number(random), any_unit(random))
}

pub fn any_calc(random: &mut Lcg, depth: usize) -> String {
    if depth > 3 || random.chance(3) {
        return any_dimension(random);
    }
    let op = pick(random, &[" + ", " - ", " * ", " / ", "+", "*", " "]);
    let inner = format!("{}{}{}", any_calc(random, depth + 1), op, any_calc(random, depth + 1));
    match random.below(3) {
        0 => format!("calc({inner})"),
        1 => format!("({inner})"),
        _ => inner,
    }
}

pub fn any_color(random: &mut Lcg) -> String {
    let hex = |random: &mut Lcg, n: usize| (0..n).map(|_| char::from(b"0123456789abcdefABCDEFg"[random.below(23)])).collect::<String>();
    match random.below(9) {
        0 => {
            let n = [3, 4, 6, 8, 5, 0, 2][random.below(7)];
            format!("#{}", hex(random, n))
        }
        1 => {
            let sep = pick(random, &[",", " ", ", ", " , "]);
            let a = if random.chance(2) { format!("{}{}", pick(random, &[",", " /", "/"]), any_number(random)) } else { String::new() };
            format!(
                "{}({}{sep}{}{sep}{}{a})",
                pick(random, &["rgb", "rgba", "RGB", "rgB"]),
                any_number(random) + pick(random, &["", "%"]),
                any_number(random) + pick(random, &["", "%"]),
                any_number(random) + pick(random, &["", "%"])
            )
        }
        2 => format!(
            "{}({}{}, {}%, {}%{})",
            pick(random, &["hsl", "hsla"]),
            any_number(random),
            pick(random, &["", "deg"]),
            any_number(random),
            any_number(random),
            if random.chance(2) { format!(", {}", any_number(random)) } else { String::new() }
        ),
        3 => pick(random, &["red", "RebeccaPurple", "transparent", "currentColor", "lightgoldenrodyellow", "notacolor", "grey", "Navy"])
            .into(),
        4 => format!("light-dark({}, {})", any_color(random), any_color(random)),
        5 => format!("var(--c{}, {})", random.below(3), any_color(random)),
        6 => format!(" {} ", any_color(random)),
        7 => {
            pick(random, &["rgb(1,2)", "rgb()", "rgb(1 2 3 / 50%)", "rgba(255, 0, 0, .5)", "hsl(120deg 100% 50%)", "#", "rgb(1,2,3"]).into()
        }
        _ => format!("{} {}", any_color(random), any_color(random)),
    }
}

pub fn any_transform(random: &mut Lcg) -> String {
    let n = 1 + random.below(3);
    let mut out = Vec::new();
    for _ in 0..n {
        let f = pick(
            random,
            &[
                "translate",
                "translateX",
                "translatey",
                "translate3d",
                "translateZ",
                "scale",
                "scaleX",
                "scaleY",
                "scale3d",
                "rotate",
                "rotateZ",
                "skew",
                "skewX",
                "skewY",
                "matrix",
                "perspective",
                "rotate3d",
                "",
            ],
        );
        let args = 1 + random.below(if f == "matrix" { 7 } else { 3 });
        let list: Vec<String> = (0..args)
            .map(|_| match random.below(4) {
                0 => any_calc(random, 0),
                _ => any_dimension(random),
            })
            .collect();
        let sep = pick(random, &[", ", ",", " "]);
        out.push(format!("{f}({})", list.join(sep)));
    }
    match random.below(8) {
        0 => "none".into(),
        1 => String::new(),
        2 => format!("{} junk", out.join(" ")),
        _ => out.join(pick(random, &[" ", "  ", "\t"])),
    }
}

pub fn any_font(random: &mut Lcg) -> String {
    let mut parts = Vec::new();
    for _ in 0..random.below(3) {
        parts.push(
            pick(random, &["italic", "oblique", "bold", "bolder", "lighter", "normal", "small-caps", "700", "100", "Bold", "450"])
                .to_string(),
        );
    }
    if !random.chance(5) {
        let size = format!("{}{}", any_number(random), pick(random, &["px", "pt", "em", "rem", "%", "", "vh"]));
        parts.push(if random.chance(3) { format!("{size}/{}", any_dimension(random)) } else { size });
    }
    for _ in 0..random.below(3) {
        parts.push(pick(random, &["sans-serif", "'Helvetica Neue'", "\"Open Sans\",", "serif", "system-ui,", "'a b'"]).to_string());
    }
    parts.join(pick(random, &[" ", "  ", "\t"]))
}

pub fn any_time(random: &mut Lcg) -> String {
    match random.below(3) {
        0 => any_calc(random, 0).replace("px", "s").replace('%', "ms"),
        _ => format!("{}{}", any_number(random), pick(random, &["s", "ms", "S", "MS", "", "sec"])),
    }
}

pub fn any_opaque(random: &mut Lcg) -> String {
    pick(
        random,
        &[
            "'a;b'",
            "\"x)y\"",
            "/* light-dark(a,b) */",
            "url(a(b).png)",
            "url( 'q' )",
            "\\(",
            "'unterminated",
            "\"bad\nstring\"",
            "URL(x)",
            "aurl(x)",
            "/* open",
            "url(x\\)y)",
            "u",
            "\\",
        ],
    )
    .into()
}

pub fn any_css_text(random: &mut Lcg, depth: usize) -> String {
    let n = 1 + random.below(5);
    let mut out = String::new();
    for _ in 0..n {
        let piece = match random.below(12) {
            0 => any_color(random),
            1 if depth < 3 => format!("light-dark({}, {})", any_css_text(random, depth + 1), any_css_text(random, depth + 1)),
            2 => any_opaque(random),
            3 => format!(
                "var(--{}{})",
                pick(random, &["a", "b", "tg-theme-bg-color", "x y", ""]),
                if random.chance(2) { format!(", {}", any_color(random)) } else { String::new() }
            ),
            4 => format!(":where({}){}", pick(random, &["a", "a,b", "a, b c", ".x"]), pick(random, &["", " {", ",", ".y", "{"])),
            5 => any_transform(random),
            6 => pick(
                random,
                &["light-dark(", "LIGHT-DARK(a,b)", "xlight-dark(a,b)", "light-dark(a)", "light-dark(a,b,c)", ")", "(", ",", ";"],
            )
            .into(),
            7 => any_calc(random, 0),
            8 => format!(
                "{}: {}{}",
                pick(random, &["color", "--X", "Background", "-webkit-x", "b@d", ""]),
                any_color(random),
                pick(random, &[";", " !important;", "!IMPORTANT", ""])
            ),
            9 => format!("var(--a, var(--b, {}))", any_color(random)),
            _ => any_dimension(random),
        };
        out.push_str(&piece);
        out.push_str(pick(random, &[" ", "", "; ", ", ", "\n"]));
    }
    out
}

pub fn any_name(random: &mut Lcg) -> Vec<u8> {
    let alphabet = b"abcXYZ019-_:.@\"> \x80";
    let most = if random.chance(10) { 140 } else { 12 };
    let n = random.below(most);
    let mut s: Vec<u8> = (0..n).map(|_| alphabet[random.below(alphabet.len())]).collect();
    if random.chance(4) {
        s.splice(0..0, b"--".iter().copied());
    } else if random.chance(4) {
        s.insert(0, b'-');
    }
    s
}

pub fn any_theme(random: &mut Lcg) -> String {
    let mut fields = Vec::new();
    if !random.chance(4) {
        fields.push(format!("\"colorScheme\":\"{}\"", pick(random, &["dark", "light", "Dark", ""])));
    }
    let keys =
        ["bg_color", "text_color", "hint_color", "link_color", "button_color", "section_separator_color", "bottom_bar_bg_color", "other"];
    let mut params = Vec::new();
    for _ in 0..random.below(5) {
        let v = match random.below(4) {
            0 => format!("\"#{:06X}\"", random.next() & 0xffffff),
            1 => format!("\"#{:03x}\"", random.next() & 0xfff),
            2 => "12".into(),
            _ => format!("\"{}\"", pick(random, &["red", "#12345g", "#abcdef", ""])),
        };
        params.push(format!("\"{}\":{v}", keys[random.below(keys.len())]));
    }
    fields.push(format!("\"params\":{{{}}}", params.join(",")));
    let mut button = Vec::new();
    for _ in 0..random.below(6) {
        let (k, v): (&str, String) = match random.below(8) {
            0 => ("bg", format!("\"#{:06x}\"", random.next() & 0xffffff)),
            1 => ("text", pick(random, &["\"#ABCDEF\"", "\"#fff\"", "\"white\""]).into()),
            2 => ("radius", pick(random, &["\"8px\"", "\"0\"", "\"1.5rem\"", "\"12345px\"", "\"8.1234px\"", "\"px\"", "8"]).into()),
            3 => (
                "font",
                pick(
                    random,
                    &[
                        "\"bold 15px system-ui\"",
                        "\"italic 600 1.5rem/1.2 sans-serif, serif\"",
                        "\"15px Arial\"",
                        "\"normal normal 15px serif\"",
                        "\"15px sans-serif,\"",
                        "\"500 15px/20px monospace\"",
                    ],
                )
                .into(),
            ),
            4 => ("padding", pick(random, &["\"8px 12px\"", "\"1px 2px 3px 4px 5px\"", "\"0\"", "\"8px  12px\"", "\"\""]).into()),
            5 => ("height", pick(random, &["\"44px\"", "\"3em\"", "\"auto\""]).into()),
            6 => (
                "pressedOpacity",
                pick(random, &["0.5", "\".5\"", "1", "\"1.000\"", "\"1.0001\"", "0.123", "0.1234", "\"0\"", "2", "1e-3"]).into(),
            ),
            _ => ("disabledOpacity", pick(random, &["\"0.25\"", "0.3333333", "\".\"", "\"01\""]).into()),
        };
        button.push(format!("\"{k}\":{v}"));
    }
    if !button.is_empty() || random.chance(3) {
        fields.push(format!("\"button\":{{{}}}", button.join(",")));
    }
    let mut text = Vec::new();
    for _ in 0..random.below(4) {
        let (k, v): (&str, String) = match random.below(3) {
            0 => (
                "fontSize",
                pick(random, &["15", "8", "64.005", "7.99", "\"15px\"", "\"15.125px\"", "\"15.5px\"", "\"100px\"", "\"15\"", "15.555"])
                    .into(),
            ),
            1 => ("lineHeight", pick(random, &["1.35", "1.2345", "\"1.5\"", "3.0004", "\"1.25px\"", "0.9", "2.9995"]).into()),
            _ => ("padding", pick(random, &["\"8px 12px\"", "\"4px\"", "12"]).into()),
        };
        text.push(format!("\"{k}\":{v}"));
    }
    if !text.is_empty() || random.chance(3) {
        fields.push(format!("\"text\":{{{}}}", text.join(",")));
    }
    format!("{{{}}}", fields.join(","))
}

pub fn any_safe_area(random: &mut Lcg) -> String {
    let keys = ["top", "right", "bottom", "left", "cornerLeft", "cornerRight"];
    let fields: Vec<String> = (0..random.below(7))
        .map(|_| {
            let v = pick(
                random,
                &["0", "24", "64", "64.004", "64.006", "200", "199.995", "-1", "1e3", "\"12\"", "12.345", "0.005", "0.004999", "null"],
            );
            format!("\"{}\":{v}", keys[random.below(keys.len())])
        })
        .collect();
    format!("{{{}}}", fields.join(","))
}

// ---- The corpus ---------------------------------------------------------------------------------

pub fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

fn between<'a>(text: &'a [u8], open: &[u8], close: &[u8], out: &mut Vec<&'a [u8]>) {
    let mut i = 0;
    while let Some(a) = find(text, open, i) {
        let start = a + open.len();
        let Some(b) = find(text, close, start) else { break };
        out.push(&text[start..b]);
        i = b + close.len();
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// The style text of every corpus widget: its `<style>` blocks and `style="…"` attributes.
pub fn corpus_styles() -> Vec<Vec<u8>> {
    let mut files = Vec::new();
    html_files(&widget_sandbox().join("corpus"), &mut files);
    html_files(&widget_sandbox().join("packages/conformance"), &mut files);
    html_files(&widget_sandbox().join("packages/stress"), &mut files);
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read(&path) else { continue };
        let mut found = Vec::new();
        between(&text, b"<style>", b"</style>", &mut found);
        between(&text, b"style=\"", b"\"", &mut found);
        between(&text, b"font = '", b"'", &mut found);
        between(&text, b"font = \"", b"\"", &mut found);
        out.extend(found.into_iter().map(<[u8]>::to_vec));
    }
    out
}
