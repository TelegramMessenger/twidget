//! tdesktop's `tgw/tgw_hosted_glue.{h,cpp}`: a hosted widget's renderer in the helper. The served host's messages go
//! to a TwidgetCore renderer end (`twidget_ends::RendererEnd`, the renderer on `twidget_document`'s engine) whose
//! Backend calls, frames and reports travel to Telegram as the platform's messages and requests, after an `Open`
//! control gave the device ratio; the glue adds what Telegram's in-process renderer decides beside it (the rules): the
//! load's halves, held context menus, the cursor at the pointer from its own document of the renderer's HTML, the
//! clicks the platform may answer, the canvas fonts' allowance, and a stop reported once.
//!
//! Every call comes on the served host's glue thread. A call made while another runs (the host answers a renderer
//! message inside it) waits in a queue for the outer one, as the C++'s `run`; the glue's flags are read outside the
//! state a step holds, so that such a call never waits for it.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use twidget_canvas::{Canvas, CanvasHost};
use twidget_document::{DocumentEngine, Process};
use twidget_ends::{Backend, Channel, Output, Proxy, RendererChannel, RendererEnd};
use twidget_json::Json;
use twidget_remote::{Caps, FrameRequest, GestureClaim, Hover, Kind as RemoteKind, Message, Paint, Touch, kind_of as remote_kind_of};
use twidget_wire::{DisplayList, FontMetrics, Rect, set_em_box};

use crate::cursor::CursorStyles;
use crate::rules::{self, CanvasSpellings};
use crate::seams::{Link, Platform, Renderer};
use crate::wire::{self, Kind};

const AUTO_CURSOR: &[u8] = b"auto";
const HAND_CURSOR: &[u8] = b"pointer";

/// The steady clock in milliseconds: the renderer's (its `Clock`) and the frame message's (`Renderer::clock()`), the
/// clock the engine's platform reads too.
pub type MsClock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// What every hosted renderer of the helper shares: litehtml's state of the process and the steady clock.
#[derive(Clone)]
pub struct HostedOptions {
    pub process: Arc<Process>,
    pub clock: MsClock,
}

impl std::fmt::Debug for HostedOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostedOptions")
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

fn outside_slop(point: Point, origin: Point) -> bool {
    rules::outside_slop(point.x - origin.x, point.y - origin.y)
}

/// A canvas font spelling: its parse, and the font and metrics the backend made of it (id 0 until then).
#[derive(Clone, Debug, Default)]
struct CanvasSpelling {
    font: twidget_css::Font,
    metrics: FontMetrics,
    id: i32,
}

/// The renderer's clock on the glue's.
struct GlueClock(MsClock);

impl twidget_renderer::Clock for GlueClock {
    fn now_ms(&mut self) -> f64 {
        (self.0)()
    }
}

type HostedRenderer = twidget_renderer::Renderer<DocumentEngine<Proxy<GlueChannel>>, Proxy<GlueChannel>>;

/// renderer.h's `Renderer` as the end drives it: the renderer on `twidget_document`'s engine, its container on the
/// same proxy.
pub struct Hosted(HostedRenderer);

impl std::fmt::Debug for Hosted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hosted")
    }
}

impl twidget_ends::Renderer for Hosted {
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

type End = RendererEnd<GlueChannel, Hosted>;
type Step = Box<dyn FnOnce(&Glue, &mut State) + Send>;

/// The glue's flags, read without the state a step holds: the queue of calls made inside a step, and the host's
/// ends of the renderer's messages (`_renderer->onMessage`, none once destroyed).
#[derive(Default)]
struct Control {
    deferred: VecDeque<Step>,
    link: Option<Link>,
    running: bool,
    destroyed: bool,
    stopped: bool,
    frozen: bool,
}

/// The canvas fonts' allowance, which the renderer's canvas font hook reads inside its calls.
struct CanvasFonts {
    spellings: CanvasSpellings<CanvasSpelling>,
    refused: HashSet<Vec<u8>>,
    requests_left: i32,
    refused_sent: bool,
}

/// What the renderer end's channel and the canvas font hook share with the glue.
struct Shared {
    platform: Arc<dyn Platform>,
    control: Mutex<Control>,
    canvas: Mutex<CanvasFonts>,
}

impl Shared {
    fn live(&self) -> bool {
        let c = lock(&self.control);
        !c.stopped && !c.destroyed
    }

    fn refuse_canvas_font(&self, canvas: &mut CanvasFonts) {
        if self.live() && !std::mem::replace(&mut canvas.refused_sent, true) {
            self.platform.send(&wire::encode_canvas_refused());
        }
    }

    fn canvas_font(&self, backend: &mut dyn Backend, css: &[u8], metrics: &mut FontMetrics) -> i32 {
        *metrics = FontMetrics::default();
        let mut guard = lock(&self.canvas);
        let canvas = &mut *guard;
        let found = canvas.spellings.find(css, |text| CanvasSpelling { font: twidget_css::parse_font(text), ..CanvasSpelling::default() });
        let Some(spelling) = found else {
            let canvas = &mut *guard;
            self.refuse_canvas_font(canvas);
            return 0;
        };
        if spelling.id != 0 {
            *metrics = spelling.metrics;
            return spelling.id;
        }
        let font = spelling.font.clone();
        if canvas.refused.contains(css) {
            return 0;
        } else if canvas.requests_left <= 0 {
            self.refuse_canvas_font(canvas);
            return 0;
        }
        canvas.requests_left -= 1;
        let id = backend.create_canvas_font(&font.family, font.size, font.weight, font.italic, metrics);
        if id == 0 {
            *metrics = FontMetrics::default();
            canvas.refused.insert(css.to_vec());
            return 0;
        } else if metrics.em_ascent == 0.0 && metrics.em_descent == 0.0 {
            let (ascent, descent) = (metrics.ascent, metrics.descent);
            set_em_box(metrics, ascent, descent, font.size);
        }
        if let Some(spelling) = canvas.spellings.find(css, |_| CanvasSpelling::default()) {
            spelling.id = id;
            spelling.metrics = *metrics;
        }
        id
    }
}

/// The renderer end's channel (`Remote::RendererChannel`, which the C++ glue is): nothing once the glue stopped.
pub struct GlueChannel {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for GlueChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GlueChannel")
    }
}

impl Channel for GlueChannel {
    fn send(&mut self, message: Vec<u8>) {
        if !lock(&self.shared.control).stopped {
            self.shared.platform.send(&message);
        }
    }
}

impl RendererChannel for GlueChannel {
    fn request(&mut self, message: Vec<u8>, answer: &mut Vec<u8>) -> bool {
        if lock(&self.shared.control).stopped {
            return false;
        }
        match self.shared.platform.request(&message) {
            None => false,
            Some(result) => {
                *answer = result;
                true
            }
        }
    }
}

#[derive(Default)]
struct Mouse {
    held: Vec<Json>,
    origin: Point,
    press: u64,
    hover: i64,
    parsed: i32,
    pressed: bool,
    secondary: bool,
    moved: bool,
    active: bool,
}

#[derive(Default)]
struct Finger {
    held: Vec<Json>,
    press: u64,
    active: bool,
    unheld_tick: bool,
}

/// What the steps change, held by the step that runs.
struct State {
    end: Option<End>,
    styles: Option<CursorStyles>,
    pending_size: Vec<u8>,
    pointer: Option<Point>,
    mouse: Mouse,
    finger: Finger,
    downs: u64,
    pointers: u64,
    cursor_builds: i32,
    hand: bool,
    dark: bool,
    load_called: bool,
    ready: bool,
    has_content: bool,
    acked: bool,
    loaded: bool,
}

/// The glue (`Tgw::Glue`).
pub struct Glue {
    shared: Arc<Shared>,
    options: HostedOptions,
    caps: Caps,
    width: f64,
    state: Mutex<State>,
}

impl std::fmt::Debug for Glue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Glue")
    }
}

impl Drop for Glue {
    fn drop(&mut self) {
        let mut st = lock(&self.state);
        st.end = None;
    }
}

impl Glue {
    fn new(platform: Arc<dyn Platform>, dark: bool, width: f64, options: HostedOptions) -> Glue {
        Glue {
            shared: Arc::new(Shared {
                platform,
                control: Mutex::new(Control::default()),
                canvas: Mutex::new(CanvasFonts {
                    spellings: CanvasSpellings::default(),
                    refused: HashSet::new(),
                    requests_left: rules::MAX_CANVAS_FONT_REQUESTS,
                    refused_sent: false,
                }),
            }),
            options,
            caps: wire::embedding_caps(),
            width,
            state: Mutex::new(State {
                end: None,
                styles: None,
                pending_size: Vec::new(),
                pointer: None,
                mouse: Mouse::default(),
                finger: Finger::default(),
                downs: 0,
                pointers: 0,
                cursor_builds: 0,
                hand: false,
                dark,
                load_called: false,
                ready: false,
                has_content: false,
                acked: false,
                loaded: false,
            }),
        }
    }

    fn control(&self) -> MutexGuard<'_, Control> {
        lock(&self.shared.control)
    }

    fn attach(&self, link: Link) {
        self.control().link = Some(link);
    }

    fn platform_message(&self, message: Vec<u8>) {
        self.run(Box::new(move |g, st| {
            if !g.live() {
            } else if wire::is_control(&message) {
                g.control_message(st, &message);
            } else if st.end.is_none() {
                g.stop(st, b"a renderer message came before Open");
            } else {
                g.deliver(st, &message);
            }
        }));
    }

    fn load(&self) {
        self.run(Box::new(|g, st| {
            if g.live() {
                st.load_called = true;
                g.become_ready(st);
            }
        }));
    }

    fn host_message(&self, json: &[u8]) {
        if !self.live() || self.control().frozen {
            return;
        }
        let Some(parsed) = Json::parse(json) else {
            self.diag(b"renderer-message", b"unparsed host message");
            return;
        };
        self.run(Box::new(move |g, st| {
            if g.live() && !g.control().frozen {
                g.received(st, &parsed);
            }
        }));
    }

    fn set_theme(&self, dark: bool) {
        self.run(Box::new(move |g, st| {
            if !g.live() || g.control().frozen {
                return;
            }
            st.dark = dark;
            if st.end.is_some() {
                g.apply_theme(st);
            }
        }));
    }

    fn destroy(&self) {
        let running = {
            let mut c = self.control();
            if std::mem::replace(&mut c.destroyed, true) {
                return;
            }
            c.link = None;
            if !c.running {
                c.deferred.clear();
            }
            c.running
        };
        if !running {
            let mut st = lock(&self.state);
            st.styles = None;
            st.end = None;
        }
    }

    fn run(&self, step: Step) {
        {
            let mut c = self.control();
            if c.running {
                c.deferred.push_back(step);
                return;
            }
            c.running = true;
        }
        let mut st = lock(&self.state);
        step(self, &mut st);
        loop {
            let next = {
                let mut c = self.control();
                if c.destroyed { None } else { c.deferred.pop_front() }
            };
            let Some(next) = next else { break };
            next(self, &mut st);
        }
        let destroyed = {
            let mut c = self.control();
            c.deferred.clear();
            c.running = false;
            c.destroyed
        };
        if destroyed {
            st.styles = None;
            st.end = None;
        }
    }

    fn control_message(&self, st: &mut State, message: &[u8]) {
        let kind = wire::kind_of(message);
        let Some(kind) = kind.filter(|k| wire::to_helper(*k) && !wire::is_tap_control(*k)) else {
            self.stop(st, b"a control the widget helper does not take");
            return;
        };
        if kind == Kind::Open {
            let dpr = wire::decode_open(message);
            if st.end.is_some() {
                self.stop(st, b"a second Open");
            } else if let Some(dpr) = dpr {
                self.open(st, dpr);
            } else {
                self.stop(st, b"an Open that did not decode");
            }
            return;
        } else if kind == Kind::CanvasRefill {
            if wire::is_exactly(message, Kind::CanvasRefill) {
                let mut canvas = lock(&self.shared.canvas);
                canvas.spellings.refill();
                canvas.refused.clear();
                canvas.requests_left = rules::MAX_CANVAS_FONT_REQUESTS;
            } else {
                self.stop(st, b"a canvas refill that did not decode");
            }
            return;
        }
        let press = wire::decode_press(message);
        if st.end.is_none() {
            self.stop(st, b"a press control came before Open");
        } else if let Some(press) = press {
            match kind {
                Kind::MenuDeliver => self.menu_deliver(st, press),
                Kind::FingerHold => self.finger_hold(st, press),
                _ => self.finger_release(st, press),
            }
        } else {
            self.stop(st, b"a press control that did not decode");
        }
    }

    fn open(&self, st: &mut State, dpr: f64) {
        let channel = GlueChannel { shared: Arc::clone(&self.shared) };
        let (process, clock) = (Arc::clone(&self.options.process), Arc::clone(&self.options.clock));
        st.end = Some(RendererEnd::new(self.width, rules::device_ratio(dpr), false, channel, self.caps, move |w, d, k, proxy| {
            let engine = DocumentEngine::new(process, proxy.clone());
            Hosted(twidget_renderer::Renderer::new(w, d, k, engine, proxy, Some(Box::new(GlueClock(clock)))))
        }));
        if self.halted(st) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        if let Some(end) = st.end.as_mut() {
            end.renderer().0.set_canvas_font_hook(Some(Box::new(
                move |backend: &mut dyn Backend, css: &[u8], metrics: &mut FontMetrics| shared.canvas_font(backend, css, metrics),
            )));
        }
        self.apply_theme(st);
        self.become_ready(st);
    }

    fn apply_theme(&self, st: &mut State) {
        if self.halted(st) {
            return;
        }
        let mut out = Vec::new();
        let dark = st.dark;
        let Some(end) = st.end.as_mut() else { return };
        end.renderer().0.set_theme(dark, &mut out);
        end.report(&out);
        if !self.halted(st) {
            self.forward_all(st, out);
            self.recheck_cursor(st);
        }
    }

    fn deliver(&self, st: &mut State, message: &[u8]) {
        let kind = remote_kind_of(message);
        let mut touch = None;
        let mut hover = None;
        let mut frame = None;
        let mut release = 0;
        if kind == Some(RemoteKind::Touch) {
            if let Some(decoded) = Touch::decode(message, &self.caps) {
                touch = Some(decoded);
                release = self.clicking(st, &decoded);
                self.pressed(st, &decoded);
            }
        } else if kind == Some(RemoteKind::Hover) {
            hover = Hover::decode(message, &self.caps);
        } else if kind == Some(RemoteKind::FrameRequest) {
            frame = FrameRequest::decode(message, &self.caps);
        }
        if let Some(f) = &frame
            && !f.tick
            && f.paint != Paint::Skip
        {
            let settled = self.take_settled(st);
            if self.halted(st) {
                return;
            }
            self.forward_all(st, settled);
            if !self.live() {
                return;
            }
        }
        let mut out = Output::default();
        if let Some(end) = st.end.as_mut() {
            end.deliver(message, &mut out);
        }
        if self.halted(st) {
            return;
        } else if kind == Some(RemoteKind::Touch) || kind == Some(RemoteKind::Hover) {
            self.pointed(st, touch.as_ref(), hover.as_ref(), &mut out.messages);
            if !self.live() {
                return;
            }
        }
        if release != 0 && out.messages.iter().any(rules::is_click) {
            self.shared.platform.send(&wire::encode_clicked(release));
        }
        if let Some(touch) = &touch {
            self.hold_menus(st, touch, &mut out.messages);
        } else if frame.as_ref().is_some_and(|f| f.tick) {
            self.hold_ticked(st, &mut out.messages);
        }
        self.forward_all(st, out.messages);
        if out.frame {
            self.forward(st, &rules::frame_message((self.options.clock)() / 1000.0));
        }
        self.recheck_cursor(st);
    }

    fn received(&self, st: &mut State, message: &Json) {
        let ty = message.get("type").as_str();
        if ty == b"frozen" {
            self.control().frozen = true;
            st.styles = None;
            st.mouse = Mouse::default();
            st.finger = Finger::default();
            return;
        }
        let Some(end) = st.end.as_mut() else { return };
        if ty == b"requestFrame" {
            end.want_frame();
            self.note_end_stop(st);
            return;
        }
        let mut out = end.receive(message);
        if self.halted(st) {
            return;
        }
        let load = rules::advance_load(message, &mut st.has_content, &mut st.acked, &mut st.loaded);
        if load.content {
            self.shared.platform.send(&wire::encode_content());
        }
        if load.loaded {
            self.shared.platform.send(&wire::encode_loaded());
            if ty == b"ops" {
                let settled = self.take_settled(st);
                rules::append(&mut out, settled);
                if self.halted(st) {
                    return;
                }
            }
        }
        self.forward_all(st, out);
        self.recheck_cursor(st);
    }

    fn take_settled(&self, st: &mut State) -> Vec<Json> {
        match st.end.as_mut() {
            Some(end) => rules::take_settled(|query| end.receive(query)),
            None => Vec::new(),
        }
    }

    fn become_ready(&self, st: &mut State) {
        if st.ready || !st.load_called || st.end.is_none() || !self.live() {
            return;
        }
        st.ready = true;
        let pending = std::mem::take(&mut st.pending_size);
        self.to_host(rules::READY_MESSAGE);
        if !pending.is_empty() && self.live() {
            self.to_host(&pending);
        }
    }

    fn pressed(&self, st: &mut State, touch: &Touch) {
        let down = touch.action == rules::TOUCH_DOWN;
        let ended = touch.action == rules::TOUCH_UP || touch.action == rules::TOUCH_CANCEL;
        if down {
            st.downs += 1;
        }
        if touch.mouse {
            if down {
                st.mouse = Mouse { press: st.downs, pressed: true, secondary: touch.button == rules::SECONDARY_BUTTON, ..Mouse::default() };
            } else if ended {
                st.mouse = Mouse::default();
            }
        } else if down {
            st.finger = Finger { press: st.downs, active: true, ..Finger::default() };
        } else if ended {
            st.finger = Finger::default();
        }
    }

    fn clicking(&self, st: &State, touch: &Touch) -> u64 {
        if touch.mouse && touch.action == rules::TOUCH_UP && st.mouse.pressed && !st.mouse.secondary { st.mouse.press } else { 0 }
    }

    fn pointed(&self, st: &mut State, touch: Option<&Touch>, hover: Option<&Hover>, out: &mut Vec<Json>) {
        st.pointers += 1;
        if let Some(hover) = hover {
            st.pointer = (hover.action == rules::HOVER_MOVE).then_some(Point { x: hover.x, y: hover.y });
            if st.pointer.is_none() {
                st.styles = None;
            }
        } else if let Some(touch) = touch.filter(|t| t.mouse) {
            if touch.button == rules::PRIMARY_BUTTON {
                st.pointer = Some(Point { x: touch.x, y: touch.y });
            }
            self.track_press(st, touch, out);
            if self.halted(st) {
                return;
            }
        }
        if let Some(end) = st.end.as_mut() {
            st.hand = end.renderer().0.cursor() == HAND_CURSOR;
        }
        self.report_cursor(st, out);
    }

    fn parsed_documents(st: &mut State) -> i32 {
        st.end.as_mut().map_or(0, |end| rules::parsed_documents(end.renderer().0.stats()))
    }

    fn builds(st: &mut State) -> i32 {
        st.end.as_mut().map_or(0, |end| end.renderer().0.stats().builds)
    }

    fn track_press(&self, st: &mut State, touch: &Touch, out: &mut Vec<Json>) {
        if !st.mouse.pressed {
            return;
        }
        let point = Point { x: touch.x, y: touch.y };
        if touch.action == rules::TOUCH_DOWN {
            st.mouse.origin = point;
            st.mouse.hover = self.hit_at(st, point, out).unwrap_or(0);
            st.mouse.parsed = Glue::parsed_documents(st);
            st.mouse.active = !st.mouse.secondary;
            return;
        } else if touch.action != rules::TOUCH_MOVE {
            return;
        }
        st.mouse.moved = st.mouse.moved || outside_slop(point, st.mouse.origin);
        if !st.mouse.moved || !st.mouse.active {
            return;
        }
        let synced = st.mouse.parsed == Glue::parsed_documents(st);
        st.mouse.active = synced && self.hit_at(st, point, out) == Some(st.mouse.hover);
    }

    fn recheck_cursor(&self, st: &mut State) {
        if st.pointer.is_none() || st.end.is_none() || !self.live() || Glue::builds(st) == st.cursor_builds {
            return;
        }
        let mut out = Vec::new();
        self.report_cursor(st, &mut out);
        if self.live() {
            self.forward_all(st, out);
        }
    }

    fn report_cursor(&self, st: &mut State, out: &mut Vec<Json>) {
        let Some(pointer) = st.pointer.filter(|_| !self.control().frozen) else {
            st.cursor_builds = Glue::builds(st);
            return;
        };
        let hand = self.hand_at(st, pointer, out);
        if self.halted(st) {
            return;
        }
        st.cursor_builds = Glue::builds(st);
        let Some(hand) = hand.filter(|h| *h != st.hand) else { return };
        st.hand = hand;
        self.shared.platform.send(&wire::encode_cursor(st.pointers, if st.hand { HAND_CURSOR } else { AUTO_CURSOR }));
    }

    fn hand_at(&self, st: &mut State, point: Point, out: &mut Vec<Json>) -> Option<bool> {
        let hit = self.hit_at(st, point, out);
        if hit.is_none() || self.halted(st) {
            return None;
        }
        let hit = hit?;
        if hit == 0 {
            return Some(false);
        }
        let held = st.mouse.pressed && !st.mouse.moved && !outside_slop(point, st.mouse.origin);
        let synced = st.mouse.parsed == Glue::parsed_documents(st);
        let targets = rules::pointer_targets(hit, held, synced, st.mouse.hover, st.mouse.active);
        let State { end, styles, .. } = st;
        let renderer = &mut end.as_mut()?.renderer().0;
        let styles = styles.get_or_insert_with(|| CursorStyles::new(renderer.dpr(), Arc::clone(&self.options.process)));
        let builds = renderer.stats().builds;
        let result = styles.pointer(renderer, hit, targets.hover, targets.active);
        if renderer.stats().builds != builds {
            let settled = self.take_settled(st);
            rules::append(out, settled);
        }
        Some(result)
    }

    fn hit_at(&self, st: &mut State, point: Point, out: &mut Vec<Json>) -> Option<i64> {
        let mut query = Json::object();
        query.set("q", "elementFromPoint");
        query.set("x", point.x);
        query.set("y", point.y);
        let id = self.local_answer(st, query, out)?;
        if !id.is_number() {
            return None;
        }
        Some(id.as_number(0.0) as i64)
    }

    fn local_answer(&self, st: &mut State, query: Json, out: &mut Vec<Json>) -> Option<Json> {
        let messages = st.end.as_mut()?.receive(&rules::local_query(query));
        let mut result = None;
        for message in messages {
            if rules::of_type(&message, b"result") {
                result = Some(message.get("result").clone());
            } else {
                out.push(message);
            }
        }
        result
    }

    fn hold_menus(&self, st: &mut State, touch: &Touch, out: &mut Vec<Json>) {
        if touch.mouse && touch.action == rules::TOUCH_DOWN && touch.button == rules::SECONDARY_BUTTON {
            st.mouse.held = rules::take_context_menus(out, b"mouse");
        }
    }

    fn hold_ticked(&self, st: &mut State, out: &mut Vec<Json>) {
        if st.finger.active && !std::mem::replace(&mut st.finger.unheld_tick, false) {
            let menus = rules::take_context_menus(out, b"touch");
            rules::append(&mut st.finger.held, menus);
        }
    }

    fn menu_deliver(&self, st: &mut State, press: u64) {
        if st.mouse.pressed && st.mouse.secondary && st.mouse.press == press {
            let held = std::mem::take(&mut st.mouse.held);
            self.forward_all(st, held);
        }
    }

    fn finger_hold(&self, st: &mut State, press: u64) {
        if st.finger.active && st.finger.press == press {
            st.finger.unheld_tick = true;
            let held = std::mem::take(&mut st.finger.held);
            self.forward_all(st, held);
        }
    }

    fn finger_release(&self, st: &mut State, press: u64) {
        if st.finger.active && st.finger.press == press {
            let held = std::mem::take(&mut st.finger.held);
            self.forward_all(st, held);
        }
    }

    fn live(&self) -> bool {
        self.shared.live()
    }

    fn halted(&self, st: &mut State) -> bool {
        self.note_end_stop(st);
        !self.live()
    }

    fn note_end_stop(&self, st: &mut State) {
        let reason = st.end.as_ref().filter(|end| end.stopped()).map(|end| end.stop_reason());
        if let Some(reason) = reason {
            self.stop(st, reason.as_bytes());
        }
    }

    fn stop(&self, st: &mut State, reason: &[u8]) {
        if std::mem::replace(&mut self.control().stopped, true) {
            return;
        }
        st.pending_size = Vec::new();
        st.styles = None;
        st.mouse = Mouse::default();
        st.finger = Finger::default();
        self.shared.platform.send(&wire::encode_stopped(reason));
    }

    fn forward(&self, st: &mut State, message: &Json) {
        if self.control().frozen || !self.live() {
        } else if st.ready {
            self.to_host(&message.dump());
        } else if rules::kept_before_ready(message) {
            st.pending_size = message.dump();
        }
    }

    fn forward_all(&self, st: &mut State, messages: Vec<Json>) {
        for message in &messages {
            self.forward(st, message);
        }
    }

    fn to_host(&self, json: &[u8]) {
        let link = {
            let c = self.control();
            if c.destroyed { None } else { c.link.clone() }
        };
        if let Some(link) = link {
            (link.message)(json);
        }
    }

    fn diag(&self, kind: &[u8], detail: &[u8]) {
        let link = {
            let c = self.control();
            if c.destroyed { None } else { c.link.clone() }
        };
        if let Some(link) = link {
            (link.diag)(kind, detail);
        }
    }
}

/// `HostedRenderer`: the served host's renderer, the glue behind it; dropping it destroys the glue.
struct HostedRendererHandle {
    glue: Arc<Glue>,
}

impl Drop for HostedRendererHandle {
    fn drop(&mut self) {
        self.glue.destroy();
    }
}

impl Renderer for HostedRendererHandle {
    fn connect(&self, link: Link) {
        self.glue.attach(link);
    }
    fn load(&self) {
        self.glue.load();
    }
    fn send(&self, json: &[u8]) {
        self.glue.host_message(json);
    }
    fn set_theme(&self, theme: &[u8]) {
        self.glue.set_theme(theme == b"dark");
    }
    fn set_width(&self, _width: f64) {}
    fn destroy(&self) {
        self.glue.destroy();
    }
}

struct NoCanvasHost;

impl CanvasHost for NoCanvasHost {
    fn canvas_font(&mut self, _css: &[u8], _metrics: &mut FontMetrics) -> i32 {
        0
    }
    fn canvas_text_width(&mut self, _font: i32, _text: &[u8]) -> f32 {
        0.0
    }
    fn canvas_source(&mut self, _node: i64) -> Option<twidget_canvas::CanvasSource> {
        None
    }
    fn canvas_allowance(&mut self, _id: i32) -> i64 {
        0
    }
}

/// `PrepareHostedRenderers`: the C++ fills TwidgetCore's lazy base64 table before any glue thread runs; Rust's tables
/// are made at compile time, and the same call is made for the same effect elsewhere (none).
pub fn prepare_hosted_renderers() {
    let mut pixels = Json::object();
    pixels.set("b64", "AAAA");
    let mut arguments = Json::array();
    arguments.push(pixels);
    let mut host = NoCanvasHost;
    let canvas = Canvas::new(0, 0, 0, &mut host);
    canvas.call(b"getTransform", &arguments);
}

/// `MakeHostedRenderer`: the glue for one widget, none without a platform; the platform's messages reach it while it
/// lives.
pub fn make_hosted_renderer(
    scheme: &[u8],
    width: f64,
    platform: Option<Arc<dyn Platform>>,
    options: &HostedOptions,
) -> Option<Box<dyn Renderer>> {
    let platform = platform?;
    let glue = Arc::new(Glue::new(Arc::clone(&platform), scheme == b"dark", width, options.clone()));
    let weak: Weak<Glue> = Arc::downgrade(&glue);
    platform.set_on_message(Arc::new(move |message: &[u8]| {
        if let Some(strong) = weak.upgrade() {
            strong.platform_message(message.to_vec());
        }
    }));
    Some(Box::new(HostedRendererHandle { glue }))
}
