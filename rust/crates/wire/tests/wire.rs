//! tests/wire_tests.cpp, ported: samples that cover every kind and member come back bit for bit and
//! shared as they were, their bytes follow docs/TWIDGET_CORE.md "Binary forms", and hostile bytes are
//! refused. Two of its checks have no Rust counterpart: encoders refusing enum values version 1 does
//! not know (a Rust enum cannot hold one), and the allocator's count of each decode (it needs a global
//! allocator, which is `unsafe`); what a decode holds is bounded instead.

mod common;

use std::sync::Arc;

use common::*;
use twidget_wire::*;

fn accepted(form: FormKind, what: &str, bytes: &[u8], caps: &Caps) -> String {
    match form.decode(bytes, caps) {
        Some((dump, held)) => {
            assert!(held <= budget(bytes.len()), "{what}: holds {held} bytes for {} input bytes", bytes.len());
            dump
        }
        None => panic!("{what}: decodes"),
    }
}

fn refused(form: FormKind, what: &str, bytes: &[u8], caps: &Caps) {
    assert!(form.decode(bytes, caps).is_none(), "{what}: refused");
}

/// Accepted, and encodes back to exactly `bytes` when `canonical`.
fn accepted_back(form: FormKind, what: &str, bytes: &[u8], caps: &Caps, canonical: bool) {
    accepted(form, what, bytes, caps);
    if canonical {
        let (status, again) = form.answer(bytes, caps, caps);
        assert_eq!(status, 1, "{what}: encodes again");
        assert_eq!(hex(&again), hex(bytes), "{what}: encodes back to its bytes");
    }
}

#[test]
fn documented_numbers() {
    assert_eq!(Form::List as u8, 1);
    assert_eq!(Form::Damage as u8, 2);
    assert_eq!(Form::Batch as u8, 3);
    assert_eq!(Form::Metrics as u8, 4);
    assert_eq!(VERSION, 1);
    assert_eq!(Cmd::LAST, Cmd::Backdrop);
    assert_eq!(Cmd::Backdrop.as_u8(), 17);
    assert_eq!(PathOp::Close.as_u8(), 4);
    assert_eq!(FilterKind::DropShadow.as_u8(), 9);
    assert_eq!(OpKind::PutImage.as_u8(), 6);
    assert_eq!(PaintKind::Conic.as_u8(), 3);
    assert_eq!(Composite::Luminosity.as_u8(), 25);
    assert_eq!(Composite::LAST, Composite::Luminosity);
    let caps = Caps::default();
    assert_eq!(caps.commands, 1 << 20);
    assert_eq!(caps.canvas_ops, 1 << 20);
    assert_eq!(caps.path_ops, 1 << 22);
    assert_eq!(caps.floats, 1 << 22);
    assert_eq!(caps.stops, 1 << 16);
    assert_eq!(caps.filters, 1 << 12);
    assert_eq!(caps.clips, 1 << 16);
    assert_eq!(caps.text, 4 << 20);
    assert_eq!(caps.pixels, 64 << 20);
    assert_eq!(caps.rects, 1 << 12);
}

#[test]
fn version_1_defaults() {
    assert_eq!(dump_list(&[Command::default()]), dump_list(&[version_1_command()]), "Command's defaults are version 1's");
    let with_path = |p: Path| Command { path: Some(Arc::new(p)), ..Command::default() };
    assert_eq!(dump_list(&[with_path(Path::default())]), dump_list(&[with_path(version_1_path())]), "Path's defaults are version 1's");
    assert_eq!(dump_batch(&[CanvasOp::default()]), dump_batch(&[version_1_op()]), "CanvasOp's defaults are version 1's");
    let with_paint = |p: CanvasPaint| CanvasOp { paint: p, ..CanvasOp::default() };
    assert_eq!(
        dump_batch(&[with_paint(CanvasPaint::default())]),
        dump_batch(&[with_paint(version_1_paint())]),
        "CanvasPaint's defaults are version 1's"
    );
    assert_eq!(Filter::default(), Filter { kind: FilterKind::Blur, amount: 0.0, x: 0.0, y: 0.0, color: Color::BLACK });
    assert_eq!(Stop::default(), Stop { offset: 0.0, color: Color::BLACK });
    assert_eq!(
        FontMetrics::default(),
        FontMetrics { ascent: 0.0, descent: 0.0, height: 0.0, x_height: 0.0, zero_width: 0.0, em_ascent: 0.0, em_descent: 0.0 }
    );

    let list_bytes = Bytes::new().head(1).u32(2).u8(0).u32(0).u8(11).u32(0x60000).u32(0).u32(0).u32(0).u32(0).s;
    let list = decode_list(&list_bytes, &Caps::default()).expect("a list of empty masks and empty payloads decodes");
    assert_eq!(list.len(), 2);
    assert_eq!(dump_list(&list[..1]), dump_list(&[version_1_command()]), "an empty command mask decodes to version 1's defaults");
    let path = list[1].path.as_ref().expect("the path is there");
    assert_eq!(
        dump_list(&[with_path((**path).clone())]),
        dump_list(&[with_path(version_1_path())]),
        "an empty path mask decodes to version 1's defaults"
    );
    assert!(list[1].filter.as_ref().is_some_and(|f| f.is_empty()), "an empty filter list decodes to an empty one, not to none");

    let batch_bytes = Bytes::new().head(3).u32(2).u8(1).u64(0).u8(1).u64(1 << 2).u8(0).u8(0).s;
    let batch = decode_batch(&batch_bytes, &Caps::default()).expect("a batch of empty masks decodes");
    assert_eq!(batch.len(), 2);
    assert_eq!(dump_batch(&batch[..1]), dump_batch(&[version_1_op()]), "an empty op mask decodes to version 1's defaults");
    assert_eq!(dump_batch(&batch[1..]), dump_batch(&[version_1_op()]), "an empty paint mask decodes to version 1's paint");
}

fn golden_list(what: &str, list: &[Command], expected: &[u8]) {
    let mut out = Vec::new();
    assert!(encode_list(list, &Caps::default(), &mut out), "{what}: encodes");
    assert_eq!(hex(&out), hex(expected), "{what}: the documented bytes");
    let back = decode_list(expected, &Caps::default()).expect("the documented bytes decode");
    assert_eq!(dump_list(&back), dump_list(list), "{what}: the documented bytes decode to it");
}

fn golden_batch(what: &str, batch: &[CanvasOp], expected: &[u8]) {
    let mut out = Vec::new();
    assert!(encode_batch(batch, &Caps::default(), &mut out), "{what}: encodes");
    assert_eq!(hex(&out), hex(expected), "{what}: the documented bytes");
    let back = decode_batch(expected, &Caps::default()).expect("the documented bytes decode");
    assert_eq!(dump_batch(&back), dump_batch(batch), "{what}: the documented bytes decode to it");
}

fn golden_damage(what: &str, damage: &Damage, expected: &[u8]) {
    let mut out = Vec::new();
    assert!(encode_damage(damage, &Caps::default(), &mut out), "{what}: encodes");
    assert_eq!(hex(&out), hex(expected), "{what}: the documented bytes");
    let back = decode_damage(expected, &Caps::default()).expect("the documented bytes decode");
    assert_eq!(dump_damage(&back), dump_damage(damage), "{what}: the documented bytes decode to it");
}

#[test]
fn golden_bytes() {
    let sparse = vec![Command { rect: Rect::new(1.0, 2.0, 3.0, 4.0), color: Color::rgba(10, 20, 30, 40), ..Command::default() }];
    golden_list(
        "a FillRect with a rect and a colour",
        &sparse,
        &unhex("0101 01000000 00 11000000 0000803f 00000040 00004040 00008040 0a141e28"),
    );

    let filled = Arc::new(Path { fill: true, ..Path::default() });
    let pair: Vec<Command> = (0..2).map(|_| Command { kind: Cmd::Path, path: Some(Arc::clone(&filled)), ..Command::default() }).collect();
    golden_list(
        "two Path commands sharing one filled path",
        &pair,
        &unhex("0101 02000000 0b 00000200 00000000 04000000 0b 00000200 01000000"),
    );

    golden_damage("damage of the whole frame", &None, &unhex("0201 00"));
    golden_damage("damage with nothing moved", &Some(Vec::new()), &unhex("0201 01 00000000"));
    golden_damage(
        "damage of one rect",
        &Some(vec![Rect::new(1.0, 2.0, 3.0, 4.0)]),
        &unhex("0201 01 01000000 0000803f 00000040 00004040 00008040"),
    );

    let mut multiply = CanvasOp { composite: Composite::Multiply, ..CanvasOp::default() };
    multiply.paint.color = Color::rgba(1, 2, 3, 4);
    golden_batch("a Fill in a solid colour, multiplied", &[multiply], &unhex("0301 01000000 01 0408000000000000 00 01 01020304 0b"));

    let metrics = FontMetrics { ascent: 1.0, descent: 2.0, height: 3.0, x_height: 4.0, zero_width: 5.0, em_ascent: 6.0, em_descent: 7.0 };
    let mut out = Vec::new();
    encode_metrics(&metrics, &mut out);
    assert_eq!(hex(&out), hex(&unhex("0401 0000803f 00000040 00004040 00008040 0000a040 0000c040 0000e040")), "metrics 1 to 7");

    golden_list("every member of a command, its path and its filters", &full_list(), &full_list_bytes());
    golden_batch("every member of an op, its paint, its clips and its pixels", &full_batch(), &full_batch_bytes());
}

fn round_trip_all(caps: &Caps, under: &str) {
    for s in list_samples() {
        let mut bytes = Vec::new();
        assert!(encode_list(&s.value, caps, &mut bytes), "list {} encodes {under}", s.name);
        let back = decode_list(&bytes, caps).unwrap_or_else(|| panic!("list {} decodes {under}", s.name));
        assert_eq!(dump_list(&back), dump_list(&s.value), "list {} comes back bit for bit and shared alike {under}", s.name);
        let mut again = Vec::new();
        assert!(encode_list(&back, caps, &mut again) && again == bytes, "list {} encodes again to its bytes {under}", s.name);
    }
    for s in batch_samples() {
        let mut bytes = Vec::new();
        assert!(encode_batch(&s.value, caps, &mut bytes), "batch {} encodes {under}", s.name);
        let back = decode_batch(&bytes, caps).unwrap_or_else(|| panic!("batch {} decodes {under}", s.name));
        assert_eq!(dump_batch(&back), dump_batch(&s.value), "batch {} comes back bit for bit and shared alike {under}", s.name);
        let mut again = Vec::new();
        assert!(encode_batch(&back, caps, &mut again) && again == bytes, "batch {} encodes again to its bytes {under}", s.name);
    }
    for s in damage_samples() {
        let mut bytes = Vec::new();
        assert!(encode_damage(&s.value, caps, &mut bytes), "damage {} encodes {under}", s.name);
        let back = decode_damage(&bytes, caps).unwrap_or_else(|| panic!("damage {} decodes {under}", s.name));
        assert_eq!(dump_damage(&back), dump_damage(&s.value), "damage {} comes back bit for bit {under}", s.name);
    }
    for s in metrics_samples() {
        let mut bytes = Vec::new();
        encode_metrics(&s.value, &mut bytes);
        assert_eq!(bytes.len(), 30, "metrics are 30 bytes");
        let back = decode_metrics(&bytes).unwrap_or_else(|| panic!("metrics {} decode", s.name));
        assert_eq!(dump_metrics(&back), dump_metrics(&s.value), "metrics {} come back bit for bit", s.name);
    }
}

#[test]
fn round_trips() {
    round_trip_all(&Caps::default(), "under default caps");
}

#[test]
fn round_trips_under_tight_caps() {
    round_trip_all(&tight(), "under tight caps");
}

/// Some sample reaches each of tight()'s caps: one less, and it no longer encodes.
#[test]
fn tight_caps_reached() {
    for name in CAP_NAMES {
        let mut caps = tight();
        *cap_mut(&mut caps, name) -= 1;
        let mut refusing = 0;
        let mut out = Vec::new();
        refusing += list_samples().iter().filter(|s| !encode_list(&s.value, &caps, &mut out)).count();
        refusing += batch_samples().iter().filter(|s| !encode_batch(&s.value, &caps, &mut out)).count();
        refusing += damage_samples().iter().filter(|s| !encode_damage(&s.value, &caps, &mut out)).count();
        assert!(refusing > 0, "a sample reaches the tight cap on {name}");
    }
}

/// An encoder appends to what `out` held and, refusing, cuts it back to that.
#[test]
fn encoders_append_and_cut_back() {
    let earlier = b"earlier bytes".to_vec();
    let mut out = earlier.clone();
    assert!(encode_list(&full_list(), &Caps::default(), &mut out));
    assert_eq!(&out[..earlier.len()], &earlier[..], "a list goes after the earlier bytes");
    assert_eq!(&out[earlier.len()..], &full_list_bytes()[..], "and is the same as alone");
    let mut caps = Caps::default();
    caps.text = 1;
    let mut out = earlier.clone();
    assert!(!encode_list(&full_list(), &caps, &mut out), "a text past its cap is refused");
    assert_eq!(out, earlier, "the refusal leaves the earlier bytes");
    let mut caps = Caps::default();
    caps.pixels = 3;
    let mut out = earlier.clone();
    assert!(!encode_batch(&full_batch(), &caps, &mut out), "pixels past their cap are refused");
    assert_eq!(out, earlier, "the refusal leaves the earlier bytes");
    let mut caps = Caps::default();
    caps.rects = 0;
    let mut out = earlier.clone();
    assert!(!encode_damage(&Some(vec![Rect::default()]), &caps, &mut out), "rects past their cap are refused");
    assert_eq!(out, earlier, "the refusal leaves the earlier bytes");
    let mut out = earlier.clone();
    encode_metrics(&FontMetrics::default(), &mut out);
    assert_eq!(out.len(), earlier.len() + 30);
}

/// Every cut of every sample, the sample with a byte after it and the sample twice over are refused.
#[test]
fn cuts() {
    for (form, name, bytes) in encoded_samples(&Caps::default()) {
        let caps = Caps::default();
        for n in 0..bytes.len() {
            refused(form, &format!("{form:?} {name} cut to {n} bytes"), &bytes[..n], &caps);
        }
        let mut more = bytes.clone();
        more.push(0);
        refused(form, &format!("{form:?} {name} with a NUL after it"), &more, &caps);
        *more.last_mut().unwrap() = 0xff;
        refused(form, &format!("{form:?} {name} with 0xff after it"), &more, &caps);
        let twice = [bytes.clone(), bytes.clone()].concat();
        refused(form, &format!("{form:?} {name} twice over"), &twice, &caps);
    }
    for form in FormKind::ALL {
        refused(form, "no bytes", &[], &Caps::default());
    }
}

/// Random changes to every sample under default and tight caps, the C++ suite's seeds: what is taken
/// encodes again under the same caps to what decodes the same, shared alike, and holds no more than a
/// bound linear in its bytes.
#[test]
fn mutations() {
    let seeds =
        [(FormKind::List, 0x11570000u64), (FormKind::Batch, 0xba7c0000), (FormKind::Damage, 0xda3a0000), (FormKind::Metrics, 0x3e7c0000)];
    for (form, seed) in seeds {
        let mut accepted_n = 0;
        let mut refused_n = 0;
        for (c, caps) in [Caps::default(), tight()].iter().enumerate() {
            let samples: Vec<_> = encoded_samples(caps).into_iter().filter(|(f, _, _)| *f == form).collect();
            for (index, (_, name, bytes)) in samples.iter().enumerate() {
                let start = seed + 0x100 * c as u64 + index as u64;
                let mut random = Lcg::new(start);
                for round in 0..ROUNDS {
                    let mut changed = bytes.clone();
                    let edits = mutate(&mut random, &mut changed);
                    let where_ = || format!("changes to {form:?} {name} from seed {start:#x}, round {round}:{edits}");
                    match form.decode(&changed, caps) {
                        Some((dump, held)) => {
                            accepted_n += 1;
                            assert!(held <= budget(changed.len()), "{}: holds {held} bytes", where_());
                            let (status, again) = form.answer(&changed, caps, caps);
                            assert_eq!(status, 1, "{}: encodes again", where_());
                            let back = form.decode(&again, caps).map(|d| d.0);
                            assert_eq!(back.as_deref(), Some(dump.as_str()), "{}: comes back the same", where_());
                        }
                        None => refused_n += 1,
                    }
                }
            }
        }
        assert!(accepted_n > 0 && refused_n > 0, "{form:?}: random changes both decode and are refused");
        eprintln!("changes: {form:?}, seeds from {seed:#x}: {accepted_n} decoded, {refused_n} refused");
    }
}

struct Site {
    name: &'static str,
    cap: &'static str,
    least: usize,
    form: u32,
    before: Vec<u8>,
    after: Vec<u8>,
    item: fn(Bytes, usize) -> Bytes,
}

impl Site {
    fn bytes(&self, declared: u32, items: usize, cut: bool) -> Vec<u8> {
        let mut b = Bytes::from(&self.before).u32(declared);
        for i in 0..items {
            b = (self.item)(b, i);
        }
        if cut {
            b.s.pop();
        }
        b.bytes(&self.after).s
    }
}

fn stop_item(b: Bytes, i: usize) -> Bytes {
    b.f32(0.125 * i as f32).color(i as u32, 2, 3, 255)
}

fn byte_item(b: Bytes, i: usize) -> Bytes {
    b.u8(u32::from(b'a') + (i % 26) as u32)
}

fn op_item(b: Bytes, i: usize) -> Bytes {
    b.u8((i % 5) as u32)
}

fn float_item(b: Bytes, i: usize) -> Bytes {
    b.f32(i as f32 + 0.5)
}

fn sites() -> Vec<Site> {
    let path: u32 = 1 << 17;
    let filter: u32 = 1 << 18;
    let opacity: u32 = 1 << 19;
    let key: u32 = 1 << 23;
    let stroke_width: u32 = 1 << 16;
    let list = Bytes::new().head(1).u32(1).s;
    let batch = Bytes::new().head(3).u32(1).s;
    let in_path = Bytes::from(&list).u8(11).u32(path).u32(0).s;
    let clips = Bytes::from(&batch).u8(1).u64((1 << 16) | (1 << 17)).u32(0).s;
    let site = |name, cap, least, form, before: Vec<u8>, after: Vec<u8>, item| Site { name, cap, least, form, before, after, item };
    vec![
        site("a list's commands", "commands", 5, 1, Bytes::new().head(1).s, vec![], |b, i| b.u8((i % 18) as u32).u32(0)),
        site("a command's stops", "stops", 8, 1, Bytes::from(&list).u8(8).u32((1 << 16) | key).s, Bytes::new().u64(7).s, stop_item),
        site("a command's text", "text", 1, 1, Bytes::from(&list).u8(4).u32((1 << 7) | key).s, Bytes::new().u64(7).s, byte_item),
        site("a path's ops", "path_ops", 1, 1, Bytes::from(&in_path).u32(1 | stroke_width).s, Bytes::new().f32(2.0).s, op_item),
        site("a path's points", "floats", 4, 1, Bytes::from(&in_path).u32((1 << 1) | stroke_width).s, Bytes::new().f32(2.0).s, float_item),
        site("a path's stops", "stops", 8, 1, Bytes::from(&in_path).u32((1 << 13) | stroke_width).s, Bytes::new().f32(2.0).s, stop_item),
        site("a path's dashes", "floats", 4, 1, Bytes::from(&in_path).u32((1 << 20) | (1 << 21)).s, Bytes::new().f32(0.5).s, float_item),
        site(
            "a path's stroke stops",
            "stops",
            8,
            1,
            Bytes::from(&list).u8(11).u32(path | opacity).u32(0).u32(1 << 24).s,
            Bytes::new().f32(0.5).s,
            stop_item,
        ),
        site(
            "a filter list's filters",
            "filters",
            17,
            1,
            Bytes::from(&list).u8(15).u32(filter | opacity).u32(0).s,
            Bytes::new().f32(0.5).s,
            |b, i| b.u8((i % 10) as u32).f32(i as f32).f32(1.0).f32(2.0).color(1, 2, 3, 4),
        ),
        site("a batch's ops", "canvas_ops", 9, 3, Bytes::new().head(3).s, vec![], |b, i| b.u8((i % 7) as u32).u64(0)),
        site("an op's path ops", "path_ops", 1, 3, Bytes::from(&batch).u8(1).u64(1 | (1 << 4)).s, Bytes::new().u32(0).f32(2.0).s, op_item),
        site("an op's points", "floats", 4, 3, Bytes::from(&batch).u8(1).u64(1 | (1 << 4)).u32(0).s, Bytes::new().f32(2.0).s, float_item),
        site("an op's dashes", "floats", 4, 3, Bytes::from(&batch).u8(1).u64((1 << 8) | (1 << 9)).s, Bytes::new().f32(0.5).s, float_item),
        site(
            "a paint's stops",
            "stops",
            8,
            3,
            Bytes::from(&batch).u8(1).u64((1 << 2) | (1 << 3)).u8(1).u8(1 << 2).s,
            Bytes::new().f32(2.0).f32(0.0).f32(0.0).f32(2.0).f32(0.0).f32(0.0).s,
            stop_item,
        ),
        site("an op's text", "text", 1, 3, Bytes::from(&batch).u8(4).u64((1 << 18) | (1 << 19)).s, Bytes::new().f32(3.0).s, byte_item),
        site("a clip list's clips", "clips", 9, 3, clips.clone(), Bytes::new().i32(5).s, |b, i| b.u32(0).u32(0).u8((i % 2) as u32)),
        site("a clip's path ops", "path_ops", 1, 3, Bytes::from(&clips).u32(1).s, Bytes::new().u32(0).u8(1).i32(5).s, op_item),
        site("a clip's points", "floats", 4, 3, Bytes::from(&clips).u32(1).u32(0).s, Bytes::new().u8(1).i32(5).s, float_item),
        site(
            "an op's pixels",
            "pixels",
            1,
            3,
            Bytes::from(&batch).u8(6).u64((1 << 35) | (1 << 36)).u32(0).s,
            Bytes::new().i32(1).s,
            |b, i| b.u8(i as u32),
        ),
        site("damage's rects", "rects", 16, 2, Bytes::new().head(2).u8(1).s, vec![], |b, i| b.f32(i as f32).f32(1.0).f32(2.0).f32(3.0)),
    ]
}

/// Counts no small input holds: around INT32_MAX, the largest, and those whose product with 4, 8 or
/// 16 bytes wraps in 32 bits.
const VAST: [u32; 9] = [0x7fffffff, 0x80000000, 0xffffffff, 0x40000000, 0x40000001, 0x20000000, 0x20000001, 0x10000000, 0x10000001];

/// Each count and length with only its own cap restrictive: complete at its cap and refused one past
/// it, at caps 0, 1 and 3, both ways; refused cut inside its items or declaring one more; refused when
/// declaring more than the bytes left hold, under the widest cap too; taken under a cap past u32::MAX.
#[test]
fn count_sites() {
    for s in sites() {
        let form = FormKind::of(s.form);
        for cap in [0usize, 1, 3] {
            let mut caps = Caps::default();
            *cap_mut(&mut caps, s.cap) = cap;
            let at_cap = s.bytes(cap as u32, cap, false);
            accepted_back(form, &format!("{} at a cap of {cap}", s.name), &at_cap, &caps, cap > 0);
            if cap > 0 {
                let mut below = Caps::default();
                *cap_mut(&mut below, s.cap) = cap - 1;
                let (status, _) = form.answer(&at_cap, &Caps::default(), &below);
                assert_eq!(status, 2, "{} at a cap of {cap}: the encoder refuses it one below", s.name);
            }
            refused(form, &format!("{} one past a cap of {cap}", s.name), &s.bytes(cap as u32 + 1, cap + 1, false), &caps);
        }
        refused(form, &format!("{} cut inside its last item", s.name), &s.bytes(3, 3, true), &Caps::default());
        refused(form, &format!("{} declaring one item more than it has", s.name), &s.bytes(3, 2, false), &Caps::default());
        accepted_back(form, &format!("{} holding one item", s.name), &s.bytes(1, 1, false), &Caps::default(), true);
        let left = s.bytes(1, 1, false).len() - s.before.len() - 4;
        refused(
            form,
            &format!("{} declaring one item more than the bytes left could hold", s.name),
            &s.bytes((left / s.least + 1) as u32, 1, false),
            &Caps::default(),
        );
        let mut open = Caps::default();
        *cap_mut(&mut open, s.cap) = usize::MAX;
        accepted_back(form, &format!("{} under a cap of usize::MAX", s.name), &s.bytes(3, 3, false), &open, true);
        for n in VAST {
            refused(form, &format!("{} declaring {n:#x} under a cap of usize::MAX", s.name), &s.bytes(n, 1, false), &open);
        }
        let mut widest = Caps::default();
        *cap_mut(&mut widest, s.cap) = u32::MAX as usize;
        accepted_back(form, &format!("{} under a cap of u32::MAX", s.name), &s.bytes(1, 1, false), &widest, true);
        let mut wide = Caps::default();
        *cap_mut(&mut wide, s.cap) = u32::MAX as usize + 1;
        accepted_back(form, &format!("{} under a cap of 2^32", s.name), &s.bytes(1, 1, false), &wide, true);
    }
}

struct Reference {
    name: &'static str,
    form: u32,
    fresh: fn(usize) -> Vec<u8>,
    naming: fn(u32) -> Vec<u8>,
}

/// The place of each payload a value's records name, by first appearance, of the kind `name` says.
fn places(form: FormKind, kind: &str, bytes: &[u8]) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    let mut place = |key: usize| match seen.iter().position(|k| *k == key) {
        Some(at) => at,
        None => {
            seen.push(key);
            seen.len() - 1
        }
    };
    match form {
        FormKind::List => {
            let list = decode_list(bytes, &Caps::default()).unwrap();
            list.iter()
                .map(|c| match kind {
                    "a path" => place(Arc::as_ptr(c.path.as_ref().unwrap()) as *const u8 as usize),
                    _ => place(Arc::as_ptr(c.filter.as_ref().unwrap()) as *const u8 as usize),
                })
                .collect()
        }
        _ => {
            let batch = decode_batch(bytes, &Caps::default()).unwrap();
            batch
                .iter()
                .map(|o| match kind {
                    "a clip list" => place(Arc::as_ptr(o.clips.as_ref().unwrap()) as *const u8 as usize),
                    _ => place(Arc::as_ptr(o.rgba.as_ref().unwrap()) as *const u8 as usize),
                })
                .collect()
        }
    }
}

/// Every reference kind names its first, a middle and its last payload so far, and is refused past
/// them, at u32::MAX, and before any payload is written.
#[test]
fn references() {
    let kinds = [
        Reference {
            name: "a path",
            form: 1,
            fresh: |i| Bytes::new().u8(11).u32(1 << 17).u32(0).u32(1 << 16).f32(i as f32 + 2.0).s,
            naming: |k| Bytes::new().u8(11).u32(1 << 17).u32(k).s,
        },
        Reference {
            name: "a filter list",
            form: 1,
            fresh: |i| Bytes::new().u8(15).u32(1 << 18).u32(0).u32(1).u8(0).f32(i as f32 + 1.0).f32(0.0).f32(0.0).color(0, 0, 0, 255).s,
            naming: |k| Bytes::new().u8(15).u32(1 << 18).u32(k).s,
        },
        Reference {
            name: "a clip list",
            form: 3,
            fresh: |i| Bytes::new().u8(1).u64(1 << 16).u32(0).u32(1).u32(0).u32(1).f32(i as f32).u8(0).s,
            naming: |k| Bytes::new().u8(1).u64(1 << 16).u32(k).s,
        },
        Reference {
            name: "pixels",
            form: 3,
            fresh: |i| Bytes::new().u8(6).u64(1 << 35).u32(0).u32(1).u8(i as u32).s,
            naming: |k| Bytes::new().u8(6).u64(1 << 35).u32(k).s,
        },
    ];
    for r in kinds {
        let form = FormKind::of(r.form);
        let with = |k: u32| [Bytes::new().head(r.form).u32(4).s, (r.fresh)(0), (r.fresh)(1), (r.fresh)(2), (r.naming)(k)].concat();
        for k in [1u32, 2, 3] {
            let what = format!("{} named by reference {k} of 3", r.name);
            accepted_back(form, &what, &with(k), &Caps::default(), true);
            assert_eq!(places(form, r.name, &with(k)), vec![0, 1, 2, k as usize - 1], "{what}: shares that payload");
        }
        for k in [4u32, 5, 0x80000000, 0xffffffff] {
            refused(form, &format!("{} named by reference {k:#x} of 3", r.name), &with(k), &Caps::default());
        }
        refused(
            form,
            &format!("{} named before any is written", r.name),
            &[Bytes::new().head(r.form).u32(1).s, (r.naming)(1)].concat(),
            &Caps::default(),
        );
        refused(
            form,
            &format!("{} named before the first is written", r.name),
            &[Bytes::new().head(r.form).u32(2).s, (r.naming)(1), (r.fresh)(0)].concat(),
            &Caps::default(),
        );
    }
}

/// Every enum, wherever it is and whether or not the kind around it paints it, takes its last value
/// and refuses the next one and above.
#[test]
fn enum_places() {
    type Place = (&'static str, u32, u32, fn(u32) -> Vec<u8>);
    let places: [Place; 8] = [
        ("a command's kind", 1, 17, |v| Bytes::new().head(1).u32(1).u8(v).u32(0).s),
        ("a filter's kind, on a FillRect", 1, 9, |v| {
            Bytes::new().head(1).u32(1).u8(0).u32(1 << 18).u32(0).u32(1).u8(v).f32(1.0).f32(2.0).f32(3.0).color(1, 2, 3, 4).s
        }),
        ("an op of a path, on a FillRect", 1, 4, |v| Bytes::new().head(1).u32(1).u8(0).u32(1 << 17).u32(0).u32(1).u32(2).u8(0).u8(v).s),
        ("an op's kind", 3, 6, |v| Bytes::new().head(3).u32(1).u8(v).u64(0).s),
        ("a paint's kind, on a Reset", 3, 3, |v| Bytes::new().head(3).u32(1).u8(0).u64(1 << 2).u8(v).u8(0).s),
        ("a composite, on a Reset", 3, 25, |v| Bytes::new().head(3).u32(1).u8(0).u64(1 << 11).u8(v).s),
        ("an op of an op's path, on a Text op", 3, 4, |v| Bytes::new().head(3).u32(1).u8(4).u64(1).u32(2).u8(0).u8(v).u32(0).s),
        ("an op of a clip's path, on a Clear", 3, 4, |v| {
            Bytes::new().head(3).u32(1).u8(3).u64(1 << 16).u32(0).u32(1).u32(2).u8(0).u8(v).u32(0).u8(0).s
        }),
    ];
    for (name, form, last, bytes) in places {
        let form = FormKind::of(form);
        accepted_back(form, &format!("{name} at {last}"), &bytes(last), &Caps::default(), true);
        for v in [last + 1, last + 2, 0x80, 0xff] {
            refused(form, &format!("{name} at {v}"), &bytes(v), &Caps::default());
        }
    }
}

/// Each mask bit past the last member is refused, next to the same record with an empty mask.
#[test]
fn mask_bits() {
    let caps = Caps::default();
    let command = Bytes::new().head(1).u32(1).u8(0).s;
    let path = Bytes::new().head(1).u32(1).u8(11).u32(1 << 17).u32(0).s;
    let op = Bytes::new().head(3).u32(1).u8(1).s;
    let paint = Bytes::from(&op).u64(1 << 2).u8(0).s;
    accepted_back(FormKind::List, "a command with an empty mask", &Bytes::from(&command).u32(0).s, &caps, true);
    for bit in 24..32 {
        refused(FormKind::List, &format!("a command with mask bit {bit}"), &Bytes::from(&command).u32(1 << bit).s, &caps);
    }
    refused(FormKind::List, "a command with every mask bit", &Bytes::from(&command).u32(0xffffffff).s, &caps);
    accepted_back(FormKind::List, "a path with an empty mask", &Bytes::from(&path).u32(0).s, &caps, true);
    for bit in 25..32 {
        refused(FormKind::List, &format!("a path with mask bit {bit}"), &Bytes::from(&path).u32(1 << bit).s, &caps);
    }
    accepted_back(FormKind::Batch, "an op with an empty mask", &Bytes::from(&op).u64(0).s, &caps, true);
    for bit in 38..64 {
        refused(FormKind::Batch, &format!("an op with mask bit {bit}"), &Bytes::from(&op).u64(1 << bit).s, &caps);
    }
    refused(FormKind::Batch, "an op with every mask bit", &Bytes::from(&op).u64(u64::MAX).s, &caps);
    accepted_back(FormKind::Batch, "a paint with an empty mask", &Bytes::from(&paint).u8(0).s, &caps, false);
    for bit in 3..8 {
        refused(FormKind::Batch, &format!("a paint with mask bit {bit}"), &Bytes::from(&paint).u8(1 << bit).s, &caps);
    }
}

/// The two flags take 0 and 1 and refuse anything else.
#[test]
fn flags() {
    let caps = Caps::default();
    accepted_back(FormKind::Damage, "damage flag 0", &Bytes::new().head(2).u8(0).s, &caps, true);
    accepted_back(FormKind::Damage, "damage flag 1", &Bytes::new().head(2).u8(1).u32(0).s, &caps, true);
    let clip = Bytes::new().head(3).u32(1).u8(1).u64(1 << 16).u32(0).u32(1).u32(0).u32(0).s;
    accepted_back(FormKind::Batch, "a clip's evenodd 0", &Bytes::from(&clip).u8(0).s, &caps, true);
    accepted_back(FormKind::Batch, "a clip's evenodd 1", &Bytes::from(&clip).u8(1).s, &caps, true);
    for v in [2u32, 0x80, 0xff] {
        refused(FormKind::Damage, &format!("damage flag {v}"), &Bytes::new().head(2).u8(v).s, &caps);
        refused(FormKind::Damage, &format!("damage flag {v} before an empty count"), &Bytes::new().head(2).u8(v).u32(0).s, &caps);
        refused(FormKind::Batch, &format!("a clip's evenodd {v}"), &Bytes::from(&clip).u8(v).s, &caps);
    }
}

/// A value marked with any other form or version is refused, and a value of one form is refused by
/// the decoders of the other three.
#[test]
fn forms_apart() {
    let caps = Caps::default();
    let mut each = Vec::new();
    let mut list = Vec::new();
    assert!(encode_list(&full_list(), &caps, &mut list));
    each.push((FormKind::List, list));
    let mut damage = Vec::new();
    assert!(encode_damage(named(&damage_samples(), "rects, special floats among them"), &caps, &mut damage));
    each.push((FormKind::Damage, damage));
    let mut batch = Vec::new();
    assert!(encode_batch(&full_batch(), &caps, &mut batch));
    each.push((FormKind::Batch, batch));
    let mut metrics = Vec::new();
    encode_metrics(named(&metrics_samples(), "ordinary metrics"), &mut metrics);
    each.push((FormKind::Metrics, metrics));
    for (form, bytes) in &each {
        accepted_back(*form, &format!("{form:?} as it is"), bytes, &caps, true);
        for f in [0u8, 1, 2, 3, 4, 5, 0x81, 0xff] {
            if f == bytes[0] {
                continue;
            }
            let mut other = bytes.clone();
            other[0] = f;
            refused(*form, &format!("{form:?} marked form {f}"), &other, &caps);
        }
        for v in [0u8, 2, 0x81, 0xff] {
            let mut other = bytes.clone();
            other[1] = v;
            refused(*form, &format!("{form:?} marked version {v}"), &other, &caps);
        }
    }
    for (of, bytes) in &each {
        for as_ in FormKind::ALL {
            if as_ != *of {
                refused(as_, &format!("a {of:?} value read as {as_:?}"), bytes, &caps);
            }
        }
    }
}

fn zeros(mut b: Bytes, n: usize) -> Bytes {
    for _ in 0..n {
        b = b.f32(0.0);
    }
    b
}

/// Every member written at its version-1 default and every list written empty decodes to the
/// defaults, and encodes again without them.
#[test]
fn present_defaults() {
    let caps = Caps::default();
    let c = Bytes::new().head(1).u32(1).u8(0).u32(0x00ffffff & !((1 << 3) | (1 << 17) | (1 << 18) | (1 << 21) | (1 << 22)));
    let c = zeros(c, 16).color(0, 0, 0, 255).f32(0.0).i32(0).u32(0).f32(0.0).f32(0.0).i32(0).i32(0);
    let c = zeros(c, 4).u32(0).f32(1.0).f32(1.0).f32(0.0).f32(0.0).f32(1.0).f32(0.0).f32(0.0).u64(0);
    let list = decode_list(&c.s, &caps).expect("a command with every member at its default decodes");
    assert_eq!(dump_list(&list), dump_list(&[Command::default()]), "it decodes to the defaults");
    let mut again = Vec::new();
    assert!(encode_list(&list, &caps, &mut again));
    assert_eq!(again, Bytes::new().head(1).u32(1).u8(0).u32(0).s, "and encodes again without them");

    let p = Bytes::new().head(1).u32(1).u8(11).u32(1 << 17).u32(0).u32(0x01ffffff & !((1 << 2) | (1 << 3) | (1 << 14)));
    let p = zeros(p.u32(0).u32(0).color(0, 0, 0, 255).i32(0), 7);
    let p = zeros(p.u32(0).color(0, 0, 0, 255).f32(1.0).i32(0).i32(0).f32(4.0).u32(0).f32(0.0).i32(0), 7).u32(0);
    let list = decode_list(&p.s, &caps).expect("a path with every member at its default decodes");
    let defaults = Command { kind: Cmd::Path, path: Some(Arc::new(Path::default())), ..Command::default() };
    assert_eq!(dump_list(&list), dump_list(&[defaults]), "it decodes to the defaults");
    let mut again = Vec::new();
    assert!(encode_list(&list, &caps, &mut again));
    assert_eq!(again, Bytes::new().head(1).u32(1).u8(11).u32(1 << 17).u32(0).u32(0).s, "and encodes again without them");

    let bools_and_payloads: u64 = (1 << 1) | (1 << 16) | (1 << 23) | (1 << 34) | (1 << 35);
    let o = Bytes::new().head(3).u32(1).u8(1).u64(((1u64 << 38) - 1) & !bools_and_payloads);
    let o = zeros(o.u32(0).u32(0).u8(0).u8(7).color(0, 0, 0, 255), 6);
    let o = o
        .u32(0)
        .f32(1.0)
        .f32(0.0)
        .f32(0.0)
        .f32(1.0)
        .f32(0.0)
        .f32(0.0)
        .f32(1.0)
        .i32(0)
        .i32(0)
        .f32(10.0)
        .u32(0)
        .f32(0.0)
        .f32(1.0)
        .u8(0)
        .color(0, 0, 0, 0);
    let o = zeros(o, 3).i32(0).u32(0).f32(0.0).f32(0.0).f32(1.0).f32(0.0).i32(0).i32(0).f32(0.0).f32(0.0).f32(-1.0).f32(-1.0);
    let o = zeros(o, 4).i32(0).i32(0);
    let batch = decode_batch(&o.s, &caps).expect("an op with every member at its default decodes");
    assert_eq!(dump_batch(&batch), dump_batch(&[CanvasOp::default()]), "it decodes to the defaults");
    let mut again = Vec::new();
    assert!(encode_batch(&batch, &caps, &mut again));
    assert_eq!(again, Bytes::new().head(3).u32(1).u8(1).u64(0).s, "and encodes again without them");
}

/// What a value refers to is in that value: the tables of payloads start empty in each.
#[test]
fn tables_per_value() {
    let caps = Caps::default();
    let list = named(&list_samples(), PAYLOADS).clone();
    let mut first = Vec::new();
    assert!(encode_list(&list, &caps, &mut first));
    let mut second = Vec::new();
    assert!(encode_list(&list, &caps, &mut second));
    assert_eq!(first, second, "a list encodes the same again: its table does not carry over");
    let batch = named(&batch_samples(), PAYLOADS).clone();
    let mut first = Vec::new();
    assert!(encode_batch(&batch, &caps, &mut first));
    let mut second = Vec::new();
    assert!(encode_batch(&batch, &caps, &mut second));
    assert_eq!(first, second, "a batch encodes the same again");
    let only_ref = Bytes::new().head(1).u32(1).u8(11).u32(1 << 17).u32(1).s;
    refused(FormKind::List, "a reference to another value's path", &only_ref, &caps);
}

#[test]
fn affine_and_em_box() {
    let m = Affine { a: 2.0, b: 0.0, c: 0.0, d: 4.0, e: 1.0, f: -1.0 };
    let inv = m.invert().expect("invertible");
    let id = m * inv;
    assert_eq!((id.a, id.b, id.c, id.d, id.e, id.f), (1.0, 0.0, 0.0, 1.0, 0.0, 0.0));
    assert!(Affine { a: 1.0, b: 2.0, c: 2.0, d: 4.0, e: 0.0, f: 0.0 }.invert().is_none(), "a flat map has no inverse");
    assert!(Affine { a: f32::NAN, ..Affine::IDENTITY }.invert().is_none(), "a NaN determinant has no inverse");
    assert_eq!(Affine::translate(3.0, 4.0).apply_x(1.0, 1.0), 4.0);
    let mut fm = FontMetrics::default();
    assert!(set_em_box(&mut fm, 1638.0, 410.0, 16.0));
    assert_eq!(fm.em_ascent, (1638.0f32 * 16.0 / 2048.0 * 64.0).round() / 64.0);
    assert_eq!(fm.em_ascent + fm.em_descent, 16.0);
    assert!(!set_em_box(&mut fm, -1.0, 2.0, 16.0));
    assert!(!set_em_box(&mut fm, 0.0, 0.0, 16.0));
}

/// The starts an encoder notes are where each command begins in what it wrote, counted from the list's first byte after
/// a prefix too: the first commands alone encode to the same bytes up to the next start, shared payloads and references
/// back to them included; a refusal leaves the bytes and the starts as they were (wire_tests.cpp's `command_starts`;
/// its refusal of an unknown kind has no Rust counterpart, a Rust enum cannot hold one).
#[test]
fn command_starts() {
    const EARLIER: &[u8] = b"earlier bytes";
    let path = full_path(3.0);
    let repeated: DisplayList = (0..6)
        .map(|i| Command {
            kind: if i % 3 != 0 { Cmd::Path } else { Cmd::FillRect },
            rect: Rect::new(1.0, 2.0, 3.0, 4.0),
            path: (i % 3 != 0).then(|| Arc::clone(&path)),
            ..Command::default()
        })
        .collect();
    let mut all: Vec<(String, DisplayList)> = vec![("the same commands again, sharing a path".into(), repeated)];
    for s in list_samples() {
        all.push((s.name.to_string(), s.value));
    }
    let caps = Caps::default();
    for (name, list) in &all {
        let what = format!("list {name}");
        let mut plain = Vec::new();
        assert!(encode_list(list, &caps, &mut plain), "{what}: encodes");
        for before in [&b""[..], EARLIER] {
            let place = if before.is_empty() { "" } else { " after a prefix" };
            let mut out = before.to_vec();
            let mut starts = vec![99];
            let ok = encode_list_with_starts(list, &caps, &mut out, &mut starts, usize::MAX);
            assert!(ok && out == [before, &plain].concat(), "{what}: writes the same bytes{place}");
            assert!(
                starts.len() == list.len() + 1 && starts[0] == 6 && *starts.last().unwrap() == plain.len(),
                "{what}: a start for each command, the first after the list's head, and the list's end{place}"
            );
            for i in 0..list.len() {
                let mut first = Vec::new();
                assert!(encode_list(&list[..i + 1], &caps, &mut first));
                assert!(
                    starts[i + 1] == first.len() && plain[starts[i]..starts[i + 1]] == first[starts[i]..],
                    "{what}: command {i} lies between its starts as the first commands alone encode it{place}"
                );
            }
            let (mut within, mut past, mut head) = (vec![99], vec![99], vec![99]);
            let (mut at_most, mut short_by_one, mut headless) = (before.to_vec(), before.to_vec(), before.to_vec());
            assert!(
                encode_list_with_starts(list, &caps, &mut at_most, &mut within, plain.len())
                    && at_most == [before, &plain].concat()
                    && within == starts,
                "{what}: a list as long as the bytes given keeps its starts{place}"
            );
            assert!(
                encode_list_with_starts(list, &caps, &mut short_by_one, &mut past, plain.len() - 1)
                    && short_by_one == [before, &plain].concat()
                    && past.is_empty(),
                "{what}: a list a byte longer is written the same with no starts{place}"
            );
            assert!(
                encode_list_with_starts(list, &caps, &mut headless, &mut head, 5)
                    && headless == [before, &plain].concat()
                    && head.is_empty(),
                "{what}: so is any list given fewer bytes than its head{place}"
            );
        }
    }
    let mut one = Caps::default();
    one.commands = 1;
    let sentinel = vec![7, 8];
    let mut out = EARLIER.to_vec();
    let mut starts = sentinel.clone();
    assert!(
        !encode_list_with_starts(&vec![Command::default(); 2], &one, &mut out, &mut starts, usize::MAX)
            && out == EARLIER
            && starts == sentinel,
        "a list past its command cap is refused, the bytes and the starts kept"
    );
}
