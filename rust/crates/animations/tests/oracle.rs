//! The Rust animator against the C++ (`oracle/animations_oracle`, built from core/src/animations.cpp with css.cpp and
//! paint_policy.cpp): scripts of builds, samples, Web Animations messages and every query through one animator on both
//! sides, compared as one transcript of every answer (overrides, events, peeks, plans, signatures, the `anims` query,
//! heavy motions and their hashes, every double by its bits); and the free functions on their own: timing functions
//! and their curves, interpolation, keyframe rules, animation and transition specs, Web Animations records. Inputs are
//! generated in the shapes widgets write and past them, changed at random, and taken from widget-sandbox's corpus.
//! Skipped without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_testkit::{Comparison, Oracle, mutate};

fn shared() -> SharedCaches {
    SharedCaches::default()
}

fn script_case(c: &mut Comparison, caches: &SharedCaches, steps: &[Step], tally: &mut Tally, what: impl Into<String>) {
    let answer = run_script(caches, steps, tally);
    c.case(&script_record(steps), (1, answer), what);
}

fn report(name: &str, cases: usize, tally: &Tally) {
    eprintln!("{name}: {cases} cases alike;{}", tally.show());
}

fn generated(seed: u64, count: usize) -> (usize, Tally) {
    let Some(oracle) = Oracle::find("animations_oracle") else { return (0, Tally::default()) };
    let caches = shared();
    let mut random = Lcg::new(seed);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..count {
        let steps = any_script(&mut random);
        if std::env::var("TWIDGET_ANIMATIONS_SHOW").is_ok_and(|v| v == format!("{seed:x}/{i}")) {
            for step in &steps {
                eprintln!("{step:?}");
            }
        }
        script_case(&mut c, &caches, &steps, &mut tally, format!("script {seed:x}/{i} (TWIDGET_ANIMATIONS_SHOW={seed:x}/{i} prints it)"));
    }
    let n = c.len();
    c.check(&oracle);
    (n, tally)
}

#[test]
fn generated_scripts_alike() {
    let (n, tally) = generated(0xa1_0001, 6000);
    report("generated scripts", n, &tally);
}

#[test]
fn more_generated_scripts_alike() {
    let (n, tally) = generated(0xa1_0002, 6000);
    report("more generated scripts", n, &tally);
    if n > 0 {
        for kind in [
            "event animationstart",
            "event animationiteration",
            "event animationend",
            "event transitionend",
            "plan loop",
            "plan span",
            "plan waiting",
            "plan none",
            "motion a",
            "motion t",
            "motion w",
            "query animation",
            "query transition",
        ] {
            assert!(tally.count(kind) > 0, "the scripts reach {kind}:{}", tally.show());
        }
    }
}

#[test]
fn long_scripts_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0003);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..800 {
        // Three scripts end to end on one animator: state that outlives many builds and samples.
        let mut steps = any_script(&mut random);
        for _ in 0..2 {
            let more = any_script(&mut random);
            let base =
                steps.iter().rev().find_map(|s| if let Step::Sample(t) | Step::Update(t, _) = s { Some(*t) } else { None }).unwrap_or(0.0);
            steps.extend(more.into_iter().map(|s| shift(s, base)));
        }
        script_case(&mut c, &caches, &steps, &mut tally, format!("long script {i}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("long scripts", n, &tally);
}

/// `step` later by `by` seconds.
fn shift(step: Step, by: f64) -> Step {
    match step {
        Step::Update(t, e) => Step::Update(t + by, e),
        Step::Sample(t) => Step::Sample(t + by),
        Step::Web(text, offset) => Step::Web(text, offset + by * 1000.0),
        Step::Active(t, s) => Step::Active(t + by, s),
        Step::Ids(t) => Step::Ids(t + by),
        Step::Peek(id, t) => Step::Peek(id, t + by),
        Step::Plan(id, t, i) => Step::Plan(id, t + by, i),
        Step::Query(ids, t) => Step::Query(ids, t + by),
        Step::Heavy(t) => Step::Heavy(t + by),
        other => other,
    }
}

fn mutated(random: &mut Lcg, bytes: &[u8]) -> Vec<u8> {
    let mut changed = bytes.to_vec();
    mutate(random, &mut changed);
    changed
}

#[test]
fn changed_scripts_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0004);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..4000 {
        let steps: Vec<Step> = any_script(&mut random)
            .into_iter()
            .map(|step| {
                if !random.chance(3) {
                    return step;
                }
                match step {
                    Step::Sheets(texts) => Step::Sheets(texts.iter().map(|t| mutated(&mut random, t)).collect()),
                    Step::Update(now, elements) => Step::Update(
                        now,
                        elements
                            .into_iter()
                            .map(|(id, pairs)| (id, pairs.into_iter().map(|(k, v)| (k, mutated(&mut random, &v))).collect()))
                            .collect(),
                    ),
                    Step::Web(text, offset) => Step::Web(mutated(&mut random, &text), offset),
                    other => other,
                }
            })
            .collect();
        script_case(&mut c, &caches, &steps, &mut tally, format!("changed script {i}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("changed scripts", n, &tally);
}

const XS: [f64; 22] = [
    -2.0,
    -0.5,
    -1e-9,
    -0.0,
    0.0,
    1e-9,
    0.1,
    0.25,
    0.3333333333333333,
    0.5,
    0.6,
    0.75,
    0.9,
    0.999999,
    1.0,
    1.000001,
    1.5,
    3.0,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.123456789,
];

#[test]
fn timings_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0005);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..20000 {
        let mut text = timing_text(&mut random).into_bytes();
        if random.chance(5) {
            text = mutated(&mut random, &text);
        }
        let mut xs: Vec<f64> = XS.to_vec();
        for _ in 0..8 {
            xs.push((random.below(30001) as f64 - 10000.0) / 10000.0);
        }
        let answer = run_timing(&caches, &text, &xs, &mut tally);
        c.case(&timing_record(&text, &xs), (1, answer), format!("timing {i}: {}", String::from_utf8_lossy(&text)));
    }
    let n = c.len();
    c.check(&oracle);
    report("timings", n, &tally);
}

const TS: [f64; 14] = [0.0, 1.0, 0.25, 0.5, 0.4999, 0.75, -0.3, 1.4, 1e-7, 0.999, f64::NAN, f64::INFINITY, 0.1, 0.9];

fn value_pair(random: &mut Lcg) -> (Vec<u8>, Vec<u8>) {
    let p = *pick(random, &ANIMATED);
    let q = if random.chance(6) { *pick(random, &ANIMATED) } else { p };
    let (mut a, mut b) = (value_of(random, p), value_of(random, q));
    match random.below(12) {
        0 => a = "none".to_string(),
        1 => b = "none".to_string(),
        2 => b = a.clone(),
        _ => {}
    }
    (a.into_bytes(), b.into_bytes())
}

#[test]
fn interpolations_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0006);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..40000 {
        let (mut a, mut b) = value_pair(&mut random);
        if random.chance(8) {
            a = mutated(&mut random, &a);
        }
        if random.chance(8) {
            b = mutated(&mut random, &b);
        }
        let t = if random.chance(2) { *pick(&mut random, &TS) } else { random.below(1001) as f64 / 1000.0 };
        let answer = run_interpolate(&caches, &a, &b, t, &mut tally);
        c.case(
            &interpolate_record(&a, &b, t),
            (1, answer),
            format!("interpolate {i}: {:?} {:?} {t}", String::from_utf8_lossy(&a), String::from_utf8_lossy(&b)),
        );
    }
    let n = c.len();
    c.check(&oracle);
    report("interpolations", n, &tally);
}

#[test]
fn pair_cache_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0007);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    let add = |c: &mut Comparison, a: &[u8], b: &[u8], t: f64, tally: &mut Tally| {
        let answer = run_interpolate(&caches, a, b, t, tally);
        c.case(
            &interpolate_record(a, b, t),
            (1, answer),
            format!("pair {:?} {:?} {t}", String::from_utf8_lossy(a), String::from_utf8_lossy(b)),
        );
    };
    // Two pairs that joined to one key while it was a, 0x01 and b: each keeps its own entry.
    for round in 0..40 {
        let k = round % 7;
        let (first, second) = ((format!("{k}px\u{1}"), format!("{}px\u{1}", k + 4)), (format!("{k}px"), format!("\u{1}{}px\u{1}", k + 4)));
        add(&mut c, first.0.as_bytes(), first.1.as_bytes(), 0.5, &mut tally);
        add(&mut c, second.0.as_bytes(), second.1.as_bytes(), 0.5, &mut tally);
        // Pairs of long lists crossing the bounds on entries and on bytes, and keys past 4096 bytes.
        for _ in 0..1 + random.below(120) {
            let n = *pick(&mut random, &[1usize, 10, 100, 300, 1000, 1400]);
            let (mut x, mut y) = (random.below(1000).to_string(), random.below(1000).to_string());
            for _ in 0..n {
                x.push_str(" 1px");
                y.push_str(" 2px");
            }
            add(&mut c, x.as_bytes(), y.as_bytes(), 0.25, &mut tally);
        }
        add(&mut c, second.0.as_bytes(), second.1.as_bytes(), 0.5, &mut tally);
        add(&mut c, first.0.as_bytes(), first.1.as_bytes(), 0.5, &mut tally);
    }
    let list = |random: &mut Lcg| -> String {
        let n = random.below(6);
        (0..n).map(|_| format!("{}px ", random.below(20))).collect()
    };
    for _ in 0..3000 {
        // (x, y 0x01 z) and (x 0x01 y, z) joined to the same key too.
        let (x, y, z) = (list(&mut random), list(&mut random), list(&mut random));
        let (one, two) = ((x.clone(), format!("{y}\u{1}{z}")), (format!("{x}\u{1}{y}"), z));
        let t = *pick(&mut random, &[0.25, 0.5, 0.75]);
        if random.chance(2) {
            add(&mut c, one.0.as_bytes(), one.1.as_bytes(), t, &mut tally);
            add(&mut c, two.0.as_bytes(), two.1.as_bytes(), t, &mut tally);
        } else {
            add(&mut c, two.0.as_bytes(), two.1.as_bytes(), t, &mut tally);
            add(&mut c, one.0.as_bytes(), one.1.as_bytes(), t, &mut tally);
        }
    }
    let n = c.len();
    c.check(&oracle);
    report("pair cache", n, &tally);
}

#[test]
fn keyframes_and_specs_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0008);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    for i in 0..8000 {
        let mut sheets: Vec<Vec<u8>> = (0..1 + random.below(3)).map(|_| style_sheet(&mut random).into_bytes()).collect();
        if random.chance(4) {
            let k = random.below(sheets.len());
            sheets[k] = mutated(&mut random, &sheets[k]);
        }
        let answer = run_keyframes(&caches, &sheets, &mut tally);
        c.case(&keyframes_record(&sheets), (1, answer), format!("keyframes {i}"));
        let mut pairs = motion_declarations(&mut random);
        if random.chance(4) && !pairs.is_empty() {
            let k = random.below(pairs.len());
            pairs[k].1 = mutated(&mut random, &pairs[k].1);
        }
        let answer = run_specs(&caches, &pairs, &mut tally);
        c.case(&specs_record(&pairs), (1, answer), format!("specs {i}: {pairs:?}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("keyframes and specs", n, &tally);
}

#[test]
fn web_records_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_0009);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    let ids = [1i64, 2, 3, 1 << 40, -5];
    for i in 0..20000 {
        let now = random.below(100000) as f64 / 100.0;
        let mut text = web_item(&mut random, &ids, now, &[now]).into_bytes();
        if random.chance(4) {
            text = mutated(&mut random, &text);
        }
        let offset = if random.chance(2) { 0.0 } else { random.below(100000) as f64 - 50000.0 };
        let answer = run_web(&caches, &text, offset, &mut tally);
        c.case(&web_record(&text, offset), (1, answer), format!("web record {i}: {}", String::from_utf8_lossy(&text)));
    }
    let n = c.len();
    c.check(&oracle);
    report("web records", n, &tally);
}

#[test]
fn corpus_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let sheets = corpus_sheets();
    let animated: Vec<&Vec<u8>> = sheets.iter().filter(|s| s.windows(9).any(|w| w.eq_ignore_ascii_case(b"keyframes"))).collect();
    assert!(animated.len() > 50, "style sheets with @keyframes in widget-sandbox's corpus: {}", animated.len());
    let caches = shared();
    let mut random = Lcg::new(0xa1_000a);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    let mut declarations = Vec::new();
    for (i, sheet) in animated.iter().enumerate() {
        let list = std::slice::from_ref(*sheet);
        let answer = run_keyframes(&caches, list, &mut tally);
        c.case(&keyframes_record(list), (1, answer), format!("corpus sheet {i}"));
        for k in 0..20 {
            let changed = vec![mutated(&mut random, sheet)];
            let answer = run_keyframes(&caches, &changed, &mut tally);
            c.case(&keyframes_record(&changed), (1, answer), format!("corpus sheet {i} changed {k}"));
        }
        declarations.extend(sheet_declarations(sheet));
    }
    assert!(declarations.len() > 100, "declarations of animations and transitions in the corpus: {}", declarations.len());
    for (i, pairs) in declarations.iter().enumerate() {
        let answer = run_specs(&caches, pairs, &mut tally);
        c.case(&specs_record(pairs), (1, answer), format!("corpus declarations {i}: {pairs:?}"));
    }
    // Every keyframe value against the next of the same property, along the way.
    let mut rules = KeyframeRules::new();
    {
        let mut timings = TimingCache::default();
        for sheet in &animated {
            parse_keyframes(&mut timings, sheet, &mut rules);
        }
    }
    let mut names: Vec<&Vec<u8>> = rules.keys().collect();
    names.sort();
    for name in &names {
        let frames = &rules[*name];
        for w in frames.windows(2) {
            for (p, a) in &w[0].declarations {
                if let Some((_, b)) = w[1].declarations.iter().find(|(q, _)| q == p) {
                    for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
                        let answer = run_interpolate(&caches, a, b, t, &mut tally);
                        c.case(
                            &interpolate_record(a, b, t),
                            (1, answer),
                            format!("corpus pair {:?} {:?}", String::from_utf8_lossy(a), String::from_utf8_lossy(b)),
                        );
                    }
                }
            }
        }
    }
    // The corpus's sheets and the declarations of its rules on elements, built and sampled over time.
    for i in 0..1500 {
        let sheet = animated[random.below(animated.len())].clone();
        let ids = [1i64, 2, 3];
        let mut steps = vec![Step::Sheets(vec![sheet])];
        let mut now = random.below(1000) as f64 / 100.0;
        let mut elements: Vec<(i64, Pairs)> = ids.iter().map(|&id| (id, declarations[random.below(declarations.len())].clone())).collect();
        steps.push(Step::Update(now, elements.clone()));
        for _ in 0..4 + random.below(12) {
            match random.below(8) {
                0 | 1 => {
                    let k = random.below(elements.len());
                    elements[k].1 = declarations[random.below(declarations.len())].clone();
                    steps.push(Step::Update(now, elements.clone()));
                }
                2 => steps.push(Step::Plan(*pick(&mut random, &ids), now, true)),
                3 => steps.push(Step::Heavy(now)),
                4 => steps.push(Step::Query(ids.to_vec(), now)),
                _ => {
                    now = now_after(&mut random, now);
                    steps.push(Step::Sample(now));
                }
            }
        }
        for &id in &ids {
            steps.push(Step::Signature(id));
            steps.push(Step::Peek(id, now + 0.05));
        }
        script_case(&mut c, &caches, &steps, &mut tally, format!("corpus script {i}"));
    }
    let n = c.len();
    c.check(&oracle);
    report(&format!("corpus ({} sheets with keyframes, {} declarations)", animated.len(), declarations.len()), n, &tally);
}

/// A Web Animation over `property`, as a JSON record.
fn composite_record(aid: usize, target: i64, property: &str, frames: &[(f64, &str, &str)], timing: &str, rest: &str) -> String {
    let frames: Vec<String> = frames
        .iter()
        .map(|(offset, value, composite)| {
            let composite = if composite.is_empty() { String::new() } else { format!(",\"composite\":\"{composite}\"") };
            format!(
                "{{\"offset\":{offset}{composite},\"values\":{{\"{property}\":{}}}}}",
                String::from_utf8_lossy(&twidget_json::quote(value))
            )
        })
        .collect();
    format!("{{\"aid\":{aid},\"target\":{target},\"keyframes\":[{}],\"timing\":{{{timing}}}{rest}}}", frames.join(","))
}

#[test]
fn composites_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_000b);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    let properties = ["transform", "transform", "filter", "color", "opacity", "width", "box-shadow", "translate"];
    for i in 0..3000 {
        let property = *pick(&mut random, &properties);
        let value = |r: &mut Lcg| if property == "transform" { scale_transform(r) } else { value_of(r, property) };
        let mut steps = vec![Step::Underlying(vec![(1, vec![(property.as_bytes().to_vec(), value(&mut random).into_bytes())])])];
        if random.chance(3) {
            steps.push(Step::Update(0.0, vec![(1, vec![(property.as_bytes().to_vec(), value(&mut random).into_bytes())])]));
        }
        let mut records = Vec::new();
        for aid in 1..=1 + random.below(3) {
            let n = 1 + random.below(3);
            let values: Vec<String> = (0..n).map(|_| value(&mut random)).collect();
            let mut frames = Vec::new();
            for (k, v) in values.iter().enumerate() {
                let offset = if n == 1 { *pick(&mut random, &[0.0, 0.5, 1.0]) } else { k as f64 / (n - 1) as f64 };
                frames.push((offset, v.as_str(), *pick(&mut random, &["", "", "add", "accumulate", "replace"])));
            }
            let easing = timing_text(&mut random);
            let timing = format!(
                "\"duration\":{},\"iterations\":{},\"direction\":\"{}\",\"fill\":\"{}\",\"easing\":{},\"delay\":{},\"endDelay\":{},\"iterationStart\":{}",
                pick(&mut random, &["1000", "250", "0"]),
                pick(&mut random, &["3", "2.5", "null", "1"]),
                pick(&mut random, &["normal", "reverse", "alternate", "alternate-reverse"]),
                pick(&mut random, &["both", "forwards", "backwards", "none"]),
                String::from_utf8_lossy(&twidget_json::quote(&easing)),
                pick(&mut random, &["0", "100", "-300"]),
                pick(&mut random, &["0", "-200", "500"]),
                pick(&mut random, &["0", "0.5", "1"]),
            );
            let rest = format!(
                ",\"composite\":\"{}\",\"iterationComposite\":\"{}\",\"startTime\":0,\"playbackRate\":{}",
                pick(&mut random, &["replace", "add", "accumulate"]),
                pick(&mut random, &["accumulate", "accumulate", "replace"]),
                pick(&mut random, &["1", "2", "-1", "0.5"]),
            );
            records.push(composite_record(aid, 1, property, &frames, &timing, &rest));
        }
        steps.push(Step::Web(format!("{{\"type\":\"anim\",\"now\":0,\"anims\":[{}]}}", records.join(",")).into_bytes(), 0.0));
        for k in 0..8 {
            let t = f64::from(k) * 0.37 + random.below(100) as f64 / 1000.0;
            steps.push(Step::Sample(t));
            if k % 3 == 0 {
                steps.push(Step::Peek(1, t));
                steps.push(Step::Plan(1, t, true));
            }
        }
        steps.push(Step::Heavy(0.5));
        script_case(&mut c, &caches, &steps, &mut tally, format!("composite script {i}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("composites", n, &tally);
}

#[test]
fn edges_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    // linear() of as many points as it takes, and one more.
    for n in [256usize, 257, 10000, 10001] {
        let text = format!("linear({})", (0..n).map(|i| format!("{}", i % 3)).collect::<Vec<_>>().join(","));
        let answer = run_timing(&caches, text.as_bytes(), &XS, &mut tally);
        c.case(&timing_record(text.as_bytes(), &XS), (1, answer), format!("linear() of {n} points"));
        let record = format!("{{\"aid\":1,\"target\":1,\"keyframes\":[],\"timing\":{{\"easing\":\"{text}\"}},\"startTime\":0}}");
        let answer = run_web(&caches, record.as_bytes(), 0.0, &mut tally);
        c.case(&web_record(record.as_bytes(), 0.0), (1, answer), format!("an effect eased by linear() of {n} points"));
    }
    // Records that are no Web Animation, and the numbers at the ends of what one may hold.
    let frame = "{\"offset\":0,\"values\":{\"opacity\":\"0\"}}";
    let many = |n: usize| vec![frame; n].join(",");
    let values = |n: usize| (0..n).map(|i| format!("\"p{i:02}\":\"{i}\"")).collect::<Vec<_>>().join(",");
    let records = [
        "{\"aid\":\"1\",\"target\":1,\"keyframes\":[],\"startTime\":0}".to_string(),
        "{\"aid\":1,\"keyframes\":[],\"startTime\":0}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":{},\"startTime\":0}".to_string(),
        format!("{{\"aid\":1,\"target\":1,\"keyframes\":[{}],\"startTime\":0}}", many(64)),
        format!("{{\"aid\":1,\"target\":1,\"keyframes\":[{}],\"startTime\":0}}", many(65)),
        format!("{{\"aid\":1,\"target\":1,\"keyframes\":[{{\"offset\":0,\"values\":{{{}}}}}],\"startTime\":0}}", values(64)),
        format!("{{\"aid\":1,\"target\":1,\"keyframes\":[{{\"offset\":0,\"values\":{{{}}}}}],\"startTime\":0}}", values(65)),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":1e400}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":1e308,\"holdTime\":1e400}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"holdTime\":1e12}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"holdTime\":-1.0000001e12}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"playbackRate\":1e400}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"playbackRate\":-1000}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"duration\":86400000,\"delay\":-86400000,\"endDelay\":86400001}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"duration\":1e400}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"iterations\":1e9,\"iterationStart\":1e9}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"iterations\":1000000001}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"easing\":\"cubic-bezier(1.1,0,0,1)\"}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"easing\":\"steps(1, jump-none)\"}}".to_string(),
        "{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{\"easing\":\"\"}}".to_string(),
        format!("{{\"aid\":1,\"target\":1,\"keyframes\":[],\"startTime\":0,\"timing\":{{\"easing\":\"{}\"}}}}", "x".repeat(4097)),
        "{\"aid\":1e300,\"target\":-1e300,\"keyframes\":[{\"offset\":1,\"composite\":7,\"values\":{\"opacity\":1,\"transform\":\"none\"}}],\"startTime\":-0}".to_string(),
    ];
    for (i, record) in records.iter().enumerate() {
        for offset in [0.0, 1e308, -5.5] {
            let answer = run_web(&caches, record.as_bytes(), offset, &mut tally);
            c.case(&web_record(record.as_bytes(), offset), (1, answer), format!("edge record {i} at {offset}"));
        }
    }
    // Messages: items that are no record, overrides past the cap, animations past the cap, and past 4096 items.
    let mut messages: Vec<String> = vec![
        concat!(
            "{\"anims\":[{\"target\":1},{\"aid\":\"1\"},",
            "{\"aid\":1,\"target\":\"1\",\"css\":{\"kind\":\"animation\",\"name\":\"spin\",\"origin\":0},\"startTime\":0},",
            "{\"aid\":1,\"target\":1,\"css\":{\"kind\":\"animation\",\"name\":7,\"origin\":0},\"startTime\":0},",
            "{\"aid\":1,\"target\":1,\"css\":{\"kind\":\"animation\",\"name\":\"spin\"},\"startTime\":0},",
            "{\"aid\":1,\"target\":1,\"css\":{\"kind\":1,\"name\":\"spin\",\"origin\":0},\"startTime\":0},",
            "{\"aid\":1,\"target\":1,\"css\":[],\"keyframes\":[],\"startTime\":0}]}"
        )
        .to_string(),
        concat!(
            "{\"anims\":[{\"aid\":1,\"target\":1,\"css\":{\"kind\":\"animation\",\"name\":\"spin\",\"origin\":0},\"startTime\":1.7e308},",
            "{\"aid\":2,\"target\":1,\"css\":{\"kind\":\"transition\",\"name\":\"opacity\",\"origin\":0},\"holdTime\":1e400},",
            "{\"aid\":3,\"target\":1,\"css\":{\"kind\":\"animation\",\"name\":\"k\",\"origin\":0},\"playbackRate\":1e400,\"startTime\":0}]}"
        )
        .to_string(),
    ];
    let overrides: Vec<String> = (0..1100)
        .map(|i| format!("{{\"aid\":{i},\"target\":{i},\"css\":{{\"kind\":\"animation\",\"name\":\"k\",\"origin\":0}},\"holdTime\":{i}}}"))
        .collect();
    messages.push(format!(
        "{{\"anims\":[{},{{\"aid\":1,\"target\":5,\"css\":{{\"kind\":\"transition\",\"name\":\"opacity\",\"origin\":0}},\"startTime\":0}}]}}",
        overrides.join(",")
    ));
    let effects: Vec<String> = (0..1100)
        .map(|i| {
            format!(
                "{{\"aid\":{i},\"target\":{},\"keyframes\":[{{\"offset\":0,\"values\":{{\"opacity\":\"0.{}\"}}}}],\"timing\":{{\"duration\":1000}},\"startTime\":0}}",
                i % 9,
                i % 10
            )
        })
        .collect();
    messages.push(format!("{{\"anims\":[{},{}]}}", effects.join(","), effects[3].replace("\"0.3\"", "\"0.75\"")));
    let flood: Vec<String> = (0..4200).map(|i| format!("{{\"aid\":{},\"target\":2,\"keyframes\":[],\"startTime\":0}}", 5000 + i)).collect();
    messages.push(format!("{{\"anims\":[{}]}}", flood.join(",")));
    let sheet = b"@keyframes k{to{opacity:0}} @keyframes spin{to{transform:rotate(1turn)}}".to_vec();
    for (i, m) in messages.iter().enumerate() {
        let mut steps = vec![
            Step::Sheets(vec![sheet.clone()]),
            Step::Update(
                0.0,
                vec![(
                    1,
                    vec![(b"animation".to_vec(), b"k 1s infinite, spin 2s".to_vec()), (b"transition".to_vec(), b"opacity 1s".to_vec())],
                )],
            ),
            Step::Web(m.clone().into_bytes(), 3.0),
        ];
        for id in [1, 2, 5, 8] {
            steps.extend([Step::Signature(id), Step::Plan(id, 0.5, true), Step::Peek(id, 0.5), Step::Query(vec![id], 0.5)]);
        }
        steps.extend([Step::Sample(0.5), Step::Heavy(0.5), Step::Ids(0.5), Step::WebTargets(2), Step::Active(0.5, vec![1])]);
        script_case(&mut c, &caches, &steps, &mut tally, format!("edge message {i}"));
    }
    // Clocks far from zero: local times that are no longer finite, sums that overflow.
    let effect = concat!(
        "{\"anims\":[{\"aid\":1,\"target\":1,\"keyframes\":[{\"offset\":0,\"values\":{\"opacity\":\"0\"}}],\"timing\":{\"duration\":1000,\"iterations\":null},",
        "\"startTime\":-1e308,\"playbackRate\":1000},",
        "{\"aid\":2,\"target\":1,\"keyframes\":[{\"offset\":1,\"values\":{\"transform\":\"scale(2)\"}}],\"timing\":{\"duration\":500,\"fill\":\"both\"},",
        "\"startTime\":1e308,\"playbackRate\":-1000},",
        "{\"aid\":3,\"target\":1,\"css\":{\"kind\":\"animation\",\"name\":\"k\",\"origin\":0},\"startTime\":-1.7e308,\"playbackRate\":1000}]}"
    );
    for (i, now) in [1e306, 1.7e308, -1.7e308, 1e300, 4e15, -0.0, 5e-324].into_iter().enumerate() {
        let decl = vec![
            (b"animation".to_vec(), b"k 1s infinite alternate both, spin 1e-9s 1e300".to_vec()),
            (b"transition".to_vec(), b"all 1s".to_vec()),
            (b"opacity".to_vec(), b"0.5".to_vec()),
        ];
        let mut steps = vec![
            Step::Sheets(vec![sheet.clone()]),
            Step::Update(0.0, vec![(1, decl.clone())]),
            Step::Update(now, vec![(1, vec![decl[0].clone(), decl[1].clone(), (b"opacity".to_vec(), b"1".to_vec())])]),
            Step::Web(effect.as_bytes().to_vec(), 0.0),
            Step::Web(effect.as_bytes().to_vec(), 1.7e308),
        ];
        for t in [now, now * 0.5, 0.0, -now] {
            steps.extend([
                Step::Sample(t),
                Step::Peek(1, t),
                Step::Plan(1, t, true),
                Step::Heavy(t),
                Step::Active(t, vec![]),
                Step::Ids(t),
                Step::Query(vec![1], t),
            ]);
        }
        steps.push(Step::Signature(1));
        script_case(&mut c, &caches, &steps, &mut tally, format!("far clock {i}: {now}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("edges", n, &tally);
}

#[test]
fn css_overrides_alike() {
    let Some(oracle) = Oracle::find("animations_oracle") else { return };
    let caches = shared();
    let mut random = Lcg::new(0xa1_000c);
    let mut tally = Tally::default();
    let mut c = Comparison::new();
    let sheet = b"@keyframes k{to{opacity:0}} @keyframes blur{from{filter:blur(0)}to{filter:blur(4px)}} @keyframes spin{to{transform:rotate(1turn)}}".to_vec();
    for i in 0..3000 {
        // Script control (getAnimations(), pause(), currentTime, playbackRate, cancel()) of what the element runs.
        let start = random.below(400) as f64 / 100.0;
        let animation =
            pick(&mut random, &["k 1s infinite", "blur 2s 3", "spin 500ms linear infinite alternate", "k 1s 200ms both, blur 1s"]);
        let transition = pick(&mut random, &["opacity 1s", "filter 2s linear", "all 500ms 100ms", "opacity 1s, filter 3s"]);
        let decl = |opacity: &str, filter: &str| {
            vec![
                (b"animation".to_vec(), animation.as_bytes().to_vec()),
                (b"transition".to_vec(), transition.as_bytes().to_vec()),
                (b"opacity".to_vec(), opacity.as_bytes().to_vec()),
                (b"filter".to_vec(), filter.as_bytes().to_vec()),
            ]
        };
        let mut steps = vec![
            Step::Sheets(vec![sheet.clone()]),
            Step::Update(start - 0.5, vec![(1, decl("1", "none")), (2, decl("1", "none"))]),
            Step::Update(start, vec![(1, decl("0", "blur(2px)")), (2, decl("0.5", "none"))]),
        ];
        let mut now = start;
        for _ in 0..1 + random.below(4) {
            let mut items = Vec::new();
            for _ in 0..1 + random.below(4) {
                let (kind, name) = *pick(
                    &mut random,
                    &[
                        ("transition", "opacity"),
                        ("transition", "filter"),
                        ("animation", "k"),
                        ("animation", "blur"),
                        ("animation", "spin"),
                    ],
                );
                let origin = if random.chance(5) { (start + 1.0) * 1000.0 } else { start * 1000.0 };
                let mut item = format!(
                    "{{\"aid\":{},\"target\":{},\"css\":{{\"kind\":\"{kind}\",\"name\":\"{name}\",\"origin\":{origin}}}",
                    random.below(9),
                    pick(&mut random, &[1, 1, 2, 3])
                );
                match random.below(6) {
                    0 => item.push_str(",\"cancelled\":true"),
                    1 => item.push_str(&format!(",\"holdTime\":{}", random.below(3000))),
                    2 => item.push_str(",\"remove\":true"),
                    _ => item.push_str(&format!(",\"startTime\":{}", now * 1000.0 - random.below(1500) as f64)),
                }
                if random.chance(2) {
                    item.push_str(&format!(",\"playbackRate\":{}", pick(&mut random, &["2", "0.5", "-1", "0", "1"])));
                }
                item.push('}');
                items.push(item);
            }
            steps.push(Step::Web(format!("{{\"type\":\"anim\",\"now\":0,\"anims\":[{}]}}", items.join(",")).into_bytes(), 0.0));
            for _ in 0..1 + random.below(3) {
                now += random.below(700) as f64 / 1000.0;
                let id = *pick(&mut random, &[1i64, 2]);
                match random.below(7) {
                    0 => steps.push(Step::Update(now, vec![(1, decl("0", "blur(2px)")), (2, decl("0.5", "none"))])),
                    1 => steps.push(Step::Plan(id, now, random.chance(2))),
                    2 => steps.push(Step::Heavy(now)),
                    3 => steps.push(Step::Query(vec![1, 2], now)),
                    4 => steps.push(Step::Peek(id, now)),
                    5 => steps.push(Step::Ids(now)),
                    _ => steps.push(Step::Sample(now)),
                }
            }
        }
        for id in [1, 2] {
            steps.extend([Step::Signature(id), Step::Plan(id, now, true), Step::Query(vec![id], now), Step::Peek(id, now)]);
        }
        steps.extend([Step::Heavy(now), Step::Active(now, vec![2]), Step::Sample(now)]);
        script_case(&mut c, &caches, &steps, &mut tally, format!("override script {i}"));
    }
    let n = c.len();
    c.check(&oracle);
    report("css overrides", n, &tally);
}
