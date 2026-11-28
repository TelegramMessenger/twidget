/* The widget helper's serving in Rust (twidget rust/crates/helper_ffi): the C++ OS glue keeps the entry point, the channel it adopted, the lockdown and the OS's answers, and serves the stream through these callbacks as WidgetHelper::Serve does; only bytes and numbers cross, and Rust never touches a descriptor or a handle. */
#ifndef TWIDGET_HELPER_H
#define TWIDGET_HELPER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Every callback below gets its table's `context` as given, may be called from any thread and several at once (a read while a write blocks) until tw_helper_serve returns and never after, must not unwind (a C++ glue's are noexcept) and must not call tw_helper_serve. */

/* The adopted stream to Telegram: `read` blocks until at least one byte came and copies at most `size` bytes to `out`, returning how many (0 at the end, on an error and after `shutdown`); `write` blocks until all `size` bytes are written (false once broken or shut down); `shutdown` wakes both without waiting, and every later call fails. */
typedef struct tw_helper_stream {
    void* context;
    size_t (*read)(void* context, uint8_t* out, size_t size);
    bool (*write)(void* context, const uint8_t* bytes, size_t size);
    void (*shutdown)(void* context);
} tw_helper_stream;

/* What only the OS answers: the calling thread's CPU seconds, std::chrono::steady_clock's seconds and whole microseconds since its epoch (the latter the Debug tap's clock), the user's time zone in seconds east of UTC at `epoch_seconds`, and `size` bytes of the system's CSPRNG written to `out`. */
typedef struct tw_helper_platform {
    void* context;
    double (*thread_cpu_seconds)(void* context);
    double (*steady_seconds)(void* context);
    int64_t (*steady_micros)(void* context);
    int32_t (*time_zone_offset)(void* context, int64_t epoch_seconds);
    void (*random)(void* context, uint8_t* out, size_t size);
} tw_helper_platform;

/* Locks the process down once the opening was accepted, before Ready, on the serving thread: why it could not (`*size` bytes, readable until tw_helper_serve returns), or `*size` 0 when it did. */
typedef struct tw_helper_lockdown {
    void* context;
    const uint8_t* (*lockdown)(void* context, size_t* size);
} tw_helper_lockdown;

/* Serves the stream until it ends, on the calling thread: why it ended (`*reason_size` bytes to free with tw_helper_free), or NULL and 0 when it ended as it should; `build` is the stamp's build (`build_size` bytes, NULL for none), `lockdown` may be NULL (no lockdown), `tapped` runs every widget through the Debug tap on `steady_micros`; a NULL `stream` or `platform`, or a NULL callback in any table, serves nothing and returns why; a NULL `reason_size` serves nothing and returns NULL. Every widget is destroyed and no callback runs once it returned, so the tables and their contexts may go then. */
uint8_t* tw_helper_serve(const tw_helper_stream* stream, const tw_helper_platform* platform, const uint8_t* build, size_t build_size,
                         const tw_helper_lockdown* lockdown, bool tapped, size_t* reason_size);

/* Frees a reason tw_helper_serve returned, with its `*reason_size`, once; NULL does nothing. */
void tw_helper_free(uint8_t* bytes, size_t size);

#ifdef __cplusplus
}
#endif

#endif
