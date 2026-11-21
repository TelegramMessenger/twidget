// tdesktop's Debug tap (test/test_widget_helper_tap.cpp, copied with its _DEBUG half on and its clock probed by helper_oracles.cmake) driven by scripted sessions on stdin, for rust/crates/helper to compare with; the session format is tests/common/tap.rs's.

#include "oracle_io.h"
#include "test/test_widget_helper_tap.h"
#include "twidget/remote_hosted.h"

#include <cstdint>
#include <deque>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

using oracle::In;
using oracle::Out;

constexpr auto kLoggedTextBytes = std::size_t(1) << 20;

struct Action {
    uint8_t kind = 0;
    std::string first;
    std::string second;
};

using Actions = std::vector<Action>;

Out Log;
std::deque<Actions> Pending;
int64_t Reads = 0;
std::weak_ptr<twidget::Renderer> Wrapper;

void LogText(std::string_view text) {
    if (text.size() > kLoggedTextBytes) {
        auto hash = uint64_t(14695981039346656037ull);
        for (const auto c : text) {
            hash = (hash ^ uint8_t(c)) * 1099511628211ull;
        }
        Log.u8('#').u64(text.size()).u64(hash);
    } else {
        Log.u8('=').str(text);
    }
}

class ScriptedPlatform final : public twidget::SandboxHostedPlatform {
public:
    void send(const std::string &message) override {
        Log.u8('p');
        LogText(message);
    }

    std::optional<std::string> request(const std::string &message) override {
        Log.u8('q');
        LogText(message);
        const auto answer = (!message.empty() && message.front() == 'X')
            ? std::optional<std::string>()
            : std::optional<std::string>("A:" + message);
        Log.u8('a').flag(answer.has_value()).str(answer.value_or(std::string()));
        return answer;
    }
};

class ScriptedInner final : public twidget::Renderer {
public:
    explicit ScriptedInner(std::shared_ptr<twidget::SandboxHostedPlatform> platform)
    : _platform(std::move(platform)) {
        if (_platform) {
            _platform->onMessage = [this, alive = std::weak_ptr<bool>(_alive)](const std::string &message) {
                if (alive.lock()) {
                    call('P', message);
                }
            };
        }
    }

    ~ScriptedInner() override {
        Log.u8('z');
    }

    void load() override { call('L', {}); }
    void send(const std::string &json) override { call('S', json); }
    void setTheme(const std::string &theme) override { call('T', theme); }
    void setWidth(double width) override {
        Log.u8('i').u8('W').f64(width);
        act();
    }
    void destroy() override { call('D', {}); }

private:
    void call(char kind, const std::string &text) {
        Log.u8('i').u8(uint8_t(kind));
        LogText(text);
        act();
    }

    void act() {
        if (Pending.empty()) {
            return;
        }
        const auto actions = std::move(Pending.front());
        Pending.pop_front();
        for (const auto &action : actions) {
            switch (action.kind) {
            case 0:
                if (onMessage) {
                    onMessage(action.first);
                }
                break;
            case 1:
                if (onDiag) {
                    onDiag(action.first, action.second);
                }
                break;
            case 2:
                if (_platform) {
                    _platform->send(action.first);
                }
                break;
            case 3:
                if (_platform) {
                    const auto answer = _platform->request(action.first);
                    Log.u8('A').flag(answer.has_value());
                }
                break;
            case 4:
                throw std::runtime_error("scripted");
            }
        }
    }

    const std::shared_ptr<bool> _alive = std::make_shared<bool>(true);
    std::shared_ptr<twidget::SandboxHostedPlatform> _platform;

};

std::shared_ptr<ScriptedPlatform> Platform;
std::shared_ptr<twidget::Renderer> Held;

void TakeActions(In &in) {
    Pending.clear();
    const auto lists = in.u32();
    for (auto i = uint32_t(); i != lists; ++i) {
        auto actions = Actions();
        const auto count = in.u32();
        for (auto j = uint32_t(); j != count; ++j) {
            auto action = Action{ .kind = in.u8() };
            action.first = in.str();
            action.second = in.str();
            actions.push_back(std::move(action));
        }
        Pending.push_back(std::move(actions));
    }
}

void Begin(In &in) {
    const auto flags = in.u8();
    const auto scheme = in.str();
    Reads = 0;
    Held = nullptr;
    Platform = (flags & 1) ? std::make_shared<ScriptedPlatform>() : nullptr;
    auto renderer = Test::TapHelperRenderer(Platform, scheme, [&](std::shared_ptr<twidget::SandboxHostedPlatform> tapped) {
        Log.u8('b').flag(tapped != nullptr).flag(tapped == Platform);
        return (flags & 2)
            ? std::unique_ptr<twidget::Renderer>(std::make_unique<ScriptedInner>(std::move(tapped)))
            : nullptr;
    });
    Log.u8('r').flag(renderer != nullptr);
    Held = std::shared_ptr<twidget::Renderer>(std::move(renderer));
    Wrapper = Held;
    if (Held) {
        Held->onMessage = [](const std::string &json) {
            Log.u8('m');
            LogText(json);
            constexpr auto kAgain = std::string_view("@again:");
            constexpr auto kTheme = std::string_view("@theme:");
            const auto again = std::string_view(json).substr(0, kAgain.size()) == kAgain;
            const auto theme = std::string_view(json).substr(0, kTheme.size()) == kTheme;
            if (const auto strong = Wrapper.lock(); strong && again) {
                strong->send(json.substr(kAgain.size()));
            } else if (strong && theme) {
                strong->setTheme(json.substr(kTheme.size()));
            }
        };
        Held->onDiag = [](const std::string &kind, const std::string &detail) {
            Log.u8('g').str(kind).str(detail);
        };
    }
}

void Step(int op, In &in) {
    auto text = std::string();
    auto width = 0.;
    if (op == 'S' || op == 'T' || op == 'P') {
        text = in.str();
    } else if (op == 'W') {
        width = in.f64();
    }
    TakeActions(in);
    try {
        if (op == 'P') {
            if (Platform && Platform->onMessage) {
                Platform->onMessage(text);
            }
        } else if (op == 'E') {
            Held = nullptr;
        } else if (Held) {
            switch (op) {
            case 'L': Held->load(); break;
            case 'S': Held->send(text); break;
            case 'T': Held->setTheme(text); break;
            case 'W': Held->setWidth(width); break;
            case 'D': Held->destroy(); break;
            }
        }
    } catch (...) {
        Log.u8('x');
    }
    Pending.clear();
}

} // namespace

int64_t OracleSteadyMicros() {
    return 1000000 + 7 * (++Reads);
}

int main() {
    return oracle::serve([](int op, In &in) {
        Log = Out();
        if (op == 'B') {
            Begin(in);
        } else if (op == 'L' || op == 'S' || op == 'T' || op == 'W' || op == 'D' || op == 'P' || op == 'E') {
            Step(op, in);
        } else {
            return false;
        }
        oracle::answer(1, Log.s);
        return true;
    });
}
