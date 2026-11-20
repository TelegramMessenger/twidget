//! The Rust computed style against the C++ (`oracle/computed_style_oracle`, which includes
//! core/src/computed_style.cpp whole over the litehtml fork lhdump builds): the helpers on generated input
//! (px of every kind of number, color of every byte quadruple's alpha, lengths of every unit and keyword,
//! alignments with their flags, box shorthands, decoration bits, custom property texts of the corpus's style
//! texts), and computed_style itself on real documents: widget-sandbox's corpus, conformance and stress widgets
//! and generated ones, laid out by the fork; the oracle writes down everything computed_style reads of each
//! element and its answers (the element, five pseudo-elements, touch-action, pointer-events), and the Rust
//! side gives its answers from what was written down. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py`
//! sets it.

use std::path::{Path, PathBuf};

use twidget_computed_style::*;
use twidget_litehtml::css_parser::{f_componentize, f_remove_whitespace, normalize};
use twidget_litehtml::css_values::*;
use twidget_litehtml::document::{Document, StyleCx, Switches};
use twidget_litehtml::document_container::DocumentContainer;
use twidget_litehtml::font_description::FontDescription;
use twidget_litehtml::style::DeclarationCache;
use twidget_litehtml::types::{FontMetrics, MediaFeatures};
use twidget_litehtml::web_color::WebColor;
use twidget_litehtml::{ElementId, Names, Px};
use twidget_testkit::{Comparison, Fields, Lcg, Oracle, framed};

// ---- The helpers ------------------------------------------------------------------------------------

fn table(which: u8) -> &'static [&'static str] {
    match which {
        0 => &style_display_strings,
        1 => &css_units_strings,
        2 => &border_style_strings,
        3 => &flex_align_items_strings,
        4 => &overflow_strings,
        _ => &background_size_strings,
    }
}

fn keywords(which: u8) -> CssValues {
    match which {
        1 => CssValues::new(&background_size_strings),
        2 => CssValues::new(&flex_basis_strings),
        3 => CssValues::new(&font_size_strings),
        4 => CssValues::new(&line_height_strings),
        _ => CssValues::default(),
    }
}

fn length_answer(font_size: f64, predefined: &str, options: i32, words: u8, text: &[u8]) -> (u8, Vec<u8>) {
    let tokens = normalize(text, f_componentize | f_remove_whitespace, None);
    let mut len = twidget_litehtml::css_length::CssLength::default();
    if tokens.len() != 1 || !len.from_token(&tokens[0], options, keywords(words)) {
        return (0, Vec::new());
    }
    (1, length(&Length::of(&len), font_size, predefined).into_bytes())
}

fn special_double(random: &mut Lcg) -> f64 {
    match random.below(12) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => -0.0,
        3 => 1e300,
        4 => 0.0005,
        5 => -0.0005,
        6 => 2.0005,
        7 => f64::from_bits(u64::from(random.next()) << 32 | u64::from(random.next())),
        _ => (random.below(200000) as f64 - 100000.0) / 1000.0 + random.below(10) as f64 * 1e-4,
    }
}

const UNITS: [&str; 18] = ["", "%", "px", "em", "rem", "pt", "in", "cm", "mm", "pc", "ex", "ch", "vw", "vh", "vmin", "vmax", "q", "foo"];

#[test]
fn helpers_alike() {
    let Some(oracle) = Oracle::find("computed_style_oracle") else { return };
    let mut random = Lcg::new(0xc5c5_0001);
    let mut c = Comparison::new();
    for i in 0..20000 {
        let v = special_double(&mut random);
        c.case(&framed(b'p', &v.to_bits().to_le_bytes()), (1, px(v).into_bytes()), format!("px {i}: {v}"));
    }
    for a in 0..=255u8 {
        for k in 0..4 {
            let (r, g, b) = (random.next() as u8, random.next() as u8, (a as usize * 7 + k) as u8);
            c.case(&framed(b'c', &[r, g, b, a]), (1, color(&WebColor::rgba(r, g, b, a)).into_bytes()), format!("color alpha {a}"));
        }
    }
    for which in 0..6u8 {
        for index in -3..30 {
            let mut o = Fields::new();
            o.u8(which).i32(index);
            c.case(&o.framed(b'k'), (1, keyword(table(which), index).into_bytes()), format!("keyword {which} {index}"));
        }
    }
    let predefined = ["auto", "none", "normal", "0px", "3px", "0%"];
    for i in 0..20000 {
        let text = match random.below(10) {
            0 => [
                "auto",
                "none",
                "normal",
                "cover",
                "contain",
                "content",
                "medium",
                "larger",
                "calc(10px + 2em)",
                "calc(50% - 3px)",
                "min(4px, 2em)",
                "inherit",
            ][random.below(12)]
            .to_string(),
            1 => format!("{}", special_double(&mut random)),
            _ => format!("{}{}", [0.0, 1.0, -2.5, 0.125, 33.3333, 1e6, 0.0001, -0.0][random.below(8)], UNITS[random.below(UNITS.len())]),
        };
        let font_size = [16.0, 13.0, 0.0, 1e10, 12.5][random.below(5)];
        let pre = predefined[random.below(predefined.len())];
        let options = [1, 2, 3, 4, 7, 15, 31][random.below(7)];
        let words = random.below(5) as u8;
        let mut o = Fields::new();
        o.f64(font_size).str(pre.as_bytes()).i32(options).u8(words);
        o.0.extend_from_slice(text.as_bytes());
        c.case(
            &o.framed(b'l'),
            length_answer(font_size, pre, options, words, text.as_bytes()),
            format!("length {i}: {text} at {font_size}"),
        );
    }
    for v in -2i32..0x1000 {
        c.case(&framed(b'a', &v.to_le_bytes()), (1, alignment(v).into_bytes()), format!("alignment {v:#x}"));
    }
    for i in 0..4000 {
        let pool = ["0px", "1px", "auto", "2px 3px", "", "rgb(0, 0, 0)"];
        let s: Vec<&str> = (0..4).map(|_| pool[random.below(3) + if random.chance(5) { 3 } else { 0 }]).collect();
        let mut o = Fields::new();
        for v in &s {
            o.str(v.as_bytes());
        }
        c.case(&o.framed(b'b'), (1, box_shorthand(s[0], s[1], s[2], s[3]).into_bytes()), format!("box {i}: {s:?}"));
    }
    for bits in -1i32..64 {
        c.case(&framed(b'd', &bits.to_le_bytes()), (1, text_decoration_line(bits).as_bytes().to_vec()), format!("decoration {bits}"));
    }
    let mut customs = 0;
    for text in corpus_custom_values() {
        let tokens = normalize(text.as_bytes(), f_componentize | f_remove_whitespace, None);
        c.case(&framed(b'v', text.as_bytes()), (1, custom_property_text(&tokens)), format!("custom property {text}"));
        customs += 1;
    }
    let tally = c.check(&oracle);
    eprintln!("helpers: {} answers alike ({} lengths refused alike, {customs} custom property values)", tally[1] + tally[0], tally[0]);
}

// ---- The corpus -------------------------------------------------------------------------------------

fn widget_sandbox() -> PathBuf {
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

fn corpus_pages() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        html_files(&widget_sandbox().join(dir), &mut files);
    }
    files.sort();
    // The attacks that nest flex, grid and table boxes deeply lay out in exponential time without the renderer's
    // layout budget, which the oracle's bare fork does not have.
    files.retain(|p| !p.to_string_lossy().contains("render-nested-"));
    files.into_iter().filter_map(|p| Some((p.display().to_string(), std::fs::read(&p).ok()?))).filter(|(_, b)| b.len() < 300_000).collect()
}

/// Every custom property value the corpus declares (`--name: value`), and the values of every declaration
/// with a `var(` in it.
fn corpus_custom_values() -> Vec<String> {
    let mut out = Vec::new();
    for (_, page) in corpus_pages() {
        let text = String::from_utf8_lossy(&page);
        for (at, _) in text.match_indices("--") {
            let rest = &text[at..];
            let Some(colon) = rest.find(':') else { continue };
            if colon > 40 || rest[..colon].contains(|c: char| c.is_whitespace() || c == ')' || c == '(') {
                continue;
            }
            let value = &rest[colon + 1..];
            let end = value.find([';', '}', '"', '\n']).unwrap_or(value.len());
            let value = value[..end].to_string();
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

// ---- Documents --------------------------------------------------------------------------------------

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let piece = &self.s[self.at..self.at + n];
        self.at += n;
        piece
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn flag(&mut self) -> bool {
        self.u8() != 0
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("four bytes"))
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }
    fn str(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }
    fn color(&mut self) -> WebColor {
        let (r, g, b, a) = (self.u8(), self.u8(), self.u8(), self.u8());
        WebColor::rgba(r, g, b, a)
    }
    fn len(&mut self) -> Length {
        let predefined = self.flag();
        let predef = self.i32();
        let units = CssUnits::from_index(self.i32()).expect("a unit");
        Length { predefined, predef, units, val: self.f32() }
    }
    fn border(&mut self) -> Border {
        Border { style: self.i32(), width: self.len(), color: self.color() }
    }
    fn lens<const N: usize>(&mut self) -> [Length; N] {
        std::array::from_fn(|_| self.len())
    }
    fn i32s<const N: usize>(&mut self) -> [i32; N] {
        std::array::from_fn(|_| self.i32())
    }
    fn f32s<const N: usize>(&mut self) -> [f32; N] {
        std::array::from_fn(|_| self.f32())
    }

    /// An element as the oracle's write_element writes it.
    fn element(&mut self) -> Element {
        let mut e = Element { is_tag: self.flag(), ..Element::default() };
        [e.display, e.position, e.float, e.clear, e.box_sizing, e.visibility, e.z_index, e.direction] = self.i32s();
        e.borders = std::array::from_fn(|_| self.border());
        e.radius = self.lens();
        e.margins = self.lens();
        e.padding = self.lens();
        e.width = self.len();
        e.height = self.len();
        e.border_collapse = self.i32();
        e.border_spacing_x = self.len();
        e.border_spacing_y = self.len();
        e.caption_side = self.i32();
        [e.min_width, e.min_height, e.max_width, e.max_height] = self.lens();
        e.offsets = self.lens();
        [e.overflow_x, e.overflow_y, e.white_space, e.overflow_wrap, e.word_break] = self.i32s();
        e.color = self.color();
        e.font_size = self.f32();
        e.font_weight = self.f32();
        e.font_style = self.i32();
        e.font_family = self.str();
        e.line_height_predefined = self.flag();
        e.line_height = self.f32();
        e.text_align = self.i32();
        e.text_indent = self.len();
        e.text_transform = self.i32();
        e.letter_spacing = self.f32();
        e.word_spacing = self.f32();
        e.vertical_align = self.i32();
        e.text_decoration_line = self.i32();
        e.text_decoration_style = self.i32();
        e.text_decoration_color = self.color();
        [e.user_select, e.list_style_type, e.list_style_position] = self.i32s();
        e.list_style_image = self.str();
        e.bg_color = self.color();
        e.bg_images = (0..self.u32()).map(|_| Image { kind: self.i32(), url: self.str(), gradient: self.str() }).collect();
        e.bg_sizes = (0..self.u32()).map(|_| (self.len(), self.len())).collect();
        e.bg_position_x = (0..self.u32()).map(|_| self.len()).collect();
        e.bg_position_y = (0..self.u32()).map(|_| self.len()).collect();
        e.bg_repeat = (0..self.u32()).map(|_| self.i32()).collect();
        e.opacity = self.f32();
        e.cursor = self.str();
        e.transform_origin_x = self.len();
        e.transform_origin_y = self.len();
        e.outline = self.border();
        e.outline_offset = self.f32();
        [e.flex_direction, e.flex_wrap, e.justify_content, e.align_items, e.align_self, e.align_content] = self.i32s();
        e.flex_grow = self.f32();
        e.flex_shrink = self.f32();
        e.flex_basis = self.len();
        e.order = self.i32();
        e.row_gap = self.len();
        e.column_gap = self.len();
        e.aspect_ratio = self.f32();
        e.content = self.str();
        if self.flag() {
            e.layout = Some(Layout { margins: self.f32s(), paddings: self.f32s(), borders: self.f32s() });
        }
        e.placement = (self.f32(), self.f32());
        e.raw = RAW_NAMES.iter().map(|n| (n.as_bytes().to_vec(), self.str())).collect();
        e.chain = (0..self.u32()).map(|_| (self.flag(), self.str(), self.str())).collect();
        let fill = self.flag();
        e.fill_paint = Some(self.color()).filter(|_| fill);
        let stroke = self.flag();
        e.stroke_paint = Some(self.color()).filter(|_| stroke);
        if self.flag() {
            e.before = Some(Box::new(self.element()));
        }
        if self.flag() {
            e.after = Some(Box::new(self.element()));
        }
        e
    }
}

const PSEUDOS: [&str; 5] = ["::before", ":after", "::marker", ":before", "::after"];

fn animated_for(index: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
    if index % 3 != 1 {
        return Vec::new();
    }
    [
        ("filter", "blur(1px)"),
        ("transform", " rotate(45deg) "),
        ("filter", "brightness(2)"),
        ("fill", "#f00"),
        ("box-shadow", "0 0 3px red"),
    ]
    .iter()
    .map(|(n, v)| (n.as_bytes().to_vec(), v.as_bytes().to_vec()))
    .collect()
}

/// A document of nested elements with style attributes drawn from every property computed_style reads, rules
/// that make pseudo-elements, and SVG shapes with paints.
fn any_document(random: &mut Lcg) -> Vec<u8> {
    let declarations = [
        "display:block",
        "display:inline",
        "display:flex",
        "display:inline-block",
        "display:grid",
        "display:list-item",
        "display:none",
        "display:table",
        "position:relative",
        "position:absolute;top:3px;left:10%",
        "position:fixed;right:1em;bottom:auto",
        "position:sticky;top:0",
        "z-index:5",
        "float:left",
        "clear:both",
        "box-sizing:border-box",
        "visibility:hidden",
        "direction:rtl",
        "margin:4px 1em",
        "margin:auto",
        "margin:10% 2px 3px 4px",
        "margin:1in 2cm 3mm 1pc",
        "margin-left:2vw",
        "margin-top:1ex",
        "padding:3px",
        "padding:1em 2% 3pt",
        "width:120px",
        "width:50%",
        "width:calc(100% - 20px)",
        "height:2em",
        "height:auto",
        "min-width:10px",
        "max-width:80%",
        "min-height:1rem",
        "max-height:none",
        "border:2px solid #f00",
        "border:1px dashed rgba(0,0,0,.5)",
        "border-top:3px double blue",
        "border-left:thin dotted",
        "border-radius:4px",
        "border-radius:10% 3px",
        "border-radius:1em/2em",
        "border-collapse:collapse",
        "border-spacing:2px 4px",
        "caption-side:bottom",
        "overflow:hidden",
        "overflow-x:auto",
        "overflow:scroll visible",
        "white-space:nowrap",
        "white-space:pre-wrap",
        "overflow-wrap:anywhere",
        "word-break:break-all",
        "color:#123456",
        "color:rgba(10,20,30,.37)",
        "color:hsl(120 50% 50% / 0.2)",
        "font-size:13px",
        "font-size:1.5em",
        "font-size:larger",
        "font-weight:bold",
        "font-weight:300",
        "font-style:italic",
        "font-family:Georgia, serif",
        "font:italic 700 14px/1.5 monospace",
        "line-height:20px",
        "line-height:1.2",
        "line-height:normal",
        "text-align:center",
        "text-align:end",
        "text-indent:2em",
        "text-transform:uppercase",
        "letter-spacing:1px",
        "letter-spacing:0.1em",
        "word-spacing:3px",
        "vertical-align:middle",
        "vertical-align:-2px",
        "text-decoration:underline",
        "text-decoration:underline overline line-through wavy red",
        "text-decoration-line:line-through",
        "user-select:text",
        "user-select:none",
        "list-style-type:square",
        "list-style:inside decimal",
        "list-style-image:url(a.png)",
        "quotes:'<' '>'",
        "quotes:inherit",
        "background:#eee",
        "background:url(x.png) no-repeat 10px 20%",
        "background:linear-gradient(red, blue)",
        "background-size:cover",
        "background-size:10px auto",
        "background-repeat:repeat-x",
        "background:url(a.png), radial-gradient(red, blue)",
        "opacity:0.5",
        "opacity:.333",
        "cursor:pointer",
        "pointer-events:none",
        "pointer-events:initial",
        "pointer-events:inherit",
        "touch-action:none",
        "touch-action: pan-x ",
        "transform:rotate(30deg)",
        "transform:translate(10px,5px) scale(2)",
        "transform-origin:0 0",
        "transform-origin:30% 2em",
        "translate:10px",
        "rotate:45deg",
        "scale:2",
        "filter:blur(2px)",
        "backdrop-filter:blur(3px)",
        "box-shadow:0 1px 2px #000",
        "outline:2px solid red",
        "outline:1px dotted",
        "outline-offset:3px",
        "flex-direction:column",
        "flex-wrap:wrap",
        "justify-content:space-between",
        "align-items:center",
        "align-items:safe end",
        "align-self:last baseline",
        "align-items:unsafe flex-start",
        "align-content:space-around",
        "flex:1 1 30%",
        "flex-grow:2",
        "flex-shrink:0.5",
        "flex-basis:content",
        "order:-1",
        "gap:4px",
        "row-gap:1em",
        "column-gap:normal",
        "aspect-ratio:16/9",
        "aspect-ratio:1.5",
        "fill:url(#g)",
        "fill:#0f0",
        "stroke:currentColor",
        "stroke:none",
        "fill:inherit",
        "font-variant-numeric:tabular-nums",
        "--x:8px 12px",
        "--y:1px,2px",
    ];
    let tags = ["div", "span", "p", "ul", "li", "b", "section", "table", "td", "button", "img"];
    let mut out = String::from(
        "<!DOCTYPE html><html><head><style>.a::before{content:'x';color:red;display:block} .b::after{content:\"y z\";margin:2px} .c::before{content:counter(n)} .d::after{content:none}</style></head><body>",
    );
    let mut open = Vec::new();
    for _ in 0..10 + random.below(40) {
        if !open.is_empty() && random.chance(3) {
            let tag = open.pop().expect("an open tag");
            out += &format!("</{tag}>");
            continue;
        }
        if random.chance(8) {
            out += r##"<svg width="40" height="20"><rect width="10" height="10" fill="#f00"/><circle r="5" stroke="blue" style="fill:none"/><g fill="green"><rect width="3" height="3"/></g></svg>"##;
            continue;
        }
        let tag = tags[random.below(tags.len())];
        let style: Vec<&str> = (0..random.below(5)).map(|_| declarations[random.below(declarations.len())]).collect();
        let class = ["", "a", "b", "c", "d", "a b"][random.below(6)];
        out += &format!(r#"<{tag} class="{class}" style="{}">"#, style.join(";"));
        if random.chance(2) {
            out += "text ";
        }
        if tag == "img" {
            continue;
        }
        open.push(tag);
    }
    while let Some(tag) = open.pop() {
        out += &format!("</{tag}>");
    }
    out += "</body></html>";
    out.into_bytes()
}

// ---- The cascade's elements ---------------------------------------------------------------------------

/// The oracle's container: fonts 16 px with fixed metrics (every font handle 1), 8 px a byte of text, a 375 x 667
/// screen at 2x, no imports.
struct CsContainer;

impl twidget_litehtml::web_color::ResolveColor for CsContainer {
    fn resolve_color(&self, _name: &[u8]) -> Vec<u8> {
        Vec::new()
    }
}

impl DocumentContainer for CsContainer {
    fn pt_to_px(&self, pt: f32) -> Px {
        Px(pt * 96.0 / 72.0)
    }
    fn get_default_font_size(&self) -> Px {
        Px(16.0)
    }
    fn import_css(&self, _url: &[u8], _baseurl: &mut Vec<u8>) -> Vec<u8> {
        Vec::new()
    }
    fn get_media_features(&self) -> MediaFeatures {
        MediaFeatures {
            ty: MediaType::media_type_screen.index(),
            width: Px(375.0),
            height: Px(667.0),
            device_width: Px(375.0),
            device_height: Px(667.0),
            color: 8,
            color_index: 256,
            monochrome: 0,
            resolution: Px(192.0),
        }
    }
    fn create_font(&self, d: &FontDescription) -> (u64, FontMetrics) {
        let size = d.size;
        let fm = FontMetrics {
            font_size: size,
            ascent: size * 0.75,
            descent: size * 0.25,
            height: size * 1.25,
            x_height: size * 0.5,
            ch_width: size * 0.5,
            ..FontMetrics::default()
        };
        (1, fm)
    }
    fn text_width(&self, text: &[u8], _font: u64) -> Px {
        Px((text.len() * 8) as f32)
    }
    fn get_default_font_name(&self) -> Vec<u8> {
        b"sans-serif".to_vec()
    }
}

/// litehtml's `master_css` (litehtml/include/litehtml/master_css.h), the raw string's text.
fn master_css() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../litehtml/include/litehtml/master_css.h");
    let text = std::fs::read(&path).expect("master_css.h");
    let text = String::from_utf8_lossy(&text).replace("\r\n", "\n");
    let start = text.find("R\"##(").expect("the raw string") + 5;
    let end = text[start..].find(")##\"").expect("its end") + start;
    text.as_bytes()[start..end].to_vec()
}

/// A document as the oracle builds it (createFromString with litehtml's master sheet), cascaded by the port,
/// and its elements in the oracle's order (the tree from the root, 300 at most).
fn cascade(names: &Names, cache: &DeclarationCache, master: &[u8], html: &[u8]) -> (Document, Vec<ElementId>) {
    let cx = StyleCx { names, container: &CsContainer, cache, switches: Switches::default() };
    let (mut doc, root) = Document::build(html, &cx.env());
    doc.finalize_from_external_root(&cx, root, master, b"");
    fn collect(doc: &Document, id: ElementId, out: &mut Vec<ElementId>) {
        if out.len() >= 300 {
            return;
        }
        out.push(id);
        for &child in doc.children(id) {
            collect(doc, child, out);
        }
    }
    let mut nodes = Vec::new();
    if let Some(root) = doc.root {
        collect(&doc, root, &mut nodes);
    }
    (doc, nodes)
}

/// What layout gave the C++'s element, taken into the cascade's: the boxes and placement and an image's line height
/// (its height); how many fields were taken. (A table cell's width, its column's, the port's render tree gives.)
fn with_layout(ours: &mut Element, theirs: &Element, replaced: bool) -> usize {
    ours.layout = theirs.layout;
    ours.placement = theirs.placement;
    let mut taken = 0;
    if replaced && ours.line_height != theirs.line_height {
        ours.line_height = theirs.line_height;
        taken += 1;
    }
    for (o, t) in [(&mut ours.before, &theirs.before), (&mut ours.after, &theirs.after)] {
        if let (Some(o), Some(t)) = (o, t) {
            taken += with_layout(o, t, false);
        }
    }
    taken
}

/// Runs the documents through the oracle and compares every element's answers with the port's.
fn documents_alike(oracle: &Oracle, documents: &[(String, Vec<u8>)]) -> (usize, usize) {
    let mut input = Vec::new();
    for (_, html) in documents {
        let mut o = Fields::new();
        o.i32(375);
        o.0.extend_from_slice(html);
        input.extend(o.framed(b'S'));
    }
    let answers = oracle.run(input, documents.len());
    let master = master_css();
    let mut elements = 0;
    let mut answers_alike = 0;
    let mut cascaded = 0;
    let mut layout_taken = 0;
    let mut differences = Vec::new();
    let mut differing = 0;
    for ((name, html), (status, bytes)) in documents.iter().zip(&answers) {
        if *status != 1 {
            continue;
        }
        // The oracle's createFromString decodes as the text says; the port's build reads UTF-8.
        let utf8 = std::str::from_utf8(html).is_ok() && !html.starts_with(b"\xEF\xBB\xBF");
        let (names, cache) = (Names::new(), DeclarationCache::default());
        let (doc, nodes) = if utf8 { cascade(&names, &cache, &master, html) } else { (Document::new(), Vec::new()) };
        let cx = StyleCx { names: &names, container: &CsContainer, cache: &cache, switches: Switches::default() };
        let mut r = Reader { s: bytes, at: 0 };
        let count = r.u32() as usize;
        if utf8 && nodes.len() != count {
            differences.push(format!("{name}: the C++ has {count} elements, the port's cascade {}", nodes.len()));
            break;
        }
        for index in 0..count {
            let inputs = r.str();
            let theirs = Reader { s: &inputs, at: 0 }.element();
            elements += 1;
            // The element computed_style reads: the cascade's, with what layout gave the C++'s.
            let el = if utf8 {
                let mut ours = Element::of_cascade(&doc, &cx, nodes[index]);
                layout_taken += with_layout(&mut ours, &theirs, doc.is_replaced(nodes[index]));
                cascaded += 1;
                if ours != theirs {
                    differing += 1;
                    if differences.len() < 5 {
                        let (a, b) = (format!("{theirs:?}"), format!("{ours:?}"));
                        let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(0).saturating_sub(120);
                        differences.push(format!(
                            "{name}, element {index}: the cascade's differs\n C++  …{}\n Rust …{}",
                            &a[at.min(a.len())..(at + 400).min(a.len())],
                            &b[at.min(b.len())..(at + 400).min(b.len())]
                        ));
                    }
                }
                ours
            } else {
                theirs
            };
            let animated = animated_for(index);
            let extras = if animated.is_empty() { None } else { Some(&animated[..]) };
            let mut ours = vec![computed_style(&el, b"", extras).dump()];
            for pseudo in PSEUDOS {
                ours.push(computed_style(&el, pseudo.as_bytes(), extras).dump());
            }
            ours.push(computed_touch_action(&el));
            ours.push(computed_pointer_events(&el));
            for (k, ours) in ours.iter().enumerate() {
                let theirs = r.str();
                if theirs == *ours {
                    answers_alike += 1;
                    continue;
                }
                differing += 1;
                if differences.len() < 5 {
                    differences.push(format!(
                        "{name}, element {index}, answer {k}:\n C++  {}\n Rust {}",
                        String::from_utf8_lossy(&theirs),
                        String::from_utf8_lossy(ours)
                    ));
                }
            }
        }
    }
    assert!(differences.is_empty(), "{differing} answers differ, the first:\n{}", differences.join("\n"));
    eprintln!("{cascaded} elements from the port's cascade alike with the C++'s ({layout_taken} fields of layout taken from the C++)");
    (elements, answers_alike)
}

#[test]
fn corpus_documents_alike() {
    let Some(oracle) = Oracle::find("computed_style_oracle") else { return };
    let pages = corpus_pages();
    assert!(pages.len() > 300, "the corpus is there ({} pages)", pages.len());
    let (elements, answers) = documents_alike(&oracle, &pages);
    eprintln!("corpus documents: {} pages, {elements} elements, {answers} answers alike", pages.len());
}

#[test]
fn generated_documents_alike() {
    let Some(oracle) = Oracle::find("computed_style_oracle") else { return };
    let mut random = Lcg::new(0xc5c5_0002);
    let documents: Vec<(String, Vec<u8>)> = (0..1000).map(|i| (format!("document {i}"), any_document(&mut random))).collect();
    let (elements, answers) = documents_alike(&oracle, &documents);
    eprintln!("generated documents: {} documents, {elements} elements, {answers} answers alike", documents.len());
}
