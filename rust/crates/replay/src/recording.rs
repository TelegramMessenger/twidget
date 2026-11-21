//! `tests/recording.h`: a renderer used directly, recorded call by call in its process's order for a replay without
//! the source: what each non-const call was given, the Backend calls and clock reads it made and what it gave back,
//! in a strict binary file that ends with its own counts. The reader refuses what the C++ reader refuses, with the
//! same words; the writer is there for the tests.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use twidget_json::Json;
use twidget_remote::{GestureClaim, State};
use twidget_wire::{
    Caps, Damage, DisplayList, FontMetrics, decode_batch, decode_damage, decode_list, decode_metrics, encode_batch, encode_damage,
    encode_list, encode_metrics,
};

use crate::harness::{Call, Op};

pub const MAGIC: &[u8] = b"TWREC";
pub const VERSION: u8 = 1;
pub const MAX_DEPTH: i32 = 1024;

/// What a step records: a renderer's construction and destruction, and each of its non-const calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Kind {
    #[default]
    End = 0,
    Construct,
    Destroy,
    Receive,
    Frame,
    Touch,
    Hover,
    SetWidth,
    SetMaxHeight,
    SetTheme,
    ClearSelection,
    ClaimAt,
    WantFrame,
    Wheel,
    Key,
    Edit,
    EndEditing,
    SetCompositing,
    ImageReady,
    SetResizeTracking,
    Trim,
    Charge,
    SetPatching,
    LastHtml,
}

const KINDS: [Kind; 24] = [
    Kind::End,
    Kind::Construct,
    Kind::Destroy,
    Kind::Receive,
    Kind::Frame,
    Kind::Touch,
    Kind::Hover,
    Kind::SetWidth,
    Kind::SetMaxHeight,
    Kind::SetTheme,
    Kind::ClearSelection,
    Kind::ClaimAt,
    Kind::WantFrame,
    Kind::Wheel,
    Kind::Key,
    Kind::Edit,
    Kind::EndEditing,
    Kind::SetCompositing,
    Kind::ImageReady,
    Kind::SetResizeTracking,
    Kind::Trim,
    Kind::Charge,
    Kind::SetPatching,
    Kind::LastHtml,
];

impl Kind {
    pub fn of(byte: u8) -> Option<Kind> {
        KINDS.get(usize::from(byte)).copied()
    }

    /// `kind_name`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::End => "End",
            Kind::Construct => "Construct",
            Kind::Destroy => "Destroy",
            Kind::Receive => "Receive",
            Kind::Frame => "Frame",
            Kind::Touch => "Touch",
            Kind::Hover => "Hover",
            Kind::SetWidth => "SetWidth",
            Kind::SetMaxHeight => "SetMaxHeight",
            Kind::SetTheme => "SetTheme",
            Kind::ClearSelection => "ClearSelection",
            Kind::ClaimAt => "ClaimAt",
            Kind::WantFrame => "WantFrame",
            Kind::Wheel => "Wheel",
            Kind::Key => "Key",
            Kind::Edit => "Edit",
            Kind::EndEditing => "EndEditing",
            Kind::SetCompositing => "SetCompositing",
            Kind::ImageReady => "ImageReady",
            Kind::SetResizeTracking => "SetResizeTracking",
            Kind::Trim => "Trim",
            Kind::Charge => "Charge",
            Kind::SetPatching => "SetPatching",
            Kind::LastHtml => "LastHtml",
        }
    }
}

/// log_backend.h's `Call::Op` from its byte.
fn op_of(byte: u8) -> Option<Op> {
    [Op::Font, Op::CanvasFont, Op::DeleteFont, Op::TextWidth, Op::Image, Op::CanvasRun, Op::CanvasRead, Op::CanvasRelease, Op::Frame]
        .get(usize::from(byte))
        .copied()
}

/// One non-const call into one renderer: its arguments, the Backend calls and clock reads (µs) it made, and what the
/// renderer gave and showed after it.
#[derive(Clone, Debug, Default)]
pub struct Step {
    pub kind: Kind,
    pub session: u32,
    pub label: Vec<u8>,
    pub names: u64,
    pub name_bytes: u64,
    pub numbers: Vec<f64>,
    pub texts: Vec<Vec<u8>>,
    pub message: Option<Json>,
    pub clock: Vec<u64>,
    pub calls: Vec<Call>,
    pub messages: Vec<Vec<u8>>,
    pub engine: bool,
    pub cursor: Vec<u8>,
    pub painted: bool,
    pub list: DisplayList,
    pub damage: Damage,
    pub claim: GestureClaim,
    pub selection: Vec<u8>,
    pub state: State,
}

#[derive(Clone, Debug, Default)]
pub struct End {
    pub sessions: u32,
    pub steps: u64,
    pub error: Vec<u8>,
}

// ---- writing ------------------------------------------------------------------------------------------------------

pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_bits().to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_flag(out: &mut Vec<u8>, v: bool) {
    out.push(u8::from(v));
}

pub fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_json(out: &mut Vec<u8>, value: &Json, depth: i32) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match value {
        Json::Null => out.push(0),
        Json::Bool(b) => out.push(if *b { 2 } else { 1 }),
        Json::Number(d) => {
            out.push(3);
            put_f64(out, *d);
        }
        Json::String(s) => {
            out.push(4);
            put_bytes(out, s);
        }
        Json::Array(items) => {
            out.push(5);
            put_varint(out, items.len() as u64);
            for item in items {
                if !put_json(out, item, depth + 1) {
                    return false;
                }
            }
        }
        Json::Object(pairs) => {
            out.push(6);
            put_varint(out, pairs.len() as u64);
            for (key, item) in pairs {
                put_bytes(out, key);
                if !put_json(out, item, depth + 1) {
                    return false;
                }
            }
        }
    }
    true
}

fn put_metrics(out: &mut Vec<u8>, metrics: &FontMetrics) {
    let mut value = Vec::new();
    encode_metrics(metrics, &mut value);
    put_bytes(out, &value);
}

fn put_call(out: &mut Vec<u8>, c: &Call) -> bool {
    out.push(c.op as u8);
    match c.op {
        Op::Font | Op::CanvasFont => {
            put_bytes(out, &c.families);
            if c.op == Op::Font {
                put_bytes(out, &c.features);
            }
            put_f32(out, c.size);
            put_i32(out, c.weight);
            put_flag(out, c.italic);
            put_metrics(out, &c.metrics_in);
            put_metrics(out, &c.metrics);
            put_i32(out, c.font);
        }
        Op::DeleteFont => put_i32(out, c.font),
        Op::TextWidth => {
            put_i32(out, c.font);
            put_bytes(out, &c.text);
            put_f32(out, c.width);
        }
        Op::Image => {
            put_i32(out, c.id);
            put_bytes(out, &c.url);
            put_f32(out, c.w_in);
            put_f32(out, c.h_in);
            put_flag(out, c.ok);
            put_f32(out, c.image_w);
            put_f32(out, c.image_h);
        }
        Op::CanvasRun => {
            let mut value = Vec::new();
            if !encode_batch(&c.ops, &Caps::default(), &mut value) {
                return false;
            }
            put_i32(out, c.id);
            put_i32(out, c.w);
            put_i32(out, c.h);
            put_bytes(out, &value);
        }
        Op::CanvasRead => {
            put_i32(out, c.id);
            put_i32(out, c.x);
            put_i32(out, c.y);
            put_i32(out, c.w);
            put_i32(out, c.h);
            put_varint(out, c.rgba_in as u64);
            put_flag(out, c.ok);
            put_bytes(out, &c.rgba);
        }
        Op::CanvasRelease => put_i32(out, c.id),
        Op::Frame => put_flag(out, c.painted),
    }
    true
}

/// `Writer`: a recording written step by step.
#[derive(Debug, Default)]
pub struct Writer {
    out: Option<BufWriter<File>>,
    clocks: BTreeMap<u32, u64>,
}

impl Writer {
    pub fn open(&mut self, path: &Path, origin: &[u8]) -> bool {
        if self.out.is_some() || Json::parse(origin).is_none() {
            return false;
        }
        let Ok(file) = File::create(path) else { return false };
        let mut out = BufWriter::new(file);
        let mut head = MAGIC.to_vec();
        head.push(VERSION);
        put_bytes(&mut head, origin);
        let written = out.write_all(&head).is_ok();
        self.out = Some(out);
        written
    }

    fn frame(&mut self, record: &[u8]) -> bool {
        let Some(out) = self.out.as_mut() else { return false };
        let mut head = Vec::new();
        put_varint(&mut head, record.len() as u64);
        out.write_all(&head).is_ok() && out.write_all(record).is_ok()
    }

    /// False, nothing written, for a step no Reader would give back: a list, damage or batch that does not encode, a
    /// clock read before its session's last, a message nested too deep.
    pub fn write(&mut self, s: &Step) -> bool {
        if self.out.is_none() || s.kind == Kind::End || s.session == 0 {
            return false;
        }
        if !s.painted && (!s.list.is_empty() || s.damage.is_some()) {
            return false;
        }
        let mut r = vec![s.kind as u8];
        put_varint(&mut r, u64::from(s.session));
        put_bytes(&mut r, &s.label);
        put_varint(&mut r, s.names);
        put_varint(&mut r, s.name_bytes);
        put_varint(&mut r, s.numbers.len() as u64);
        for number in &s.numbers {
            put_f64(&mut r, *number);
        }
        put_varint(&mut r, s.texts.len() as u64);
        for text in &s.texts {
            put_bytes(&mut r, text);
        }
        put_flag(&mut r, s.message.is_some());
        if let Some(message) = &s.message
            && !put_json(&mut r, message, 0)
        {
            return false;
        }
        let mut last = self.clocks.get(&s.session).copied().unwrap_or(0);
        put_varint(&mut r, s.clock.len() as u64);
        for &read in &s.clock {
            if read < last {
                return false;
            }
            put_varint(&mut r, read - last);
            last = read;
        }
        put_varint(&mut r, s.calls.len() as u64);
        for c in &s.calls {
            if !put_call(&mut r, c) {
                return false;
            }
        }
        put_varint(&mut r, s.messages.len() as u64);
        for m in &s.messages {
            put_bytes(&mut r, m);
        }
        let flags = u8::from(s.engine)
            | u8::from(s.painted) << 1
            | u8::from(s.claim.x) << 2
            | u8::from(s.claim.y) << 3
            | u8::from(s.claim.hold) << 4
            | u8::from(s.state.dirty) << 5
            | u8::from(s.state.wants_tick) << 6
            | u8::from(s.state.frame_asked) << 7;
        r.push(flags);
        put_bytes(&mut r, &s.cursor);
        if s.painted {
            let mut value = Vec::new();
            if !encode_list(&s.list, &Caps::default(), &mut value) {
                return false;
            }
            put_bytes(&mut r, &value);
            value.clear();
            if !encode_damage(&s.damage, &Caps::default(), &mut value) {
                return false;
            }
            put_bytes(&mut r, &value);
        }
        put_bytes(&mut r, &s.selection);
        put_f64(&mut r, s.state.width);
        put_i32(&mut r, s.state.height);
        put_f64(&mut r, s.state.next_tick_in);
        if !self.frame(&r) {
            return false;
        }
        self.clocks.insert(s.session, last);
        true
    }

    pub fn close(&mut self, end: &End) -> bool {
        if self.out.is_none() {
            return false;
        }
        let mut r = vec![Kind::End as u8];
        put_varint(&mut r, u64::from(end.sessions));
        put_varint(&mut r, end.steps);
        put_bytes(&mut r, &end.error);
        let written = self.frame(&r);
        let flushed = self.out.take().is_some_and(|mut out| out.flush().is_ok());
        written && flushed
    }
}

// ---- reading ------------------------------------------------------------------------------------------------------

enum Got {
    Value,
    Short,
    Bad,
}

/// LEB128 of at most ten bytes in its shortest form; Short when `next` runs out first.
fn get_varint(mut next: impl FnMut(&mut u8) -> bool, v: &mut u64) -> Got {
    *v = 0;
    let mut shift = 0;
    while shift < 64 {
        let mut b = 0u8;
        if !next(&mut b) {
            return Got::Short;
        }
        if (shift == 63 && b > 1) || (shift > 0 && b == 0) {
            return Got::Bad;
        }
        *v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Got::Value;
        }
        shift += 7;
    }
    Got::Bad
}

/// The fields of one record, in order; false with `bad` empty when the record runs past its frame, else with `bad`
/// naming the value.
struct Take<'a> {
    rest: &'a [u8],
    bad: String,
}

impl<'a> Take<'a> {
    fn refuse(&mut self, why: String) -> bool {
        self.bad = why;
        false
    }
    fn byte(&mut self, v: &mut u8) -> bool {
        let Some((&b, rest)) = self.rest.split_first() else { return false };
        *v = b;
        self.rest = rest;
        true
    }
    fn varint(&mut self, v: &mut u64) -> bool {
        let mut rest = self.rest;
        let got = get_varint(
            |b| {
                let Some((&first, tail)) = rest.split_first() else { return false };
                *b = first;
                rest = tail;
                true
            },
            v,
        );
        self.rest = rest;
        match got {
            Got::Value => true,
            Got::Short => false,
            Got::Bad => self.refuse("a malformed number".to_string()),
        }
    }
    fn count(&mut self, n: &mut usize, item_bytes: usize) -> bool {
        let mut c = 0u64;
        if !self.varint(&mut c) || c > (self.rest.len() / item_bytes) as u64 {
            return false;
        }
        *n = c as usize;
        true
    }
    fn little<const N: usize>(&mut self) -> Option<[u8; N]> {
        if self.rest.len() < N {
            return None;
        }
        let (head, rest) = self.rest.split_at(N);
        self.rest = rest;
        head.try_into().ok()
    }
    fn f64(&mut self, v: &mut f64) -> bool {
        self.little::<8>().map(|b| *v = f64::from_bits(u64::from_le_bytes(b))).is_some()
    }
    fn f32(&mut self, v: &mut f32) -> bool {
        self.little::<4>().map(|b| *v = f32::from_bits(u32::from_le_bytes(b))).is_some()
    }
    fn i32(&mut self, v: &mut i32) -> bool {
        self.little::<4>().map(|b| *v = i32::from_le_bytes(b)).is_some()
    }
    fn flag(&mut self, v: &mut bool) -> bool {
        let mut b = 0u8;
        if !self.byte(&mut b) {
            return false;
        }
        if b > 1 {
            return self.refuse(format!("a flag of {b}"));
        }
        *v = b == 1;
        true
    }
    fn view(&mut self) -> Option<&'a [u8]> {
        let mut n = 0usize;
        if !self.count(&mut n, 1) {
            return None;
        }
        let (head, rest) = self.rest.split_at(n);
        self.rest = rest;
        Some(head)
    }
    fn bytes(&mut self, v: &mut Vec<u8>) -> bool {
        self.view().map(|b| *v = b.to_vec()).is_some()
    }
    fn metrics(&mut self, m: &mut FontMetrics) -> bool {
        let Some(b) = self.view() else { return false };
        match decode_metrics(b) {
            Some(decoded) => {
                *m = decoded;
                true
            }
            None => self.refuse("font metrics that do not decode".to_string()),
        }
    }

    fn json(&mut self, out: &mut Json, depth: i32) -> bool {
        if depth > MAX_DEPTH {
            return self.refuse(format!("a message nested past {MAX_DEPTH}"));
        }
        let mut tag = 0u8;
        if !self.byte(&mut tag) {
            return false;
        }
        match tag {
            0 => *out = Json::Null,
            1 => *out = Json::Bool(false),
            2 => *out = Json::Bool(true),
            3 => {
                let mut d = 0.0;
                if !self.f64(&mut d) {
                    return false;
                }
                *out = Json::Number(d);
            }
            4 => {
                let mut s = Vec::new();
                if !self.bytes(&mut s) {
                    return false;
                }
                *out = Json::String(s);
            }
            5 => {
                let mut n = 0usize;
                if !self.count(&mut n, 1) {
                    return false;
                }
                let mut items = Vec::new();
                for _ in 0..n {
                    let mut item = Json::Null;
                    if !self.json(&mut item, depth + 1) {
                        return false;
                    }
                    items.push(item);
                }
                *out = Json::Array(items);
            }
            6 => {
                let mut n = 0usize;
                if !self.count(&mut n, 2) {
                    return false;
                }
                let mut pairs = Vec::new();
                for _ in 0..n {
                    let mut key = Vec::new();
                    let mut item = Json::Null;
                    if !self.bytes(&mut key) || !self.json(&mut item, depth + 1) {
                        return false;
                    }
                    pairs.push((key, item));
                }
                *out = Json::Object(pairs);
            }
            _ => return self.refuse(format!("a message value of tag {tag}")),
        }
        true
    }

    fn call(&mut self, c: &mut Call) -> bool {
        let mut byte = 0u8;
        if !self.byte(&mut byte) {
            return false;
        }
        let Some(op) = op_of(byte) else { return self.refuse(format!("a call of unknown op {byte}")) };
        *c = Call { op, ..Call::default() };
        match op {
            Op::Font | Op::CanvasFont => {
                self.bytes(&mut c.families)
                    && (op == Op::CanvasFont || self.bytes(&mut c.features))
                    && self.f32(&mut c.size)
                    && self.i32(&mut c.weight)
                    && self.flag(&mut c.italic)
                    && self.metrics(&mut c.metrics_in)
                    && self.metrics(&mut c.metrics)
                    && self.i32(&mut c.font)
            }
            Op::DeleteFont => self.i32(&mut c.font),
            Op::TextWidth => self.i32(&mut c.font) && self.bytes(&mut c.text) && self.f32(&mut c.width),
            Op::Image => {
                self.i32(&mut c.id)
                    && self.bytes(&mut c.url)
                    && self.f32(&mut c.w_in)
                    && self.f32(&mut c.h_in)
                    && self.flag(&mut c.ok)
                    && self.f32(&mut c.image_w)
                    && self.f32(&mut c.image_h)
            }
            Op::CanvasRun => {
                if !self.i32(&mut c.id) || !self.i32(&mut c.w) || !self.i32(&mut c.h) {
                    return false;
                }
                let Some(value) = self.view() else { return false };
                match decode_batch(value, &Caps::default()) {
                    Some(ops) => {
                        c.ops = ops;
                        true
                    }
                    None => self.refuse("a canvas batch that does not decode".to_string()),
                }
            }
            Op::CanvasRead => {
                let mut pixels = 0u64;
                if !self.i32(&mut c.id)
                    || !self.i32(&mut c.x)
                    || !self.i32(&mut c.y)
                    || !self.i32(&mut c.w)
                    || !self.i32(&mut c.h)
                    || !self.varint(&mut pixels)
                    || !self.flag(&mut c.ok)
                {
                    return false;
                }
                let Some(value) = self.view() else { return false };
                let Ok(rgba_in) = usize::try_from(pixels) else { return self.refuse(format!("a pixel count of {pixels}")) };
                c.rgba_in = rgba_in;
                c.rgba = value.to_vec();
                true
            }
            Op::CanvasRelease => self.i32(&mut c.id),
            Op::Frame => self.flag(&mut c.painted),
        }
    }
}

fn take_step(t: &mut Take<'_>, s: &mut Step, clocks: &mut BTreeMap<u32, u64>) -> bool {
    let mut session = 0u64;
    if !t.varint(&mut session) {
        return false;
    }
    if session == 0 || session > u64::from(u32::MAX) {
        return t.refuse(format!("session {session}"));
    }
    s.session = session as u32;
    let mut n = 0usize;
    let mut message = false;
    if !t.bytes(&mut s.label) || !t.varint(&mut s.names) || !t.varint(&mut s.name_bytes) || !t.count(&mut n, 8) {
        return false;
    }
    s.numbers = vec![0.0; n];
    for number in &mut s.numbers {
        if !t.f64(number) {
            return false;
        }
    }
    if !t.count(&mut n, 1) {
        return false;
    }
    for _ in 0..n {
        let mut text = Vec::new();
        if !t.bytes(&mut text) {
            return false;
        }
        s.texts.push(text);
    }
    if !t.flag(&mut message) {
        return false;
    }
    if message {
        let mut m = Json::Null;
        if !t.json(&mut m, 0) {
            return false;
        }
        s.message = Some(m);
    }
    if !t.count(&mut n, 1) {
        return false;
    }
    let last = clocks.entry(s.session).or_insert(0);
    for _ in 0..n {
        let mut delta = 0u64;
        if !t.varint(&mut delta) {
            return false;
        }
        if delta > u64::MAX - *last {
            return t.refuse("a clock read past 64 bits".to_string());
        }
        *last += delta;
        s.clock.push(*last);
    }
    if !t.count(&mut n, 2) {
        return false;
    }
    for _ in 0..n {
        let mut c = Call::default();
        if !t.call(&mut c) {
            return false;
        }
        s.calls.push(c);
    }
    if !t.count(&mut n, 1) {
        return false;
    }
    for _ in 0..n {
        let mut m = Vec::new();
        if !t.bytes(&mut m) {
            return false;
        }
        s.messages.push(m);
    }
    let mut flags = 0u8;
    if !t.byte(&mut flags) || !t.bytes(&mut s.cursor) {
        return false;
    }
    s.engine = flags & 1 != 0;
    s.painted = flags & 2 != 0;
    s.claim = GestureClaim { x: flags & 4 != 0, y: flags & 8 != 0, hold: flags & 16 != 0 };
    s.state.dirty = flags & 32 != 0;
    s.state.wants_tick = flags & 64 != 0;
    s.state.frame_asked = flags & 128 != 0;
    if s.painted {
        let Some(value) = t.view() else { return false };
        match decode_list(value, &Caps::default()) {
            Some(list) => s.list = list,
            None => return t.refuse("a list that does not decode".to_string()),
        }
        let Some(value) = t.view() else { return false };
        match decode_damage(value, &Caps::default()) {
            Some(damage) => s.damage = damage,
            None => return t.refuse("damage that does not decode".to_string()),
        }
    }
    t.bytes(&mut s.selection) && t.f64(&mut s.state.width) && t.i32(&mut s.state.height) && t.f64(&mut s.state.next_tick_in)
}

/// `Reader`: a recording's steps in order, refused at the first thing the C++ reader refuses.
#[derive(Debug)]
pub struct Reader {
    input: Option<BufReader<File>>,
    left: u64,
    steps: u64,
    sessions: u32,
    clocks: BTreeMap<u32, u64>,
    origin: Vec<u8>,
    frame: Vec<u8>,
    error: String,
    end: End,
    ended: bool,
    done: bool,
}

impl Reader {
    /// The recording at `path` with its origin read; with `error()` set and nothing to give when it is refused.
    pub fn open(path: &Path) -> Reader {
        let mut r = Reader {
            input: None,
            left: 0,
            steps: 0,
            sessions: 0,
            clocks: BTreeMap::new(),
            origin: Vec::new(),
            frame: Vec::new(),
            error: String::new(),
            end: End::default(),
            ended: false,
            done: false,
        };
        let shown = path.display().to_string();
        let Ok(mut file) = File::open(path) else { return r.refused(format!("cannot open {shown}")) };
        let Ok(size) = file.seek(SeekFrom::End(0)).and_then(|size| file.seek(SeekFrom::Start(0)).map(|_| size)) else {
            return r.refused(format!("cannot read {shown}"));
        };
        r.left = size;
        r.input = Some(BufReader::new(file));
        let mut magic = [0u8; 5];
        let mut version = 0u8;
        if !r.read(&mut magic) || magic != MAGIC || !r.get(&mut version) {
            return r.refused("not a recording".to_string());
        }
        if version != VERSION {
            return r.refused(format!("version {version}"));
        }
        let mut n = 0u64;
        match r.length(&mut n) {
            Got::Bad => return r.refused("a malformed origin length".to_string()),
            Got::Short => return r.refused("truncated in the origin".to_string()),
            Got::Value if n > r.left => return r.refused("truncated in the origin".to_string()),
            Got::Value => {}
        }
        let mut origin = vec![0u8; n as usize];
        if !r.read(&mut origin) {
            return r.refused("truncated in the origin".to_string());
        }
        r.origin = origin;
        if Json::parse(&r.origin).is_none() {
            return r.refused("the origin is not JSON".to_string());
        }
        r
    }

    pub fn origin(&self) -> &[u8] {
        &self.origin
    }

    /// Whether `open` refused the recording.
    pub fn refused_at_open(&self) -> bool {
        self.done && !self.ended
    }

    /// The next step and the bytes its frame took; false at a complete end record, with `ended()`, or on a refusal,
    /// with `error()`.
    pub fn next(&mut self, step: &mut Step, bytes: &mut usize) -> bool {
        *bytes = 0;
        if self.done {
            return false;
        }
        let at = format!("step {}", self.steps + 1);
        if self.left == 0 {
            let why = format!("no end record after {} steps", self.steps);
            return self.refuse(why);
        }
        let before = self.left;
        let mut n = 0u64;
        match self.length(&mut n) {
            Got::Short => return self.refuse(format!("truncated in {at}")),
            Got::Bad => return self.refuse(format!("{at} has a malformed length")),
            Got::Value => {}
        }
        if n > self.left {
            let why =
                if self.peek() == Some(Kind::End as u8) { "truncated in the end record".to_string() } else { format!("truncated in {at}") };
            return self.refuse(why);
        }
        let mut frame = std::mem::take(&mut self.frame);
        frame.resize(n as usize, 0);
        let read = self.read(&mut frame);
        self.frame = frame;
        if !read {
            return self.refuse(format!("truncated in {at}"));
        }
        *bytes = (before - self.left) as usize;
        let Some(&kind) = self.frame.first() else { return self.refuse(format!("{at} does not fill its frame exactly")) };
        let frame = std::mem::take(&mut self.frame);
        let mut t = Take { rest: &frame[1..], bad: String::new() };
        if kind == Kind::End as u8 {
            let finished = self.finish(&mut t);
            self.frame = frame;
            return finished;
        }
        let Some(kind) = Kind::of(kind) else {
            self.frame = frame;
            return self.refuse(format!("{at} has unknown kind {kind}"));
        };
        *step = Step { kind, ..Step::default() };
        let taken = take_step(&mut t, step, &mut self.clocks) && t.rest.is_empty();
        let bad = std::mem::take(&mut t.bad);
        self.frame = frame;
        if !taken {
            return self.refuse(format!(
                "{at}{}",
                if bad.is_empty() { " does not fill its frame exactly".to_string() } else { format!(" holds {bad}") }
            ));
        }
        self.steps += 1;
        if step.kind == Kind::Construct {
            self.sessions += 1;
        }
        true
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn end(&self) -> &End {
        &self.end
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    fn refused(mut self, why: String) -> Reader {
        self.refuse(why);
        self
    }

    fn refuse(&mut self, why: String) -> bool {
        self.error = why;
        self.done = true;
        false
    }

    fn read(&mut self, to: &mut [u8]) -> bool {
        if to.len() as u64 > self.left {
            return false;
        }
        let Some(input) = self.input.as_mut() else { return false };
        if !to.is_empty() && input.read_exact(to).is_err() {
            return false;
        }
        self.left -= to.len() as u64;
        true
    }

    fn get(&mut self, b: &mut u8) -> bool {
        let mut c = [0u8; 1];
        if !self.read(&mut c) {
            return false;
        }
        *b = c[0];
        true
    }

    fn peek(&mut self) -> Option<u8> {
        self.input.as_mut().and_then(|input| input.fill_buf().ok().and_then(|buf| buf.first().copied()))
    }

    fn length(&mut self, n: &mut u64) -> Got {
        get_varint(|b| self.get(b), n)
    }

    fn finish(&mut self, t: &mut Take<'_>) -> bool {
        let mut sessions = 0u64;
        let mut steps = 0u64;
        let mut error = Vec::new();
        if !t.varint(&mut sessions) || !t.varint(&mut steps) || !t.bytes(&mut error) || !t.rest.is_empty() {
            let why = if t.bad.is_empty() {
                "the end record does not fill its frame exactly".to_string()
            } else {
                format!("the end record holds {}", t.bad)
            };
            return self.refuse(why);
        }
        self.end.steps = steps;
        self.end.error = error;
        if sessions > u64::from(u32::MAX) {
            return self.refuse(format!("the end record counts {sessions} sessions"));
        }
        self.end.sessions = sessions as u32;
        if self.left != 0 {
            return self.refuse("bytes after the end record".to_string());
        }
        if !self.end.error.is_empty() {
            let why = format!("the recording ends with an error: {}", String::from_utf8_lossy(&self.end.error));
            return self.refuse(why);
        }
        if u64::from(self.end.sessions) != u64::from(self.sessions) || self.end.steps != self.steps {
            let why = format!(
                "the end record counts {} sessions and {} steps where the file has {} and {}",
                self.end.sessions, self.end.steps, self.sessions, self.steps
            );
            return self.refuse(why);
        }
        self.ended = true;
        self.done = true;
        false
    }
}
