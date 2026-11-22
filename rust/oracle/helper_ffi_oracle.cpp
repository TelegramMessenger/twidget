// The helper's serving end to end, side by side: tdesktop's WidgetHelper::Serve (_other/widget_helper_renderer.cpp on widget-sandbox's engine end, the hosted glue and the tap, compiled by helper_oracles.cmake) and the Rust one through twidget_helper_ffi's C ABI (tw_helper_serve), each over an in-memory stream this program drives as Telegram would; rust/crates/helper_ffi/tests/oracle.rs sends the sessions, mutated streams and the ABI's negative cases.

#include "core/src/platform_end.h"
#include "core/src/renderer.h"
#include "log_backend.h"
#include "oracle_io.h"
#include "test/test_widget_helper_tap.h"
#include "tgw/tgw_hosted_wire.h"
#include "twidget/engine_platform.h"
#include "twidget/remote_engine.h"
#include "twidget/remote_hosted.h"
#include "twidget/remote_stream.h"
#include "twidget_helper.h"

#ifdef _WIN32
#include <windows.h>
#include <psapi.h>
#else
#include <sys/resource.h>
#endif

#include <algorithm>
#include <atomic>
#include <bit>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstring>
#include <deque>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

namespace WidgetHelper {
std::string Serve(twidget::SandboxRemoteStream &stream, const std::string &build, const std::function<std::string()> &lockdown);
std::string ServeUntapped(twidget::SandboxRemoteStream &stream, const std::string &build, const std::function<std::string()> &lockdown);
} // namespace WidgetHelper

namespace {

namespace Remote = twlite::remote;
using oracle::In;
using oracle::Out;

// The session's clock, one value a step: both arms read it, the C++ through its engine's platform and the copied clocks, the Rust through the platform table.
std::atomic<uint64_t> SteadyBits{std::bit_cast<uint64_t>(1000.)};
std::atomic<int64_t> MicrosNow{1000000000};

double Steady() {
    return std::bit_cast<double>(SteadyBits.load());
}

void SetClock(double seconds) {
    SteadyBits.store(std::bit_cast<uint64_t>(seconds));
    MicrosNow.store(int64_t(seconds * 1e6));
}

void Pattern(uint8_t *out, size_t size) {
    for (size_t i = 0; i != size; ++i) {
        out[i] = uint8_t(i * 7 + 3);
    }
}

class StepClock final : public twlite::Clock {
public:
    double now_ms() override { return Steady() * 1000.; }
};

const auto kBuild = std::string("helper-ffi-oracle");
const auto kNoRuntime = std::string("ERROR no runtime");
constexpr auto kWaitSeconds = 60;
constexpr auto kFrameLimit = size_t(48) << 20;

enum Kind : uint8_t {
    Open = 2, SafeArea = 10, Visible = 12, Stop = 16, Probe = 18, Close = 21,
    HostingOpening = 32, HostedStart = 33, HostedTheme = 34, Platform = 35, PlatformAnswer = 36,
    Ready = 64, Refused = 65, Stats = 85, Died = 86, ProbeAnswer = 87,
    PlatformMessage = 96, PlatformRequest = 97, ValidatedOps = 98, RendererReady = 99, Rendered = 100, Nodes = 101,
};

// One direction of an in-memory stream: bytes in order, ended by its writer or by a shutdown of either end.
struct Lane {
    std::deque<char> bytes;
    bool ended = false;
};

struct Pipes {
    std::mutex mutex;
    std::condition_variable changed;
    Lane toEngine;
    Lane toHost;
    bool engineShut = false;
    bool hostShut = false;
};

// One end of the pair, as a socket's: a shutdown wakes and fails this end's calls and ends both lanes, the other end reading what was already written, then 0; the host's end can stop writing alone. The engine's end can fail on purpose: its reads after `bytesLeft` bytes, or every write.
class End final : public twidget::SandboxRemoteStream {
public:
    End(std::shared_ptr<Pipes> pipes, bool engine, size_t bytesLeft = size_t(-1), bool failWrites = false)
    : _pipes(std::move(pipes)), _engine(engine), _bytesLeft(bytesLeft), _failWrites(failWrites) {}

    size_t read(char *out, size_t size) override {
        auto lock = std::unique_lock(_pipes->mutex);
        auto &lane = _engine ? _pipes->toEngine : _pipes->toHost;
        _pipes->changed.wait(lock, [&] { return shut() || !lane.bytes.empty() || lane.ended; });
        if (shut() || lane.bytes.empty() || !_bytesLeft) {
            return 0;
        }
        const auto n = std::min({size, lane.bytes.size(), _bytesLeft});
        std::copy_n(lane.bytes.begin(), n, out);
        lane.bytes.erase(lane.bytes.begin(), lane.bytes.begin() + n);
        if (_bytesLeft != size_t(-1)) _bytesLeft -= n;
        return n;
    }

    bool write(std::string_view bytes) override {
        {
            const auto lock = std::lock_guard(_pipes->mutex);
            auto &lane = _engine ? _pipes->toHost : _pipes->toEngine;
            if (shut() || lane.ended || _failWrites) {
                return false;
            }
            lane.bytes.insert(lane.bytes.end(), bytes.begin(), bytes.end());
        }
        _pipes->changed.notify_all();
        return true;
    }

    void shutdown() override {
        {
            const auto lock = std::lock_guard(_pipes->mutex);
            (_engine ? _pipes->engineShut : _pipes->hostShut) = true;
            _pipes->toEngine.ended = true;
            _pipes->toHost.ended = true;
        }
        _pipes->changed.notify_all();
    }

    // This end writes no more: the other reads what is left, then 0.
    void finishWriting() {
        {
            const auto lock = std::lock_guard(_pipes->mutex);
            (_engine ? _pipes->toHost : _pipes->toEngine).ended = true;
        }
        _pipes->changed.notify_all();
    }

private:
    bool shut() const {
        return _engine ? _pipes->engineShut : _pipes->hostShut;
    }

    const std::shared_ptr<Pipes> _pipes;
    const bool _engine;
    size_t _bytesLeft;
    const bool _failWrites;
};

// A frame as the stream's table lays it out, built field by field.
class FrameOut {
public:
    FrameOut(uint32_t runtime, uint8_t kind) {
        u32(runtime);
        _body.push_back(char(kind));
    }
    FrameOut &str(std::string_view s) {
        u32(uint32_t(s.size()));
        _body.append(s);
        return *this;
    }
    FrameOut &i64(int64_t v) {
        for (auto i = 0; i != 8; ++i) _body.push_back(char(uint8_t(uint64_t(v) >> (8 * i))));
        return *this;
    }
    FrameOut &boolean(bool v) {
        _body.push_back(char(v ? 1 : 0));
        return *this;
    }
    [[nodiscard]] std::string bytes() const {
        auto out = std::string();
        const auto n = uint32_t(_body.size());
        for (auto i = 0; i != 4; ++i) out.push_back(char(uint8_t(n >> (8 * i))));
        return out + _body;
    }

private:
    void u32(uint32_t v) {
        for (auto i = 0; i != 4; ++i) _body.push_back(char(uint8_t(v >> (8 * i))));
    }
    std::string _body;
};

uint64_t Little(std::string_view bytes, size_t at, size_t n) {
    auto v = uint64_t();
    for (size_t i = 0; i != n && at + i < bytes.size(); ++i) {
        v |= uint64_t(uint8_t(bytes[at + i])) << (8 * i);
    }
    return v;
}

uint32_t RuntimeOf(std::string_view frame) {
    return uint32_t(Little(frame, 4, 4));
}

uint8_t KindOf(std::string_view frame) {
    return frame.size() > 8 ? uint8_t(frame[8]) : 0;
}

// The first string field of a frame whose fields start with one, after `skip` bytes of others.
std::string FirstString(std::string_view frame, size_t skip = 0) {
    const auto at = size_t(9) + skip;
    const auto n = size_t(Little(frame, at, 4));
    return (at + 4 <= frame.size()) ? std::string(frame.substr(at + 4, std::min(n, frame.size() - at - 4))) : std::string();
}

class NoHost final : public Remote::PlatformEnd::Host {
public:
    void frame(Remote::Frame &&) override {}
    void state(const Remote::State &) override {}
    void messages(std::vector<twidget::Json> &&) override {}
    void pointer(const Remote::Pointer &) override {}
    void selection(const Remote::Selection &) override {}
    void claim(const Remote::ClaimAnswer &) override {}
    void refused(const std::string &) override {}
};

class NoChannel final : public Remote::Channel {
public:
    void send(std::string) override {}
};

// Telegram's side of one hosted widget's platform: its requests answered by a platform end over the log backend, as hosted_glue_oracle.cpp answers them.
class HostPlatform {
public:
    explicit HostPlatform(bool imagesLoad) { _backend.images_load = imagesLoad; }

    std::string answer(std::string_view request) {
        auto reply = std::string();
        return _end.answer(request, reply) ? reply : std::string();
    }

private:
    LogBackend _backend;
    NoHost _host;
    NoChannel _channel;
    Remote::PlatformEnd _end{_backend, _host, _channel, Tgw::HostedWire::EmbeddingCaps()};
};

// What the Rust arm's callbacks count: every call, and those after tw_helper_serve returned, which the ABI promises never happen; never freed, so a late call stays countable.
enum class Callback { Read, Write, Shutdown, Cpu, Steady, Micros, Zone, Random, Count };

struct Glue {
    End *stream = nullptr;
    std::atomic<bool> returned = false;
    std::atomic<uint64_t> calls = 0;
    std::atomic<uint64_t> late = 0;
    std::atomic<uint64_t> lockdowns = 0;
    std::atomic<uint64_t> each[size_t(Callback::Count)] = {};
    std::string lockdownWhy;

    void called(Callback which) {
        ++calls;
        ++each[size_t(which)];
        if (returned) ++late;
    }
};

Glue &GlueOf(void *context) {
    return *static_cast<Glue*>(context);
}

size_t GlueRead(void *context, uint8_t *out, size_t size) noexcept {
    auto &glue = GlueOf(context);
    glue.called(Callback::Read);
    return glue.stream->read(reinterpret_cast<char*>(out), size);
}

bool GlueWrite(void *context, const uint8_t *bytes, size_t size) noexcept {
    auto &glue = GlueOf(context);
    glue.called(Callback::Write);
    return glue.stream->write(std::string_view(reinterpret_cast<const char*>(bytes), size));
}

void GlueShutdown(void *context) noexcept {
    auto &glue = GlueOf(context);
    glue.called(Callback::Shutdown);
    glue.stream->shutdown();
}

double GlueCpu(void *context) noexcept {
    GlueOf(context).called(Callback::Cpu);
    return 0.;
}

double GlueSteady(void *context) noexcept {
    GlueOf(context).called(Callback::Steady);
    return Steady();
}

int64_t GlueMicros(void *context) noexcept {
    GlueOf(context).called(Callback::Micros);
    return MicrosNow.load();
}

int32_t GlueZone(void *context, int64_t) noexcept {
    GlueOf(context).called(Callback::Zone);
    return 0;
}

void GlueRandom(void *context, uint8_t *out, size_t size) noexcept {
    GlueOf(context).called(Callback::Random);
    Pattern(out, size);
}

const uint8_t *GlueLockdown(void *context, size_t *size) noexcept {
    auto &glue = GlueOf(context);
    ++glue.lockdowns;
    *size = glue.lockdownWhy.size();
    return reinterpret_cast<const uint8_t*>(glue.lockdownWhy.data());
}

std::string Taken(uint8_t *reason, size_t size) {
    auto out = reason ? std::string(reinterpret_cast<const char*>(reason), size) : std::string();
    tw_helper_free(reason, size);
    return out;
}

// How one serving is set up: which end, tapped or not, and what its glue does wrong on purpose.
struct Setup {
    bool rust = false;
    bool tapped = false;
    std::string build = kBuild;
    std::string lockdownWhy;
    bool lockdown = true;
    bool failWrites = false;
    size_t bytesLeft = size_t(-1);
    bool answerRequests = true;
};

// One helper serving its stream on a thread, and Telegram's side of it: a reader that keeps every frame and answers the platform requests, and the writes of the session.
class Arm {
public:
    explicit Arm(Setup setup) : _setup(std::move(setup)) {
        const auto pipes = std::make_shared<Pipes>();
        _host = std::make_unique<End>(pipes, false);
        _engine = std::make_unique<End>(pipes, true, _setup.bytesLeft, _setup.failWrites);
        _glue = new Glue();
        _glue->stream = _engine.get();
        _glue->lockdownWhy = _setup.lockdownWhy;
        _serving = std::thread([this] { serve(); });
        _reader = std::thread([this] { read(); });
    }
    Arm(const Arm &) = delete;
    Arm &operator=(const Arm &) = delete;

    ~Arm() {
        _host->finishWriting();
        if (_serving.joinable()) _serving.join();
        _engine->shutdown();
        if (_reader.joinable()) _reader.join();
    }

    void send(const std::string &bytes) {
        const auto lock = std::lock_guard(_writing);
        _host->write(bytes);
    }

    void opening() {
        const auto stamp = [](const std::string &bundle) { return bundle.substr(0, bundle.find('\n')); };
        send(FrameOut(0, HostingOpening).i64(1).str(kBuild).str(Resources().sandboxBundle).str(Resources().validatorBundle).str(stamp(Resources().rendererBundle)).bytes());
    }

    void open(uint32_t runtime, bool imagesLoad) {
        {
            const auto lock = std::lock_guard(_mutex);
            _platforms[runtime] = std::make_shared<HostPlatform>(imagesLoad);
        }
        send(FrameOut(runtime, Open).bytes());
    }

    // A probe of `code` on `runtime` and its answer; none when the wait ran out or the serving ended.
    std::optional<std::string> probe(uint32_t runtime, const std::string &code) {
        const auto seq = ++_seq;
        send(FrameOut(runtime, Probe).i64(seq).str(code).bytes());
        auto lock = std::unique_lock(_mutex);
        const auto done = _changed.wait_for(lock, std::chrono::seconds(kWaitSeconds), [&] { return _probes.contains(seq) || _readerDone; });
        if (!done || !_probes.contains(seq)) {
            return std::nullopt;
        }
        return _probes[seq];
    }

    // Two probes in a row: what the inputs before them made, through the glue's thread and the engine and back, is on the stream after them.
    bool barrier(uint32_t runtime) {
        const auto first = probe(runtime, "1");
        const auto second = first ? probe(runtime, "1") : std::nullopt;
        if (!second) {
            return false;
        } else if (*first == kNoRuntime || *second == kNoRuntime) {
            settle();
        }
        return true;
    }

    // A probe of a widget without its engine (before Open, after its death) answers at once, past the glue's thread: until the stream is quiet then.
    void settle() {
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
        for (;;) {
            std::this_thread::sleep_for(std::chrono::milliseconds(5));
            const auto now = std::chrono::steady_clock::now();
            const auto lock = std::lock_guard(_mutex);
            if (now - _lastFrameAt >= std::chrono::milliseconds(30) || now >= deadline) {
                return;
            }
        }
    }

    // Until Ready, Refused or the end.
    bool ready() {
        auto lock = std::unique_lock(_mutex);
        return _changed.wait_for(lock, std::chrono::seconds(kWaitSeconds), [&] { return _greeted || _readerDone; });
    }

    auto pendingStats(std::ptrdiff_t waitFor = 0) {
        auto lock = std::unique_lock(_mutex);
        const auto count = [&] {
            return std::count_if(_frames.begin(), _frames.end(), [](const std::string &frame) { return KindOf(frame) == Stats; });
        };
        if (waitFor) {
            _changed.wait_for(lock, std::chrono::seconds(1), [&] { return _readerDone || count() >= waitFor; });
        }
        return count();
    }

    // The frames since the last take, ordered by runtime and kind with each kind's in the order they came (each kind of a runtime comes from one thread in order, the kinds from several).
    std::vector<std::string> take() {
        auto frames = std::vector<std::string>();
        {
            const auto lock = std::lock_guard(_mutex);
            frames.swap(_frames);
        }
        std::stable_sort(frames.begin(), frames.end(), [](const std::string &a, const std::string &b) {
            return std::pair(RuntimeOf(a), KindOf(a)) < std::pair(RuntimeOf(b), KindOf(b));
        });
        return frames;
    }

    // Telegram stops writing: the serving reads to the end and returns.
    bool finish() {
        _host->finishWriting();
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(kWaitSeconds);
        while (!_served && std::chrono::steady_clock::now() < deadline) {
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
        if (!_served) {
            return false;
        }
        _serving.join();
        // A callback after tw_helper_serve returned would show here.
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        _engine->shutdown();
        _reader.join();
        return true;
    }

    const std::string &reason() const { return _reason; }
    const Glue &glue() const { return *_glue; }
    uint64_t lockdowns() const { return _setup.rust ? _glue->lockdowns.load() : _cppLockdowns.load(); }

    static const twidget::SandboxResources &Resources() {
        static const auto resources = twidget::SandboxResources::fromDirectory(TW_WIDGET_JS);
        return *resources;
    }

private:
    void serve() {
        if (_setup.rust) {
            const auto stream = tw_helper_stream{_glue, &GlueRead, &GlueWrite, &GlueShutdown};
            const auto platform = tw_helper_platform{_glue, &GlueCpu, &GlueSteady, &GlueMicros, &GlueZone, &GlueRandom};
            const auto lockdown = tw_helper_lockdown{_glue, &GlueLockdown};
            auto size = size_t();
            const auto reason = tw_helper_serve(&stream, &platform, reinterpret_cast<const uint8_t*>(_setup.build.data()), _setup.build.size(),
                                                _setup.lockdown ? &lockdown : nullptr, _setup.tapped, &size);
            _glue->returned = true;
            _reason = Taken(reason, size);
        } else {
            auto lockdown = std::function<std::string()>();
            if (_setup.lockdown) {
                lockdown = [this] {
                    ++_cppLockdowns;
                    return _setup.lockdownWhy;
                };
            }
            auto &stream = *static_cast<twidget::SandboxRemoteStream*>(_engine.get());
            _reason = _setup.tapped ? WidgetHelper::Serve(stream, _setup.build, lockdown) : WidgetHelper::ServeUntapped(stream, _setup.build, lockdown);
        }
        _served = true;
    }

    // The engine's frames until its end; each platform request answered at once by its widget's platform end.
    void read() {
        auto buffer = std::string();
        auto chunk = std::string(1 << 16, '\0');
        for (;;) {
            while (buffer.size() >= 4 && buffer.size() >= 4 + Little(buffer, 0, 4)) {
                const auto size = size_t(4 + Little(buffer, 0, 4));
                handle(buffer.substr(0, size));
                buffer.erase(0, size);
            }
            if (buffer.size() >= 4 && Little(buffer, 0, 4) > kFrameLimit) {
                break;
            }
            const auto n = _host->read(chunk.data(), chunk.size());
            if (!n) {
                break;
            }
            buffer.append(chunk.data(), n);
        }
        {
            const auto lock = std::lock_guard(_mutex);
            _readerDone = true;
            if (!buffer.empty()) _frames.push_back("<cut> " + std::to_string(buffer.size()));
        }
        _changed.notify_all();
    }

    void handle(std::string frame) {
        const auto runtime = RuntimeOf(frame);
        const auto kind = KindOf(frame);
        if (kind == PlatformRequest && _setup.answerRequests) {
            auto platform = std::shared_ptr<HostPlatform>();
            {
                const auto lock = std::lock_guard(_mutex);
                if (const auto i = _platforms.find(runtime); i != _platforms.end()) platform = i->second;
            }
            const auto reply = platform ? platform->answer(FirstString(frame)) : std::string();
            send(FrameOut(runtime, PlatformAnswer).str(reply).bytes());
        }
        {
            const auto lock = std::lock_guard(_mutex);
            if (kind == ProbeAnswer) {
                _probes[int64_t(Little(frame, 9, 8))] = FirstString(frame, 8);
            } else if (kind == Ready || kind == Refused) {
                _greeted = true;
            }
            _frames.push_back(std::move(frame));
            _lastFrameAt = std::chrono::steady_clock::now();
        }
        _changed.notify_all();
    }

    const Setup _setup;
    std::unique_ptr<End> _host;
    std::unique_ptr<End> _engine;
    Glue *_glue = nullptr;
    std::thread _serving;
    std::thread _reader;
    std::atomic<bool> _served = false;
    std::atomic<uint64_t> _cppLockdowns = 0;
    std::string _reason;
    std::mutex _writing;
    std::mutex _mutex;
    std::condition_variable _changed;
    std::vector<std::string> _frames;
    std::map<int64_t, std::string> _probes;
    std::map<uint32_t, std::shared_ptr<HostPlatform>> _platforms;
    std::atomic<int64_t> _seq = 0;
    bool _greeted = false;
    bool _readerDone = false;
    std::chrono::steady_clock::time_point _lastFrameAt = std::chrono::steady_clock::now();
};

// The palette the sessions' themes carry, as hosted_glue_oracle's sessions have it.
std::map<std::string, std::string> Palette(const std::string &scheme) {
    if (scheme == "dark") {
        return {{"bg_color", "#212121"}, {"text_color", "#ffffff"}};
    }
    return {{"bg_color", "#ffffff"}, {"text_color", "#000000"}, {"button_color", "#3390ec"}};
}

// The kinds a transcript counts, for the Rust side's tallies.
constexpr uint8_t kCounted[] = {PlatformMessage, PlatformRequest, ValidatedOps, RendererReady, Rendered, Nodes, Died, ProbeAnswer, Stats, Ready, Refused};

// Both arms' transcripts of a step: alike (status 1, with the tallies), or the first difference (status 2), or a wait that ran out (status 3).
void Compare(const std::vector<std::string> &cpp, const std::vector<std::string> &rust, std::string_view failure) {
    if (std::getenv("TW_HELPER_ORACLE_DUMP")) {
        const auto text = [](const std::string &f) {
            if (KindOf(f) == Stats) {
                auto t = std::string();
                for (auto i = 0; i != 11; ++i) t += std::to_string(std::bit_cast<double>(Little(f, 9 + 8 * i, 8))) + " ";
                return t;
            }
            const auto t = FirstString(f, KindOf(f) == ProbeAnswer ? 8 : 0);
            return t.substr(0, 300);
        };
        for (const auto &f : cpp) std::fprintf(stderr, "C++  %u %u %s\n", RuntimeOf(f), KindOf(f), text(f).c_str());
        for (const auto &f : rust) std::fprintf(stderr, "Rust %u %u %s\n", RuntimeOf(f), KindOf(f), text(f).c_str());
    }
    if (!failure.empty()) {
        oracle::answer(3, failure);
        return;
    }
    if (cpp != rust) {
        auto out = std::string("C++ " + std::to_string(cpp.size()) + " frames, Rust " + std::to_string(rust.size()) + "\n");
        for (size_t i = 0; i != std::max(cpp.size(), rust.size()); ++i) {
            const auto a = i < cpp.size() ? cpp[i] : std::string("<none>");
            const auto b = i < rust.size() ? rust[i] : std::string("<none>");
            if (a == b) continue;
            const auto shown = [](const std::string &f) {
                if (f.size() < 9) {
                    return f;
                }
                return "runtime " + std::to_string(RuntimeOf(f)) + " kind " + std::to_string(KindOf(f)) + " (" + std::to_string(f.size()) + " bytes): " + f.substr(9, 600);
            };
            out += "frame " + std::to_string(i) + "\n  C++:  " + shown(a) + "\n  Rust: " + shown(b) + "\n";
            break;
        }
        oracle::answer(2, out);
        return;
    }
    auto out = Out();
    auto bytes = uint64_t();
    for (const auto &f : rust) bytes += f.size();
    out.u32(uint32_t(rust.size())).u64(bytes);
    for (const auto kind : kCounted) {
        out.u32(uint32_t(std::count_if(rust.begin(), rust.end(), [&](const std::string &f) { return KindOf(f) == kind; })));
    }
    // The glue's controls among the platform messages: Content, Loaded, Stopped, Mirrored, Cursor, CanvasRefused, Clicked.
    for (uint8_t control = 64; control != 71; ++control) {
        out.u32(uint32_t(std::count_if(rust.begin(), rust.end(), [&](const std::string &f) {
            const auto message = FirstString(f);
            return KindOf(f) == PlatformMessage && message.size() >= 2 && uint8_t(message[0]) == Tgw::HostedWire::kTag && uint8_t(message[1]) == control;
        })));
    }
    oracle::answer(1, out.s);
}

std::unique_ptr<Arm> Arms[2];

// The engine's heap in a Stats frame (its third field). Both ends count it on nova_meter's accounts, the same blocks for the same work, but it is left out of the comparison: Nova keys its hash tables at random (per process and per table), so when a table that lost entries grows depends on the keys, and the count varies from run to run by multiples of 32 bytes, C++ against C++ as much as against Rust. With the keys pinned every Stats of the sessions is alike but one at a session's end, which varies with time (rust/PLAN.md, H2). TW_HELPER_ORACLE_MEMLOG=<file> appends every pair there: the step, then each arm's runtime and bytes.
constexpr auto kStatsMemoryAt = size_t(9 + 2 * 8);

void MaskMemory(std::vector<std::vector<std::string>> &taken) {
    static FILE *const log = [] {
        const auto path = std::getenv("TW_HELPER_ORACLE_MEMLOG");
        return path ? std::fopen(path, "ab") : nullptr;
    }();
    static auto step = uint64_t();
    ++step;
    std::vector<std::pair<uint32_t, double>> seen[2];
    for (auto i = 0; i != 2; ++i) {
        for (auto &f : taken[i]) {
            if (KindOf(f) == Stats && f.size() >= kStatsMemoryAt + 8) {
                seen[i].emplace_back(RuntimeOf(f), std::bit_cast<double>(Little(f, kStatsMemoryAt, 8)));
                std::fill_n(f.begin() + kStatsMemoryAt, 8, '\0');
            }
        }
    }
    if (!log) return;
    for (size_t k = 0; k != std::max(seen[0].size(), seen[1].size()); ++k) {
        const auto none = std::pair<uint32_t, double>(0, -1);
        const auto a = k < seen[0].size() ? seen[0][k] : none;
        const auto b = k < seen[1].size() ? seen[1][k] : none;
        std::fprintf(log, "%llu %u %.0f %u %.0f %s\n", (unsigned long long)step, a.first, a.second, b.first, b.second, a.second == b.second ? "same" : "DIFF");
    }
    std::fflush(log);
}

// Runs `step` on each arm in turn, the C++ one first, and compares what each sent.
void Both(const std::function<bool(Arm &)> &step) {
    auto taken = std::vector<std::vector<std::string>>(2);
    auto failure = std::string();
    for (auto i = 0; i != 2; ++i) {
        if (!Arms[i]) continue;
        if (!step(*Arms[i])) {
            failure += std::string(i ? "Rust" : "C++") + ": the barrier's wait ran out or the serving ended\n";
        }
    }
    if (Arms[0] && Arms[1] && failure.empty()) {
        // A trailing Stats uses a real 200 ms timer, including while the other arm is taking its step.
        const auto cpp = Arms[0]->pendingStats();
        const auto rust = Arms[1]->pendingStats();
        if (cpp < rust) Arms[0]->pendingStats(rust);
        if (rust < cpp) Arms[1]->pendingStats(cpp);
    }
    for (auto i = 0; i != 2; ++i) {
        if (Arms[i]) taken[i] = Arms[i]->take();
    }
    MaskMemory(taken);
    Compare(taken[0], taken[1], failure);
}

void Begin(In &in) {
    const auto runtime = in.u32();
    SetClock(in.f64());
    const auto html = in.str();
    const auto width = in.f64();
    const auto scheme = in.str();
    const auto flags = in.u8();
    auto config = twidget::SandboxConfig();
    config.innerWidth = width;
    config.theme = scheme;
    config.themeParams = Palette(scheme);
    auto start = twidget::SandboxHostedStart();
    start.theme.colorScheme = scheme;
    start.theme.params = Palette(scheme);
    start.validatedOps = (flags & 2) != 0;
    const auto started = FrameOut(runtime, HostedStart)
        .str(html)
        .str(twidget::SandboxRemoteCoding::encodeConfig(config))
        .str(twidget::SandboxRemoteCoding::encodeHostedStart(start))
        .bytes();
    Both([&](Arm &arm) {
        arm.open(runtime, (flags & 1) != 0);
        arm.send(started);
        return arm.barrier(runtime);
    });
}

void Step(int op, In &in) {
    const auto runtime = in.u32();
    SetClock(in.f64());
    auto frame = std::string();
    auto code = std::string();
    switch (op) {
    case 'P': frame = FrameOut(runtime, Platform).str(in.str()).bytes(); break;
    case 'T': {
        auto theme = twidget::WidgetTheme();
        theme.colorScheme = in.str();
        theme.params = Palette(theme.colorScheme);
        frame = FrameOut(runtime, HostedTheme).str(twidget::SandboxRemoteCoding::encodeTheme(theme)).bytes();
    } break;
    case 'A': {
        auto area = twidget::WidgetContentSafeArea();
        area.top = in.f64();
        area.bottom = in.f64();
        frame = FrameOut(runtime, SafeArea).str(twidget::SandboxRemoteCoding::encodeSafeArea(area)).bytes();
    } break;
    case 'V': frame = FrameOut(runtime, Visible).boolean(in.u8() != 0).bytes(); break;
    case 'X': frame = FrameOut(runtime, Stop).str(in.str()).bytes(); break;
    case 'J': code = in.str(); break;
    }
    Both([&](Arm &arm) {
        if (op == 'J') {
            return arm.probe(runtime, code).has_value() && arm.barrier(runtime);
        }
        arm.send(frame);
        return arm.barrier(runtime);
    });
}

void Finish(Out &out, Arm &arm) {
    const auto finished = arm.finish();
    out.flag(finished).str(arm.reason()).u64(arm.lockdowns()).u64(arm.glue().calls).u64(arm.glue().late);
    for (const auto &each : arm.glue().each) out.u64(each);
    const auto frames = arm.take();
    out.u32(uint32_t(frames.size()));
    for (const auto &f : frames) out.str(f);
}

uint64_t PeakMemoryBytes() {
#ifdef _WIN32
    auto counters = PROCESS_MEMORY_COUNTERS_EX();
    return K32GetProcessMemoryInfo(GetCurrentProcess(), reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&counters), sizeof(counters))
        ? counters.PeakPagefileUsage : 0;
#else
    auto usage = rusage();
    if (getrusage(RUSAGE_SELF, &usage) != 0) return 0;
#ifdef __APPLE__
    return uint64_t(usage.ru_maxrss);
#else
    return uint64_t(usage.ru_maxrss) * 1024;
#endif
#endif
}

// A stream of the host's bytes, mutated: each arm serves it to its end, its requests answered empty; why each ended, what each wrote, and the process's peak memory use.
void Fuzz(In &in) {
    const auto tapped = in.u8() != 0;
    const auto bytes = in.str();
    auto out = Out();
    for (const auto rust : {false, true}) {
        auto setup = Setup();
        setup.rust = rust;
        setup.tapped = tapped;
        auto arm = Arm(setup);
        arm.send(bytes);
        const auto finished = arm.finish();
        const auto frames = arm.take();
        auto written = uint64_t();
        for (const auto &f : frames) written += f.size();
        out.flag(finished).str(arm.reason()).u32(uint32_t(frames.size())).u64(written).u64(arm.glue().late);
    }
    out.u64(PeakMemoryBytes());
    oracle::answer(1, out.s);
}

// The ABI's contract broken or stretched on purpose, through tw_helper_serve alone; what came of it.
void Negative(In &in) {
    const auto scenario = in.u8();
    auto out = Out();
    const auto table = [](Glue *glue) {
        return std::pair(tw_helper_stream{glue, &GlueRead, &GlueWrite, &GlueShutdown}, tw_helper_platform{glue, &GlueCpu, &GlueSteady, &GlueMicros, &GlueZone, &GlueRandom});
    };
    switch (scenario) {
    case 'z': { // NULL reason_size, then NULL tables and callbacks: nothing served, no callback called.
        const auto glue = new Glue();
        auto [stream, platform] = table(glue);
        const auto nothing = tw_helper_serve(&stream, &platform, nullptr, 0, nullptr, false, nullptr);
        out.flag(nothing == nullptr).u64(glue->calls);
        const auto reasons = [&](const tw_helper_stream *s, const tw_helper_platform *p, const tw_helper_lockdown *l) {
            auto size = size_t(77);
            const auto reason = tw_helper_serve(s, p, nullptr, 0, l, false, &size);
            out.str(Taken(reason, size));
        };
        reasons(nullptr, &platform, nullptr);
        reasons(&stream, nullptr, nullptr);
        auto broken = stream;
        broken.read = nullptr;
        reasons(&broken, &platform, nullptr);
        broken = stream;
        broken.shutdown = nullptr;
        reasons(&broken, &platform, nullptr);
        auto blind = platform;
        blind.steady_micros = nullptr;
        reasons(&stream, &blind, nullptr);
        blind = platform;
        blind.random = nullptr;
        reasons(&stream, &blind, nullptr);
        const auto lockless = tw_helper_lockdown{glue, nullptr};
        reasons(&stream, &platform, &lockless);
        out.u64(glue->calls);
        tw_helper_free(nullptr, 0);
        tw_helper_free(nullptr, 5);
    } break;
    case 'e': { // A stream that ends at once, and one that fails inside the opening; its writes failing; a lockdown that fails; an empty and a huge build.
        const auto run = [&](Setup setup, bool open) {
            for (const auto rust : {false, true}) {
                setup.rust = rust;
                auto arm = Arm(setup);
                if (open) arm.opening();
                const auto finished = arm.finish();
                const auto frames = arm.take();
                out.flag(finished).str(arm.reason()).u64(arm.lockdowns()).u64(arm.glue().late).u32(uint32_t(frames.size()));
                for (const auto &f : frames) out.str(f.substr(0, 4096));
            }
        };
        auto setup = Setup();
        run(setup, false);
        setup.bytesLeft = 4096;
        run(setup, true);
        setup = Setup();
        setup.failWrites = true;
        run(setup, true);
        setup = Setup();
        setup.lockdownWhy = "the oracle's lockdown failed";
        run(setup, true);
        setup = Setup();
        setup.build = std::string();
        run(setup, true);
        setup.build = std::string(size_t(64) << 20, 'b');
        run(setup, true);
        setup = Setup();
        setup.lockdown = false;
        run(setup, true);
    } break;
    case 'l': { // Late callbacks: the stream ends while widgets boot, run and paint; none may be called once tw_helper_serve returned.
        const auto count = in.u32();
        auto setup = Setup();
        setup.rust = true;
        setup.tapped = in.u8() != 0;
        auto arm = Arm(setup);
        arm.opening();
        for (auto runtime = uint32_t(1); runtime <= count; ++runtime) {
            auto config = twidget::SandboxConfig();
            arm.open(runtime, true);
            arm.send(FrameOut(runtime, HostedStart).str(in.str()).str(twidget::SandboxRemoteCoding::encodeConfig(config)).str(twidget::SandboxRemoteCoding::encodeHostedStart({})).bytes());
            for (auto i = 0; i != 8; ++i) arm.send(FrameOut(runtime, Platform).str(in.str()).bytes());
        }
        const auto finished = arm.finish();
        out.flag(finished).str(arm.reason()).u64(arm.glue().calls).u64(arm.glue().late);
    } break;
    }
    oracle::answer(1, out.s);
}

} // namespace

double OracleSteadySeconds() {
    return Steady();
}

twlite::Clock *OracleClock() {
    static auto clock = StepClock();
    return &clock;
}

double OracleClockMs() {
    return Steady() * 1000.;
}

std::int64_t OracleSteadyMicros() {
    return MicrosNow.load();
}

std::unique_ptr<twidget::Renderer> OracleUntapped(std::shared_ptr<twidget::SandboxHostedPlatform> platform, const std::string &, const Test::HelperRendererBuild &build) {
    return build(std::move(platform));
}

namespace twidget::platform {

double threadCpuSeconds() {
    return 0.;
}

double steadySeconds() {
    return Steady();
}

int32_t timeZoneOffset(int64_t) {
    return 0;
}

void randomBytes(uint8_t *out, size_t size) {
    Pattern(out, size);
}

} // namespace twidget::platform

int main() {
    const auto began = std::chrono::steady_clock::now();
    const auto dump = std::getenv("TW_HELPER_ORACLE_DUMP") != nullptr;
    return oracle::serve([&](int op, In &in) {
        if (dump) {
            std::fprintf(stderr, "[%.3f] record %c\n", std::chrono::duration<double>(std::chrono::steady_clock::now() - began).count(), char(op));
        }
        switch (op) {
        case 'O': {
            const auto tapped = in.u8() != 0;
            SetClock(in.f64());
            for (auto i = 0; i != 2; ++i) {
                auto setup = Setup();
                setup.rust = (i == 1);
                setup.tapped = tapped;
                if (const auto only = std::getenv("TW_HELPER_ORACLE_ONLY"); only && std::atoi(only) != i) continue;
                Arms[i] = std::make_unique<Arm>(setup);
            }
            Both([](Arm &arm) {
                arm.opening();
                return arm.ready();
            });
        } break;
        case 'B': Begin(in); break;
        case 'P': case 'T': case 'A': case 'V': case 'X': case 'J': Step(op, in); break;
        case 'E': {
            const auto runtime = in.u32();
            Both([&](Arm &arm) {
                arm.send(FrameOut(runtime, Close).bytes());
                return true;
            });
        } break;
        case 'Q': {
            auto out = Out();
            for (auto &arm : Arms) {
                if (!arm) continue;
                Finish(out, *arm);
                arm = nullptr;
            }
            oracle::answer(1, out.s);
        } break;
        case 'F': Fuzz(in); break;
        case 'N': Negative(in); break;
        default: return false;
        }
        std::fflush(stdout);
        return true;
    });
}
