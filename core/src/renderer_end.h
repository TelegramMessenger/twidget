// The renderer end of TwidgetCore's two ends: a Renderer whose Backend calls, frames and reports travel to a platform end over the embedder's channel; docs/TWIDGET_CORE.md "Two ends over a byte channel".
#pragma once

#include "remote.h"
#include "renderer.h"
#include "twidget/json.h"

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace twlite::remote {

class RendererEnd {
public:
    /// Every renderer message of one deliver(), and whether the engine's frame message is due after them.
    struct Output {
        std::vector<twidget::Json> messages;
        bool frame = false;
    };

    RendererEnd(double width, double dpr, bool dark, RendererChannel& channel, Caps caps = {}, Clock* clock = nullptr);
    ~RendererEnd();
    RendererEnd(const RendererEnd&) = delete;
    RendererEnd& operator=(const RendererEnd&) = delete;

    std::vector<twidget::Json> receive(const twidget::Json& message);
    void want_frame();
    bool deliver(std::string_view message, Output& out);
    /// For the helper's own calls the channel does not carry, compositing among them: only while not stopped(), each followed by report().
    Renderer& renderer() { return *renderer_; }
    void report(const std::vector<twidget::Json>& messages);

    bool stopped() const { return !reason_.empty(); }
    const std::string& stop_reason() const { return reason_; }

private:
    class Proxy;

    template <class Encode> bool post(Encode encode_message);
    template <class M> bool send(const M& message);
    template <class Request, class Decode> bool ask(const Request& request, Decode decode_answer);
    template <class M> bool take(std::string_view message, M& out);
    bool stop(const std::string& reason);
    bool run(std::string_view message, Output& out);
    void run_frame(const FrameRequest& request, Output& out);
    void pointed(const std::vector<twidget::Json>& messages);
    void finish(const std::vector<twidget::Json>& messages, const Pointer* pointer = nullptr, Frame* frame = nullptr, const ClaimAnswer* claim = nullptr);
    State current_state() const;

    RendererChannel& channel_;
    Caps caps_;
    std::string reason_;
    std::optional<State> sent_state_;
    std::string sent_selection_;
    bool frame_asked_ = false;
    std::unique_ptr<Proxy> proxy_;
    std::unique_ptr<Renderer> renderer_;
};

} // namespace twlite::remote
