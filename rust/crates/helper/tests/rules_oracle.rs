//! The renderer rules against tdesktop's (`oracle/renderer_rules_oracle`, tgw_renderer_rules.h over a seam renderer
//! whose build counts and clock the cases set): ratios, slops and pointer targets on generated and edge numbers,
//! generated renderer messages through every predicate and the local query, load sequences, context menus taken
//! from generated lists, settled lists, canvas spelling scripts past their limits, frame messages and the constants.
//! Skipped without `TWIDGET_ORACLE_DIR` or without tdesktop's sources.

mod common;

use common::*;
use twidget_helper::rules::{self, CanvasSpellings};
use twidget_json::Json;
use twidget_renderer::BuildStats;

const TYPES: [&[u8]; 7] = [b"ops", b"query", b"event", b"size", b"result", b"frame", b""];

const EDGES: [f64; 16] = [
    0.0,
    -0.0,
    0.25,
    0.2499999,
    64.0,
    64.0000001,
    1.0,
    -1.0,
    1e-310,
    1e308,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
    10.0,
    7.0710678118654755,
    3.5,
];

fn any_double(random: &mut Lcg) -> f64 {
    match random.below(4) {
        0 => EDGES[random.below(EDGES.len())],
        1 => f64::from_bits((u64::from(random.next()) << 32) | u64::from(random.next())),
        2 => (random.below(20001) as f64 - 10000.0) / 100.0,
        _ => random.below(100) as f64 * 0.25,
    }
}

fn pick<'a>(random: &mut Lcg, choices: &[&'a str]) -> &'a str {
    choices[random.below(choices.len())]
}

/// A renderer message as text, mostly well formed, of the kinds the rules look at and with their fields of every
/// type.
fn any_message(random: &mut Lcg) -> String {
    if random.chance(12) {
        return pick(random, &["[]", "5", "\"ops\"", "null", "{", "{\"type\":}", "[{\"type\":\"ops\"}]", "true"]).to_string();
    }
    let mut fields: Vec<String> = Vec::new();
    let ty = pick(
        random,
        &[
            "\"ops\"",
            "\"query\"",
            "\"event\"",
            "\"size\"",
            "\"result\"",
            "\"frame\"",
            "\"ready\"",
            "\"OPS\"",
            "5",
            "null",
            "[\"ops\"]",
            "\"\"",
        ],
    );
    if !random.chance(10) {
        fields.push(format!("\"type\":{ty}"));
    }
    if random.chance(2) {
        let ops = pick(random, &["[]", "[{\"op\":\"el\",\"id\":1}]", "[1,2]", "\"x\"", "{}", "null", "[[]]"]);
        fields.push(format!("\"ops\":{ops}"));
    }
    if random.chance(2) {
        let qid =
            pick(random, &["-1", "-1.0", "-1.5", "-0.5", "-2", "0", "1e10", "-1e10", "\"-1\"", "null", "true", "-1e-300", "-0.99999"]);
        fields.push(format!("\"qid\":{qid}"));
    }
    if random.chance(2) {
        let ev_type = pick(random, &["\"contextmenu\"", "\"click\"", "\"Click\"", "\"pointerdown\"", "5", "null"]);
        let pointer = pick(random, &["\"mouse\"", "\"touch\"", "\"\"", "\"pen\"", "null", "1"]);
        let ev = match random.below(4) {
            0 => format!("{{\"type\":{ev_type}}}"),
            1 => format!("{{\"pointerType\":{pointer},\"type\":{ev_type},\"x\":3.25}}"),
            2 => pick(random, &["[]", "\"contextmenu\"", "null"]).to_string(),
            _ => format!("{{\"type\":{ev_type},\"pointerType\":{pointer}}}"),
        };
        fields.push(format!("\"ev\":{ev}"));
    }
    if random.chance(4) {
        fields.push(format!("\"type\":{}", pick(random, &["\"event\"", "\"ops\""])));
    }
    if random.chance(3) {
        fields.push(format!(
            "\"result\":{}",
            pick(random, &["{\"x\":1}", "[0.1,2e21,-3]", "\"a\\u00e9\\ud83d\\ude00\"", "1.7976931348623157e308"])
        ));
    }
    for i in (1..fields.len()).rev() {
        let j = random.below(i + 1);
        fields.swap(i, j);
    }
    format!("{{{}}}", fields.join(","))
}

fn parsed(text: &[u8]) -> Json {
    Json::parse(text).unwrap_or(Json::Null)
}

fn put_list(out: &mut Out, list: &[Json]) {
    out.u32(list.len() as u32);
    for m in list {
        out.str(&m.dump());
    }
}

fn list_record(record: &mut Out, texts: &[String]) {
    record.u32(texts.len() as u32);
    for t in texts {
        record.str(t.as_bytes());
    }
}

#[test]
fn numbers_alike() {
    let Some(oracle) = helper_oracle("renderer_rules_oracle") else { return };
    let mut random = Lcg::new(0x5255_0001);
    let mut c = Comparison::new();
    for i in 0..20000 {
        let ratio = any_double(&mut random);
        c.case(
            &framed(b'r', &ratio.to_bits().to_le_bytes()),
            (1, Out::default().f64(rules::device_ratio(ratio)).0.clone()),
            format!("ratio {i}"),
        );
        let (dx, dy) = (any_double(&mut random), any_double(&mut random));
        let mut record = Out::default();
        record.f64(dx).f64(dy);
        c.case(&framed(b's', &record.0), (1, vec![u8::from(rules::outside_slop(dx, dy))]), format!("slop {i}"));
        let ids = [0i64, 1, 2, 7, -1, i64::MAX, i64::MIN];
        let (hit, press_hit) = (ids[random.below(ids.len())], ids[random.below(ids.len())]);
        let (held, synced, active) = (random.chance(2), random.chance(2), random.chance(2));
        let mut record = Out::default();
        record.i64(hit).flag(held).flag(synced).i64(press_hit).flag(active);
        let targets = rules::pointer_targets(hit, held, synced, press_hit, active);
        c.case(&framed(b'p', &record.0), (1, Out::default().i64(targets.hover).i64(targets.active).0.clone()), format!("targets {i}"));
        let builds = random.below(1 << 20) as i32;
        let patched = random.below(builds as usize + 1) as i32;
        let mut record = Out::default();
        record.i32(builds).i32(patched);
        let stats = BuildStats { builds, patched, ..BuildStats::default() };
        c.case(&framed(b'q', &record.0), (1, Out::default().i32(rules::parsed_documents(&stats)).0.clone()), format!("parsed {i}"));
        let clock = if random.chance(2) { random.below(1 << 30) as f64 / 1000.0 } else { any_double(&mut random) };
        c.case(
            &framed(b'f', &clock.to_bits().to_le_bytes()),
            (1, Out::default().str(&rules::frame_message(clock).dump()).0.clone()),
            format!("frame {i}"),
        );
    }
    let cases = c.len();
    c.check(&oracle);
    eprintln!("rules: {cases} ratios, slops, targets, parse counts and frame messages alike");
}

#[test]
fn messages_alike() {
    let Some(oracle) = helper_oracle("renderer_rules_oracle") else { return };
    let mut random = Lcg::new(0x5255_0002);
    let mut c = Comparison::new();
    for i in 0..30000 {
        let text = any_message(&mut random);
        let answer = match Json::parse(text.as_bytes()) {
            None => (0, Vec::new()),
            Some(message) => {
                let mut out = Out::default();
                out.flag(rules::kept_before_ready(&message));
                out.flag(rules::draws_content(&message));
                out.flag(rules::is_load_ack(&message));
                out.flag(rules::is_context_menu(&message, b"mouse"));
                out.flag(rules::is_context_menu(&message, b"touch"));
                out.flag(rules::is_context_menu(&message, b""));
                out.flag(rules::is_click(&message));
                for ty in TYPES {
                    out.flag(rules::of_type(&message, ty));
                }
                out.str(&rules::local_query(message).dump());
                (1, out.0)
            }
        };
        c.case(&framed(b'j', &Out::default().str(text.as_bytes()).0), answer, format!("message {i}: {text}"));
    }
    let cases = c.len();
    c.check(&oracle);
    eprintln!("rules: {cases} generated messages alike");
}

#[test]
fn lists_alike() {
    let Some(oracle) = helper_oracle("renderer_rules_oracle") else { return };
    let mut random = Lcg::new(0x5255_0003);
    let mut c = Comparison::new();
    for i in 0..8000 {
        let n = random.below(10);
        let texts: Vec<String> = (0..n).map(|_| any_message(&mut random)).collect();
        let messages: Vec<Json> = texts.iter().map(|t| parsed(t.as_bytes())).collect();
        let (mut content, mut acked, mut loaded) = (random.chance(4), random.chance(4), random.chance(6));
        let mut record = Out::default();
        record.flag(content).flag(acked).flag(loaded);
        list_record(&mut record, &texts);
        let mut out = Out::default();
        for m in &messages {
            let change = rules::advance_load(m, &mut content, &mut acked, &mut loaded);
            out.flag(change.content).flag(change.loaded).flag(content).flag(acked).flag(loaded);
        }
        c.case(&framed(b'a', &record.0), (1, out.0), format!("load {i}"));
        let pointer: &[u8] = [&b"mouse"[..], b"touch", b"", b"pen"][random.below(4)];
        let mut record = Out::default();
        record.str(pointer);
        list_record(&mut record, &texts);
        let mut kept = messages.clone();
        let menus = rules::take_context_menus(&mut kept, pointer);
        let mut out = Out::default();
        put_list(&mut out, &menus);
        put_list(&mut out, &kept);
        c.case(&framed(b'm', &record.0), (1, out.0), format!("menus {i}"));
        let mut record = Out::default();
        list_record(&mut record, &texts);
        let mut asked = Vec::new();
        let settled = rules::take_settled(|query| {
            asked = query.dump();
            messages.clone()
        });
        let mut out = Out::default();
        out.str(&asked);
        put_list(&mut out, &settled);
        c.case(&framed(b't', &record.0), (1, out.0), format!("settled {i}"));
    }
    let cases = c.len();
    c.check(&oracle);
    eprintln!("rules: {cases} load sequences, menu and settled lists alike");
}

#[test]
fn spellings_and_constants_alike() {
    let Some(oracle) = helper_oracle("renderer_rules_oracle") else { return };
    let mut random = Lcg::new(0x5255_0004);
    let mut c = Comparison::new();
    for i in 0..600 {
        let mut spellings = CanvasSpellings::<u32>::default();
        let mut parses = 0u32;
        let pool = 1 + random.below(140);
        let steps = random.below(400);
        let mut record = Out::default();
        record.u32(steps as u32);
        let mut out = Out::default();
        for _ in 0..steps {
            let refill = random.chance(40);
            let css: Vec<u8> = match random.below(12) {
                0 => vec![b'f'; rules::MAX_CANVAS_FONT_SPELLING_SIZE - 1 + random.below(3)],
                _ => format!("{}px f{}", 10 + random.below(4), random.below(pool)).into_bytes(),
            };
            record.flag(refill).str(&css);
            if refill {
                spellings.refill();
                continue;
            }
            let mut was_parsed = false;
            let entry = spellings
                .find(&css, |text| {
                    was_parsed = true;
                    parses += 1;
                    parses + text.len() as u32 * 1000
                })
                .copied();
            out.flag(entry.is_some()).flag(was_parsed).u32(entry.unwrap_or(0));
        }
        c.case(&framed(b'c', &record.0), (1, out.0), format!("spellings {i}"));
    }
    let mut out = Out::default();
    out.str(rules::READY_MESSAGE).str(rules::canvas_font_limit_text().as_bytes()).str(rules::CANVAS_FONT_LIMIT_KIND);
    out.f64(rules::MIN_RATIO).f64(rules::MAX_RATIO);
    for value in [
        rules::TOUCH_DOWN,
        rules::TOUCH_MOVE,
        rules::TOUCH_UP,
        rules::TOUCH_CANCEL,
        rules::HOVER_MOVE,
        rules::HOVER_LEAVE,
        rules::MOUSE_POINTER,
        rules::FINGER_POINTER,
        rules::PRIMARY_BUTTON,
        rules::SECONDARY_BUTTON,
        rules::LOCAL_QUERY_ID,
        rules::LOAD_ACK_QUERY_ID,
        rules::MAX_NEW_CANVAS_FONTS,
        rules::MAX_CANVAS_FONTS,
        rules::MAX_CANVAS_FONT_PARSES,
        rules::MAX_CANVAS_FONT_REQUESTS,
    ] {
        out.i32(value);
    }
    out.u64(rules::MAX_CANVAS_FONT_SPELLINGS as u64).u64(rules::MAX_CANVAS_FONT_SPELLING_SIZE as u64);
    out.str(&rules::viewport_query().dump());
    c.case(&framed(b'x', &[]), (1, out.0), "constants");
    let cases = c.len();
    c.check(&oracle);
    eprintln!("rules: {cases} spelling scripts and the constants alike");
}
