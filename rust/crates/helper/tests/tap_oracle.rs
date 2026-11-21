//! The Debug tap against tdesktop's (`oracle/helper_tap_oracle`: test_widget_helper_tap.cpp with its `_DEBUG` half
//! on and its steady clock the oracle's, over test_widget_renderer_forward.h): written and generated sessions of
//! platform messages (the arming controls, frame requests of every paint, other controls and bytes), renderer calls
//! and what the wrapped renderer does inside them (messages and diagnostics to the host, which calls back in, platform
//! sends of frames and others, requests answered or not, panics), every transcript alike byte for byte: the mirrored
//! records with their work records and stamps, the forwarding and its order. Skipped without `TWIDGET_ORACLE_DIR` or
//! without tdesktop's sources.

mod common;

use common::tap::{Action, Session, Step};
use common::*;
use twidget_helper::wire;
use twidget_remote::{FrameRequest, Message, Paint, Touch};

/// The scripted panics say nothing; every other panic as before.
fn quiet_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<&str>() != Some(&"scripted") {
                default(info);
            }
        }));
    });
}

fn call(op: u8, text: &[u8], actions: Vec<Vec<Action>>) -> Step {
    Step::Call { op, text: text.to_vec(), width: 0.0, actions }
}

fn act(kind: u8, first: &[u8]) -> Action {
    Action { kind, first: first.to_vec(), second: b"detail".to_vec() }
}

fn frame_request(seq: u64, tick: bool, paint: Paint) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(FrameRequest { seq, tick, paint }.encode(&wire::embedding_caps(), &mut out));
    out
}

/// A remote message of `kind` with version 1 and `n` bytes in all.
fn remote(kind: u8, n: usize) -> Vec<u8> {
    let mut out = vec![kind, 1];
    out.extend((0..n.saturating_sub(2)).map(|i| i as u8));
    out
}

fn any_platform_message(random: &mut Lcg) -> Vec<u8> {
    match random.below(14) {
        0 | 1 => wire::encode_mirror(),
        2 => wire::encode_mirror_work(),
        3 => [wire::encode_mirror(), vec![0]].concat(),
        4..=6 => frame_request(random.next().into(), random.chance(2), [Paint::Skip, Paint::IfDirty, Paint::Force][random.below(3)]),
        7 => {
            let full = frame_request(5, true, Paint::Force);
            full[..random.below(full.len())].to_vec()
        }
        8 => {
            let mut out = Vec::new();
            let touch = Touch { action: 0, x: 3.0, y: 4.0, pointer: 1, primary: true, mouse: true, modifiers: 0, button: 0 };
            assert!(touch.encode(&wire::embedding_caps(), &mut out));
            out
        }
        9 => wire::encode_open(2.0),
        10 => wire::encode_finger_hold(random.next().into()),
        11 => Vec::new(),
        _ => (0..random.below(12)).map(|_| random.next() as u8).collect(),
    }
}

fn any_host_message(random: &mut Lcg) -> Vec<u8> {
    let choices: [&[u8]; 7] = [
        br#"{"type":"ops","ops":[]}"#,
        br#"{"type":"ops""#,
        br#"{"type":"query","qid":3,"query":{"q":"box"}}"#,
        br#"{"type":"theme"}"#,
        br#"{"type":"frozen"}"#,
        b"",
        b"not json",
    ];
    choices[random.below(choices.len())].to_vec()
}

fn any_actions(random: &mut Lcg) -> Vec<Vec<Action>> {
    let lists = random.below(4);
    (0..lists)
        .map(|_| {
            let n = random.below(5);
            (0..n)
                .map(|_| match random.below(30) {
                    0..=7 => {
                        let texts: [&[u8]; 6] = [
                            br#"{"type":"size","h":5}"#,
                            br#"{"type":"ready"}"#,
                            br#"@again:{"type":"query","qid":0}"#,
                            br#"@again:{"type":"ops","ops":[1]}"#,
                            b"@theme:dark",
                            b"@theme:light",
                        ];
                        act(0, texts[random.below(texts.len())])
                    }
                    8 | 9 => act(1, b"renderer-violation"),
                    10..=19 => {
                        let n = random.below(300);
                        let kind = [64u8, 73, 65, 66, 70][random.below(5)];
                        act(2, &remote(kind, n))
                    }
                    20..=28 => act(3, if random.chance(3) { b"X refused" } else { b"R a font" }),
                    _ => act(4, b""),
                })
                .collect()
        })
        .collect()
}

fn any_session(random: &mut Lcg) -> Vec<Step> {
    let schemes: [&[u8]; 4] = [b"light", b"dark", b"", b"sepia"];
    let mut steps = vec![Step::Begin {
        platform: !random.chance(10),
        renderer: !random.chance(10),
        scheme: schemes[random.below(schemes.len())].to_vec(),
    }];
    let n = 5 + random.below(36);
    for _ in 0..n {
        let actions = any_actions(random);
        let step = match random.below(20) {
            0..=7 => call(b'P', &any_platform_message(random), actions),
            8..=11 => call(b'S', &any_host_message(random), actions),
            12 | 13 => call(b'T', schemes[random.below(schemes.len())], actions),
            14 => Step::Call { op: b'W', text: Vec::new(), width: random.below(1000) as f64 / 3.0, actions },
            15 => call(b'L', b"", actions),
            16 => call(b'D', b"", actions),
            17 if random.chance(3) => call(b'E', b"", actions),
            _ => call(b'P', &wire::encode_mirror_work(), actions),
        };
        steps.push(step);
    }
    steps
}

/// Steps, transcript bytes, then the mirrored records of ways b, t, h, r and w the transcripts hold.
type Tally = [usize; 7];

fn count_records(transcript: &[u8], tally: &mut Tally) {
    for (k, way) in [b'b', b't', b'h', b'r', b'w'].iter().enumerate() {
        tally[2 + k] += transcript.windows(3).filter(|w| w[0] == wire::TAG && w[1] == wire::Kind::Mirrored as u8 && w[2] == *way).count();
    }
}

fn compare(c: &mut Comparison, session: &[Step], name: &str, tally: &mut Tally) {
    let mut driver = Session::default();
    for (i, step) in session.iter().enumerate() {
        let transcript = driver.run(step);
        tally[0] += 1;
        tally[1] += transcript.len();
        count_records(&transcript, tally);
        let shown = match step {
            Step::Call { op, text, .. } if text.len() > 200 => format!("{} with {} bytes", *op as char, text.len()),
            _ => format!("{step:?}"),
        };
        c.case(&step.record(), (1, transcript), format!("{name}, step {i}: {shown}"));
    }
    c.case(&call(b'E', b"", Vec::new()).record(), (1, driver.run(&call(b'E', b"", Vec::new()))), format!("{name}, the end"));
}

fn written() -> Vec<(&'static str, Vec<Step>)> {
    let begin = |platform, renderer, scheme: &[u8]| Step::Begin { platform, renderer, scheme: scheme.to_vec() };
    let ready = vec![vec![act(0, br#"{"type":"ready"}"#)]];
    let painting = vec![vec![act(2, &remote(64, 40)), act(3, b"R width"), act(2, &remote(73, 20)), act(2, &remote(65, 9))]];
    vec![
        ("unarmed", vec![begin(true, true, b"light"), call(b'L', b"", ready.clone()), call(b'S', br#"{"type":"theme"}"#, Vec::new())]),
        (
            "armed",
            vec![
                begin(true, true, b"light"),
                call(b'T', b"dark", Vec::new()),
                call(b'P', &wire::encode_mirror(), Vec::new()),
                call(b'S', br#"{"type":"ops","ops":[]}"#, Vec::new()),
                call(b'S', br#"{"type":"query"}"#, ready.clone()),
                call(b'P', &wire::encode_mirror(), Vec::new()),
            ],
        ),
        (
            "timed",
            vec![
                begin(true, true, b"dark"),
                call(b'P', &wire::encode_mirror_work(), Vec::new()),
                call(b'P', &frame_request(7, true, Paint::IfDirty), painting.clone()),
                call(b'P', &frame_request(8, true, Paint::Skip), painting.clone()),
                call(b'P', &frame_request(9, false, Paint::Skip), painting.clone()),
                call(b'P', &wire::encode_open(2.0), painting.clone()),
                call(b'S', br#"{"type":"x"}"#, vec![vec![act(0, br#"@again:{"type":"y"}"#), act(3, b"R")], vec![act(3, b"X")]]),
                call(b'T', b"light", Vec::new()),
            ],
        ),
        (
            "panicking",
            vec![
                begin(true, true, b"light"),
                call(b'P', &wire::encode_mirror_work(), Vec::new()),
                call(b'S', br#"{"type":"x"}"#, vec![vec![act(2, &remote(64, 10)), act(4, b"")]]),
                call(b'P', &frame_request(1, true, Paint::Force), vec![vec![act(0, br#"@again:{}"#)], vec![act(4, b"")]]),
                call(b'S', br#"{"type":"after"}"#, painting.clone()),
            ],
        ),
        (
            "gone",
            vec![
                begin(true, true, b"light"),
                call(b'E', b"", Vec::new()),
                call(b'P', &wire::encode_mirror(), Vec::new()),
                call(b'P', b"anything", Vec::new()),
            ],
        ),
        ("no platform", vec![begin(false, true, b"dark"), call(b'L', b"", ready.clone()), call(b'P', &wire::encode_mirror(), Vec::new())]),
        ("no renderer", vec![begin(true, false, b"dark"), call(b'P', &wire::encode_mirror(), Vec::new()), call(b'S', b"{}", Vec::new())]),
    ]
}

#[test]
fn written_sessions_alike() {
    let Some(oracle) = helper_oracle("helper_tap_oracle") else { return };
    quiet_panics();
    let mut c = Comparison::new();
    let mut tally = Tally::default();
    for (name, session) in written() {
        compare(&mut c, &session, name, &mut tally);
    }
    c.check(&oracle);
    eprintln!("tap: {} written steps alike ({} transcript bytes)", tally[0], tally[1]);
}

#[test]
fn generated_sessions_alike() {
    let Some(oracle) = helper_oracle("helper_tap_oracle") else { return };
    quiet_panics();
    let mut random = Lcg::new(0x5441_5001);
    let mut c = Comparison::new();
    let mut tally = Tally::default();
    for i in 0..3000 {
        let session = any_session(&mut random);
        compare(&mut c, &session, &format!("session {i}"), &mut tally);
    }
    c.check(&oracle);
    eprintln!(
        "tap: 3000 generated sessions, {} steps alike ({} transcript bytes; mirrored b/t/h/r/w {:?})",
        tally[0],
        tally[1],
        &tally[2..]
    );
}

/// A message past the mirror's room is forwarded, its record skipped.
#[test]
fn past_the_mirror_alike() {
    let Some(oracle) = helper_oracle("helper_tap_oracle") else { return };
    let mut c = Comparison::new();
    let mut tally = Tally::default();
    let long = vec![b'{'; wire::MAX_MIRRORED_TEXT_BYTES + 1];
    let session = vec![
        Step::Begin { platform: true, renderer: true, scheme: b"light".to_vec() },
        call(b'P', &wire::encode_mirror(), Vec::new()),
        call(b'S', &long[..long.len() - 1], Vec::new()),
        call(b'S', &long, Vec::new()),
    ];
    compare(&mut c, &session, "long", &mut tally);
    c.check(&oracle);
}
