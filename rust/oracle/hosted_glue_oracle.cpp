// tdesktop's hosted glue (tgw/tgw_hosted_glue.cpp and tgw_cursor.cpp, copied by helper_oracles.cmake with the renderer's clock and the frame message's on the session's step clock) on TwidgetCore's real renderer end and renderer, driven by scripted sessions on stdin for rust/crates/helper to compare with; the session format is tests/common/glue.rs's.

#include "core/src/platform_end.h"
#include "core/src/renderer.h"
#include "log_backend.h"
#include "oracle_io.h"
#include "tgw/tgw_hosted_glue.h"
#include "tgw/tgw_hosted_wire.h"
#include "twidget/remote_hosted.h"

#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace {

namespace Remote = twlite::remote;
using oracle::In;
using oracle::Out;

constexpr auto kLoggedBytes = std::size_t(4096);

double NowMs = 0.;
Out Log;

class StepClock final : public twlite::Clock {
public:
    double now_ms() override { return NowMs; }
};

void LogBytes(std::string_view bytes) {
    if (bytes.size() > kLoggedBytes) {
        auto hash = uint64_t(14695981039346656037ull);
        for (const auto c : bytes) {
            hash = (hash ^ uint8_t(c)) * 1099511628211ull;
        }
        Log.u8('#').u64(bytes.size()).u64(hash);
    } else {
        Log.u8('=').str(bytes);
    }
}

class NoHost final : public Remote::PlatformEnd::Host {
public:
    void frame(Remote::Frame &&) override {}
    void state(const Remote::State &) override {}
    void messages(std::vector<twidget::Json> &&) override {}
    void pointer(const Remote::Pointer &) override {}
    void selection(const Remote::Selection &) override {}
    void claim(const Remote::ClaimAnswer &) override {}
    void refused(const std::string &reason) override { Log.u8('!').str(reason); }
};

class NoChannel final : public Remote::Channel {
public:
    void send(std::string) override {}
};

class Platform final : public twidget::SandboxHostedPlatform {
public:
    explicit Platform(bool imagesLoad) {
        _backend.images_load = imagesLoad;
    }

    void send(const std::string &message) override {
        Log.u8('p');
        LogBytes(message);
    }

    std::optional<std::string> request(const std::string &message) override {
        Log.u8('q');
        LogBytes(message);
        auto reply = std::string();
        const auto answered = _end.answer(message, reply);
        Log.u8('a').flag(answered);
        LogBytes(reply);
        return answered ? std::optional<std::string>(std::move(reply)) : std::nullopt;
    }

private:
    LogBackend _backend;
    NoHost _host;
    NoChannel _channel;
    Remote::PlatformEnd _end{ _backend, _host, _channel, Tgw::HostedWire::EmbeddingCaps() };

};

std::shared_ptr<Platform> Hosted;
std::shared_ptr<twidget::Renderer> Held;
std::vector<std::string> ReadyQueue;

void Begin(In &in) {
    const auto scheme = in.str();
    const auto width = in.f64();
    const auto flags = in.u8();
    ReadyQueue.clear();
    const auto queued = in.u32();
    for (auto i = uint32_t(); i != queued; ++i) {
        ReadyQueue.push_back(in.str());
    }
    Held = nullptr;
    Hosted = (flags & 1) ? std::make_shared<Platform>((flags & 2) != 0) : nullptr;
    Held = Tgw::MakeHostedRenderer(scheme, width, Hosted);
    Log.u8('r').flag(Held != nullptr);
    if (!Held) {
        return;
    }
    const auto weak = std::weak_ptr<twidget::Renderer>(Held);
    Held->onMessage = [weak](const std::string &json) {
        Log.u8('m');
        LogBytes(json);
        const auto strong = weak.lock();
        if (strong && json == "{\"type\":\"ready\"}") {
            for (const auto &message : std::exchange(ReadyQueue, {})) {
                strong->send(message);
            }
        }
    };
    Held->onDiag = [](const std::string &kind, const std::string &detail) {
        Log.u8('g').str(kind).str(detail);
    };
}

void Step(int op, In &in) {
    NowMs = in.f64();
    switch (op) {
    case 'L': if (Held) Held->load(); break;
    case 'S': {
        const auto json = in.str();
        if (Held) Held->send(json);
    } break;
    case 'T': {
        const auto theme = in.str();
        if (Held) Held->setTheme(theme);
    } break;
    case 'W': {
        const auto width = in.f64();
        if (Held) Held->setWidth(width);
    } break;
    case 'P': {
        const auto message = in.str();
        if (Hosted && Hosted->onMessage) Hosted->onMessage(message);
    } break;
    case 'D': if (Held) Held->destroy(); break;
    case 'E': Held = nullptr; break;
    }
}

} // namespace

twlite::Clock *OracleClock() {
    static auto clock = StepClock();
    return &clock;
}

double OracleClockMs() {
    return NowMs;
}

int main() {
    Tgw::PrepareHostedRenderers();
    return oracle::serve([](int op, In &in) {
        Log = Out();
        if (op == 'B') {
            NowMs = in.f64();
            Begin(in);
        } else if (op == 'L' || op == 'S' || op == 'T' || op == 'W' || op == 'P' || op == 'D' || op == 'E') {
            Step(op, in);
        } else {
            return false;
        }
        oracle::answer(1, Log.s);
        return true;
    });
}
