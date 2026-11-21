//! tests/recording.h's file, written and read back: every kind of value survives the trip, and the reader refuses a
//! damaged file at the first thing the C++ reader refuses, with its words.

use std::path::PathBuf;
use std::sync::Arc;

use twidget_json::Json;
use twidget_remote::{GestureClaim, State};
use twidget_replay::recording::{End, Kind, Reader, Step, Writer, put_bytes, put_varint};
use twidget_replay::{Call, Op};
use twidget_wire::{CanvasClip, CanvasOp, Cmd, Command, FontMetrics, OpKind, Path as WirePath, Rect};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("twidget_replay_tests_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir.join(name)
}

fn steps() -> Vec<Step> {
    let construct = Step {
        kind: Kind::Construct,
        session: 1,
        label: b"a widget".to_vec(),
        names: 300,
        name_bytes: 2000,
        numbers: vec![320.0, 2.0, 0.0],
        clock: vec![5, 9],
        ..Step::default()
    };
    let clips = Arc::new(vec![CanvasClip::default()]);
    let ops = vec![
        CanvasOp { kind: OpKind::Fill, clips: Some(Arc::clone(&clips)), ..CanvasOp::default() },
        CanvasOp { kind: OpKind::PutImage, clips: Some(clips), rgba: Some(Arc::new(vec![1, 2, 3, 4])), w: 1, h: 1, ..CanvasOp::default() },
    ];
    let calls = vec![
        Call {
            op: Op::Font,
            families: b"serif".to_vec(),
            features: b"tnum=1".to_vec(),
            size: 13.5,
            weight: 700,
            italic: true,
            metrics: FontMetrics { ascent: 10.0, ..FontMetrics::default() },
            font: 3,
            ..Call::default()
        },
        Call { op: Op::CanvasFont, families: b"sans".to_vec(), size: 10.0, font: 4, ..Call::default() },
        Call { op: Op::TextWidth, font: 3, text: "wörd".as_bytes().to_vec(), width: 31.25, ..Call::default() },
        Call {
            op: Op::Image,
            id: 7,
            url: b"data:image/png;base64,AAAA".to_vec(),
            w_in: -1.0,
            ok: true,
            image_w: 24.0,
            image_h: 16.0,
            ..Call::default()
        },
        Call { op: Op::CanvasRun, id: 2, w: 300, h: 150, ops, ..Call::default() },
        Call { op: Op::CanvasRead, id: 2, x: -4, y: 1, w: 2, h: 1, rgba_in: 0, ok: true, rgba: vec![9; 8], ..Call::default() },
        Call { op: Op::CanvasRelease, id: 2, ..Call::default() },
        Call { op: Op::DeleteFont, font: 3, ..Call::default() },
        Call { op: Op::Frame, painted: true, ..Call::default() },
    ];
    let path = Arc::new(WirePath::default());
    let list = vec![
        Command { kind: Cmd::Path, path: Some(Arc::clone(&path)), key: 0xfeed, ..Command::default() },
        Command { kind: Cmd::Path, path: Some(path), text: b"t".to_vec(), ..Command::default() },
    ];
    let message = Json::parse(r#"{"type":"ops","ops":[[1,"div",{"a":null,"b":true,"c":-0.5}]],"deep":[[[[]]]]}"#).expect("json");
    let frame = Step {
        kind: Kind::Frame,
        session: 1,
        numbers: vec![1.0, 1.0],
        clock: vec![12, 12, 40],
        calls,
        messages: vec![br#"{"type":"size","height":40}"#.to_vec()],
        engine: true,
        painted: true,
        list,
        damage: Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]),
        selection: b"sel".to_vec(),
        state: State { width: 320.0, height: 40, dirty: false, wants_tick: true, next_tick_in: 0.016, frame_asked: true },
        ..Step::default()
    };
    let receive = Step { kind: Kind::Receive, session: 1, message: Some(message), clock: vec![10], ..Step::default() };
    let claim = Step {
        kind: Kind::ClaimAt,
        session: 1,
        numbers: vec![40.0, 30.0],
        claim: GestureClaim { x: true, y: false, hold: true },
        ..Step::default()
    };
    let touch = Step {
        kind: Kind::Touch,
        session: 1,
        numbers: vec![0.0, 1.0, 2.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        cursor: b"pointer".to_vec(),
        ..Step::default()
    };
    let key = Step {
        kind: Kind::Key,
        session: 1,
        numbers: vec![0.0, 2.0],
        texts: vec![b"keydown".to_vec(), b"a".to_vec(), b"KeyA".to_vec()],
        ..Step::default()
    };
    let destroy = Step { kind: Kind::Destroy, session: 1, ..Step::default() };
    vec![construct, receive, frame, claim, touch, key, destroy]
}

fn write(name: &str, steps: &[Step], end: &End) -> PathBuf {
    let path = scratch(name);
    let mut w = Writer::default();
    assert!(w.open(&path, br#"{"source":"tests"}"#), "the writer opens");
    for s in steps {
        assert!(w.write(s), "the writer takes a {:?}", s.kind);
    }
    assert!(w.close(end), "the writer closes");
    path
}

fn valid() -> PathBuf {
    write("valid.twrec", &steps(), &End { sessions: 1, steps: 7, error: Vec::new() })
}

/// The reader's verdict on a file: the steps it gave and its error, empty when it ended.
fn read(path: &std::path::Path) -> (Vec<Step>, String) {
    let mut r = Reader::open(path);
    if r.refused_at_open() {
        return (Vec::new(), r.error().to_string());
    }
    let mut got = Vec::new();
    let mut step = Step::default();
    let mut bytes = 0;
    while r.next(&mut step, &mut bytes) {
        assert!(bytes > 0, "a step takes bytes");
        got.push(step.clone());
    }
    (got, if r.ended() { String::new() } else { r.error().to_string() })
}

fn with_bytes(name: &str, bytes: Vec<u8>) -> PathBuf {
    let path = scratch(name);
    std::fs::write(&path, bytes).expect("written");
    path
}

#[test]
fn round_trip() {
    let path = valid();
    let (got, error) = read(&path);
    assert_eq!(error, "", "the recording reads to its end");
    let want = steps();
    assert_eq!(got.len(), want.len());
    for (a, b) in got.iter().zip(&want) {
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.session, b.session);
        assert_eq!(a.label, b.label);
        assert_eq!((a.names, a.name_bytes), (b.names, b.name_bytes));
        assert_eq!(a.numbers, b.numbers);
        assert_eq!(a.texts, b.texts);
        assert_eq!(a.message, b.message);
        assert_eq!(a.clock, b.clock, "clock reads come back from their deltas");
        assert_eq!(a.messages, b.messages);
        assert_eq!((a.engine, a.painted, a.claim), (b.engine, b.painted, b.claim));
        assert_eq!(a.cursor, b.cursor);
        assert_eq!(a.selection, b.selection);
        assert_eq!(a.state, b.state);
        assert!(twidget_replay::compare::same_list(&a.list, &b.list).is_ok(), "the list comes back");
        assert_eq!(twidget_replay::compare::list_sharing(&a.list), twidget_replay::compare::list_sharing(&b.list), "and shares as it did");
        assert!(twidget_replay::compare::same_damage(&a.damage, &b.damage).is_ok());
        assert_eq!(twidget_replay::compare::same_calls(&a.calls, &b.calls), Ok(()), "every call comes back, its batch shared as it was");
    }
}

#[test]
fn the_writer_refuses_what_no_reader_gives_back() {
    let path = scratch("refused.twrec");
    let mut w = Writer::default();
    assert!(!w.open(&path, b"not json"), "an origin that is not JSON");
    assert!(w.open(&path, b"{}"));
    assert!(!w.write(&Step { kind: Kind::End, session: 1, ..Step::default() }), "an End step");
    assert!(!w.write(&Step { kind: Kind::Frame, session: 0, ..Step::default() }), "session 0");
    assert!(!w.write(&Step { kind: Kind::Frame, session: 1, list: vec![Command::default()], ..Step::default() }), "a list unpainted");
    assert!(w.write(&Step { kind: Kind::Construct, session: 1, clock: vec![10], ..Step::default() }));
    assert!(!w.write(&Step { kind: Kind::Frame, session: 1, clock: vec![9], ..Step::default() }), "a clock read before the session's last");
    let mut deep = Json::Null;
    for _ in 0..1030 {
        deep = Json::Array(vec![deep]);
    }
    assert!(!w.write(&Step { kind: Kind::Receive, session: 1, message: Some(deep), ..Step::default() }), "a message nested too deep");
    assert!(w.close(&End { sessions: 1, steps: 1, error: Vec::new() }));
}

#[test]
fn refusals() {
    let good = std::fs::read(valid()).expect("read");
    let origin_end = 5 + 1 + 1 + br#"{"source":"tests"}"#.len();
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("empty", Vec::new(), "not a recording"),
        ("magic", [b"TWREX".as_slice(), &good[5..]].concat(), "not a recording"),
        ("version", [&good[..5], &[2u8][..], &good[6..]].concat(), "version 2"),
        ("origin cut", good[..origin_end - 3].to_vec(), "truncated in the origin"),
        ("origin length", [&good[..6], &[0x80u8, 0x00][..]].concat(), "a malformed origin length"),
        ("no end", good[..origin_end].to_vec(), "no end record after 0 steps"),
        ("cut", good[..good.len() - 2].to_vec(), "truncated in the end record"),
        ("trailing", [good.as_slice(), &[0u8][..]].concat(), "bytes after the end record"),
    ];
    for (name, bytes, want) in cases {
        let (_, error) = read(&with_bytes(&format!("{name}.twrec"), bytes));
        assert_eq!(error, want, "{name}");
    }
    let mut origin = b"TWREC\x01".to_vec();
    put_bytes(&mut origin, b"{oops");
    origin.push(1);
    origin.push(0);
    let (_, error) = read(&with_bytes("origin json.twrec", origin));
    assert_eq!(error, "the origin is not JSON");

    let all = steps();
    let counts = write("counts.twrec", &all, &End { sessions: 1, steps: 6, error: Vec::new() });
    assert_eq!(read(&counts).1, "the end record counts 1 sessions and 6 steps where the file has 1 and 7");
    let failed = write("failed.twrec", &all, &End { sessions: 1, steps: 7, error: b"a widget died".to_vec() });
    assert_eq!(read(&failed).1, "the recording ends with an error: a widget died");

    // A step whose frame holds a flag of 2, and one of an unknown kind.
    let mut head = b"TWREC\x01".to_vec();
    put_bytes(&mut head, b"{}");
    let mut flag = vec![Kind::Construct as u8];
    put_varint(&mut flag, 1);
    put_bytes(&mut flag, b"");
    put_varint(&mut flag, 0);
    put_varint(&mut flag, 0);
    put_varint(&mut flag, 0);
    put_varint(&mut flag, 0);
    flag.push(2);
    let mut bytes = head.clone();
    put_varint(&mut bytes, flag.len() as u64);
    bytes.extend_from_slice(&flag);
    assert_eq!(read(&with_bytes("flag.twrec", bytes)).1, "step 1 holds a flag of 2");
    let mut bytes = head.clone();
    put_varint(&mut bytes, 1);
    bytes.push(24);
    assert_eq!(read(&with_bytes("kind.twrec", bytes)).1, "step 1 has unknown kind 24");
    let mut bytes = head.clone();
    put_varint(&mut bytes, 2);
    bytes.extend_from_slice(&[Kind::Destroy as u8, 0]);
    assert_eq!(read(&with_bytes("session.twrec", bytes)).1, "step 1 holds session 0");
    let mut bytes = head;
    put_varint(&mut bytes, 3);
    bytes.extend_from_slice(&[Kind::Destroy as u8, 0x81, 0x00]);
    assert_eq!(read(&with_bytes("number.twrec", bytes)).1, "step 1 holds a malformed number");
}
