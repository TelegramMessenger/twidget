// The platform end of TwidgetCore's two ends: it checks every message of a renderer end it does not trust, calls the host's Backend, and gives the host frames and reports; docs/TWIDGET_CORE.md "Two ends over a byte channel".
#pragma once

#include "remote.h"
#include "twidget/json.h"

#include <cstdint>
#include <deque>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace twlite::remote {

class PlatformEnd {
public:
    class Host {
    public:
        virtual ~Host() = default;
        /// The answer to one request_frame(), unchanged ones included; the host paints its fonts, images and canvases before it passes on later traffic.
        virtual void frame(Frame&& frame) = 0;
        virtual void state(const State& state) = 0;
        virtual void messages(std::vector<twidget::Json>&& messages) = 0;
        virtual void pointer(const Pointer& pointer) = 0;
        virtual void selection(const Selection& selection) = 0;
        virtual void claim(const ClaimAnswer& answer) = 0;
        /// Called once, when the end stops for good: the host stops the widget.
        virtual void refused(const std::string& reason) = 0;
    };

    PlatformEnd(Backend& backend, Host& host, Channel& channel, Caps caps = {});
    PlatformEnd(const PlatformEnd&) = delete;
    PlatformEnd& operator=(const PlatformEnd&) = delete;

    uint64_t request_frame(bool tick, Paint paint);
    void set_width(double width);
    void set_max_height(double height);
    void set_theme(bool dark);
    void touch(int action, double x, double y, int pointer, bool primary, bool mouse = false, int modifiers = 0, int button = 0);
    void hover(int action, double x, double y, int modifiers = 0);
    void clear_selection();
    uint64_t ask_claim(double x, double y);

    bool receive(std::string_view message);
    bool answer(std::string_view request, std::string& reply);
    /// Stops the end for good without a word to the host, as when the embedder ends the route itself: no reply is due any more and no painted list is kept. It allocates nothing.
    void close() noexcept;

    const State& state() const { return state_; }
    bool refusing() const { return refused_; }

private:
    struct Expected {
        Kind kind = Kind::Frame;
        uint64_t seq = 0;
    };

    template <class M> bool send(const M& message, const Expected* expected = nullptr);
    template <class M> bool take(std::string_view message, Kind kind, M& out);
    template <class M> bool respond(const M& message, std::string& reply);
    bool solicited(Kind kind, uint64_t seq);
    bool due(Kind kind, Kind answers, uint64_t seq);
    bool take_delta(std::string_view message);
    void framed(Frame&& frame, std::optional<std::string> list);
    bool refuse(const std::string& reason);
    bool take_messages(std::string_view message);
    bool answer_pixels(std::string_view request, std::string& reply);

    Backend& backend_;
    Host& host_;
    Channel& channel_;
    Caps caps_;
    State state_;
    /// Dropped whole on close: an empty deque may allocate (libstdc++ does), and close() must not.
    std::optional<std::deque<Expected>> expected_{std::in_place};
    uint64_t frames_ = 0;
    uint64_t claims_ = 0;
    bool refused_ = false;
    /// The list of the last painted Frame while it is within `delta_base`, for a FrameDelta to copy from.
    std::optional<std::string> base_;
    uint64_t base_seq_ = 0;
};

} // namespace twlite::remote
