//! The Rust shadow tree against the C++ renderer (`oracle/shadow_oracle`, built from core/src/renderer.cpp and the
//! whole core on litehtml): scripts of messages through one renderer on both sides, compared after every message as
//! one transcript of the violations the tree reports, the backend's canvas calls in their order (each batch by its
//! wire form), and `last_html()` byte for byte. The scripts are every session of tests/recordings/corpus.twrec step by
//! step and their batches changed at random, generated batches of every op kind and past them, and every limit the
//! ops meet crossed. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_testkit::{Oracle, hex, mutate};

/// Scripts compared in one run of the oracle: its answers first, from which the port takes what the renderer decided
/// itself (its layout and name limits), then the port's, byte for byte.
#[derive(Default)]
struct Cases(Vec<(Script, String)>);

impl Cases {
    fn new() -> Cases {
        Cases::default()
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn check(self, oracle: &Oracle, tally: &mut Tally) {
        let input: Vec<u8> = self.0.iter().flat_map(|(s, _)| s.record()).collect();
        let answers = oracle.run(input, self.0.len());
        let mut differences = Vec::new();
        for ((script, what), (status, cpp)) in self.0.iter().zip(&answers) {
            let facts = facts_of(cpp, script.digest);
            let rust = run(script, &facts, tally);
            if *status != 1 || *cpp != rust {
                let at = cpp.iter().zip(&rust).position(|(a, b)| a != b).unwrap_or(cpp.len().min(rust.len()));
                let near = |b: &[u8]| {
                    let piece = &b[at.saturating_sub(16).min(b.len())..(at + 48).min(b.len())];
                    if piece.iter().all(|c| (0x20..0x7f).contains(c)) {
                        format!("{:?}", String::from_utf8_lossy(piece))
                    } else {
                        hex(piece)
                    }
                };
                differences.push(format!(
                    "{what}: {} / {} bytes, first difference at {at}: C++ …{} / Rust …{}",
                    cpp.len(),
                    rust.len(),
                    near(cpp),
                    near(&rust)
                ));
            }
        }
        assert!(
            differences.is_empty(),
            "{} of {} scripts differ, the first:\n{}",
            differences.len(),
            answers.len(),
            differences[..differences.len().min(10)].join("\n")
        );
    }
}

fn case(c: &mut Cases, script: &Script, _tally: &mut Tally, what: impl Into<String>) {
    c.0.push((script.clone(), what.into()));
}

fn report(name: &str, cases: usize, tally: &Tally) {
    eprintln!("{name}: {cases} scripts alike;{}", tally.show());
}

fn corpus() -> Vec<Script> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    recorded_sessions(&bytes)
}

#[test]
fn recorded_sessions_alike() {
    let Some(oracle) = Oracle::find("shadow_oracle") else { return };
    let sessions = corpus();
    assert_eq!(sessions.len(), 4, "the corpus recording's sessions");
    let batches: usize = sessions.iter().map(|s| s.steps.iter().filter(|x| matches!(x, Step::Ops(_))).count()).sum();
    assert_eq!(batches, 60, "the corpus recording's ops batches");
    let mut random = Lcg::new(0x5d_0001);
    let mut tally = Tally::default();
    let mut c = Cases::new();
    for (i, s) in sessions.iter().enumerate() {
        case(&mut c, s, &mut tally, format!("recorded session {i}"));
        // Every batch changed at random, the rest of the session as recorded.
        for (k, step) in s.steps.iter().enumerate() {
            let Step::Ops(text) = step else { continue };
            for n in 0..40 {
                let mut changed = text.clone();
                let edits = mutate(&mut random, &mut changed);
                let mut script = s.clone();
                script.steps[k] = Step::Ops(changed);
                case(&mut c, &script, &mut tally, format!("recorded session {i}, batch {k} changed {n}:{edits}"));
            }
        }
        // Its batches again, in the other scheme, and in reverse order.
        let mut again = s.clone();
        again.steps.extend(s.steps.clone());
        again.dark = !again.dark;
        case(&mut c, &again, &mut tally, format!("recorded session {i} twice"));
        let mut reversed = s.clone();
        reversed.steps.reverse();
        case(&mut c, &reversed, &mut tally, format!("recorded session {i} reversed"));
        // Generated batches spliced in between the recorded ones.
        for n in 0..60 {
            let mut script = s.clone();
            let mut m = Model::new();
            let at = random.below(script.steps.len() + 1);
            let k = 1 + random.below(20);
            script.steps.insert(at, Step::Ops(batch(&mut random, &mut m, k)));
            case(&mut c, &script, &mut tally, format!("recorded session {i} with a batch at {at} ({n})"));
        }
    }
    let n = c.len();
    c.check(&oracle, &mut tally);
    report(&format!("recorded sessions ({batches} batches)"), n, &tally);
}

fn generated(seed: u64, count: usize) -> (usize, Tally) {
    let Some(oracle) = Oracle::find("shadow_oracle") else { return (0, Tally::default()) };
    let mut random = Lcg::new(seed);
    let mut tally = Tally::default();
    let mut c = Cases::new();
    for i in 0..count {
        let script = any_script(&mut random);
        if std::env::var("TWIDGET_SHADOW_SHOW").is_ok_and(|v| v == format!("{seed:x}/{i}")) {
            for step in &script.steps {
                match step {
                    Step::Ops(t) | Step::Message(t) => eprintln!("{}", String::from_utf8_lossy(t)),
                    Step::Dark(d) => eprintln!("dark {d}"),
                }
            }
        }
        case(&mut c, &script, &mut tally, format!("script {seed:x}/{i} (TWIDGET_SHADOW_SHOW={seed:x}/{i} prints it)"));
    }
    let n = c.len();
    c.check(&oracle, &mut tally);
    (n, tally)
}

#[test]
fn generated_scripts_alike() {
    let (n, tally) = generated(0x5d_0002, 5000);
    report("generated scripts", n, &tally);
    if n > 0 {
        for kind in
            ["violation element name", "violation attribute onclick", "violation property name", "canvases 1", "canvases 3", "ends frozen"]
        {
            assert!(tally.count(kind) > 0, "the scripts reach {kind}:{}", tally.show());
        }
    }
}

#[test]
fn more_generated_scripts_alike() {
    let (n, tally) = generated(0x5d_0003, 5000);
    report("more generated scripts", n, &tally);
}

#[test]
fn changed_scripts_alike() {
    let Some(oracle) = Oracle::find("shadow_oracle") else { return };
    let mut random = Lcg::new(0x5d_0004);
    let mut tally = Tally::default();
    let mut c = Cases::new();
    for i in 0..4000 {
        let mut script = any_script(&mut random);
        for step in &mut script.steps {
            if let Step::Ops(text) | Step::Message(text) = step
                && random.chance(2)
            {
                mutate(&mut random, text);
            }
        }
        case(&mut c, &script, &mut tally, format!("changed script {i}"));
    }
    let n = c.len();
    c.check(&oracle, &mut tally);
    report("changed scripts", n, &tally);
}

fn joined(parts: impl IntoIterator<Item = String>) -> Vec<u8> {
    format!("[{}]", parts.into_iter().collect::<Vec<_>>().join(",")).into_bytes()
}

#[test]
fn limits_alike() {
    let Some(oracle) = Oracle::find("shadow_oracle") else { return };
    let mut tally = Tally::default();
    let mut c = Cases::new();
    let add = |c: &mut Cases, steps: Vec<Step>, what: &str, tally: &mut Tally| {
        let mut script = Script::new([vec![Step::Ops(PAGE.as_bytes().to_vec())], steps].concat());
        script.digest = true;
        case(c, &script, tally, what);
    };
    let el = |id: i64, tag: &str| format!("{{\"op\":\"el\",\"id\":{id},\"tag\":\"{tag}\",\"attrs\":[]}}");
    let ins = |parent: i64, id: i64| format!("{{\"op\":\"ins\",\"parent\":{parent},\"id\":{id},\"before\":null}}");
    // MAX_BATCH_OPS, and one under it.
    for n in [200000usize, 199999] {
        let ops = std::iter::once(el(9000, "div")).chain(std::iter::repeat_n("{\"op\":\"rm\",\"id\":1}".to_string(), n));
        add(&mut c, vec![Step::Ops(joined(ops)), Step::Ops(joined([ins(4, 9000)]))], &format!("1 + {n} ops"), &mut tally);
    }
    // MAX_TEXT_BYTES and MAX_TEXT_PIECES: texts that fit, one past them, made shorter, removed.
    let words = |n: usize| "a ".repeat(n);
    let text = |id: i64, n: usize| format!("{{\"op\":\"text\",\"id\":{id},\"data\":\"{}\"}}", words(n));
    add(
        &mut c,
        vec![
            Step::Ops(joined([text(100, 60000)])),
            Step::Ops(joined([text(101, 40000), ins(4, 101)])),
            Step::Ops(joined([text(102, 20000)])),
            Step::Ops(joined([format!("{{\"op\":\"data\",\"id\":101,\"data\":\"{}\"}}", words(60000))])),
            Step::Ops(joined([format!("{{\"op\":\"data\",\"id\":101,\"data\":\"{}\"}}", words(10000))])),
            Step::Ops(joined([text(102, 20000), text(103, 20000), text(104, 20000)])),
            Step::Ops(joined(["{\"op\":\"rm\",\"id\":102}".to_string(), "{\"op\":\"rm\",\"id\":103}".to_string()])),
            Step::Ops(joined([text(105, 40000)])),
        ],
        "a document's text",
        &mut tally,
    );
    let long = "x".repeat(1 << 20);
    add(
        &mut c,
        vec![Step::Ops(joined((0..5).map(|i| format!("{{\"op\":\"text\",\"id\":{},\"data\":\"{long}\"}}", 200 + i))))],
        "4 MiB of text in five pieces",
        &mut tally,
    );
    // A textarea's value is text the document holds.
    add(
        &mut c,
        vec![
            Step::Ops(joined([
                el(50, "textarea"),
                ins(4, 50),
                format!("{{\"op\":\"prop\",\"id\":50,\"name\":\"value\",\"value\":\"{}\"}}", words(70000)),
            ])),
            Step::Ops(joined([format!("{{\"op\":\"prop\",\"id\":50,\"name\":\"value\",\"value\":\"{}\"}}", words(110000))])),
        ],
        "a textarea's value",
        &mut tally,
    );
    // MAX_NODE_ATTRS attributes and declarations, then the ones it has changed.
    let mut ops = vec![el(100, "div")];
    ops.extend((0..4097).map(|i| format!("{{\"op\":\"attr\",\"id\":100,\"name\":\"data-a{i}\",\"value\":\"1\"}}")));
    ops.extend((0..4097).map(|i| format!("{{\"op\":\"style\",\"id\":100,\"name\":\"--p{i}\",\"value\":\"1\",\"important\":false}}")));
    ops.push("{\"op\":\"attr\",\"id\":100,\"name\":\"data-a7\",\"value\":\"2\"}".to_string());
    ops.push("{\"op\":\"style\",\"id\":100,\"name\":\"--p7\",\"value\":\"2\",\"important\":true}".to_string());
    let style: String = (0..4100).map(|i| format!("--q{i}:{i};")).collect();
    ops.push(format!("{{\"op\":\"el\",\"id\":101,\"tag\":\"div\",\"attrs\":[[\"style\",\"{style}\"]]}}"));
    add(&mut c, vec![Step::Ops(joined(ops))], "4097 attributes and declarations", &mut tally);
    // MAX_SHEET_BLOCKS over a document's sheets.
    let rules = "a{}".repeat(9000);
    let sheet = |id: i64, css: &str| joined([el(id, "style"), format!("{{\"op\":\"css\",\"id\":{id},\"text\":\"{css}\"}}")]);
    add(
        &mut c,
        vec![
            Step::Ops(sheet(100, &rules)),
            Step::Ops(sheet(101, &rules)),
            Step::Ops(joined(["{\"op\":\"css\",\"id\":100,\"text\":\"a{}\"}".to_string()])),
            Step::Ops(sheet(101, &rules)),
            Step::Ops(joined(["{\"op\":\"rm\",\"id\":101}".to_string()])),
            Step::Ops(sheet(102, &rules)),
        ],
        "a document's sheets",
        &mut tally,
    );
    // MAX_TREE_BYTES: 13 MB set on one attribute again and again, then on new ones until the widget is stopped.
    let value = "A".repeat(13 << 20);
    let mut steps = vec![Step::Ops(joined([el(100, "div")]))];
    for i in 0..6 {
        steps.push(Step::Ops(format!("[{{\"op\":\"attr\",\"id\":100,\"name\":\"data-a\",\"value\":\"{value}\"}}]").into_bytes()));
        let _ = i;
    }
    for i in 0..5 {
        steps.push(Step::Ops(format!("[{{\"op\":\"attr\",\"id\":100,\"name\":\"data-b{i}\",\"value\":\"{value}\"}}]").into_bytes()));
    }
    steps.push(Step::Ops(joined([el(200, "div"), ins(4, 200)])));
    add(&mut c, steps, "78 MB in turn, then past 48 MB held", &mut tally);
    add(
        &mut c,
        vec![Step::Ops(format!("[{{\"op\":\"text\",\"id\":300,\"data\":\"{}\"}}]", "B".repeat((16 << 20) + 1)).into_bytes())],
        "one op past a quarter of the tree limit",
        &mut tally,
    );
    // MAX_NODES, detached, and a node removed to make room.
    let mut ops: Vec<String> = (100..100 + 50002).map(|id| format!("{{\"op\":\"text\",\"id\":{id},\"data\":\"t{id}\"}}")).collect();
    ops.extend([
        "{\"op\":\"el\",\"id\":60000,\"tag\":\"p\",\"attrs\":[[\"class\",\"late\"]]}".to_string(),
        ins(4, 60000),
        ins(4, 100),
        ins(4, 50099),
        ins(4, 50100),
        ins(4, 50101),
    ]);
    add(
        &mut c,
        vec![
            Step::Ops(joined(ops)),
            Step::Ops(joined([
                "{\"op\":\"rm\",\"id\":100}".to_string(),
                "{\"op\":\"el\",\"id\":60000,\"tag\":\"p\",\"attrs\":[[\"class\",\"late\"]]}".to_string(),
                ins(4, 60000),
            ])),
        ],
        "50,002 nodes",
        &mut tally,
    );
    // MAX_TREE_DEPTH: a chain built upward past 1024 levels, attached, and one removed whole.
    let chain = |levels: i64| {
        let mut ops = vec![el(100000, "div")];
        for i in 1..levels {
            ops.push(el(100000 + i, "div"));
            ops.push(ins(100000 + i, 100000 + i - 1));
        }
        ops
    };
    let mut ops = chain(3000);
    ops.push(ins(4, 102999));
    add(&mut c, vec![Step::Ops(joined(ops))], "a chain of 3000 attached", &mut tally);
    let mut ops = chain(3000);
    ops.push("{\"op\":\"rm\",\"id\":102999}".to_string());
    add(&mut c, vec![Step::Ops(joined(ops))], "a chain of 3000 removed", &mut tally);
    // Chains built downward under the body, every level past the limit refused, and a subtree moved under a deep node.
    for levels in [1020i64, 1100] {
        let mut ops = Vec::new();
        for i in 0..levels {
            ops.push(el(300000 + i, "div"));
            ops.push(ins(if i == 0 { 4 } else { 300000 + i - 1 }, 300000 + i));
        }
        ops.push(el(399999, "p"));
        ops.push(el(399998, "p"));
        ops.push(ins(399999, 399998));
        ops.push(ins(300000 + levels - 5, 399999));
        ops.push(ins(300000 + 1015, 399999));
        add(&mut c, vec![Step::Ops(joined(ops))], &format!("a chain of {levels} built downward"), &mut tally);
    }
    // Each limit at its boundary, one case on it and one past it.
    let bare = |c: &mut Cases, steps: Vec<Step>, what: &str, tally: &mut Tally| {
        let mut script = Script::new(steps);
        script.digest = true;
        case(c, &script, tally, what);
    };
    let attr = |id: i64, name: &str, len: usize| {
        format!("[{{\"op\":\"attr\",\"id\":{id},\"name\":\"{name}\",\"value\":\"{}\"}}]", "v".repeat(len)).into_bytes()
    };
    const MIB: usize = 1 << 20;
    for extra in [0usize, 1] {
        // MAX_TREE_BYTES / 4 * 3 held: four attributes, the count pumped past the limit by setting one again.
        let mut steps = vec![Step::Ops(joined([el(100, "div")]))];
        for (k, name) in ["data-a", "data-b", "data-c", "data-d"].iter().enumerate() {
            steps.push(Step::Ops(attr(100, name, 12 * MIB - 6 + if k == 3 { extra } else { 0 })));
        }
        steps.push(Step::Ops(attr(100, "data-a", 12 * MIB - 6)));
        steps.push(Step::Ops(attr(100, "data-a", 12 * MIB - 6)));
        steps.push(Step::Ops(joined([el(101, "p"), ins(4, 101)])));
        bare(&mut c, steps, &format!("48 MiB held, and {extra} more"), &mut tally);
        // MAX_TREE_BYTES / 4 in one op, once the count asks for the nodes' bytes.
        let mut steps = vec![Step::Ops(joined([el(100, "div")]))];
        for _ in 0..4 {
            steps.push(Step::Ops(attr(100, "data-a", 13 * MIB)));
        }
        steps.push(Step::Ops(attr(100, "data-b", 16 * MIB - 6 + extra)));
        steps.push(Step::Ops(joined([el(101, "p"), ins(4, 101)])));
        bare(&mut c, steps, &format!("16 MiB in one op, and {extra} more"), &mut tally);
        // MAX_TEXT_BYTES and MAX_TEXT_PIECES.
        let t = |id: i64, data: String| {
            format!("[{{\"op\":\"text\",\"id\":{id},\"data\":\"{data}\"}},{{\"op\":\"ins\",\"parent\":4,\"id\":{id},\"before\":null}}]")
                .into_bytes()
        };
        bare(
            &mut c,
            vec![
                Step::Ops(t(200, "x".repeat(4 * MIB - 1 + extra))),
                Step::Ops(t(201, "y".to_string())),
                Step::Ops(t(202, "z".to_string())),
            ],
            &format!("4 MiB of text less 1, and {extra} more"),
            &mut tally,
        );
        bare(
            &mut c,
            vec![
                Step::Ops(t(200, format!("{}{}", "a ".repeat(49999), if extra == 1 { "a" } else { "" }))),
                Step::Ops(t(201, "b".to_string())),
                Step::Ops(t(202, "c".to_string())),
            ],
            &format!("99,998 text pieces and {extra} more"),
            &mut tally,
        );
        // MAX_SHEET_BLOCKS.
        bare(
            &mut c,
            vec![
                Step::Ops(sheet(100, &"a{}".repeat(16383 + extra))),
                Step::Ops(sheet(101, "b{}")),
                Step::Ops(joined([format!("{{\"op\":\"css\",\"id\":101,\"text\":\"{}\"}}", "b{}".repeat(2))])),
            ],
            &format!("16,383 sheet blocks and {extra} more"),
            &mut tally,
        );
    }
    // MAX_TREE_DEPTH: a subtree of height h under the node j of an attached chain, on the boundary (j + h = 1021) and past it.
    let mut ops = Vec::new();
    for i in 0..1030 {
        ops.push(el(300000 + i, "div"));
        ops.push(ins(if i == 0 { 4 } else { 300000 + i - 1 }, 300000 + i));
    }
    for h in 1..=8i64 {
        for (k, j) in [(0, 1021 - h), (1, 1022 - h)] {
            let top = 400000 + 100 * h + 50 * k;
            for level in 0..h {
                ops.push(el(top + level, "i"));
                if level > 0 {
                    ops.push(ins(top + level - 1, top + level));
                }
            }
            ops.push(ins(300000 + j, top));
            // The subtree back under the body shows whether it was taken.
            ops.push(format!("{{\"op\":\"text\",\"id\":{},\"data\":\"h{h}k{k}\"}}", top + 49));
            ops.push(ins(top + h - 1, top + 49));
        }
    }
    let reveal: Vec<String> = (1..=8i64).flat_map(|h| [0, 1].map(|k| ins(4, 400000 + 100 * h + 50 * k))).collect();
    bare(
        &mut c,
        vec![Step::Ops(PAGE.as_bytes().to_vec()), Step::Ops(joined(ops)), Step::Ops(joined(reveal))],
        "subtrees on the depth boundary",
        &mut tally,
    );
    // MAX_BUILD_DEPTH: 300 levels written to the 128th.
    let mut ops = Vec::new();
    for i in 0..300 {
        let (id, parent) = (1000 + i, if i == 0 { 4 } else { 999 + i });
        ops.push(el(id, "div"));
        ops.push(ins(parent, id));
        if i % 100 == 99 {
            ops.push(format!("{{\"op\":\"text\",\"id\":{},\"data\":\"at{}\"}}", 2000 + i, i + 1));
            ops.push(ins(id, 2000 + i));
        }
    }
    add(&mut c, vec![Step::Ops(joined(ops))], "300 nested elements", &mut tally);
    // MAX_CANVASES, the canvases' pixels together, a resize past them, and a chain of canvases drawn from each other.
    let mut ops = Vec::new();
    for i in 0..70 {
        let id = 500 + i;
        ops.push(format!(
            "{{\"op\":\"el\",\"id\":{id},\"tag\":\"canvas\",\"attrs\":[[\"width\",\"{}\"],[\"height\",\"1024\"]]}}",
            512 * (1 + i % 5)
        ));
        ops.push(ins(4, id));
        ops.push(format!("{{\"op\":\"canvas\",\"id\":{id},\"cmds\":[[\"c\",\"fillRect\",0,0,5,5]]}}"));
    }
    ops.push("{\"op\":\"attr\",\"id\":500,\"name\":\"width\",\"value\":\"8192\"}".to_string());
    ops.push("{\"op\":\"attr\",\"id\":501,\"name\":\"height\",\"value\":\"1\"}".to_string());
    ops.push("{\"op\":\"rm\",\"id\":502}".to_string());
    ops.push("{\"op\":\"canvas\",\"id\":569,\"cmds\":[[\"c\",\"fillRect\",0,0,5,5]]}".to_string());
    add(&mut c, vec![Step::Ops(joined(ops))], "70 canvases", &mut tally);
    let mut ops: Vec<String> = Vec::new();
    for i in 0..100 {
        ops.push(format!(
            "{{\"op\":\"canvas\",\"id\":{},\"cmds\":[[\"c\",\"drawImage\",{{\"n\":{}}},0,0],[\"c\",\"fillRect\",{i},0,1,1]]}}",
            700 + i,
            701 + i
        ));
        ops.push(format!("{{\"op\":\"el\",\"id\":{},\"tag\":\"canvas\",\"attrs\":[[\"width\",\"{}\"]]}}", 701 + i, 10 + i));
        ops.push(ins(4, 701 + i));
    }
    ops.insert(0, "{\"op\":\"el\",\"id\":700,\"tag\":\"canvas\",\"attrs\":[]}".to_string());
    ops.insert(1, ins(4, 700));
    add(&mut c, vec![Step::Ops(joined(ops))], "100 canvases drawn from the next, each made after its drawing", &mut tally);
    let ops: Vec<String> = (0..200000)
        .map(|i| format!("{{\"op\":\"canvas\",\"id\":{},\"cmds\":[[\"c\",\"drawImage\",{{\"n\":{}}},0,0]]}}", 1000 + i, 1001 + i))
        .collect();
    add(&mut c, vec![Step::Ops(joined(ops))], "200,000 canvas ops drawing the next", &mut tally);
    // Where the tree meets its odd corners: images drawn into a canvas, options picked by index, roots adopted again
    // under new ids and with attributes they had, a textarea whose value starts a line.
    add(
        &mut c,
        vec![Step::Ops(
            br##"[{"op":"el","id":60,"tag":"img","attrs":[["src","data:image/svg+xml;base64,PHN2Zy8+"]]},{"op":"ins","parent":4,"id":60,"before":null},
            {"op":"el","id":61,"tag":"img","attrs":[["src","x"],["src","DATA:image/svg+xml,<svg/>"]]},
            {"op":"el","id":62,"tag":"img","attrs":[["src","data:image/png;base64,AA=="]]},
            {"op":"el","id":63,"tag":"img","ns":"svg","attrs":[["src","data:image/svg+xml,x"]]},
            {"op":"el","id":64,"tag":"canvas","attrs":[]},{"op":"ins","parent":4,"id":64,"before":null},
            {"op":"canvas","id":64,"cmds":[["c","drawImage",{"n":60},0,0],["c","drawImage",{"n":61},1,1,5,5],["c","drawImage",{"n":62},0,0],["c","drawImage",{"n":63},0,0],["c","drawImage",{"n":64},2,2]]},
            {"op":"el","id":70,"tag":"select","attrs":[]},{"op":"ins","parent":4,"id":70,"before":null},
            {"op":"el","id":71,"tag":"option","attrs":[["selected",""]]},{"op":"ins","parent":70,"id":71,"before":null},
            {"op":"text","id":72,"data":"between"},{"op":"ins","parent":70,"id":72,"before":null},
            {"op":"el","id":73,"tag":"option","attrs":[]},{"op":"ins","parent":70,"id":73,"before":null},
            {"op":"el","id":74,"tag":"option","attrs":[]},{"op":"ins","parent":70,"id":74,"before":null},
            {"op":"prop","id":70,"name":"selectedIndex","value":1},
            {"op":"el","id":80,"tag":"textarea","attrs":[]},{"op":"ins","parent":4,"id":80,"before":null},
            {"op":"prop","id":80,"name":"value","value":"\nfirst line\n<second>"}]"##
                .to_vec(),
        ),
        Step::Ops(
            br##"[{"op":"el","id":7,"tag":"html","adopt":"html","attrs":[["lang","en"],["a b","x"],["lang","fr"],["onload","x"],["style","color:red"]]},
            {"op":"el","id":8,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":2,"tag":"body","adopt":"body","attrs":[]},
            {"op":"el","id":9,"tag":"body","adopt":"body","attrs":[["class","b"],["class","c"]]},
            {"op":"prop","id":70,"name":"selectedIndex","value":7},{"op":"text","id":90,"data":"after"},{"op":"ins","parent":2,"id":90,"before":null}]"##
                .to_vec(),
        ),
        Step::Ops(br##"[{"op":"el","id":7,"tag":"html","adopt":"html","attrs":[["lang","de"],["data-x","1"]]},{"op":"prop","id":70,"name":"selectedIndex","value":0}]"##.to_vec())],
        "odd corners",
        &mut tally,
    );
    let n = c.len();
    c.check(&oracle, &mut tally);
    report("limits", n, &tally);
}

#[test]
fn corpus_widgets_alike() {
    let Some(oracle) = Oracle::find("shadow_oracle") else { return };
    let files = corpus_files();
    assert!(files.len() > 200, "widget-sandbox's corpus widgets: {}", files.len());
    let mut random = Lcg::new(0x5d_0005);
    let mut tally = Tally::default();
    let mut c = Cases::new();
    let mut nodes = 0usize;
    for (i, path) in files.iter().enumerate() {
        let html = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let ops = html_to_ops(&html, 1000);
        nodes += ops.iter().filter(|o| o.starts_with("{\"op\":\"el\"") || o.starts_with("{\"op\":\"text\"")).count();
        // As the sandbox hands a long flush over: in parts of 1000 ops.
        let mut steps = vec![Step::Ops(ROOTS.as_bytes().to_vec())];
        for part in ops.chunks(1000) {
            steps.push(Step::Ops(format!("[{}]", part.join(",")).into_bytes()));
        }
        let mut script = Script::new(steps);
        script.dark = i % 3 == 0;
        script.digest = true;
        case(&mut c, &script, &mut tally, format!("corpus widget {}", path.display()));
        // Then changed by script, in the other scheme, and with every node moved under the body.
        let mut m = Model::new();
        for k in 0..3 {
            let mut changed = script.clone();
            let n = 1 + random.below(30);
            changed.steps.push(Step::Ops(batch(&mut random, &mut m, n)));
            if k == 1 {
                changed.steps.push(Step::Dark(!script.dark));
            }
            if k == 2 {
                let reveal: Vec<String> = (1001..1001 + ops.len() as i64 / 2)
                    .rev()
                    .map(|id| format!("{{\"op\":\"ins\",\"parent\":4,\"id\":{id},\"before\":null}}"))
                    .collect();
                changed.steps.push(Step::Ops(format!("[{}]", reveal.join(",")).into_bytes()));
            }
            case(&mut c, &changed, &mut tally, format!("corpus widget {} changed {k}", path.display()));
        }
    }
    let n = c.len();
    c.check(&oracle, &mut tally);
    report(&format!("corpus widgets ({} files, {nodes} nodes)", files.len()), n, &tally);
}
