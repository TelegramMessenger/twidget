//! The hosted wire's bytes as tgw_hosted_wire.h lays them out, its refusals, and the helper work record; and
//! test_widget_ops_check.cpp's `ExerciseHelperWork`, whose records decode or not as written there (the client's
//! own checks of a decoded record, its room and its receipt, are Telegram's).

use twidget_helper::wire::{self, Kind};
use twidget_helper::{
    HELPER_WORK_RECORD_BYTES, HelperWorkKind, HelperWorkRecord, MAX_HELPER_INTERVAL, decode_helper_work, encode_helper_work,
};

#[test]
fn documented_bytes() {
    assert_eq!(wire::encode_open(2.0), [&[0xFF, 1][..], &2.0f64.to_bits().to_le_bytes()].concat());
    assert_eq!(wire::encode_menu_deliver(0x0102), [0xFF, 2, 2, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(wire::encode_finger_hold(7)[..2], [0xFF, 3]);
    assert_eq!(wire::encode_finger_release(7)[..2], [0xFF, 4]);
    assert_eq!(wire::encode_mirror(), [0xFF, 5]);
    assert_eq!(wire::encode_mirror_work(), [0xFF, 6]);
    assert_eq!(wire::encode_canvas_refill(), [0xFF, 7]);
    assert_eq!(wire::encode_content(), [0xFF, 64]);
    assert_eq!(wire::encode_loaded(), [0xFF, 65]);
    assert_eq!(wire::encode_stopped(b"no"), [0xFF, 66, 2, 0, 0, 0, b'n', b'o']);
    assert_eq!(wire::encode_mirrored(b'r', b"{}").expect("a way"), [0xFF, 67, b'r', 2, 0, 0, 0, b'{', b'}']);
    assert_eq!(wire::encode_cursor(3, b"pointer")[..14], [0xFF, 68, 3, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0]);
    assert_eq!(wire::encode_canvas_refused(), [0xFF, 69]);
    assert_eq!(wire::encode_clicked(9)[..3], [0xFF, 70, 9]);
}

#[test]
fn round_trips_and_refusals() {
    for dpr in [0.25, 1.0, 1.5, 3.0, -0.0, 1e300] {
        assert_eq!(wire::decode_open(&wire::encode_open(dpr)).map(f64::to_bits), Some(dpr.to_bits()));
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(wire::decode_open(&wire::encode_open(bad)), None, "a ratio that is not finite");
    }
    for (bytes, press) in
        [(wire::encode_menu_deliver(5), 5), (wire::encode_finger_hold(6), 6), (wire::encode_finger_release(u64::MAX), u64::MAX)]
    {
        assert_eq!(wire::decode_press(&bytes), Some(press));
        assert_eq!(wire::decode_press(&bytes[..bytes.len() - 1]), None, "cut");
    }
    assert_eq!(wire::decode_press(&wire::encode_clicked(5)), None, "Clicked is no press");
    assert_eq!(wire::decode_clicked(&wire::encode_clicked(5)), Some(5));
    let long = vec![b'x'; wire::MAX_STOP_REASON_BYTES + 10];
    let stopped = wire::encode_stopped(&long);
    assert_eq!(wire::decode_stopped(&stopped).map(<[u8]>::len), Some(wire::MAX_STOP_REASON_BYTES), "a reason is cut to its room");
    let cursor = wire::encode_cursor(11, &[b'c'; 300]);
    let report = wire::decode_cursor(&cursor).expect("a cursor");
    assert_eq!((report.pointers, report.cursor.len()), (11, wire::MAX_CURSOR_BYTES));
    assert_eq!(wire::encode_mirrored(b'x', b""), None, "an unknown way");
    let mirrored = wire::encode_mirrored(b'w', b"abc").expect("a way");
    let record = wire::decode_mirrored(&mirrored).expect("a record");
    assert_eq!((record.way, record.text), (b'w', &b"abc"[..]));
    let mut extra = mirrored.clone();
    extra.push(0);
    assert_eq!(wire::decode_mirrored(&extra), None, "a length that is not all of the rest");
    assert!(wire::is_exactly(&wire::encode_mirror(), Kind::Mirror) && !wire::is_exactly(&[0xFF, 5, 0], Kind::Mirror));
    assert_eq!(wire::kind_of(&[0xFF, 8]), None, "a kind no control has");
    assert_eq!(wire::kind_of(&[0xFE, 1]), None, "no tag");
    let caps = wire::embedding_caps();
    assert_eq!((caps.message, caps.frame, caps.delta_base, caps.values.commands), (32 << 20, 8 << 20, 256 << 10, 1 << 16));
}

#[test]
fn helper_work_records() {
    let record =
        HelperWorkRecord { started: 5, finished: 12, waited: 7, seq: 11, waits: u32::MAX, frame_bytes: 99, kind: HelperWorkKind::Tick };
    let bytes = encode_helper_work(&record);
    assert_eq!(bytes.len(), HELPER_WORK_RECORD_BYTES);
    assert_eq!(bytes[0], 3);
    assert_eq!(decode_helper_work(&bytes), Some(record));
}

/// `ExerciseHelperWork`'s records: one within the room, one spanning exactly an hour, then a stray byte, stamps
/// unordered, negative or more than an hour apart, more waited than the interval; and one that ends after its
/// receipt, which decodes (its receipt is the client's check).
#[test]
fn exercise_helper_work() {
    let now = 7_200_000_000i64;
    let record = HelperWorkRecord {
        started: now - 1000,
        finished: now - 993,
        waited: 3,
        seq: 11,
        waits: 2,
        frame_bytes: 99,
        kind: HelperWorkKind::Paint,
    };
    let decoded = |r: &HelperWorkRecord| decode_helper_work(&encode_helper_work(r));
    assert_eq!(decoded(&record), Some(record));
    let hour = HelperWorkRecord { started: record.finished - MAX_HELPER_INTERVAL, waited: MAX_HELPER_INTERVAL, ..record };
    assert_eq!(decoded(&hour), Some(hour), "exactly an hour");
    let mut stray = encode_helper_work(&record);
    stray.push(b'x');
    assert_eq!(decode_helper_work(&stray), None, "a stray byte");
    assert_eq!(decoded(&HelperWorkRecord { finished: record.started - 1, ..record }), None, "unordered");
    assert_eq!(decoded(&HelperWorkRecord { started: -1, ..record }), None, "negative");
    assert_eq!(decoded(&HelperWorkRecord { started: record.finished - MAX_HELPER_INTERVAL - 1, ..record }), None, "over an hour");
    assert_eq!(decoded(&HelperWorkRecord { waited: 8, ..record }), None, "overwaited");
    let ahead = HelperWorkRecord { started: now - 5, finished: now + 60_000_000, waited: 0, ..record };
    assert_eq!(decoded(&ahead), Some(ahead), "ends after its receipt: the client refuses it, not the decoder");
    let mut unknown = encode_helper_work(&record);
    unknown[0] = 7;
    assert_eq!(decode_helper_work(&unknown), None, "a kind past Control");
}
