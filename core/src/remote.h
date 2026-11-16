// The messages between TwidgetCore's renderer end and its platform end, and the channel the embedder carries them on; docs/TWIDGET_CORE.md "Two ends over a byte channel" has the layouts.
#pragma once

#include "wire.h"
#include "twidget/gesture_policy.h"

#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace twlite::remote {

constexpr uint8_t VERSION = 1;

/// Kinds from 1 go from the platform to the renderer and kinds from 64 back; those from 32 answer the requests from 96.
enum class Kind : uint8_t {
    FrameRequest = 1, SetWidth = 2, SetMaxHeight = 3, SetTheme = 4, Touch = 5, Hover = 6, ClearSelection = 7, ClaimQuery = 8,
    FontAnswer = 32, WidthAnswer = 33, ImageAnswer = 34, PixelsAnswer = 35,
    Frame = 64, State = 65, Messages = 66, Pointer = 67, Selection = 68, ClaimAnswer = 69, FontRelease = 70, CanvasRun = 71, CanvasRelease = 72,
    FontRequest = 96, WidthRequest = 97, ImageRequest = 98, PixelsRequest = 99,
};

struct Caps {
    wire::Caps values;
    /// Bytes of one message either way: widget-sandbox's frames hold 32 MiB and 64 KiB, which leaves room for their framing.
    size_t message = size_t(32) << 20;
    size_t text = size_t(4) << 20;
    size_t url = size_t(8) << 20;
    size_t messages = 1024;
};

enum class Paint : uint8_t { Skip = 0, IfDirty = 1, Force = 2 };

struct FrameRequest {
    uint64_t seq = 0;
    bool tick = false;
    Paint paint = Paint::IfDirty;
};

struct SetWidth {
    double width = 0;
};

struct SetMaxHeight {
    double height = 0;
};

struct SetTheme {
    bool dark = false;
};

struct Touch {
    int32_t action = 0;
    double x = 0, y = 0;
    int32_t pointer = 0;
    bool primary = false, mouse = false;
    int32_t modifiers = 0, button = 0;
};

struct Hover {
    int32_t action = 0;
    double x = 0, y = 0;
    int32_t modifiers = 0;
};

struct ClearSelection {};

struct ClaimQuery {
    uint64_t seq = 0;
    double x = 0, y = 0;
};

struct FontAnswer {
    int32_t font = 0;
    FontMetrics metrics;
};

struct WidthAnswer {
    float width = 0;
};

struct ImageAnswer {
    bool ok = false;
    float w = 0, h = 0;
};

struct PixelsAnswer {
    bool ok = false;
    std::vector<uint8_t> rgba;
};

struct State {
    double width = 0;
    int32_t height = 0;
    bool dirty = false, wants_tick = false;
    double next_tick_in = std::numeric_limits<double>::infinity();
    bool frame_asked = false;
};

struct Frame {
    uint64_t seq = 0;
    State state;
    bool painted = false;
    DisplayList list;
    wire::Damage damage;
};

struct Messages {
    std::vector<std::string> texts;
};

struct Pointer {
    std::string cursor;
    std::optional<twidget::WidgetGestureClaim> claim;
};

struct Selection {
    std::string text;
};

struct ClaimAnswer {
    uint64_t seq = 0;
    twidget::WidgetGestureClaim claim;
};

struct FontRelease {
    int32_t font = 0;
};

struct CanvasRun {
    int32_t id = 0, w = 0, h = 0;
    std::vector<CanvasOp> ops;
};

struct CanvasRelease {
    int32_t id = 0;
};

struct FontRequest {
    bool canvas = false;
    std::string families;
    float size = 0;
    int32_t weight = 0;
    bool italic = false;
    std::string features;
};

struct WidthRequest {
    int32_t font = 0;
    std::string text;
};

struct ImageRequest {
    int32_t id = 0;
    std::string url;
};

struct PixelsRequest {
    int32_t id = 0, x = 0, y = 0, w = 0, h = 0;
};

std::optional<Kind> kind_of(std::string_view message);
bool is_platform_message(std::string_view type);

bool encode(const FrameRequest& message, const Caps& caps, std::string& out);
bool encode(const SetWidth& message, const Caps& caps, std::string& out);
bool encode(const SetMaxHeight& message, const Caps& caps, std::string& out);
bool encode(const SetTheme& message, const Caps& caps, std::string& out);
bool encode(const Touch& message, const Caps& caps, std::string& out);
bool encode(const Hover& message, const Caps& caps, std::string& out);
bool encode(const ClearSelection& message, const Caps& caps, std::string& out);
bool encode(const ClaimQuery& message, const Caps& caps, std::string& out);
bool encode(const FontAnswer& message, const Caps& caps, std::string& out);
bool encode(const WidthAnswer& message, const Caps& caps, std::string& out);
bool encode(const ImageAnswer& message, const Caps& caps, std::string& out);
bool encode(const PixelsAnswer& message, const Caps& caps, std::string& out);
bool encode(const Frame& message, const Caps& caps, std::string& out);
bool encode(const State& message, const Caps& caps, std::string& out);
bool encode(const Messages& message, const Caps& caps, std::string& out);
bool encode(const Pointer& message, const Caps& caps, std::string& out);
bool encode(const Selection& message, const Caps& caps, std::string& out);
bool encode(const ClaimAnswer& message, const Caps& caps, std::string& out);
bool encode(const FontRelease& message, const Caps& caps, std::string& out);
bool encode(const CanvasRun& message, const Caps& caps, std::string& out);
bool encode(const CanvasRelease& message, const Caps& caps, std::string& out);
bool encode(const FontRequest& message, const Caps& caps, std::string& out);
bool encode(const WidthRequest& message, const Caps& caps, std::string& out);
bool encode(const ImageRequest& message, const Caps& caps, std::string& out);
bool encode(const PixelsRequest& message, const Caps& caps, std::string& out);
bool encode_canvas_run(int32_t id, int32_t w, int32_t h, const std::vector<CanvasOp>& ops, const Caps& caps, std::string& out);

bool decode(std::string_view message, const Caps& caps, FrameRequest& out);
bool decode(std::string_view message, const Caps& caps, SetWidth& out);
bool decode(std::string_view message, const Caps& caps, SetMaxHeight& out);
bool decode(std::string_view message, const Caps& caps, SetTheme& out);
bool decode(std::string_view message, const Caps& caps, Touch& out);
bool decode(std::string_view message, const Caps& caps, Hover& out);
bool decode(std::string_view message, const Caps& caps, ClearSelection& out);
bool decode(std::string_view message, const Caps& caps, ClaimQuery& out);
bool decode(std::string_view message, const Caps& caps, FontAnswer& out);
bool decode(std::string_view message, const Caps& caps, WidthAnswer& out);
bool decode(std::string_view message, const Caps& caps, ImageAnswer& out);
bool decode(std::string_view message, const Caps& caps, const PixelsRequest& asked, PixelsAnswer& out);
bool decode(std::string_view message, const Caps& caps, Frame& out);
bool decode(std::string_view message, const Caps& caps, State& out);
bool decode(std::string_view message, const Caps& caps, Messages& out);
bool decode(std::string_view message, const Caps& caps, Pointer& out);
bool decode(std::string_view message, const Caps& caps, Selection& out);
bool decode(std::string_view message, const Caps& caps, ClaimAnswer& out);
bool decode(std::string_view message, const Caps& caps, FontRelease& out);
bool decode(std::string_view message, const Caps& caps, CanvasRun& out);
bool decode(std::string_view message, const Caps& caps, CanvasRelease& out);
bool decode(std::string_view message, const Caps& caps, FontRequest& out);
bool decode(std::string_view message, const Caps& caps, WidthRequest& out);
bool decode(std::string_view message, const Caps& caps, ImageRequest& out);
bool decode(std::string_view message, const Caps& caps, PixelsRequest& out);

class Channel {
public:
    virtual ~Channel() = default;
    virtual void send(std::string message) = 0;
};

class RendererChannel : public Channel {
public:
    virtual bool request(std::string message, std::string& answer) = 0;
};

} // namespace twlite::remote
