// renderer_end.cpp, platform_end.cpp and remote.cpp on stdin for rust/crates/ends: each 'c' record a session as JSON, each answer its transcript (crates/ends/tests/oracle.rs lays both out).
#include "log_backend.h"
#include "platform_end.h"
#include "remote.h"
#include "renderer_end.h"
#include "wire.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <deque>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using twidget::Json;
using scripted::f32s;
using scripted::f64s;

namespace {

bool read_exact(void* to, size_t n) {
    return n == 0 || std::fread(to, 1, n, stdin) == n;
}

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

std::string unhex(const std::string& digits) {
    std::string out;
    auto value = [](char c) { return c >= 'a' ? c - 'a' + 10 : c - '0'; };
    for (size_t i = 0; i + 1 < digits.size(); i += 2) out.push_back(char(value(digits[i]) * 16 + value(digits[i + 1])));
    return out;
}

std::string hexs(const std::string& bytes) {
    return scripted::hex(bytes);
}

std::string hexs(const std::vector<uint8_t>& bytes) {
    return scripted::hex(std::string(bytes.begin(), bytes.end()));
}

std::string flag(bool b) {
    return b ? "1" : "0";
}

std::string state_line(const remote::State& s) {
    return f64s(s.width) + " " + std::to_string(s.height) + " " + flag(s.dirty) + " " + flag(s.wants_tick) + " " + f64s(s.next_tick_in) + " " + flag(s.frame_asked);
}

std::string claim_line(const twidget::WidgetGestureClaim& c) {
    return flag(c.x) + flag(c.y) + flag(c.hold);
}

const char* OP_NAMES[] = {"Font", "CanvasFont", "DeleteFont", "TextWidth", "Image", "CanvasRun", "CanvasRead", "CanvasRelease", "Frame"};

// A Backend call as crates/ends/tests/common/mod.rs writes Call::line: every member, floats as bits, the ops in their wire form.
std::string call_line(const Call& c) {
    std::string batch;
    wire::encode(c.ops, wire::Caps{}, batch);
    std::string out = OP_NAMES[c.op];
    for (const std::string* s : {&c.families, &c.features, &c.text, &c.url}) out += " " + Json::quote(*s);
    out += " " + f32s(c.size) + " " + std::to_string(c.weight) + " " + flag(c.italic) + " " + std::to_string(c.font);
    out += " " + scripted::metrics_line(c.metrics_in) + " " + scripted::metrics_line(c.metrics) + " " + f32s(c.width);
    for (int v : {c.id, c.x, c.y, c.w, c.h}) out += " " + std::to_string(v);
    for (float v : {c.w_in, c.h_in, c.image_w, c.image_h}) out += " " + f32s(v);
    out += " " + flag(c.ok) + " " + flag(c.painted) + " " + std::to_string(c.rgba_in) + " " + hexs(c.rgba) + " " + hexs(batch);
    return out;
}

struct Queue : remote::Channel {
    std::deque<std::string> messages;

    void send(std::string message) override { messages.push_back(std::move(message)); }
};

// What the platform end gives its host, as lines; a frame also marks the Backend's log.
struct Heard : remote::PlatformEnd::Host {
    explicit Heard(LogBackend& backend) : backend(backend) {}

    void frame(remote::Frame&& f) override {
        backend.frame(f.painted);
        std::string list, damage;
        wire::encode(f.list, wire::Caps{}, list);
        wire::encode(f.damage, wire::Caps{}, damage);
        lines.push_back("frame " + std::to_string(f.seq) + " " + flag(f.painted) + " " + state_line(f.state) + " " + hexs(list) + " " + hexs(damage));
    }
    void state(const remote::State& s) override { lines.push_back("state " + state_line(s)); }
    void messages(std::vector<Json>&& messages) override { lines.push_back("messages " + scripted::dumps(messages)); }
    void pointer(const remote::Pointer& p) override { lines.push_back("pointer " + Json::quote(p.cursor) + " " + (p.claim ? claim_line(*p.claim) : std::string("-"))); }
    void selection(const remote::Selection& s) override { lines.push_back("selection " + Json::quote(s.text)); }
    void claim(const remote::ClaimAnswer& c) override { lines.push_back("claim " + std::to_string(c.seq) + " " + claim_line(c.claim)); }
    void refused(const std::string& reason) override { lines.push_back("refused " + Json::quote(reason)); }

    LogBackend& backend;
    std::vector<std::string> lines;
};

enum class Spoil { None, Unanswered, Empty, WrongKind, WrongVersion, Cut, Trailing, Replaced };

struct Fault {
    Spoil how = Spoil::None;
    size_t at = 0;
    std::optional<remote::Kind> kind;
    std::string reply;
};

// The renderer end's channel, as loopback.h's Wire, writing its traffic down.
struct Wire : remote::RendererChannel {
    explicit Wire(remote::PlatformEnd& platform) : platform(platform) {}

    void send(std::string message) override {
        traffic.push_back("> " + hexs(message));
        outbox.push_back(std::move(message));
    }

    bool request(std::string message, std::string& answer) override {
        const uint8_t asked = message.empty() ? 0 : uint8_t(message[0]);
        traffic.push_back("? " + hexs(message));
        requests++;
        flush();
        const bool hit = fault.how != Spoil::None && (fault.at ? fault.at == requests : (fault.kind && uint8_t(*fault.kind) == asked));
        const Fault now = hit ? std::exchange(fault, Fault()) : Fault();
        if (now.how == Spoil::Unanswered) {
            traffic.push_back("! -");
            return false;
        }
        std::string reply;
        if (!platform.answer(message, reply)) {
            traffic.push_back("! refused");
            return false;
        }
        switch (now.how) {
        case Spoil::Empty: reply.clear(); break;
        case Spoil::WrongKind: reply[0] = char(uint8_t(remote::Kind::FontAnswer) + (uint8_t(reply[0]) - uint8_t(remote::Kind::FontAnswer) + 1) % 4); break;
        case Spoil::WrongVersion: reply[1] = char(remote::VERSION + 1); break;
        case Spoil::Cut: reply.pop_back(); break;
        case Spoil::Trailing: reply.push_back('\0'); break;
        case Spoil::Replaced: reply = now.reply; break;
        default: break;
        }
        traffic.push_back("! " + hexs(reply));
        answer = std::move(reply);
        return true;
    }

    void flush() {
        while (!outbox.empty()) {
            const std::string message = std::move(outbox.front());
            outbox.pop_front();
            platform.receive(message);
        }
    }

    remote::PlatformEnd& platform;
    std::deque<std::string> outbox;
    size_t requests = 0;
    Fault fault;
    std::vector<std::string> traffic;
};

size_t cap(const Json& caps, const char* name, size_t fallback) {
    const Json* v = caps.find(name);
    return v && v->isNumber() ? size_t(v->asNumber(0)) : fallback;
}

remote::Caps caps_of(const Json& j) {
    remote::Caps c;
    c.message = cap(j, "message", c.message);
    c.text = cap(j, "text", c.text);
    c.url = cap(j, "url", c.url);
    c.messages = cap(j, "messages", c.messages);
    c.values.commands = cap(j, "commands", c.values.commands);
    c.values.canvas_ops = cap(j, "canvas_ops", c.values.canvas_ops);
    c.values.pixels = cap(j, "pixels", c.values.pixels);
    c.values.text = cap(j, "value_text", c.values.text);
    c.values.rects = cap(j, "rects", c.values.rects);
    c.values.clips = cap(j, "clips", c.values.clips);
    c.values.floats = cap(j, "floats", c.values.floats);
    c.widths = cap(j, "widths", c.widths);
    c.widths_text = cap(j, "widths_text", c.widths_text);
    c.frame = cap(j, "frame", c.frame);
    c.delta_base = cap(j, "delta_base", c.delta_base);
    return c;
}

// One session: the steps through both ends, and after each what went over the wire, what the renderer, the Backend and the host saw, its results and where the ends stand.
std::string run(const Json& session) {
    const remote::Caps caps = caps_of(session["caps"]);
    const Json& script = session["script"];
    LogBackend backend;
    Heard host(backend);
    Queue queue;
    remote::PlatformEnd platform(backend, host, queue, caps);
    Wire wire(platform);
    std::unique_ptr<remote::RendererEnd> end;
    remote::RendererEnd::Output out;
    g_renderer_log.clear();
    std::string t;
    size_t traffic = 0, logged = 0, calls = 0, heard = 0;
    auto deliveries = [&](bool ok) { t += "= deliver " + flag(ok) + " " + flag(out.frame) + " " + scripted::dumps(out.messages) + "\n"; };
    size_t index = 0;
    for (const Json& step : session["steps"].arr()) {
        const std::string& what = step[size_t(0)].asString();
        std::string result = "= " + what;
        const bool open = end != nullptr;
        if (what == "open") {
            g_renderer_script = &script;
            end = std::make_unique<remote::RendererEnd>(step[size_t(1)].asNumber(0), step[size_t(2)].asNumber(0), step[size_t(3)].asBool(false), wire, caps, nullptr);
            wire.flush();
        } else if (what == "receive" || what == "want_frame" || what == "report" || what == "deliver" || what == "push" || what == "helper" || what == "close") {
            if (!open) {
                result += " none";
            } else if (what == "receive") {
                const std::vector<Json> got = end->receive(step[size_t(1)]);
                wire.flush();
                result += " " + scripted::dumps(got);
            } else if (what == "want_frame") {
                end->want_frame();
                wire.flush();
            } else if (what == "report") {
                end->report(step[size_t(1)].arr());
                wire.flush();
            } else if (what == "deliver") {
                const bool ok = end->deliver(unhex(step[size_t(1)].asString()), out);
                wire.flush();
                result += " " + flag(ok) + " " + flag(out.frame) + " " + scripted::dumps(out.messages);
            } else if (what == "push") {
                end->renderer().push(step[size_t(1)].asString(), step[size_t(2)]);
            } else if (what == "helper") {
                if (end->stopped()) {
                    result += " skipped";
                } else {
                    const std::vector<Json> got = end->renderer().receive(step[size_t(1)]);
                    end->report(got);
                    wire.flush();
                    result += " " + scripted::dumps(got);
                }
            } else {
                end.reset();
                wire.flush();
            }
        } else if (what == "frame") {
            result += " " + std::to_string(platform.request_frame(step[size_t(1)].asBool(false), remote::Paint(std::clamp(step[size_t(2)].asInt(1), 0, 2))));
        } else if (what == "width") {
            platform.set_width(step[size_t(1)].asNumber(0));
        } else if (what == "max") {
            platform.set_max_height(step[size_t(1)].asNumber(0));
        } else if (what == "theme") {
            platform.set_theme(step[size_t(1)].asBool(false));
        } else if (what == "touch") {
            platform.touch(step[size_t(1)].asInt(0), step[size_t(2)].asNumber(0), step[size_t(3)].asNumber(0), step[size_t(4)].asInt(0), step[size_t(5)].asBool(false), step[size_t(6)].asBool(false),
                           step[size_t(7)].asInt(0), step[size_t(8)].asInt(0));
        } else if (what == "hover") {
            platform.hover(step[size_t(1)].asInt(0), step[size_t(2)].asNumber(0), step[size_t(3)].asNumber(0), step[size_t(4)].asInt(0));
        } else if (what == "clear") {
            platform.clear_selection();
        } else if (what == "claim") {
            result += " " + std::to_string(platform.ask_claim(step[size_t(1)].asNumber(0), step[size_t(2)].asNumber(0)));
        } else if (what == "pump") {
            const size_t steps = size_t(std::max(step[size_t(1)].asInt(64), 0));
            wire.flush();
            bool ok = true;
            std::vector<std::pair<bool, remote::RendererEnd::Output>> done;
            for (size_t i = 0; !queue.messages.empty(); i++) {
                if (i == steps || !end) {
                    ok = false;
                    break;
                }
                const std::string message = std::move(queue.messages.front());
                queue.messages.pop_front();
                const bool delivered = end->deliver(message, out);
                done.emplace_back(delivered, out);
                wire.flush();
            }
            result += " " + flag(ok);
            for (const auto& [delivered, output] : done) result += "\n= deliver " + flag(delivered) + " " + flag(output.frame) + " " + scripted::dumps(output.messages);
        } else if (what == "fault") {
            Fault f;
            f.how = Spoil(std::clamp(step[size_t(1)].asInt(0), 0, 7));
            f.at = size_t(std::max(step[size_t(2)].asInt(0), 0));
            if (step[size_t(3)].asInt(0) != 0) f.kind = remote::Kind(uint8_t(step[size_t(3)].asInt(0)));
            f.reply = unhex(step[size_t(4)].asString());
            wire.fault = f;
        } else if (what == "backend") {
            backend.images_load = step[size_t(1)].asBool(true);
            backend.reads = LogBackend::Read(std::clamp(step[size_t(2)].asInt(0), 0, 2));
        } else if (what == "platform_receive") {
            result += " " + flag(platform.receive(unhex(step[size_t(1)].asString())));
        } else if (what == "platform_close") {
            platform.close();
        } else if (what == "platform_answer") {
            std::string reply;
            const bool ok = platform.answer(unhex(step[size_t(1)].asString()), reply);
            result += " " + flag(ok) + " " + hexs(reply);
        } else {
            result += " unknown";
        }
        t += "step " + std::to_string(index++) + " " + step.dump() + "\n";
        for (; traffic < wire.traffic.size(); traffic++) t += wire.traffic[traffic] + "\n";
        for (; logged < g_renderer_log.size(); logged++) t += "R " + g_renderer_log[logged] + "\n";
        for (; calls < backend.calls.size(); calls++) t += "B " + call_line(backend.calls[calls]) + "\n";
        for (; heard < host.lines.size(); heard++) t += "H " + host.lines[heard] + "\n";
        t += result + "\n";
        t += "S " + flag(end != nullptr) + " " + (end ? flag(end->stopped()) + " " + Json::quote(end->stop_reason()) : std::string("-")) + " " + flag(platform.refusing()) + " " + state_line(platform.state()) + " " +
             std::to_string(queue.messages.size()) + "\n";
    }
    end.reset();
    wire.flush();
    for (; traffic < wire.traffic.size(); traffic++) t += wire.traffic[traffic] + "\n";
    for (; logged < g_renderer_log.size(); logged++) t += "R " + g_renderer_log[logged] + "\n";
    for (; calls < backend.calls.size(); calls++) t += "B " + call_line(backend.calls[calls]) + "\n";
    for (; heard < host.lines.size(); heard++) t += "H " + host.lines[heard] + "\n";
    return t;
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::string bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        unsigned char b[4];
        if (!read_exact(b, 4)) return 2;
        const uint32_t n = uint32_t(b[0]) | (uint32_t(b[1]) << 8) | (uint32_t(b[2]) << 16) | (uint32_t(b[3]) << 24);
        bytes.resize(n);
        if (!read_exact(bytes.data(), n)) return 2;
        if (op != 'c') return 3;
        const std::optional<Json> session = Json::parse(bytes, 1024);
        if (!session) return 4;
        answer(1, run(*session));
    }
    std::fflush(stdout);
    return 0;
}
