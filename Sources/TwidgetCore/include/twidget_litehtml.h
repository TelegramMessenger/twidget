// C ABI of the renderer (platform/apple/ffi.cpp, platform/web/wasm.cpp). The
// same shape as widget-sandbox's Blitz renderer (twidget_blitz.h) with the
// `twl_` prefix, so a host drives either engine through one table of functions.
// One renderer per widget behind an opaque handle; every call on the main
// thread. Strings returned by twl_* functions are released with twl_string_free.
#ifndef TWIDGET_LITEHTML_H
#define TWIDGET_LITEHTML_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct twl_renderer twl_renderer;

/// width in CSS px, dpr = device pixel ratio, theme "light" | "dark". NULL when the renderer could not be created.
twl_renderer *twl_create(double width, double dpr, const char *theme);
void twl_destroy(twl_renderer *handle);
/// Host → renderer message (JSON, docs/protocol.md). Returns a JSON array of renderer → host messages.
char *twl_receive(twl_renderer *handle, const char *json);
char *twl_set_width(twl_renderer *handle, double width);
/// The tallest the widget may grow, in CSS px; 900 until set.
char *twl_set_max_height(twl_renderer *handle, double height);
char *twl_set_theme(twl_renderer *handle, const char *theme);
/// A touch in CSS px. action: 0 down, 1 move, 2 up, 3 cancel. Returns renderer → host messages.
char *twl_touch(twl_renderer *handle, int32_t action, double x, double y, int32_t pointer, bool primary);
/// A pointer as twl_touch, from a mouse (`mouse`: no touch events) with modifiers (bit0 alt, bit1 ctrl, bit2 meta, bit3 shift).
char *twl_pointer(twl_renderer *handle, int32_t action, double x, double y, int32_t pointer, bool primary, bool mouse, int32_t modifiers);
/// A mouse over the view with no button down (action 0) or leaving it (1).
char *twl_hover(twl_renderer *handle, int32_t action, double x, double y, int32_t modifiers);
/// A wheel or trackpad scroll in CSS px; mode 0 pixels, 1 lines, 2 pages.
char *twl_wheel(twl_renderer *handle, double x, double y, double dx, double dy, int32_t mode, int32_t modifiers);
/// type "keydown" | "keyup", DOM key and code.
char *twl_key(twl_renderer *handle, const char *type, const char *key, const char *code, bool repeat, int32_t modifiers);
/// The text control being edited, for the platform to cover with its own editor (Renderer::EditorSpec
/// as JSON), or `null`. Check it after every call that returns messages: its generation changes with it.
char *twl_editor(twl_renderer *handle);
/// The platform's editor changed the text of editor generation `generation`. Returns renderer → host messages.
char *twl_edit(twl_renderer *handle, uint64_t generation, const char *text, const char *input_type);
/// The platform's editor let go of the keyboard. Returns renderer → host messages.
char *twl_end_editing(twl_renderer *handle);
/// One display frame: animations advance and coalesced moves go out. Call it each frame while twl_wants_tick holds,
/// otherwise after twl_next_tick_in seconds (infinity: nothing scheduled). Returns renderer → host messages.
char *twl_tick(twl_renderer *handle);
bool twl_wants_tick(twl_renderer *handle);
double twl_next_tick_in(twl_renderer *handle);
/// Hands transform and opacity animations to the platform compositor (src/compositor.h); off by default.
void twl_set_compositing(twl_renderer *handle, bool enabled);
/// The layers above the last paint as JSON (to_json(Composition)), and the renderer clock their tracks run on.
char *twl_composition(twl_renderer *handle);
double twl_clock(void);
/// Layer `index` of the last composition as a display list (document coordinates); NULL past the end.
char *twl_layer_content(twl_renderer *handle, uint32_t index);
/// Physical size (px) of the next render.
void twl_size(twl_renderer *handle, uint32_t *width, uint32_t *height);
bool twl_dirty(twl_renderer *handle);
/// Paints premultiplied RGBA8 into buffer (>= width*height*4 bytes). Returns the paint time in ms, negative on failure.
double twl_render(twl_renderer *handle, uint8_t *buffer, size_t capacity);
/// The size of an asynchronously decoded image (web backend); rebuilds the document. Returns renderer → host messages.
char *twl_image_ready(twl_renderer *handle, int32_t image, float width, float height);
/// The display list of the current document as JSON (src/display_list.h), CSS px; clears `dirty`. Release with twl_string_free.
char *twl_display_list(twl_renderer *handle);
/// Where the last display list differs from the one before, CSS px: `[[x, y, w, h], ...]`, or `null`
/// when everything must be painted. A host keeps its last frame and repaints only these areas.
char *twl_damage(twl_renderer *handle);
/// Drops caches that are made again on demand: on memory pressure, or when the widget goes off screen.
void twl_trim(twl_renderer *handle);
/// Diagnostics: the last rebuild as JSON (`serializeMs`, `parseLayoutMs`, `passes`, `htmlBytes`, `builds`, `displayOps`). Release with twl_string_free.
char *twl_stats(twl_renderer *handle);
/// Diagnostics: the HTML the document was last built from. Release with twl_string_free.
char *twl_html(twl_renderer *handle);
void twl_string_free(char *s);

#ifdef __cplusplus
}
#endif
#endif
