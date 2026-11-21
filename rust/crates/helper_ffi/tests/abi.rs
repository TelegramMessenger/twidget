//! The C ABI called as a C caller would, with callbacks of the test's own: what it serves with an opening and no widget
//! (no engine runs, so these hold in a debug build too), what it refuses, its reason handed over and freed, the
//! guard's promise that no callback runs after it returned, and a panic aborting the process rather than unwinding
//! into the caller. The comparison with the C++ serving and its negative cases from C++ are `oracle.rs`.

use std::ffi::c_void;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use twidget_helper_ffi::{TwHelperLockdown, TwHelperPlatform, TwHelperStream, tw_helper_free, tw_helper_serve};
use twidget_stream::{Field, Kind, MAX_OPENING_BYTES, VERSION, encode};

/// The test's side of the stream and the platform: the bytes the helper reads, what it wrote, and every call counted,
/// those after tw_helper_serve returned apart.
#[derive(Default)]
struct Glue {
    input: Mutex<Vec<u8>>,
    output: Mutex<Vec<u8>>,
    fail_writes: bool,
    lockdown: Vec<u8>,
    lockdowns: AtomicUsize,
    calls: AtomicUsize,
    late: AtomicUsize,
    returned: AtomicBool,
    panic_in_read: bool,
}

impl Glue {
    fn called(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.returned.load(Ordering::SeqCst) {
            self.late.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn glue<'a>(context: *mut c_void) -> &'a Glue {
    // SAFETY: every table below carries a `&Glue` that outlives the serving.
    unsafe { &*(context as *const Glue) }
}

unsafe extern "C" fn read(context: *mut c_void, out: *mut u8, size: usize) -> usize {
    let g = glue(context);
    g.called();
    if g.panic_in_read {
        panic!("a callback of the test's panics");
    }
    let mut input = g.input.lock().unwrap();
    let n = size.min(input.len());
    // SAFETY: the ABI's promise: `out` is writable for `size` bytes.
    unsafe { std::ptr::copy_nonoverlapping(input.as_ptr(), out, n) };
    input.drain(..n);
    n
}

unsafe extern "C" fn write(context: *mut c_void, bytes: *const u8, size: usize) -> bool {
    let g = glue(context);
    g.called();
    if g.fail_writes {
        return false;
    }
    // SAFETY: the ABI's promise: `bytes` is readable for `size` bytes.
    g.output.lock().unwrap().extend_from_slice(unsafe { std::slice::from_raw_parts(bytes, size) });
    true
}

unsafe extern "C" fn shutdown(context: *mut c_void) {
    glue(context).called();
}

unsafe extern "C" fn seconds(context: *mut c_void) -> f64 {
    glue(context).called();
    1000.0
}

unsafe extern "C" fn micros(context: *mut c_void) -> i64 {
    glue(context).called();
    1_000_000_000
}

unsafe extern "C" fn zone(context: *mut c_void, _epoch: i64) -> i32 {
    glue(context).called();
    0
}

unsafe extern "C" fn random(context: *mut c_void, out: *mut u8, size: usize) {
    glue(context).called();
    // SAFETY: the ABI's promise: `out` is writable for `size` bytes.
    unsafe { std::ptr::write_bytes(out, 4, size) };
}

unsafe extern "C" fn lockdown(context: *mut c_void, size: *mut usize) -> *const u8 {
    let g = glue(context);
    g.lockdowns.fetch_add(1, Ordering::SeqCst);
    // SAFETY: the ABI's promise: `size` is writable.
    unsafe { *size = g.lockdown.len() };
    g.lockdown.as_ptr()
}

fn tables(g: &Glue) -> (TwHelperStream, TwHelperPlatform, TwHelperLockdown) {
    let context = g as *const Glue as *mut c_void;
    (
        TwHelperStream { context, read: Some(read), write: Some(write), shutdown: Some(shutdown) },
        TwHelperPlatform {
            context,
            thread_cpu_seconds: Some(seconds),
            steady_seconds: Some(seconds),
            steady_micros: Some(micros),
            time_zone_offset: Some(zone),
            random: Some(random),
        },
        TwHelperLockdown { context, lockdown: Some(lockdown) },
    )
}

const BUILD: &[u8] = b"abi-tests";

/// A hosting opening of `build` whose bundles are stamp lines alone: accepted, and no widget is ever started.
fn opening(build: &[u8]) -> Vec<u8> {
    let fields = [
        Field::I64(VERSION),
        Field::Str(build),
        Field::Str(b"/* sandbox */"),
        Field::Str(b"/* validator */"),
        Field::Str(b"/* renderer */"),
    ];
    encode(0, Kind::HostingOpening, MAX_OPENING_BYTES, &fields).expect("an opening")
}

/// Serves `g`'s input with its tables (the lockdown's when `lock`) and returns the reason as tw_helper_serve handed it
/// over, freed through tw_helper_free.
fn serve(g: &Glue, build: &[u8], lock: bool, tapped: bool) -> Option<Vec<u8>> {
    let (stream, platform, lockdown) = tables(g);
    let mut size = 0usize;
    let lockdown: *const TwHelperLockdown = if lock { &lockdown } else { std::ptr::null() };
    // SAFETY: the tables, the build and the size live through the call, and the callbacks keep the header's contract.
    let reason = unsafe { tw_helper_serve(&stream, &platform, build.as_ptr(), build.len(), lockdown, tapped, &mut size) };
    g.returned.store(true, Ordering::SeqCst);
    assert_eq!(reason.is_null(), size == 0, "NULL exactly for no reason");
    if reason.is_null() {
        return None;
    }
    // SAFETY: what tw_helper_serve returned, `size` bytes, read before it is freed once below.
    let bytes = unsafe { std::slice::from_raw_parts(reason, size) }.to_vec();
    // SAFETY: as tw_helper_serve returned it.
    unsafe { tw_helper_free(reason, size) };
    Some(bytes)
}

/// The frames the helper wrote: kind and first string field.
fn frames(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while rest.len() >= 9 {
        let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let frame = &rest[4..4 + size];
        let text = if frame.len() >= 9 && frame[4] == Kind::Refused as u8 {
            let n = u32::from_le_bytes(frame[5..9].try_into().unwrap()) as usize;
            frame[9..9 + n].to_vec()
        } else {
            Vec::new()
        };
        out.push((frame[4], text));
        rest = &rest[4 + size..];
    }
    out
}

#[test]
fn an_opening_is_answered_ready_with_its_lockdown_or_without_one() {
    for (lock, tapped) in [(true, false), (false, false), (true, true), (false, true)] {
        let g = Glue { input: Mutex::new(opening(BUILD)), ..Glue::default() };
        assert_eq!(serve(&g, BUILD, lock, tapped), None, "the stream's end");
        assert_eq!(g.lockdowns.load(Ordering::SeqCst), usize::from(lock));
        assert_eq!(frames(&g.output.lock().unwrap()), [(Kind::Ready as u8, Vec::new())], "lock {lock}");
        assert_eq!(g.late.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn a_failed_lockdown_is_the_reason_and_the_refusal() {
    let g = Glue { input: Mutex::new(opening(BUILD)), lockdown: b"no seatbelt".to_vec(), ..Glue::default() };
    assert_eq!(serve(&g, BUILD, true, false).as_deref(), Some(&b"no seatbelt"[..]));
    assert_eq!(frames(&g.output.lock().unwrap()), [(Kind::Refused as u8, b"no seatbelt".to_vec())]);
}

#[test]
fn builds_empty_and_huge_are_refused_by_their_quote() {
    let g = Glue { input: Mutex::new(opening(BUILD)), ..Glue::default() };
    assert_eq!(serve(&g, &[], true, false).as_deref(), Some(&b"build \"abi-tests\", this end's is \"\""[..]));
    let huge = vec![b'b'; 64 << 20];
    let g = Glue { input: Mutex::new(opening(BUILD)), ..Glue::default() };
    let reason = serve(&g, &huge, true, false).expect("a refusal");
    assert!(reason.starts_with(b"build \"abi-tests\", this end's is \"bbbb") && reason.ends_with(b"...\"") && reason.len() < 200);
    assert_eq!(g.lockdowns.load(Ordering::SeqCst), 0, "no lockdown for a refused opening");
}

#[test]
fn streams_that_end_or_fail_end_the_serving() {
    let g = Glue::default();
    assert_eq!(serve(&g, BUILD, true, false), None, "an empty stream ends as it should");
    assert!(g.output.lock().unwrap().is_empty());
    let mut cut = opening(BUILD);
    cut.truncate(20);
    let g = Glue { input: Mutex::new(cut), ..Glue::default() };
    assert_eq!(serve(&g, BUILD, true, false).as_deref(), Some(&b"the stream ended inside a frame"[..]));
    let g = Glue { input: Mutex::new(opening(BUILD)), fail_writes: true, ..Glue::default() };
    assert_eq!(serve(&g, BUILD, true, false), None, "a stream whose writes fail still reads to its end");
}

#[test]
fn null_arguments_and_tables_serve_nothing() {
    let g = Glue { input: Mutex::new(opening(BUILD)), ..Glue::default() };
    let (stream, platform, lockdown) = tables(&g);
    // SAFETY: a NULL reason_size and NULL tables are within the header's contract: nothing is served.
    unsafe {
        assert!(tw_helper_serve(&stream, &platform, BUILD.as_ptr(), BUILD.len(), &lockdown, false, std::ptr::null_mut()).is_null());
        let mut size = 5usize;
        let reason = tw_helper_serve(std::ptr::null(), &platform, std::ptr::null(), 0, std::ptr::null(), false, &mut size);
        assert_eq!(std::slice::from_raw_parts(reason, size), b"tw_helper_serve: no stream");
        tw_helper_free(reason, size);
        let blind = TwHelperPlatform { time_zone_offset: None, ..platform };
        let reason = tw_helper_serve(&stream, &blind, std::ptr::null(), 0, std::ptr::null(), false, &mut size);
        assert_eq!(std::slice::from_raw_parts(reason, size), b"tw_helper_serve: the platform has no time_zone_offset");
        tw_helper_free(reason, size);
        let mute = TwHelperStream { write: None, ..stream };
        let reason = tw_helper_serve(&mute, &platform, std::ptr::null(), 0, std::ptr::null(), false, &mut size);
        assert_eq!(std::slice::from_raw_parts(reason, size), b"tw_helper_serve: the stream has no write");
        tw_helper_free(reason, size);
        tw_helper_free(std::ptr::null_mut(), 0);
        tw_helper_free(std::ptr::null_mut(), 9);
    }
    assert_eq!(g.calls.load(Ordering::SeqCst), 0, "no callback of a refused call");
}

/// Re-runs this test binary on one test with `TW_ABI_DEATH` set: how it ended and what it wrote to stderr.
fn child(test: &str) -> (std::process::ExitStatus, String) {
    let out = Command::new(std::env::current_exe().expect("the test binary"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env("TW_ABI_DEATH", "1")
        .output()
        .expect("the child runs");
    (out.status, String::from_utf8_lossy(&out.stderr).into_owned())
}

/// A callback of the Rust side's own that panics: the panic never unwinds through tw_helper_serve into its C caller
/// (here this test), the process aborts.
#[test]
fn a_panicking_callback_aborts_the_process() {
    if std::env::var_os("TW_ABI_DEATH").is_some() {
        let g = Glue { input: Mutex::new(opening(BUILD)), panic_in_read: true, ..Glue::default() };
        let _ = serve(&g, BUILD, true, false);
        eprintln!("tw_helper_serve returned after a panic");
        return;
    }
    let (status, stderr) = child("a_panicking_callback_aborts_the_process");
    assert!(!status.success() && status.code() != Some(101), "the process aborts rather than failing a test: {status:?}\n{stderr}");
    assert!(stderr.contains("a callback of the test's panics"), "{stderr}");
    assert!(!stderr.contains("tw_helper_serve returned"), "{stderr}");
}
