//! The C ABI between the widget helper's C++ OS glue and its serving in Rust (`include/twidget_helper.h`, rust/PLAN.md
//! stage H2): the glue keeps the entry point, the channel it adopted, the lockdown and the OS's answers, and hands
//! [`tw_helper_serve`] the stream as read, write and shutdown callbacks and the OS's answers as a platform table;
//! everything past them is [`twidget_helper::serve`], which is safe. Only byte buffers and numbers cross: Rust never
//! touches a descriptor or a handle, and the glue's context pointers are passed back to it as they came.
//!
//! The one crate of the helper with `unsafe` (tools/unsafe_report.py): the types of the glue's callbacks, the calls to
//! them, its tables and buffers read through raw pointers, the reason handed over and taken back, and the promise that
//! its callbacks may be called from any thread. Each block and impl has its SAFETY note; AUDIT.md lists them with the
//! contract that makes each sound and the tests that run it. Every call into the glue holds a guard that serving
//! closes before it returns, so none reaches the glue later, and a panic under an entry aborts the process, as one on
//! the helper's Rust threads does.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

#[cfg(feature = "nova-c-api")]
pub use nova_embed_ffi;
use twidget_helper::SteadyMicros;
use twidget_js::Platform;
use twidget_stream::Stream;

/// The helper's allocator: nova-safe's metering allocator, on which `twidget_js` counts each widget engine's bytes
/// for its memory limit, as nova_embed_ffi's counts the C++ helper's Nova. Its `unsafe` is nova_meter's (audited in
/// nova-safe). With `nova-c-api` nova_embed_ffi installs the same allocator, which then serves both servings.
#[cfg(not(feature = "nova-c-api"))]
#[global_allocator]
static METERED: nova_meter::Metered = nova_meter::Metered;

/// `read(context, out, size)`: blocks until at least one byte came, copies at most `size` to `out`, returns how many.
pub type ReadFn = unsafe extern "C" fn(context: *mut c_void, out: *mut u8, size: usize) -> usize;
/// `write(context, bytes, size)`: blocks until all `size` bytes at `bytes` are written; false once the stream broke.
pub type WriteFn = unsafe extern "C" fn(context: *mut c_void, bytes: *const u8, size: usize) -> bool;
/// `shutdown(context)`: wakes a blocked read and write without waiting; every later one fails.
pub type ShutdownFn = unsafe extern "C" fn(context: *mut c_void);
/// A clock in seconds: the calling thread's CPU time, or the steady clock's.
pub type SecondsFn = unsafe extern "C" fn(context: *mut c_void) -> f64;
/// The steady clock's whole microseconds.
pub type MicrosFn = unsafe extern "C" fn(context: *mut c_void) -> i64;
/// Seconds east of UTC of the user's time zone at `epoch_seconds`.
pub type TimeZoneFn = unsafe extern "C" fn(context: *mut c_void, epoch_seconds: i64) -> i32;
/// `size` bytes of the system's CSPRNG at `out`.
pub type RandomFn = unsafe extern "C" fn(context: *mut c_void, out: *mut u8, size: usize);
/// The lockdown: why it failed, `*size` bytes, or `*size` 0.
pub type LockdownFn = unsafe extern "C" fn(context: *mut c_void, size: *mut usize) -> *const u8;

/// The stream the glue adopted (`tw_helper_stream`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TwHelperStream {
    pub context: *mut c_void,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    pub shutdown: Option<ShutdownFn>,
}

/// What only the OS answers (`tw_helper_platform`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TwHelperPlatform {
    pub context: *mut c_void,
    pub thread_cpu_seconds: Option<SecondsFn>,
    pub steady_seconds: Option<SecondsFn>,
    pub steady_micros: Option<MicrosFn>,
    pub time_zone_offset: Option<TimeZoneFn>,
    pub random: Option<RandomFn>,
}

/// The lockdown run once the opening was accepted (`tw_helper_lockdown`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TwHelperLockdown {
    pub context: *mut c_void,
    pub lockdown: Option<LockdownFn>,
}

/// Calls reach the glue only while serving: `close` waits for the calls running and refuses every later one. Entering
/// never waits, so a call made while others run (a write while a read blocks, a clock read on every thread) goes
/// straight through, and the lock is never held across a call into the glue.
struct Guard {
    state: Mutex<GuardState>,
    idle: Condvar,
}

struct GuardState {
    open: bool,
    running: usize,
}

impl Guard {
    fn new() -> Guard {
        Guard { state: Mutex::new(GuardState { open: true, running: 0 }), idle: Condvar::new() }
    }

    /// `call`'s result while serving, `closed` without calling it after `close`.
    fn run<R>(&self, closed: R, call: impl FnOnce() -> R) -> R {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if !state.open {
                return closed;
            }
            state.running += 1;
        }
        let result = call();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.running -= 1;
        if state.running == 0 {
            self.idle.notify_all();
        }
        result
    }

    /// Refuses every later call, then waits for the running ones to return.
    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.open = false;
        while state.running != 0 {
            state = self.idle.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The glue's callbacks, every one there, read once from its tables and kept for one serving behind its guard.
struct Glue {
    stream: *mut c_void,
    read: ReadFn,
    write: WriteFn,
    shutdown: ShutdownFn,
    platform: *mut c_void,
    thread_cpu_seconds: SecondsFn,
    steady_seconds: SecondsFn,
    steady_micros: MicrosFn,
    time_zone_offset: TimeZoneFn,
    random: RandomFn,
    guard: Guard,
}

// SAFETY: twidget_helper.h's contract: every callback of the stream and of the platform may be called with its context
// from any thread, several at once, until tw_helper_serve returns; the guard refuses every call after it closed, which
// tw_helper_serve does before it returns.
unsafe impl Send for Glue {}
// SAFETY: as for Send: the glue's callbacks are made for calls from several threads at once, and Glue holds nothing
// else but its guard, which is Sync.
unsafe impl Sync for Glue {}

impl Glue {
    /// The callbacks of `stream` and `platform`, or why serving cannot start: a table or a callback missing.
    fn new(stream: Option<TwHelperStream>, platform: Option<TwHelperPlatform>) -> Result<Glue, &'static str> {
        let s = stream.ok_or("tw_helper_serve: no stream")?;
        let p = platform.ok_or("tw_helper_serve: no platform")?;
        Ok(Glue {
            stream: s.context,
            read: s.read.ok_or("tw_helper_serve: the stream has no read")?,
            write: s.write.ok_or("tw_helper_serve: the stream has no write")?,
            shutdown: s.shutdown.ok_or("tw_helper_serve: the stream has no shutdown")?,
            platform: p.context,
            thread_cpu_seconds: p.thread_cpu_seconds.ok_or("tw_helper_serve: the platform has no thread_cpu_seconds")?,
            steady_seconds: p.steady_seconds.ok_or("tw_helper_serve: the platform has no steady_seconds")?,
            steady_micros: p.steady_micros.ok_or("tw_helper_serve: the platform has no steady_micros")?,
            time_zone_offset: p.time_zone_offset.ok_or("tw_helper_serve: the platform has no time_zone_offset")?,
            random: p.random.ok_or("tw_helper_serve: the platform has no random")?,
            guard: Guard::new(),
        })
    }

    fn micros(&self) -> i64 {
        // SAFETY: the glue's steady_micros with its context, while serving (the guard).
        self.guard.run(0, || unsafe { (self.steady_micros)(self.platform) })
    }

    /// `twidget_helper::serve` on the glue's stream and platform, the tap on its steady micros when `tapped`; once it
    /// returned no callback runs.
    fn serve(self: Arc<Glue>, build: &[u8], lockdown: Option<&dyn Fn() -> Vec<u8>>, tapped: bool) -> Vec<u8> {
        let stream: Arc<dyn Stream> = Arc::new(CStream(Arc::clone(&self)));
        let platform: Arc<dyn Platform> = Arc::new(CPlatform(Arc::clone(&self)));
        let tap = tapped.then(|| {
            let glue = Arc::clone(&self);
            Arc::new(move || glue.micros()) as SteadyMicros
        });
        let reason = twidget_helper::serve(stream, build, lockdown, platform, tap);
        self.guard.close();
        reason
    }
}

struct CStream(Arc<Glue>);

impl Stream for CStream {
    fn read(&self, out: &mut [u8]) -> usize {
        let g = &*self.0;
        // SAFETY: the glue's read with its context, while serving (the guard); `out` is writable for its length, of
        // which the glue's read copies at most `size` bytes, and a count past it is not believed.
        g.guard.run(0, || unsafe { (g.read)(g.stream, out.as_mut_ptr(), out.len()) }).min(out.len())
    }

    fn write(&self, bytes: &[u8]) -> bool {
        let g = &*self.0;
        // SAFETY: the glue's write with its context, while serving; `bytes` is readable for its length for the call.
        g.guard.run(false, || unsafe { (g.write)(g.stream, bytes.as_ptr(), bytes.len()) })
    }

    fn shutdown(&self) {
        let g = &*self.0;
        // SAFETY: the glue's shutdown with its context, while serving.
        g.guard.run((), || unsafe { (g.shutdown)(g.stream) });
    }
}

struct CPlatform(Arc<Glue>);

impl Platform for CPlatform {
    fn thread_cpu_seconds(&self) -> f64 {
        let g = &*self.0;
        // SAFETY: the glue's thread_cpu_seconds with its context, while serving.
        g.guard.run(0.0, || unsafe { (g.thread_cpu_seconds)(g.platform) })
    }

    fn steady_seconds(&self) -> f64 {
        let g = &*self.0;
        // SAFETY: the glue's steady_seconds with its context, while serving.
        g.guard.run(0.0, || unsafe { (g.steady_seconds)(g.platform) })
    }

    fn time_zone_offset(&self, epoch_seconds: i64) -> i32 {
        let g = &*self.0;
        // SAFETY: the glue's time_zone_offset with its context, while serving.
        g.guard.run(0, || unsafe { (g.time_zone_offset)(g.platform, epoch_seconds) })
    }

    fn random(&self, bytes: &mut [u8]) {
        let g = &*self.0;
        let called = g.guard.run(false, || {
            // SAFETY: the glue's random with its context, while serving; `bytes` is writable for its length, which is
            // what the glue's random fills.
            unsafe { (g.random)(g.platform, bytes.as_mut_ptr(), bytes.len()) };
            true
        });
        // A caller after serving returned gets zeros: no callback runs then.
        if !called {
            bytes.fill(0);
        }
    }
}

/// `data` as a slice, empty for NULL and for a size no buffer can have.
///
/// # Safety
/// `data` is NULL or readable for `size` bytes for `'a`.
unsafe fn borrowed<'a>(data: *const u8, size: usize) -> &'a [u8] {
    if data.is_null() || size == 0 || size > isize::MAX as usize {
        return &[];
    }
    // SAFETY: the caller's promise, `size` within isize.
    unsafe { std::slice::from_raw_parts(data, size) }
}

/// `bytes` handed to the C caller, `*size` bytes it frees with `tw_helper_free`; NULL with 0 for none.
fn hand_over(bytes: Vec<u8>, size: &mut usize) -> *mut u8 {
    *size = bytes.len();
    if bytes.is_empty() {
        return std::ptr::null_mut();
    }
    Box::into_raw(bytes.into_boxed_slice()).cast::<u8>()
}

/// An entry's body: a panic under it aborts the process, as one on the helper's Rust threads does (twidget_js's
/// `JsThread`), and never unwinds into the C caller.
fn entry<R>(body: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(_) => std::process::abort(),
    }
}

/// `WidgetHelper::Serve`: serves the glue's stream until it ends, on the calling thread, and returns why it ended,
/// `*reason_size` bytes to free with [`tw_helper_free`], or NULL and 0 when it ended as it should. `lockdown` runs once
/// the opening was accepted (NULL: no lockdown); `tapped` serves every widget through the Debug tap on the platform's
/// steady micros. A NULL `stream` or `platform`, or a NULL callback in a table, serves nothing and says so; a NULL
/// `reason_size` serves nothing and returns NULL. Every widget is destroyed and no callback runs once it returned.
///
/// # Safety
/// `stream`, `platform` and `lockdown` are NULL or point to their tables, readable during the call; each callback of
/// `stream` and `platform` may be called with its context from any thread, several at once, until this returns, and
/// keeps twidget_helper.h's contract (`read` copies at most `size` bytes to `out`, `random` fills `size`, none of them
/// unwinds); the lockdown's text is readable for `*size` bytes until this returns; `build` is NULL or readable for
/// `build_size` bytes during the call; `reason_size` is NULL or points to a writable `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tw_helper_serve(
    stream: *const TwHelperStream,
    platform: *const TwHelperPlatform,
    build: *const u8,
    build_size: usize,
    lockdown: *const TwHelperLockdown,
    tapped: bool,
    reason_size: *mut usize,
) -> *mut u8 {
    entry(|| {
        // SAFETY: the caller's promise: NULL or a writable size_t, borrowed for this call alone.
        let Some(reason_size) = (unsafe { reason_size.as_mut() }) else { return std::ptr::null_mut() };
        // SAFETY: the caller's promise: each table NULL or readable during the call; each is read once and copied.
        let (stream, platform, lockdown) = unsafe { (stream.as_ref().copied(), platform.as_ref().copied(), lockdown.as_ref().copied()) };
        let reason = match (Glue::new(stream, platform), lockdown.map(|l| l.lockdown.map(|call| (l.context, call)))) {
            (Err(why), _) => why.as_bytes().to_vec(),
            (Ok(_), Some(None)) => b"tw_helper_serve: the lockdown has no callback".to_vec(),
            (Ok(glue), lockdown) => {
                let lock = lockdown.flatten().map(|(context, call)| {
                    move || {
                        let mut size = 0usize;
                        // SAFETY: the glue's lockdown with its context, on this thread during tw_helper_serve; its text
                        // is readable for `size` bytes until serving returns, and is copied at once.
                        unsafe { borrowed(call(context, &mut size), size).to_vec() }
                    }
                });
                // SAFETY: the caller's promise: `build` NULL or readable for `build_size` bytes during the call.
                let build = unsafe { borrowed(build, build_size) };
                Arc::new(glue).serve(build, lock.as_ref().map(|f| f as &dyn Fn() -> Vec<u8>), tapped)
            }
        };
        hand_over(reason, reason_size)
    })
}

/// Frees what [`tw_helper_serve`] returned; NULL does nothing.
///
/// # Safety
/// `bytes` is NULL, or a reason `tw_helper_serve` returned with `size` its `*reason_size`, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tw_helper_free(bytes: *mut u8, size: usize) {
    entry(|| {
        if bytes.is_null() {
            return;
        }
        // SAFETY: the caller's promise: the boxed slice `hand_over` leaked, of that length, freed once.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes, size)) });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_closed_guard_refuses_and_waits_for_the_calls_running() {
        let guard = Arc::new(Guard::new());
        let entered = Arc::new(AtomicUsize::new(0));
        let (inside, leave) = (Arc::new(Mutex::new(())), Arc::new(AtomicUsize::new(0)));
        let held = inside.lock().unwrap();
        let running = {
            let (guard, entered, inside, leave) = (Arc::clone(&guard), Arc::clone(&entered), Arc::clone(&inside), Arc::clone(&leave));
            std::thread::spawn(move || {
                guard.run(0, || {
                    entered.fetch_add(1, Ordering::SeqCst);
                    // A call made inside a running one goes through: entering never waits.
                    assert_eq!(guard.run(0, || 2), 2);
                    drop(inside.lock().unwrap());
                    leave.fetch_add(1, Ordering::SeqCst);
                    1
                })
            })
        };
        while entered.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let closing = {
            let guard = Arc::clone(&guard);
            std::thread::spawn(move || guard.close())
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!closing.is_finished(), "close waits for the call running");
        assert_eq!(guard.run(7, || 8), 7, "a call after close began is refused");
        drop(held);
        assert_eq!(running.join().unwrap(), 1);
        closing.join().unwrap();
        assert_eq!(leave.load(Ordering::SeqCst), 1);
        assert_eq!(guard.run(7, || 8), 7);
    }

    #[test]
    fn sizes_no_buffer_has_are_empty() {
        let byte = 1u8;
        // SAFETY: NULL, and a real byte read with a size past isize, which `borrowed` refuses before reading.
        unsafe {
            assert!(borrowed(std::ptr::null(), 4).is_empty());
            assert!(borrowed(&byte, usize::MAX).is_empty());
            assert_eq!(borrowed(&byte, 1), &[1]);
        }
    }
}
