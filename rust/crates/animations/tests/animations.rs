//! tests/core_tests.cpp's animation checks, each on what the animator gives the renderer there: the declarations the
//! renderer reads out of its document after a build, the times it samples at, the `anim` messages it forwards with the
//! clock offset it computes, and the answers it turns into events, inline styles, layers, the `anims` query and the
//! heavy-box count. Then what the port promises besides.

use std::collections::BTreeMap;

use twidget_animations::*;
use twidget_json::Json;

/// core_tests.cpp's PAGE style sheet.
const PAGE_CSS: &str = "#a{width:100px;height:100px;float:left;color:light-dark(#010203,#040506)} #a1{width:50px;height:50px} #b{width:100px;height:100px;float:left} @keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .spin{animation:spin 1s linear infinite} .t{transition:opacity 200ms linear}";

fn declared(pairs: &[(&str, &str)]) -> Declared {
    pairs.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect()
}

fn build(a: &mut Animator, elements: &[(i64, &[(&str, &str)])], now: f64) -> Vec<AnimationEvent> {
    let all: BTreeMap<i64, Declared> = elements.iter().map(|(id, pairs)| (*id, declared(pairs))).collect();
    a.update(&all, now);
    a.take_events()
}

fn value<'a>(values: &'a StyleValues, property: &str) -> Option<&'a str> {
    values.iter().rev().find(|(p, _)| p == property.as_bytes()).map(|(_, v)| std::str::from_utf8(v).expect("ASCII"))
}

fn sampled(out: &StyleOverrides, id: i64, property: &str) -> Option<String> {
    out.get(&id).and_then(|v| value(v, property)).map(str::to_string)
}

fn types(events: &[AnimationEvent]) -> String {
    events.iter().map(|e| format!("{}:{} ", e.kind.name(), e.target)).collect()
}

fn message(text: &str) -> Json {
    Json::parse(text).expect("a JSON message")
}

/// What the renderer hands `apply_web`: its clock in milliseconds less the message's.
fn apply(a: &mut Animator, text: &str, now: f64) {
    let m = message(text);
    let offset = now * 1000.0 - m.get("now").as_number(now * 1000.0);
    a.apply_web(&m, offset);
}

fn number(v: &str) -> f64 {
    v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == ')')
        .trim_start_matches(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
        .parse()
        .expect("a number")
}

// ── CSS animation ──
#[test]
fn css_animation() {
    let mut a = Animator::new();
    a.set_style_sheets(&[PAGE_CSS]);
    let t0 = 5.0;
    let events = build(&mut a, &[(12, &[("animation", "spin 1s linear infinite")])], t0);
    assert!(types(&events).contains("animationstart:12"), "adding the class starts the animation (got {})", types(&events));
    let out = a.sample(t0);
    assert_eq!(sampled(&out, 12, "transform").as_deref(), Some("rotate(0deg)"), "the sampled transform is written");
    assert!(a.active(t0), "an infinite animation keeps the clock ticking");
    let later = a.sample(t0 + 0.12);
    assert_eq!(sampled(&later, 12, "transform").as_deref(), Some("rotate(43.2deg)"), "a frame later it has turned");
}

// ── transition ──
#[test]
fn transition() {
    let mut a = Animator::new();
    a.set_style_sheets(&[PAGE_CSS]);
    let t = [("transition", "opacity 200ms linear")];
    build(&mut a, &[(11, &[t[0], ("opacity", "1")])], 1.0);
    build(&mut a, &[(11, &[t[0], ("opacity", "0")])], 1.0);
    let half = a.sample(1.1);
    assert_eq!(sampled(&half, 11, "opacity").as_deref(), Some("0.5"), "half way the opacity is half way");
    assert!(a.take_events().is_empty());
    a.sample(1.25);
    let events = a.take_events();
    assert!(types(&events).contains("transitionend:11"), "the opacity transition ends with transitionend (got {})", types(&events));
    assert_eq!(events[0].name, b"opacity");
    assert_eq!(events[0].elapsed, 0.2);
    assert!(!a.active(1.3), "and nothing moves after it");
}

// ── Web Animations ──
#[test]
fn web_animations() {
    let mut a = Animator::new();
    a.set_style_sheets(&[PAGE_CSS]);
    let now = 10.0;
    build(&mut a, &[(12, &[("animation", "spin 1s linear infinite")])], now);
    apply(
        &mut a,
        r#"{"type":"anim","now":0,"anims":[{"aid":1,"target":10,"keyframes":[{"offset":0,"values":{"opacity":"0"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"both"},"startTime":-500}]}"#,
        now,
    );
    let out = a.sample(now);
    assert_eq!(sampled(&out, 10, "opacity").as_deref(), Some("0.5"), "a Web Animation half way through samples opacity 0.5");
    assert_eq!(a.web_targets(), vec![10]);
    apply(&mut a, r#"{"type":"anim","now":0,"anims":[{"aid":1,"remove":true}]}"#, now);
    let out = a.sample(now);
    assert_eq!(sampled(&out, 10, "opacity"), None, "removing the record drops its value");
    let list = a.css_animations(&[12], now);
    assert!(
        list.arr().len() == 1 && list.arr()[0].get("name").as_str() == b"spin",
        "the anims query reports the running CSS animation: {}",
        String::from_utf8_lossy(&list.dump())
    );
}

const LAYERS_CSS: &str = "#s{width:40px;height:40px;margin:20px;background:#ff0000;animation:spin 1s linear infinite} #t{height:30px;background:#00ff00;transition:opacity 400ms linear} #w{height:10px;background:#0000ff;animation:grow 1s infinite} @keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} @keyframes grow{from{width:10px}to{width:90px}}";

// ── compositor layers ──
#[test]
fn compositor_layers() {
    let mut a = Animator::new();
    a.set_style_sheets(&[LAYERS_CSS]);
    let now = 2.0;
    let spin: &[(&str, &str)] = &[("animation", "spin 1s linear infinite")];
    let fade: &[(&str, &str)] = &[("transition", "opacity 400ms linear")];
    build(&mut a, &[(40, spin), (41, fade)], now);
    a.sample(now);
    assert_eq!(a.animated_ids(now), vec![40], "the endless rotation is the one animated element");
    let plan = a.plan(40, now, false).expect("a plan for the rotation");
    assert!(plan.ready && plan.repeat && (plan.duration - 1.0).abs() < 1e-6, "one second, looping: {plan:?}");
    assert_eq!(plan.start, now, "from the loop's start");
    let quarter = a.peek(40, plan.start + 0.25);
    assert_eq!(value(&quarter, "transform"), Some("rotate(90deg)"), "a quarter of the track is a quarter turn");
    assert!(!a.active_except(now, &[40].into_iter().collect()), "a layer alone needs no frames");

    // The opacity transition after an animationiteration listener: the core wakes for the next iteration.
    let fade_from: &[(&str, &str)] = &[("transition", "opacity 400ms linear"), ("opacity", "1")];
    let fade_to: &[(&str, &str)] = &[("transition", "opacity 400ms linear"), ("opacity", "0")];
    build(&mut a, &[(40, spin), (41, fade_from)], now);
    build(&mut a, &[(40, spin), (41, fade_to)], now + 0.01);
    let t = now + 0.01;
    let plan = a.plan(41, t, true).expect("a plan for the transition");
    assert!(!plan.repeat && (plan.duration - 0.4).abs() < 0.05, "it plays once, over what is left of 400 ms: {plan:?}");
    let first = number(value(&a.peek(41, plan.start), "opacity").expect("opacity at its start"));
    let last = number(value(&a.peek(41, plan.start + plan.duration * 0.999), "opacity").expect("opacity at its end"));
    assert!(first > 0.9 && last < 0.1, "1 to 0 ({first} to {last})");
    let wake = a.plan(40, t, true).expect("the rotation's plan").wake.min(plan.wake) - t;
    assert!(wake > 0.2 && wake < 1.01, "the core wakes for the next animationiteration or the transition's end ({wake} s)");

    // Width cannot be composited.
    build(&mut a, &[(40, spin), (41, fade_to), (43, &[("animation", "grow 1s infinite")])], t);
    assert_eq!(a.animated_properties(43), vec![b"width".to_vec()], "a width animation animates width alone");
    let out = a.sample(t + 0.5);
    assert!(sampled(&out, 43, "width").is_some_and(|w| w.ends_with("px")), "and is sampled into the document");
}

// ── a finished animation inside an element does not keep it out of layers ──
#[test]
fn finished_animation_inside_an_element() {
    let mut a = Animator::new();
    a.set_style_sheets(&[".card{margin:30px;height:60px} .like{width:20px;height:20px} .pop{animation:pop 100ms} .slide{animation:slide 1s} @keyframes pop{from{transform:scale(1.7)}} @keyframes slide{from{opacity:0;transform:translateX(28px)}}"]);
    build(&mut a, &[(80, &[]), (81, &[("animation", "pop 100ms")])], 0.0);
    a.sample(0.0);
    a.sample(0.15);
    assert!(types(&a.take_events()).contains("animationend:81"));
    build(&mut a, &[(80, &[("animation", "slide 1s")]), (81, &[("animation", "pop 100ms")])], 0.15);
    a.sample(0.15);
    assert_eq!(a.animated_ids(0.15), vec![80], "a pop that has ended inside the card leaves the card's slide alone");
}

// ── animation-play-state ──
#[test]
fn animation_play_state() {
    let mut a = Animator::new();
    a.set_style_sheets(&[PAGE_CSS]);
    let current = |a: &Animator, now: f64| {
        let list = a.css_animations(&[12], now);
        if list.arr().is_empty() { -1.0 } else { list.arr()[0].get("currentTime").as_number(0.0) }
    };
    let spin = ("animation", "spin 1s linear infinite");
    build(&mut a, &[(12, &[spin])], 1.0);
    build(&mut a, &[(12, &[spin, ("animation-play-state", "paused")])], 1.06);
    let paused = current(&a, 1.06);
    assert!(paused >= 50.0 && current(&a, 1.14) == paused, "a paused animation's time stands still ({paused} then {})", current(&a, 1.14));
    build(&mut a, &[(12, &[spin, ("animation-play-state", "running")])], 1.14);
    let resumed = current(&a, 1.14);
    assert!(resumed >= paused && resumed < paused + 40.0, "resumed, it carries on from where it paused ({resumed})");
    assert!((current(&a, 1.24) - (paused + 100.0)).abs() < 1e-6);
}

// ── Web Animations on the compositor ──
#[test]
fn web_animations_on_the_compositor() {
    let mut a = Animator::new();
    let now = 0.3;
    apply(
        &mut a,
        r#"{"type":"anim","now":0,"anims":[{"aid":1,"target":70,"keyframes":[{"offset":0,"values":{"transform":"translateX(0px)"}},{"offset":1,"values":{"transform":"translateX(100px)"}}],"timing":{"duration":1000,"iterations":null,"easing":"linear"},"startTime":0}]}"#,
        now,
    );
    let later = now + 0.45;
    assert_eq!(a.animated_ids(later), vec![70]);
    let plan = a.plan(70, later, false).expect("a plan for the Web Animation");
    assert!(plan.repeat && (plan.duration - 1.0).abs() < 1e-6, "{plan:?}");
    assert!((plan.start - now).abs() < 1e-9, "baked from the loop's start ({})", plan.start);
    let xs: Vec<f64> = (0..=10)
        .map(|k| {
            let t = if k == 10 { 1.0 - 1e-6 } else { f64::from(k) / 10.0 };
            number(value(&a.peek(70, plan.start + t * plan.duration), "transform").expect("a transform"))
        })
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] <= w[1]) && xs[0].abs() < 0.01 && (xs[10] - 100.0).abs() < 0.05,
        "its loop is baked from 0 to 100 px ({xs:?})"
    );
    assert!(!a.active_except(later, &[70].into_iter().collect()), "and the document needs no frames for it");
}

// ── nested layers ──
#[test]
fn nested_layers() {
    let mut a = Animator::new();
    a.set_style_sheets(&["@keyframes spin{to{transform:rotate(360deg)}} .orbit{margin:40px;width:100px;height:100px;animation:spin 4s linear infinite} .moon{width:20px;height:20px;background:red;animation:spin 1s linear infinite}"]);
    build(&mut a, &[(30, &[("animation", "spin 4s linear infinite")]), (31, &[("animation", "spin 1s linear infinite")])], 0.0);
    assert_eq!(a.animated_ids(0.0), vec![30, 31], "a spinner inside a spinner: both animate");
    let (outer, inner) = (a.plan(30, 0.0, false).expect("the orbit's plan"), a.plan(31, 0.0, false).expect("the moon's plan"));
    assert!(outer.repeat && outer.duration == 4.0 && inner.repeat && inner.duration == 1.0, "{outer:?} {inner:?}");
    assert_eq!(value(&a.peek(31, 0.5), "transform"), Some("rotate(180deg)"), "a to frame alone turns from none");
}

// ── what a build in paint() says waits for the next tick ──
#[test]
fn a_short_animation_ends() {
    let mut a = Animator::new();
    a.set_style_sheets(&["@keyframes f{from{opacity:0}to{opacity:1}} .a{animation:f 0.05s}"]);
    build(&mut a, &[(10, &[("animation", "f 0.05s")])], 0.0);
    a.sample(0.0);
    assert!(a.active(0.0), "while the animation runs, frames are due");
    a.sample(0.12);
    assert!(types(&a.take_events()).contains("animationend:10"));
    assert!(!a.active(0.12), "once it has ended, none are");
}

// ── :hover and :active start their transitions at once ──
#[test]
fn hover_and_active_start_their_transitions() {
    let mut a = Animator::new();
    let t = ("transition", "opacity 1s");
    build(&mut a, &[(10, &[t, ("opacity", "1")])], 0.0);
    let idle = !a.active(0.0);
    build(&mut a, &[(10, &[t, ("opacity", "0")])], 0.1);
    assert!(idle && a.active(0.1), "a restyle starts its transition");
}

// ── animated computed style, ResizeObserver, <details>, animationstart before any listen ──
#[test]
fn animated_computed_style() {
    let mut a = Animator::new();
    a.set_style_sheets(&["@keyframes slide{from{transform:translateX(0)}to{transform:translateX(40px)}} #pk{width:20px;height:20px;animation:slide 10s linear -2.5s paused}"]);
    let events = build(&mut a, &[(10, &[("animation", "slide 10s linear -2.5s paused")])], 0.0);
    assert!(types(&events).contains("animationstart:10"), "animationstart goes out at once (got {})", types(&events));
    let tx = |value: &str| twidget_css::parse_transform(value.as_bytes(), 100.0, 20.0).map_or(-1e9, |m| f64::from(m.e));
    for t in [0.0, 3.0] {
        let out = a.sample(t);
        let transform = sampled(&out, 10, "transform").expect("a transform");
        assert_eq!(tx(&transform), 10.0, "a paused animation's frame is a quarter of the way ({transform})");
    }
    // Math on one side only still interpolates (Chromium does), as a sum of the two translations by weight.
    let mut pairs = PairCache::default();
    let quarter = interpolate(&mut pairs, b"translateX(0px)", b"translateX(calc(100% + 10px))", 0.25);
    let from_none = interpolate(&mut pairs, b"none", b"translateX(calc(100% + 10px))", 0.25);
    let back = interpolate(&mut pairs, b"translateX(calc(50% - 1 * (100% + 10px) + 50px))", b"translateX(20px)", 0.5);
    let s = |v: &[u8]| String::from_utf8(v.to_vec()).expect("ASCII");
    let (quarter, from_none, back) = (s(&quarter), s(&from_none), s(&back));
    assert!(
        (tx(&quarter) - 27.5).abs() < 0.01 && (tx(&from_none) - 27.5).abs() < 0.01 && (tx(&back) - 5.0).abs() < 0.01,
        "a transition into or out of calc() moves, not flips (got {quarter} | {from_none} | {back})"
    );
}

// ── a transition retargeted over and over between a length and a percentage ──
#[test]
fn a_transition_retargeted_over_and_over() {
    let mut a = Animator::new();
    let t = ("transition", "transform 1000s linear");
    build(&mut a, &[(10, &[t, ("transform", "translateX(10px)")])], 0.0);
    let mut longest = 0;
    for i in 0..150 {
        let now = f64::from(i + 1) * 0.001;
        let value = if i % 2 == 1 { "translateX(10px)" } else { "translateX(calc(50% + 1px))" };
        build(&mut a, &[(10, &[t, ("transform", value)])], now);
        let out = a.sample(now);
        longest = longest.max(sampled(&out, 10, "transform").map_or(0, |v| v.len()));
    }
    assert!(longest > 0 && longest < 40000, "a transition retargeted 150 times keeps its value bounded (longest {longest} bytes)");
}

// ── an iteration count below zero ──
#[test]
fn an_iteration_count_below_zero() {
    let mut a = Animator::new();
    a.set_style_sheets(&["@keyframes k{from{opacity:0}to{opacity:0.5}}.y{animation:k 1ms -1e300 alternate forwards}"]);
    build(&mut a, &[(10, &[("animation", "k 1ms -1e300 alternate forwards")])], 0.0);
    let out = a.sample(0.0);
    assert_eq!(sampled(&out, 10, "opacity").as_deref(), Some("0.5"), "it ends on its first iteration, forwards");
}

// ── heavy boxes: rule 5 ──
#[test]
fn heavy_boxes_rule_5() {
    let mut a = Animator::new();
    a.set_style_sheets(&[
        "@keyframes fade{from{opacity:1}to{opacity:1}} .late{animation:fade 1s linear 10s} .now{animation:fade 1s linear infinite}",
    ]);
    build(&mut a, &[(46, &[])], 0.0);
    assert!(a.heavy_motion(0.0, None) == 0, "a plain box has no motion");
    build(&mut a, &[(46, &[("animation", "fade 1s linear 10s")])], 0.0);
    let mut motions = Vec::new();
    a.heavy_motion(0.5, Some(&mut motions));
    assert!(motions.is_empty(), "nor while its animation waits out the delay");
    build(&mut a, &[(46, &[("animation", "fade 1s linear infinite")])], 0.5);
    let hash = a.heavy_motion(0.6, Some(&mut motions));
    assert!(
        motions.len() == 1 && motions[0].kind == b'a' && motions[0].paint.heavy && motions[0].name == b"fade",
        "but it is once the animation runs: {motions:?}"
    );
    assert_eq!(a.heavy_motion(0.6, None), hash, "the hash is the same without the list");

    let mut w = Animator::new();
    apply(
        &mut w,
        r#"{"type":"anim","now":0,"anims":[{"aid":1,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"forwards"},"startTime":-5000}]}"#,
        0.0,
    );
    w.heavy_motion(0.0, Some(&mut motions));
    assert!(motions.is_empty(), "a finished Web Animation that holds its end is not counted");
    apply(
        &mut w,
        r#"{"type":"anim","now":0,"anims":[{"aid":2,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"iterations":null},"startTime":0}]}"#,
        0.0,
    );
    w.heavy_motion(0.0, Some(&mut motions));
    assert!(motions.len() == 1 && motions[0].kind == b'w' && motions[0].aid == 2 && motions[0].id == 46, "a running one is: {motions:?}");

    let mut p = Animator::new();
    p.set_style_sheets(&[
        ".h{box-shadow:0 2px 6px rgba(0,0,0,.4)} @keyframes p{to{opacity:.55}} .a{animation:p 1.2s ease-in-out infinite alternate}",
    ]);
    build(&mut p, &[(46, &[("animation", "p 1.2s ease-in-out infinite alternate")])], 0.0);
    p.heavy_motion(0.1, Some(&mut motions));
    assert!(motions.len() == 1 && motions[0].paint.heavy, "an animation with a to frame only: {motions:?}");
}

// ── ids: a number that fits no int names no node ──
#[test]
fn an_animation_id_past_int() {
    let mut a = Animator::new();
    apply(
        &mut a,
        r#"{"type":"anim","now":0,"anims":[{"aid":1e300,"target":10,"keyframes":[{"offset":0,"values":{"opacity":"0"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"both"},"startTime":-500},
            {"aid":7,"target":-1e300,"css":{"kind":"animation","name":"spin","origin":0},"cancelled":true},{"aid":8,"target":1e300,"keyframes":[]}]}"#,
        0.0,
    );
    let animated = sampled(&a.sample(0.0), 10, "opacity").is_some_and(|v| v.starts_with("0."));
    apply(&mut a, r#"{"type":"anim","now":0,"anims":[{"aid":2147483647,"remove":true}]}"#, 0.0);
    assert!(animated && a.sample(0.0).is_empty(), "an animation's id past the range of an int is the end of that range");
    assert!(a.web_targets().is_empty());
}

// ── animations: the cache of interpolated value pairs is bounded by the bytes of their text ──
#[test]
fn the_pair_cache_is_bounded() {
    let mut pairs = PairCache::default();
    interpolate(&mut pairs, b"1px", b"2px", 0.5);
    let mut most = (0, 0);
    for i in 0..600 {
        let (mut x, mut y) = (i.to_string(), (i + 1).to_string());
        for _ in 0..1000 {
            x.push_str(" 1");
            y.push_str(" 2");
        }
        interpolate(&mut pairs, x.as_bytes(), y.as_bytes(), 0.5);
        most = (most.0.max(pairs.bytes()), most.1.max(pairs.len()));
    }
    assert!(most.0 <= 256 * 1024 && most.1 <= 2048, "600 value pairs of 1000 numbers each keep 256 K of pairs at most ({most:?})");
    assert!(most.0 > 200 * 1024, "and do keep that much ({most:?})");
}

// ── besides: what the port promises ──

#[test]
fn events_come_by_element() {
    let mut a = Animator::new();
    a.set_style_sheets(&["@keyframes k{to{opacity:0}}"]);
    let spec = [("animation", "k 1s 2"), ("transition", "opacity 100ms")];
    let ids = [9i64, -3, 1 << 40, 4, 0];
    let elements: Vec<(i64, &[(&str, &str)])> = ids.iter().map(|&id| (id, &spec[..])).collect();
    let starts = build(&mut a, &elements, 0.0);
    assert_eq!(starts.iter().map(|e| e.target).collect::<Vec<_>>(), vec![-3, 0, 4, 9, 1 << 40], "animationstart by element id");
    let with_opacity = [spec[0], spec[1], ("opacity", "0.5")];
    let elements: Vec<(i64, &[(&str, &str)])> = ids.iter().map(|&id| (id, &with_opacity[..])).collect();
    build(&mut a, &elements, 0.0);
    a.sample(1.5);
    let events = a.take_events();
    assert_eq!(
        types(&events),
        "animationiteration:-3 animationiteration:0 animationiteration:4 animationiteration:9 animationiteration:1099511627776 transitionend:-3 transitionend:0 transitionend:4 transitionend:9 transitionend:1099511627776 "
    );
}

#[test]
fn two_pairs_that_join_alike_keep_their_own_cache_entries() {
    // The cache keys a pair by the length of a, a colon, a and b: pairs that join alike are told apart.
    let mut pairs = PairCache::default();
    assert_eq!(interpolate(&mut pairs, b"0px\x01", b"4px\x01", 0.5), b"2px\x01");
    assert_eq!(interpolate(&mut pairs, b"0px", b"\x014px\x01", 0.5), b"\x014px\x01", "the second pair flips, as it does alone");
    assert_eq!(interpolate(&mut pairs, b"10px", b"20px", 0.5), b"15px");
    assert_eq!(interpolate(&mut pairs, b"1", b"0px20px", 0.5), b"0px20px", "nor do pairs whose texts join alike without a separator");
    assert_eq!(pairs.len(), 4);
    assert_eq!(pairs.bytes(), 4 * 10, "each key is its length prefix, a colon and both texts");
}

#[test]
fn timing_functions() {
    let mut timings = TimingCache::default();
    let parse = |timings: &mut TimingCache, t: &str| Timing::parse(timings, t.as_bytes());
    let ease = parse(&mut timings, " EASE ").expect("ease");
    assert_eq!((ease.kind, ease.x1, ease.y1, ease.text.as_slice()), (Kind::Bezier, 0.25, 0.1, &b"ease"[..]));
    let steps = parse(&mut timings, "steps(4, jump-none)").expect("steps");
    assert_eq!((steps.steps, steps.jump), (4, Jump::JumpNone));
    assert_eq!(steps.web(0.5, false), 2.0 / 3.0, "jump-none: four steps over three jumps");
    for refused in ["steps(0)", "steps(2.5)", "steps(100001)", "cubic-bezier(1,2,3)", "linear()", "bounce", "steps(2, sideways)"] {
        assert!(parse(&mut timings, refused).is_none(), "{refused}");
    }
    let points = parse(&mut timings, "linear(0, 0.25 75%, 1)").expect("linear()");
    assert_eq!(points.points, vec![(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)]);
    assert_eq!(points.css(0.375), 0.125);
    let bezier = parse(&mut timings, "cubic-bezier(0.5, -1, 0.5, 2)").expect("a bézier");
    assert_eq!(bezier.web(-1.0, false), 2.0, "before 0 it follows its first tangent");
    assert_eq!(bezier.css(-1.0), 0.0, "CSS clamps it");
    for _ in 0..300 {
        parse(&mut timings, "ease");
    }
    assert!(timings.len() <= 256);
}

#[test]
fn hostile_inputs_end() {
    let mut a = Animator::new();
    let mut sheet = String::from("@keyframes a{");
    for i in 0..2000 {
        sheet.push_str(&format!("{}%{{opacity:{};transform:rotate({i}deg) scale({})}}", i % 101, i % 3, i % 5));
    }
    a.set_style_sheets(&[sheet.as_str(), "@keyframes", "@keyframes {", "@keyframes x{{{{", "keyframes keyframes @keyframes"]);
    let long = "translateX(1px) ".repeat(5000);
    for (k, now) in [0.0, 1e300, -1e300, f64::MAX, 1e-300].into_iter().enumerate() {
        let pairs: Vec<(&str, &str)> = vec![
            ("animation", "a 1s infinite alternate both"),
            ("transition", "all 1s"),
            ("transform", if k % 2 == 0 { long.as_str() } else { "none" }),
        ];
        build(&mut a, &[(1, &pairs), (i64::MAX, &pairs), (i64::MIN, &pairs)], now);
        a.sample(now);
        a.peek(1, now);
        a.plan(1, now, true);
        a.heavy_motion(now, None);
        a.css_animations(&[1, i64::MIN], now);
    }
    let anims: Vec<String> = (0..5000)
        .map(|i| {
            format!(
                "{{\"aid\":{i},\"target\":{},\"keyframes\":[{{\"offset\":0,\"values\":{{\"opacity\":\"0\"}}}}],\"startTime\":0}}",
                i % 7
            )
        })
        .collect();
    a.apply_web(&message(&format!("{{\"anims\":[{}]}}", anims.join(","))), 0.0);
    assert_eq!(a.web_targets().len(), 7);
    a.sample(0.5);
}
