//! The Rust SVG geometry against the fork's (`oracle/svg_oracle`, el_svg.cpp compiled whole): numbers and
//! flags, path data and the builder's calls with edge values, path lengths and the points along them,
//! `transform` attributes and matrices, and documents laid out by the fork (every SVG the corpus writes, in its
//! pages and in its scripts' strings, their changes, and generated ones), compared element by element: outlines,
//! CTMs, CSS transforms, view boxes, intrinsic sizes and view groups, `<use>` placements with their budget, the
//! parents `<use>` lends, gradient coordinates. Each run reports what it reached. Skipped without
//! `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use std::collections::BTreeMap;

use common::svg::*;
use common::*;
use twidget_litehtml::css_effects::TransformMatrix;
use twidget_litehtml::el_svg::*;
use twidget_litehtml::html::lowcase;

/// Cases for one svg_oracle run and what they reached.
struct Run {
    c: Comparison,
    tally: Tally,
    kinds: BTreeMap<&'static str, usize>,
}

impl Run {
    fn new() -> Run {
        Run { c: Comparison::new(), tally: Tally::default(), kinds: BTreeMap::new() }
    }

    fn case(&mut self, kind: &'static str, op: u8, input: &[u8], answer: Vec<u8>, what: impl Into<String>) {
        *self.kinds.entry(kind).or_default() += 1;
        self.c.case(&framed(op, input), (1, answer), what);
    }

    fn check(self, name: &str) {
        let Some(oracle) = Oracle::find("svg_oracle") else { return };
        let total = self.c.len();
        self.c.check(&oracle);
        let kinds: Vec<String> = self.kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("{name}: {total} answers alike ({})", kinds.join(", "));
        eprintln!("{name} reached: {}", self.tally.report());
    }
}

fn changed(random: &mut Lcg, text: &[u8]) -> (Vec<u8>, String) {
    let mut bytes = text.to_vec();
    if bytes.is_empty() {
        bytes.push(b'M');
    }
    let what = mutate(random, &mut bytes);
    (bytes, what)
}

#[test]
fn constants_are_the_cpp_floats() {
    assert_eq!(pi.to_bits(), "3.14159265".parse::<f32>().unwrap().to_bits());
    let mut b = PathBuilder::new();
    b.ellipse(0.0, 0.0, 1.0, 1.0);
    assert_eq!(b.path.points[3].to_bits(), "0.5522847498".parse::<f32>().unwrap().to_bits());
}

#[test]
fn path_errors_end_the_path_where_it_is() {
    for (d, ops, end) in [
        (&b"M1 2 L3 4"[..], 2, PathEnd::End),
        (b"L1 2", 0, PathEnd::NoMoveto),
        (b"1 2", 0, PathEnd::NoMoveto),
        (b"M1 2 3 4 5", 2, PathEnd::BadArguments(b'L')),
        (b"M1 2z 3 4", 2, PathEnd::AfterClose),
        (b"M1 2 X 3", 1, PathEnd::UnknownCommand(b'X')),
        (b"M1 2 A1 1 0 2 1 5 5", 1, PathEnd::BadArguments(b'A')),
        (b"M1 2\0L3 4", 1, PathEnd::End),
    ] {
        let mut b = PathBuilder::new();
        assert_eq!(b.parse(d), end, "{}", String::from_utf8_lossy(d));
        assert_eq!(b.path.ops.len(), ops, "{}", String::from_utf8_lossy(d));
    }
    let mut b = PathBuilder::new();
    b.parse(b"m10 10 a5 5 0 1 0 10 0");
    assert_eq!(svg_path_length(&b.path, -1.0).1, None);
    assert_eq!(svg_path_length(&b.path, f32::NAN).1, None);
    assert_eq!(svg_path_length(&b.path, 1e9).1, Some((20.0, 10.0)));
}

#[test]
fn numbers_alike() {
    let mut run = Run::new();
    let texts = corpus_texts();
    let mut corpus = Vec::new();
    for name in
        ["viewBox", "points", "x", "y", "width", "height", "r", "cx", "cy", "rx", "ry", "x1", "y1", "x2", "y2", "offset", "dx", "dy"]
    {
        corpus.extend(corpus_attribute_values(&texts, name));
    }
    for v in &corpus {
        numbers_tally(&mut run.tally, v);
        let (input, answer) = numbers_case(v);
        run.case("corpus", b'n', &input, answer, format!("corpus {}", String::from_utf8_lossy(v)));
    }
    let mut random = Lcg::new(0x5_0001);
    for k in 0..24000 {
        let text = match k % 4 {
            0 => any_number_text(&mut random),
            1 => {
                let mut s = Vec::new();
                for _ in 0..random.below(8) {
                    s.extend_from_slice(path_number(&mut random).as_bytes());
                    s.extend_from_slice(pick(&mut random, &[" ", ",", "%", "px ", "e", "", "\t", " , ", "1", "0", "-"]).as_bytes());
                }
                s
            }
            2 => any_path_data(&mut random),
            _ => {
                let base = &corpus[random.below(corpus.len())];
                changed(&mut random, base).0
            }
        };
        numbers_tally(&mut run.tally, &text);
        let (input, answer) = numbers_case(&text);
        run.case("generated", b'n', &input, answer, format!("numbers of {}", hex(&text)));
    }
    run.check("numbers");
}

#[test]
fn paths_alike() {
    let mut run = Run::new();
    let texts = corpus_texts();
    let corpus = corpus_attribute_values(&texts, "d");
    assert!(corpus.len() > 100, "the corpus's path data is there ({})", corpus.len());
    let mut random = Lcg::new(0x5_0002);
    for d in &corpus {
        let ats = any_ats(&mut random);
        let (input, answer, end) = path_case(&ats, d);
        run.tally.path(d, end);
        run.case("corpus", b'p', &input, answer, format!("corpus {}", String::from_utf8_lossy(d)));
        for _ in 0..12 {
            let (bytes, what) = changed(&mut random, d);
            let (input, answer, end) = path_case(&ats, &bytes);
            run.tally.path(&bytes, end);
            run.case("corpus changed", b'p', &input, answer, format!("{}{what}", String::from_utf8_lossy(d)));
        }
    }
    for _ in 0..30000 {
        let d = any_path_data(&mut random);
        let ats = any_ats(&mut random);
        let (input, answer, end) = path_case(&ats, &d);
        run.tally.path(&d, end);
        run.case("generated", b'p', &input, answer, format!("generated {}", hex(&d)));
        if random.chance(4) {
            let (bytes, what) = changed(&mut random, &d);
            let (input, answer, end) = path_case(&ats, &bytes);
            run.tally.path(&bytes, end);
            run.case("generated changed", b'p', &input, answer, format!("{}{what}", hex(&d)));
        }
    }
    for key in [
        "command M",
        "command m",
        "command L",
        "command l",
        "command H",
        "command h",
        "command V",
        "command v",
        "command C",
        "command c",
        "command S",
        "command s",
        "command Q",
        "command q",
        "command T",
        "command t",
        "command A",
        "command a",
        "command Z",
        "command z",
        "path end End",
        "path end NoMoveto",
        "path end AfterClose",
    ] {
        assert!(run.tally.counts.contains_key(key), "nothing reached {key}");
    }
    run.check("paths");
}

#[test]
fn builders_alike() {
    let mut run = Run::new();
    let mut random = Lcg::new(0x5_0003);
    for _ in 0..24000 {
        let steps = any_builder_steps(&mut random);
        let ats = any_ats(&mut random);
        let (input, answer, arcs) = builder_case(&ats, &steps);
        for arc in arcs {
            run.tally.arc(arc);
        }
        run.case("generated", b'b', &input, answer, format!("{steps:?} at {ats:?}"));
    }
    // Arcs at every edge: zero, negative and huge radii, the same end point, NaN and infinite operands, rotations.
    let edges = [0.0f32, -0.0, 1.0, -1.0, 5.0, 1e-30, 1e30, f32::MAX, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1e-45, 3e38];
    for (k, &rx) in edges.iter().enumerate() {
        for &ry in &edges {
            for (rotation, x) in [(0.0, 10.0), (45.0, 0.0), (90.0, -7.5), (720.0, 1e30), (f32::NAN, 3.0), (-30.0, f32::INFINITY)] {
                let steps = vec![Step::Move(0.0, 0.0), Step::Arc(rx, ry, rotation, (k % 2) as i32, ((k / 2) % 2) as i32, x, 5.0)];
                let (input, answer, arcs) = builder_case(&[0.0, 3.0, 1e9], &steps);
                for arc in arcs {
                    run.tally.arc(arc);
                }
                run.case("arc edges", b'b', &input, answer, format!("{steps:?}"));
            }
        }
    }
    for key in ["arc Line", "arc NotFinite", "arc Cubics(1)", "arc Cubics(2)", "arc Cubics(3)", "arc Cubics(4)"] {
        assert!(run.tally.counts.contains_key(key), "nothing reached {key}");
    }
    run.check("builders");
}

#[test]
fn transforms_alike() {
    let mut run = Run::new();
    let texts = corpus_texts();
    let corpus = corpus_attribute_values(&texts, "transform");
    assert!(corpus.len() > 10, "the corpus's transforms are there ({})", corpus.len());
    let mut random = Lcg::new(0x5_0004);
    let (input, answer) = transform_case(None);
    run.case("null", b't', &input, answer, "a null text");
    for t in &corpus {
        transform_tally(&mut run.tally, t);
        let (input, answer) = transform_case(Some(t));
        run.case("corpus", b't', &input, answer, format!("corpus {}", String::from_utf8_lossy(t)));
        for _ in 0..10 {
            let (bytes, what) = changed(&mut random, t);
            let (input, answer) = transform_case(Some(&bytes));
            run.case("corpus changed", b't', &input, answer, format!("{}{what}", String::from_utf8_lossy(t)));
        }
    }
    for _ in 0..20000 {
        let t = any_transform_text(&mut random);
        transform_tally(&mut run.tally, &t);
        let (input, answer) = transform_case(Some(&t));
        run.case("generated", b't', &input, answer, format!("generated {}", hex(&t)));
        if random.chance(4) {
            let (bytes, what) = changed(&mut random, &t);
            let (input, answer) = transform_case(Some(&bytes));
            run.case("generated changed", b't', &input, answer, format!("{}{what}", hex(&t)));
        }
    }
    let any_matrix = |random: &mut Lcg| {
        if random.chance(5) {
            return TransformMatrix::default();
        }
        let mut f = || if random.chance(3) { any_f32(random) } else { builder_float(random) };
        TransformMatrix::new(f(), f(), f(), f(), f(), f())
    };
    for _ in 0..20000 {
        let (a, b) = (any_matrix(&mut random), any_matrix(&mut random));
        let (input, answer) = matrices_case(&a, &b);
        run.case("matrices", b'm', &input, answer, format!("{a:?} then {b:?}"));
    }
    run.check("transforms");
}

/// How many numbers a text holds, whether its first is a percentage, whether it starts with a flag, whether it is
/// a reference id.
fn numbers_tally(tally: &mut Tally, text: &[u8]) {
    tally.add(match numbers(Some(text)).len() {
        0 => "no number",
        1 => "one number",
        2..=4 => "2 to 4 numbers",
        _ => "5 numbers or more",
    });
    let mut p = 0;
    if next_number(text, &mut p).is_some() && text.get(p) == Some(&b'%') {
        tally.add("a percentage");
    }
    let mut p = 0;
    if next_flag(text, &mut p).is_some() {
        tally.add("a flag");
    }
    if svg_reference_id(c_str(text)) {
        tally.add("a reference id");
    }
}

/// The functions a transform text names before a `(`, by lower-case name and how many numbers they hold.
fn transform_tally(tally: &mut Tally, text: &[u8]) {
    let text = c_str(text);
    let mut at = 0;
    while let Some(open) = text[at..].iter().position(|c| *c == b'(').map(|p| p + at) {
        let start = text[..open].iter().rposition(|c| !c.is_ascii_alphabetic()).map_or(0, |p| p + 1);
        let Some(close) = text[open..].iter().position(|c| *c == b')').map(|p| p + open) else { break };
        let count = numbers(Some(&text[open + 1..close])).len();
        let name = lowcase(&text[start..open]);
        if [&b"matrix"[..], b"translate", b"scale", b"rotate", b"skewx", b"skewy"].contains(&&name[..]) {
            tally.add(format!("{}({count})", String::from_utf8_lossy(&name)));
        } else {
            tally.add("unknown function");
        }
        at = close + 1;
    }
}

/// Runs documents through svg_oracle and compares each with the port.
fn documents_alike(documents: &[(String, Vec<u8>)], name: &str) {
    let Some(oracle) = Oracle::find("svg_oracle") else { return };
    let mut input = Vec::new();
    for (_, html) in documents {
        let mut o = Out::default();
        o.i32(375);
        o.s.extend_from_slice(html);
        input.extend(framed(b'D', &o.s));
    }
    let answers = oracle.run(input, documents.len());
    let names = twidget_litehtml::Names::new();
    let mut tally = Tally::default();
    let mut total = DocumentResult::default();
    let mut differences = Vec::new();
    let mut built = 0;
    for ((what, _), (status, bytes)) in documents.iter().zip(&answers) {
        if *status != 1 {
            continue;
        }
        built += 1;
        let r = document_compare(&names, bytes, &mut tally);
        total.nodes += r.nodes;
        total.answers_alike += r.answers_alike;
        total.calc_skipped += r.calc_skipped;
        total.uses += r.uses;
        total.chains += r.chains;
        total.gradients_compared += r.gradients_compared;
        total.gradients_refused += r.gradients_refused;
        for d in r.differences {
            if differences.len() < 8 {
                differences.push(format!("{what}: {d}"));
            } else {
                differences.push(String::new());
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences, the first:\n{}",
        differences.len(),
        differences.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
    eprintln!(
        "{name}: {built} of {} documents, {} elements' answers alike ({} left out for a calc length), {} <use> calls, {} host chains, {} gradients alike ({} the C++ painted none)",
        documents.len(),
        total.answers_alike,
        total.calc_skipped,
        total.uses,
        total.chains,
        total.gradients_compared,
        total.gradients_refused
    );
    eprintln!("{name} reached: {}", tally.report());
}

fn corpus_documents() -> Vec<(String, Vec<u8>)> {
    let texts = corpus_texts();
    let mut documents: Vec<(String, Vec<u8>)> = texts
        .iter()
        .filter(|(name, text)| {
            name.ends_with(".html")
                && !name.contains("render-nested-")
                && text.len() < 300_000
                && text.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
        })
        .cloned()
        .collect();
    for (name, snippet) in corpus_svg_snippets(&texts) {
        let mut html = b"<!doctype html><html><body>".to_vec();
        html.extend_from_slice(&snippet);
        html.extend_from_slice(b"</body></html>");
        documents.push((name, html));
    }
    documents
}

#[test]
fn corpus_documents_alike() {
    let documents = corpus_documents();
    assert!(documents.len() > 250, "the corpus's SVGs are there ({})", documents.len());
    documents_alike(&documents, "corpus documents");
}

#[test]
fn changed_corpus_documents_alike() {
    let mut random = Lcg::new(0x5_0005);
    let mut documents = Vec::new();
    for (name, html) in corpus_documents().iter().filter(|(name, _)| name.contains(" svg ")) {
        for k in 0..4 {
            let (bytes, what) = changed(&mut random, html);
            documents.push((format!("{name} change {k}{what}"), bytes));
        }
    }
    documents_alike(&documents, "changed corpus SVGs");
}

#[test]
fn generated_documents_alike() {
    let mut random = Lcg::new(0x5_0006);
    let documents: Vec<(String, Vec<u8>)> = (0..2400).map(|i| (format!("document {i}"), any_svg_document(&mut random))).collect();
    documents_alike(&documents, "generated documents");
}
