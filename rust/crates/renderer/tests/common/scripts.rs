//! This stage's scripts: documents patched batch after batch (holes, style attributes, texts, canvases, SVG attributes,
//! structural changes and what has no hole), themes and safe areas, sizes, every query kind with valid, malformed and
//! limit-crossing arguments, animations sampled and queried, resize diffs, `frozen`, and documents past the layout
//! budget and the name table's bounds.

use twidget_testkit::Lcg;

use super::inputs::{document, pick, quote};
use super::{Script, Step};

/// html(2) head(3) body(4); a style sheet (5); boxes 10–17, texts 20–27 in them, an inline span (30) with a long text
/// (31), a textarea (32), a canvas (33), an SVG (34) with a rect (35), a path (36) and a text (37) holding (38), an
/// image (39), a transformed box (40), a positioned box (41) holding (42), a details (43) with a summary (44).
pub fn page(r: &mut Lcg) -> (Vec<String>, u32) {
    let mut ops = Vec::new();
    let sheet = pick(
        r,
        &[
            ".a{color:red;padding:2px}.b{border:1px solid blue;width:40px}@keyframes f{from{opacity:0}to{opacity:1}} .anim{animation:f .3s linear 2}.t{transition:opacity .2s linear,width .2s}",
            ".a{height:12px}.b{display:flex}.c{font-size:20px;line-height:24px} @media (max-width:250px){.a{height:30px}} .t{transition:height .1s}",
            ".q{width:25vw;height:10px}.a{min-height:5vh}@keyframes s{to{transform:rotate(1turn)}} .anim{animation:s 1s infinite}",
            ".a{margin:var(--m,3px)} :root{--m:4px} body{padding-bottom:var(--tg-content-safe-area-inset-bottom,9px)} .b{color:var(--tg-theme-text-color,black)}",
            "",
        ],
    );
    ops.push(format!("{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}", quote(sheet)));
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
            ops.push(format!(
                "{{\"op\":\"el\",\"id\":5,\"tag\":\"style\",\"attrs\":[]}},{{\"op\":\"css\",\"id\":5,\"text\":{}}},{{\"op\":\"ins\",\"parent\":3,\"id\":5,\"before\":null}}",
                quote(&format!(".x{{display:{display}}} @keyframes w{{from{{width:10px}}to{{width:90px}}}} .y{{animation:w 1s infinite}}"))
            ));
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
