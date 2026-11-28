// Lossless, versioned binary forms of TwidgetCore's paint values for a renderer and a painter in two processes, read by decoders that take hostile bytes; docs/TWIDGET_CORE.md "Binary forms" has the layouts.
#pragma once

#include "backend.h"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace twlite::wire {

/// What a value is: its first byte.
enum class Form : uint8_t { List = 1, Damage = 2, Batch = 3, Metrics = 4 };
/// A value's second byte, the layout of every form: a changed layout takes a new version.
constexpr uint8_t VERSION = 1;

/// The caller's bounds, each on one counted field of a value and not on a total (the length of the bytes bounds that); a count travels as a u32 whatever its cap.
struct Caps {
    /// Commands of one display list.
    size_t commands = size_t(1) << 20;
    /// CanvasOps of one batch.
    size_t canvas_ops = size_t(1) << 20;
    /// Ops of one Path or Geometry.
    size_t path_ops = size_t(1) << 22;
    /// Points of one Path or Geometry; dashes of one Path or CanvasOp.
    size_t floats = size_t(1) << 22;
    /// Stops of one gradient: a Command's, a Path's fill or stroke, a CanvasPaint's.
    size_t stops = size_t(1) << 16;
    /// Functions of one filter list.
    size_t filters = size_t(1) << 12;
    /// Clips of one clip list.
    size_t clips = size_t(1) << 16;
    /// Bytes of one string: Command::text, CanvasOp::text.
    size_t text = size_t(4) << 20;
    /// Bytes of one rgba payload.
    size_t pixels = size_t(64) << 20;
    /// Rectangles of one damage.
    size_t rects = size_t(1) << 12;
};

/// What moved since the last paint: nullopt when the whole frame must be painted (Renderer::damage returned false), an empty vector when nothing moved.
using Damage = std::optional<std::vector<Rect>>;

/// Appends `list` to `out` without the members equal to their version-1 defaults. False, `out` cut back to its size on entry, for what the decoder would refuse under `caps`: a count or a length past its cap or UINT32_MAX, or an enum value version 1 does not know, in a member its kind paints or not.
bool encode(const DisplayList& list, const Caps& caps, std::string& out);
/// As encode(list, caps, out), and replaces `starts` with where each command starts and, last, where the list ends, counted from the list's first byte whatever `out` held before, while the list keeps within `most` bytes: for a longer one `starts` is left empty. `starts` untouched on false.
bool encode(const DisplayList& list, const Caps& caps, std::string& out, std::vector<size_t>& starts, size_t most = SIZE_MAX);
/// Appends `damage` to `out`; false, `out` as on entry, for more rectangles than `caps.rects` or UINT32_MAX.
bool encode(const Damage& damage, const Caps& caps, std::string& out);
/// Appends `ops` to `out`; false, `out` as on entry, for what the decoder would refuse, as for a list.
bool encode(const std::vector<CanvasOp>& ops, const Caps& caps, std::string& out);
/// Appends `metrics` to `out`: seven floats, nothing to refuse.
void encode(const FontMetrics& metrics, std::string& out);

/// Replaces `out`, after the last byte, with the one list all of `bytes` hold, an absent member at its version-1 default. False, `out` untouched, for anything else. It checks structure, not what the host does (finite floats, UTF-8, ids, paint cost); an allocation the caps allow may throw std::bad_alloc, `out` still untouched.
bool decode(std::string_view bytes, const Caps& caps, DisplayList& out);
/// The damage all of `bytes` hold, as for a list.
bool decode(std::string_view bytes, const Caps& caps, Damage& out);
/// The canvas ops all of `bytes` hold, as for a list.
bool decode(std::string_view bytes, const Caps& caps, std::vector<CanvasOp>& out);
/// The metrics all of `bytes` hold, as for a list.
bool decode(std::string_view bytes, FontMetrics& out);

} // namespace twlite::wire
