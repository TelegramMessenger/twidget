//! This stage's scripts: documents patched batch after batch (holes, style attributes, texts, canvases, SVG attributes,
//! structural changes and what has no hole), themes and safe areas, sizes, every query kind with valid, malformed and
//! limit-crossing arguments, animations sampled and queried, resize diffs, `frozen`, and documents past the layout
//! budget and the name table's bounds. Then the bot's profile page and a bubble's place, named scripts both the
//! renderer's and the document's comparisons run.

use twidget_testkit::Lcg;

use super::inputs::{document, pick, quote};
use super::{Script, Step};

pub fn el(id: i64, tag: &str, parent: i64, attrs: &[(&str, &str)]) -> String {
    let attrs: Vec<String> = attrs.iter().map(|(k, v)| format!("[{},{}]", quote(k), quote(v))).collect();
    format!(
        "{{\"op\":\"el\",\"id\":{id},\"tag\":{},\"attrs\":[{}]}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}",
        quote(tag),
        attrs.join(",")
    )
}

pub fn style_op(id: i64, name: &str, value: &str) -> String {
    format!("{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(value))
}

pub fn attr_op(id: i64, name: &str, value: &str) -> String {
    format!("{{\"op\":\"attr\",\"id\":{id},\"name\":{},\"value\":{}}}", quote(name), quote(value))
}

pub fn batch(ops: &[String]) -> Vec<u8> {
    format!("[{}]", ops.join(",")).into_bytes()
}

/// A style sheet (5) in the head.
pub fn sheet(text: &str) -> String {
    format!(
        "{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}",
        quote(text)
    )
}

/// html(2) head(3) body(4); a style sheet (5); boxes 10–17, texts 20–27 in them, an inline span (30) with a long text
/// (31), a textarea (32), a canvas (33), an SVG (34) with a rect (35), a path (36) and a text (37) holding (38), an
/// image (39), a transformed box (40), a positioned box (41) holding (42), a details (43) with a summary (44).
pub fn page(r: &mut Lcg) -> (Vec<String>, u32) {
    let mut ops = Vec::new();
    let text = pick(
        r,
        &[
            ".a{color:red;padding:2px}.b{border:1px solid blue;width:40px}@keyframes f{from{opacity:0}to{opacity:1}} .anim{animation:f .3s linear 2}.t{transition:opacity .2s linear,width .2s}",
            ".a{height:12px}.b{display:flex}.c{font-size:20px;line-height:24px} @media (max-width:250px){.a{height:30px}} .t{transition:height .1s}",
            ".q{width:25vw;height:10px}.a{min-height:5vh}@keyframes s{to{transform:rotate(1turn)}} .anim{animation:s 1s infinite}",
            ".a{margin:var(--m,3px)} :root{--m:4px} body{padding-bottom:var(--tg-content-safe-area-inset-bottom,9px)} .b{color:var(--tg-theme-text-color,black)}",
            "",
        ],
    );
    ops.push(sheet(text));
    for i in 0..8 {
        let (e, t) = (10 + i, 20 + i);
        let class = pick(r, &["a", "b", "a b", "c", "anim", "t", "q", ""]);
        let style = pick(
            r,
            &[
                "height:10px",
                "width:50px;height:20px",
                "",
                "position:relative;left:3px",
                "transform:scale(2)",
                "overflow:hidden;height:15px",
                "padding:4px;border:2px solid red",
            ],
        );
        ops.push(format!(
            "{{\"op\":\"el\",\"id\":{e},\"tag\":\"div\",\"attrs\":[[\"class\",{}],[\"style\",{}]]}},{{\"op\":\"ins\",\"parent\":4,\"id\":{e},\"before\":null}},{{\"op\":\"text\",\"id\":{t},\"data\":{}}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{t},\"before\":null}}",
            quote(class),
            quote(style),
            quote(pick(r, &["item", "a longer line of text that wraps", "x", "ünï"]))
        ));
    }
    ops.push(r#"{"op":"el","id":30,"tag":"span","attrs":[["style","font-size:10px"]]},{"op":"ins","parent":12,"id":30,"before":null},{"op":"text","id":31,"data":"words words words words words words words words words words words words words"},{"op":"ins","parent":30,"id":31,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":32,"tag":"textarea","attrs":[["style","height:20px;padding:0;border:0;min-height:0;font-size:10px;line-height:10px"]]},{"op":"ins","parent":4,"id":32,"before":null},{"op":"prop","id":32,"name":"value","value":"\na\nb\nc"}"#.to_string());
    ops.push(r#"{"op":"el","id":33,"tag":"canvas","attrs":[["width","40"],["height","20"]]},{"op":"ins","parent":4,"id":33,"before":null},{"op":"canvas","id":33,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10]]}"#.to_string());
    ops.push(r#"{"op":"el","id":34,"tag":"svg","ns":"svg","attrs":[["width","100"],["height","50"],["viewBox","0 0 200 100"]]},{"op":"ins","parent":4,"id":34,"before":null},{"op":"el","id":35,"tag":"rect","ns":"svg","attrs":[["x","10"],["y","10"],["width","30"],["height","20"],["fill","red"]]},{"op":"ins","parent":34,"id":35,"before":null},{"op":"el","id":36,"tag":"path","ns":"svg","attrs":[["d","M0 0 L50 0 L50 50 Z"],["stroke","blue"]]},{"op":"ins","parent":34,"id":36,"before":null},{"op":"el","id":37,"tag":"text","ns":"svg","attrs":[["x","5"],["y","40"]]},{"op":"ins","parent":34,"id":37,"before":null},{"op":"text","id":38,"data":"svg text"},{"op":"ins","parent":37,"id":38,"before":null}"#.to_string());
    let src = pick(
        r,
        &["data:image/png;base64,iVBORw0KGgo=", "data:image/png;base64,async", "data:image/png;base64,fail", "http://example.com/x.png"],
    );
    ops.push(format!(
        "{{\"op\":\"el\",\"id\":39,\"tag\":\"img\",\"attrs\":[[\"src\",{}]]}},{{\"op\":\"ins\",\"parent\":4,\"id\":39,\"before\":null}}",
        quote(src)
    ));
    ops.push(r#"{"op":"el","id":40,"tag":"div","attrs":[["style","width:50px;height:10px;transform:rotate(30deg)"]]},{"op":"ins","parent":4,"id":40,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":41,"tag":"div","attrs":[["style","position:relative;margin-left:20px;padding:5px;border:1px solid red"]]},{"op":"ins","parent":4,"id":41,"before":null},{"op":"el","id":42,"tag":"div","attrs":[["style","margin-left:7px;width:50px;height:10px"]]},{"op":"ins","parent":41,"id":42,"before":null}"#.to_string());
    ops.push(r#"{"op":"el","id":43,"tag":"details","attrs":[]},{"op":"ins","parent":4,"id":43,"before":null},{"op":"el","id":44,"tag":"summary","attrs":[]},{"op":"ins","parent":43,"id":44,"before":null},{"op":"text","id":45,"data":"more"},{"op":"ins","parent":44,"id":45,"before":null}"#.to_string());
    (ops, 60)
}

/// A batch of what a busy widget sends: style properties, classes, texts, attributes, canvas drawing, SVG attributes,
/// and now and then what has no hole (a new element, a removal, markup in text, another attribute).
pub fn patch_batch(r: &mut Lcg) -> String {
    let mut ops = Vec::new();
    for _ in 0..1 + r.below(4) {
        let e = 10 + r.below(8);
        let t = 20 + r.below(8);
        let op = match r.below(26) {
            0..=4 => format!(
                "{{\"op\":\"style\",\"id\":{e},\"name\":{},\"value\":{}}}",
                quote(pick(
                    r,
                    &["transform", "background-color", "opacity", "color", "height", "width", "padding-top", "--m", "font-size", "display"]
                )),
                quote(pick(r, &["translateX(4px)", "rgb(3,4,5)", "0.5", "red", "17px", "33px", "2px", "8px", "none", "block", "12px", ""]))
            ),
            5..=7 => format!(
                "{{\"op\":\"attr\",\"id\":{e},\"name\":\"class\",\"value\":{}}}",
                quote(pick(r, &["a", "a b", "b", "c", "anim", "t", "q", ""]))
            ),
            8..=11 => format!(
                "{{\"op\":\"data\",\"id\":{t},\"data\":{}}}",
                quote(pick(r, &["text 1", "text 22", "a longer text that wraps onto a second line here", "&<>", "", "ünï 😀"]))
            ),
            12 => format!(
                "{{\"op\":\"attr\",\"id\":{e},\"name\":\"style\",\"value\":{}}}",
                quote(pick(r, &["height:20px", "", "color:blue;width:10px"]))
            ),
            13 => format!("{{\"op\":\"attr\",\"id\":{e},\"name\":\"title\",\"value\":\"t\"}}"),
            14 => format!(
                "{{\"op\":\"attr\",\"id\":{},\"name\":{},\"value\":{}}}",
                *pick(r, &[35, 36, 34]),
                quote(pick(r, &["fill", "x", "width", "d", "stroke", "viewBox"])),
                quote(pick(r, &["green", "20", "M0 0 L10 10", "0 0 100 100", "5"]))
            ),
            15 => format!(
                "{{\"op\":\"canvas\",\"id\":33,\"cmds\":[[\"s\",\"width\",{}],[\"c\",\"fillRect\",0,0,5,5]]}}",
                pick(r, &[40, 41, 60, 300])
            ),
            16 => r#"{"op":"canvas","id":33,"cmds":[["s","fillStyle","red"],["c","fillRect",1,1,3,3]]}"#.to_string(),
            17 => format!(
                "{{\"op\":\"attr\",\"id\":39,\"name\":\"src\",\"value\":{}}}",
                quote(pick(r, &["data:image/png;base64,QUJD", "data:image/png;base64,asyncB", "data:x", "data:image/png;base64,fail2"]))
            ),
            18 => format!(
                "{{\"op\":\"prop\",\"id\":32,\"name\":\"value\",\"value\":{}}}",
                quote(pick(r, &["x", "\nlead", "", "a\nb\nc\nd\ne\nf"]))
            ),
            19 => {
                let fresh = 100 + r.below(50);
                format!(
                    "{{\"op\":\"el\",\"id\":{fresh},\"tag\":\"div\",\"attrs\":[[\"style\",\"height:7px\"]]}},{{\"op\":\"ins\",\"parent\":{e},\"id\":{fresh},\"before\":null}}"
                )
            }
            20 => format!("{{\"op\":\"rm\",\"id\":{}}}", 100 + r.below(50)),
            21 => format!(
                "{{\"op\":\"style\",\"id\":{e},\"name\":\"transform\",\"value\":\"translateX({}px)\",\"important\":{}}}",
                r.below(30),
                r.chance(4)
            ),
            22 => "{\"op\":\"attr\",\"id\":43,\"name\":\"open\",\"value\":\"\"}".to_string(),
            23 => format!(
                "{{\"op\":\"call\",\"id\":{e},\"method\":{},\"args\":[]}}",
                quote(pick(r, &["observeResize", "showModal", "close"]))
            ),
            24 => format!(
                "{{\"op\":\"style\",\"id\":{e},\"name\":\"animation\",\"value\":{}}}",
                quote(pick(r, &["f .2s", "s 1s infinite", "none"]))
            ),
            _ => "{\"op\":\"style\",\"id\":2,\"name\":\"--tg-theme-bg-color\",\"value\":\"#123456\"}".to_string(),
        };
        ops.push(op);
    }
    format!("[{}]", ops.join(","))
}

/// A query of any kind, its arguments valid, malformed or past a limit.
pub fn query(r: &mut Lcg, max_id: u32) -> String {
    let id = if r.chance(12) { *pick(r, &[0i64, 1, 9999, -3, 21]) } else { (2 + r.below(max_id as usize - 1)) as i64 };
    let id_field = match r.below(20) {
        0 => String::new(),
        1 => ",\"id\":\"10\"".to_string(),
        2 => ",\"id\":1e20".to_string(),
        3 => ",\"id\":10.7".to_string(),
        _ => format!(",\"id\":{id}"),
    };
    let q = match r.below(24) {
        0 | 1 => format!("{{\"q\":\"rect\"{id_field}}}"),
        2 | 3 => format!("{{\"q\":\"rects\"{id_field}}}"),
        4 | 5 => format!("{{\"q\":\"box\"{id_field}}}"),
        6 => format!("{{\"q\":\"cs\"{id_field}}}"),
        7 => format!("{{\"q\":\"cs\"{id_field},\"pseudo\":{}}}", quote(pick(r, &["::before", ":after", "::marker", ""]))),
        8 => format!(
            "{{\"q\":\"cs\"{id_field},\"prop\":{}}}",
            quote(pick(r, &["--m", "--tg-content-safe-area-inset-bottom", "--tg-theme-bg-color", "--none"]))
        ),
        9 => format!("{{\"q\":\"elementFromPoint\",\"x\":{},\"y\":{}}}", r.below(320) as f64 - 5.0, r.below(200) as f64 - 5.0),
        10 => format!(
            "{{\"q\":\"measureText\",\"font\":{},\"text\":{}}}",
            pick(r, &["\"12px serif\"", "\"bold 14px sans-serif\"", "5", "null", "\"\"", "\"italic 300 9.5px 'A B'\""]),
            quote(pick(r, &["hi", "", "a longer text", "ünï"]))
        ),
        11 => "{\"q\":\"validity\"}".to_string(),
        12 => format!(
            "{{\"q\":\"matchMedia\",\"query\":{}}}",
            quote(pick(
                r,
                &[
                    "(prefers-color-scheme: dark)",
                    "(max-width: 250px)",
                    "(min-width:300px)",
                    "(hover: none)",
                    "(pointer: coarse)",
                    "(orientation: portrait)",
                    "",
                    "(MAX-WIDTH: 1e3px)",
                    "(min-width: abc)"
                ]
            ))
        ),
        13 => "{\"q\":\"viewport\"}".to_string(),
        14 => format!("{{\"q\":\"anims\"{id_field},\"subtree\":{}}}", r.chance(2)),
        15 => format!(
            "{{\"q\":\"imageData\",\"id\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
            pick(r, &[33, 33, 10, 999]),
            r.below(10),
            r.below(10),
            pick(r, &["4", "0", "-3", "9000", "4096", "1e10", "\"x\""]),
            pick(r, &["3", "0", "8192", "4097", "2"])
        ),
        16 => format!(
            "{{\"q\":\"canvasCall\",\"id\":{},\"method\":{},\"args\":[{}]}}",
            pick(r, &[33, 33, 10]),
            quote(pick(
                r,
                &[
                    "getTransform",
                    "getLineDash",
                    "isPointInPath",
                    "isPointInStroke",
                    "toDataURL",
                    "measureText",
                    "getContextAttributes",
                    "nope"
                ]
            )),
            pick(r, &["", "1,2", "\"x\""])
        ),
        17 => format!(
            "{{\"q\":\"svg\",\"id\":{},\"method\":{},\"args\":[{}]}}",
            pick(r, &[35, 36, 37, 34, 10]),
            quote(pick(r, &["getBBox", "getTotalLength", "getPointAtLength", "getComputedTextLength", "nope"])),
            pick(r, &["10", "-5", "1e40", "\"x\"", ""])
        ),
        18 => "{\"q\":\"nope\"}".to_string(),
        19 => "5".to_string(),
        20 => "{\"q\":\"rect\",\"id\":21}".to_string(),
        21 => format!("{{\"q\":\"box\",\"id\":{}}}", pick(r, &[32, 41, 42, 30, 2, 4])),
        22 => format!("{{\"q\":\"rects\",\"id\":{}}}", pick(r, &[30, 31, 12, 40])),
        _ => format!("{{\"q\":\"cs\",\"id\":{}}}", pick(r, &[10, 4, 2, 30, 35])),
    };
    match r.below(15) {
        0 => "{\"type\":\"query\",\"qid\":7}".to_string(),
        1 => format!("{{\"type\":\"query\",\"query\":{q}}}"),
        _ => format!("{{\"type\":\"query\",\"qid\":{},\"query\":{q}}}", r.below(100)),
    }
}

/// A host message of the theme, the safe area, the policy, listeners or animations.
pub fn host_message(r: &mut Lcg) -> String {
    match r.below(9) {
        0 | 1 => format!(
            "{{\"type\":\"theme\",\"theme\":{{\"colorScheme\":{},\"params\":{{{}}}}}}}",
            quote(pick(r, &["light", "dark", "nope"])),
            pick(r, &["", "\"bg_color\":\"#101010\"", "\"text_color\":\"#fafafa\",\"button_color\":\"#2481cc\"", "\"bg_color\":5"])
        ),
        2 | 3 => format!(
            "{{\"type\":\"safeArea\",\"inset\":{{\"top\":{},\"right\":0,\"bottom\":{},\"left\":0,\"cornerLeft\":{},\"cornerRight\":{}}}}}",
            pick(r, &["0", "12", "-4"]),
            pick(r, &["0", "24.567", "1e9", "\"x\""]),
            pick(r, &["0", "16", "300"]),
            pick(r, &["0", "300", "20"])
        ),
        4 => format!("{{\"type\":\"policy\",\"policy\":{}}}", quote(pick(r, &["compat", "strict", "default"]))),
        5 => "{\"type\":\"listen\",\"types\":[\"animationiteration\",\"transitionend\",\"animationstart\",\"click\"]}".to_string(),
        6 => {
            let aid = 1 + r.below(3);
            if r.chance(5) {
                format!("{{\"type\":\"anim\",\"now\":{},\"anims\":[{{\"aid\":{aid},\"remove\":true}}]}}", r.below(1000))
            } else {
                format!(
                    "{{\"type\":\"anim\",\"now\":{},\"anims\":[{{\"aid\":{aid},\"target\":{},\"keyframes\":[{{\"offset\":0,\"values\":{{{}}}}},{{\"offset\":1,\"values\":{{{}}}}}],\"timing\":{{\"duration\":{},\"fill\":{},\"iterations\":{}}},\"startTime\":{}{}}}]}}",
                    r.below(1000),
                    10 + r.below(8),
                    pick(r, &["\"opacity\":\"0\"", "\"transform\":\"translateX(0px)\"", "\"width\":\"10px\""]),
                    pick(r, &["\"opacity\":\"1\"", "\"transform\":\"translateX(40px)\"", "\"width\":\"90px\""]),
                    pick(r, &[100, 400, 1000]),
                    quote(pick(r, &["both", "none", "forwards"])),
                    pick(r, &["1", "2", "null"]),
                    pick(r, &["-500", "0", "100"]),
                    pick(r, &["", ",\"playbackRate\":2", ",\"composite\":\"add\""])
                )
            }
        }
        7 => {
            format!("{{\"type\":\"frozen\",\"reason\":{},\"fatal\":{}}}", quote(pick(r, &["Paused", "Stopped <b>&</b>", ""])), r.chance(2))
        }
        _ => "{\"type\":\"unknown\"}".to_string(),
    }
}

/// A script over the page: batches patched in place or not, queries, theme and safe area changes, sizes, frames,
/// resize tracking, trims and inspections, the clock moving on by a frame to a second.
pub fn patch_script(r: &mut Lcg, steps: usize) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.width = *pick(r, &[300.0, 300.0, 240.0, 412.5]);
    s.dark = r.chance(4);
    if r.chance(2) {
        s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"compat\"}".to_vec()));
    }
    if r.chance(3) {
        s.after(0.0, Step::ResizeTracking { enabled: true, prefers_bottom: r.chance(2) });
    }
    if r.chance(6) {
        s.after(0.0, Step::Patching(false));
    }
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..steps {
        let dt = *pick(r, &[0.0, 8.0, 16.0, 16.0, 40.0, 120.0, 300.0, 1100.0]);
        match r.below(40) {
            0..=13 => {
                let batch = patch_batch(r);
                s.after(dt, Step::Ops(batch.into_bytes()));
            }
            14..=21 => {
                let q = query(r, max_id);
                s.after(dt, Step::Message(q.into_bytes()));
            }
            22..=25 => {
                s.after(dt, Step::Tick);
            }
            26 => {
                s.after(dt, Step::LastHtml);
            }
            27 | 28 => {
                let m = host_message(r);
                s.after(dt, Step::Message(m.into_bytes()));
            }
            29 => {
                s.after(dt, Step::Width(*pick(r, &[200.0, 320.0, 150.5, 300.0, f64::NAN, -5.0, 2e6])));
            }
            30 => {
                s.after(dt, Step::MaxHeight(*pick(r, &[100.0, 900.0, 30.0, 600.0, f64::INFINITY])));
            }
            31 => {
                s.after(dt, Step::Dark(r.chance(2)));
            }
            32 => {
                s.after(dt, Step::Inspect);
            }
            33 => {
                s.after(dt, Step::Trim);
            }
            34 => {
                s.after(dt, Step::ResizeTracking { enabled: !r.chance(4), prefers_bottom: r.chance(2) });
            }
            35 => {
                s.after(
                    dt,
                    Step::Touch {
                        action: 0,
                        x: r.below(300) as f64,
                        y: r.below(200) as f64,
                        pointer: 0,
                        primary: true,
                        mouse: true,
                        modifiers: 0,
                        button: 0,
                    },
                );
                s.after(
                    30.0,
                    Step::Touch {
                        action: 2,
                        x: r.below(300) as f64,
                        y: r.below(200) as f64,
                        pointer: 0,
                        primary: true,
                        mouse: true,
                        modifiers: 0,
                        button: 0,
                    },
                );
            }
            36 => {
                s.after(dt, Step::ImageReady { id: 1 + r.below(6) as i32, w: *pick(r, &[0.0, 12.0, 30.5]), h: *pick(r, &[0.0, 7.0]) });
            }
            37 => {
                s.after(dt, Step::Hover { action: 0, x: r.below(300) as f64, y: r.below(200) as f64, modifiers: 0 });
            }
            38 => {
                s.after(dt, Step::Patching(!r.chance(3)));
            }
            _ => {
                s.after(dt, Step::Charge(*pick(r, &[0.01, 0.05, 0.2])));
            }
        }
    }
    s.after(16.0, Step::Inspect);
    s
}

/// The document of a nesting `depth` deep of `display` boxes, each laid out in its parent's measuring passes: past the
/// layout budget from some depth on.
pub fn nested(display: &str, depth: usize, first_id: i64) -> Vec<String> {
    let mut ops = Vec::new();
    let mut parent = 4;
    for i in 0..depth as i64 {
        let id = first_id + i;
        ops.push(format!(
            "{{\"op\":\"el\",\"id\":{id},\"tag\":\"div\",\"attrs\":[[\"style\",{}]]}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}",
            quote(&format!("display:{display}"))
        ));
        parent = id;
    }
    let text = first_id + depth as i64;
    ops.push(format!(
        "{{\"op\":\"text\",\"id\":{text},\"data\":\"deep\"}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{text},\"before\":null}}"
    ));
    ops
}

/// Documents run past the layout budget: as built, by a patch that turns boxes into flex or grid containers (in place,
/// through the style holes, or through a new document), by a width that lays out again, by an animation's restyle,
/// and the badge after; with queries, frames, ops and input after the stop.
pub fn budget_script(r: &mut Lcg, n: usize) -> Script {
    let display = *pick(r, &["flex", "grid", "table", "inline-flex", "block"]);
    let depth = *pick(r, &[8usize, 14, 18, 22, 28, 40]);
    let mut s = Script::new(200);
    if r.chance(2) {
        s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"compat\"}".to_vec()));
    }
    match n % 4 {
        0 => {
            s.after(0.0, Step::Ops(document(&nested(display, depth, 100)).into_bytes()));
        }
        1 => {
            // Blocks first, then a batch that makes them flex boxes: the patch path meets the budget.
            s.after(0.0, Step::Ops(document(&nested("block", depth, 100)).into_bytes()));
            let ops: Vec<String> = (0..depth)
                .map(|i| format!("{{\"op\":\"style\",\"id\":{},\"name\":\"display\",\"value\":{}}}", 100 + i, quote(display)))
                .collect();
            s.after(16.0, Step::Ops(format!("[{}]", ops.join(",")).into_bytes()));
        }
        2 => {
            // Blocks under a sheet whose class makes them flex boxes, the class set in one batch.
            let mut ops = nested("block", depth, 100);
            ops.push(sheet(&format!(
                ".x{{display:{display}}} @keyframes w{{from{{width:10px}}to{{width:90px}}}} .y{{animation:w 1s infinite}}"
            )));
            s.after(0.0, Step::Ops(document(&ops).into_bytes()));
            let ops: Vec<String> =
                (0..depth).map(|i| format!("{{\"op\":\"attr\",\"id\":{},\"name\":\"class\",\"value\":\"x y\"}}", 100 + i)).collect();
            s.after(16.0, Step::Ops(format!("[{}]", ops.join(",")).into_bytes()));
            for _ in 0..3 {
                s.after(100.0, Step::Tick);
            }
        }
        _ => {
            // Laid out within the budget at one width, past it at another.
            s.after(0.0, Step::Ops(document(&nested(display, depth.min(16), 100)).into_bytes()));
            s.after(16.0, Step::Width(*pick(r, &[100.0, 320.0, 50.0])));
        }
    }
    for _ in 0..6 {
        match r.below(8) {
            0 => {
                s.after(16.0, Step::Tick);
            }
            1 => {
                s.after(16.0, Step::LastHtml);
            }
            2 => {
                let q = query(r, 140);
                s.after(0.0, Step::Message(q.into_bytes()));
            }
            3 => {
                s.after(16.0, Step::Ops(b"[{\"op\":\"style\",\"id\":100,\"name\":\"color\",\"value\":\"red\"}]".to_vec()));
            }
            4 => {
                s.after(16.0, Step::Touch { action: 0, x: 10.0, y: 10.0, pointer: 0, primary: true, mouse: true, modifiers: 0, button: 0 });
                s.after(16.0, Step::Touch { action: 2, x: 10.0, y: 10.0, pointer: 0, primary: true, mouse: true, modifiers: 0, button: 0 });
            }
            5 => {
                s.after(16.0, Step::Inspect);
            }
            6 => {
                let m = host_message(r);
                s.after(16.0, Step::Message(m.into_bytes()));
            }
            _ => {
                s.after(16.0, Step::Dark(r.chance(2)));
            }
        }
    }
    s.after(16.0, Step::Inspect);
    s
}

/// A widget that keeps inventing names (custom properties the fork interns), one sheet after another, until the name
/// table's bounds stop it; the prefix keeps its names its own in a process that ran others before it.
pub fn names_script(r: &mut Lcg, prefix: &str) -> Script {
    let mut s = Script::new(40);
    s.after(
        0.0,
        Step::Ops(
            document(&[r#"{"op":"el","id":10,"tag":"div","attrs":[]},{"op":"ins","parent":4,"id":10,"before":null}"#.to_string()])
                .into_bytes(),
        ),
    );
    let per = *pick(r, &[3000usize, 6000, 9000]);
    for batch in 0..5 {
        let mut sheet = String::from(".x{");
        for i in 0..per {
            sheet.push_str(&format!("--{prefix}{batch}n{i}:1;"));
        }
        sheet.push('}');
        let id = 20 + batch;
        s.after(
            16.0,
            Step::Ops(
                format!(
                    "[{{\"op\":\"el\",\"id\":{id},\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":{id},\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":{id},\"before\":null}}]",
                    quote(&sheet)
                )
                .into_bytes(),
            ),
        );
        if r.chance(2) {
            s.after(16.0, Step::Tick);
        }
    }
    s.after(16.0, Step::Ops(b"[{\"op\":\"style\",\"id\":10,\"name\":\"color\",\"value\":\"red\"}]".to_vec()));
    s.after(16.0, Step::LastHtml);
    s.after(16.0, Step::Inspect);
    s
}

/// Themes, safe areas, schemes, sizes and the master CSS they make, read back through cs, matchMedia and inspections.
pub fn theme_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.dark = r.chance(2);
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..20 {
        let dt = *pick(r, &[0.0, 16.0, 100.0]);
        match r.below(10) {
            0..=2 => {
                let m = host_message(r);
                s.after(dt, Step::Message(m.into_bytes()));
            }
            3 => {
                s.after(dt, Step::Dark(r.chance(2)));
            }
            4 => {
                s.after(dt, Step::Width(*pick(r, &[200.0, 300.0, 412.5, 0.0, 1e7])));
            }
            5 => {
                s.after(dt, Step::MaxHeight(*pick(r, &[60.0, 600.0, 2000.0, 20000.0, f64::NAN])));
            }
            6 => {
                s.after(dt, Step::Inspect);
            }
            7 => {
                let q = format!(
                    "{{\"q\":\"cs\",\"id\":{},\"prop\":{}}}",
                    pick(r, &[2, 4, 10]),
                    quote(pick(
                        r,
                        &[
                            "--tg-content-safe-area-inset-bottom",
                            "--tg-theme-bg-color",
                            "--tg-theme-text-color",
                            "--tg-content-safe-area-corner-right",
                            "--m"
                        ]
                    ))
                );
                s.query(dt, 1, &q);
            }
            8 => {
                let q = format!("{{\"q\":\"cs\",\"id\":{}}}", pick(r, &[2, 4, 10, 11]));
                s.query(dt, 2, &q);
            }
            _ => {
                let q = query(r, max_id);
                s.after(dt, Step::Message(q.into_bytes()));
            }
        }
    }
    s
}

/// Animations, transitions and Web Animations sampled frame by frame, restyled by the pointer, queried as they run.
pub fn animation_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.after(
        0.0,
        Step::Message(
            b"{\"type\":\"listen\",\"types\":[\"animationiteration\",\"transitionend\",\"animationstart\",\"animationend\"]}".to_vec(),
        ),
    );
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..30 {
        let dt = *pick(r, &[8.0, 16.0, 16.0, 50.0, 200.0]);
        match r.below(12) {
            0..=4 => {
                s.after(dt, Step::Tick);
            }
            5 => {
                let id = 10 + r.below(8);
                let ops = format!(
                    "[{{\"op\":\"attr\",\"id\":{id},\"name\":\"class\",\"value\":{}}}]",
                    quote(pick(r, &["anim", "t", "a t", "", "anim t"]))
                );
                s.after(dt, Step::Ops(ops.into_bytes()));
            }
            6 => {
                let id = 10 + r.below(8);
                let ops = format!(
                    "[{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}]",
                    quote(pick(r, &["opacity", "width", "height", "transform"])),
                    quote(pick(r, &["0.3", "80px", "5px", "rotate(10deg)", ""]))
                );
                s.after(dt, Step::Ops(ops.into_bytes()));
            }
            7 => {
                let m = host_message(r);
                s.after(dt, Step::Message(m.into_bytes()));
            }
            8 => {
                let q = format!(
                    "{{\"q\":{},\"id\":{},\"subtree\":{}}}",
                    quote(pick(r, &["anims", "cs", "rect"])),
                    pick(r, &[4, 10, 11, 12]),
                    r.chance(2)
                );
                s.query(dt, 3, &q);
            }
            9 => {
                s.after(dt, Step::Hover { action: 0, x: r.below(200) as f64, y: r.below(150) as f64, modifiers: 0 });
            }
            10 => {
                s.after(dt, Step::LastHtml);
            }
            _ => {
                s.after(dt, Step::Inspect);
            }
        }
    }
    s
}

/// The resize diffs of a document whose height changes above, below and around a press, with tracking on.
pub fn resize_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.after(0.0, Step::ResizeTracking { enabled: true, prefers_bottom: r.chance(2) });
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..16 {
        let dt = *pick(r, &[16.0, 50.0, 100.0, 400.0, 2000.0]);
        match r.below(6) {
            0 | 1 => {
                let id = 10 + r.below(8);
                let ops = format!("[{{\"op\":\"style\",\"id\":{id},\"name\":\"height\",\"value\":\"{}px\"}}]", r.below(60));
                s.after(dt, Step::Ops(ops.into_bytes()));
            }
            2 => {
                let fresh = 100 + r.below(20);
                let parent = 10 + r.below(8);
                let ops = format!(
                    "[{{\"op\":\"el\",\"id\":{fresh},\"tag\":\"div\",\"attrs\":[[\"style\",\"height:{}px\"]]}},{{\"op\":\"ins\",\"parent\":{parent},\"id\":{fresh},\"before\":null}}]",
                    r.below(40)
                );
                s.after(dt, Step::Ops(ops.into_bytes()));
            }
            3 => {
                let (x, y) = (r.below(200) as f64, r.below(200) as f64);
                s.after(dt, Step::Touch { action: 0, x, y, pointer: 0, primary: true, mouse: r.chance(2), modifiers: 0, button: 0 });
                s.after(40.0, Step::Touch { action: 2, x, y, pointer: 0, primary: true, mouse: false, modifiers: 0, button: 0 });
            }
            4 => {
                s.after(dt, Step::Tick);
            }
            _ => {
                s.after(dt, Step::ResizeTracking { enabled: !r.chance(5), prefers_bottom: r.chance(2) });
            }
        }
    }
    s
}

/// A long flush handed over in parts of 1000 ops, the last shorter, and what reads the document after.
pub fn flush_script(r: &mut Lcg) -> Script {
    let mut s = Script::new(4000);
    s.after(0.0, Step::Ops(document(&[]).into_bytes()));
    let parts = 1 + r.below(3);
    let mut id = 10;
    for p in 0..=parts {
        let n = if p == parts { 1 + r.below(50) } else { 1000 };
        let mut ops = Vec::with_capacity(n);
        while ops.len() + 2 <= n {
            ops.push(format!("{{\"op\":\"el\",\"id\":{id},\"tag\":\"div\",\"attrs\":[[\"style\",\"height:1px\"]]}}"));
            ops.push(format!("{{\"op\":\"ins\",\"parent\":4,\"id\":{id},\"before\":null}}"));
            id += 1;
        }
        while ops.len() < n {
            ops.push("{\"op\":\"style\",\"id\":4,\"name\":\"color\",\"value\":\"red\"}".to_string());
        }
        s.after(if p == 0 { 16.0 } else { 0.0 }, Step::Ops(format!("[{}]", ops.join(",")).into_bytes()));
        if r.chance(3) {
            s.after(0.0, Step::Inspect);
        }
    }
    s.after(16.0, Step::Tick);
    s.after(0.0, Step::Inspect);
    s.query(0.0, 9, "{\"q\":\"box\",\"id\":4}");
    s
}

/// `frozen` messages, paused or fatal, with the ops, queries, frames and input after them.
pub fn frozen_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    for _ in 0..12 {
        let dt = *pick(r, &[0.0, 16.0, 200.0]);
        match r.below(7) {
            0 => {
                let m = format!(
                    "{{\"type\":\"frozen\",\"reason\":{},\"fatal\":{}}}",
                    quote(pick(r, &["Paused", "Stopped <b>&amp;</b>", "", "x"])),
                    r.chance(2)
                );
                s.after(dt, Step::Message(m.into_bytes()));
            }
            1 => {
                let b = patch_batch(r);
                s.after(dt, Step::Ops(b.into_bytes()));
            }
            2 => {
                let q = query(r, max_id);
                s.after(dt, Step::Message(q.into_bytes()));
            }
            3 => {
                s.after(dt, Step::Tick);
            }
            4 => {
                s.after(dt, Step::Touch { action: 0, x: 20.0, y: 20.0, pointer: 0, primary: true, mouse: true, modifiers: 0, button: 0 });
            }
            5 => {
                s.after(dt, Step::LastHtml);
            }
            _ => {
                s.after(dt, Step::Inspect);
            }
        }
    }
    s
}

/// Scripted layers: with compositing on, an element a script moves batch after batch through its layer properties
/// becomes a layer, and the batches that only move it go to the layer without a build until a query or another op
/// asks for the document.
pub fn scripted_script(r: &mut Lcg) -> Script {
    let (ops, max_id) = page(r);
    let mut s = Script::new(max_id);
    s.after(0.0, Step::Compositing(true));
    if r.chance(2) {
        s.after(0.0, Step::Message(b"{\"type\":\"policy\",\"policy\":\"compat\"}".to_vec()));
    }
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    let movers = [10 + r.below(8), 10 + r.below(8)];
    for k in 0..24 {
        let dt = *pick(r, &[16.0, 16.0, 33.0, 100.0]);
        match if k < 12 { 0 } else { r.below(10) } {
            0..=5 => {
                let ops: Vec<String> = movers
                    .iter()
                    .map(|id| {
                        format!(
                            "{{\"op\":\"style\",\"id\":{id},\"name\":{},\"value\":{}}}",
                            quote(if k % 4 == 3 { "opacity" } else { "transform" }),
                            quote(&if k % 4 == 3 { format!("0.{}", 1 + k % 9) } else { format!("translateX({}px)", k % 6) })
                        )
                    })
                    .collect();
                s.after(dt, Step::Ops(format!("[{}]", ops.join(",")).into_bytes()));
            }
            6 => {
                s.after(dt, Step::Tick);
            }
            7 => {
                let q = query(r, max_id);
                s.after(dt, Step::Message(q.into_bytes()));
            }
            8 => {
                let b = patch_batch(r);
                s.after(dt, Step::Ops(b.into_bytes()));
            }
            _ => {
                s.after(dt, Step::Inspect);
            }
        }
    }
    s
}

/// A script whose steps the tests read by name.
#[derive(Clone, Debug)]
pub struct Marked {
    pub script: Script,
    marks: Vec<(String, usize)>,
}

impl Marked {
    pub fn new(script: Script) -> Marked {
        Marked { script, marks: Vec::new() }
    }

    /// Names the step added last.
    pub fn mark(&mut self, name: impl Into<String>) -> &mut Marked {
        let at = self.script.steps.len().saturating_sub(1);
        self.marks.push((name.into(), at));
        self
    }

    /// `step` `ms` after the last one, named `name`.
    pub fn step(&mut self, name: impl Into<String>, ms: f64, step: Step) -> &mut Marked {
        self.script.after(ms, step);
        self.mark(name)
    }

    /// A host message `ms` after the last step, named `name`.
    pub fn message(&mut self, name: impl Into<String>, ms: f64, msg: &str) -> &mut Marked {
        self.script.message(ms, msg);
        self.mark(name)
    }

    /// A query at once, named `name`.
    pub fn ask(&mut self, name: impl Into<String>, query: &str) -> &mut Marked {
        self.script.query(0.0, 1, query);
        self.mark(name)
    }

    /// The step named `name`.
    pub fn at(&self, name: &str) -> usize {
        match self.marks.iter().find(|(n, _)| n == name) {
            Some((_, at)) => *at,
            None => panic!("no step named {name:?}"),
        }
    }
}

/// The place's variables on the root, in the order renderer.cpp's `apply_place` sets them: the device's edges, the
/// client's chrome, a page's viewport.
pub const PLACE_VARIABLES: [&str; 11] = [
    "--tg-safe-area-inset-top",
    "--tg-safe-area-inset-right",
    "--tg-safe-area-inset-bottom",
    "--tg-safe-area-inset-left",
    "--tg-content-safe-area-inset-top",
    "--tg-content-safe-area-inset-right",
    "--tg-content-safe-area-inset-bottom",
    "--tg-content-safe-area-inset-left",
    "--tg-content-safe-area-corner-left",
    "--tg-content-safe-area-corner-right",
    "--tg-viewport-height",
];

/// auto/46-profile-safe-area.html's style sheet as the validator hands it over: each `env(safe-area-inset-*)` the sum
/// of both layers (css-sanitizer.ts `resolveEnv`, a unitless 0 fallback as 0px).
pub const AUTO_46_SHEET: &str = ".p{position:relative;height:6px;margin:0 0 2px;background:#eef}.p>div{height:6px;background:#46c}#e1{width:calc(var(--tg-safe-area-inset-top,0px) + var(--tg-content-safe-area-inset-top))}#e2{width:calc(var(--tg-safe-area-inset-bottom,0px) + var(--tg-content-safe-area-inset-bottom,77px))}#e3{width:calc(var(--tg-safe-area-inset-left,0px) + var(--tg-content-safe-area-inset-left,5px))}#e4{width:calc(10px + calc(var(--tg-safe-area-inset-top,0px) + var(--tg-content-safe-area-inset-top,0px)))}#e5{width:max(4px,calc(var(--tg-safe-area-inset-right,0px) + var(--tg-content-safe-area-inset-right)))}#e6{width:var(--tg-safe-area-inset-top)}#e7{width:var(--tg-safe-area-inset-bottom)}#e8{width:var(--tg-content-safe-area-inset-top)}";

/// Neither layer over the widget.
pub const NO_INSET: &str = "{\"top\":0,\"right\":0,\"bottom\":0,\"left\":0,\"cornerLeft\":0,\"cornerRight\":0}";

const VIEWPORT: &str = "{\"q\":\"viewport\"}";

/// The host's `surface` message: the place and the viewport's JSON.
pub fn surface(place: &str, viewport: &str) -> String {
    format!("{{\"type\":\"surface\",\"surface\":{},\"viewportHeight\":{viewport}}}", quote(place))
}

pub fn safe_area(inset: &str) -> String {
    format!("{{\"type\":\"safeArea\",\"inset\":{inset}}}")
}

pub fn device_safe_area(inset: &str) -> String {
    format!("{{\"type\":\"deviceSafeArea\",\"inset\":{inset}}}")
}

/// What WidgetHost sends before any ops (docs/sdk-v1.md §3.4): the theme, the place, the client's chrome, the device's
/// edges and the policy; `content` and `device` are the insets' JSON.
pub fn profile_start(s: &mut Script, surface: &str, content: &str, device: &str) {
    s.message(
        0.0,
        "{\"type\":\"theme\",\"theme\":{\"colorScheme\":\"light\",\"params\":{\"bg_color\":\"#ffffff\",\"text_color\":\"#000000\"}}}",
    );
    s.message(0.0, surface);
    s.message(0.0, &safe_area(content));
    s.message(0.0, &device_safe_area(device));
    s.message(0.0, "{\"type\":\"policy\",\"policy\":\"strict\"}");
}

/// auto/46's probes: its sheet and the `.p` boxes 11–18 in the body, each holding `#e1`–`#e8` (21–28).
fn auto_46_probes() -> Vec<String> {
    let mut ops = vec![sheet(AUTO_46_SHEET)];
    for e in 1..=8 {
        let (p, d) = (10 + e, 20 + e);
        ops.push(el(p, "div", 4, &[("class", "p")]));
        ops.push(el(d, "div", p, &[("id", &format!("e{e}"))]));
    }
    ops
}

/// The root's place variables (`cs` of the adopted html), as steps named `state` and the variable.
fn read_variables(m: &mut Marked, state: &str) {
    for name in PLACE_VARIABLES {
        m.ask(format!("{state} {name}"), &format!("{{\"q\":\"cs\",\"id\":2,\"prop\":{}}}", quote(name)));
    }
}

/// What auto/46 reads, as steps named `state` and what they read: the root's place variables, the unstyled body's
/// computed style ("body"), the eight probes' rects ("e1"–"e8") and the viewport.
fn read_probes(m: &mut Marked, state: &str) {
    read_variables(m, state);
    m.ask(format!("{state} body"), "{\"q\":\"cs\",\"id\":4}");
    for e in 1..=8 {
        m.ask(format!("{state} e{e}"), &format!("{{\"q\":\"rect\",\"id\":{}}}", 20 + e));
    }
    m.ask(format!("{state} viewport"), VIEWPORT);
}

/// auto/46-profile-safe-area.html on `REFERENCE_PROFILE` (widget-sandbox's shared/reference.ts: 390 × 844, the
/// device's edges 47 / 34, the client's chrome 56 / 64): the start WidgetHost sends, its root inspected ("start
/// inspect"), the probes loaded ("load") and read ("reference"); distinct right and left edges on both layers, with
/// corners the page drops ("sides"); a host theme ("theme"); the widget's own style ops on the root's place
/// variables ("overwritten").
pub fn reference_profile() -> Marked {
    let mut s = Script::new(30);
    s.width = 390.0;
    profile_start(
        &mut s,
        &surface("profile", "844"),
        "{\"top\":56,\"right\":0,\"bottom\":64,\"left\":0,\"cornerLeft\":0,\"cornerRight\":0}",
        "{\"top\":47,\"right\":0,\"bottom\":34,\"left\":0}",
    );
    let mut m = Marked::new(s);
    m.step("start inspect", 0.0, Step::Inspect);
    m.step("load", 16.0, Step::Ops(document(&auto_46_probes()).into_bytes()));
    read_probes(&mut m, "reference");
    m.message("sides content", 16.0, &safe_area("{\"top\":56,\"right\":7,\"bottom\":64,\"left\":5,\"cornerLeft\":12,\"cornerRight\":30}"));
    m.message("sides device", 0.0, &device_safe_area("{\"top\":47,\"right\":13,\"bottom\":34,\"left\":11}"));
    read_probes(&mut m, "sides");
    m.message(
        "theme message",
        16.0,
        "{\"type\":\"theme\",\"theme\":{\"colorScheme\":\"dark\",\"params\":{\"bg_color\":\"#101010\",\"text_color\":\"#fafafa\"}}}",
    );
    read_probes(&mut m, "theme");
    let own: Vec<String> =
        ["--tg-safe-area-inset-top", "--tg-safe-area-inset-right", "--tg-content-safe-area-inset-top", "--tg-viewport-height"]
            .iter()
            .map(|name| style_op(2, name, "1px"))
            .collect();
    m.step("overwrite", 16.0, Step::Ops(batch(&own)));
    read_probes(&mut m, "overwritten");
    m
}

/// core_tests.cpp's "the bot's profile page": the ops first, then the place at a fractional viewport ("surface"), the
/// client's chrome with a side past 64 and a corner, the device's edges with a negative one; the root's variables,
/// `#hero` sized by the viewport's ("hero") and the body read back; the widget's style ops on the root ("overwrite");
/// `#hero` 9000 px tall ("tall") and the frame after it.
pub fn profile_page() -> Marked {
    let mut s = Script::new(12);
    s.width = 390.0;
    let ops = vec![sheet("#hero{height:var(--tg-viewport-height,10px)}"), el(10, "div", 4, &[("id", "hero")])];
    s.after(0.0, Step::Ops(document(&ops).into_bytes()));
    let mut m = Marked::new(s);
    m.message("surface", 16.0, &surface("profile", "844.567"));
    m.message("content", 0.0, &safe_area("{\"top\":56,\"right\":0,\"bottom\":120,\"left\":0,\"cornerLeft\":0,\"cornerRight\":30}"));
    m.message("device", 0.0, &device_safe_area("{\"top\":47,\"right\":-3,\"bottom\":34,\"left\":0}"));
    read_variables(&mut m, "page");
    m.ask("hero", "{\"q\":\"cs\",\"id\":10}");
    m.ask("body", "{\"q\":\"cs\",\"id\":4}");
    let own = [style_op(2, "--tg-safe-area-inset-top", "1px"), style_op(2, "--tg-viewport-height", "1px")];
    m.step("overwrite", 16.0, Step::Ops(batch(&own)));
    read_variables(&mut m, "overwritten");
    m.step("tall", 16.0, Step::Ops(batch(&[style_op(10, "height", "9000px")])));
    m.step("tall tick", 16.0, Step::Tick);
    m.ask("tall viewport", VIEWPORT);
    m.step("tall inspect", 0.0, Step::Inspect);
    m
}

/// A page at DPR 1 without margins or insets, its viewport 844: a short document with a box half the viewport tall
/// ("short"), one 2000 px tall ("midrange"), one 9000 px tall ("clipped", painted); the host's own bounds below the
/// viewport ("lowered") and between it and the ceiling ("between").
pub fn profile_heights() -> Marked {
    let mut s = Script::new(12);
    s.width = 390.0;
    profile_start(&mut s, &surface("profile", "844"), NO_INSET, NO_INSET);
    let mut m = Marked::new(s);
    let ops = vec![
        style_op(4, "margin", "0"),
        el(10, "div", 4, &[("id", "v"), ("style", "height:50vh")]),
        el(11, "div", 4, &[("id", "c"), ("style", "height:100px")]),
    ];
    m.step("short", 16.0, Step::Ops(document(&ops).into_bytes()));
    m.ask("short v", "{\"q\":\"rect\",\"id\":10}");
    m.ask("short viewport", VIEWPORT);
    m.step("short inspect", 0.0, Step::Inspect);
    m.step("midrange", 16.0, Step::Ops(batch(&[style_op(10, "display", "none"), style_op(11, "height", "2000px")])));
    m.ask("midrange viewport", VIEWPORT);
    m.step("midrange inspect", 0.0, Step::Inspect);
    m.step("clipped", 16.0, Step::Ops(batch(&[style_op(11, "height", "9000px")])));
    m.step("clipped tick", 16.0, Step::Tick);
    m.ask("clipped viewport", VIEWPORT);
    m.step("clipped inspect", 0.0, Step::Inspect);
    m.step("clipped paint", 0.0, Step::Paint);
    m.step("lowered", 16.0, Step::MaxHeight(100.0));
    m.step("between", 16.0, Step::MaxHeight(3000.0));
    m
}

/// A page whose viewport is none, past the ceiling ("none": no `--tg-viewport-height`, the widget's fallback, the
/// least height 50); a fractional one ("fraction", the least height to the next pixel), the ceiling itself
/// ("ceiling"), a string ("string") and a place of another name ("named"), each read back: the variable ("var"),
/// `#f` the variable sizes ("f") and the viewport.
pub fn profile_without_viewport() -> Marked {
    let mut s = Script::new(12);
    profile_start(&mut s, &surface("profile", "4000.5"), NO_INSET, NO_INSET);
    let mut m = Marked::new(s);
    let ops = vec![style_op(4, "margin", "0"), sheet("#f{height:var(--tg-viewport-height,30px)}"), el(10, "div", 4, &[("id", "f")])];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    for (state, viewport, place) in [
        ("none", "", ""),
        ("fraction", "844.567", "profile"),
        ("ceiling", "4000", "profile"),
        ("string", "\"844\"", "profile"),
        ("named", "844", "Profile"),
    ] {
        if !viewport.is_empty() {
            m.message(state, 16.0, &surface(place, viewport));
        }
        m.ask(format!("{state} var"), "{\"q\":\"cs\",\"id\":2,\"prop\":\"--tg-viewport-height\"}");
        m.ask(format!("{state} f"), "{\"q\":\"rect\",\"id\":10}");
        m.ask(format!("{state} viewport"), VIEWPORT);
    }
    m
}

/// A settled page, its document 1000 px tall and its viewport 600 ("load"), then one change after another, each
/// with the frame after it ("… tick"): the device's top edge ("device"), the client's bottom band ("content"), a
/// viewport taller than the padded document ("viewport"), each once more as it is ("… again"); then a left edge
/// that moves the document's box but no reported number ("sides"), its rect read before and after.
pub fn profile_relayout() -> Marked {
    let mut s = Script::new(12);
    s.width = 390.0;
    profile_start(&mut s, &surface("profile", "600"), NO_INSET, NO_INSET);
    let mut m = Marked::new(s);
    let ops = vec![style_op(4, "margin", "0"), el(10, "div", 4, &[("id", "c"), ("style", "height:1000px")])];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    m.step("load tick", 16.0, Step::Tick);
    let changes = [
        ("device", device_safe_area("{\"top\":20,\"right\":0,\"bottom\":0,\"left\":0}")),
        ("content", safe_area("{\"top\":0,\"right\":0,\"bottom\":30,\"left\":0,\"cornerLeft\":0,\"cornerRight\":0}")),
        ("viewport", surface("profile", "1500")),
    ];
    for (name, msg) in &changes {
        m.message(*name, 16.0, msg);
        m.step(format!("{name} tick"), 16.0, Step::Tick);
        m.message(format!("{name} again"), 16.0, msg);
        m.step(format!("{name} again tick"), 16.0, Step::Tick);
    }
    m.ask("before sides", "{\"q\":\"rect\",\"id\":10}");
    m.message("sides", 16.0, &device_safe_area("{\"top\":20,\"right\":0,\"bottom\":0,\"left\":10}"));
    m.step("sides tick", 16.0, Step::Tick);
    m.ask("after sides", "{\"q\":\"rect\",\"id\":10}");
    m
}

/// The bubble's root variables, body, `#f` and viewport, as steps named `state` and what they read.
fn read_bubble(m: &mut Marked, state: &str) {
    read_variables(m, state);
    m.ask(format!("{state} body"), "{\"q\":\"cs\",\"id\":4}");
    m.ask(format!("{state} f"), "{\"q\":\"rect\",\"id\":10}");
    m.ask(format!("{state} viewport"), VIEWPORT);
}

/// A bubble the host gives the device's edges and a viewport, a bottom band and corners of the client's: its root
/// inspected ("start inspect"), `#f` sized by the viewport variable's fallback ("load"), the root, the body and the
/// viewport read back; the device's edges and the place again, which change nothing ("device", "surface", "inert
/// tick"); a side of the client's within its range ("side"); a document past the bubble's bound ("tall") and the
/// host's own bound above it ("override").
pub fn bubble_place() -> Marked {
    let mut s = Script::new(12);
    profile_start(
        &mut s,
        &surface("message", "844"),
        "{\"top\":0,\"right\":0,\"bottom\":24,\"left\":0,\"cornerLeft\":16,\"cornerRight\":20}",
        "{\"top\":47,\"right\":13,\"bottom\":34,\"left\":11}",
    );
    let mut m = Marked::new(s);
    m.step("start inspect", 0.0, Step::Inspect);
    let ops = vec![sheet("#f{height:var(--tg-viewport-height,5px)}"), el(10, "div", 4, &[("id", "f")])];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    read_bubble(&mut m, "load");
    m.message("device", 16.0, &device_safe_area("{\"top\":60,\"right\":20,\"bottom\":40,\"left\":30}"));
    m.message("surface", 16.0, &surface("message", "500"));
    m.step("inert tick", 16.0, Step::Tick);
    m.message("side", 16.0, &safe_area("{\"top\":0,\"right\":10,\"bottom\":24,\"left\":0,\"cornerLeft\":16,\"cornerRight\":20}"));
    read_bubble(&mut m, "side");
    m.step("tall", 16.0, Step::Ops(batch(&[el(11, "div", 4, &[("id", "t"), ("style", "height:2000px")])])));
    m.step("override", 16.0, Step::MaxHeight(3000.0));
    m
}

/// The profile page's scripts both comparisons run (the renderer's on the C++ document's answers, the document's on
/// the Rust engine), by name.
pub fn profile_scripts() -> Vec<(Script, String)> {
    [
        ("the reference profile page", reference_profile()),
        ("core_tests' profile page", profile_page()),
        ("a page's heights", profile_heights()),
        ("a page without a viewport", profile_without_viewport()),
        ("a page laid out again", profile_relayout()),
        ("a bubble's place", bubble_place()),
    ]
    .into_iter()
    .map(|(name, m)| (m.script, name.to_string()))
    .collect()
}

/// A page ("page") or a bubble 9000 px tall, given the host's own bounds: past the page's ceiling ("raised"),
/// endless ("endless"), below the page's viewport ("lowered"), between ("between"); then a page's bound of 1200
/// ("bound") and its viewport sent again ("same viewport"), lower ("lower viewport") and above the bound ("higher
/// viewport"). renderer.cpp lets a host raise a page past its ceiling and its `surface` puts the ceiling back, the
/// port keeps the page's ceiling and the host's bound (`Renderer::set_max_height`): the port's own, never compared.
pub fn host_bounds(page: bool) -> Marked {
    let mut s = Script::new(12);
    s.width = 390.0;
    if page {
        profile_start(&mut s, &surface("profile", "844"), NO_INSET, NO_INSET);
    }
    let mut m = Marked::new(s);
    let ops = vec![style_op(4, "margin", "0"), el(10, "div", 4, &[("id", "c"), ("style", "height:9000px")])];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    m.step("raised", 16.0, Step::MaxHeight(8000.0));
    m.step("endless", 16.0, Step::MaxHeight(f64::INFINITY));
    m.step("lowered", 16.0, Step::MaxHeight(100.0));
    m.step("between", 16.0, Step::MaxHeight(3000.0));
    if page {
        m.step("bound", 16.0, Step::MaxHeight(1200.0));
        for (name, viewport) in [("same viewport", "844"), ("lower viewport", "1000"), ("higher viewport", "1500")] {
            m.message(name, 16.0, &surface("profile", viewport));
        }
    }
    m
}

/// The root's `--tg-viewport-height` ("… var") and `--own` ("… own") and `#f`'s rect ("… f"), as steps named `state`
/// and what they read.
fn read_root(m: &mut Marked, state: &str) {
    m.ask(format!("{state} var"), "{\"q\":\"cs\",\"id\":2,\"prop\":\"--tg-viewport-height\"}");
    m.ask(format!("{state} own"), "{\"q\":\"cs\",\"id\":2,\"prop\":\"--own\"}");
    m.ask(format!("{state} f"), "{\"q\":\"rect\",\"id\":10}");
}

/// A page ("page") or a bubble whose root the widget's style attribute gives `--tg-viewport-height` and `--own`,
/// adopted with it ("adopted") and set again ("set"), `#f` 30 px without the variable ("load"); a page then without
/// its viewport ("withdrawn"), each read back. renderer.cpp keeps the widget's variable where the host sets none, the
/// port writes the host's names on the root only with the host's values: the port's own, never compared.
pub fn root_style(page: bool) -> Marked {
    let mut s = Script::new(12);
    profile_start(&mut s, &surface(if page { "profile" } else { "message" }, "844"), NO_INSET, NO_INSET);
    let mut m = Marked::new(s);
    let ops = vec![sheet("#f{height:var(--tg-viewport-height,30px)}"), el(10, "div", 4, &[("id", "f")])];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    read_root(&mut m, "load");
    let adopted =
        "{\"op\":\"el\",\"id\":2,\"tag\":\"html\",\"adopt\":\"html\",\"attrs\":[[\"style\",\"--tg-viewport-height:123px;--own:4px\"]]}";
    m.step("adopted", 16.0, Step::Ops(batch(&[adopted.to_string()])));
    read_root(&mut m, "adopted");
    m.step("set", 16.0, Step::Ops(batch(&[attr_op(2, "style", "--own:5px;--tg-viewport-height:123px")])));
    read_root(&mut m, "set");
    if page {
        m.message("withdrawn", 16.0, &surface("profile", "0"));
        read_root(&mut m, "withdrawn");
    }
    m
}

/// A page 600 px tall ("page") whose 900 px document, or a bubble whose 100 px one, a long batch makes 100 px taller at
/// its frame ("batch"), built by a claim before that frame ("claim"), then the host's `update` once the 60 ms
/// animation of the box (10, its size observed) has ended ("update") and the frame after it ("tick"). With `late` the
/// claim comes at the update's time, the animation ending in its build: its `animationend` is held before its size.
/// renderer.cpp delivers the claim's size after a new one of the update's, the port before it: the port's own, never
/// compared.
pub fn held_size(page: bool, late: bool, update: &str) -> Marked {
    let mut s = Script::new(12);
    s.width = 390.0;
    profile_start(&mut s, &surface(if page { "profile" } else { "message" }, "600"), NO_INSET, NO_INSET);
    s.message(0.0, "{\"type\":\"listen\",\"types\":[\"animationend\"]}");
    let mut m = Marked::new(s);
    let (from, to) = if page { ("height:900px", "1000px") } else { ("height:100px", "200px") };
    let ops = vec![
        style_op(4, "margin", "0"),
        sheet("@keyframes fade{from{opacity:1}to{opacity:.5}} #c{animation:fade .06s linear}"),
        el(10, "div", 4, &[("id", "c"), ("style", from)]),
        "{\"op\":\"call\",\"id\":10,\"method\":\"observeResize\",\"args\":[]}".to_string(),
    ];
    m.step("load", 16.0, Step::Ops(document(&ops).into_bytes()));
    m.step("load tick", 16.0, Step::Tick);
    let long = vec![style_op(10, "height", to); twidget_renderer::PARTIAL_BATCH_OPS];
    m.step("batch", 16.0, Step::Ops(batch(&long)));
    let (claim, update_ms) = if late { (44.0, 0.0) } else { (0.0, 44.0) };
    m.step("claim", claim, Step::Claim { x: 10.0, y: 10.0 });
    m.message("update", update_ms, update);
    m.step("tick", 16.0, Step::Tick);
    m
}
