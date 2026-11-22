//! What the animator comparison shares with `oracle/animations_oracle`: the records both sides read, the
//! answers both write, and seeded generators of style sheets, declarations, Web Animations messages and
//! scripts of steps through one animator.
//!
//! Every record is a u8 kind, a u32 length and its fields, little-endian; a string is a u32 length and
//! bytes, a declared map a u32 count of (name, value) strings, an id list a u32 count of i64s. Every answer
//! is status 1 and bytes; a double is written by its bits, every NaN as 0x7ff8000000000000.
//!   'S' a script through one Animator: u32 steps, each a u8 kind (echoed into the answer) and its input:
//!       'k' u32 n style sheets;                       'u' f64 now, u32 n (i64 id, declared) -> events;
//!       'p' f64 now -> u32 n (i64 id, values), events; 'w' message JSON text, f64 clock offset -> u8 parsed;
//!       'b' u32 n (i64 id, declared): `underlying`;    'a' f64 now, ids skipped -> u8 active, u8 active except;
//!       'i' f64 now -> animated ids;                   'r' i64 id -> animated properties;
//!       'e' i64 id, f64 t -> peeked values;            'v' i64 id, property -> base value;
//!       'd' i64 id -> u8 declares motion;              'l' i64 id, f64 now, u8 iterations -> u8 some, then
//!           u8 ready, f64 start, f64 duration, u8 repeat, f64 wake;
//!       'g' i64 id -> u64 signature;                   'q' ids, f64 now -> css_animations dump;
//!       't' i64 id -> web targets, web properties;    'x' ids alive -> (prune);
//!       'h' f64 now -> u64 hash, u64 hash, u32 n motions (i64 id, u8 kind, name, i64 aid, u8 heavy, u8 backdrop).
//!     Events are a u32 count of (type, i64 target, name, f64 elapsed). Both sides keep their maps by
//!     element id and write events and motions as they come.
//!   'T' timing text, u32 n f64 x -> u8 parsed, the timing, then css(x), web(x, false), web(x, true) for each.
//!   'I' a, b, f64 t -> interpolate(a, b, t).
//!   'K' u32 n style sheets -> the keyframe rules by name: u32 n (name, u32 frames (f64 offset, values,
//!       u8 timed, timing)).
//!   'A' declared -> u32 n animation specs (name, f64 duration, timing text, f64 delay, f64 iterations,
//!       direction, fill, u8 paused, timing()), u32 n transition specs (property, f64 duration, timing, f64 delay).
//!   'W' record JSON text, f64 clock offset -> u8 JSON, u8 parsed, then its fields.
//! A timing is u8 kind, u8 jump, f64 x1 y1 x2 y2, i32 steps, u32 n (f64, f64) points and its text.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

pub use twidget_animations::*;
use twidget_json::Json;
pub use twidget_testkit::{Fields, Lcg};

/// The answer both sides write.
#[derive(Debug, Default)]
pub struct Out(pub Vec<u8>);

impl Out {
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        let bits = if v.is_nan() { 0x7ff8000000000000 } else { v.to_bits() };
        self.0.extend_from_slice(&bits.to_le_bytes());
    }
    pub fn flag(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    pub fn str(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    pub fn list(&mut self, v: &[Vec<u8>]) {
        self.u32(v.len() as u32);
        for x in v {
            self.str(x);
        }
    }
    pub fn ids(&mut self, v: &[i64]) {
        self.u32(v.len() as u32);
        for &x in v {
            self.i64(x);
        }
    }
    pub fn values(&mut self, v: &StyleValues) {
        self.u32(v.len() as u32);
        for (name, value) in v {
            self.str(name);
            self.str(value);
        }
    }
    pub fn timing(&mut self, t: &Timing) {
        self.u8(t.kind as u8);
        self.u8(t.jump as u8);
        for v in [t.x1, t.y1, t.x2, t.y2] {
            self.f64(v);
        }
        self.i32(t.steps);
        self.u32(t.points.len() as u32);
        for &(input, output) in &t.points {
            self.f64(input);
            self.f64(output);
        }
        self.str(&t.text);
    }
    pub fn events(&mut self, list: &[AnimationEvent]) {
        self.u32(list.len() as u32);
        for e in list {
            self.str(e.kind.name().as_bytes());
            self.i64(e.target);
            self.str(&e.name);
            self.f64(e.elapsed);
        }
    }
}

pub type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

fn declared(pairs: &Pairs) -> Declared {
    let mut d = Declared::new();
    for (name, value) in pairs {
        d.insert(name.clone(), value.clone());
    }
    d
}

fn put_declared(f: &mut Fields, pairs: &Pairs) {
    f.u32(pairs.len() as u32);
    for (name, value) in pairs {
        f.str(name).str(value);
    }
}

fn put_ids(f: &mut Fields, ids: &[i64]) {
    f.u32(ids.len() as u32);
    for &id in ids {
        f.i64(id);
    }
}

/// One step through an animator.
#[derive(Clone, Debug)]
pub enum Step {
    Sheets(Vec<Vec<u8>>),
    Update(f64, Vec<(i64, Pairs)>),
    Sample(f64),
    Web(Vec<u8>, f64),
    Underlying(Vec<(i64, Pairs)>),
    Active(f64, Vec<i64>),
    Ids(f64),
    Properties(i64),
    Peek(i64, f64),
    Base(i64, Vec<u8>),
    Declares(i64),
    Plan(i64, f64, bool),
    Signature(i64),
    Query(Vec<i64>, f64),
    WebTargets(i64),
    Prune(Vec<i64>),
    Heavy(f64),
}

/// What the Rust side's answers came to, by kind.
#[derive(Debug, Default)]
pub struct Tally(pub BTreeMap<String, usize>);

impl Tally {
    pub fn add(&mut self, kind: impl Into<String>) {
        *self.0.entry(kind.into()).or_default() += 1;
    }
    pub fn merge(&mut self, other: Tally) {
        for (k, v) in other.0 {
            *self.0.entry(k).or_default() += v;
        }
    }
    pub fn show(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.0 {
            let _ = write!(s, " {k}={v}");
        }
        s
    }
    pub fn count(&self, kind: &str) -> usize {
        self.0.get(kind).copied().unwrap_or(0)
    }
}

pub fn script_record(steps: &[Step]) -> Vec<u8> {
    let mut f = Fields::new();
    f.u32(steps.len() as u32);
    for step in steps {
        match step {
            Step::Sheets(texts) => {
                f.u8(b'k').u32(texts.len() as u32);
                for t in texts {
                    f.str(t);
                }
            }
            Step::Update(now, elements) => {
                f.u8(b'u').f64(*now).u32(elements.len() as u32);
                for (id, pairs) in elements {
                    f.i64(*id);
                    put_declared(&mut f, pairs);
                }
            }
            Step::Sample(now) => {
                f.u8(b'p').f64(*now);
            }
            Step::Web(text, offset) => {
                f.u8(b'w').str(text).f64(*offset);
            }
            Step::Underlying(elements) => {
                f.u8(b'b').u32(elements.len() as u32);
                for (id, pairs) in elements {
                    f.i64(*id);
                    put_declared(&mut f, pairs);
                }
            }
            Step::Active(now, skip) => {
                f.u8(b'a').f64(*now);
                put_ids(&mut f, skip);
            }
            Step::Ids(now) => {
                f.u8(b'i').f64(*now);
            }
            Step::Properties(id) => {
                f.u8(b'r').i64(*id);
            }
            Step::Peek(id, t) => {
                f.u8(b'e').i64(*id).f64(*t);
            }
            Step::Base(id, property) => {
                f.u8(b'v').i64(*id).str(property);
            }
            Step::Declares(id) => {
                f.u8(b'd').i64(*id);
            }
            Step::Plan(id, now, iterations) => {
                f.u8(b'l').i64(*id).f64(*now).flag(*iterations);
            }
            Step::Signature(id) => {
                f.u8(b'g').i64(*id);
            }
            Step::Query(ids, now) => {
                f.u8(b'q');
                put_ids(&mut f, ids);
                f.f64(*now);
            }
            Step::WebTargets(id) => {
                f.u8(b't').i64(*id);
            }
            Step::Prune(alive) => {
                f.u8(b'x');
                put_ids(&mut f, alive);
            }
            Step::Heavy(now) => {
                f.u8(b'h').f64(*now);
            }
        }
    }
    f.framed(b'S')
}

#[derive(Default)]
pub struct ScriptHashes {
    signatures: BTreeMap<u64, u64>,
    motions: BTreeMap<u64, u64>,
}

// The renderer compares these hashes for equality. Non-MSVC C++ libraries give their own string hashes.
fn hash_id(hash: u64, seen: &mut BTreeMap<u64, u64>) -> u64 {
    if cfg!(target_env = "msvc") || hash == 0 {
        return hash;
    }
    let next = seen.len() as u64 + 1;
    *seen.entry(hash).or_insert(next)
}

/// The script through the port, writing what the oracle writes.
pub fn run_script(caches: &SharedCaches, hashes: &mut ScriptHashes, steps: &[Step], tally: &mut Tally) -> Vec<u8> {
    let mut a = Animator::with_caches(caches.clone());
    let mut o = Out::default();
    for step in steps {
        match step {
            Step::Sheets(texts) => {
                o.u8(b'k');
                a.set_style_sheets(texts);
            }
            Step::Update(now, elements) => {
                o.u8(b'u');
                let mut all = BTreeMap::new();
                for (id, pairs) in elements {
                    all.insert(*id, declared(pairs));
                }
                a.update(&all, *now);
                let events = a.take_events();
                for e in &events {
                    tally.add(format!("event {}", e.kind.name()));
                }
                o.events(&events);
            }
            Step::Sample(now) => {
                o.u8(b'p');
                let out = a.sample(*now);
                tally.add(if out.is_empty() { "sample empty" } else { "sample with overrides" });
                o.u32(out.len() as u32);
                for (id, values) in &out {
                    o.i64(*id);
                    o.values(values);
                    for (name, _) in values {
                        tally.add(format!("sampled {}", String::from_utf8_lossy(name)));
                    }
                }
                let events = a.take_events();
                for e in &events {
                    tally.add(format!("event {}", e.kind.name()));
                }
                o.events(&events);
            }
            Step::Web(text, offset) => {
                o.u8(b'w');
                let message = Json::parse(text);
                o.flag(message.is_some());
                if let Some(message) = message {
                    a.apply_web(&message, *offset);
                    tally.add(format!("web targets {}", a.web_targets().len().min(3)));
                }
            }
            Step::Underlying(elements) => {
                o.u8(b'b');
                a.underlying.clear();
                for (id, pairs) in elements {
                    a.underlying.insert(*id, declared(pairs));
                }
            }
            Step::Active(now, skip) => {
                o.u8(b'a');
                let skip_set: HashSet<i64> = skip.iter().copied().collect();
                let (all, except) = (a.active(*now), a.active_except(*now, &skip_set));
                tally.add(format!("active {all} {except}"));
                o.flag(all);
                o.flag(except);
            }
            Step::Ids(now) => {
                o.u8(b'i');
                let ids = a.animated_ids(*now);
                tally.add(format!("animated ids {}", ids.len().min(3)));
                o.ids(&ids);
            }
            Step::Properties(id) => {
                o.u8(b'r');
                o.list(&a.animated_properties(*id));
            }
            Step::Peek(id, t) => {
                o.u8(b'e');
                let values = a.peek(*id, *t);
                tally.add(if values.is_empty() { "peek empty" } else { "peek values" });
                o.values(&values);
            }
            Step::Base(id, property) => {
                o.u8(b'v');
                o.str(&a.base_value(*id, property));
            }
            Step::Declares(id) => {
                o.u8(b'd');
                o.flag(a.declares_motion(*id));
            }
            Step::Plan(id, now, iterations) => {
                o.u8(b'l');
                let plan = a.plan(*id, *now, *iterations);
                o.flag(plan.is_some());
                match plan {
                    None => tally.add("plan none"),
                    Some(p) => {
                        tally.add(if !p.ready {
                            "plan waiting"
                        } else if p.repeat {
                            "plan loop"
                        } else {
                            "plan span"
                        });
                        o.flag(p.ready);
                        o.f64(p.start);
                        o.f64(p.duration);
                        o.flag(p.repeat);
                        o.f64(p.wake);
                    }
                }
            }
            Step::Signature(id) => {
                o.u8(b'g');
                o.u64(hash_id(a.signature(*id), &mut hashes.signatures));
            }
            Step::Query(ids, now) => {
                o.u8(b'q');
                let list = a.css_animations(ids, *now);
                for e in list.arr() {
                    tally.add(format!("query {}", String::from_utf8_lossy(e.get("kind").as_str())));
                }
                o.str(&list.dump());
            }
            Step::WebTargets(id) => {
                o.u8(b't');
                o.ids(&a.web_targets());
                o.list(&a.web_properties(*id));
            }
            Step::Prune(alive) => {
                o.u8(b'x');
                a.prune(|id| alive.contains(&id));
            }
            Step::Heavy(now) => {
                o.u8(b'h');
                o.u64(hash_id(a.heavy_motion(*now, None), &mut hashes.motions));
                let mut list = Vec::new();
                o.u64(hash_id(a.heavy_motion(*now, Some(&mut list)), &mut hashes.motions));
                o.u32(list.len() as u32);
                for m in &list {
                    tally.add(format!("motion {}", m.kind as char));
                    o.i64(m.id);
                    o.u8(m.kind);
                    o.str(&m.name);
                    o.i64(m.aid);
                    o.flag(m.paint.heavy);
                    o.flag(m.paint.backdrop);
                }
            }
        }
    }
    o.0
}

pub fn timing_record(text: &[u8], xs: &[f64]) -> Vec<u8> {
    let mut f = Fields::new();
    f.str(text).u32(xs.len() as u32);
    for &x in xs {
        f.f64(x);
    }
    f.framed(b'T')
}

pub fn run_timing(caches: &SharedCaches, text: &[u8], xs: &[f64], tally: &mut Tally) -> Vec<u8> {
    let mut c = caches.lock().unwrap();
    let t = Timing::parse(&mut c.timings, text);
    let mut o = Out::default();
    o.flag(t.is_some());
    match &t {
        Some(t) => {
            tally.add(format!("timing {:?} {:?}", t.kind, if t.kind == Kind::Steps { Some(t.jump) } else { None }));
            o.timing(t);
            for &x in xs {
                o.f64(t.css(x));
                o.f64(t.web(x, false));
                o.f64(t.web(x, true));
            }
        }
        None => tally.add("timing refused"),
    }
    o.0
}

pub fn interpolate_record(a: &[u8], b: &[u8], t: f64) -> Vec<u8> {
    Fields::new().str(a).str(b).f64(t).framed(b'I')
}

pub fn run_interpolate(caches: &SharedCaches, a: &[u8], b: &[u8], t: f64, tally: &mut Tally) -> Vec<u8> {
    let mut c = caches.lock().unwrap();
    let v = interpolate(&mut c.pairs, a, b, t);
    tally.add(if v == a {
        "interpolate first"
    } else if v == b {
        "interpolate second"
    } else if v.windows(6).any(|w| w == b"calc((") || v.windows(3).any(|w| w == b") t") {
        "interpolate weighted"
    } else {
        "interpolate mixed"
    });
    let mut o = Out::default();
    o.str(&v);
    o.0
}

pub fn keyframes_record(texts: &[Vec<u8>]) -> Vec<u8> {
    let mut f = Fields::new();
    f.u32(texts.len() as u32);
    for t in texts {
        f.str(t);
    }
    f.framed(b'K')
}

pub fn run_keyframes(caches: &SharedCaches, texts: &[Vec<u8>], tally: &mut Tally) -> Vec<u8> {
    let mut c = caches.lock().unwrap();
    let mut rules = KeyframeRules::new();
    for t in texts {
        parse_keyframes(&mut c.timings, t, &mut rules);
    }
    let mut names: Vec<&Vec<u8>> = rules.keys().collect();
    names.sort();
    let mut o = Out::default();
    o.u32(names.len() as u32);
    for name in names {
        o.str(name);
        let frames = &rules[name];
        tally.add(format!("keyframe rule of {} frames", frames.len().min(4)));
        o.u32(frames.len() as u32);
        for f in frames {
            o.f64(f.offset);
            o.values(&f.declarations);
            o.flag(f.timing.is_some());
            if let Some(t) = &f.timing {
                tally.add("keyframe timed");
                o.timing(t);
            }
        }
    }
    o.0
}

pub fn specs_record(pairs: &Pairs) -> Vec<u8> {
    let mut f = Fields::new();
    put_declared(&mut f, pairs);
    f.framed(b'A')
}

pub fn run_specs(caches: &SharedCaches, pairs: &Pairs, tally: &mut Tally) -> Vec<u8> {
    let mut c = caches.lock().unwrap();
    let d = declared(pairs);
    let mut o = Out::default();
    let animations = AnimationSpec::list(&mut c.timings, &d);
    tally.add(format!("animation specs {}", animations.len().min(3)));
    o.u32(animations.len() as u32);
    for s in &animations {
        o.str(&s.name);
        o.f64(s.duration);
        o.str(&s.timing_text);
        o.f64(s.delay);
        o.f64(s.iterations);
        o.str(&s.direction);
        o.str(&s.fill);
        o.flag(s.paused);
        let t = s.timing(&mut c.timings);
        o.timing(&t);
    }
    let transitions = TransitionSpec::list(&mut c.timings, &d);
    tally.add(format!("transition specs {}", transitions.len().min(3)));
    o.u32(transitions.len() as u32);
    for s in &transitions {
        o.str(&s.property);
        o.f64(s.duration);
        o.timing(&s.timing);
        o.f64(s.delay);
    }
    o.0
}

pub fn web_record(text: &[u8], offset: f64) -> Vec<u8> {
    Fields::new().str(text).f64(offset).framed(b'W')
}

pub fn run_web(caches: &SharedCaches, text: &[u8], offset: f64, tally: &mut Tally) -> Vec<u8> {
    let mut c = caches.lock().unwrap();
    let mut o = Out::default();
    let record = Json::parse(text);
    o.flag(record.is_some());
    let Some(record) = record else {
        tally.add("web record not JSON");
        return o.0;
    };
    let a = WebAnimation::parse(&mut c.timings, &record, offset);
    o.flag(a.is_some());
    let Some(a) = a else {
        tally.add("web record refused");
        return o.0;
    };
    tally.add("web record parsed");
    o.i64(a.aid);
    o.i64(a.target);
    o.u32(a.frames.len() as u32);
    for f in &a.frames {
        o.f64(f.offset);
        o.timing(&f.easing);
        o.str(&f.composite);
        o.values(&f.values);
    }
    for v in [a.duration, a.delay, a.end_delay, a.iterations, a.iteration_start] {
        o.f64(v);
    }
    o.str(&a.direction);
    o.str(&a.fill);
    o.timing(&a.easing);
    o.str(&a.composite);
    o.str(&a.iteration_composite);
    o.flag(a.start_ms.is_some());
    o.f64(a.start_ms.unwrap_or(0.0));
    o.flag(a.hold_ms.is_some());
    o.f64(a.hold_ms.unwrap_or(0.0));
    o.f64(a.rate);
    o.list(&a.properties);
    o.flag(a.running());
    o.0
}

// ---- Generators ------------------------------------------------------------------------------------

pub fn pick<'a, T>(r: &mut Lcg, items: &'a [T]) -> &'a T {
    &items[r.below(items.len())]
}

fn b(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// A number as a widget writes one, now and then an odd one.
pub fn num(r: &mut Lcg) -> String {
    match r.below(12) {
        0 => pick(r, &["0", "1", "-1", "0.5", ".5", "10", "100", "360", "-0.25", "1e3", "2.5e-2", "-0", "0.0001", "1e-7", "12345.678"])
            .to_string(),
        1 => format!("{}", r.below(10)),
        2 => format!("-{}.{}", r.below(50), r.below(100)),
        3 => format!("{}.{:02}", r.below(400), r.below(100)),
        4 => pick(r, &["1e30", "-1e300", "+3", "1.", "00.5", "1e+2", "7E1"]).to_string(),
        _ => format!("{}", (r.below(4001) as f64 - 2000.0) / 8.0),
    }
}

pub fn unit(r: &mut Lcg) -> &'static str {
    pick(r, &["px", "px", "px", "%", "em", "rem", "deg", "turn", "", "vw", "s"])
}

pub fn length(r: &mut Lcg) -> String {
    match r.below(10) {
        0 => format!("calc({}% + {}px)", num(r), num(r)),
        1 => format!("calc(100% - {}px)", r.below(50)),
        2 => pick(r, &["auto", "0", "inherit", "50%"]).to_string(),
        _ => format!("{}{}", num(r), unit(r)),
    }
}

pub fn color(r: &mut Lcg) -> String {
    match r.below(9) {
        0 => format!("#{:03x}", r.below(0x1000)),
        1 => format!("#{:06x}", r.next() & 0xffffff),
        2 => format!("rgb({}, {}, {})", r.below(256), r.below(300), r.below(256)),
        3 => format!("rgba({},{},{},{})", r.below(256), r.below(256), r.below(256), pick(r, &["0", "0.5", ".4", "1", "2"])),
        4 => format!("rgb({}% {}% {}%)", r.below(101), r.below(101), r.below(101)),
        5 => format!("#{:08x}", r.next()),
        6 => format!("hsl({}, 50%, 50%)", r.below(360)),
        _ => pick(r, &["red", "blue", "transparent", "white", "black", "currentColor", "rebeccapurple", "light-dark(#000, #fff)"])
            .to_string(),
    }
}

pub fn transform(r: &mut Lcg) -> String {
    let one = |r: &mut Lcg| -> String {
        match r.below(16) {
            0 => format!("rotate({}deg)", num(r)),
            1 => format!("translateX({})", length(r)),
            2 => format!("translateY({})", length(r)),
            3 => format!("translate({}, {})", length(r), length(r)),
            4 => format!("translate({})", length(r)),
            5 => format!("scale({})", num(r)),
            6 => format!("scale({}, {})", num(r), num(r)),
            7 => format!("scaleX({})", num(r)),
            8 => format!("scaleY({})", num(r)),
            9 => format!("skew({}deg)", num(r)),
            10 => format!("skewX({}deg)", num(r)),
            11 => format!("skewY({}deg)", num(r)),
            12 => format!("matrix(1, 0, 0, 1, {}, {})", num(r), num(r)),
            13 => format!("rotate({}turn)", num(r)),
            14 => format!("translate3d({}, {}, 0)", length(r), length(r)),
            _ => format!("translateX(calc({}% - 1 * ({}% + {}px) + {}px))", num(r), num(r), num(r), num(r)),
        }
    };
    match r.below(8) {
        0 => "none".to_string(),
        1..=3 => one(r),
        4 | 5 => format!("{} {}", one(r), one(r)),
        6 => format!("{} {} {}", one(r), one(r), one(r)),
        _ => pick(r, &["NONE", "rotate(0deg)", "translateX(0)", "scale(1)", "rotate(10deg", "x(1)y", "translate(1px,2px) foo"]).to_string(),
    }
}

pub fn filter(r: &mut Lcg) -> String {
    let one = |r: &mut Lcg| -> String {
        match r.below(7) {
            0 => format!("blur({}px)", r.below(20)),
            1 => format!("brightness({})", num(r)),
            2 => format!("saturate({}%)", r.below(300)),
            3 => format!("opacity({})", num(r)),
            4 => format!("contrast({})", num(r)),
            5 => format!("drop-shadow(0 {}px {}px {})", r.below(9), r.below(9), color(r)),
            _ => format!("hue-rotate({}deg)", num(r)),
        }
    };
    match r.below(5) {
        0 => "none".to_string(),
        1 | 2 => one(r),
        _ => format!("{} {}", one(r), one(r)),
    }
}

pub fn shadow(r: &mut Lcg) -> String {
    let one = |r: &mut Lcg| -> String {
        match r.below(4) {
            0 => format!("0 {}px {}px {}", r.below(9), r.below(9), color(r)),
            1 => format!("{}px {}px 0 {}", r.below(5), r.below(5), color(r)),
            2 => format!("inset 0 1px {}px {}", r.below(4), color(r)),
            _ => format!("0 0 0 {}px {}", r.below(4), color(r)),
        }
    };
    match r.below(5) {
        0 => "none".to_string(),
        1 => format!("{}, {}", one(r), one(r)),
        _ => one(r),
    }
}

/// A transform of scale functions and others, which Web Animations add to and accumulate onto their own way.
pub fn scale_transform(r: &mut Lcg) -> String {
    let one = |r: &mut Lcg| -> String {
        match r.below(6) {
            0 => format!("scale({})", num(r)),
            1 => format!("scale({}, {})", num(r), num(r)),
            2 => format!("scaleX({})", num(r)),
            3 => format!("scale3d({}, {}, 1)", num(r), num(r)),
            4 => format!("rotate({}deg)", num(r)),
            _ => format!("translateX({}px)", num(r)),
        }
    };
    match r.below(3) {
        0 => one(r),
        1 => format!("{} {}", one(r), one(r)),
        _ => pick(r, &["scale(2)", "scale(1.5) rotate(10deg)", "none", "scale(0)", "scaleY(3) translateX(4px)"]).to_string(),
    }
}

/// A value of `property` as a widget may declare or animate it.
pub fn value_of(r: &mut Lcg, property: &str) -> String {
    if r.chance(40) {
        return pick(r, &["", "  ", "none", "inherit", "initial", "1px 2px", "red", "var(--x)", "#", "()", "1e400px", "0x10px"])
            .to_string();
    }
    match property {
        "transform" => transform(r),
        "opacity" => pick(r, &["0", "1", "0.5", ".55", "50%", "0.25", "1.0", "0.999"]).to_string(),
        "color" | "background-color" | "border-color" | "fill" | "stroke" => color(r),
        "background" => {
            if r.chance(2) {
                color(r)
            } else {
                format!("linear-gradient(90deg, {}, {})", color(r), color(r))
            }
        }
        "box-shadow" => shadow(r),
        "filter" | "backdrop-filter" => filter(r),
        "translate" => format!("{} {}", length(r), length(r)),
        "scale" => {
            if r.chance(2) {
                num(r)
            } else {
                format!("{} {}", num(r), num(r))
            }
        }
        "rotate" => format!("{}deg", num(r)),
        "letter-spacing" => pick(r, &["0px", "1px", "2px", "0.1em", "normal", "-1px"]).to_string(),
        "flex-grow" | "stroke-dashoffset" => num(r),
        "transform-origin" => pick(r, &["50% 50%", "left top", "0 0", "center", "10px 20px"]).to_string(),
        _ => length(r),
    }
}

pub const ANIMATED: [&str; 24] = [
    "transform",
    "opacity",
    "color",
    "background-color",
    "background",
    "width",
    "height",
    "top",
    "left",
    "box-shadow",
    "border-color",
    "max-height",
    "stroke-dashoffset",
    "fill",
    "translate",
    "scale",
    "rotate",
    "filter",
    "letter-spacing",
    "font-size",
    "margin-left",
    "flex-grow",
    "transform-origin",
    "backdrop-filter",
];

pub const NAMES: [&str; 9] = ["spin", "fade", "slide", "pulse", "grow", "glow", "k", "Shimmer", "none-such"];

pub fn timing_text(r: &mut Lcg) -> String {
    let v = |r: &mut Lcg| -> String {
        pick(r, &["0", "1", "0.25", "0.42", "0.58", "-0.5", "1.5", "2", ".1", "0.68", "-0.55", "1.265"]).to_string()
    };
    match r.below(14) {
        0 => pick(r, &["linear", "ease", "ease-in", "ease-out", "ease-in-out", "step-start", "step-end"]).to_string(),
        1 => pick(r, &["EASE-IN", " ease ", "Linear", "steps(1)", "steps(4, end)"]).to_string(),
        2 | 3 => format!("cubic-bezier({}, {}, {}, {})", v(r), v(r), v(r), v(r)),
        4 | 5 => {
            let position =
                pick(r, &["", ", start", ", end", ", jump-start", ", jump-end", ", jump-both", ", jump-none", ", sideways", ",end"]);
            format!("steps({}{position})", pick(r, &["1", "2", "3", "5", "10", "0", "2.5", "-1", "100000", "100001", "1e3"]))
        }
        6..=8 => {
            let n = 1 + r.below(5);
            let mut parts = Vec::new();
            for _ in 0..n {
                let mut p = v(r);
                for _ in 0..r.below(3) {
                    let _ = write!(p, " {}%", pick(r, &["0", "25", "50", "75", "100", "-20", "120", "10.5"]));
                }
                parts.push(p);
            }
            format!("linear({})", parts.join(", "))
        }
        9 => pick(
            r,
            &[
                "cubic-bezier(1, 2, 3)",
                "cubic-bezier(a, b, c, d)",
                "steps()",
                "linear()",
                "linear(,)",
                "linear(1 x)",
                "bounce",
                "steps(2, start, end)",
                "",
            ],
        )
        .to_string(),
        10 => format!("linear({})", (0..r.below(300) + 1).map(|i| format!("{}", i % 7)).collect::<Vec<_>>().join(",")),
        _ => pick(r, &["linear", "ease", "ease-in-out", "cubic-bezier(0.25, 0.1, 0.25, 1)", "steps(4)"]).to_string(),
    }
}

pub fn time_text(r: &mut Lcg) -> String {
    match r.below(10) {
        0 => pick(r, &["0s", "-1s", "-0.25s", "calc(1s + 100ms)", "2S", "1MS", "0.5 s", "1", "infinite", "10s"]).to_string(),
        1 | 2 => format!("{}ms", 1 + r.below(1500)),
        3 => format!("{}ms", pick(r, &["1", "16", "50", "100", "400"])),
        4 => format!("-{}ms", r.below(2000)),
        _ => format!("{}s", pick(r, &["0.1", "0.2", "0.3", "0.5", "1", "1.2", "2", "3", "4", "0.05", ".25"])),
    }
}

/// An `animation` shorthand of one to three animations.
pub fn animation_shorthand(r: &mut Lcg) -> String {
    let mut items = Vec::new();
    let most = if r.chance(4) { 3 } else { 1 };
    for _ in 0..1 + r.below(most) {
        let mut tokens = vec![pick(r, &NAMES).to_string()];
        if !r.chance(8) {
            tokens.push(time_text(r));
        }
        if r.chance(2) {
            tokens.push(timing_text(r));
        }
        if r.chance(3) {
            tokens.push(time_text(r));
        }
        if r.chance(2) {
            tokens.push(pick(r, &["infinite", "2", "3", "0.5", "1.5", "0", "-1", "1e9", "-1e300"]).to_string());
        }
        if r.chance(2) {
            tokens.push(pick(r, &["normal", "reverse", "alternate", "alternate-reverse", "REVERSE"]).to_string());
        }
        if r.chance(2) {
            tokens.push(pick(r, &["forwards", "backwards", "both", "none"]).to_string());
        }
        if r.chance(4) {
            tokens.push(pick(r, &["paused", "running"]).to_string());
        }
        for i in (1..tokens.len()).rev() {
            if r.chance(3) {
                let j = r.below(i + 1);
                tokens.swap(i, j);
            }
        }
        items.push(tokens.join(if r.chance(10) { "  " } else { " " }));
    }
    items.join(", ")
}

pub fn transition_shorthand(r: &mut Lcg) -> String {
    let mut items = Vec::new();
    for _ in 0..1 + r.below(3) {
        let mut tokens = Vec::new();
        if !r.chance(5) {
            tokens.push(
                pick(
                    r,
                    &[
                        "all",
                        "opacity",
                        "transform",
                        "color",
                        "background-color",
                        "width",
                        "filter",
                        "box-shadow",
                        "left",
                        "none",
                        "Opacity",
                    ],
                )
                .to_string(),
            );
        }
        tokens.push(time_text(r));
        if r.chance(2) {
            tokens.push(timing_text(r));
        }
        if r.chance(3) {
            tokens.push(time_text(r));
        }
        if r.chance(5) {
            tokens.reverse();
        }
        items.push(tokens.join(" "));
    }
    items.join(", ")
}

fn list_of(r: &mut Lcg, n: usize, one: impl Fn(&mut Lcg) -> String) -> String {
    (0..n).map(|_| one(r)).collect::<Vec<_>>().join(", ")
}

/// What an element declares of the animation and transition properties.
pub fn motion_declarations(r: &mut Lcg) -> Pairs {
    let mut out = Pairs::new();
    match r.below(10) {
        0..=3 => out.push((b("animation"), animation_shorthand(r).into_bytes())),
        4 => {
            let n = 1 + r.below(3);
            out.push((b("animation-name"), list_of(r, n, |r| pick(r, &NAMES).to_string()).into_bytes()));
            for (key, f) in [
                ("animation-duration", time_text as fn(&mut Lcg) -> String),
                ("animation-timing-function", timing_text),
                ("animation-delay", time_text),
                ("animation-iteration-count", |r: &mut Lcg| pick(r, &["infinite", "2", "0.5", "x"]).to_string()),
                ("animation-direction", |r: &mut Lcg| pick(r, &["normal", "reverse", "alternate", "alternate-reverse"]).to_string()),
                ("animation-fill-mode", |r: &mut Lcg| pick(r, &["forwards", "backwards", "both", "none"]).to_string()),
                ("animation-play-state", |r: &mut Lcg| pick(r, &["paused", "running"]).to_string()),
            ] {
                if r.chance(2) {
                    let n = 1 + r.below(2);
                    out.push((b(key), list_of(r, n, f).into_bytes()));
                }
            }
        }
        5 => {
            out.push((b("animation"), animation_shorthand(r).into_bytes()));
            out.push((b("animation-play-state"), b(pick(r, &["paused", "running", "paused, running"]))));
        }
        _ => {}
    }
    match r.below(8) {
        0..=2 => out.push((b("transition"), transition_shorthand(r).into_bytes())),
        3 => {
            let n = 1 + r.below(3);
            out.push((
                b("transition-property"),
                list_of(r, n, |r| pick(r, &["opacity", "transform", "all", "color", "width", "none"]).to_string()).into_bytes(),
            ));
            if !r.chance(4) {
                let n = 1 + r.below(2);
                out.push((b("transition-duration"), list_of(r, n, time_text).into_bytes()));
            }
            if r.chance(2) {
                out.push((b("transition-timing-function"), timing_text(r).into_bytes()));
            }
            if r.chance(3) {
                out.push((b("transition-delay"), time_text(r).into_bytes()));
            }
        }
        4 => {
            out.push((b("transition"), transition_shorthand(r).into_bytes()));
            let n = 1 + r.below(2);
            out.push((b("transition-duration"), list_of(r, n, time_text).into_bytes()));
        }
        5 => {
            // The shorthand and the property list: each property takes what the shorthand's item at its index gave.
            out.push((b("transition"), transition_shorthand(r).into_bytes()));
            let n = 1 + r.below(4);
            out.push((
                b("transition-property"),
                list_of(r, n, |r| pick(r, &["opacity", "transform", "all", "color", "width", "none"]).to_string()).into_bytes(),
            ));
            if r.chance(2) {
                out.push((b("transition-timing-function"), timing_text(r).into_bytes()));
            }
            if r.chance(2) {
                out.push((b("transition-delay"), time_text(r).into_bytes()));
            }
        }
        _ => {}
    }
    out
}

/// One `@keyframes` block, its frames now and then out of order, out of range or not frames at all.
pub fn keyframes_block(r: &mut Lcg, name: &str) -> String {
    let at = pick(r, &["@keyframes", "@keyframes", "@keyframes", "@-webkit-keyframes", "@KEYFRAMES", "keyframes", "@-moz-keyframes"]);
    let quoted = match r.below(8) {
        0 => format!("\"{name}\""),
        1 => format!("'{name}'"),
        _ => name.to_string(),
    };
    let mut s = format!("{at} {quoted}{}{{", if r.chance(3) { "" } else { " " });
    let properties: Vec<&str> = (0..1 + r.below(3)).map(|_| *pick(r, &ANIMATED)).collect();
    for _ in 0..1 + r.below(4) {
        let selector = match r.below(10) {
            0 => "from".to_string(),
            1 => "to".to_string(),
            2 => "from, to".to_string(),
            3 => format!("{}%", r.below(101)),
            4 => format!("{}%, {}%", r.below(50), 50 + r.below(51)),
            5 => pick(r, &["-10%", "150%", "abc", "FROM", " 25% ", "50.5%", "%", "0%,", "1e1%"]).to_string(),
            _ => pick(r, &["0%", "100%", "50%", "25%", "75%"]).to_string(),
        };
        let _ = write!(s, "{selector}{{");
        for p in &properties {
            if r.chance(6) {
                continue;
            }
            let _ = write!(s, "{p}:{};", value_of(r, p));
        }
        if r.chance(4) {
            let _ = write!(s, "animation-timing-function:{};", timing_text(r));
        }
        if r.chance(10) {
            s.push_str("opacity:1 !important;");
        }
        s.push('}');
    }
    s.push('}');
    s
}

pub fn style_sheet(r: &mut Lcg) -> String {
    let mut s = String::new();
    for _ in 0..1 + r.below(4) {
        match r.below(6) {
            0 => {
                let _ = write!(s, ".c{}{{color:red;animation:{}}} ", r.below(9), animation_shorthand(r));
            }
            1 if r.chance(3) => s.push_str("@keyframes broken{from{opacity:0}"),
            _ => {
                let name = *pick(r, &NAMES);
                s.push_str(&keyframes_block(r, name));
                s.push(' ');
            }
        }
    }
    s
}

pub fn now_after(r: &mut Lcg, now: f64) -> f64 {
    match r.below(20) {
        0 => now,
        1 => now + 1000.0 + r.below(1000) as f64,
        2 => now - r.below(100) as f64 / 1000.0,
        3 => now + 0.0001,
        4..=9 => now + r.below(80) as f64 / 1000.0,
        _ => now + r.below(600) as f64 / 1000.0,
    }
}

fn json_string(s: &str) -> String {
    String::from_utf8(twidget_json::quote(s)).unwrap_or_default()
}

fn json_number(r: &mut Lcg, choices: &[&str]) -> String {
    pick(r, choices).to_string()
}

/// One record of an `anim` message: a Web Animation, or script control of a CSS animation or transition.
pub fn web_item(r: &mut Lcg, ids: &[i64], now: f64, starts: &[f64]) -> String {
    let aid = 1 + r.below(6);
    let target = if r.chance(8) { *pick(r, &[0i64, 99, -1, 7]) } else { *pick(r, ids) };
    if r.chance(10) {
        return format!("{{\"aid\":{aid},\"remove\":true}}");
    }
    if r.chance(4) {
        let kind = pick(r, &["animation", "animation", "transition", "other", "1"]);
        let name = if *kind == "transition" {
            pick(r, &["opacity", "transform", "color", "width"]).to_string()
        } else {
            pick(r, &NAMES).to_string()
        };
        let origin = if !starts.is_empty() && !r.chance(4) { pick(r, starts) * 1000.0 } else { r.below(5000) as f64 };
        let mut s = format!(
            "{{\"aid\":{aid},\"target\":{target},\"css\":{{\"kind\":\"{kind}\",\"name\":{},\"origin\":{origin}}}",
            json_string(&name)
        );
        if r.chance(3) {
            let _ = write!(s, ",\"startTime\":{}", now * 1000.0 - r.below(2000) as f64);
        }
        if r.chance(3) {
            let _ = write!(s, ",\"holdTime\":{}", r.below(3000));
        }
        if r.chance(3) {
            let _ = write!(s, ",\"playbackRate\":{}", json_number(r, &["1", "2", "0.5", "-1", "0", "1001", "-0.25"]));
        }
        if r.chance(4) {
            s.push_str(",\"cancelled\":true");
        }
        if r.chance(10) {
            s.push_str(",\"remove\":true");
        }
        s.push('}');
        return s;
    }
    let properties: Vec<&str> = (0..1 + r.below(2))
        .map(|_| {
            *pick(r, &["opacity", "transform", "color", "width", "filter", "box-shadow", "backdrop-filter", "left", "translate", "scale"])
        })
        .collect();
    let mut frames = Vec::new();
    let n = r.below(5);
    let mut offset = 0.0;
    for i in 0..n {
        let mut f = String::from("{");
        offset = match r.below(8) {
            0 => offset,
            1 if i > 0 => offset - 0.1,
            2 => 1.0,
            _ => {
                if n <= 1 {
                    *pick(r, &[0.0, 1.0, 0.5])
                } else {
                    (offset + (1.0 - offset) * (i as f64 / (n - 1) as f64)).min(1.0)
                }
            }
        };
        if !r.chance(20) {
            let _ = write!(f, "\"offset\":{offset}");
        } else {
            f.push_str("\"offset\":null");
        }
        if r.chance(4) {
            let _ = write!(f, ",\"easing\":{}", json_string(&timing_text(r)));
        }
        if r.chance(4) {
            let _ = write!(f, ",\"composite\":\"{}\"", pick(r, &["replace", "add", "accumulate", "auto"]));
        }
        f.push_str(",\"values\":{");
        let mut first = true;
        for p in &properties {
            if r.chance(5) {
                continue;
            }
            if !first {
                f.push(',');
            }
            first = false;
            let value = if r.chance(15) {
                "1".to_string()
            } else if *p == "transform" && r.chance(2) {
                json_string(&scale_transform(r))
            } else {
                json_string(&value_of(r, p))
            };
            let _ = write!(f, "\"{p}\":{value}");
        }
        f.push_str("}}");
        frames.push(f);
    }
    let mut timing = Vec::new();
    for (key, choices) in [
        ("duration", &["1000", "500", "0", "250", "3000", "-1", "1e300", "null", "16"][..]),
        ("delay", &["0", "200", "-500", "1000", "null"][..]),
        ("endDelay", &["0", "100", "-300"][..]),
        ("iterations", &["1", "2", "0.5", "null", "0", "3", "-1", "1e10", "\"x\""][..]),
        ("iterationStart", &["0", "0.5", "1.25", "-1"][..]),
    ] {
        if r.chance(2) {
            timing.push(format!("\"{key}\":{}", json_number(r, choices)));
        }
    }
    if r.chance(2) {
        timing.push(format!("\"direction\":\"{}\"", pick(r, &["normal", "reverse", "alternate", "alternate-reverse", "sideways"])));
    }
    if r.chance(2) {
        timing.push(format!("\"fill\":\"{}\"", pick(r, &["none", "forwards", "backwards", "both", "auto", "all"])));
    }
    if r.chance(2) {
        timing.push(format!("\"easing\":{}", json_string(&timing_text(r))));
    }
    let mut s = format!("{{\"aid\":{aid},\"target\":{target},\"keyframes\":[{}],\"timing\":{{{}}}", frames.join(","), timing.join(","));
    if r.chance(4) {
        let _ = write!(s, ",\"composite\":\"{}\"", pick(r, &["replace", "add", "accumulate", "auto"]));
    }
    if r.chance(4) {
        let _ = write!(s, ",\"iterationComposite\":\"{}\"", pick(r, &["replace", "accumulate", "add"]));
    }
    if !r.chance(6) {
        let _ = write!(s, ",\"startTime\":{}", now * 1000.0 - r.below(3000) as f64 + 500.0);
    }
    if r.chance(5) {
        let _ = write!(s, ",\"holdTime\":{}", json_number(r, &["0", "250", "1000", "1e13", "-50"]));
    }
    if r.chance(3) {
        let _ = write!(s, ",\"playbackRate\":{}", json_number(r, &["1", "2", "0.5", "-1", "0", "1001", "-2"]));
    }
    s.push('}');
    s
}

pub fn web_message(r: &mut Lcg, ids: &[i64], now: f64, starts: &[f64]) -> (Vec<u8>, f64) {
    let items: Vec<String> = (0..1 + r.below(4)).map(|_| web_item(r, ids, now, starts)).collect();
    let message_now = if r.chance(2) { 0.0 } else { now * 1000.0 - r.below(100) as f64 };
    let offset = if r.chance(2) { 0.0 } else { now * 1000.0 - message_now };
    (format!("{{\"type\":\"anim\",\"now\":{message_now},\"anims\":[{}]}}", items.join(",")).into_bytes(), offset)
}

/// An element's declarations: its motion and the watched values it has now.
#[derive(Clone, Debug)]
pub struct Element {
    pub id: i64,
    pub motion: Pairs,
    pub values: Vec<(&'static str, String)>,
    pub present: bool,
}

impl Element {
    pub fn declared(&self) -> Pairs {
        let mut out = self.motion.clone();
        for (p, v) in &self.values {
            out.push((b(p), v.clone().into_bytes()));
        }
        out
    }
}

/// A generated script: style sheets, elements whose declarations change between builds, Web Animations,
/// and every query between them.
pub fn any_script(r: &mut Lcg) -> Vec<Step> {
    let pool: [i64; 8] = [1, 2, 3, 5, 8, 13, 1 << 40, -7];
    let count = 1 + r.below(4);
    let mut elements: Vec<Element> = (0..count)
        .map(|i| {
            let id = pool[(i + r.below(3)) % pool.len()];
            let mut values = Vec::new();
            for _ in 0..r.below(3) {
                let p = *pick(r, &ANIMATED);
                values.push((p, value_of(r, p)));
            }
            Element { id, motion: motion_declarations(r), values, present: true }
        })
        .collect();
    let ids: Vec<i64> = elements.iter().map(|e| e.id).collect();
    let mut steps = Vec::new();
    let sheets: Vec<Vec<u8>> = (0..1 + r.below(2)).map(|_| style_sheet(r).into_bytes()).collect();
    steps.push(Step::Sheets(sheets));
    let mut now = match r.below(4) {
        0 => 0.0,
        1 => r.below(100) as f64 / 7.0,
        _ => 1.0 + r.below(2000) as f64 / 100.0,
    };
    let mut starts = vec![now];
    let update =
        |elements: &[Element], now: f64| Step::Update(now, elements.iter().filter(|e| e.present).map(|e| (e.id, e.declared())).collect());
    steps.push(update(&elements, now));
    steps.push(Step::Sample(now));
    let n = 4 + r.below(24);
    for _ in 0..n {
        let id = if r.chance(10) { 4242 } else { *pick(r, &ids) };
        match r.below(30) {
            0..=7 => {
                now = now_after(r, now);
                steps.push(Step::Sample(now));
            }
            8..=12 => {
                // A build: some elements change what they declare.
                for e in &mut elements {
                    match r.below(8) {
                        0 => e.motion = motion_declarations(r),
                        1 | 2 => {
                            let p = *pick(r, &ANIMATED);
                            let v = value_of(r, p);
                            match e.values.iter_mut().find(|(q, _)| *q == p) {
                                Some(slot) => slot.1 = v,
                                None => e.values.push((p, v)),
                            }
                        }
                        3 if !e.values.is_empty() => {
                            let k = r.below(e.values.len());
                            e.values.remove(k);
                        }
                        4 if r.chance(3) => e.present = !e.present,
                        5 => {
                            let state = pick(r, &["paused", "running"]);
                            e.motion.retain(|(k, _)| k != b"animation-play-state");
                            e.motion.push((b("animation-play-state"), b(state)));
                        }
                        _ => {}
                    }
                }
                if r.chance(3) {
                    now = now_after(r, now);
                }
                starts.push(now);
                steps.push(update(&elements, now));
            }
            13 | 14 => {
                let (text, offset) = web_message(r, &ids, now, &starts);
                steps.push(Step::Web(text, offset));
            }
            15 => {
                let mut under: Vec<(i64, Pairs)> = Vec::new();
                for &id in &ids {
                    if r.chance(2) {
                        let p = *pick(r, &ANIMATED);
                        under.push((id, vec![(b(p), value_of(r, p).into_bytes())]));
                    }
                }
                steps.push(Step::Underlying(under));
            }
            16 => steps.push(Step::Active(now, ids.iter().copied().filter(|_| r.chance(2)).collect())),
            17 => steps.push(Step::Ids(now)),
            18 => steps.push(Step::Properties(id)),
            19 | 20 => {
                let t = if r.chance(2) { now } else { now_after(r, now) };
                steps.push(Step::Peek(id, t));
            }
            21 => steps.push(Step::Base(id, b(pick(r, &ANIMATED)))),
            22 => steps.push(Step::Declares(id)),
            23 | 24 => steps.push(Step::Plan(id, now, r.chance(2))),
            25 => steps.push(Step::Signature(id)),
            26 => steps.push(Step::Query(ids.iter().copied().filter(|_| !r.chance(3)).collect(), now)),
            27 => steps.push(Step::WebTargets(id)),
            28 => {
                if r.chance(3) {
                    steps.push(Step::Prune(ids.iter().copied().filter(|_| r.chance(2)).collect()));
                } else {
                    steps.push(Step::Sheets((0..r.below(3)).map(|_| style_sheet(r).into_bytes()).collect()));
                }
            }
            _ => steps.push(Step::Heavy(now)),
        }
    }
    // Every element looked at once at the end.
    for &id in &ids {
        steps.push(Step::Signature(id));
        steps.push(Step::Plan(id, now, true));
        steps.push(Step::Peek(id, now));
    }
    steps.push(Step::Query(ids.clone(), now));
    steps.push(Step::Heavy(now));
    steps
}

// ---- The corpus ---------------------------------------------------------------------------------

pub fn widget_sandbox() -> std::path::PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn html_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "html") {
            out.push(path);
        }
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// The `<style>` blocks of every corpus widget.
pub fn corpus_sheets() -> Vec<Vec<u8>> {
    let mut files = Vec::new();
    let root = widget_sandbox();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        html_files(&root.join(dir), &mut files);
    }
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read(&path) else { continue };
        let mut i = 0;
        while let Some(a) = find(&text, b"<style>", i) {
            let start = a + 7;
            let Some(e) = find(&text, b"</style>", start) else { break };
            out.push(text[start..e].to_vec());
            i = e + 8;
        }
    }
    out
}

/// The declarations of every rule of `sheet` that declares one of the animator's watched properties.
pub fn sheet_declarations(sheet: &[u8]) -> Vec<Pairs> {
    let watched: Vec<&[u8]> = Animator::watched().iter().map(|s| s.as_bytes()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(open) = sheet[i..].iter().position(|&c| c == b'{').map(|p| p + i) {
        let Some(close) = sheet[open..].iter().position(|&c| c == b'}').map(|p| p + open) else { break };
        let body = &sheet[open + 1..close];
        if !body.contains(&b'{') {
            let pairs: Pairs = twidget_css::parse_declarations(body)
                .into_iter()
                .filter(|d| watched.contains(&d.name.as_slice()))
                .map(|d| (d.name, d.value))
                .collect();
            if !pairs.is_empty() {
                out.push(pairs);
            }
        }
        i = open + 1;
    }
    out
}
