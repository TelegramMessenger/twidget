//! The Rust canvas interpreter against the C++ (`oracle/canvas_oracle`, built from core/src/canvas.cpp with css.cpp and
//! wire.cpp): scripts of command lists, `canvasCall` queries, size changes, trims, `measureText` and `imageData` answers
//! through one canvas and the same scripted host on both sides, compared as one transcript of every batch in its binary
//! form (every float by its bits, shared clip lists shared), every answer, the canvas's size and path floats after each
//! step, and every host call where it is made. The command lists are generated in canvas.ts's shapes and past them,
//! taken from the corpus recording and changed at random, the stress widget's frames, charts as the corpus draws them, and
//! every limit crossed. Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_testkit::{Comparison, Oracle, mutate};

fn case(c: &mut Comparison, script: &Script, what: impl Into<String>) {
    c.case(&script.input(), (1, script.transcript()), what);
}

fn generated(seed: u64, count: usize) {
    let Some(oracle) = Oracle::find("canvas_oracle") else { return };
    let mut random = Lcg::new(seed);
    let mut c = Comparison::new();
    for i in 0..count {
        let script = any_script(&mut random);
        case(&mut c, &script, format!("script {seed:x}/{i} (TWIDGET_CANVAS_SHOW={seed:x}/{i} prints it)"));
        if std::env::var("TWIDGET_CANVAS_SHOW").is_ok_and(|v| v == format!("{seed:x}/{i}")) {
            eprintln!("{script:?}");
            for step in &script.steps {
                if let Step::Run(text) = step {
                    eprintln!("{}", String::from_utf8_lossy(text));
                }
            }
        }
    }
    let tally = c.check(&oracle);
    eprintln!("generated scripts {seed:x}: {} transcripts alike", tally[1]);
}

#[test]
fn generated_scripts_alike() {
    generated(0xca_0001, 8000);
}

#[test]
fn more_generated_scripts_alike() {
    generated(0xca_0002, 8000);
}

#[test]
fn long_generated_scripts_alike() {
    let Some(oracle) = Oracle::find("canvas_oracle") else { return };
    let mut random = Lcg::new(0xca_0003);
    let mut c = Comparison::new();
    for i in 0..1500 {
        // State carried across many runs: stacks, clips, Path2D objects and gradients that outlive a batch.
        let mut steps = Vec::new();
        for _ in 0..8 + random.below(16) {
            let n = 10 + random.below(80);
            steps.push(Step::Run(commands(&mut random, n)));
            if random.chance(3) {
                let (m, a) = query(&mut random);
                steps.push(Step::Call(m, a));
            }
        }
        let script = Script::new(steps);
        case(&mut c, &script, format!("long script {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("long scripts: {} transcripts alike", tally[1]);
}

#[test]
fn recorded_streams_alike() {
    let Some(oracle) = Oracle::find("canvas_oracle") else { return };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/recordings/corpus.twrec");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let streams = recorded_streams(&bytes);
    assert!(streams.len() >= 20, "the corpus recording's canvas ops: {}", streams.len());
    let mut random = Lcg::new(0xca_0004);
    let mut c = Comparison::new();
    case(&mut c, &Script::runs(&streams), "every recorded stream on one canvas");
    for (i, stream) in streams.iter().enumerate() {
        for (w, h, allowance) in
            [(300, 150, twidget_canvas::Canvas::MAX_PIXELS), (0, 0, twidget_canvas::Canvas::MAX_PIXELS), (300, 150, 50_000)]
        {
            let mut script = Script::runs(std::slice::from_ref(stream));
            (script.width, script.height, script.allowance) = (w, h, allowance);
            case(&mut c, &script, format!("recorded stream {i} at {w} x {h}, {allowance} px allowed"));
        }
        let mut twice = Script::runs(&[stream.clone(), stream.clone()]);
        let (m, a) = query(&mut random);
        twice.steps.push(Step::Call(m, a));
        case(&mut c, &twice, format!("recorded stream {i} twice"));
        for k in 0..60 {
            let mut changed = stream.clone();
            let edits = mutate(&mut random, &mut changed);
            if twidget_json::Json::parse(&changed).is_none() {
                continue;
            }
            case(&mut c, &Script::runs(&[changed]), format!("recorded stream {i} changed {k}:{edits}"));
        }
        for k in 0..20 {
            // Generated commands spliced in at a command boundary.
            let at = stream.iter().enumerate().filter(|&(_, &b)| b == b'[').map(|(p, _)| p).nth(1 + random.below(40)).unwrap_or(1);
            let mut spliced = stream[..at].to_vec();
            let n = 1 + random.below(6);
            let extra = commands(&mut random, n);
            spliced.extend_from_slice(&extra[1..extra.len() - 1]);
            spliced.push(b',');
            spliced.extend_from_slice(&stream[at..]);
            if twidget_json::Json::parse(&spliced).is_none() {
                continue;
            }
            case(&mut c, &Script::runs(&[spliced]), format!("recorded stream {i} spliced {k}"));
        }
    }
    let tally = c.check(&oracle);
    eprintln!("{} recorded streams: {} transcripts alike", streams.len(), tally[1]);
}

#[test]
fn stress_and_chart_frames_alike() {
    let Some(oracle) = Oracle::find("canvas_oracle") else { return };
    let mut random = Lcg::new(0xca_0005);
    let mut c = Comparison::new();
    for n in [200, 1000, 2000, 4000, 8000, 16000] {
        let frames: Vec<Vec<u8>> = [0.0, 16.67, 33.4, 1000.0, 12345.6].iter().map(|&t| stress_frame(n, t)).collect();
        let mut script = Script::runs(&frames);
        (script.width, script.height) = (600, 600);
        script.digest = n > 2000;
        case(&mut c, &script, format!("canvas-cmds/{n}"));
    }
    for i in 0..600 {
        let frames: Vec<Vec<u8>> = (0..1 + random.below(3)).map(|_| chart_frame(&mut random)).collect();
        let mut script = Script::runs(&frames);
        if random.chance(3) {
            script.steps.push(Step::Measure(b"12px system-ui,sans-serif".to_vec(), b"Weekly budget".to_vec()));
        }
        case(&mut c, &script, format!("chart {i}"));
    }
    let tally = c.check(&oracle);
    eprintln!("stress and chart frames: {} transcripts alike", tally[1]);
}

fn joined(parts: impl IntoIterator<Item = String>) -> Vec<u8> {
    format!("[{}]", parts.into_iter().collect::<Vec<_>>().join(",")).into_bytes()
}

#[test]
fn limits_alike() {
    let Some(oracle) = Oracle::find("canvas_oracle") else { return };
    let mut c = Comparison::new();
    fn add(c: &mut Comparison, steps: Vec<Step>, what: &str) {
        let mut script = Script::new(steps);
        script.digest = true;
        case(c, &script, what);
    }
    let fill = |id: &str| format!("[\"s\",\"fillStyle\",{{\"g\":{id}}}],[\"c\",\"fillRect\",0,0,5,5]");
    // MAX_OBJECTS gradients: the oldest goes for a new one, a known one is replaced.
    let mut g: Vec<String> = (1..=4100).map(|i| format!("[\"g\",{i},\"linear\",0,0,10,0],[\"gs\",{i},0,\"red\"]")).collect();
    g.extend([fill("1"), fill("4"), fill("5"), fill("4100"), "[\"g\",4100,\"radial\",0,0,1,5,5,9]".into(), fill("4100")]);
    add(&mut c, vec![Step::Run(joined(g))], "4,100 gradients");
    // MAX_OBJECTS Path2D objects: past them a new one is refused, a known one is replaced.
    let mut p: Vec<String> = (1..=4100).map(|i| format!("[\"pn\",{i},\"M{} 0L4 0L4 4Z\"]", i % 5)).collect();
    p.extend([
        "[\"c\",\"fill\",{\"p\":1}],[\"c\",\"fill\",{\"p\":4096}],[\"c\",\"fill\",{\"p\":4097}]".to_string(),
        "[\"pn\",2,\"M9 9L1 1\"],[\"c\",\"stroke\",{\"p\":2}]".to_string(),
    ]);
    add(&mut c, vec![Step::Run(joined(p))], "4,100 Path2D objects");
    // MAX_STOPS stops, inserted after equal offsets.
    let stops: Vec<String> = (0..300).map(|i| format!("[\"gs\",1,{},\"#{:06x}\"]", (i % 7) as f64 / 6.0, i * 9973)).collect();
    add(
        &mut c,
        vec![Step::Run(joined(["[\"g\",1,\"linear\",0,0,10,0]".to_string()].into_iter().chain(stops).chain([fill("1")])))],
        "300 stops",
    );
    // MAX_STACK states, each with its own transform.
    let saves: Vec<String> = (0..600).map(|i| format!("[\"c\",\"translate\",{i},1],[\"c\",\"save\"]")).collect();
    let restores: Vec<String> = (0..600).map(|_| "[\"c\",\"restore\"],[\"c\",\"fillRect\",0,0,1,1]".to_string()).collect();
    add(
        &mut c,
        vec![
            Step::Run(joined(saves)),
            Step::Call(b"getTransform".to_vec(), b"[]".to_vec()),
            Step::Run(joined(restores)),
            Step::Call(b"getTransform".to_vec(), b"[]".to_vec()),
        ],
        "600 saves",
    );
    // MAX_COLORS colours, the cache cleared past them, and a trim.
    let colors: Vec<String> =
        (0..700).map(|i| format!("[\"s\",\"fillStyle\",\"#{:06x}\"],[\"c\",\"fillRect\",0,0,1,1]", (i % 300) * 4099)).collect();
    add(&mut c, vec![Step::Run(joined(colors.clone())), Step::Trim, Step::Run(joined(colors))], "700 colours");
    // MAX_FONT bytes of font, MAX_TEXT bytes of text cut at a character.
    for n in [511, 512, 513] {
        let font = format!("{}px s{}", 10, "x".repeat(n - 6));
        let texts = ["a".repeat(4096), format!("{}Ж", "a".repeat(4095)), format!("{}ЖЖ", "a".repeat(4093)), "Ж".repeat(3000)];
        let mut cmds = vec![format!("[\"s\",\"font\",{}]", quoted(&font))];
        cmds.extend(texts.iter().map(|t| format!("[\"c\",\"fillText\",{},0,10]", quoted(t))));
        let steps = vec![Step::Run(joined(cmds)), Step::Measure(font.into_bytes(), "a".repeat(5000).into_bytes())];
        add(&mut c, steps, &format!("a font of {n} bytes"));
    }
    // MAX_PATH_FLOATS in the current path, and an SVG path text of MAX_SVG_PATH bytes and one past.
    let lines: Vec<String> = (0..530_000).map(|i| format!("[\"c\",\"lineTo\",{},{}]", i % 97, i % 89)).collect();
    add(
        &mut c,
        vec![Step::Run(joined(lines.into_iter().chain([
            "[\"c\",\"stroke\"]".to_string(),
            "[\"c\",\"lineTo\",1,1]".to_string(),
            "[\"c\",\"fill\"]".to_string(),
        ])))],
        "530,000 lineTo",
    );
    for extra in [0usize, 1] {
        let mut d = String::from("M0 0");
        while d.len() < (1 << 20) - 8 {
            d.push_str(" L1 2");
        }
        while d.len() < (1 << 20) + extra {
            d.push(' ');
        }
        add(
            &mut c,
            vec![Step::Run(joined([format!("[\"pn\",1,{}]", quoted(&d)), "[\"c\",\"fill\",{\"p\":1}]".to_string()]))],
            &format!("an SVG path of 1 MiB + {extra}"),
        );
    }
    // MAX_PATHS_FLOATS over a canvas's Path2D objects, alone and with other canvases' floats.
    let mut grow = vec!["[\"pn\",1,\"M0 0L10 10L20 0Z\"]".to_string(), "[\"pn\",2,\"M0 0L10 10L20 0Z\"]".to_string()];
    for _ in 0..14 {
        grow.push("[\"pc\",1,\"addPath\",{\"p\":2}],[\"pc\",2,\"addPath\",{\"p\":1}]".to_string());
    }
    grow.extend([
        "[\"pn\",3,null]".to_string(),
        "[\"pc\",3,\"addPath\",{\"p\":1}]".to_string(),
        "[\"c\",\"addPath\",{\"p\":2}]".to_string(),
    ]);
    grow.extend(["[\"c\",\"fill\",{\"p\":1}],[\"c\",\"fill\",{\"p\":2}],[\"c\",\"fill\",{\"p\":3}],[\"c\",\"fill\"]".to_string()]);
    for others in [0, 1 << 21, (1 << 22) - 1000, u64::MAX - 5] {
        add(
            &mut c,
            vec![Step::Others(others), Step::Run(joined(grow.clone()))],
            &format!("Path2D objects added to each other, {others} floats elsewhere"),
        );
    }
    // MAX_CLIP_FLOATS and MAX_STACK_CLIP_FLOATS.
    let mut clip = vec!["[\"c\",\"beginPath\"]".to_string()];
    clip.extend((0..500).map(|i| format!("[\"c\",\"lineTo\",{},{}]", i % 100, i * 7 % 100)));
    clip.extend((0..300).map(|_| "[\"c\",\"clip\"],[\"c\",\"fillRect\",0,0,1,1]".to_string()));
    add(&mut c, vec![Step::Run(joined(clip))], "300 clips of a long path");
    let mut stack = vec!["[\"c\",\"rect\",0,0,9,9]".to_string()];
    stack.extend((0..512).map(|_| "[\"c\",\"clip\"],[\"c\",\"save\"]".to_string()));
    stack.extend((0..512).map(|_| "[\"c\",\"restore\"],[\"c\",\"fillRect\",0,0,1,1]".to_string()));
    add(&mut c, vec![Step::Run(joined(stack))], "512 clipped states");
    // A placeholder under a long clip, again and again: its clip copies are charged.
    let mut d = String::from("M0 0");
    for i in 0..8000 {
        d.push_str(&format!("L{} {}", i % 9, i % 7));
    }
    let mut placeholders = vec![format!("[\"pn\",1,{}]", quoted(&d)), "[\"c\",\"clip\",{\"p\":1}]".to_string()];
    placeholders.extend((0..1200).map(|_| "[\"c\",\"drawImage\",{\"n\":7},0,0]".to_string()));
    add(&mut c, vec![Step::Run(joined(placeholders))], "1,200 placeholders under a long clip");
    // MAX_RUN_FLOATS: a long Path2D filled again and again.
    let mut d = String::from("M0 0");
    for i in 0..50_000 {
        d.push_str(&format!(" {} {}", i % 100, i * 7 % 100));
    }
    let mut fills = vec![format!("[\"pn\",3,{}]", quoted(&d))];
    fills.extend((0..400).map(|_| "[\"c\",\"fill\",{\"p\":3}]".to_string()));
    add(&mut c, vec![Step::Run(joined(fills.clone())), Step::Run(joined(fills))], "400 fills of a long path, twice");
    // MAX_SIDE, MAX_PIXELS and the allowance.
    for (w, h) in [(8192, 2048), (8193, 1), (4096, 4096), (4097, 4096), (2000, 2400), (0, 5), (-1, -1), (i32::MAX, i32::MIN)] {
        for allowance in [twidget_canvas::Canvas::MAX_PIXELS, 2000 * 2000, 0, -1] {
            add(
                &mut c,
                vec![Step::Allowance(allowance), Step::Resize(w, h), Step::Run(b"[[\"c\",\"fillRect\",0,0,5,5]]".to_vec())],
                &format!("{w} x {h} with {allowance} allowed"),
            );
        }
    }
    // imageData at and past MAX_PIXELS.
    for (w, h) in [(4096, 4096), (4096, 4097), (8192, 8192), (1, 8193)] {
        let q = format!("{{\"q\":\"imageData\",\"x\":3,\"y\":-2,\"w\":{w},\"h\":{h}}}").into_bytes();
        add(&mut c, vec![Step::ImageData(q.clone(), 1), Step::ImageData(q, 0)], &format!("imageData {w} x {h}"));
    }
    // putImageData's sizes.
    for (w, h, n) in [(1, 1, 4), (1, 1, 3), (2, 3, 24), (8192, 1, 32768), (8193, 1, 32772), (0, 1, 4)] {
        let b64 = twidget_base64::encode(&vec![9u8; n]);
        let cmd = format!("[[\"c\",\"putImageData\",{{\"w\":{w},\"h\":{h},\"b64\":\"{b64}\"}},1.5,-2]]");
        add(&mut c, vec![Step::Run(cmd.into_bytes())], &format!("putImageData {w} x {h} of {n} bytes"));
    }
    // An arc and an SVG arc without a finite sweep, and objects named past every int.
    add(
        &mut c,
        vec![Step::Run(
            b"[[\"c\",\"moveTo\",1,1],[\"c\",\"arc\",50,50,10,1e308,-1e308],[\"c\",\"lineTo\",2,2],[\"c\",\"stroke\"],[\"pn\",1,\"M1e308 0A5 5 0 0 1 -1e308 5\"],[\"c\",\"fill\",{\"p\":1}],\
            [\"pn\",1e300,\"M0 0L5 5L0 5Z\"],[\"c\",\"fill\",{\"p\":1e300}],[\"g\",-1e300,\"linear\",0,0,10,0],[\"gs\",-1e300,0,\"#f00\"],[\"s\",\"fillStyle\",{\"g\":-1e300}],\
            [\"c\",\"fillRect\",0,0,10,10],[\"c\",\"drawImage\",{\"n\":1e300},0,0]]"
                .to_vec(),
        )],
        "huge sweeps and huge ids",
    );
    let tally = c.check(&oracle);
    eprintln!("limits: {} transcripts alike", tally[1]);
}
