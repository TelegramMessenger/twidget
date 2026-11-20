//! tests/core_tests.cpp's checks of the tree the ops build and the HTML it is written as, each on what the renderer's
//! tree gives there (`last_html()`, the violations of its ops, the nodes it knows); the checks of layout in the same
//! sections are the container's. Then what the tree promises besides.

mod common;

use common::*;
use twidget_json::Json;

fn parse(text: &str) -> Json {
    Json::parse(text).unwrap_or_else(|| panic!("JSON: {text}"))
}

struct Page {
    doc: Document,
    host: StubHost,
}

impl Page {
    fn new() -> Page {
        Page { doc: Document::new(false), host: StubHost::new() }
    }

    fn with_page() -> Page {
        let mut p = Page::new();
        p.ops(PAGE);
        p
    }

    /// An `ops` message; what it answers.
    fn ops(&mut self, ops: &str) -> Vec<Json> {
        let mut m = Json::object();
        m.set("type", "ops").set("ops", parse(ops));
        receive(&mut self.doc, &mut self.host, &m)
    }

    fn send(&mut self, ops: Json) -> Vec<Json> {
        let mut m = Json::object();
        m.set("type", "ops").set("ops", ops);
        receive(&mut self.doc, &mut self.host, &m)
    }

    fn html(&self) -> String {
        let overrides = StyleOverrides::new();
        String::from_utf8(self.doc.serialize(&Chrome { overrides: &overrides, editing: 0 })).expect("UTF-8")
    }

    /// The opening tag of node `id` as written.
    fn tag(&self, id: i64) -> String {
        let html = self.html();
        let start = html.find(&format!("data-twid=\"{id}\"")).expect("the node is written");
        html[start..start + html[start..].find('>').expect("a tag")].to_string()
    }
}

fn refused(out: &[Json], what: &str) -> usize {
    out.iter().filter(|m| m.get("type").as_str() == b"violation" && m.get("detail").as_str().starts_with(what.as_bytes())).count()
}

fn deepest_div(html: &str) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    let mut at = 0;
    while let Some(p) = html[at..].find('<') {
        let i = at + p;
        if html[i..].starts_with("<div") {
            depth += 1;
            deepest = deepest.max(depth);
        } else if html[i..].starts_with("</div") {
            depth -= 1;
        }
        at = i + 1;
    }
    deepest
}

// ── light-dark() ──
#[test]
fn light_dark() {
    let mut p = Page::with_page();
    assert!(p.html().contains("#010203") && !p.html().contains("light-dark"), "light-dark() resolves to the light side in the HTML");
    p.doc.set_dark(true);
    assert!(p.html().contains("#040506"), "light-dark() follows the theme");
}

// ── names ──
#[test]
fn names() {
    let mut p = Page::with_page();
    let out = p.ops(r#"[{"op":"style","id":12,"name":"color:red;x","value":"blue"}]"#);
    assert!(
        refused(&out, "property name") == 1 && !p.html().contains("color:red;x"),
        "a property name that would break out of its declaration is refused"
    );
}

// ── inline style: the attribute and the style ops are one list ──
#[test]
fn inline_style() {
    let mut p = Page::with_page();
    p.ops(
        r#"[{"op":"el","id":60,"tag":"div","attrs":[["style","position:absolute;left:3px;top:-83px;width:56px"]]},{"op":"ins","parent":4,"id":60,"before":null},
        {"op":"style","id":60,"name":"position","value":""},{"op":"style","id":60,"name":"top","value":""},{"op":"style","id":60,"name":"margin","value":"3px"}]"#,
    );
    let tag = p.tag(60);
    assert!(
        !tag.contains("position") && !tag.contains("top:") && tag.contains("left:3px") && tag.contains("margin:3px"),
        "a style op removes what the style attribute set ({tag})"
    );
    p.ops(r#"[{"op":"attr","id":60,"name":"style","value":"color:red"}]"#);
    let tag = p.tag(60);
    assert!(tag.contains("color:red") && !tag.contains("margin"), "setting the style attribute replaces every inline declaration ({tag})");
}

fn chain(levels: i64) -> String {
    let mut ops = String::from(r#"{"op":"el","id":100000,"tag":"div","attrs":[]}"#);
    for i in 1..levels {
        let id = 100000 + i;
        ops += &format!(r#",{{"op":"el","id":{id},"tag":"div","attrs":[]}},{{"op":"ins","parent":{id},"id":{},"before":null}}"#, id - 1);
    }
    ops
}

// ── trees deeper than a document may be ──
#[test]
fn trees_deeper_than_a_document_may_be() {
    let levels = 100000;
    let top = 100000 + levels - 1;
    let mut d = Page::with_page();
    let out = d.ops(&format!(r#"[{},{{"op":"ins","parent":4,"id":{top},"before":null}}]"#, chain(levels)));
    let deepest = deepest_div(&d.html());
    assert!(deepest > 0 && deepest <= 1024, "a chain of 100000 nested elements is not built past 1024 levels ({deepest})");
    assert!(refused(&out, "tree depth") > 0);
    let mut f = Page::with_page();
    f.ops(&format!(r#"[{},{{"op":"rm","id":{top}}}]"#, chain(levels)));
    assert!(f.html().contains("id=\"a\""), "and a detached one is removed without recursing through it");
}

// ── canvases drawn from one another down a long chain ──
#[test]
fn canvases_drawn_from_one_another_down_a_long_chain() {
    let mut c = Page::with_page();
    let ops: Vec<String> = (0..200000)
        .map(|i| format!(r#"{{"op":"canvas","id":{},"cmds":[["c","drawImage",{{"n":{}}},0,0]]}}"#, 1000 + i, 1001 + i))
        .collect();
    c.ops(&format!("[{}]", ops.join(",")));
    assert!(
        c.html().contains("id=\"a\""),
        "a batch of 200000 canvas ops, each drawing the next one's canvas, does not recurse through them all"
    );
}

// ── a style element where the parser reads markup: its sheet is text, never tags ──
#[test]
fn a_style_element_where_the_parser_reads_markup() {
    let mut t = Page::new();
    t.ops(
        r##"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":10,"tag":"div","attrs":[["id","real"],["style","width:40px;height:20px"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":11,"tag":"svg","ns":"svg","attrs":[["width","10"],["height","10"]]},{"op":"ins","parent":4,"id":11,"before":null},
        {"op":"el","id":12,"tag":"g","ns":"svg","attrs":[]},{"op":"ins","parent":11,"id":12,"before":null},
        {"op":"el","id":13,"tag":"style","ns":"svg","attrs":[]},{"op":"ins","parent":12,"id":13,"before":null},
        {"op":"css","id":13,"text":"#real{margin-left:7px} a::before{content:\"<div data-twid='10' style='width:222px;height:111px'><title>&amp;<plaintext>\"} body>#after{width:77px}"},
        {"op":"el","id":14,"tag":"div","attrs":[["id","after"],["style","height:5px"]]},{"op":"ins","parent":4,"id":14,"before":null}]"##,
    );
    assert!(!t.html().contains("<div data-twid='10'"), "markup in the sheet of a style element in svg content makes no element");
    t.ops(
        r##"[{"op":"css","id":13,"text":"#real{margin-left:8px} body>#after{width:77px} /* <div data-twid='10' style='width:222px'> */ <div data-twid='10' style='width:222px'></div> <!--<div data-twid='10' style='width:222px'>-->"}]"##,
    );
    assert!(!t.html().contains("<div data-twid='10'"), "nor does markup in a comment of the sheet or between its rules");
    t.ops(
        r#"[{"op":"el","id":30,"tag":"style","attrs":[]},{"op":"ins","parent":3,"id":30,"before":null},
        {"op":"css","id":30,"text":"<!--\n#gen{float:left;height:5px}#gen::before{content:\"<b>\\<&</b>\"}\n-->@media (100px<width){body>#real{margin-left:9px}}"},
        {"op":"el","id":31,"tag":"div","attrs":[["id","gen"]]},{"op":"ins","parent":4,"id":31,"before":null}]"#,
    );
    let html = t.html();
    assert!(
        !html.contains("<b>") && html.contains("\\3c &\\3c /b>") && html.contains("(100px< width)"),
        "a string of a sheet keeps its text without its `<`: {html}"
    );
}

// ── what ops hold: a batch, a document's text, a node's attributes, the sheets' blocks and the nodes' bytes are bounded natively ──
#[test]
fn what_ops_hold() {
    let el = |id: i64| {
        let mut o = Json::object();
        o.set("op", "el").set("id", id).set("tag", "div").set("attrs", Json::array());
        o
    };
    let rm = || {
        let mut o = Json::object();
        o.set("op", "rm").set("id", 1);
        o
    };
    {
        let mut r = Page::with_page();
        let mut ops = Json::array();
        ops.push(el(9000));
        for _ in 0..200000 {
            ops.push(rm());
        }
        let out = r.send(ops);
        assert!(refused(&out, "ops limit") == 1 && r.doc.node(9000).is_none(), "a batch of 200,001 ops is not applied");
        let mut ops = Json::array();
        ops.push(el(9000));
        for _ in 0..199999 {
            ops.push(rm());
        }
        let out = r.send(ops);
        assert!(refused(&out, "ops limit") == 0 && r.doc.node(9000).is_some(), "one of 200,000 is");
    }
    {
        let mut r = Page::with_page();
        let words = |n: usize| "a ".repeat(n);
        let text = |r: &mut Page, id: i64, n: usize| r.ops(&format!(r#"[{{"op":"text","id":{id},"data":"{}"}}]"#, words(n)));
        assert_eq!(refused(&text(&mut r, 100, 60000), "text limit"), 1, "a text of 120,000 words and spaces is refused");
        assert!(
            refused(&text(&mut r, 101, 40000), "text limit") == 0 && refused(&text(&mut r, 102, 20000), "text limit") == 1,
            "a document's text nodes hold 100,000 together"
        );
        let out = r.ops(&format!(r#"[{{"op":"data","id":101,"data":"{}"}}]"#, words(60000)));
        assert_eq!(refused(&out, "text limit"), 1, "a text node does not grow past them");
        r.ops(&format!(r#"[{{"op":"data","id":101,"data":"{}"}}]"#, words(10000)));
        assert!(
            refused(&text(&mut r, 102, 20000), "text limit") == 0
                && refused(&text(&mut r, 103, 20000), "text limit") == 0
                && refused(&text(&mut r, 104, 20000), "text limit") == 1,
            "text made shorter is counted off"
        );
        r.ops(r#"[{"op":"rm","id":102},{"op":"rm","id":103}]"#);
        assert_eq!(refused(&text(&mut r, 105, 40000), "text limit"), 0, "and so is text removed");
    }
    {
        let mut r = Page::with_page();
        let mut ops = vec![r#"{"op":"el","id":100,"tag":"div","attrs":[]}"#.to_string()];
        ops.extend((0..4097).map(|i| format!(r#"{{"op":"attr","id":100,"name":"data-a{i}","value":"1"}}"#)));
        ops.extend((0..4097).map(|i| format!(r#"{{"op":"style","id":100,"name":"--p{i}","value":"1","important":false}}"#)));
        ops.push(r#"{"op":"attr","id":100,"name":"data-a7","value":"2"}"#.to_string());
        ops.push(r#"{"op":"style","id":100,"name":"--p7","value":"2","important":false}"#.to_string());
        let out = r.ops(&format!("[{}]", ops.join(",")));
        assert!(
            refused(&out, "attribute count") == 1 && refused(&out, "declaration count") == 1,
            "an element takes 4096 attributes and 4096 declarations"
        );
        let n = r.doc.get(r.doc.node(100).expect("node 100"));
        assert!(
            n.attrs.iter().any(|(k, v)| k == b"data-a7" && v == b"2") && n.styles.iter().any(|(k, v)| k == b"--p7" && v == b"2"),
            "and still changes those it has"
        );
    }
    {
        let mut r = Page::with_page();
        let rules = "a{}".repeat(9000);
        let sheet = |r: &mut Page, id: i64, css: &str| {
            r.ops(&format!(r#"[{{"op":"el","id":{id},"tag":"style","attrs":[]}},{{"op":"css","id":{id},"text":"{css}"}}]"#))
        };
        assert!(
            refused(&sheet(&mut r, 100, &rules), "stylesheet limit") == 0 && refused(&sheet(&mut r, 101, &rules), "stylesheet limit") == 1,
            "a document's sheets hold 16,384 blocks together"
        );
        r.ops(r#"[{"op":"css","id":100,"text":"a{}"}]"#);
        assert_eq!(refused(&sheet(&mut r, 101, &rules), "stylesheet limit"), 0, "a sheet replaced is counted off");
        r.ops(r#"[{"op":"rm","id":101}]"#);
        assert_eq!(refused(&sheet(&mut r, 102, &rules), "stylesheet limit"), 0, "and so is one removed");
    }
    {
        let mut r = Page::with_page();
        let value = "A".repeat(13 << 20);
        r.ops(r#"[{"op":"el","id":100,"tag":"div","attrs":[]}]"#);
        let mut stopped = 0;
        for _ in 0..6 {
            stopped += refused(&r.ops(&format!(r#"[{{"op":"attr","id":100,"name":"data-a","value":"{value}"}}]"#)), "tree limit");
        }
        assert!(
            stopped == 0 && !r.html().contains("data-tw-badge"),
            "78 MB set on one attribute in turn is 13 MB held: the widget goes on"
        );
        for i in 0..5 {
            stopped += refused(&r.ops(&format!(r#"[{{"op":"attr","id":100,"name":"data-b{i}","value":"{value}"}}]"#)), "tree limit");
        }
        assert!(
            stopped == 1 && r.html().contains("data-tw-badge=\"fatal\""),
            "past 48 MB held in its nodes the widget is stopped, as past the layout limit"
        );
        r.ops(r#"[{"op":"el","id":200,"tag":"div","attrs":[]}]"#);
        assert!(r.doc.node(200).is_none(), "and a stopped widget's ops are not applied");
    }
}

// ── an attribute of the widget's never stands in for the renderer's own data-twid or style, nor for the fork's key ──
#[test]
fn an_attribute_never_stands_in_for_the_renderers_own() {
    let mut t = Page::new();
    t.ops(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[["STYLE","--tg-theme-bg-color:#123456 !important"]]},
        {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":10,"tag":"div","attrs":[["style","width:40px;height:20px"],["data-twid","11"],["data-lh-id","k"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":0,"tag":"div","attrs":[["data-twid","10"],["Data-TwId","10"],["style","width:222px;height:111px"]]},{"op":"ins","parent":4,"id":0,"before":null},
        {"op":"el","id":12,"tag":"div","attrs":[["STYLE","width:99px"],["Style","width:98px"],["style","height:7px"],["Data-LH-Id","k"]]},{"op":"ins","parent":4,"id":12,"before":null}]"#,
    );
    let html = t.html();
    assert!(
        !html.contains("data-twid=\"11\"") && !html.contains("222px"),
        "an element cannot take a node's id by a data-twid attribute of its own"
    );
    assert!(
        !html.contains("#123456") && !html.contains("99px") && t.tag(12).contains("style=\"height:7px\""),
        "a style attribute spelt in another case does not replace the one the renderer writes"
    );
    assert!(!html.to_ascii_lowercase().contains("data-lh-id"), "the key the fork pairs children by is not an attribute a widget can write");
}

// ── an adopt op that names a live node's id ──
#[test]
fn an_adopt_op_that_names_a_live_nodes_id() {
    let mut a = Page::with_page();
    a.ops(r#"[{"op":"text","id":30,"data":"kept"},{"op":"ins","parent":11,"id":30,"before":null}]"#);
    a.ops(r#"[{"op":"el","id":11,"tag":"head","adopt":"head","attrs":[]}]"#);
    assert!(a.html().contains("kept"), "an adopt op naming a live node's id is ignored: the node stays in the tree");
    a.ops(r#"[{"op":"rm","id":10}]"#);
    assert!(!a.html().contains("kept") && a.html().contains("id=\"b\""), "and its parent is removed with it afterwards");
}

// ── a textarea whose value replaced its children, then removed ──
#[test]
fn a_textarea_whose_value_replaced_its_children() {
    let mut t = Page::with_page();
    t.ops(
        r#"[{"op":"el","id":50,"tag":"textarea","attrs":[]},{"op":"text","id":51,"data":"old"},{"op":"ins","parent":50,"id":51,"before":null},
        {"op":"ins","parent":4,"id":50,"before":null},{"op":"prop","id":50,"name":"value","value":"new"},{"op":"rm","id":50}]"#,
    );
    t.ops(r#"[{"op":"ins","parent":4,"id":51,"before":null}]"#);
    assert!(
        t.html().contains("old") && !t.html().contains("<textarea"),
        "the text a textarea's value replaced does not point at the textarea once that is removed"
    );
}

// ── ids: an element, a text or a second root that names an adopted root's id is ignored ──
#[test]
fn ids_naming_an_adopted_roots_id() {
    let mut d = Page::new();
    d.ops(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},{"op":"el","id":5,"tag":"style","attrs":[]},
        {"op":"css","id":5,"text":".k{width:50px;height:50px}"},
        {"op":"ins","parent":3,"id":5,"before":null},{"op":"el","id":10,"tag":"div","attrs":[]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"el","id":4,"tag":"div","attrs":[["class","k"]]},{"op":"ins","parent":10,"id":4,"before":null},
        {"op":"text","id":3,"data":"again"},{"op":"ins","parent":10,"id":3,"before":null},
        {"op":"el","id":2,"tag":"body","adopt":"body","attrs":[]}]"#,
    );
    let html = d.html();
    assert!(
        html.find("data-twid=\"4\"") == html.rfind("data-twid=\"4\"") && html.contains("<body data-twid=\"4\""),
        "an element that names the body's id is ignored"
    );
    assert!(!html.contains("again"), "and a text that names the head's");
    assert!(
        html.contains("<html data-twid=\"2\"") && !html.contains("<body data-twid=\"2\""),
        "and a root adopted under another root's id keeps its own"
    );
}

// ── ids: a number that fits no int names no node ──
#[test]
fn ids_that_fit_no_int() {
    let mut d = Page::new();
    d.ops(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},
        {"op":"el","id":1e300,"tag":"div","attrs":[["class","huge"]]},{"op":"ins","parent":4,"id":1e300,"before":-1e300},
        {"op":"el","id":-1e300,"tag":"div","attrs":[["class","low"]]},{"op":"ins","parent":4,"id":-1e300,"before":null},
        {"op":"el","id":4294967306,"tag":"canvas","attrs":[["class","wide"]]},{"op":"ins","parent":4,"id":4294967306,"before":null},
        {"op":"canvas","id":4294967306,"cmds":[["c","drawImage",{"n":1e300},0,0]]},{"op":"canvas","id":1e300,"cmds":[["c","fillRect",0,0,5,5]]},
        {"op":"text","id":1e19,"data":"stray"},{"op":"ins","parent":4,"id":1e19,"before":null},{"op":"ins","parent":1e300,"id":4,"before":null},
        {"op":"el","id":0,"tag":"div","attrs":[["class","zero"]]},{"op":"ins","parent":4,"id":0,"before":null},
        {"op":"el","id":10,"tag":"div","attrs":[["class","plain"]]},{"op":"ins","parent":4,"id":10,"before":null}]"#,
    );
    let html = d.html();
    assert!(
        !html.contains("huge") && !html.contains("low") && !html.contains("wide") && !html.contains("stray"),
        "an id that fits no int makes no node"
    );
    assert!(!html.contains("zero") && html.contains("plain"), "nor does 0, and the nodes around them are made");
}

// ── nodes: a renderer holds at most the 50,000 the validator lets a widget have ──
#[test]
fn nodes_at_most_50000() {
    let mut d = Page::new();
    let mut ops = vec![
        r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]}"#
            .to_string(),
    ];
    ops.extend((100..100 + 50002).map(|id| format!(r#"{{"op":"text","id":{id},"data":"t{id}"}}"#)));
    ops.push(r#"{"op":"el","id":60000,"tag":"p","attrs":[["class","late"]]},{"op":"ins","parent":4,"id":60000,"before":null}"#.to_string());
    ops.push(
        r#"{"op":"ins","parent":4,"id":100,"before":null},{"op":"ins","parent":4,"id":50099,"before":null},{"op":"ins","parent":4,"id":50101,"before":null}"#
            .to_string(),
    );
    d.ops(&format!("[{}]", ops.join(",")));
    let html = d.html();
    assert!(html.contains("t100") && html.contains("t50099"), "the first 50,000 nodes are made");
    assert!(!html.contains("t50101") && !html.contains("late"), "and no node past them");
    d.ops(r#"[{"op":"rm","id":100},{"op":"el","id":60000,"tag":"p","attrs":[["class","late"]]},{"op":"ins","parent":4,"id":60000,"before":null}]"#);
    assert!(d.html().contains("late"), "a removed node makes room for a new one");
}

// ── nesting: a document is built 128 levels deep, which is what its walks cost in stack ──
#[test]
fn nesting_128_levels() {
    let mut n = Page::new();
    let mut ops = vec![
        r#"{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]}"#
            .to_string(),
    ];
    for i in 0..300 {
        let (id, parent) = (1000 + i, if i > 0 { 999 + i } else { 4 });
        ops.push(format!(r#"{{"op":"el","id":{id},"tag":"div","attrs":[]}},{{"op":"ins","parent":{parent},"id":{id},"before":null}}"#));
        if i % 100 == 99 {
            ops.push(format!(
                r#"{{"op":"text","id":{},"data":"at{}"}},{{"op":"ins","parent":{id},"id":{},"before":null}}"#,
                2000 + i,
                i + 1,
                2000 + i
            ));
        }
    }
    n.ops(&format!("[{}]", ops.join(",")));
    let html = n.html();
    assert!(
        html.contains("at100") && !html.contains("at200") && deepest_div(&html) == 126,
        "300 nested elements are written to the 128th level, <html> the first"
    );
}

// ── elements that are not tags ──
#[test]
fn inline_styles_and_classes_on_a_style_element() {
    let mut t = Page::new();
    t.ops(
        r#"[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
        {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
        {"op":"el","id":20,"tag":"style","attrs":[["id","s"]]},{"op":"css","id":20,"text":".x{color:red}"},{"op":"ins","parent":3,"id":20,"before":null},
        {"op":"el","id":10,"tag":"div","attrs":[["class","x"]]},{"op":"ins","parent":4,"id":10,"before":null},
        {"op":"text","id":11,"data":"anchor"},{"op":"ins","parent":10,"id":11,"before":null}]"#,
    );
    let style =
        |t: &mut Page, op: &str, name: &str, value: &str| t.ops(&format!(r#"[{{"op":"{op}","id":20,"name":"{name}","value":"{value}"}}]"#));
    style(&mut t, "attr", "style", "transform:translateX(1px)");
    for v in ["translateX(2px)", "translateX(3px)", "none"] {
        style(&mut t, "style", "transform", v);
    }
    for v in ["0.5", "0.25", "1"] {
        style(&mut t, "style", "opacity", v);
    }
    for v in ["10px", "20px"] {
        style(&mut t, "style", "width", v);
    }
    for v in ["a", "b", ""] {
        style(&mut t, "attr", "class", v);
    }
    let tag = t.tag(20);
    assert!(
        tag.contains("width:20px") && tag.contains("class=\"\"") && t.html().contains(".x{color:red}"),
        "a <style> element's inline styles and classes are its attributes ({tag})"
    );
}

// ── besides: what the tree promises ──

#[test]
fn the_frozen_chrome() {
    let mut p = Page::with_page();
    let mut host = StubHost::new();
    p.doc.freeze(b"Paused <now>", false, &mut host);
    let html = p.html();
    assert!(html.contains("<html data-twid=\"2\" data-tw-frozen=\"paused\""), "{html}");
    assert!(
        html.contains("<style>[data-tw-frozen=fatal]") && html.contains("<div data-tw-badge=\"paused\">Paused &lt;now&gt;</div></body>"),
        "{html}"
    );
    p.doc.stop(b"layout limit", b"Layout limit exceeded", &mut host);
    assert!(
        p.html().starts_with("<!DOCTYPE html><html data-tw-frozen=\"fatal\"><head><style>")
            && p.html().contains(">Paused &lt;now&gt;</div>"),
        "frozen first, its reason stays"
    );
}

#[test]
fn canvases_are_written_by_their_bitmap() {
    let mut p = Page::with_page();
    p.ops(
        r#"[{"op":"el","id":40,"tag":"canvas","attrs":[["width","64"],["height"," 32"],["src","x"],["class","c"]]},{"op":"ins","parent":4,"id":40,"before":null},
        {"op":"canvas","id":40,"cmds":[["c","fillRect",0,0,5,5]]}]"#,
    );
    assert_eq!(p.tag(40), "data-twid=\"40\" src=\"tw-canvas:40/64x32\" class=\"c\"");
    p.ops(r#"[{"op":"attr","id":40,"name":"height","value":"9 rows"}]"#);
    assert!(p.tag(40).contains("tw-canvas:40/64x9"));
    p.ops(r#"[{"op":"rm","id":40}]"#);
    assert!(p.doc.canvases().is_empty() && p.host.log.0.ends_with(&[b'X', 40, 0, 0, 0]), "a canvas removed is released");
}

#[test]
fn the_patch_log() {
    let mut p = Page::with_page();
    p.doc.take_patch_log();
    p.ops(r#"[{"op":"attr","id":10,"name":"class","value":"x"},{"op":"style","id":12,"name":"color","value":"red"}]"#);
    let log = p.doc.take_patch_log();
    let (a, b) = (p.doc.node(10).expect("10"), p.doc.node(12).expect("12"));
    assert!(log.valid && log.attributes[&a].contains(&b"class"[..]) && log.attributes[&b].contains(&b"style"[..]));
    p.ops(r#"[{"op":"text","id":70,"data":"x"},{"op":"ins","parent":11,"id":70,"before":null},{"op":"data","id":70,"data":"y"}]"#);
    let log = p.doc.take_patch_log();
    assert!(log.valid && log.texts.contains(&p.doc.node(11).expect("11")), "text in and out of an attached element patches its holes");
    p.ops(r#"[{"op":"attr","id":10,"name":"title","value":"x"}]"#);
    assert!(!p.doc.take_patch_log().valid, "any other attribute builds");
    p.ops(r#"[{"op":"el","id":71,"tag":"p","attrs":[]},{"op":"attr","id":71,"name":"title","value":"x"}]"#);
    assert!(p.doc.take_patch_log().valid, "a detached element changes nothing the document shows");
}

#[test]
fn the_color_scheme_in_media_queries() {
    assert_eq!(
        rewrite_color_scheme(b"@media (prefers-color-scheme: dark){a{}} @media (PREFERS-COLOR-SCHEME:light){b{}} @media (prefers-color-scheme){c{}} x-prefers-color-scheme \"prefers-color-scheme\"", true),
        b"@media (min-width:0px){a{}} @media (min-width:999999px){b{}} @media (min-width:0px){c{}} x-prefers-color-scheme \"prefers-color-scheme\"".to_vec()
    );
}

#[test]
fn what_the_renderer_adds_when_it_writes() {
    let mut p = Page::with_page();
    p.ops(r#"[{"op":"el","id":30,"tag":"input","attrs":[["placeholder","Name"],["style","color:light-dark(red,blue)"]]},{"op":"ins","parent":4,"id":30,"before":null}]"#);
    let mut overrides = StyleOverrides::new();
    overrides.insert(
        10,
        vec![(b"transform".to_vec(), b"rotate(5deg)".to_vec()), (b"a b".to_vec(), b"x".to_vec()), (b"opacity".to_vec(), b"0.5\"".to_vec())],
    );
    overrides.insert(2, vec![(b"opacity".to_vec(), b"1".to_vec())]);
    let html = String::from_utf8(p.doc.serialize(&Chrome { overrides: &overrides, editing: 30 })).expect("UTF-8");
    assert!(
        html.contains("<div data-twid=\"10\" id=\"a\" style=\"-lh-anim:1;transform:rotate(5deg);opacity:0.5&quot;\">"),
        "animated values after the marker: {html}"
    );
    assert!(
        html.contains("<input data-twid=\"30\" style=\"color:red;-lh-anim:1;color:transparent\">"),
        "the edited control without its placeholder, its text transparent: {html}"
    );
    assert!(html.contains(";-lh-anim:1;opacity:1\">"), "the root's after its theme: {html}");
}
