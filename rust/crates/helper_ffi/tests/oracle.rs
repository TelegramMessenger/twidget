//! The C ABI from C++ (`oracle/helper_ffi_oracle`, built from a tdesktop checkout by `oracle/helper_oracles.cmake`): the
//! helper's stream served by tdesktop's `WidgetHelper::Serve` (`_other/widget_helper_renderer.cpp` on widget-sandbox's
//! engine end, the hosted glue and the tap) and by `tw_helper_serve` from the same C++ program, each over an in-memory
//! stream the program drives as Telegram would, on one step clock; every frame either sends is compared byte for byte
//! (but the engine's heap in `Stats`: both ends count it on nova_meter's accounts, and Nova's random hash keys make it
//! vary from run to run; `TW_HELPER_ORACLE_MEMLOG` logs it). The sessions are the glue comparison's
//! (`twidget_helper`'s `glue_oracle.rs`): widget-sandbox's corpus widgets and the generated documents, now as hosted
//! widgets, with Telegram's platform traffic as it is, the theme, safe area, visibility and stop as frames, and the
//! widget's own batches, listeners, queries and canvases made by its script through probes. Then mutated streams
//! through both, and the ABI's contract stretched and broken. Skipped without `TWIDGET_ORACLE_DIR` or tdesktop.

#[path = "../../events/tests/common/mod.rs"]
mod events;

use std::path::PathBuf;

use events::inputs::{corpus_files, generated_html, pick};
use twidget_bundle_stamp::SandboxResources;
use twidget_engine::{SandboxConfig, SandboxHostedStart, WidgetContentSafeArea, WidgetTheme};
use twidget_helper::wire;
use twidget_remote::{ClaimQuery, ClearSelection, FrameRequest, Hover, Message, Paint, SetMaxHeight, SetTheme, SetWidth, Touch};
use twidget_server::coding::{encode_config, encode_hosted_start, encode_safe_area, encode_theme};
use twidget_stream::{Field, Kind, MAX_OPENING_BYTES, VERSION};
use twidget_testkit::{Fields, Lcg, Oracle, mutate};

/// `helper_ffi_oracle`, or None when the oracle was built without tdesktop's sources (helper_oracles.cmake's marker).
fn oracle() -> Option<Oracle> {
    let dir = std::env::var_os("TWIDGET_ORACLE_DIR")?;
    let marker = PathBuf::from(&dir).join("helper_oracles.txt");
    match std::fs::read_to_string(&marker) {
        Ok(text) if text.starts_with("built") => Oracle::find("helper_ffi_oracle"),
        Ok(text) => {
            eprintln!("the oracle was configured without tdesktop's sources ({}): the C ABI's comparison is skipped", text.trim());
            None
        }
        Err(_) => {
            assert!(std::env::var_os("TWIDGET_ORACLE_REQUIRED").is_none(), "no {} beside the oracle programs", marker.display());
            None
        }
    }
}

fn encode(m: &impl Message) -> Vec<u8> {
    let mut out = Vec::new();
    assert!(m.encode(&wire::embedding_caps(), &mut out));
    out
}

/// The records of one run of the oracle, and what each is for a failure.
#[derive(Default)]
struct Run {
    input: Vec<u8>,
    shown: Vec<String>,
}

impl Run {
    fn push(&mut self, op: u8, f: &Fields, shown: String) {
        self.input.extend(f.framed(op));
        self.shown.push(shown);
    }
}

/// One hosted widget's session: its start, Telegram's traffic and the widget's script, each step on the clock.
struct Builder<'a> {
    r: &'a mut Lcg,
    run: &'a mut Run,
    runtime: u32,
    clock: f64,
    width: f64,
    seq: u64,
    name: String,
    tapped: bool,
    /// The glue stopped (a control it does not take): the engine's layout queries then wait out their 3 s, which the
    /// C++ end measures on `std::chrono` and the Rust one on the platform's clock, the oracle's step clock here.
    stopped: bool,
}

impl Builder<'_> {
    fn tick(&mut self) -> f64 {
        self.clock += *pick(self.r, &[0.0, 0.008, 0.016, 0.016, 0.016, 0.033, 0.12, 0.4, 1.2]);
        self.clock
    }
    fn step(&mut self, op: u8, f: impl FnOnce(&mut Fields), shown: String) {
        let clock = self.tick();
        let mut fields = Fields::new();
        fields.u32(self.runtime).f64(clock);
        f(&mut fields);
        self.run.push(op, &fields, format!("{}, step {}: {} {shown}", self.name, self.run.shown.len(), op as char));
    }
    fn platform(&mut self, message: Vec<u8>) {
        let shown = format!("{:?}", &message[..message.len().min(24)]);
        self.step(b'P', |f| _ = f.str(&message), shown);
    }
    fn script(&mut self, code: String) {
        let shown = code.chars().take(160).collect();
        self.step(b'J', |f| _ = f.str(code.as_bytes()), shown);
    }
    fn frame(&mut self, tick: bool, paint: Paint) {
        self.seq += 1;
        let seq = self.seq;
        self.platform(encode(&FrameRequest { seq, tick, paint }));
    }
    fn point(&mut self) -> (f64, f64) {
        let x = if self.r.chance(12) { -4.0 } else { self.r.below(self.width as usize + 6) as f64 + self.r.below(4) as f64 * 0.25 };
        let tall = if self.r.chance(3) { 80 } else { 360 };
        let y = self.r.below(tall) as f64 + self.r.below(2) as f64 * 0.5;
        (x, y)
    }
    fn touch(&mut self, action: i32, mouse: bool, button: i32) {
        let (x, y) = self.point();
        self.platform(encode(&Touch { action, x, y, pointer: if mouse { 0 } else { 1 }, primary: true, mouse, modifiers: 0, button }));
    }
    /// The widget's script changing its document a little, as `Shadow::patch`'s batches do: classes, cursors, texts,
    /// removals, insertions, links, attributes gone.
    fn patch(&mut self) {
        let mut body = String::new();
        for _ in 0..1 + self.r.below(4) {
            let k = self.r.below(1000);
            let change = match self.r.below(8) {
                0 | 1 => format!("e.className='c{} c{}'", self.r.below(8), self.r.below(8)),
                2 => format!("e.style.cursor='{}'", pick(self.r, &["pointer", "text", "auto", ""])),
                3 => format!("if(!e.children.length)e.textContent='{}'", pick(self.r, &["changed", "x", ""])),
                4 => "if(e.parentNode)e.remove()".to_string(),
                5 => {
                    let tag = pick(self.r, &["div", "span", "a", "button"]);
                    let into = if self.r.chance(3) { "document.body" } else { "e" };
                    format!(
                        "{{const n=document.createElement('{tag}');n.className='c{}';n.textContent='n';{into}.appendChild(n)}}",
                        self.r.below(8)
                    )
                }
                6 => "e.setAttribute('href','#')".to_string(),
                _ => "e.removeAttribute('class')".to_string(),
            };
            body.push_str(&format!("{{const a=document.querySelectorAll('body *');if(a.length){{const e=a[{k}%a.length];{change}}}}}"));
        }
        self.script(format!("(()=>{{{body};return document.querySelectorAll('body *').length}})()"));
    }
}

/// Scripts out of a corpus widget: the glue's sessions see its document as the validator would make it of the
/// markup alone, and a script's timers run on the real clock, which two servings cannot share.
fn without_scripts(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.to_ascii_lowercase().find("<script") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        rest = match tail.to_ascii_lowercase().find("</script>") {
            Some(end) => &tail[end + "</script>".len()..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

const LISTEN: &str = "(()=>{document.body.addEventListener('click',e=>{if(e.target&&e.target.classList)e.target.classList.toggle('c1')});\
document.body.addEventListener('pointerdown',e=>{document.body.setAttribute('data-p',String(e.clientX))});\
document.body.addEventListener('contextmenu',e=>{e.preventDefault()});\
document.body.addEventListener('mouseover',e=>{if(e.target&&e.target.classList)e.target.classList.add('c2')});return 'listening'})()";

/// The time zone and the CSPRNG, which the widget reaches through the platform's answers.
const CLOCKS: &str = "(()=>{const d=new Date(1700000000000);const a=new Uint8Array(6);let r='none';try{crypto.getRandomValues(a);r=Array.from(a).join('.')}catch(e){r='threw '+e}return [d.getHours(),d.getTimezoneOffset(),r].join(',')})()";

const CANVAS: &str = "(()=>{const c=document.createElement('canvas');c.width=120;c.height=40;document.body.appendChild(c);\
globalThis.__g=c.getContext('2d');return typeof globalThis.__g})()";

/// The glue comparison's session (`glue_oracle.rs`) for a hosted widget: its start and the platform's Open (load and
/// Open in either order there; here the served host loads it at its start), the widget's listeners and canvas, then
/// hovers, mouse and finger presses with the press controls, frames that tick and paint, the script's patches and
/// queries, the theme and safe area, sizes, claims, selection, refills, bad controls, canvas drawing, a stop; the
/// tap's mirror at the end, then its close.
fn session(b: &mut Builder<'_>, html: &str, steps_n: usize) {
    let width = *pick(b.r, &[300.0, 320.0, 412.5]);
    b.width = width;
    let dark = b.r.chance(4);
    let scheme: &[u8] = if dark { b"dark" } else { b"light" };
    let flags = u8::from(!b.r.chance(5)) | (u8::from(b.r.chance(2)) << 1);
    let clock = b.tick();
    let mut start = Fields::new();
    start.u32(b.runtime).f64(clock).str(html.as_bytes()).f64(width).str(scheme).u8(flags);
    b.run.push(b'B', &start, format!("{}, start", b.name));
    let dpr = *pick(b.r, &[1.0, 2.0, 1.5, 3.0, 0.1, 100.0]);
    b.platform(wire::encode_open(dpr));
    if !b.r.chance(4) {
        b.script(LISTEN.to_string());
    }
    if b.r.chance(2) {
        b.script(CLOCKS.to_string());
    }
    let drawing = b.r.chance(2);
    if drawing {
        b.script(CANVAS.to_string());
    }
    if b.tapped && b.r.chance(3) {
        b.platform(wire::encode_mirror_work());
    }
    b.frame(true, Paint::IfDirty);
    let mut downs = 0u64;
    for _ in 0..steps_n {
        match b.r.below(46) {
            0..=5 => {
                let (x, y) = b.point();
                let action = if b.r.chance(10) { 1 } else { 0 };
                b.platform(encode(&Hover { action, x, y, modifiers: 0 }));
            }
            6..=8 => {
                downs += 1;
                let button = if b.r.chance(4) { 2 } else { 0 };
                b.touch(0, true, button);
                for _ in 0..b.r.below(3) {
                    b.touch(1, true, button);
                }
                if b.r.chance(4) {
                    b.frame(true, Paint::IfDirty);
                }
                if button == 2 && b.r.chance(2) {
                    b.platform(wire::encode_menu_deliver(downs));
                }
                let end = if b.r.chance(8) { 3 } else { 2 };
                b.touch(end, true, button);
            }
            9 | 10 => {
                downs += 1;
                b.touch(0, false, 0);
                if b.r.chance(2) {
                    b.frame(true, Paint::Skip);
                }
                match b.r.below(3) {
                    0 => b.platform(wire::encode_finger_hold(downs)),
                    1 => b.platform(wire::encode_finger_release(downs)),
                    _ => {}
                }
                b.frame(true, Paint::IfDirty);
                b.touch(2, false, 0);
            }
            11..=17 => {
                let tick = !b.r.chance(4);
                let paint = *pick(b.r, &[Paint::IfDirty, Paint::IfDirty, Paint::Force, Paint::Skip]);
                b.frame(tick, paint);
            }
            18..=21 => b.patch(),
            22 | 23 if b.stopped => b.frame(true, Paint::IfDirty),
            22 => {
                let k = b.r.below(1000);
                b.script(format!(
                    "(()=>{{const a=document.querySelectorAll('body *');if(!a.length)return 'none';const r=a[{k}%a.length].getBoundingClientRect();return [r.x,r.y,r.width,r.height].join(',')}})()"
                ));
            }
            23 => {
                let (x, y) = b.point();
                b.script(format!("(()=>{{const e=document.elementFromPoint({x},{y});return e?e.tagName:'none'}})()"));
            }
            24 => b.script("(()=>{requestAnimationFrame(t=>{document.body.setAttribute('data-t',String(t))});return 'raf'})()".to_string()),
            25 | 32 => {
                let scheme = if b.r.chance(2) { b"dark".to_vec() } else { b"light".to_vec() };
                b.step(b'T', |f| _ = f.str(&scheme), String::from_utf8_lossy(&scheme).into_owned());
            }
            26 => {
                let w = *pick(b.r, &[280.0, 300.0, 412.5]);
                b.platform(encode(&SetWidth { width: w }));
            }
            27 => {
                let height = *pick(b.r, &[200.0, 600.0, 1200.0]);
                b.platform(encode(&SetMaxHeight { height }));
            }
            28 => {
                let dark = b.r.chance(2);
                b.platform(encode(&SetTheme { dark }));
            }
            29 => {
                let (x, y) = b.point();
                b.platform(encode(&ClaimQuery { seq: 3, x, y }));
            }
            30 => b.platform(encode(&ClearSelection)),
            31 => b.platform(wire::encode_canvas_refill()),
            33 => {
                let press = b.r.below(4) as u64;
                b.platform(wire::encode_menu_deliver(press));
            }
            34 => {
                if b.r.chance(6) {
                    b.step(b'X', |f| _ = f.str(b"Widget stopped"), "stop".to_string());
                } else {
                    let (top, bottom) = (b.r.below(3) as f64 * 8.0, b.r.below(3) as f64 * 4.0);
                    b.step(b'A', |f| _ = f.f64(top).f64(bottom), format!("{top} {bottom}"));
                }
            }
            35 if b.r.chance(5) => {
                let bad = match b.r.below(6) {
                    0 => wire::encode_mirror(),
                    1 => wire::encode_open(2.0),
                    2 => vec![wire::TAG, 2, 1],
                    3 => vec![wire::TAG, 99],
                    4 => vec![7, 1, 2],
                    _ => [wire::encode_canvas_refill(), vec![0]].concat(),
                };
                b.stopped = true;
                b.platform(bad);
            }
            36 => b.script("}{".to_string()),
            37 => {
                let visible = b.r.chance(2);
                b.step(b'V', |f| _ = f.u8(u8::from(visible)), visible.to_string());
            }
            38..=45 if drawing => {
                let mut calls = String::new();
                for _ in 0..4 + b.r.below(30) {
                    let font = if b.r.chance(30) {
                        format!("{}px {}", 10 + b.r.below(4), "f".repeat(520))
                    } else {
                        format!("{}{}px f{}", if b.r.chance(4) { "bold " } else { "" }, 8 + b.r.below(6), b.r.below(120))
                    };
                    calls.push_str(&format!("g.font='{font}';g.fillText('hi',0,10);"));
                }
                b.script(format!("(()=>{{const g=globalThis.__g;if(!g)return 'none';{calls}return g.font}})()"));
            }
            _ => b.frame(true, Paint::IfDirty),
        }
    }
    b.platform(wire::encode_mirror());
    b.run.push(b'E', Fields::new().u32(b.runtime), format!("{}, close", b.name));
}

/// What the oracle counts of the frames both sent alike.
#[derive(Default, Debug)]
struct Tally {
    steps: usize,
    frames: u64,
    bytes: u64,
    kinds: [u64; 11],
    controls: [u64; 7],
}

impl Tally {
    fn add(&mut self, bytes: &[u8]) {
        let u32_at = |i: usize| u64::from(u32::from_le_bytes(bytes[i..i + 4].try_into().expect("four bytes")));
        self.steps += 1;
        self.frames += u32_at(0);
        self.bytes += u64::from_le_bytes(bytes[4..12].try_into().expect("eight bytes"));
        for (i, k) in self.kinds.iter_mut().enumerate() {
            *k += u32_at(12 + 4 * i);
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            *c += u32_at(12 + 44 + 4 * i);
        }
    }
}

/// Little-endian fields of an answer, read in order.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        head
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().expect("four bytes"))
    }
    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take(8).try_into().expect("eight bytes"))
    }
    fn str(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }
}

/// How one arm's serving ended: whether it ended, its reason, its lockdowns, its callbacks (all, and after it
/// returned) and the frames it sent last.
#[derive(Debug, PartialEq)]
struct Ended {
    finished: bool,
    reason: Vec<u8>,
    lockdowns: u64,
    calls: u64,
    late: u64,
    /// Calls of read, write, shutdown, thread_cpu_seconds, steady_seconds, steady_micros, time_zone_offset, random.
    each: [u64; 8],
    frames: Vec<Vec<u8>>,
}

fn ended(r: &mut Reader<'_>) -> Ended {
    let (finished, reason, lockdowns, calls, late) = (r.u8() != 0, r.str(), r.u64(), r.u64(), r.u64());
    let each = std::array::from_fn(|_| r.u64());
    let frames = (0..r.u32()).map(|_| r.str()).collect();
    Ended { finished, reason, lockdowns, calls, late, each, frames }
}

/// Runs the sessions of `docs` through both servings and fails on the first differences.
fn compare(docs: Vec<(String, String)>, seed: u64, steps_n: usize, tapped: bool, what: &str) {
    let limit = std::env::var("TW_HELPER_FFI_SESSIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let mut r = Lcg::new(seed);
    let mut run = Run::default();
    let mut opening = Fields::new();
    opening.u8(u8::from(tapped)).f64(1000.0);
    run.push(b'O', &opening, "the opening".to_string());
    let mut clock = 1000.0;
    let sessions = docs.len().min(limit);
    for (i, (name, html)) in docs.into_iter().take(limit).enumerate() {
        let mut b = Builder { r: &mut r, run: &mut run, runtime: i as u32 + 1, clock, width: 300.0, seq: 0, name, tapped, stopped: false };
        session(&mut b, &html, steps_n);
        clock = b.clock;
    }
    run.push(b'Q', &Fields::new(), "the end".to_string());
    if let Some(path) = std::env::var_os("TW_HELPER_FFI_SAVE") {
        std::fs::write(PathBuf::from(path).join(format!("{seed:x}.in")), &run.input).expect("the input saved");
    }
    let Some(oracle) = oracle() else { return };
    let started = std::time::Instant::now();
    let answers = oracle.run(run.input, run.shown.len());
    let (last, answers) = answers.split_last().expect("the end's answer");
    let mut tally = Tally::default();
    let mut failures = Vec::new();
    for (answer, shown) in answers.iter().zip(&run.shown) {
        let (status, bytes) = answer;
        match status {
            1 => tally.add(bytes),
            _ if failures.len() < 6 => failures.push(format!("{shown}: {}", String::from_utf8_lossy(bytes))),
            _ => failures.push(String::new()),
        }
    }
    let mut end = Reader(&last.1);
    let (cpp, rust) = (ended(&mut end), ended(&mut end));
    assert!(failures.is_empty(), "{what}: {} of {} steps differ:\n{}", failures.len(), tally.steps + failures.len(), failures.join("\n"));
    assert!(cpp.finished && rust.finished, "{what}: a serving did not end: {cpp:?} {rust:?}");
    assert_eq!((&cpp.reason, &rust.reason), (&Vec::new(), &Vec::new()), "{what}: both end as they should");
    assert_eq!((cpp.lockdowns, rust.lockdowns), (1, 1), "{what}: one lockdown each");
    assert_eq!(rust.late, 0, "{what}: callbacks after tw_helper_serve returned");
    for (i, name) in ["read", "write", "shutdown", "thread_cpu_seconds", "steady_seconds", "steady_micros", "time_zone_offset", "random"]
        .iter()
        .enumerate()
    {
        let expected = i != 5 || tapped;
        assert_eq!(rust.each[i] > 0, expected, "{what}: {name} called {} times through the ABI", rust.each[i]);
    }
    assert_eq!(cpp.frames, rust.frames, "{what}: the frames after the last step");
    let k = &tally.kinds;
    eprintln!(
        "C ABI, {what}: {sessions} sessions, {} steps alike in {:.1} s ({} frames, {} bytes: {} platform messages, {} requests, {} validated batches, {} renderer ready, {} rendered, {} nodes, {} deaths, {} probe answers, {} stats); {} callbacks through the ABI ({:?}), none late",
        tally.steps,
        started.elapsed().as_secs_f64(),
        tally.frames,
        tally.bytes,
        k[0],
        k[1],
        k[2],
        k[3],
        k[4],
        k[5],
        k[6],
        k[7],
        k[8],
        rust.calls,
        rust.each
    );
    eprintln!("C ABI, {what}: Content, Loaded, Stopped, Mirrored, Cursor, CanvasRefused, Clicked sent {:?}", tally.controls);
}

#[test]
fn corpus_sessions_alike() {
    let docs = corpus_files(&["corpus"])
        .into_iter()
        .map(|path| {
            let html = String::from_utf8_lossy(&std::fs::read(&path).expect("a corpus file")).into_owned();
            (path.display().to_string(), without_scripts(&html))
        })
        .collect();
    compare(docs, 0x4846_0002, 32, true, "the corpus, tapped");
}

#[test]
fn generated_sessions_alike() {
    let mut r = Lcg::new(0x474c_0001 + 1);
    let docs = (0..800).map(|i| (format!("generated {i}"), generated_html(&mut r))).collect();
    compare(docs, 0x4846_0001, 40, false, "generated documents");
}

const BUILD: &[u8] = b"helper-ffi-oracle";

fn frame(runtime: u32, kind: Kind, fields: &[Field<'_>]) -> Vec<u8> {
    twidget_stream::encode(runtime, kind, MAX_OPENING_BYTES, fields).expect("a frame of its kind's shape")
}

/// The hosting opening Telegram writes: the stream's version, the build, the sandbox and validator bundles whole and
/// the renderer bundle's stamp line.
fn opening() -> Vec<u8> {
    let dir = events::inputs::widget_sandbox().join("packages/desktop/WidgetJS");
    let res = SandboxResources::from_directory(&dir.display().to_string()).expect("the desktop bundles");
    let stamp = res.renderer_bundle.split(|&c| c == b'\n').next().unwrap_or(b"").to_vec();
    let fields =
        [Field::I64(VERSION), Field::Str(BUILD), Field::Str(&res.sandbox_bundle), Field::Str(&res.validator_bundle), Field::Str(&stamp)];
    frame(0, Kind::HostingOpening, &fields)
}

const SMALL: [&str; 4] = [
    "<p id='p'>hello</p>",
    "<style>.a{color:red}</style><div class='a'>a <b>b</b></div><button>go</button>",
    "<canvas width='40' height='20'></canvas><img src='data:image/png;base64,iVBORw0KGgo='>",
    "<details><summary>s</summary><input value='x'></details>",
];

/// What Telegram writes after the opening for a widget or two: their starts, platform traffic, themes, safe areas,
/// probes, visibility, stops and closes; and where the first widget's start and Open end.
fn traffic(r: &mut Lcg) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut started = 0;
    for runtime in 1..=1 + r.below(2) as u32 {
        let config = SandboxConfig { inner_width: *pick(r, &[300.0, 412.5]), ..SandboxConfig::default() };
        let start = SandboxHostedStart { validated_ops: r.chance(2), ..SandboxHostedStart::default() };
        out.extend(frame(runtime, Kind::Open, &[]));
        let html = pick(r, &SMALL).as_bytes();
        let (config, start) = (encode_config(&config), encode_hosted_start(&start));
        out.extend(frame(runtime, Kind::HostedStart, &[Field::Str(html), Field::Str(&config), Field::Str(&start)]));
        out.extend(frame(runtime, Kind::Platform, &[Field::Str(&wire::encode_open(*pick(r, &[1.0, 2.0])))]));
        if started == 0 {
            started = out.len();
        }
        for seq in 1..=2 + r.below(4) as u64 {
            let message = match r.below(5) {
                0 => encode(&Hover { action: 0, x: 10.0, y: 10.0, modifiers: 0 }),
                1 => encode(&Touch { action: 0, x: 12.0, y: 8.0, pointer: 0, primary: true, mouse: true, modifiers: 0, button: 0 }),
                2 => wire::encode_mirror_work(),
                _ => encode(&FrameRequest { seq, tick: true, paint: Paint::IfDirty }),
            };
            out.extend(frame(runtime, Kind::Platform, &[Field::Str(&message)]));
        }
        let theme = WidgetTheme { color_scheme: b"dark".to_vec(), ..WidgetTheme::default() };
        out.extend(frame(runtime, Kind::HostedTheme, &[Field::Str(&encode_theme(&theme))]));
        let area = WidgetContentSafeArea { top: 8.0, ..WidgetContentSafeArea::default() };
        out.extend(frame(runtime, Kind::SafeArea, &[Field::Str(&encode_safe_area(&area))]));
        out.extend(frame(runtime, Kind::Probe, &[Field::I64(7), Field::Str(b"document.body.children.length")]));
        out.extend(frame(runtime, Kind::Visible, &[Field::Bool(r.chance(2))]));
        if r.chance(2) {
            out.extend(frame(runtime, Kind::Stop, &[Field::Str(b"stopped")]));
        }
        out.extend(frame(runtime, Kind::Close, &[]));
    }
    (out, started)
}

/// Peak commit of the oracle's process over the whole fuzz: what both servings, one after the other, held at most.
const PEAK_BOUND: u64 = 512 << 20;

#[test]
fn mutated_streams_end_alike_and_bounded() {
    let Some(oracle) = oracle() else { return };
    let cases = std::env::var("TW_HELPER_FFI_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(240usize);
    let opening = opening();
    let mut r = Lcg::new(0x4846_0003);
    let mut input = Vec::new();
    let mut shown = Vec::new();
    for case in 0..cases {
        let mut head = opening.clone();
        let mut what = format!("case {case}:");
        if r.chance(8) {
            let mut start = head[..64].to_vec();
            what.push_str(&format!(" the opening's head{}", mutate(&mut r, &mut start)));
            head.splice(..64, start);
        }
        // Half the cases keep the first widget's start, so it boots and the mutations reach a live widget.
        let (mut rest, started) = traffic(&mut r);
        if r.chance(2) {
            let mut tail = rest.split_off(started);
            what.push_str(&format!(" the traffic after the start{}", mutate(&mut r, &mut tail)));
            rest.extend(tail);
        } else {
            what.push_str(&format!(" the traffic{}", mutate(&mut r, &mut rest)));
        }
        let tapped = r.chance(2);
        input.extend(Fields::new().u8(u8::from(tapped)).str(&[head, rest].concat()).framed(b'F'));
        shown.push(what);
    }
    let answers = oracle.run(input, cases);
    let (mut refused, mut clean, mut peak) = (0usize, 0usize, 0u64);
    let mut reasons = std::collections::BTreeMap::<String, usize>::new();
    for ((status, bytes), shown) in answers.iter().zip(&shown) {
        assert_eq!(*status, 1, "{shown}");
        let mut a = Reader(bytes);
        let arm = |a: &mut Reader<'_>| (a.u8() != 0, a.str(), a.u32(), a.u64(), a.u64());
        let (cpp, rust) = (arm(&mut a), arm(&mut a));
        peak = a.u64();
        assert!(cpp.0 && rust.0, "{shown}: a serving did not end");
        assert_eq!(String::from_utf8_lossy(&rust.1), String::from_utf8_lossy(&cpp.1), "{shown}: the reasons");
        assert_eq!(rust.4, 0, "{shown}: callbacks after tw_helper_serve returned");
        if rust.1.is_empty() {
            clean += 1;
        } else {
            refused += 1;
        }
        let words: Vec<&str> = std::str::from_utf8(&rust.1).unwrap_or("<not UTF-8>").split(' ').take(3).collect();
        *reasons.entry(words.join(" ")).or_default() += 1;
    }
    assert!(peak < PEAK_BOUND, "the oracle's peak commit {peak} past {PEAK_BOUND}");
    eprintln!(
        "C ABI, mutated streams: {cases} served by both to their end with the same reason ({refused} with one, {clean} at the stream's end), no callback late, the process's peak commit {} MiB",
        peak >> 20
    );
    eprintln!("C ABI, mutated streams' reasons by their first words: {reasons:?}");
}

/// tw_helper_serve with its contract broken: nothing served, no callback called, and why.
#[test]
fn broken_contracts_serve_nothing() {
    let Some(oracle) = oracle() else { return };
    let answers = oracle.run(Fields::new().u8(b'z').framed(b'N'), 1);
    let mut a = Reader(&answers[0].1);
    assert!(a.u8() != 0, "a NULL reason_size returns NULL");
    assert_eq!(a.u64(), 0, "and calls nothing");
    let reasons: Vec<String> = (0..7).map(|_| String::from_utf8_lossy(&a.str()).into_owned()).collect();
    assert_eq!(
        reasons,
        [
            "tw_helper_serve: no stream",
            "tw_helper_serve: no platform",
            "tw_helper_serve: the stream has no read",
            "tw_helper_serve: the stream has no shutdown",
            "tw_helper_serve: the platform has no steady_micros",
            "tw_helper_serve: the platform has no random",
            "tw_helper_serve: the lockdown has no callback",
        ]
    );
    assert_eq!(a.u64(), 0, "no callback of a refused table is called");
}

/// Streams that end or fail, a lockdown that fails, an empty and a huge build, no lockdown: each served alike by
/// both, the Rust one through the ABI.
#[test]
fn failing_streams_lockdowns_and_builds_end_alike() {
    let Some(oracle) = oracle() else { return };
    let answers = oracle.run(Fields::new().u8(b'e').framed(b'N'), 1);
    let mut a = Reader(&answers[0].1);
    let cases = [
        "a stream that ends at once",
        "a stream that fails inside the opening",
        "writes that fail",
        "a lockdown that fails",
        "an empty build",
        "a 64 MiB build",
        "no lockdown",
    ];
    let mut seen = Vec::new();
    for what in cases {
        let mut arm = || {
            let (finished, reason, lockdowns, late) = (a.u8() != 0, a.str(), a.u64(), a.u64());
            let frames: Vec<Vec<u8>> = (0..a.u32()).map(|_| a.str()).collect();
            (finished, reason, lockdowns, late, frames)
        };
        let (cpp, rust) = (arm(), arm());
        assert!(cpp.0 && rust.0, "{what}: a serving did not end");
        assert_eq!(rust, cpp, "{what}");
        seen.push((what, String::from_utf8_lossy(&rust.1).into_owned(), rust.2, rust.4.len()));
    }
    let reason = |i: usize| seen[i].1.as_str();
    assert_eq!((reason(0), seen[0].2, seen[0].3), ("", 0, 0), "{:?}", seen[0]);
    assert!(!reason(1).is_empty() && seen[1].2 == 0, "{:?}", seen[1]);
    assert_eq!((reason(3), seen[3].2), ("the oracle's lockdown failed", 1), "{:?}", seen[3]);
    assert!(reason(4).starts_with("build \"helper-ffi-oracle\", this end's is \"\""), "{:?}", seen[4]);
    assert!(reason(5).starts_with("build \"helper-ffi-oracle\", this end's is \"bbbb") && reason(5).len() < 200, "{:?}", seen[5]);
    assert_eq!((reason(6), seen[6].2, seen[6].3), ("", 0, 1), "{:?}", seen[6]);
    for (what, reason, lockdowns, frames) in &seen {
        eprintln!("C ABI, {what}: {lockdowns} lockdowns, {frames} frames, ended with {reason:?}");
    }
}

/// The stream ends while widgets boot, paint and are tapped: no callback once tw_helper_serve returned.
#[test]
fn no_callback_after_serving_returned() {
    let Some(oracle) = oracle() else { return };
    let mut r = Lcg::new(0x4846_0004);
    let mut input = Vec::new();
    let rounds = 12;
    for round in 0..rounds {
        let count = 1 + round % 6;
        let mut f = Fields::new();
        f.u8(b'l').u32(count as u32).u8(u8::from(round % 2 == 0));
        for _ in 0..count {
            f.str(pick(&mut r, &SMALL).as_bytes());
            for seq in 0..8u64 {
                let message = match seq {
                    0 => wire::encode_open(2.0),
                    1 => wire::encode_mirror_work(),
                    _ => encode(&FrameRequest { seq, tick: true, paint: Paint::Force }),
                };
                f.str(&message);
            }
        }
        input.extend(f.framed(b'N'));
    }
    let answers = oracle.run(input, rounds);
    let mut calls = 0;
    for (round, (_, bytes)) in answers.iter().enumerate() {
        let mut a = Reader(bytes);
        let (finished, reason, n, late) = (a.u8() != 0, a.str(), a.u64(), a.u64());
        assert!(finished, "round {round}: the serving did not end");
        assert_eq!(late, 0, "round {round}: callbacks after tw_helper_serve returned ({reason:?})");
        calls += n;
    }
    eprintln!("C ABI, streams ended under load: {rounds} servings, {calls} callbacks, none after tw_helper_serve returned");
}
