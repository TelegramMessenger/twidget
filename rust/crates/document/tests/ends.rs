//! tests/ends_tests.cpp's tests of a layout's words in batches, which need the renderer's real container: the
//! renderer on `twidget_document`'s engine, called directly against the same renderer behind the two ends over an
//! in-memory channel (twidget_ends' test harness), every step compared at once. A build's unknown words go in one
//! widths request after its fonts, the units a spaced run is painted in and the hidden texts' own widths with them, the
//! breaks its layout takes in one more, a patched text's words alone before any layout, a face's cache bounding the
//! batch; under caps the requests stay within them, and a request that fails stops the end. bounded_batches and
//! batch_failures, which twidget_ends runs on the scripted renderer, run here on the real one too.

#[path = "../../ends/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use common::*;
use twidget_document::{DocumentEngine, Process};
use twidget_ends::{Backend, Output, PlatformEnd, Proxy, RendererEnd, TextInFont};
use twidget_json::Json;
use twidget_remote::*;
use twidget_wire::{DisplayList, FontMetrics, Rect};

// ---- the renderer the ends drive -------------------------------------------------------------------------------

/// A time only the test moves, which the renderer's clock reads from its own handle (a renderer's clock is `Send`).
#[derive(Debug, Default)]
struct Time(std::sync::atomic::AtomicU64);

impl Time {
    fn new(ms: f64) -> Time {
        Time(std::sync::atomic::AtomicU64::new(ms.to_bits()))
    }
    fn get(&self) -> f64 {
        f64::from_bits(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

/// ends_tests.cpp's `VirtualClock`: a time only the test moves, from 1000 ms.
struct VirtualClock(Arc<Time>);

impl twidget_renderer::Clock for VirtualClock {
    fn now_ms(&mut self) -> f64 {
        self.0.get()
    }
}

/// renderer.h's `Renderer`: the renderer on a `twidget_document` engine, both on one Backend, as the C++'s container
/// holds the renderer's.
struct Real<B: Backend + Clone>(twidget_renderer::Renderer<DocumentEngine<B>, B>);

impl<B: Backend + Clone> Real<B> {
    fn new(width: f64, dpr: f64, dark: bool, backend: B, process: &Arc<Process>, clock: &Arc<Time>) -> Real<B> {
        let engine = DocumentEngine::new(Arc::clone(process), backend.clone());
        Real(twidget_renderer::Renderer::new(width, dpr, dark, engine, backend, Some(Box::new(VirtualClock(Arc::clone(clock))))))
    }
}

impl<B: Backend + Clone> twidget_ends::Renderer for Real<B> {
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

/// The renderer calls a helper makes, on either arm's renderer.
trait HelperCalls {
    fn trim(&mut self);
}

impl<B: Backend + Clone> HelperCalls for Real<B> {
    fn trim(&mut self) {
        self.0.trim()
    }
}

type Looped = Real<Proxy<Wire>>;

/// A loop whose end runs the real renderer at 300 px, the given scale, light, on `process`.
fn looped(caps: Caps, dpr: f64, process: &Arc<Process>, clock: &Arc<Time>) -> Loopback<Looped> {
    let mut lp = Loopback::<Looped>::made(caps);
    lp.open_with(300.0, dpr, false, |w, d, k, proxy| Real::new(w, d, k, proxy, process, clock));
    lp
}

fn state_of<R: twidget_ends::Renderer>(r: &mut R, frame_asked: bool) -> State {
    let width = r.width();
    let height = r.viewport_height();
    let dirty = r.dirty();
    let wants_tick = r.wants_tick();
    let next_tick_in = r.next_tick_in();
    State { width, height, dirty, wants_tick, next_tick_in, frame_asked }
}

// ---- a renderer called directly against the two ends ----------------------------------------------------------------

/// A check of a session step: the condition is evaluated before the check borrows the session.
macro_rules! ok {
    ($a:expr, $cond:expr, $what:expr $(,)?) => {{
        let cond = $cond;
        let what = $what;
        $a.ok(cond, &what)
    }};
}

type Step = Expected;

/// One focused sequence made of calls, each on a renderer called directly, which gives what is expected, and then
/// through the two ends; every step compares the two at once: the reports and the Backend calls (ends_tests.cpp's
/// `Arms`). Both renderers share one process's litehtml, as the C++'s do.
struct Arms {
    name: String,
    label: String,
    steps: usize,
    direct_backend: LogHandle,
    direct: Option<Real<LogHandle>>,
    lp: Loopback<Looped>,
    asked: bool,
    selection: Vec<u8>,
    sent: State,
    requested: usize,
}

impl Arms {
    fn new(name: &str) -> Arms {
        let process = Arc::new(Process::default());
        let (direct_clock, clock) = (Arc::new(Time::new(1000.0)), Arc::new(Time::new(1000.0)));
        let direct_backend = LogHandle::new();
        let direct = Real::new(300.0, 2.0, false, direct_backend.clone(), &process, &direct_clock);
        let lp = looped(Caps::default(), 2.0, &process, &clock);
        let mut a = Arms {
            name: name.to_string(),
            label: "construction".to_string(),
            steps: 0,
            direct_backend,
            direct: Some(direct),
            lp,
            asked: false,
            selection: Vec::new(),
            sent: State::default(),
            requested: 0,
        };
        let calls = same_calls(&a.direct_backend.calls(), &a.lp.backend.calls());
        ok!(a, a.lp.backend.len() > 0 && calls.is_ok(), format!("construction makes the direct renderer's Backend calls {calls:?}"));
        a.sent = state_of(a.direct(), false);
        let sent = a.sent;
        ok!(
            a,
            a.lp.events() == vec![Kind::State]
                && same_state(a.lp.host.borrow().states.last().expect("a state"), &sent)
                && same_state(&a.lp.platform_state(), &sent),
            "the constructor's one report is the direct renderer's State",
        );
        a.healthy();
        a
    }

    fn ok(&self, cond: bool, what: &str) {
        assert!(cond, "{} step {} ({}): {what}", self.name, self.steps, self.label);
    }

    fn direct(&mut self) -> &mut Real<LogHandle> {
        self.direct.as_mut().expect("a renderer")
    }

    fn healthy(&mut self) {
        let stopped = self.lp.end().stopped();
        let reason = self.lp.end().stop_reason();
        let refusals = self.lp.host.borrow().refusals.clone();
        let quiet = self.lp.wire.borrow().refused == 0 && !self.lp.refusing() && self.lp.queue.0.borrow().is_empty();
        ok!(self, !stopped && refusals.is_empty() && quiet, format!("both ends stay healthy: {reason} {refusals:?}"));
    }

    /// The reads finish() makes after the call, in its order, then the frame's marker.
    fn expect(&mut self, s: &mut Step) {
        let asked = self.asked;
        let d = self.direct();
        s.selection = if twidget_ends::Renderer::may_have_selection(d) { twidget_ends::Renderer::selected_text(d) } else { Vec::new() };
        s.state = state_of(d, asked);
        predict(s, &self.selection, &self.sent);
        if let Some(frame) = &s.frame {
            self.direct_backend.0.borrow_mut().frame(frame.painted);
        }
    }

    fn compare(&mut self, s: &Step, heard_before: usize, returned: &[Json], out: Option<&Output>) {
        self.healthy();
        ok!(self, dumps(returned) == dumps(&s.messages), "the helper gets every message the direct renderer gave");
        if let Some(out) = out {
            ok!(self, out.frame == s.engine, format!("the engine's frame is {}", if s.engine { "due" } else { "not due" }));
        }
        let heard: Vec<Kind> = self.lp.events()[heard_before..].to_vec();
        ok!(self, heard == s.events, format!("the host hears {:?}, not {heard:?}", s.events));
        {
            let host = self.lp.host.borrow();
            for kind in &heard {
                match kind {
                    Kind::Messages => ok!(
                        self,
                        dumps(host.batches.last().expect("a batch")) == dumps(&platform_only(&s.messages)),
                        "the Messages report holds the call's platform messages alone"
                    ),
                    Kind::Pointer => {
                        let p = host.pointers.last().expect("a pointer");
                        let e = s.pointer.as_ref().expect("a pointer");
                        ok!(self, p.cursor == e.cursor && p.claim == e.claim, format!("the Pointer report is {e:?}, not {p:?}"));
                    }
                    Kind::Selection => ok!(
                        self,
                        host.selections.last().expect("a selection").text == s.selection,
                        "the Selection report is the direct selected text"
                    ),
                    Kind::Frame => {
                        let f = host.frames.last().expect("a frame");
                        let e = s.frame.as_ref().expect("a frame");
                        ok!(self, f.seq == s.seq && f.painted == e.painted, format!("the Frame answers request {}", s.seq));
                        ok!(self, list_bytes(&f.list) == list_bytes(&e.list), "the frame's list is the direct paint");
                        ok!(self, sharing(&f.list) == sharing(&e.list), "the frame's list shares its payloads as the direct paint does");
                        ok!(self, damage_bytes(&f.damage) == damage_bytes(&e.damage), "the frame's damage is the direct damage");
                        ok!(self, same_state(&f.state, &s.state), "the Frame carries the direct state");
                    }
                    Kind::ClaimAnswer => {
                        let c = host.claims.last().expect("a claim");
                        ok!(
                            self,
                            c.seq == s.seq && Some(c.claim) == s.claim,
                            format!("the ClaimAnswer is {:?}, not {:?}", s.claim, c.claim)
                        );
                    }
                    Kind::State => {
                        ok!(self, same_state(host.states.last().expect("a state"), &s.state), "the State report is the direct state")
                    }
                    _ => {}
                }
            }
        }
        ok!(self, same_state(&self.lp.platform_state(), &s.state), "the platform end's state is the direct renderer's");
        let calls = same_calls(&self.direct_backend.calls(), &self.lp.backend.calls());
        ok!(self, calls.is_ok(), format!("the Backend calls are the direct renderer's, in order: {calls:?}"));
        self.selection = s.selection.clone();
        self.sent = s.state;
    }

    fn begin(&mut self, what: &str) {
        self.steps += 1;
        self.label = what.to_string();
    }

    /// Platform input: `call` makes the direct call and its reads into the step, `send` hands the same input to the
    /// platform end and returns its seq.
    fn input(&mut self, what: &str, call: impl FnOnce(&mut Arms, &mut Step), send: impl FnOnce(&mut Platform) -> u64) -> Step {
        self.begin(what);
        let mut s = Step::default();
        call(self, &mut s);
        self.expect(&mut s);
        let heard = self.lp.events().len();
        let delivered = self.lp.delivered.len();
        s.seq = send(&mut self.lp.platform.borrow_mut());
        let pumped = self.lp.pump();
        ok!(
            self,
            pumped && self.lp.delivered.len() == delivered + 1 && *self.lp.delivered.last().expect("a delivery"),
            "the platform input is delivered and taken"
        );
        let out = self.lp.outputs.last().cloned().unwrap_or_default();
        self.compare(&s, heard, &out.messages, Some(&out));
        s
    }

    fn receive(&mut self, json: &str, what: &str) -> Step {
        self.begin(what);
        let message = parse(json);
        let mut s = Step { messages: twidget_ends::Renderer::receive(self.direct(), &message), ..Step::default() };
        self.expect(&mut s);
        let heard = self.lp.events().len();
        let got = self.lp.receive(&message);
        self.compare(&s, heard, &got, None);
        s
    }

    fn frame(&mut self, what: &str, tick: bool, paint: Paint) -> Step {
        self.requested += 1;
        self.input(
            what,
            |a, s| {
                let d = a.direct();
                if tick {
                    s.messages = twidget_ends::Renderer::tick(d);
                    s.engine = std::mem::replace(&mut a.asked, false);
                }
                let d = a.direct();
                let mut f = Frame::default();
                if paint == Paint::Force || (paint == Paint::IfDirty && twidget_ends::Renderer::dirty(d)) {
                    f.list = twidget_ends::Renderer::paint(d);
                    let mut rects = Vec::new();
                    if twidget_ends::Renderer::damage(d, &mut rects) {
                        f.damage = Some(rects);
                    }
                    f.painted = true;
                }
                s.frame = Some(f);
            },
            |p| p.request_frame(tick, paint),
        )
    }

    fn set_width(&mut self, what: &str, width: f64) -> Step {
        self.input(
            what,
            |a, s| twidget_ends::Renderer::set_width(a.direct(), width, &mut s.messages),
            |p| {
                p.set_width(width);
                0
            },
        )
    }

    /// A call the channel does not carry, made on each renderer by the helper, which then hands the end's renderer
    /// messages to report().
    fn helper(&mut self, what: &str, f: impl Fn(&mut dyn HelperCalls) -> Vec<Json>) -> Step {
        self.begin(what);
        let mut s = Step { messages: f(self.direct()), ..Step::default() };
        self.expect(&mut s);
        let heard = self.lp.events().len();
        ok!(self, !self.lp.end().stopped(), "the helper calls the renderer of a healthy end");
        let got = f(self.lp.end().renderer());
        self.lp.report(&got);
        self.compare(&s, heard, &got, None);
        s
    }

    /// Drops both renderers while their Backends live, compares what they released, and that every frame asked for
    /// came.
    fn close(&mut self) {
        self.begin("destruction");
        let heard = self.lp.events().len();
        self.direct = None;
        self.lp.close();
        let calls = same_calls(&self.direct_backend.calls(), &self.lp.backend.calls());
        ok!(self, calls.is_ok(), format!("destruction releases what the direct renderer releases, in its order: {calls:?}"));
        let quiet = self.lp.events().len() == heard
            && self.lp.host.borrow().refusals.is_empty()
            && self.lp.wire.borrow().refused == 0
            && !self.lp.refusing();
        ok!(self, quiet, "destruction reports nothing and the platform end takes all of it");
        ok!(
            self,
            self.lp.backend.0.borrow().live_fonts() == 0 && self.direct_backend.0.borrow().live_fonts() == 0,
            "no font outlives its renderer"
        );
        let frames = self.lp.host.borrow().frames.len();
        ok!(
            self,
            frames == self.requested,
            format!("the host got a frame for each request and no other ({frames} for {})", self.requested)
        );
        let widths = self.lp.backend.count(Op::TextWidth);
        let requests = self.lp.count_kind(Kind::WidthsRequest) + self.lp.count_kind(Kind::WidthRequest);
        eprintln!("{}: {} steps, {widths} widths in {requests} requests, {frames} frames", self.name, self.steps);
    }

    fn kinds_since(&self, from: usize) -> Vec<u8> {
        self.lp.kinds()[from..].to_vec()
    }
}

// ---- what the checks read ---------------------------------------------------------------------------------------

fn count(kinds: &[u8], kind: Kind) -> usize {
    kinds.iter().filter(|k| **k == kind as u8).count()
}

/// The kinds by name, for the checks' messages (reports.h's `reports_name`).
fn reports_name(kinds: &[u8]) -> String {
    let names: Vec<String> = kinds.iter().map(|k| Kind::from_u8(*k).map_or(format!("{k}"), |k| format!("{k:?}"))).collect();
    names.join(" ")
}

fn widths_since(backend: &LogHandle, from: usize) -> Vec<Call> {
    backend.0.borrow().calls[from..].iter().filter(|c| c.op == Op::TextWidth).cloned().collect()
}

/// No font and text measured twice.
fn measured_once(widths: &[Call]) -> bool {
    let mut seen = BTreeSet::new();
    widths.iter().all(|c| seen.insert((c.font, c.text.clone())))
}

/// Exactly `n` widths requests, the first after every font request, the last before every single width request.
fn batches_of(kinds: &[u8], n: usize) -> bool {
    let is = |kind: Kind| move |k: &u8| *k == kind as u8;
    let Some(first) = kinds.iter().position(is(Kind::WidthsRequest)) else { return false };
    let last = kinds.iter().rposition(is(Kind::WidthsRequest)).map_or(0, |l| l + 1);
    count(kinds, Kind::WidthsRequest) == n
        && !kinds[first..].iter().any(is(Kind::FontRequest))
        && !kinds[..last].iter().any(is(Kind::WidthRequest))
}

fn one_batch(kinds: &[u8]) -> bool {
    batches_of(kinds, 1)
}

fn texts_of(widths: &[Call]) -> BTreeSet<Vec<u8>> {
    widths.iter().map(|c| c.text.clone()).collect()
}

fn texts(words: &[&str]) -> BTreeSet<Vec<u8>> {
    words.iter().map(|w| w.as_bytes().to_vec()).collect()
}

fn none_measured(widths: &[Call], texts: &[&str]) -> bool {
    !widths.iter().any(|c| texts.iter().any(|t| c.text == t.as_bytes()))
}

fn has(asked: &BTreeSet<Vec<u8>>, text: &str) -> bool {
    asked.contains(text.as_bytes())
}

// ---- a layout's unknown words in batches ---------------------------------------------------------------------------

/// Some fifty words in five faces: repeated words, a bold copy of five, an uppercase line, a pre block with a tab,
/// spaced words, and a word broken anywhere in a narrow box.
const WORDS: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen alpha beta gamma delta pi rho tau mu"},
 {"op":"el","id":12,"tag":"p","attrs":[["style","font-weight:bold"]]},
 {"op":"text","id":13,"data":"alpha beta gamma delta epsilon"},
 {"op":"el","id":14,"tag":"p","attrs":[["style","text-transform:uppercase"]]},
 {"op":"text","id":15,"data":"Mixed case words"},
 {"op":"el","id":16,"tag":"pre","attrs":[["style","font:13px monospace"]]},
 {"op":"text","id":17,"data":"a\tb  c\nd"},
 {"op":"el","id":18,"tag":"p","attrs":[["style","letter-spacing:2px;word-spacing:3px"]]},
 {"op":"text","id":19,"data":"spaced words here"},
 {"op":"el","id":20,"tag":"div","attrs":[["style","width:40px;overflow-wrap:anywhere"]]},
 {"op":"text","id":21,"data":"supercalifragilisticexpialidocious"},
 {"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},{"op":"ins","parent":12,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},{"op":"ins","parent":14,"id":15,"before":null},
 {"op":"ins","parent":4,"id":16,"before":null},{"op":"ins","parent":16,"id":17,"before":null},
 {"op":"ins","parent":4,"id":18,"before":null},{"op":"ins","parent":18,"id":19,"before":null},
 {"op":"ins","parent":4,"id":20,"before":null},{"op":"ins","parent":20,"id":21,"before":null}
]}"#;

/// A build's unknown words go in one widths request after its fonts and the breaks its layout takes in one more, a
/// relayout finds them cached, later builds ask for their new words alone, transforms and spacing measure what they
/// show, and after trim the words are asked for again; each call as the direct renderer makes it.
#[test]
fn batches() {
    let mut a = Arms::new("batches");
    let (mut sent, mut calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(WORDS, "a page of some fifty words in five faces");
    let mut kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    let singles = count(&kinds, Kind::WidthRequest);
    let batched = widths.len() - singles;
    ok!(
        a,
        batches_of(&kinds, 2),
        "the build asks for its words in one widths request after its fonts, and its layout for the breaks in one more, before any single width"
    );
    ok!(a, batched > 50, format!("the requests hold the page's distinct words and spaces, and their letters ({batched})"));
    let letters_alone = widths[widths.len() - singles..].iter().any(|c| c.text.len() < 2);
    ok!(
        a,
        singles > 0 && !letters_alone,
        format!("the word broken anywhere is measured in longer pieces in the layout, after the batches, never a letter alone ({singles})")
    );
    ok!(a, measured_once(&widths), "no font and text is measured twice");
    let upper = widths.iter().any(|c| c.text == b"MIXED");
    let tab = widths.iter().any(|c| c.text == b"    ");
    ok!(a, upper && tab, "the batch measures what the texts show: the uppercase words and a tab's spaces");
    a.frame("the first frame", true, Paint::IfDirty);

    sent = a.lp.kinds().len();
    a.set_width("a narrower view", 200.0);
    ok!(a, count(&a.kinds_since(sent), Kind::WidthsRequest) == 0, "a relayout at another width finds every word cached");

    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(
        r#"{"type":"ops","ops":[{"op":"el","id":30,"tag":"p","attrs":[]},{"op":"text","id":31,"data":"alpha omega seventeen eighteen alpha nineteen"},{"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":30,"id":31,"before":null}]}"#,
        "a paragraph of known and new words",
    );
    kinds = a.kinds_since(sent);
    let asked = texts_of(&widths_since(&a.lp.backend, calls));
    ok!(
        a,
        one_batch(&kinds) && asked == texts(&["seventeen", "eighteen", "nineteen"]),
        "only the new words are asked for, in one widths request"
    );

    calls = a.lp.backend.len();
    a.receive(r#"{"type":"ops","ops":[{"op":"style","id":18,"name":"letter-spacing","value":"5px"}]}"#, "a new letter spacing");
    let spaced = widths_since(&a.lp.backend, calls).iter().any(|c| c.text == b"spaced" || c.text == b"here");
    ok!(a, !spaced, "the spaced words keep their face's widths, the spacing added where they are used");
    a.receive(
        r#"{"type":"ops","ops":[{"op":"style","id":14,"name":"text-transform","value":"lowercase"}]}"#,
        "the uppercase line turned lowercase",
    );
    a.frame("the frame after the restyles", true, Paint::IfDirty);

    a.helper("the helper trims", |r| {
        r.trim();
        Vec::new()
    });
    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(
        r#"{"type":"ops","ops":[{"op":"el","id":32,"tag":"p","attrs":[]},{"op":"text","id":33,"data":"alpha beta gamma"},{"op":"ins","parent":4,"id":32,"before":null},{"op":"ins","parent":32,"id":33,"before":null}]}"#,
        "words measured before the trim",
    );
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthsRequest) == 2 && !widths_since(&a.lp.backend, calls).is_empty(),
        "the words the trim forgot are asked for again in one request, and the breaks of the word broken anywhere in one more"
    );
    a.frame("the last frame", true, Paint::IfDirty);
    a.close();
}

/// A letter-spaced line (11), a word-spaced number with a no-break space (13), and a letter-spaced word (16) under a
/// display:none box (14).
fn spaced() -> String {
    r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[["style","letter-spacing:2px"]]},
 {"op":"text","id":11,"data":"Spaced CAPS"},
 {"op":"el","id":12,"tag":"p","attrs":[["style","word-spacing:4px"]]},
 {"op":"text","id":13,"data":"10{nbsp}000 apart"},
 {"op":"el","id":14,"tag":"div","attrs":[["style","display:none"]]},
 {"op":"el","id":15,"tag":"p","attrs":[["style","letter-spacing:3px"]]},
 {"op":"text","id":16,"data":"QXZ"},
 {"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},{"op":"ins","parent":12,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},{"op":"ins","parent":14,"id":15,"before":null},{"op":"ins","parent":15,"id":16,"before":null}
]}"#
    .replace("{nbsp}", "\u{a0}")
}

/// A run painted with letter or word spacing is drawn unit by unit: the build's one request holds the units of every
/// run it can see, and the paint measures nothing.
#[test]
fn spaced_units() {
    let mut a = Arms::new("spaced units");
    let (mut sent, mut calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(&spaced(), "spaced runs, one under a display:none box");
    let mut kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    let mut asked = texts_of(&widths);
    ok!(
        a,
        one_batch(&kinds) && count(&kinds, Kind::WidthRequest) == 0,
        format!("the build makes one widths request and no single one ({})", reports_name(&kinds))
    );
    let letters = ["S", "p", "a", "c", "e", "d", "C", "A", "P"].iter().all(|u| has(&asked, u));
    ok!(a, letters, "it holds each letter of the letter-spaced line");
    ok!(
        a,
        has(&asked, "10") && has(&asked, "\u{a0}") && has(&asked, "000"),
        "and the words and the space the word-spaced number is drawn in"
    );
    ok!(a, has(&asked, "QXZ") && none_measured(&widths, &["Q", "X", "Z"]), "the hidden word's own width, but not its letters");
    ok!(a, measured_once(&widths), "no font and text is measured twice");
    sent = a.lp.kinds().len();
    a.frame("the first frame", true, Paint::IfDirty);
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 0 && count(&kinds, Kind::WidthsRequest) == 0,
        format!("the paint measures nothing ({})", reports_name(&kinds))
    );

    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(r#"{"type":"ops","ops":[{"op":"style","id":14,"name":"display","value":"block"}]}"#, "the hidden box shown");
    kinds = a.kinds_since(sent);
    asked = texts_of(&widths_since(&a.lp.backend, calls));
    ok!(
        a,
        one_batch(&kinds) && count(&kinds, Kind::WidthRequest) == 0 && asked == texts(&["Q", "X", "Z"]),
        "its letters come in the layout's one request, and nothing else"
    );
    sent = a.lp.kinds().len();
    a.frame("the frame that shows it", true, Paint::IfDirty);
    kinds = a.kinds_since(sent);
    ok!(a, count(&kinds, Kind::WidthRequest) == 0 && count(&kinds, Kind::WidthsRequest) == 0, "and its paint measures nothing");

    sent = a.lp.kinds().len();
    a.receive(r#"{"type":"ops","ops":[{"op":"style","id":10,"name":"letter-spacing","value":"5px"}]}"#, "a wider letter spacing");
    a.frame("the frame after it", true, Paint::IfDirty);
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 0 && count(&kinds, Kind::WidthsRequest) == 0,
        "the letters keep their face's widths, the spacing added where they are drawn"
    );
    a.close();
}

/// Two words in a flex row (12, 14), whose items' smallest widths break them, a paragraph that fits (16), and a word
/// in a flex row under a display:none box (20).
const BREAKING: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":11,"tag":"div","attrs":[]},
 {"op":"text","id":12,"data":"breakable"},
 {"op":"el","id":13,"tag":"div","attrs":[]},
 {"op":"text","id":14,"data":"pieces"},
 {"op":"el","id":15,"tag":"p","attrs":[]},
 {"op":"text","id":16,"data":"plain fits"},
 {"op":"el","id":17,"tag":"div","attrs":[["style","display:none"]]},
 {"op":"el","id":18,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":19,"tag":"div","attrs":[]},
 {"op":"text","id":20,"data":"VWK"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},{"op":"ins","parent":11,"id":12,"before":null},
 {"op":"ins","parent":10,"id":13,"before":null},{"op":"ins","parent":13,"id":14,"before":null},
 {"op":"ins","parent":4,"id":15,"before":null},{"op":"ins","parent":15,"id":16,"before":null},
 {"op":"ins","parent":4,"id":17,"before":null},{"op":"ins","parent":17,"id":18,"before":null},
 {"op":"ins","parent":18,"id":19,"before":null},{"op":"ins","parent":19,"id":20,"before":null}
]}"#;

fn flex_item(row: i32, id: i32, text: &str) -> String {
    format!(
        r#"{{"op":"el","id":{id},"tag":"div","attrs":[]}},{{"op":"text","id":{},"data":"{text}"}},{{"op":"ins","parent":{id},"id":{},"before":null}},{{"op":"ins","parent":{row},"id":{id},"before":null}}"#,
        id + 1,
        id + 1
    )
}

/// The breaks a layout measures: its first unmeasured one asks, in one request, for the break points of every text it
/// may wrap that it can see; each is measured once and none alone, a relayout asks for nothing, and a later layout for
/// its new letters alone.
#[test]
fn breaks() {
    let mut a = Arms::new("breaks");
    let (mut sent, mut calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(BREAKING, "words in a flex row, a paragraph and a hidden row");
    let mut kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    let mut asked = texts_of(&widths);
    ok!(
        a,
        batches_of(&kinds, 2) && count(&kinds, Kind::WidthRequest) == 0,
        format!(
            "the build asks for its words in one widths request and its layout for the breaks in one more, with no single width ({})",
            reports_name(&kinds)
        )
    );
    let letters = ["b", "r", "e", "a", "k", "l", "p", "i", "c", "s"].iter().all(|l| has(&asked, l));
    ok!(a, letters, "the breaks hold every letter of the row's words");
    ok!(a, has(&asked, "f") && has(&asked, "t"), "and of the paragraph, which the layout may wrap too");
    ok!(a, has(&asked, "VWK") && none_measured(&widths, &["V", "W", "K"]), "the hidden word's own width, but not its letters");
    ok!(a, measured_once(&widths), "no font and text is measured twice");
    sent = a.lp.kinds().len();
    a.frame("the first frame", true, Paint::IfDirty);
    a.set_width("a narrower view", 200.0);
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 0 && count(&kinds, Kind::WidthsRequest) == 0,
        "the paint and a relayout at another width measure nothing"
    );

    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(&format!(r#"{{"type":"ops","ops":[{}]}}"#, flex_item(10, 30, "QXZ")), "a new word in the row");
    kinds = a.kinds_since(sent);
    asked = texts_of(&widths_since(&a.lp.backend, calls));
    ok!(
        a,
        batches_of(&kinds, 2) && count(&kinds, Kind::WidthRequest) == 0 && asked == texts(&["QXZ", "Q", "X", "Z"]),
        "the word in one request, and its new letters alone in the layout's one more"
    );

    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(
        &format!(
            r#"{{"type":"ops","ops":[{{"op":"el","id":40,"tag":"div","attrs":[["style","display:flex"]]}},{{"op":"ins","parent":4,"id":40,"before":null}},{},{}]}}"#,
            flex_item(40, 41, "JJ"),
            flex_item(10, 43, "YY")
        ),
        "new words in two rows",
    );
    kinds = a.kinds_since(sent);
    asked = texts_of(&widths_since(&a.lp.backend, calls));
    ok!(
        a,
        batches_of(&kinds, 2) && count(&kinds, Kind::WidthRequest) == 0 && asked == texts(&["JJ", "YY", "J", "Y"]),
        "two boxes that break in one layout share its one request for breaks"
    );
    a.frame("the last frame", true, Paint::IfDirty);
    a.close();
}

/// html(2) head(3) body(4), a paragraph (10) whose text (11) is all it holds.
const SOLE_TEXT: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"alpha beta"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]}"#;

fn data_op(text: &str) -> String {
    format!(r#"{{"type":"ops","ops":[{{"op":"data","id":11,"data":"{text}"}}]}}"#)
}

/// A sole text patched in place: the document reads the new words' sizes before any layout, one request each, until a
/// size differs; the rest wait for the layout's batch.
#[test]
fn patched_words() {
    let mut a = Arms::new("patches");
    a.receive(SOLE_TEXT, "a paragraph whose text is all it holds");
    a.frame("the first frame", true, Paint::IfDirty);
    let mut sent = a.lp.kinds().len();
    a.receive(&data_op("gamma zeta"), "new words of the same widths");
    let mut kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 2 && count(&kinds, Kind::WidthsRequest) == 0,
        format!("each new word is read before any layout, one width request each ({})", reports_name(&kinds))
    );
    a.frame("the frame after it", true, Paint::IfDirty);
    sent = a.lp.kinds().len();
    a.receive(&data_op("epsilons iota"), "a longer first word");
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 1 && count(&kinds, Kind::WidthsRequest) == 1,
        "the first word is read, and the layout its new width needs asks for the rest in one request"
    );
    a.frame("the frame after the longer word", true, Paint::IfDirty);
    sent = a.lp.kinds().len();
    a.receive(&data_op("one two three"), "a text of more words");
    kinds = a.kinds_since(sent);
    ok!(
        a,
        count(&kinds, Kind::WidthRequest) == 0 && count(&kinds, Kind::WidthsRequest) == 1,
        "new text elements are measured in the layout's one request"
    );
    a.frame("the last frame", true, Paint::IfDirty);
    a.close();
}

/// More different words in one face than its cache keeps: the batch takes what the cache keeps, the rest are measured
/// one by one, none twice.
#[test]
fn cache_pressure() {
    let mut a = Arms::new("cache pressure");
    let words: String = (0..4200).map(|i| format!("w{i} ")).collect();
    let (sent, calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(
        &format!(
            r#"{{"type":"ops","ops":[{{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]}},{{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]}},{{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]}},{{"op":"el","id":10,"tag":"p","attrs":[]}},{{"op":"text","id":11,"data":"{words}"}},{{"op":"ins","parent":4,"id":10,"before":null}},{{"op":"ins","parent":10,"id":11,"before":null}}]}}"#
        ),
        "a paragraph of 4,200 different words in one face",
    );
    let kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    let singles = count(&kinds, Kind::WidthRequest);
    let batches = count(&kinds, Kind::WidthsRequest);
    let batched = widths.len() - singles;
    let per = Caps::default().widths;
    ok!(a, measured_once(&widths) && widths.len() == 4201, format!("each word and the space measured once ({} measured)", widths.len()));
    ok!(
        a,
        batched == 4096 && batches == batched.div_ceil(per) && singles == 4201 - 4096,
        format!(
            "the batches take what the face keeps, in requests of at most {per}, the rest one by one ({batched} in {batches} requests, {singles} alone)"
        )
    );
    a.frame("the frame", true, Paint::IfDirty);
    a.close();
}

// ---- under caps, and requests that fail ------------------------------------------------------------------------------

/// Twenty words of many lengths, one of 30 bytes, and the space between them, in the body's one face, none wider than
/// the line.
const MANY_WORDS: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"a bb ccc dddd eeeee ffffff ggggggg hhhhhhhh iiiiiiiii jjjjjjjjjj kk lll mmmm nnnnn oooooo ppppppp qqqqqqqqqqqqqqqqqqqqqqqqqqqqqq rr sss tttt"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]}"#;

/// A Backend that notes each widths batch it is handed.
#[derive(Clone, Default)]
struct BatchLog {
    log: LogHandle,
    batches: Rc<RefCell<Vec<usize>>>,
}

impl Backend for BatchLog {
    fn create_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        self.log.create_font(families, size, weight, italic, features, out)
    }
    fn create_canvas_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.log.create_canvas_font(families, size, weight, italic, out)
    }
    fn delete_font(&mut self, font: i32) {
        self.log.delete_font(font)
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.log.text_width(font, text)
    }
    fn text_widths(&mut self, texts: &[TextInFont]) -> Vec<f32> {
        self.batches.borrow_mut().push(texts.len());
        self.log.text_widths(texts)
    }
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        self.log.load_image(id, url, w, h)
    }
}

/// Each request and its answer.
type Exchanges = Rc<RefCell<Vec<(Vec<u8>, Vec<u8>)>>>;

/// A renderer end's channel straight into a platform end on a Backend that notes each batch, keeping every request and
/// answer (ends_tests.cpp's `Straight`).
struct Straight {
    platform: Rc<RefCell<PlatformEnd<BatchLog, HeardHandle, Queue>>>,
    requests: Exchanges,
    refused: Rc<Cell<usize>>,
}

impl twidget_ends::Channel for Straight {
    fn send(&mut self, message: Vec<u8>) {
        if !self.platform.borrow_mut().receive(&message) {
            self.refused.set(self.refused.get() + 1);
        }
    }
}

impl twidget_ends::RendererChannel for Straight {
    fn request(&mut self, message: Vec<u8>, answer: &mut Vec<u8>) -> bool {
        let mut reply = Vec::new();
        if !self.platform.borrow_mut().answer(&message, &mut reply) {
            self.refused.set(self.refused.get() + 1);
            return false;
        }
        self.requests.borrow_mut().push((message, reply.clone()));
        *answer = reply;
        true
    }
}

/// What a page did given to a direct renderer and to a renderer end on `Straight`: whether both stayed healthy with the
/// same messages, the Backend calls compared, the platform end's Backend and every exchange.
struct Straightened {
    healthy: bool,
    calls: Result<(), String>,
    backend: BatchLog,
    exchanges: Vec<(Vec<u8>, Vec<u8>)>,
}

/// A direct renderer at scale 1 and a renderer end on `Straight` under `caps`, both given `page`.
fn straight(caps: Caps, page: &str) -> Straightened {
    let process = Arc::new(Process::default());
    let (direct_clock, clock) = (Arc::new(Time::new(1000.0)), Arc::new(Time::new(1000.0)));
    let direct_backend = LogHandle::new();
    let mut direct = Real::new(300.0, 1.0, false, direct_backend.clone(), &process, &direct_clock);
    let backend = BatchLog::default();
    let host = Rc::new(RefCell::new(Heard::default()));
    let platform = Rc::new(RefCell::new(PlatformEnd::new(
        backend.clone(),
        HeardHandle { heard: Rc::clone(&host), backend: backend.log.clone() },
        Queue::default(),
        caps,
    )));
    let requests: Exchanges = Rc::default();
    let refused = Rc::new(Cell::new(0));
    let channel = Straight { platform: Rc::clone(&platform), requests: Rc::clone(&requests), refused: Rc::clone(&refused) };
    let mut end = RendererEnd::new(300.0, 1.0, false, channel, caps, |w, d, k, proxy| Real::new(w, d, k, proxy, &process, &clock));
    let page = parse(page);
    let want = twidget_ends::Renderer::receive(&mut direct, &page);
    let got = end.receive(&page);
    let healthy = !end.stopped() && refused.get() == 0 && dumps(&got) == dumps(&want);
    let calls = same_calls(&direct_backend.calls(), &backend.log.calls());
    let exchanges = requests.borrow().clone();
    Straightened { healthy, calls, backend, exchanges }
}

/// Under caps of each kind, the words go in requests and answers within every cap, a text no batch can hold alone goes
/// as a single width request, and the platform end makes the direct renderer's Backend calls, each request in one
/// Backend call.
#[test]
fn bounded_batches() {
    let d = Caps::default();
    let cases: [(&str, usize, usize, usize, usize, usize); 13] = [
        ("the default caps", d.widths, d.widths_text, d.message, 1, 0),
        ("4 texts", 4, d.widths_text, d.message, 6, 0),
        ("1 text", 1, d.widths_text, d.message, 21, 0),
        ("0 texts", 0, d.widths_text, d.message, 0, 21),
        ("16 bytes of texts, less than the long word's", d.widths, 16, d.message, 8, 1),
        ("a message of 96 bytes", d.widths, d.widths_text, 96, 4, 0),
        ("40 bytes of texts", d.widths, 40, d.message, 4, 0),
        ("21 texts, all of them", 21, d.widths_text, d.message, 1, 0),
        ("20 texts, one fewer", 20, d.widths_text, d.message, 2, 0),
        ("122 bytes of texts, theirs exactly", d.widths, 122, d.message, 1, 0),
        ("121 bytes of texts, a byte short", d.widths, 121, d.message, 2, 0),
        ("a message of 296 bytes, the request of all 21 exactly", d.widths, d.widths_text, 296, 1, 0),
        ("a message of 295 bytes, a byte short", d.widths, d.widths_text, 295, 2, 0),
    ];
    for (name, widths, widths_text, message, want_requests, want_singles) in cases {
        let what = format!("under a cap of {name}");
        let caps = Caps { widths, widths_text, message, ..Caps::default() };
        let Straightened { healthy, calls, backend, exchanges } = straight(caps, MANY_WORDS);
        assert!(healthy, "{what}: both ends stay healthy and give the direct renderer's messages");
        assert!(calls.is_ok(), "{what}: the platform end makes the direct renderer's Backend calls {calls:?}");
        let (mut requests, mut singles, mut within, mut items) = (0, 0, 0, 0);
        let mut sizes = Vec::new();
        for (request, answer) in &exchanges {
            let kind = kind_of(request);
            if kind == Some(Kind::WidthRequest) {
                singles += 1;
            }
            if kind != Some(Kind::WidthsRequest) {
                continue;
            }
            requests += 1;
            let decoded = WidthsRequest::decode(request, &caps).expect("a request within the caps");
            let text: usize = decoded.texts.iter().map(|t| t.text.len()).sum();
            if WidthsAnswer::decode(answer, &caps, &decoded).is_some() {
                within += usize::from(
                    decoded.texts.len() <= widths && text <= widths_text && request.len() <= message && answer.len() <= message,
                );
            }
            items += decoded.texts.len();
            sizes.push(decoded.texts.len());
        }
        assert_eq!(within, requests, "{what}: every request and answer is within the caps");
        assert_eq!(sizes, *backend.batches.borrow(), "{what}: each request reaches the Backend in one call of its texts");
        assert_eq!(items + singles, backend.log.count(Op::TextWidth), "{what}: every word goes in a batch or alone, once");
        assert!(
            requests == want_requests && singles == want_singles && items + singles == 21,
            "{what}: the 21 texts go in {want_requests} requests and {want_singles} single widths, not {requests} and {singles}"
        );
    }
}

/// Under batches of 4 the layout's breaks go in several requests, each within the caps, and the platform end makes the
/// direct renderer's Backend calls.
#[test]
fn bounded_breaks() {
    let caps = Caps { widths: 4, ..Caps::default() };
    let Straightened { healthy, calls, exchanges, .. } = straight(caps, BREAKING);
    assert!(healthy, "breaks in batches of 4: both ends stay healthy and give the direct renderer's messages");
    assert!(calls.is_ok(), "breaks in batches of 4: the platform end makes the direct renderer's Backend calls {calls:?}");
    let (mut requests, mut within, mut singles) = (0, 0, 0);
    for (request, _) in &exchanges {
        let kind = kind_of(request);
        singles += usize::from(kind == Some(Kind::WidthRequest));
        if kind != Some(Kind::WidthsRequest) {
            continue;
        }
        requests += 1;
        within += usize::from(WidthsRequest::decode(request, &caps).is_some_and(|r| r.texts.len() <= caps.widths));
    }
    assert!(
        requests > 4 && within == requests && singles == 0,
        "breaks in batches of 4: the words and the breaks go in {requests} requests within the cap, none alone"
    );
}

/// html(2) head(3) body(4), a div 300 px tall (10) with a word (11).
const TALL: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","height:300px"]]},
 {"op":"text","id":11,"data":"tall"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]}"#;

/// html(2) head(3) body(4); a text (11) in a div (10), a PNG (12) and a canvas (13) that draws text in its own font.
const PAGE: &str = r##"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[]},
 {"op":"text","id":11,"data":"two ends"},
 {"op":"el","id":12,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgo="]]},
 {"op":"el","id":13,"tag":"canvas","attrs":[["width","40"],["height","20"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},
 {"op":"ins","parent":4,"id":13,"before":null},
 {"op":"canvas","id":13,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10],["s","fillStyle","#336699"],["c","fillRect",0,0,5,5]]}
]}"##;

const SPOILS: [Spoil; 6] = [Spoil::Unanswered, Spoil::Empty, Spoil::WrongKind, Spoil::WrongVersion, Spoil::Cut, Spoil::Trailing];

fn spoil_name(how: Spoil) -> &'static str {
    match how {
        Spoil::Unanswered => "unanswered",
        Spoil::Empty => "answered empty",
        Spoil::WrongKind => "answered as another request",
        Spoil::WrongVersion => "answered in another version",
        Spoil::Cut => "answered one byte short",
        Spoil::Trailing => "answered with a byte after",
        Spoil::Replaced => "answered otherwise",
        Spoil::None => "answered",
    }
}

fn reason_for(how: Spoil) -> &'static str {
    if how == Spoil::Unanswered { "a request got no answer" } else { "an answer did not decode" }
}

fn stale_output() -> Output {
    Output { messages: vec![parse(r#"{"type":"event","ev":{"type":"stale"}}"#)], frame: true }
}

fn empty(out: &Output) -> bool {
    out.messages.is_empty() && !out.frame
}

/// The spoiled request was the last message of the call, and the end gives the expected reason.
fn stopped_at_spoil(lp: &mut Loopback<Looped>, kind: Kind, reason: &str, what: &str) {
    let at = lp.wire.borrow().spoiled;
    let kinds = lp.kinds();
    assert!(at > 0 && kinds[at - 1] == kind as u8, "{what}: the spoiled request is the one named");
    assert_eq!(kinds.len(), at, "{what}: nothing goes out after the spoiled request");
    assert!(lp.end().stopped() && lp.end().stop_reason() == reason, "{what}: stops with \"{reason}\", not \"{}\"", lp.end().stop_reason());
}

/// Once stopped the end answers nothing, sends nothing and calls nothing, and goes without a word.
fn inert(lp: &mut Loopback<Looped>, what: &str) {
    let reason = lp.end().stop_reason();
    let sent = lp.kinds().len();
    let calls = lp.backend.len();
    let heard = lp.events().len();
    assert!(lp.end().stopped() && !reason.is_empty(), "{what}: the end says it stopped");
    assert!(lp.receive(&parse(PAGE)).is_empty(), "{what}: an engine message gets nothing back");
    lp.want_frame();
    lp.report(&[parse(r#"{"type":"size","width":1,"height":2}"#)]);
    let mut out = stale_output();
    assert!(
        !lp.end().deliver(&encoded(&FrameRequest { seq: 9, tick: true, paint: Paint::Force }), &mut out) && empty(&out),
        "{what}: a frame request gets false and an empty output"
    );
    {
        let mut p = lp.platform.borrow_mut();
        p.set_width(200.0);
        p.touch(0, 5.0, 5.0, 1, true, false, 0, 0);
        p.request_frame(true, Paint::Force);
        p.ask_claim(5.0, 5.0);
    }
    let delivered = lp.delivered.len();
    lp.out = stale_output();
    assert!(lp.pump(), "{what}: the platform input drains");
    let refused = lp.delivered.len() == delivered + 4 && (delivered..lp.delivered.len()).all(|i| !lp.delivered[i] && empty(&lp.outputs[i]));
    assert!(refused, "{what}: each platform input gets false and an empty output");
    assert!(
        lp.kinds().len() == sent && lp.wire.borrow().outbox.is_empty() && lp.backend.len() == calls && lp.events().len() == heard,
        "{what}: nothing goes out once stopped"
    );
    assert!(lp.end().stopped() && lp.end().stop_reason() == reason, "{what}: the first reason stays");
    lp.close();
    assert!(lp.kinds().len() == sent && lp.backend.len() == calls && lp.events().len() == heard, "{what}: the end goes without a word");
    assert!(
        lp.host.borrow().refusals.is_empty() && lp.wire.borrow().refused == 0 && !lp.refusing(),
        "{what}: the platform end took all it was sent"
    );
}

/// A loop at scale 1 on the real renderer, given `page`, then a fault on the second request after.
fn failing(caps: Caps, page: &str, how: Spoil) -> (Loopback<Looped>, usize, usize) {
    let process = Arc::new(Process::default());
    let clock = Arc::new(Time::new(1000.0));
    let mut lp = looped(caps, 1.0, &process, &clock);
    lp.receive(&parse(page));
    let batches = lp.count_kind(Kind::WidthsRequest);
    let states = lp.host.borrow().states.len();
    let at = lp.wire.borrow().requests + 2;
    lp.wire.borrow_mut().fault = Fault { how, at, kind: None, reply: Vec::new() };
    (lp, batches, states)
}

/// With batches of 4, a build's second widths request fails every way: the first was answered, nothing goes out after
/// the second, and the end stops.
#[test]
fn batch_failures() {
    let words = r#"{"type":"ops","ops":[{"op":"el","id":30,"tag":"div","attrs":[]},{"op":"text","id":31,"data":"one two three four five six seven eight nine ten"},{"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":30,"id":31,"before":null}]}"#;
    for how in SPOILS {
        let what = format!("the second of three widths requests {}", spoil_name(how));
        let (mut lp, batches, states) = failing(Caps { widths: 4, ..Caps::default() }, TALL, how);
        assert!(lp.receive(&parse(words)).is_empty(), "{what}: the engine message gets nothing back");
        stopped_at_spoil(&mut lp, Kind::WidthsRequest, reason_for(how), &what);
        assert!(
            lp.count_kind(Kind::WidthsRequest) == batches + 2 && lp.host.borrow().states.len() == states,
            "{what}: the first request went, no third and no report"
        );
        inert(&mut lp, &what);
    }
}

/// The layout's request for breaks fails every way: the words' request was answered, nothing goes out after it, and
/// the end stops.
#[test]
fn break_failures() {
    for how in SPOILS {
        let what = format!("the request for breaks {}", spoil_name(how));
        let (mut lp, batches, states) = failing(Caps::default(), BREAKING, how);
        let got = lp.receive(&parse(&format!(r#"{{"type":"ops","ops":[{}]}}"#, flex_item(10, 30, "QXZ"))));
        assert!(got.is_empty(), "{what}: the engine message gets nothing back");
        stopped_at_spoil(&mut lp, Kind::WidthsRequest, reason_for(how), &what);
        assert!(
            lp.count_kind(Kind::WidthsRequest) == batches + 2
                && lp.count_kind(Kind::WidthRequest) == 0
                && lp.host.borrow().states.len() == states,
            "{what}: the words' request went, then the failed one, and nothing after"
        );
        inert(&mut lp, &what);
    }
}

// ---- warm relayouts ----------------------------------------------------------------------------------------------------

/// A word in a box 300 px wide, in a 2 px face.
const LONG_WORD: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:2px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","width:300px"]]},
 {"op":"text","id":11,"data":"LONGWORD"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]}"#;

/// A warm relayout fits the long word again with probes of hundreds of letters, longer than the width cache keeps:
/// each is measured alone, as before the batches, and none asks for the letters of the texts the layout may break,
/// which a new short text's letters show.
#[test]
fn warm_fit_probes() {
    let page = LONG_WORD.replacen("LONGWORD", &"x".repeat(700), 1);
    let mut a = Arms::new("warm fit probes");
    let (mut sent, mut calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(&page, "a word of 700 letters in a box 300 px wide");
    let mut kinds = a.kinds_since(sent);
    let mut asked = texts_of(&widths_since(&a.lp.backend, calls));
    ok!(
        a,
        count(&kinds, Kind::WidthsRequest) == 1 && has(&asked, "x"),
        format!("the first layout asks for the word's letter in a request for breaks ({})", reports_name(&kinds))
    );
    a.frame("the first frame", true, Paint::IfDirty);
    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(
        r#"{"type":"ops","ops":[{"op":"el","id":20,"tag":"p","attrs":[]},{"op":"text","id":21,"data":"qz"},{"op":"ins","parent":4,"id":20,"before":null},{"op":"ins","parent":20,"id":21,"before":null}]}"#,
        "a short text after it, which fits its line",
    );
    kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    asked = texts_of(&widths);
    let long_probes = !widths.is_empty() && widths.iter().all(|c| c.text == b"qz" || c.text.len() > 256);
    ok!(
        a,
        count(&kinds, Kind::WidthsRequest) == 1 && has(&asked, "qz") && !has(&asked, "q") && !has(&asked, "z"),
        format!("the relayout asks for the new word in one request and for no breaks ({})", reports_name(&kinds))
    );
    ok!(
        a,
        long_probes && count(&kinds, Kind::WidthRequest) > 0,
        format!("its fit probes past the cache's longest key are measured alone ({} widths)", widths.len())
    );
    a.frame("the frame after it", true, Paint::IfDirty);
    sent = a.lp.kinds().len();
    calls = a.lp.backend.len();
    a.receive(
        r#"{"type":"ops","ops":[{"op":"style","id":10,"name":"width","value":"100px"},{"op":"el","id":22,"tag":"p","attrs":[]},{"op":"text","id":23,"data":"jk"},{"op":"ins","parent":4,"id":22,"before":null},{"op":"ins","parent":22,"id":23,"before":null}]}"#,
        "the box 100 px wide and another short text",
    );
    kinds = a.kinds_since(sent);
    asked = texts_of(&widths_since(&a.lp.backend, calls));
    let fresh_probes = asked.iter().any(|t| t.len() > 1 && t.len() <= 256 && t.iter().all(|&c| c == b'x'));
    ok!(
        a,
        count(&kinds, Kind::WidthsRequest) == 1 && has(&asked, "jk") && !has(&asked, "j") && !has(&asked, "k") && fresh_probes,
        format!(
            "probes the cache could keep but has not measured yet ask for no breaks either, only for themselves ({})",
            reports_name(&kinds)
        )
    );
    a.frame("the last frame", true, Paint::IfDirty);
    a.close();
}

/// A flex item sized at its narrowest whose preserved tab breaks anywhere, in a face of its own: the layout's request
/// for breaks holds the tab's space.
const PRESERVED_TAB: &str = r#"{"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":11,"tag":"div","attrs":[["style","white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif"]]},
 {"op":"text","id":12,"data":"ab\tcd"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":11,"id":12,"before":null}
]}"#;

fn font_of_size(backend: &LogHandle, size: f32) -> i32 {
    backend.0.borrow().calls.iter().find(|c| c.op == Op::Font && c.size == size).map_or(0, |c| c.font)
}

/// The narrowest a content-sized box can be measures the characters of its preserved spaces too: the layout's one
/// request for breaks asks for them with the letters, and no width goes alone.
#[test]
fn preserved_tab() {
    let mut a = Arms::new("a preserved tab");
    let (sent, calls) = (a.lp.kinds().len(), a.lp.backend.len());
    a.receive(PRESERVED_TAB, "a flex item of two words and a tab, preserved and broken anywhere");
    let kinds = a.kinds_since(sent);
    let widths = widths_since(&a.lp.backend, calls);
    let face = font_of_size(&a.lp.backend, 17.0);
    let space = widths.iter().any(|c| c.font == face && c.text == b" ");
    let asked = texts_of(&widths);
    let letters = ["a", "b", "c", "d"].iter().all(|l| has(&asked, l));
    ok!(
        a,
        batches_of(&kinds, 2) && count(&kinds, Kind::WidthRequest) == 0,
        format!("the build asks for its words and for its breaks in two requests, with no single width ({})", reports_name(&kinds))
    );
    ok!(a, face != 0 && space && letters, "the breaks hold the letters and the tab's space in the item's face");
    ok!(a, measured_once(&widths), "no font and text is measured twice");
    a.frame("the first frame", true, Paint::IfDirty);
    a.close();
}

/// A narrow box whose word breaks anywhere in a 13 px face, a box of `tabs` preserved tabs between two letters that
/// breaks anywhere in a 17 px face, and a box whose preserved line break shows nothing in a 19 px face.
fn tabs_page(tabs: usize) -> String {
    let text = format!("a{}b", r"\t".repeat(tabs));
    format!(
        r#"{{"type":"ops","ops":[
 {{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]}},
 {{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]}},
 {{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]}},
 {{"op":"el","id":10,"tag":"div","attrs":[["style","width:20px;overflow-wrap:anywhere;font:13px monospace"]]}},
 {{"op":"text","id":11,"data":"abcdefgh"}},
 {{"op":"el","id":12,"tag":"div","attrs":[["style","width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif"]]}},
 {{"op":"text","id":13,"data":"{text}"}},
 {{"op":"el","id":14,"tag":"div","attrs":[["style","width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:19px serif"]]}},
 {{"op":"text","id":15,"data":"ab\ncd"}},
 {{"op":"ins","parent":4,"id":10,"before":null}},
 {{"op":"ins","parent":10,"id":11,"before":null}},
 {{"op":"ins","parent":4,"id":12,"before":null}},
 {{"op":"ins","parent":12,"id":13,"before":null}},
 {{"op":"ins","parent":4,"id":14,"before":null}},
 {{"op":"ins","parent":14,"id":15,"before":null}}
]}}"#
    )
}

/// Another narrow box in the 13 px face, before the tabs, whose word's letters no face has measured.
const NEW_LETTERS: &str = r#"{"type":"ops","ops":[{"op":"el","id":20,"tag":"div","attrs":[["style","width:20px;overflow-wrap:anywhere;font:13px monospace"]]},{"op":"text","id":21,"data":"mnopqrst"},
 {"op":"ins","parent":4,"id":20,"before":12},{"op":"ins","parent":20,"id":21,"before":null}]}"#;

/// A Backend that notes, for each widths batch, where its calls lie in the log (ends_tests.cpp's `Marking`, without
/// the heap's blocks).
#[derive(Clone, Default)]
struct Marking {
    log: LogHandle,
    batches: Rc<RefCell<Vec<(usize, usize)>>>,
}

impl Backend for Marking {
    fn create_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, features: &[u8], out: &mut FontMetrics) -> i32 {
        self.log.create_font(families, size, weight, italic, features, out)
    }
    fn create_canvas_font(&mut self, families: &[u8], size: f32, weight: i32, italic: bool, out: &mut FontMetrics) -> i32 {
        self.log.create_canvas_font(families, size, weight, italic, out)
    }
    fn delete_font(&mut self, font: i32) {
        self.log.delete_font(font)
    }
    fn text_width(&mut self, font: i32, text: &[u8]) -> f32 {
        self.log.text_width(font, text)
    }
    fn text_widths(&mut self, texts: &[TextInFont]) -> Vec<f32> {
        let from = self.log.len();
        let widths = self.log.text_widths(texts);
        self.batches.borrow_mut().push((from, self.log.len()));
        widths
    }
    fn load_image(&mut self, id: i32, url: &[u8], w: &mut f32, h: &mut f32) -> bool {
        self.log.load_image(id, url, w, h)
    }
}

fn asked_in(backend: &LogHandle, batch: (usize, usize), font: i32, text: &str) -> bool {
    backend.0.borrow().calls[batch.0..batch.1].iter().any(|c| c.font == font && c.text == text.as_bytes())
}

/// What a warm relayout asks between its words' request and its breaks' request, and in the breaks' request: the
/// layout up to the first character of the new word, then the scan of every text it may break, the tabs' spaces
/// among them. The C++ counts the heap's blocks the scan makes, which safe Rust cannot; the calls are compared instead.
fn discovery(tabs: usize) -> Vec<Vec<u8>> {
    let what = format!("{tabs} preserved tabs: ");
    let process = Arc::new(Process::default());
    let clock = Arc::new(Time::new(1000.0));
    let backend = Marking::default();
    let mut direct = Real::new(300.0, 2.0, false, backend.clone(), &process, &clock);
    twidget_ends::Renderer::receive(&mut direct, &parse(&tabs_page(tabs)));
    let log = &backend.log;
    let (monospace, tab_face, line_face) = (font_of_size(log, 13.0), font_of_size(log, 17.0), font_of_size(log, 19.0));
    let batches = backend.batches.borrow().clone();
    assert!(
        monospace != 0 && tab_face != 0 && line_face != 0 && batches.len() == 2,
        "{what}the cold build asks for its words and its breaks in two requests"
    );
    let cold = batches[1];
    let letters = ["a", "b", "c", "d"].iter().all(|l| asked_in(log, cold, line_face, l));
    assert!(
        asked_in(log, cold, tab_face, " ") && asked_in(log, cold, monospace, "a"),
        "{what}the cold breaks hold the tabs' space and the narrow word's letters"
    );
    assert!(
        letters && !asked_in(log, cold, line_face, " ") && !asked_in(log, cold, line_face, ""),
        "{what}and the letters around the preserved line break, which adds nothing"
    );
    backend.batches.borrow_mut().clear();
    twidget_ends::Renderer::receive(&mut direct, &parse(NEW_LETTERS));
    let batches = backend.batches.borrow().clone();
    assert_eq!(batches.len(), 2, "{what}the warm relayout asks for its new word and then for its breaks");
    let warm = batches[1];
    let calls = log.calls();
    let fresh = warm.1 > warm.0
        && calls[warm.0..warm.1].iter().all(|c| c.font == monospace && c.text.len() == 1 && (b'm'..=b't').contains(&c.text[0]));
    assert!(fresh && asked_in(log, warm, monospace, "m"), "{what}its breaks hold the new letters alone");
    calls[batches[0].1..warm.1].iter().map(Call::line).collect()
}

/// Break discovery in a warm face asks once for a run of spaces' one character: a relayout that misses a letter in
/// another face asks the same beside 200 preserved tabs as beside 100, and the ends make the direct renderer's calls.
#[test]
fn warm_tabs() {
    let (fewer, more) = (discovery(100), discovery(200));
    assert!(
        !fewer.is_empty() && more == fewer,
        "the discovery asks for nothing more for a tab ({} calls beside 100 tabs, {} beside 200)",
        fewer.len(),
        more.len()
    );
    let mut a = Arms::new("many preserved tabs");
    a.receive(&tabs_page(100), "a narrow word, 100 preserved tabs and a preserved line break");
    a.frame("the first frame", true, Paint::IfDirty);
    a.receive(NEW_LETTERS, "a narrow word of new letters before the tabs");
    a.frame("the frame after it", true, Paint::IfDirty);
    a.close();
}
