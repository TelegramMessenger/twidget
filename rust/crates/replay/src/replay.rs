//! `tests/replay.cpp`: each recorded renderer replayed through the two ends with the recorded Backend answers and
//! clock reads, every step compared member by member with what the recorded renderer gave; the C++'s renderer end
//! constructs TwidgetCore, this one the Rust renderer on `twidget_document`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use twidget_document::{DocumentEngine, Process};
use twidget_ends::{Output, Proxy};
use twidget_json::{Json, Surrogates};
use twidget_remote::{Caps, Frame, GestureClaim, Kind as Report, Paint, Pointer, State};
use twidget_wire::{DisplayList, FontMetrics, Rect};

use crate::compare::{hex, list_sharing, same_calls, same_damage, same_list};
use crate::harness::{Call, Expected, Loopback, Op, Wire, gesture_of, platform_only, predict, same_state};
use crate::recording::{Kind, Reader, Step};

const OP_NAMES: [&str; 8] =
    ["document fonts", "canvas fonts", "font releases", "widths", "images", "canvas runs", "pixel reads", "canvas releases"];

// ---- the recorded answers and clock -------------------------------------------------------------------------------

/// One step's recorded Backend answers, handed out in order past its Frame markers; a call of another kind, or one
/// past them, gets zeros and is named.
#[derive(Debug, Default)]
struct Answers {
    calls: Arc<Vec<Call>>,
    next: usize,
    taken: usize,
    mismatch: String,
}

impl Answers {
    fn start(&mut self, calls: Arc<Vec<Call>>) {
        self.calls = calls;
        self.next = 0;
        self.taken = 0;
        self.mismatch.clear();
    }

    /// The recorded calls no replayed call has taken.
    fn left(&self) -> usize {
        self.calls[self.next.min(self.calls.len())..].iter().filter(|c| c.op != Op::Frame).count()
    }

    fn take(&mut self, op: Op) -> Option<usize> {
        while self.next < self.calls.len() && self.calls[self.next].op == Op::Frame {
            self.next += 1;
        }
        let taken = self.taken;
        self.taken += 1;
        let at = || format!("replayed Backend call {taken} ({})", OP_NAMES[op as usize]);
        if self.next == self.calls.len() {
            return self.miss(format!("{} is past the recorded ones", at()));
        }
        let i = self.next;
        self.next += 1;
        if self.calls[i].op != op {
            let recorded = OP_NAMES[self.calls[i].op as usize];
            return self.miss(format!("{} is where the recording has one of the {recorded}", at()));
        }
        Some(i)
    }

    fn miss(&mut self, why: String) -> Option<usize> {
        if self.mismatch.is_empty() {
            self.mismatch = why;
        }
        None
    }

    fn font(&mut self, op: Op, out: &mut FontMetrics) -> i32 {
        let Some(i) = self.take(op) else { return 0 };
        *out = self.calls[i].metrics;
        self.calls[i].font
    }
}

/// The platform end's answers, shared with the replay that starts each step.
struct AnswersHandle(Arc<Mutex<Answers>>);

impl twidget_ends::Backend for AnswersHandle {
    fn create_font(&mut self, _: &[u8], _: f32, _: i32, _: bool, _: &[u8], out: &mut FontMetrics) -> i32 {
        self.0.lock().unwrap().font(Op::Font, out)
    }
    fn create_canvas_font(&mut self, _: &[u8], _: f32, _: i32, _: bool, out: &mut FontMetrics) -> i32 {
        self.0.lock().unwrap().font(Op::CanvasFont, out)
    }
    fn delete_font(&mut self, _: i32) {
        self.0.lock().unwrap().take(Op::DeleteFont);
    }
    fn text_width(&mut self, _: i32, _: &[u8]) -> f32 {
        let mut a = self.0.lock().unwrap();
        a.take(Op::TextWidth).map_or(0.0, |i| a.calls[i].width)
    }
    fn load_image(&mut self, _: i32, _: &[u8], w: &mut f32, h: &mut f32) -> bool {
        let mut a = self.0.lock().unwrap();
        let Some(i) = a.take(Op::Image) else { return false };
        let c = &a.calls[i];
        *w = c.image_w;
        *h = c.image_h;
        c.ok
    }
    fn canvas_run(&mut self, _: i32, _: i32, _: i32, _: &[twidget_wire::CanvasOp]) {
        self.0.lock().unwrap().take(Op::CanvasRun);
    }
    fn canvas_read(&mut self, _: i32, _: i32, _: i32, _: i32, _: i32, rgba: &mut Vec<u8>) -> bool {
        let mut a = self.0.lock().unwrap();
        let Some(i) = a.take(Op::CanvasRead) else { return false };
        let c = &a.calls[i];
        rgba.clone_from(&c.rgba);
        c.ok
    }
    fn canvas_release(&mut self, _: i32) {
        self.0.lock().unwrap().take(Op::CanvasRelease);
    }
}

/// One step's recorded clock reads, in µs and in order; a read past them repeats the session's last one and is marked.
#[derive(Debug, Default)]
struct ReplayClock {
    reads: Vec<u64>,
    next: usize,
    last: u64,
    over: bool,
}

impl ReplayClock {
    fn start(&mut self, reads: &[u64]) {
        self.reads.clear();
        self.reads.extend_from_slice(reads);
        self.next = 0;
        self.over = false;
    }
}

struct ClockHandle(Arc<Mutex<ReplayClock>>);

impl twidget_renderer::Clock for ClockHandle {
    fn now_ms(&mut self) -> f64 {
        let mut c = self.0.lock().unwrap();
        if c.next < c.reads.len() {
            c.last = c.reads[c.next];
            c.next += 1;
        } else {
            c.over = true;
        }
        c.last as f64 / 1000.0
    }
}

// ---- the renderer behind the end ----------------------------------------------------------------------------------

type Core = twidget_renderer::Renderer<DocumentEngine<Proxy<Wire>>, Proxy<Wire>>;

/// renderer.h's `Renderer` as the renderer end constructs it: the Rust renderer on a `twidget_document` engine, both on
/// the end's proxy, with the replay's clock.
pub struct Replayed(Core);

impl std::fmt::Debug for Replayed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Replayed").finish_non_exhaustive()
    }
}

impl twidget_ends::Renderer for Replayed {
    fn receive(&mut self, message: &Json) -> Vec<Json> {
        self.0.receive(message)
    }
    fn set_width(&mut self, width: f64, out: &mut Vec<Json>) {
        self.0.set_width(width, out)
    }
    fn set_max_height(&mut self, height: f64, out: &mut Vec<Json>) {
        self.0.set_max_height(height, out)
    }
    fn set_theme(&mut self, dark: bool, out: &mut Vec<Json>) {
        self.0.set_theme(dark, out)
    }
    fn touch(&mut self, action: i32, x: f64, y: f64, pointer: i32, primary: bool, mouse: bool, modifiers: i32, button: i32) -> Vec<Json> {
        self.0.touch(action, x, y, pointer, primary, mouse, modifiers, button)
    }
    fn hover(&mut self, action: i32, x: f64, y: f64, modifiers: i32) -> Vec<Json> {
        self.0.hover(action, x, y, modifiers)
    }
    fn cursor(&mut self) -> Vec<u8> {
        self.0.cursor()
    }
    fn claim_at(&mut self, x: f64, y: f64) -> GestureClaim {
        let c = self.0.claim_at(x, y);
        GestureClaim { x: c.x, y: c.y, hold: c.hold }
    }
    fn clear_selection(&mut self) -> bool {
        self.0.clear_selection()
    }
    fn may_have_selection(&mut self) -> bool {
        self.0.may_have_selection()
    }
    fn selected_text(&mut self) -> Vec<u8> {
        self.0.selected_text()
    }
    fn tick(&mut self) -> Vec<Json> {
        self.0.tick()
    }
    fn dirty(&mut self) -> bool {
        self.0.dirty()
    }
    fn paint(&mut self) -> DisplayList {
        self.0.paint()
    }
    fn damage(&mut self, out: &mut Vec<Rect>) -> bool {
        self.0.damage(out)
    }
    fn width(&mut self) -> f64 {
        self.0.width()
    }
    fn viewport_height(&mut self) -> i32 {
        self.0.viewport_height()
    }
    fn wants_tick(&mut self) -> bool {
        self.0.wants_tick()
    }
    fn next_tick_in(&mut self) -> f64 {
        self.0.next_tick_in()
    }
}

/// One recorded renderer replayed: its answers and clock, then the two ends over the in-memory channel, and what the
/// host was last told.
struct Session {
    answers: Arc<Mutex<Answers>>,
    clock: Arc<Mutex<ReplayClock>>,
    lp: Loopback<Replayed>,
    label: Vec<u8>,
    selection: Vec<u8>,
    sent: State,
    /// Which recorded and replayed SVG paint keys stand for each other, both ways: litehtml derives those keys from
    /// heap addresses.
    keys_to: BTreeMap<u64, u64>,
    keys_back: BTreeMap<u64, u64>,
    first_key: u64,
}

impl Session {
    fn new(caps: Caps) -> Session {
        let answers = Arc::new(Mutex::new(Answers::default()));
        let mut lp = Loopback::<Replayed>::made(caps);
        lp.omit_formatted_transcript();
        lp.backend.0.borrow_mut().answers = Some(Box::new(AnswersHandle(Arc::clone(&answers))));
        Session {
            answers,
            clock: Arc::new(Mutex::new(ReplayClock::default())),
            lp,
            label: Vec::new(),
            selection: Vec::new(),
            sent: State::default(),
            keys_to: BTreeMap::new(),
            keys_back: BTreeMap::new(),
            first_key: 0,
        }
    }
}

// ---- counts -------------------------------------------------------------------------------------------------------

/// What the replay compared, over a whole source or one labelled session.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub sessions: u64,
    pub steps: u64,
    pub bytes: u64,
    pub lists: u64,
    pub unchanged: u64,
    pub ticks: u64,
    pub engine: u64,
    pub calls: u64,
    pub pointers: u64,
    pub claims: u64,
    pub claiming: u64,
    pub holds: u64,
    pub holding: u64,
    pub selections: u64,
    pub states: u64,
    pub messages: u64,
    pub resizes: u64,
    pub themes: u64,
    pub violations: u64,
    pub clock: u64,
    pub keyed: u64,
    pub keys: u64,
    /// The renderer's messages compared, every step's (those the helper got back).
    pub returned: u64,
    pub ops: [u64; 8],
}

impl Counts {
    fn add(&mut self, step: &Step, frame_bytes: usize, lp: &Loopback<Replayed>, returned: &[Json], keyed: usize, new_keys: usize) {
        self.steps += 1;
        self.bytes += frame_bytes as u64;
        self.keyed += keyed as u64;
        self.keys += new_keys as u64;
        self.sessions += u64::from(step.kind == Kind::Construct);
        self.ticks += u64::from(step.kind == Kind::Frame && step.numbers[0] != 0.0);
        self.engine += u64::from(step.engine);
        self.resizes += u64::from(step.kind == Kind::SetWidth || step.kind == Kind::SetMaxHeight);
        self.themes += u64::from(step.kind == Kind::SetTheme);
        self.clock += step.clock.len() as u64;
        for c in &lp.backend.0.borrow().calls {
            if c.op == Op::Frame {
                continue;
            }
            self.calls += 1;
            self.ops[c.op as usize] += 1;
        }
        let host = lp.host.borrow();
        for f in &host.frames {
            if f.painted {
                self.lists += 1;
            } else {
                self.unchanged += 1;
            }
        }
        for p in &host.pointers {
            self.pointers += 1;
            self.claims += u64::from(p.claim.is_some());
            self.claiming += u64::from(p.claim.is_some_and(|c| c.x || c.y || c.hold));
        }
        for c in &host.claims {
            self.holds += 1;
            self.holding += u64::from(c.claim.x || c.claim.y || c.claim.hold);
        }
        self.selections += host.selections.len() as u64;
        self.states += host.states.len() as u64;
        self.messages += host.batches.len() as u64;
        self.returned += returned.len() as u64;
        self.violations += returned.iter().filter(|m| m.get("type").as_str() == b"violation").count() as u64;
    }

    /// Another source's counts added to these.
    pub fn absorb(&mut self, o: &Counts) {
        let pairs: [(&mut u64, u64); 23] = [
            (&mut self.sessions, o.sessions),
            (&mut self.steps, o.steps),
            (&mut self.bytes, o.bytes),
            (&mut self.lists, o.lists),
            (&mut self.unchanged, o.unchanged),
            (&mut self.ticks, o.ticks),
            (&mut self.engine, o.engine),
            (&mut self.calls, o.calls),
            (&mut self.pointers, o.pointers),
            (&mut self.claims, o.claims),
            (&mut self.claiming, o.claiming),
            (&mut self.holds, o.holds),
            (&mut self.holding, o.holding),
            (&mut self.selections, o.selections),
            (&mut self.states, o.states),
            (&mut self.messages, o.messages),
            (&mut self.resizes, o.resizes),
            (&mut self.themes, o.themes),
            (&mut self.violations, o.violations),
            (&mut self.clock, o.clock),
            (&mut self.keyed, o.keyed),
            (&mut self.keys, o.keys),
            (&mut self.returned, o.returned),
        ];
        for (mine, theirs) in pairs {
            *mine += theirs;
        }
        for (mine, theirs) in self.ops.iter_mut().zip(o.ops) {
            *mine += theirs;
        }
    }

    pub fn text(&self) -> String {
        let ops: Vec<String> = (0..8).map(|op| format!("{} {}", self.ops[op], OP_NAMES[op])).collect();
        format!(
            "{} sessions, {} steps, {} recorded bytes; frames {} painted, {} unchanged, {} ticks, {} engine frames; Backend {} calls ({}); host {} Pointer ({} with a claim, {} claiming), {} ClaimAnswer ({} claiming), {} Selection, {} State, {} Messages; {} keyed commands with {} distinct keys compared by correspondence; {} resizes, {} theme changes, {} violations, {} clock reads",
            self.sessions,
            self.steps,
            self.bytes,
            self.lists,
            self.unchanged,
            self.ticks,
            self.engine,
            self.calls,
            ops.join(", "),
            self.pointers,
            self.claims,
            self.claiming,
            self.holds,
            self.holding,
            self.selections,
            self.states,
            self.messages,
            self.keyed,
            self.keys,
            self.resizes,
            self.themes,
            self.violations,
            self.clock
        )
    }

    /// The count `--needs` names, nothing for a name it does not know.
    pub fn need(&self, name: &str) -> Option<u64> {
        Some(match name {
            "sessions" => self.sessions,
            "steps" => self.steps,
            "lists" => self.lists,
            "batches" => self.ops[Op::CanvasRun as usize],
            "reads" => self.ops[Op::CanvasRead as usize],
            "calls" => self.calls,
            "pointers" => self.pointers,
            "claims" => self.claims,
            "holds" => self.holds,
            "selections" => self.selections,
            "ticks" => self.ticks,
            "engine" => self.engine,
            "resizes" => self.resizes,
            "themes" => self.themes,
            "images" => self.ops[Op::Image as usize],
            "keys" => self.keyed,
            _ => return None,
        })
    }
}

// ---- options ------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Perturb {
    #[default]
    None,
    List,
    Call,
    Key,
}

impl Perturb {
    fn name(self) -> &'static str {
        match self {
            Perturb::List => "list",
            Perturb::Call => "call",
            _ => "key",
        }
    }
}

/// Where `--perturb key` changes a replayed key: the step, from 1, and the command of its painted list; a merge or a
/// repeated key.
#[derive(Clone, Copy, Debug)]
struct KeyTarget {
    step: u64,
    command: usize,
    merge: bool,
}

/// The first keyed command whose recorded key its session has painted before, else the first that brings a
/// session's second distinct key.
fn key_target(path: &Path) -> Option<KeyTarget> {
    let mut reader = Reader::open(path);
    if reader.refused_at_open() {
        return None;
    }
    let mut seen: BTreeMap<u32, BTreeSet<u64>> = BTreeMap::new();
    let mut merge = None;
    let mut step = Step::default();
    let mut bytes = 0;
    let mut index = 1u64;
    while reader.next(&mut step, &mut bytes) {
        if step.kind == Kind::Frame && step.painted {
            let keys = seen.entry(step.session).or_default();
            for (i, c) in step.list.iter().enumerate() {
                let key = c.key;
                if key == 0 {
                    continue;
                }
                if keys.contains(&key) {
                    return Some(KeyTarget { step: index, command: i, merge: false });
                }
                if keys.len() == 1 && merge.is_none() {
                    merge = Some(KeyTarget { step: index, command: i, merge: true });
                }
                keys.insert(key);
            }
        }
        index += 1;
    }
    merge
}

/// replay.cpp's options: the recording, the source's name, the counts it needs above zero, the sessions it must
/// hold, the message cap, and a perturbation.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub recording: PathBuf,
    pub source: String,
    pub needs: Vec<String>,
    pub sessions: Option<u64>,
    pub message_bytes: Option<u64>,
    pub perturb: Perturb,
    pub benchmark: bool,
}

impl Options {
    /// From the command line, as replay.cpp's `options_of` takes it; None for a usage error.
    pub fn parse(args: &[String]) -> Option<Options> {
        let mut o = Options::default();
        let mut recording = None;
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            let valued = matches!(arg, "--source" | "--needs" | "--sessions" | "--message-bytes" | "--perturb");
            if valued && i + 1 == args.len() {
                return None;
            }
            let number = |text: &str| -> Option<u64> {
                (!text.is_empty() && text.len() <= 19 && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse().ok()).flatten()
            };
            match arg {
                "--benchmark" => o.benchmark = true,
                "--source" => {
                    i += 1;
                    o.source = args[i].clone();
                }
                "--needs" => {
                    i += 1;
                    for name in args[i].split(',') {
                        Counts::default().need(name)?;
                        o.needs.push(name.to_string());
                    }
                }
                "--sessions" => {
                    i += 1;
                    o.sessions = Some(number(&args[i])?);
                }
                "--message-bytes" => {
                    i += 1;
                    let n = number(&args[i])?;
                    usize::try_from(n).ok()?;
                    o.message_bytes = Some(n);
                }
                "--perturb" => {
                    i += 1;
                    o.perturb = match args[i].as_str() {
                        "list" => Perturb::List,
                        "call" => Perturb::Call,
                        "key" => Perturb::Key,
                        _ => return None,
                    };
                }
                _ if arg.is_empty() || arg.starts_with('-') || recording.is_some() => return None,
                _ => recording = Some(PathBuf::from(arg)),
            }
            i += 1;
        }
        o.recording = recording?;
        (!o.source.is_empty() && (!o.benchmark || o.perturb == Perturb::None)).then_some(o)
    }
}

/// The numbers a step of `kind` carries, a letter each (d a double, f a float, i an int, b a bool, u an edit
/// generation, p a Paint), and how many texts.
fn shape_of(kind: Kind) -> (&'static str, usize) {
    match kind {
        Kind::Construct => ("ddb", 0),
        Kind::Frame => ("bp", 0),
        Kind::Touch => ("iddibbii", 0),
        Kind::Hover => ("iddi", 0),
        Kind::SetWidth | Kind::SetMaxHeight | Kind::Charge => ("d", 0),
        Kind::SetTheme | Kind::SetCompositing | Kind::SetPatching => ("b", 0),
        Kind::ClaimAt => ("dd", 0),
        Kind::SetResizeTracking => ("bb", 0),
        Kind::Wheel => ("ddddii", 0),
        Kind::Key => ("bi", 3),
        Kind::Edit => ("u", 2),
        Kind::ImageReady => ("iff", 0),
        _ => ("", 0),
    }
}

fn fits(kind: u8, v: f64) -> bool {
    match kind {
        b'd' => true,
        b'f' => v.is_nan() || v.is_infinite() || (v.abs() <= f64::from(f32::MAX) && f64::from(v as f32) == v),
        b'i' => v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX) && v == v.floor(),
        b'b' => v == 0.0 || v == 1.0,
        b'u' => (0.0..=9007199254740992.0).contains(&v) && v == v.floor(),
        b'p' => v == 0.0 || v == 1.0 || v == 2.0,
        _ => false,
    }
}

fn well_formed(step: &Step) -> bool {
    let (numbers, texts) = shape_of(step.kind);
    if step.numbers.len() != numbers.len() || step.texts.len() != texts || step.message.is_some() != (step.kind == Kind::Receive) {
        return false;
    }
    numbers.bytes().zip(&step.numbers).all(|(kind, &v)| fits(kind, v))
}

/// A call the channel does not carry, made by the helper on the end's renderer with the recorded arguments.
fn helper(r: &mut Replayed, step: &Step) -> Vec<Json> {
    let n = &step.numbers;
    let t = &step.texts;
    let r = &mut r.0;
    match step.kind {
        Kind::Wheel => r.wheel(n[0], n[1], n[2], n[3], n[4] as i32, n[5] as i32),
        Kind::Key => r.key(&t[0], &t[1], &t[2], n[0] != 0.0, n[1] as i32),
        Kind::Edit => r.edit(n[0] as u64, &t[0], &t[1]),
        Kind::EndEditing => r.end_editing(),
        Kind::SetCompositing => {
            r.set_compositing(n[0] != 0.0);
            Vec::new()
        }
        Kind::ImageReady => r.image_ready(n[0] as i32, n[1] as f32, n[2] as f32),
        Kind::SetResizeTracking => {
            r.set_resize_tracking(n[0] != 0.0, n[1] != 0.0);
            Vec::new()
        }
        Kind::Trim => {
            r.trim();
            Vec::new()
        }
        Kind::Charge => {
            r.core_mut().charge(n[0]);
            Vec::new()
        }
        Kind::SetPatching => {
            r.set_patching(n[0] != 0.0);
            Vec::new()
        }
        Kind::LastHtml => {
            r.last_html();
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn forget(lp: &mut Loopback<Replayed>) {
    {
        let mut h = lp.host.borrow_mut();
        h.events.clear();
        h.frames.clear();
        h.states.clear();
        h.batches.clear();
        h.pointers.clear();
        h.selections.clear();
        h.claims.clear();
        h.refusals.clear();
        h.lines.clear();
    }
    lp.backend.0.borrow_mut().calls.clear();
    lp.outputs.clear();
    lp.delivered.clear();
    let mut w = lp.wire.borrow_mut();
    w.kinds.clear();
    w.traffic.clear();
}

// ---- reports.h ----------------------------------------------------------------------------------------------------

fn dumps(messages: &[Json]) -> Vec<u8> {
    let mut out = Vec::new();
    for m in messages {
        out.extend_from_slice(&m.dump());
        out.push(b'\n');
    }
    out
}

fn report_name(kind: Report) -> String {
    match kind {
        Report::Frame => "Frame".to_string(),
        Report::State => "State".to_string(),
        Report::Messages => "Messages".to_string(),
        Report::Pointer => "Pointer".to_string(),
        Report::Selection => "Selection".to_string(),
        Report::ClaimAnswer => "ClaimAnswer".to_string(),
        other => format!("kind {}", other as u8),
    }
}

fn reports_name(kinds: &[Report]) -> String {
    if kinds.is_empty() {
        return "nothing".to_string();
    }
    kinds.iter().map(|k| report_name(*k)).collect::<Vec<_>>().join(" ")
}

fn claim_name(claim: Option<GestureClaim>) -> String {
    match claim {
        None => "none".to_string(),
        Some(c) => format!("{}{}{}", if c.x { "x" } else { "-" }, if c.y { "y" } else { "-" }, if c.hold { "h" } else { "-" }),
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A step's failures, in the order its checks run.
#[derive(Debug, Default)]
struct Failures(Vec<String>);

impl Failures {
    fn ok(&mut self, cond: bool, what: String) {
        if !cond {
            self.0.push(what);
        }
    }
}

/// reports.h's `healthy`.
fn healthy(f: &mut Failures, lp: &mut Loopback<Replayed>) {
    let stopped = lp.end().stopped();
    let reason = lp.end().stop_reason();
    let refusals = lp.host.borrow().refusals.clone();
    let quiet = lp.wire.borrow().refused == 0 && !lp.refusing() && lp.queue.0.borrow().is_empty();
    f.ok(
        !stopped && refusals.is_empty() && quiet,
        format!(
            "both ends stay healthy{}{}",
            if stopped { format!(": {reason}") } else { String::new() },
            refusals.first().map(|r| format!(": {r}")).unwrap_or_default()
        ),
    );
}

/// reports.h's `compare_reports`.
fn compare_reports(f: &mut Failures, s: &Expected, lp: &Loopback<Replayed>, returned: &[Json], out: Option<&Output>) {
    f.ok(dumps(returned) == dumps(&s.messages), "the helper gets every message the direct renderer gave".to_string());
    if let Some(out) = out {
        f.ok(out.frame == s.engine, format!("the engine's frame is {}", if s.engine { "due" } else { "not due" }));
    }
    let host = lp.host.borrow();
    let heard = host.events.clone();
    f.ok(heard == s.events, format!("the host hears {}, not {}", reports_name(&s.events), reports_name(&heard)));
    if heard == s.events {
        for kind in &heard {
            match kind {
                Report::Messages => f.ok(
                    host.batches.last().is_some_and(|b| dumps(b) == dumps(&platform_only(&s.messages))),
                    "the Messages report holds the call's platform messages alone".to_string(),
                ),
                Report::Pointer => {
                    let p = host.pointers.last().cloned().unwrap_or_default();
                    let want = s.pointer.clone().unwrap_or_default();
                    f.ok(
                        p.cursor == want.cursor && p.claim == want.claim,
                        format!(
                            "the Pointer report is {} and {}, not {} and {}",
                            lossy(&want.cursor),
                            claim_name(want.claim),
                            lossy(&p.cursor),
                            claim_name(p.claim)
                        ),
                    );
                }
                Report::Selection => f.ok(
                    host.selections.last().is_some_and(|sel| sel.text == s.selection),
                    format!("the Selection report is the direct selected text '{}'", lossy(&s.selection)),
                ),
                Report::Frame => {
                    let got = host.frames.last().cloned().unwrap_or_default();
                    let want = s.frame.clone().unwrap_or_default();
                    f.ok(
                        got.seq == s.seq && got.painted == want.painted,
                        format!("the Frame answers request {}{}", s.seq, if want.painted { ", painted" } else { ", unpainted" }),
                    );
                    let list = same_list(&want.list, &got.list);
                    f.ok(list.is_ok(), format!("the frame's list is the direct paint {}", list.err().unwrap_or_default()));
                    f.ok(
                        list_sharing(&want.list) == list_sharing(&got.list),
                        "the frame's list shares its payloads as the direct paint does".to_string(),
                    );
                    let damage = same_damage(&want.damage, &got.damage);
                    f.ok(damage.is_ok(), format!("the frame's damage is the direct damage {}", damage.err().unwrap_or_default()));
                    f.ok(same_state(&got.state, &s.state), "the Frame carries the direct state".to_string());
                }
                Report::ClaimAnswer => {
                    let c = host.claims.last().copied().unwrap_or_default();
                    f.ok(
                        c.seq == s.seq && Some(c.claim) == s.claim,
                        format!("the ClaimAnswer is {}, not {}", claim_name(s.claim), claim_name(Some(c.claim))),
                    );
                }
                Report::State => f.ok(
                    host.states.last().is_some_and(|state| same_state(state, &s.state)),
                    "the State report is the direct state".to_string(),
                ),
                _ => {}
            }
        }
    }
    drop(host);
    f.ok(same_state(&lp.platform_state(), &s.state), "the platform end's state is the direct renderer's".to_string());
}

// ---- the replay ---------------------------------------------------------------------------------------------------

/// One recording replayed: its sessions, each through its own two ends, all on one process's litehtml, as the C++
/// replay runs them in a fresh process.
#[derive(Debug)]
pub struct Replay {
    o: Options,
    caps: Caps,
    process: Arc<Process>,
    live: BTreeMap<u32, SessionBox>,
    labelled: BTreeMap<u32, (Vec<u8>, Counts)>,
    total: Counts,
    timings: BTreeMap<u32, SessionTiming>,
    index: u64,
    made: u32,
    most_alive: usize,
    perturbed: bool,
    key_target: Option<KeyTarget>,
    /// What the run printed, in order.
    pub lines: Vec<String>,
    /// Whether lines are printed as they come too.
    pub echo: bool,
    /// The step and the failures of the step that stopped the run.
    pub failure: Option<(u64, Vec<String>)>,
}

struct SessionBox(Box<Session>);

#[derive(Debug, Default)]
struct SessionTiming {
    label: Vec<u8>,
    steps: Vec<(Kind, u64)>,
    frames: Vec<(usize, bool, usize)>,
    first_paint_step: Option<usize>,
}

impl SessionTiming {
    fn add(&mut self, step: &Step, ns: u64) {
        if step.kind == Kind::Construct {
            self.label.clone_from(&step.label);
        }
        self.steps.push((step.kind, ns));
        if step.kind == Kind::Frame {
            let index = self.steps.len();
            self.frames.push((index, step.painted, step.list.len()));
            if step.painted && !step.list.is_empty() && self.first_paint_step.is_none() {
                self.first_paint_step = Some(index);
            }
        }
    }

    fn json(&self, id: u32) -> Json {
        let mut report = Json::object();
        report.set("id", id).set("label", self.label.as_slice());
        report.set("first_paint_step", self.first_paint_step.map(Json::from).unwrap_or_default());
        report.set(
            "first_paint_ns",
            self.first_paint_step.map(|n| Json::from(self.steps[..n].iter().map(|s| s.1).sum::<u64>())).unwrap_or_default(),
        );
        report.set("steps", self.steps.iter().map(|(kind, ns)| Json::Array(vec![kind.name().into(), (*ns).into()])).collect::<Vec<_>>());
        report.set(
            "frames",
            self.frames
                .iter()
                .map(|&(index, painted, commands)| Json::Array(vec![index.into(), painted.into(), commands.into()]))
                .collect::<Vec<_>>(),
        );
        report
    }
}

impl std::fmt::Debug for SessionBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("label", &lossy(&self.0.label)).finish_non_exhaustive()
    }
}

impl Replay {
    pub fn new(o: Options) -> Replay {
        let mut caps = Caps::default();
        if let Some(bytes) = o.message_bytes {
            caps.message = bytes as usize;
        }
        Replay {
            o,
            caps,
            process: Arc::new(Process::default()),
            live: BTreeMap::new(),
            labelled: BTreeMap::new(),
            total: Counts::default(),
            timings: BTreeMap::new(),
            index: 0,
            made: 0,
            most_alive: 0,
            perturbed: false,
            key_target: None,
            lines: Vec::new(),
            echo: false,
            failure: None,
        }
    }

    /// What the whole source compared.
    pub fn counts(&self) -> &Counts {
        &self.total
    }

    /// Each labelled session's label and counts.
    pub fn labelled(&self) -> impl Iterator<Item = (&[u8], &Counts)> {
        self.labelled.values().map(|(label, counts)| (label.as_slice(), counts))
    }

    fn print(&mut self, line: String) {
        if self.echo {
            println!("{line}");
        }
        self.lines.push(line);
    }

    fn refuse(&mut self, why: String) -> i32 {
        let line = format!("{}: {why}", self.o.source);
        self.print(line);
        2
    }

    fn at(&self, step: &Step) -> String {
        let label = if step.kind == Kind::Construct {
            step.label.clone()
        } else {
            self.live.get(&step.session).map(|s| s.0.label.clone()).unwrap_or_default()
        };
        format!(
            "step {}, a {} of session {}{}",
            self.index,
            step.kind.name(),
            step.session,
            if label.is_empty() { String::new() } else { format!(" ({})", lossy(&label)) }
        )
    }

    /// Changes the first eligible recorded value once, and gives the start of the one failure that must name it.
    fn perturb(&mut self, step: &mut Step) -> String {
        if self.perturbed {
            return String::new();
        }
        if self.o.perturb == Perturb::List && step.kind == Kind::Frame && step.painted && !step.list.is_empty() {
            let x = step.list[0].rect.x;
            let moved = x.next_up();
            if !x.is_finite() || moved.to_bits() == x.to_bits() {
                return String::new();
            }
            let line = format!(
                "{}: --perturb list moves the list's [0].rect.x of {} from {} to {}",
                self.o.source,
                self.at(step),
                hex(u64::from(x.to_bits())),
                hex(u64::from(moved.to_bits()))
            );
            self.print(line);
            step.list[0].rect.x = moved;
            self.perturbed = true;
            return "the frame's list is the direct paint [0].rect.x: ".to_string();
        }
        if self.o.perturb == Perturb::Call {
            for i in 0..step.calls.len() {
                if step.calls[i].op != Op::TextWidth {
                    continue;
                }
                let line = format!("{}: --perturb call appends a byte to the text of Backend call {i} of {}", self.o.source, self.at(step));
                self.print(line);
                step.calls[i].text.push(b'x');
                self.perturbed = true;
                return format!("the Backend calls are the direct renderer's, in order call[{i}].text: ");
            }
        }
        String::new()
    }

    /// Holds the session's one-to-one correspondence of keys over a heard list (zero only with zero, any other key
    /// always with the same partner), then gives the heard list the recorded keys, so the comparison sees all else;
    /// the first command that breaks it otherwise.
    fn correspond(&mut self, id: u32, recorded: &DisplayList, keyed: &mut usize, fresh: &mut usize) -> Result<(), String> {
        let index = self.index;
        let source = self.o.source.clone();
        let target = self.key_target;
        let mut perturbed = self.perturbed;
        let mut printed = None;
        let session = &mut self.live.get_mut(&id).expect("a live session").0;
        let mut host = session.lp.host.borrow_mut();
        let Some(heard) = host.frames.last_mut().map(|f| &mut f.list) else { return Ok(()) };
        let n = recorded.len().min(heard.len());
        let mut held = Ok(());
        for i in 0..n {
            let r = recorded[i].key;
            let k = &mut heard[i].key;
            if let Some(t) = target
                && !perturbed
                && index == t.step
                && i == t.command
            {
                let was = *k;
                let first = session.keys_to.get(&session.first_key).copied();
                *k = if t.merge && first.is_some() { first.unwrap_or_default() } else { *k ^ 1 };
                perturbed = true;
                printed = Some(format!(
                    "{source}: --perturb key changes the replayed [{i}].key of step {index} from {} to {}, {}",
                    hex(was),
                    hex(*k),
                    if t.merge { "the session's first key, merging two" } else { "its lowest bit flipped where the recorded key repeats" }
                ));
            }
            let k = *k;
            if r == 0 && k == 0 {
                continue;
            }
            let to = if r != 0 { session.keys_to.get(&r).copied() } else { None };
            let back = if k != 0 { session.keys_back.get(&k).copied() } else { None };
            let broken = if r == 0 || k == 0 {
                "a key against none".to_string()
            } else if let Some(partner) = to.filter(|&p| p != k) {
                format!("where the session's earlier commands gave {} the key {}", hex(r), hex(partner))
            } else if let (None, Some(other)) = (to, back) {
                format!("where {} already stands for {}", hex(k), hex(other))
            } else {
                String::new()
            };
            if !broken.is_empty() {
                held = Err(format!("[{i}].key: {} against {}, {broken}", hex(r), hex(k)));
                break;
            }
            *keyed += 1;
            if to.is_some() {
                continue;
            }
            session.keys_to.insert(r, k);
            session.keys_back.insert(k, r);
            if session.first_key == 0 {
                session.first_key = r;
            }
            *fresh += 1;
        }
        for i in 0..n {
            heard[i].key = recorded[i].key;
        }
        drop(host);
        self.perturbed = perturbed;
        if let Some(line) = printed {
            self.print(line);
        }
        held
    }

    /// What the recorded renderer gave, with the reports the host must hear of it; None when a recorded message does
    /// not read back as it was written.
    fn expect(step: &Step, session: &Session) -> Option<Expected> {
        let mut s = Expected::default();
        for text in &step.messages {
            let m = Json::parse_with(text, Surrogates::Keep, 4096)?;
            if &m.dump() != text {
                return None;
            }
            s.messages.push(m);
        }
        s.engine = step.engine;
        if step.kind == Kind::Touch || step.kind == Kind::Hover {
            s.pointer = Some(Pointer { cursor: step.cursor.clone(), claim: gesture_of(&s.messages) });
        }
        if step.kind == Kind::Frame {
            s.frame = Some(Frame { painted: step.painted, list: step.list.clone(), damage: step.damage.clone(), ..Frame::default() });
        }
        if step.kind == Kind::ClaimAt {
            s.claim = Some(step.claim);
        }
        s.selection = step.selection.clone();
        s.state = step.state;
        if step.kind == Kind::Construct {
            if !s.selection.is_empty() {
                s.events.push(Report::Selection);
            }
            s.events.push(Report::State);
        } else if step.kind != Kind::Destroy {
            predict(&mut s, &session.selection, &session.sent);
        }
        Some(s)
    }

    fn print_failures(&mut self, step: &Step, failures: &[String]) {
        let mut lines = vec![format!("{}: {} differs:", self.o.source, self.at(step))];
        for f in failures.iter().take(6) {
            lines.push(format!("  {f}"));
        }
        if failures.len() > 6 {
            lines.push(format!("  and {} more", failures.len() - 6));
        }
        for line in lines {
            self.print(line);
        }
    }

    /// One step through its session's two ends: None when it matched, else the exit code.
    fn play(&mut self, step: &mut Step, bytes: usize) -> Option<i32> {
        let kind = step.kind;
        if !well_formed(step) {
            let why = format!("{} does not hold the arguments such a step takes", self.at(step));
            return Some(self.refuse(why));
        }
        if kind == Kind::Construct {
            if self.live.contains_key(&step.session) || step.session <= self.made {
                let why = format!("{} makes a session that is not new", self.at(step));
                return Some(self.refuse(why));
            }
            self.made = step.session;
            let mut session = Session::new(self.caps);
            session.label = step.label.clone();
            self.live.insert(step.session, SessionBox(Box::new(session)));
            self.most_alive = self.most_alive.max(self.live.len());
        } else if !self.live.contains_key(&step.session) {
            let why = format!("{} names no live session", self.at(step));
            return Some(self.refuse(why));
        }
        let mut f = Failures::default();
        let usage = self.process.names.usage();
        f.ok(
            usage.names as u64 == step.names && usage.bytes as u64 == step.name_bytes,
            format!(
                "litehtml's name table holds {} names of {} bytes where the recording had {} of {}",
                usage.names, usage.bytes, step.names, step.name_bytes
            ),
        );
        let mut leaf = self.perturb(step);
        if let Some(t) = self.key_target
            && !self.perturbed
            && self.index == t.step
        {
            leaf = format!("the frame's list is the direct paint [{}].key: ", t.command);
        }
        let Some(mut s) = Replay::expect(step, &self.live[&step.session].0) else {
            let why = format!("{} holds a message that does not read back as it was recorded", self.at(step));
            return Some(self.refuse(why));
        };
        let calls = Arc::new(std::mem::take(&mut step.calls));
        let process = Arc::clone(&self.process);
        let session = &mut self.live.get_mut(&step.session).expect("a live session").0;
        session.answers.lock().unwrap().start(Arc::clone(&calls));
        session.clock.lock().unwrap().start(&step.clock);
        let n = &step.numbers;
        let lp = &mut session.lp;
        let mut returned: Vec<Json> = Vec::new();
        let mut out: Option<Output> = None;
        let mut input = false;
        let mut stopped = false;
        let start = self.o.benchmark.then(Instant::now);
        match kind {
            Kind::Construct => {
                let clock = Arc::clone(&session.clock);
                lp.open_with(n[0], n[1], n[2] != 0.0, |w, d, k, proxy| {
                    let engine = DocumentEngine::new(process, proxy.clone());
                    Replayed(twidget_renderer::Renderer::new(w, d, k, engine, proxy, Some(Box::new(ClockHandle(clock)))))
                });
            }
            Kind::Destroy => lp.close(),
            Kind::Receive => returned = lp.receive(step.message.as_ref().expect("a received message")),
            Kind::WantFrame => lp.want_frame(),
            Kind::Frame => {
                let paint = match n[1] as u8 {
                    0 => Paint::Skip,
                    1 => Paint::IfDirty,
                    _ => Paint::Force,
                };
                s.seq = lp.platform.borrow_mut().request_frame(n[0] != 0.0, paint);
                input = true;
            }
            Kind::Touch => {
                lp.platform.borrow_mut().touch(n[0] as i32, n[1], n[2], n[3] as i32, n[4] != 0.0, n[5] != 0.0, n[6] as i32, n[7] as i32);
                input = true;
            }
            Kind::Hover => {
                lp.platform.borrow_mut().hover(n[0] as i32, n[1], n[2], n[3] as i32);
                input = true;
            }
            Kind::SetWidth => {
                lp.platform.borrow_mut().set_width(n[0]);
                input = true;
            }
            Kind::SetMaxHeight => {
                lp.platform.borrow_mut().set_max_height(n[0]);
                input = true;
            }
            Kind::SetTheme => {
                lp.platform.borrow_mut().set_theme(n[0] != 0.0);
                input = true;
            }
            Kind::ClearSelection => {
                lp.platform.borrow_mut().clear_selection();
                input = true;
            }
            Kind::ClaimAt => {
                s.seq = lp.platform.borrow_mut().ask_claim(n[0], n[1]);
                input = true;
            }
            _ => {
                stopped = lp.end().stopped();
                if !stopped {
                    returned = helper(lp.end().renderer(), step);
                    lp.report(&returned);
                }
            }
        }
        let pumped = input && lp.pump();
        if let Some(start) = start {
            let ns = start.elapsed().as_nanos() as u64;
            self.timings.entry(step.session).or_default().add(step, ns);
        }
        if stopped {
            f.0.push("the helper calls the renderer of a healthy end".to_string());
        }
        if input {
            f.ok(
                pumped && lp.delivered.len() == 1 && lp.delivered.last() == Some(&true),
                "the platform input is delivered and taken".to_string(),
            );
            let o = lp.outputs.last().cloned().unwrap_or_default();
            returned = o.messages.clone();
            out = Some(o);
        }
        let (mut keyed, mut fresh) = (0usize, 0usize);
        if kind == Kind::Destroy {
            let lp = &self.live[&step.session].0.lp;
            let quiet = lp.host.borrow().events.is_empty()
                && lp.host.borrow().refusals.is_empty()
                && lp.wire.borrow().refused == 0
                && !lp.refusing()
                && lp.queue.0.borrow().is_empty();
            f.ok(quiet, "destruction reports nothing and the platform end takes all of it".to_string());
        } else {
            healthy(&mut f, &mut self.live.get_mut(&step.session).expect("a live session").0.lp);
            let heard_frame = !self.live[&step.session].0.lp.host.borrow().frames.is_empty();
            if let Some(frame) = s.frame.as_ref().filter(|_| heard_frame) {
                let recorded = frame.list.clone();
                if let Err(broken) = self.correspond(step.session, &recorded, &mut keyed, &mut fresh) {
                    f.0.push(format!("the frame's list is the direct paint {broken}"));
                }
            }
            compare_reports(&mut f, &s, &self.live[&step.session].0.lp, &returned, out.as_ref());
        }
        let session = &self.live[&step.session].0;
        if let Err(why) = same_calls(&calls, &session.lp.backend.0.borrow().calls) {
            f.0.push(format!("the Backend calls are the direct renderer's, in order {why}"));
        }
        let answers = session.answers.lock().unwrap();
        if !answers.mismatch.is_empty() {
            f.0.push(answers.mismatch.clone());
        }
        let left = answers.left();
        if left != 0 {
            f.0.push(format!("{left} recorded Backend calls are never made"));
        }
        drop(answers);
        let clock = session.clock.lock().unwrap();
        if clock.over || clock.next != step.clock.len() {
            f.0.push(format!(
                "the renderer reads the clock {}{} recorded times",
                if clock.over { "past its ".to_string() } else { format!("{} of its ", clock.next) },
                step.clock.len()
            ));
        }
        drop(clock);
        let failures = f.0;
        step.calls = Arc::try_unwrap(calls).unwrap_or_else(|shared| (*shared).clone());
        if !leaf.is_empty() {
            let name = self.o.perturb.name();
            if failures.len() == 1 && failures[0].starts_with(&leaf) {
                let line =
                    format!("{}: --perturb {name} is caught at {}, on that leaf alone: {}", self.o.source, self.at(step), failures[0]);
                self.print(line);
                return Some(0);
            }
            if !failures.is_empty() {
                self.print_failures(step, &failures);
            }
            let line = format!("{}: --perturb {name} is not caught at {} on precisely the changed leaf", self.o.source, self.at(step));
            self.print(line);
            self.failure = Some((self.index, failures));
            return Some(1);
        }
        if !failures.is_empty() {
            self.print_failures(step, &failures);
            self.failure = Some((self.index, failures));
            return Some(1);
        }
        let session = &self.live[&step.session].0;
        self.total.add(step, bytes, &session.lp, &returned, keyed, fresh);
        if !session.label.is_empty() {
            let entry = self.labelled.entry(step.session).or_default();
            entry.0.clone_from(&session.label);
            entry.1.add(step, bytes, &session.lp, &returned, keyed, fresh);
        }
        let session = &mut self.live.get_mut(&step.session).expect("a live session").0;
        session.selection = s.selection;
        session.sent = s.state;
        forget(&mut session.lp);
        if kind == Kind::Destroy {
            self.live.remove(&step.session);
        }
        None
    }

    /// The whole recording: 0 when every step of every session matches and the counts it needs are there, 1 on a
    /// difference or a missing count, 2 when the recording is refused; a perturbed run gives 0 when its change is caught
    /// where it was made, on that leaf alone.
    pub fn run(&mut self) -> i32 {
        let path = self.o.recording.clone();
        let mut reader = Reader::open(&path);
        if reader.refused_at_open() {
            let why = format!("the recording is refused: {}", reader.error());
            return self.refuse(why);
        }
        let line = format!("{}: origin {}", self.o.source, lossy(reader.origin()));
        self.print(line);
        if self.o.perturb == Perturb::Key {
            self.key_target = key_target(&path);
            if self.key_target.is_none() {
                let line = format!("{}: --perturb key found no keyed command to change", self.o.source);
                self.print(line);
                return 1;
            }
        }
        let mut step = Step::default();
        let mut bytes = 0;
        while reader.next(&mut step, &mut bytes) {
            self.index += 1;
            if let Some(code) = self.play(&mut step, bytes) {
                return code;
            }
        }
        if !reader.ended() {
            let why = format!("the recording is refused: {}", reader.error());
            return self.refuse(why);
        }
        if self.o.perturb != Perturb::None {
            let line = format!("{}: --perturb {} found no step to change", self.o.source, self.o.perturb.name());
            self.print(line);
            return 1;
        }
        if let Some((&first, _)) = self.live.iter().next() {
            let line = format!("{}: {} sessions are still alive at the end, the first {first}", self.o.source, self.live.len());
            self.print(line);
            return 1;
        }
        let line = format!(
            "{}: compared {}; at most {} sessions alive at once; message cap {} bytes",
            self.o.source,
            self.total.text(),
            self.most_alive,
            self.caps.message
        );
        self.print(line);
        let labelled: Vec<String> = self
            .labelled
            .iter()
            .map(|(id, (label, counts))| format!("{} session {id} ({}): {}", self.o.source, lossy(label), counts.text()))
            .collect();
        for line in labelled {
            self.print(line);
        }
        let mut met = true;
        if let Some(sessions) = self.o.sessions
            && self.total.sessions != sessions
        {
            let line = format!("{}: {} sessions where {sessions} are due", self.o.source, self.total.sessions);
            self.print(line);
            met = false;
        }
        for name in self.o.needs.clone() {
            if self.total.need(&name).unwrap_or(0) > 0 {
                continue;
            }
            let line = format!("{}: nothing compared of {name}", self.o.source);
            self.print(line);
            met = false;
        }
        if !met {
            return 1;
        }
        let line = format!("{}: every step of every session matches", self.o.source);
        self.print(line);
        if self.o.benchmark {
            let sessions: Vec<_> = self.timings.iter().map(|(&id, timing)| timing.json(id)).collect();
            let mut report = Json::object();
            report.set("schema", 1).set("timing", "renderer_loopback_ns").set("sessions", sessions);
            self.print(report.dump_lossy());
        }
        0
    }
}

// ---- a tree of recordings -----------------------------------------------------------------------------------------

/// Every recording under a directory replayed, each in a replay of its own as the C++ replays each in a process of its
/// own: the counts by the first part of the recordings' paths, and the recordings that failed with their first lines.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub recordings: usize,
    pub folders: BTreeMap<String, (usize, Counts)>,
    pub failed: Vec<(String, i32, Vec<String>)>,
}

impl Tree {
    pub fn total(&self) -> Counts {
        let mut all = Counts::default();
        for (_, counts) in self.folders.values() {
            all.absorb(counts);
        }
        all
    }
}

fn recordings_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            recordings_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "twrec") {
            out.push(path);
        }
    }
}

/// The message cap `twidget_parity_core` gives the replay of `core.twrec` (`-DMESSAGE_BYTES`).
pub const CORE_MESSAGE_BYTES: u64 = 268435456;

/// Replays every `.twrec` under `dir`, its source named by its path below `dir`; `each` sees every finished replay.
pub fn replay_tree(dir: &Path, mut each: impl FnMut(&str, i32, &Replay)) -> Tree {
    let mut paths = Vec::new();
    recordings_under(dir, &mut paths);
    let mut tree = Tree::default();
    for path in paths {
        let source = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        // core_tests' canvas-limit batches pass remote's 32 MiB message cap, which twidget_parity_core raises as here.
        let message_bytes = path.file_name().is_some_and(|name| name == "core.twrec").then_some(CORE_MESSAGE_BYTES);
        let mut replay = Replay::new(Options { recording: path.clone(), source: source.clone(), message_bytes, ..Options::default() });
        let code = replay.run();
        each(&source, code, &replay);
        tree.recordings += 1;
        let folder = source.split('/').next().unwrap_or_default().to_string();
        let entry = tree.folders.entry(folder).or_default();
        entry.0 += 1;
        if code == 0 {
            entry.1.absorb(replay.counts());
        } else {
            let tail: Vec<String> = replay.lines.iter().skip(1).take(8).cloned().collect();
            tree.failed.push((source, code, tail));
        }
    }
    tree
}
