//! tests/core_tests.cpp's compositor checks, on the lists and tracks the renderer gives these functions there:
//! sibling clips, a spinner's loop baked and resumed, a Web Animation's loop, and the damage of a page whose
//! colour, canvas and paint order change. Then what the functions promise besides.

mod common;

use std::sync::Arc;

use twidget_compositor::*;
use twidget_wire::*;

fn fill(x: f32, y: f32, w: f32, h: f32, color: Color) -> Command {
    Command { kind: Cmd::FillRect, rect: Rect::new(x, y, w, h), color, ..Command::default() }
}

fn spin(t: f64, key: &mut LayerKey) -> bool {
    // rotate(360deg · t) about the centre (40, 40) of the box at (20, 20), 40 × 40.
    let angle = (std::f64::consts::TAU * t) as f32;
    let (c, s) = (angle.cos(), angle.sin());
    key.matrix = Affine::translate(40.0, 40.0) * Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 } * Affine::translate(-40.0, -40.0);
    true
}

fn spin_track() -> LayerTrack {
    let mut keys = Vec::new();
    assert!(bake_track(spin, &Rect::new(20.0, 20.0, 40.0, 40.0), &mut keys));
    LayerTrack { start: 0.0, duration: 1.0, repeat: true, keys }
}

#[test]
fn sibling_clips_all_go_their_content_kept_in_order() {
    let mut list = DisplayList::new();
    for i in 0..1000 {
        list.push(Command { kind: Cmd::ClipPush, rect: Rect::new(i as f32, 0.0, 10.0, 10.0), ..Command::default() });
        list.push(Command { kind: Cmd::FillRect, rect: Rect::new(i as f32 + 2.0, 2.0, 2.0, 2.0), width: i as f32, ..Command::default() });
        list.push(Command { kind: Cmd::ClipPop, ..Command::default() });
    }
    let gone = drop_inert_clips(&mut list);
    let ordered = list.len() == 1000 && list.iter().enumerate().all(|(i, c)| c.kind == Cmd::FillRect && c.width == i as f32);
    assert!(gone == 1000 && ordered, "1,000 sibling clips around their content all go, the content kept in order");
}

#[test]
fn a_spinning_loop_is_baked_and_resumed() {
    let track = spin_track();
    assert!(track.repeat && (track.duration - 1.0).abs() < 1e-6 && track.keys.len() >= 3, "one second, looping, {} keys", track.keys.len());
    // A quarter turn about the centre (40, 40) takes the top left corner (20, 20) to the top right.
    let at = |keys: &[LayerKey], t: f32| {
        for w in keys.windows(2) {
            if t < w[0].t || t > w[1].t {
                continue;
            }
            let u = (t - w[0].t) / (w[1].t - w[0].t);
            let (a, b) = (&w[0].matrix, &w[1].matrix);
            return (
                (1.0 - u) * a.apply_x(20.0, 20.0) + u * b.apply_x(20.0, 20.0),
                (1.0 - u) * a.apply_y(20.0, 20.0) + u * b.apply_y(20.0, 20.0),
            );
        }
        (0.0, 0.0)
    };
    let (x, y) = at(&track.keys, 0.25);
    assert!((x - 60.0).abs() < 0.2 && (y - 20.0).abs() < 0.2, "a quarter of the track is a quarter turn about the centre ({x}, {y})");

    let (resumed, span) = resume_track(&track, 0.25 + 3.0);
    let (front, back) = (resumed[0], resumed[resumed.len() - 1]);
    assert!(
        span == 1.0
            && front.t == 0.0
            && back.t == 1.0
            && (front.matrix.apply_x(20.0, 20.0) - 60.0).abs() < 0.2
            && back.matrix == front.matrix,
        "resumed 3.25 s in, the loop starts and ends at the quarter turn"
    );
    let once = LayerTrack { repeat: false, ..track };
    let (rest, span) = resume_track(&once, 0.5);
    assert!(
        (span - 0.5).abs() < 1e-9 && rest[0].t == 0.0 && (rest[0].matrix.apply_x(20.0, 20.0) - 60.0).abs() < 0.2,
        "a one-shot resumed half way keeps its second half"
    );
    let (held, span) = resume_track(&once, 2.0);
    assert!(
        held.len() == 1 && held[0].t == 0.0 && held[0].matrix == once.keys[once.keys.len() - 1].matrix && span == 1.0,
        "past its end it holds the last key"
    );
}

#[test]
fn a_web_animation_loop_is_baked_from_0_to_100_px() {
    let mut keys = Vec::new();
    let baked = bake_track(
        |t, key| {
            key.matrix = Affine::translate((100.0 * t) as f32, 0.0);
            true
        },
        &Rect::new(0.0, 0.0, 40.0, 40.0),
        &mut keys,
    );
    let rising = keys.windows(2).all(|w| w[0].matrix.e <= w[1].matrix.e);
    assert!(
        baked && rising && keys[0].matrix.e.abs() < 0.01 && (keys[keys.len() - 1].matrix.e - 100.0).abs() < 0.05,
        "its loop is baked from 0 to 100 px ({} keys)",
        keys.len()
    );
    assert_eq!(keys.len(), 9, "a straight line keeps the ends of the eight first spans: only equal keys merge");
}

#[test]
fn a_track_that_cannot_be_sampled_or_jumps_too_often_is_refused() {
    let mut keys = Vec::new();
    let mut calls = 0;
    assert!(
        !bake_track(
            |_, _| {
                calls += 1;
                calls < 5
            },
            &Rect::new(0.0, 0.0, 10.0, 10.0),
            &mut keys
        ),
        "a failing sample fails the track"
    );
    // Noise: every span splits down to the last level.
    let noise = |t: f64, key: &mut LayerKey| {
        let mut z = t.to_bits().wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        key.opacity = ((z ^ (z >> 31)) >> 54) as f32 / 1024.0;
        true
    };
    assert!(!bake_track(noise, &Rect::new(0.0, 0.0, 10.0, 10.0), &mut keys), "more than 1,024 keys are refused");
    assert_eq!(keys.len(), 1025, "the keys baked before the refusal stay");
    let flip = |t: f64, key: &mut LayerKey| {
        key.opacity = if t < 0.5 { 1.0 } else { 0.0 };
        true
    };
    assert!(bake_track(flip, &Rect::new(0.0, 0.0, 10.0, 10.0), &mut keys));
    let ts: Vec<f32> = keys.iter().map(|k| k.t).collect();
    // As in the C++, the run that ends the track keeps the key before its last one too.
    assert_eq!(ts, [0.0, 0.5 - 1.0 / 4096.0, 0.5, 0.875, 1.0], "a flip is a short ramp between two holds");
}

fn page() -> DisplayList {
    vec![
        fill(10.0, 10.0, 20.0, 20.0, Color::rgba(255, 0, 0, 255)),
        fill(100.0, 10.0, 20.0, 20.0, Color::rgba(0, 0, 255, 255)),
        fill(105.0, 15.0, 20.0, 20.0, Color::rgba(0, 128, 0, 255)),
        Command { kind: Cmd::Image, rect: Rect::new(200.0, 10.0, 10.0, 10.0), canvas: 8, ..Command::default() },
    ]
}

fn covers(rects: &[Rect], x: f32, y: f32, w: f32, h: f32) -> bool {
    rects.iter().any(|r| r.x <= x && r.y <= y && r.x + r.w >= x + w && r.y + r.h >= y + h)
}

fn within(rects: &[Rect], x: f32, y: f32, w: f32, h: f32) -> bool {
    rects.iter().all(|r| r.x >= x && r.y >= y && r.x + r.w <= x + w && r.y + r.h <= y + h)
}

#[test]
fn damage_is_where_a_paint_changed() {
    let before = summarize(&page());
    let mut rects = Vec::new();
    assert!(damage_between(&before, &before, None, &mut rects) && rects.is_empty(), "an unchanged paint damages nothing");

    let mut yellow = page();
    yellow[0].color = Color::rgba(255, 255, 0, 255);
    assert!(damage_between(&before, &summarize(&yellow), None, &mut rects));
    assert!(
        covers(&rects, 10.0, 10.0, 20.0, 20.0) && within(&rects, 5.0, 5.0, 30.0, 30.0),
        "a new colour damages its box and nothing else"
    );

    let mut asked = Vec::new();
    let mut drew = |canvas: i32| {
        asked.push(canvas);
        canvas == 8
    };
    assert!(damage_between(&before, &before, Some(&mut drew), &mut rects));
    assert!(covers(&rects, 200.0, 10.0, 10.0, 10.0) && within(&rects, 195.0, 5.0, 20.0, 20.0), "a canvas that drew damages itself");
    assert_eq!(asked, [8], "only the canvas's command is asked about");

    let mut swapped = page();
    swapped.swap(1, 2);
    assert!(damage_between(&before, &summarize(&swapped), None, &mut rects));
    assert!(covers(&rects, 105.0, 15.0, 15.0, 15.0), "boxes that swap paint order damage where they overlap");

    let many: DisplayList = (0..40).map(|i| fill(i as f32 * 10.0, 0.0, 5.0, 5.0, Color::BLACK)).collect();
    let moved: DisplayList = (0..40).map(|i| fill(i as f32 * 10.0, 1.0, 5.0, 5.0, Color::BLACK)).collect();
    assert!(damage_between(&summarize(&many), &summarize(&moved), None, &mut rects));
    assert_eq!(rects, [Rect::new(-1.0, -1.0, 397.0, 8.0)], "more than 32 areas are merged into the one around them");
}

#[test]
fn a_backdrop_over_damage_is_damaged_whole() {
    let mut list = page();
    list.push(Command { kind: Cmd::Backdrop, rect: Rect::new(0.0, 0.0, 50.0, 50.0), ..Command::default() });
    let before = summarize(&list);
    list[0].color = Color::rgba(1, 2, 3, 255);
    let mut rects = Vec::new();
    assert!(damage_between(&before, &summarize(&list), None, &mut rects));
    assert!(covers(&rects, -1.0, -1.0, 52.0, 52.0), "the backdrop filters what changed under it: {rects:?}");
}

#[test]
fn a_summary_keeps_the_context_of_each_command() {
    let inner = fill(0.0, 0.0, 10.0, 10.0, Color::BLACK);
    let group = |hidden: bool, opacity: f32| Command {
        kind: Cmd::GroupPush,
        matrix: [2.0, 0.0, 0.0, 2.0, 5.0, 5.0],
        hidden,
        opacity,
        ..Command::default()
    };
    let pop = Command { kind: Cmd::GroupPop, ..Command::default() };
    let plain = summarize(std::slice::from_ref(&inner));
    let grouped = summarize(&[group(false, 1.0), inner.clone(), pop.clone()]);
    assert_eq!(grouped.entries.len(), 1, "pushes and pops are no entries");
    assert_ne!(grouped.entries[0].key, plain.entries[0].key, "a command in a group is another command");
    assert_eq!(grouped.entries[0].ink, Rect::new(3.0, 3.0, 24.0, 24.0), "its ink is in document coordinates");
    for (hidden, opacity) in [(true, 1.0), (false, 0.0)] {
        let s = summarize(&[group(hidden, opacity), inner.clone(), pop.clone()]);
        assert_eq!(s.entries[0].ink, Rect::default(), "a hidden or transparent group paints nowhere");
    }
    let filtered =
        summarize(&[Command { kind: Cmd::FilterPush, rect: Rect::new(0.0, 0.0, 100.0, 100.0), ..Command::default() }, inner, pop]);
    assert_eq!(filtered.entries[0].ink, Rect::new(-1.0, -1.0, 102.0, 102.0), "inside a filter a change may change all it paints");
}

#[test]
fn a_digest_is_of_what_a_list_paints() {
    assert_eq!(paint_digest(&[]), 1469598103934665603, "an empty list is FNV's offset basis");
    let list = page();
    assert_eq!(paint_digest(&list), paint_digest(&list.clone()));
    let mut keyed = list.clone();
    keyed[0].key = 7;
    assert_eq!(paint_digest(&keyed), paint_digest(&list), "a group's raster key paints nothing");
    let mut zero = list.clone();
    zero[0].rect.x = -0.0;
    let mut zero_too = list.clone();
    zero_too[0].rect.x = 0.0;
    assert_ne!(paint_digest(&zero), paint_digest(&zero_too), "floats are their bits");
    let mut path = list.clone();
    path[1].path = Some(Arc::new(Path::default()));
    assert_ne!(paint_digest(&path), paint_digest(&list), "a path, even an empty one, is part of it");
    let mut filtered = list.clone();
    filtered[1].filter = Some(Arc::new(Filters::new()));
    assert_eq!(paint_digest(&filtered), paint_digest(&list), "an empty filter list adds no bytes, as in the C++");
}

#[test]
fn clips_that_change_pixels_stay() {
    let clip = |rect: Rect, radii: Radii| Command { kind: Cmd::ClipPush, rect, radii, ..Command::default() };
    let pop = Command { kind: Cmd::ClipPop, ..Command::default() };
    let r = 4.0;
    let round = Radii { tlx: r, tly: r, trx: r, try_: r, brx: r, bry: r, blx: r, bly: r };
    let mut list = vec![clip(Rect::new(0.0, 0.0, 20.0, 20.0), Radii::default()), fill(0.0, 0.0, 30.0, 5.0, Color::BLACK), pop.clone()];
    assert_eq!(drop_inert_clips(&mut list), 0, "content past the clip keeps it");
    let mut list = vec![clip(Rect::new(0.0, 0.0, 20.0, 20.0), round), fill(4.0, 4.0, 12.0, 12.0, Color::BLACK), pop.clone()];
    assert_eq!(drop_inert_clips(&mut list), 0, "content in a rounded corner's reach keeps it (the ink grows by 1 px)");
    let mut list = vec![clip(Rect::new(0.0, 0.0, 20.0, 20.0), round), fill(6.0, 6.0, 8.0, 8.0, Color::BLACK), pop.clone()];
    assert_eq!(drop_inert_clips(&mut list), 1, "content within the corners' inset drops it");
    let rotated = Command { kind: Cmd::GroupPush, matrix: [0.0, 1.0, -1.0, 0.0, 0.0, 0.0], ..Command::default() };
    let group_pop = Command { kind: Cmd::GroupPop, ..Command::default() };
    let mut list = vec![
        rotated,
        clip(Rect::new(0.0, 0.0, 20.0, 20.0), Radii::default()),
        fill(5.0, 5.0, 2.0, 2.0, Color::BLACK),
        pop.clone(),
        group_pop,
    ];
    assert_eq!(drop_inert_clips(&mut list), 0, "a clip under a rotation stays");
    let mut path_clip = clip(Rect::new(0.0, 0.0, 20.0, 20.0), Radii::default());
    path_clip.path = Some(Arc::new(Path::default()));
    let mut list = vec![path_clip, fill(5.0, 5.0, 2.0, 2.0, Color::BLACK), pop];
    assert_eq!(drop_inert_clips(&mut list), 0, "a clip path stays");
}
