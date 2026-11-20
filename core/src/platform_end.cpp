// PlatformEnd: a message is decoded and checked whole before the Backend or the host sees any of it, and the first refusal stops the end for good.
#include "platform_end.h"

#include <optional>
#include <utility>

namespace twlite::remote {
namespace {

constexpr int JSON_DEPTH = 4;
constexpr size_t PIXELS_ANSWER_HEAD = 2 + 1 + 4;

bool platform_message(const twidget::Json& json) {
    const twidget::Json* type = json.find("type");
    return type && type->isString() && is_platform_message(type->asString());
}

std::string named(Kind kind) {
    return "kind " + std::to_string(int(kind));
}

} // namespace

PlatformEnd::PlatformEnd(Backend& backend, Host& host, Channel& channel, Caps caps)
    : backend_(backend), host_(host), channel_(channel), caps_(caps) {}

template <class M> bool PlatformEnd::send(const M& message, const Expected* expected) {
    if (refused_) return false;
    std::string bytes;
    if (!encode(message, caps_, bytes)) return refuse("an input did not encode within the caps");
    if (expected) expected_.push_back(*expected);
    channel_.send(std::move(bytes));
    return true;
}

template <class M> bool PlatformEnd::take(std::string_view message, Kind kind, M& out) {
    return decode(message, caps_, out) || refuse("a message of " + named(kind) + " did not decode");
}

template <class M> bool PlatformEnd::respond(const M& message, std::string& reply) {
    if (refused_) return false;
    std::string bytes;
    if (!encode(message, caps_, bytes)) return refuse("an answer did not encode within the caps");
    reply = std::move(bytes);
    return true;
}

bool PlatformEnd::solicited(Kind kind, uint64_t seq) {
    if (expected_.empty()) return refuse("an unsolicited message of " + named(kind));
    const Expected& front = expected_.front();
    if (front.kind != kind) return refuse("a message of " + named(kind) + " where " + named(front.kind) + " was due");
    if (front.seq != seq) return refuse("a message of " + named(kind) + " for request " + std::to_string(seq) + " where " + std::to_string(front.seq) + " was due");
    expected_.pop_front();
    return true;
}

bool PlatformEnd::refuse(const std::string& reason) {
    if (refused_) return false;
    refused_ = true;
    expected_.clear();
    host_.refused(reason);
    return false;
}

uint64_t PlatformEnd::request_frame(bool tick, Paint paint) {
    const Expected next{Kind::Frame, frames_ + 1};
    if (!send(FrameRequest{next.seq, tick, paint}, &next)) return 0;
    frames_ = next.seq;
    return frames_;
}

void PlatformEnd::set_width(double width) {
    send(SetWidth{width});
}

void PlatformEnd::set_max_height(double height) {
    send(SetMaxHeight{height});
}

void PlatformEnd::set_theme(bool dark) {
    send(SetTheme{dark});
}

void PlatformEnd::touch(int action, double x, double y, int pointer, bool primary, bool mouse, int modifiers, int button) {
    const Expected report{Kind::Pointer, 0};
    send(Touch{action, x, y, pointer, primary, mouse, modifiers, button}, &report);
}

void PlatformEnd::hover(int action, double x, double y, int modifiers) {
    const Expected report{Kind::Pointer, 0};
    send(Hover{action, x, y, modifiers}, &report);
}

void PlatformEnd::clear_selection() {
    send(ClearSelection{});
}

uint64_t PlatformEnd::ask_claim(double x, double y) {
    const Expected next{Kind::ClaimAnswer, claims_ + 1};
    if (!send(ClaimQuery{next.seq, x, y}, &next)) return 0;
    claims_ = next.seq;
    return claims_;
}

bool PlatformEnd::receive(std::string_view message) {
    if (refused_) return false;
    const std::optional<Kind> kind = kind_of(message);
    if (!kind) return refuse("a message of no known kind or version");
    switch (*kind) {
    case Kind::Frame: {
        Frame frame;
        if (!take(message, *kind, frame) || !solicited(*kind, frame.seq)) return false;
        state_ = frame.state;
        host_.frame(std::move(frame));
        break;
    }
    case Kind::State: {
        State state;
        if (!take(message, *kind, state)) return false;
        state_ = state;
        host_.state(state_);
        break;
    }
    case Kind::Messages:
        if (!take_messages(message)) return false;
        break;
    case Kind::Pointer: {
        Pointer pointer;
        if (!take(message, *kind, pointer) || !solicited(*kind, 0)) return false;
        host_.pointer(pointer);
        break;
    }
    case Kind::Selection: {
        Selection selection;
        if (!take(message, *kind, selection)) return false;
        host_.selection(selection);
        break;
    }
    case Kind::ClaimAnswer: {
        ClaimAnswer claim;
        if (!take(message, *kind, claim) || !solicited(*kind, claim.seq)) return false;
        host_.claim(claim);
        break;
    }
    case Kind::FontRelease: {
        FontRelease release;
        if (!take(message, *kind, release)) return false;
        backend_.delete_font(release.font);
        break;
    }
    case Kind::CanvasRun: {
        CanvasRun run;
        if (!take(message, *kind, run)) return false;
        backend_.canvas_run(run.id, run.w, run.h, run.ops);
        break;
    }
    case Kind::CanvasRelease: {
        CanvasRelease release;
        if (!take(message, *kind, release)) return false;
        backend_.canvas_release(release.id);
        break;
    }
    case Kind::FontRequest:
    case Kind::WidthRequest:
    case Kind::ImageRequest:
    case Kind::PixelsRequest:
    case Kind::WidthsRequest:
        return refuse("a request of " + named(*kind) + " where a one-way message belongs");
    default:
        return refuse("a message of " + named(*kind) + ", which only the platform end sends");
    }
    return !refused_;
}

bool PlatformEnd::take_messages(std::string_view message) {
    Messages batch;
    if (!take(message, Kind::Messages, batch)) return false;
    std::vector<twidget::Json> parsed;
    parsed.reserve(batch.texts.size());
    for (const std::string& text : batch.texts) {
        std::optional<twidget::Json> json = twidget::Json::parse(text, JSON_DEPTH);
        if (!json || !platform_message(*json)) return refuse("a report that is not one of the five platform messages");
        parsed.push_back(std::move(*json));
    }
    host_.messages(std::move(parsed));
    return true;
}

bool PlatformEnd::answer(std::string_view request, std::string& reply) {
    if (refused_) return false;
    const std::optional<Kind> kind = kind_of(request);
    if (!kind) return refuse("a request of no known kind or version");
    switch (*kind) {
    case Kind::FontRequest: {
        FontRequest m;
        if (!take(request, *kind, m)) return false;
        FontMetrics metrics;
        const int font = m.canvas ? backend_.create_canvas_font(m.families, m.size, m.weight, m.italic, metrics)
                                  : backend_.create_font(m.families, m.size, m.weight, m.italic, m.features, metrics);
        return respond(FontAnswer{font, metrics}, reply);
    }
    case Kind::WidthRequest: {
        WidthRequest m;
        if (!take(request, *kind, m)) return false;
        return respond(WidthAnswer{backend_.text_width(m.font, m.text)}, reply);
    }
    case Kind::ImageRequest: {
        ImageRequest m;
        if (!take(request, *kind, m)) return false;
        float w = 0, h = 0;
        const bool ok = backend_.load_image(m.id, m.url, w, h);
        return respond(ImageAnswer{ok, w, h}, reply);
    }
    case Kind::PixelsRequest:
        return answer_pixels(request, reply);
    case Kind::WidthsRequest: {
        WidthsRequest m;
        if (!take(request, *kind, m)) return false;
        std::vector<float> widths = backend_.text_widths(m.texts);
        if (widths.size() != m.texts.size()) return refuse("the Backend gave " + std::to_string(widths.size()) + " widths for " + std::to_string(m.texts.size()) + " texts");
        return respond(WidthsAnswer{std::move(widths)}, reply);
    }
    case Kind::Frame:
    case Kind::State:
    case Kind::Messages:
    case Kind::Pointer:
    case Kind::Selection:
    case Kind::ClaimAnswer:
    case Kind::FontRelease:
    case Kind::CanvasRun:
    case Kind::CanvasRelease:
        return refuse("a one-way message of " + named(*kind) + " where a request belongs");
    default:
        return refuse("a message of " + named(*kind) + ", which only the platform end sends");
    }
}

bool PlatformEnd::answer_pixels(std::string_view request, std::string& reply) {
    PixelsRequest m;
    if (!take(request, Kind::PixelsRequest, m)) return false;
    const uint64_t bytes = uint64_t(m.w) * uint64_t(m.h) * 4;
    const bool fits = bytes <= caps_.values.pixels && caps_.message >= PIXELS_ANSWER_HEAD && bytes <= caps_.message - PIXELS_ANSWER_HEAD;
    PixelsAnswer pixels;
    if (fits) {
        std::vector<uint8_t> rgba;
        if (backend_.canvas_read(m.id, m.x, m.y, m.w, m.h, rgba) && rgba.size() == bytes) {
            pixels.ok = true;
            pixels.rgba = std::move(rgba);
        }
    }
    return respond(pixels, reply);
}

} // namespace twlite::remote
