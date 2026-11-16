// RendererEnd: each Backend call of the renderer becomes a message or a request that waits, each call ends with the reports in a fixed order, and the first failure stops the end for good.
#include "renderer_end.h"

#include <cstdint>
#include <cstring>
#include <utility>

namespace twlite::remote {
namespace {

std::optional<twidget::WidgetGestureClaim> claim_of(const std::vector<twidget::Json>& messages) {
    for (const twidget::Json& m : messages)
        if (m["type"].asString() == "gesture") return twidget::WidgetGestureClaim{m["x"].asBool(), m["y"].asBool(), m["hold"].asBool()};
    return std::nullopt;
}

uint64_t bits_of(const double& d) {
    uint64_t b = 0;
    std::memcpy(&b, &d, sizeof b);
    return b;
}

bool same(const State& a, const State& b) {
    return bits_of(a.width) == bits_of(b.width) && a.height == b.height && a.dirty == b.dirty && a.wants_tick == b.wants_tick
        && bits_of(a.next_tick_in) == bits_of(b.next_tick_in) && a.frame_asked == b.frame_asked;
}

} // namespace

template <class Encode> bool RendererEnd::post(Encode encode_message) {
    if (stopped()) return false;
    std::string bytes;
    if (!encode_message(bytes)) return stop("a message past the caps");
    channel_.send(std::move(bytes));
    return true;
}

template <class M> bool RendererEnd::send(const M& message) {
    return post([&](std::string& bytes) { return encode(message, caps_, bytes); });
}

template <class Request, class Decode> bool RendererEnd::ask(const Request& request, Decode decode_answer) {
    if (stopped()) return false;
    std::string bytes;
    if (!encode(request, caps_, bytes)) return stop("a request past the caps");
    std::string answer;
    if (!channel_.request(std::move(bytes), answer)) return stop("a request got no answer");
    return decode_answer(std::string_view(answer)) || stop("an answer did not decode");
}

template <class M> bool RendererEnd::take(std::string_view message, M& out) {
    return decode(message, caps_, out) || stop("a platform message that did not decode");
}

class RendererEnd::Proxy final : public Backend {
public:
    explicit Proxy(RendererEnd& end) : end_(end) {}

    int create_font(const std::string& families, float size_px, int weight, bool italic, const std::string& features, FontMetrics& out) override {
        return requested_font(FontRequest{false, families, size_px, weight, italic, features}, out);
    }

    int create_canvas_font(const std::string& families, float size_px, int weight, bool italic, FontMetrics& out) override {
        return requested_font(FontRequest{true, families, size_px, weight, italic, {}}, out);
    }

    void delete_font(int font) override {
        end_.send(FontRelease{font});
    }

    float text_width(int font, const std::string& text) override {
        WidthAnswer answer;
        return answered(WidthRequest{font, text}, answer) ? answer.width : 0;
    }

    bool load_image(int id, const std::string& url, float& w, float& h) override {
        if (url.size() > end_.caps_.url) return false;
        ImageAnswer answer;
        if (!answered(ImageRequest{id, url}, answer)) return false;
        w = answer.w;
        h = answer.h;
        return answer.ok;
    }

    void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override {
        end_.post([&](std::string& bytes) { return encode_canvas_run(id, w, h, ops, end_.caps_, bytes); });
    }

    bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) override {
        const PixelsRequest request{id, x, y, w, h};
        PixelsAnswer answer;
        if (!end_.ask(request, [&](std::string_view bytes) { return decode(bytes, end_.caps_, request, answer); })) return false;
        if (answer.ok) rgba = std::move(answer.rgba);
        return answer.ok;
    }

    void canvas_release(int id) override {
        end_.send(CanvasRelease{id});
    }

private:
    template <class Request, class Answer> bool answered(const Request& request, Answer& answer) {
        return end_.ask(request, [&](std::string_view bytes) { return decode(bytes, end_.caps_, answer); });
    }

    int requested_font(const FontRequest& request, FontMetrics& out) {
        FontAnswer answer;
        if (!answered(request, answer)) return 0;
        out = answer.metrics;
        return answer.font;
    }

    RendererEnd& end_;
};

RendererEnd::RendererEnd(double width, double dpr, bool dark, RendererChannel& channel, Caps caps, Clock* clock)
    : channel_(channel), caps_(caps), proxy_(std::make_unique<Proxy>(*this)), renderer_(std::make_unique<Renderer>(width, dpr, dark, *proxy_, clock)) {
    finish({});
}

RendererEnd::~RendererEnd() = default;

bool RendererEnd::stop(const std::string& reason) {
    if (reason_.empty()) reason_ = reason;
    return false;
}

std::vector<twidget::Json> RendererEnd::receive(const twidget::Json& message) {
    if (stopped()) return {};
    std::vector<twidget::Json> out = renderer_->receive(message);
    finish(out);
    if (stopped()) return {};
    return out;
}

void RendererEnd::want_frame() {
    if (stopped()) return;
    frame_asked_ = true;
    finish({});
}

void RendererEnd::report(const std::vector<twidget::Json>& messages) {
    finish(messages);
}

bool RendererEnd::deliver(std::string_view message, Output& out) {
    out = {};
    if (stopped()) return false;
    Output result;
    if (!run(message, result) || stopped()) return false;
    out = std::move(result);
    return true;
}

bool RendererEnd::run(std::string_view message, Output& out) {
    const std::optional<Kind> kind = kind_of(message);
    if (!kind) return stop("a platform message of no known kind or version");
    switch (*kind) {
    case Kind::FrameRequest: {
        FrameRequest m;
        if (!take(message, m)) return false;
        run_frame(m, out);
        return true;
    }
    case Kind::SetWidth: {
        SetWidth m;
        if (!take(message, m)) return false;
        renderer_->set_width(m.width, out.messages);
        finish(out.messages);
        return true;
    }
    case Kind::SetMaxHeight: {
        SetMaxHeight m;
        if (!take(message, m)) return false;
        renderer_->set_max_height(m.height, out.messages);
        finish(out.messages);
        return true;
    }
    case Kind::SetTheme: {
        SetTheme m;
        if (!take(message, m)) return false;
        renderer_->set_theme(m.dark, out.messages);
        finish(out.messages);
        return true;
    }
    case Kind::Touch: {
        Touch m;
        if (!take(message, m)) return false;
        out.messages = renderer_->touch(m.action, m.x, m.y, m.pointer, m.primary, m.mouse, m.modifiers, m.button);
        pointed(out.messages);
        return true;
    }
    case Kind::Hover: {
        Hover m;
        if (!take(message, m)) return false;
        out.messages = renderer_->hover(m.action, m.x, m.y, m.modifiers);
        pointed(out.messages);
        return true;
    }
    case Kind::ClearSelection: {
        ClearSelection m;
        if (!take(message, m)) return false;
        renderer_->clear_selection();
        finish({});
        return true;
    }
    case Kind::ClaimQuery: {
        ClaimQuery m;
        if (!take(message, m)) return false;
        const ClaimAnswer answer{m.seq, renderer_->claim_at(m.x, m.y)};
        finish({}, nullptr, nullptr, &answer);
        return true;
    }
    default:
        return stop("a message of kind " + std::to_string(int(*kind)) + " where platform input belongs");
    }
}

void RendererEnd::run_frame(const FrameRequest& request, Output& out) {
    if (request.tick) {
        out.messages = renderer_->tick();
        out.frame = std::exchange(frame_asked_, false);
    }
    Frame frame;
    frame.seq = request.seq;
    if (!stopped() && (request.paint == Paint::Force || (request.paint == Paint::IfDirty && renderer_->dirty()))) {
        frame.list = renderer_->paint();
        std::vector<Rect> damage;
        if (renderer_->damage(damage)) frame.damage = std::move(damage);
        frame.painted = true;
    }
    finish(out.messages, nullptr, &frame);
}

void RendererEnd::pointed(const std::vector<twidget::Json>& messages) {
    if (stopped()) return;
    const Pointer pointer{renderer_->cursor(), claim_of(messages)};
    finish(messages, &pointer);
}

void RendererEnd::finish(const std::vector<twidget::Json>& messages, const Pointer* pointer, Frame* frame, const ClaimAnswer* claim) {
    if (stopped()) return;
    Messages reports;
    for (const twidget::Json& m : messages)
        if (is_platform_message(m["type"].asString())) reports.texts.push_back(m.dump());
    if (!reports.texts.empty() && !send(reports)) return;
    if (pointer && !send(*pointer)) return;
    Selection selection;
    if (renderer_->may_have_selection()) selection.text = renderer_->selected_text();
    if (stopped()) return;
    if (selection.text != sent_selection_) {
        if (!send(selection)) return;
        sent_selection_ = std::move(selection.text);
    }
    const State state = current_state();
    if (frame) {
        frame->state = state;
        if (send(*frame)) sent_state_ = state;
        return;
    }
    if (claim && !send(*claim)) return;
    if (sent_state_ && same(*sent_state_, state)) return;
    if (send(state)) sent_state_ = state;
}

State RendererEnd::current_state() const {
    return State{renderer_->width(), renderer_->viewport_height(), renderer_->dirty(), renderer_->wants_tick(), renderer_->next_tick_in(), frame_asked_};
}

} // namespace twlite::remote
