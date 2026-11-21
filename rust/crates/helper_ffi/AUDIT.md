# `twidget_helper_ffi`: the audit

The one crate of the widget helper with `unsafe` (rust/PLAN.md, "The one `unsafe` layer"; `tools/unsafe_report.py`'s
`ALLOWED`). It is the C ABI the C++ OS glue calls instead of `_other/widget_helper_renderer.cpp`'s `WidgetHelper::Serve`:
`tw_helper_serve` and `tw_helper_free` in `include/twidget_helper.h`. Everything under it is `twidget_helper::serve` and
the crates it links, all `#![forbid(unsafe_code)]`.

What crosses: byte buffers (the stream's bytes both ways, the build, the lockdown's text, the reason, the CSPRNG's bytes)
and numbers (sizes, clocks, a time-zone offset, a bool). The glue's two context pointers cross back to it unread. Rust
never touches a descriptor, a handle, a C++ object or a C string.

## The `unsafe` in `src/lib.rs`

`unsafe_report.py` counts 30 `unsafe` keywords in the crate's sources: 29 in the ABI and 1 in its unit test. Every block
and impl has a `// SAFETY:` note (`clippy::undocumented_unsafe_blocks` is `deny` for the crate), every `unsafe fn` a
`# Safety` section (`clippy::missing_safety_doc`), and `unsafe_op_in_unsafe_fn` is `deny`.

| # | Where | What | Invariant | Guaranteed by | Exercised by |
|---|---|---|---|---|---|
| 1–8 | `ReadFn` … `LockdownFn` | the types of the glue's callbacks, `unsafe extern "C" fn` | a call is sound only under the header's contract, so each call site is an `unsafe` block of its own (12–19) | the declarations, matching twidget_helper.h field for field (`#[repr(C)]` tables) | every test through the ABI |
| 9, 10 | `unsafe impl Send`, `Sync for Glue` | the glue's callbacks and contexts used from the helper's threads | every callback may be called with its context from any thread, several at once, until `tw_helper_serve` returns; never after (the guard) | the C caller: twidget_helper.h, "Every callback below … may be called from any thread and several at once … until tw_helper_serve returns" | `oracle.rs`: writes from the glue, engine and probe threads, clocks and randomness from the JS threads, the tap's clock on the glue thread |
| 11 | `Glue::micros` | `steady_micros(context)` | a valid callback and context, while serving | the C caller (the platform table), the guard | `corpus_sessions_alike` (tapped: the tap's work records carry its micros) |
| 12 | `CStream::read` | `read(context, out, size)` with Rust's buffer | `out` writable for `size`; the callback copies at most `size` bytes; a count past `size` is clamped and never believed | Rust (the buffer), the C caller (at most `size`) | every serving: `abi.rs`, `oracle.rs` |
| 13 | `CStream::write` | `write(context, bytes, size)` | `bytes` readable for `size` during the call; the callback does not keep the pointer | Rust (the slice), the C caller | every serving that writes a frame |
| 14 | `CStream::shutdown` | `shutdown(context)` | a valid callback, from any thread, also while a read or write blocks | the C caller | every serving's end; `failing_streams_…` ("writes that fail") |
| 15–17 | `CPlatform::thread_cpu_seconds`, `steady_seconds`, `time_zone_offset` | the clocks and the time zone | valid callbacks, from any thread | the C caller | `oracle.rs` sessions (every engine task reads the CPU clock and the steady clock; the sessions' date probe the time zone) |
| 18 | `CPlatform::random` | `random(context, out, size)` with Rust's buffer | `out` writable for `size`; the callback fills at most `size` (fewer leaves Rust's zeros, never uninitialised memory) | Rust (the buffer), the C caller | `oracle.rs` sessions (the date probe's `crypto.getRandomValues`) |
| 19 | `borrowed` (an `unsafe fn`) | | `data` NULL or readable for `size` bytes for `'a` | its two callers (21, 22), each with the C caller's promise | |
| 20 | `borrowed`'s body | `slice::from_raw_parts(data, size)` | non-NULL, `size` ≠ 0 and ≤ `isize::MAX` (checked first, else empty), readable (the caller's promise) | `borrowed`'s checks and its `# Safety` | `sizes_no_buffer_has_are_empty` (unit), every serving |
| 21 | `tw_helper_serve`'s lockdown closure | `lockdown(context, &size)`, then its text borrowed and copied at once | the text readable for `*size` bytes until `tw_helper_serve` returns; called once, on the serving thread | the C caller (the lockdown table) | `a_failed_lockdown_is_the_reason_and_the_refusal` (`abi.rs`), `failing_streams_…` (`oracle.rs`) |
| 22 | `tw_helper_serve` | `build` borrowed for the call | NULL or readable for `build_size` bytes during the call | the C caller | every serving; empty and 64 MiB builds in `builds_empty_and_huge_…` and `failing_streams_…` |
| 23 | `tw_helper_serve` | `reason_size.as_mut()` | NULL (then nothing is served) or a valid, aligned, writable `size_t` | the C caller | every serving; NULL in `null_arguments_…` and `broken_contracts_…` |
| 24 | `tw_helper_serve` | the three tables read through `as_ref().copied()` | each NULL (refused with a reason) or a valid table, read once, at entry | the C caller | every serving; each NULL table and NULL callback in `null_arguments_…`, `broken_contracts_…` |
| 25 | `tw_helper_free` | `Box::from_raw(slice_from_raw_parts_mut(bytes, size))` | `bytes` NULL (nothing) or exactly a reason `tw_helper_serve` returned with its `*reason_size`, freed once | the C caller | every reason freed (`abi.rs`'s `serve`, the oracle's `Taken`); NULL in `null_arguments_…`, `broken_contracts_…` |
| 26–29 | `#[unsafe(no_mangle)]` and `pub unsafe extern "C" fn` on both entries | the two exported symbols | names unique to the helper (`tw_helper_`), callable only under their `# Safety` | the header | every test through the ABI |
| 30 | the unit test `sizes_no_buffer_has_are_empty` | `borrowed` on NULL and a size past `isize::MAX` | refused before any read | the test | itself |

The integration tests (`tests/abi.rs`, the C caller written in Rust) have `unsafe` of their own, as a C caller would: the
callbacks they hand over and their calls of the two entries; `unsafe_report.py` counts them with the crate.

## The guard: no callback after `tw_helper_serve` returned

`Glue` holds the callbacks behind a `Guard` (a mutex over `open` and a count of running calls, and a condition variable).
Every call into the glue goes through `Guard::run`: under the lock it checks `open` and counts itself in, then calls the
glue without the lock, then counts itself out and wakes `close` at zero. `tw_helper_serve` calls `Guard::close` after
`twidget_helper::serve` returned: under the lock it clears `open`, then waits until no call runs. So:

- a call that entered before `close` finishes before `tw_helper_serve` returns, and the glue may free its contexts and
  tables the moment it returns;
- a call that comes later, from a thread or an `Arc` that outlived the serving, never reaches the glue: it gets the
  closed answer (`read` 0, `write` false, `shutdown` nothing, the clocks 0, the time zone 0, `random` zeros);
- entering never waits on another call, and the lock is never held across a call into the glue, so a write while a read
  blocks and a clock read from every thread go straight through, and `close` cannot deadlock with a callback.

`close` waits for a callback that blocks: `twidget_server`'s end shuts the stream down first (`break_off`), which wakes
a blocked read and write, and the contract makes every call after `shutdown` fail at once. A glue whose `read` or
`write` ignored `shutdown` would keep `tw_helper_serve` from returning (below).

Tested: `a_closed_guard_refuses_and_waits_for_the_calls_running` (unit: a call running holds `close`, a call made inside
a running one goes through, a call after `close` began is refused); `no_callback_after_serving_returned` (`oracle.rs`:
the stream ends while widgets boot, paint and are tapped, 12 servings, and the oracle's callbacks count every call after
`tw_helper_serve` returned for 100 ms: none); every session, fuzz and negative serving counts them too (none).

## Panics

Every entry runs under `entry`: `catch_unwind` around its body, and a panic aborts the process (`std::process::abort`),
as a panic on one of the helper's Rust threads does (`twidget_js`'s `JsThread`), so none unwinds into the C caller. With
`panic = "abort"` the panic aborts at once. A panic inside a callback written in Rust (a caller's own) aborts at that
callback's `extern "C"` boundary. Tested: `a_panicking_callback_aborts_the_process` (`abi.rs`, in a child process: the
panic's message, then an abnormal exit, and `tw_helper_serve` never returns).

## Threads

- `tw_helper_serve` runs on the caller's thread, which is the serving thread: it reads the stream, calls the lockdown
  once (after the opening was accepted, before `Ready`), and runs `twidget_server`'s dispatch and end.
- `read`: on the serving thread alone.
- `write`: from any thread, one at a time (`twidget_server`'s `Output` writes whole frames under one lock): the serving
  thread (`Ready`, refusals), the glue's host thread (the renderer's platform messages and requests), the engines' JS
  threads (their calls to the host), `widget.probe` (probe answers), `widget.reaper`.
- `shutdown`: from the serving thread at the end, or from the writing thread whose write failed; it may run while a read
  and a write block.
- `thread_cpu_seconds`, `steady_seconds`, `time_zone_offset`, `random`: from any thread, several at once (each engine's JS
  thread, the glue's host thread, the served host's threads, the serving thread).
- `steady_micros`: on the glue's host thread (the tap's records), tapped servings only.
- `tw_helper_free`: any thread, once per reason.

## Lifetimes

| Pointer | Lives | Rust's use |
|---|---|---|
| `stream`, `platform`, `lockdown` (the tables) | readable during the call | read once at entry and copied |
| the tables' `context`s | until `tw_helper_serve` returns | passed back to their callbacks, never dereferenced; no call after the return (the guard) |
| the callbacks | callable until `tw_helper_serve` returns | through the guard alone |
| `build` | readable during the call | borrowed for the serving (the opening's check, at most 64 bytes quoted in a refusal) |
| the lockdown's returned text | readable for `*size` until `tw_helper_serve` returns | copied right after the callback returns |
| `out` given to `read` and `random` | the callback's call | Rust's own buffer |
| `bytes` given to `write` | the callback's call | Rust's own frame; the glue copies what it keeps |
| `reason_size` | the call | written once, before the return |
| the returned reason | from the return until `tw_helper_free` | a boxed slice handed over; NULL with 0 for none |

## When the C side breaks the contract

| Breach | What happens |
|---|---|
| a table pointer neither NULL nor a valid table; a callback that is not a function of its type | undefined behaviour at the read or the call (a crash, at best) |
| `read` writing past `size` | a heap overflow of Rust's buffer, before Rust can see it; a count past `size` alone is clamped and harmless |
| `read`, `write` or `shutdown` blocking forever, also after `shutdown` | `tw_helper_serve` never returns: the end waits for the reader, and the guard for the running call |
| `write` keeping `bytes` past its call | the glue reads freed memory later; Rust is not affected |
| `random` filling fewer than `size` bytes | the rest stay zero: predictable "random" bytes for the widget, memory-safe |
| a clock that goes backwards, NaN or infinite | safe Rust throughout (`Duration::try_from_secs_f64` and the like); timers, budgets and waits misbehave for the widgets, and the stats show it |
| a callback that unwinds (a C++ exception) | undefined behaviour: unwinding through `extern "C"` frames; the C++ glue declares every callback `noexcept`, so an exception terminates there |
| a callback calling `tw_helper_serve` | a second serving inside the first: memory-safe, but its frames interleave with the first's on the stream |
| the lockdown's text shorter than `*size`, or freed before it is copied | an out-of-bounds read; a size past `isize::MAX` reads nothing |
| `build` shorter than `build_size` | an out-of-bounds read at the opening's check |
| `reason_size` not writable or misaligned | undefined behaviour at the write |
| `tw_helper_free` with another size, another pointer, or twice | heap corruption (the system allocators ignore the size, but the contract does not) |
| a context used by the glue after the return | fine: Rust no longer calls it |

## What else was checked

- The ABI from C++: `oracle/helper_ffi_oracle.cpp` links the static library (with `nova-c-api`, so the C++ serving runs
  on the same Nova) and serves every session of the glue comparison through `tw_helper_serve` beside tdesktop's
  `WidgetHelper::Serve`, frame for frame (`tests/oracle.rs`); mutated streams through both; the contract stretched and
  broken. The counts are in rust/PLAN.md's H2 row.
- Imports: the static library linked into a helper-like executable for `x86_64-win7-windows-msvc` and
  `i686-win7-windows-msvc` (H3's check, done for H2 by hand; the row has the tables).
