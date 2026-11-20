// The renderer end against a platform end over an in-memory channel: focused sequences match a renderer called directly step by step, a layout's unknown words, the units its paint spaces and the breaks it takes go in bounded batches, the time and canvas-font seams hold, a missing or bad answer, bad platform input or a value past the caps stops it for good with nothing sent after, and, watching the heap, a paint writes its Frame once and keeps no more than its list, and a closed platform end gives its list back.
#include "platform_end.h"
#include "remote.h"
#include "renderer.h"
#include "renderer_end.h"

#include "log_backend.h"
#include "loopback.h"
#include "reports.h"
#include "wire_compare.h"

#include <litehtml.h>
#include <litehtml/el_text.h>

#include <algorithm>
#include <atomic>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <new>
#include <optional>
#include <set>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace {

struct Watch;

/// The watch of the thread that started it: every other thread's blocks pass it by.
constinit thread_local Watch* watching = nullptr;

/// The heap blocks its thread made while a watch is on and not yet freed, by address, which lets a check read what a call kept; what the watched call asked for; and, while `deny` is set, how often the thread asked and was refused.
struct Watch {
    struct Block {
        void* at = nullptr;
        size_t bytes = 0;
    };
    static constexpr size_t SLOTS = size_t(1) << 18;

    Block blocks[SLOTS];
    char gone = 0;
    bool on = false, full = false, deny = false;
    size_t used = 0, live = 0, alive = 0, asked = 0, made = 0, big = SIZE_MAX, bigs = 0, denied = 0;

    static size_t slot(const void* p) { return size_t((uint64_t(reinterpret_cast<uintptr_t>(p)) >> 4) * 0x9E3779B97F4A7C15ull >> 46); }

    void start(size_t at_least = SIZE_MAX) {
        for (Block& b : blocks) b = Block();
        watching = this;
        on = true;
        full = deny = false;
        used = live = alive = asked = made = bigs = denied = 0;
        big = at_least;
    }
    void stop() { on = false; }
    void noted(void* p, size_t n) {
        if (!on || !p) return;
        asked += n;
        made++;
        bigs += n >= big;
        if (used >= SLOTS / 2) {
            full = true;
            return;
        }
        size_t i = slot(p);
        while (blocks[i].at && blocks[i].at != &gone) i = (i + 1) % SLOTS;
        blocks[i] = {p, n};
        used++;
        live += n;
        alive++;
    }
    void freed(void* p) {
        if (!alive || !p) return;
        for (size_t i = slot(p); blocks[i].at; i = (i + 1) % SLOTS) {
            if (blocks[i].at != p) continue;
            live -= blocks[i].bytes;
            alive--;
            blocks[i] = {&gone, 0};
            return;
        }
    }
};

Watch watch;

void* made(size_t n) noexcept {
    Watch* w = watching;
    if (w && w->deny) {
        w->denied++;
        return nullptr;
    }
    void* p = std::malloc(n ? n : 1);
    if (w) w->noted(p, n);
    return p;
}

void unmade(void* p) noexcept {
    if (Watch* w = watching) w->freed(p);
    std::free(p);
}

} // namespace

void* operator new(size_t n) {
    if (void* p = made(n)) return p;
    throw std::bad_alloc();
}

void* operator new[](size_t n) {
    if (void* p = made(n)) return p;
    throw std::bad_alloc();
}

void* operator new(size_t n, const std::nothrow_t&) noexcept {
    return made(n);
}

void* operator new[](size_t n, const std::nothrow_t&) noexcept {
    return made(n);
}

void operator delete(void* p) noexcept {
    unmade(p);
}

void operator delete[](void* p) noexcept {
    unmade(p);
}

void operator delete(void* p, size_t) noexcept {
    unmade(p);
}

void operator delete[](void* p, size_t) noexcept {
    unmade(p);
}

void operator delete(void* p, const std::nothrow_t&) noexcept {
    unmade(p);
}

void operator delete[](void* p, const std::nothrow_t&) noexcept {
    unmade(p);
}

using twidget::Json;

namespace {

int checks = 0, failures = 0;
size_t stop_cases = 0;

void check(bool ok, const std::string& what) {
    checks++;
    if (!ok) {
        std::printf("FAIL %s\n", what.c_str());
        failures++;
    }
}

/// Stands still until a test moves it, so both renderers of a comparison read the same times.
struct VirtualClock : Clock {
    double ms = 1000;

    double now_ms() override { return ms; }
};

/// Moves `step` ms on at each read, as time passes inside a call.
struct SteppingClock : Clock {
    double ms = 1000, step = 0;

    double now_ms() override {
        const double now = ms;
        ms += step;
        return now;
    }
};

Json parse(const std::string& s) {
    std::optional<Json> json = Json::parse(s);
    if (!json) {
        std::printf("bad json: %s\n", s.c_str());
        std::exit(2);
    }
    return *json;
}

template <class M> std::string encoded(const M& message) {
    std::string bytes;
    if (!remote::encode(message, {}, bytes)) {
        std::printf("a test message did not encode\n");
        std::exit(2);
    }
    return bytes;
}

remote::State state_of(const Renderer& r, bool frame_asked) {
    return remote::State{r.width(), r.viewport_height(), r.dirty(), r.wants_tick(), r.next_tick_in(), frame_asked};
}

remote::RendererEnd::Output stale_output() {
    remote::RendererEnd::Output out;
    out.messages.push_back(parse(R"({"type":"event","ev":{"type":"stale"}})"));
    out.frame = true;
    return out;
}

/// The base64 of `bytes` zero bytes: an imageData answer the renderer zero-filled.
std::string zero_base64(size_t bytes) {
    std::string out;
    for (size_t i = 0; i < bytes / 3; i++) out += "AAAA";
    if (bytes % 3 == 1) out += "AA==";
    if (bytes % 3 == 2) out += "AAA=";
    return out;
}

bool empty(const remote::RendererEnd::Output& out) {
    return out.messages.empty() && !out.frame;
}

size_t count(const std::vector<remote::Kind>& kinds, remote::Kind kind) {
    size_t n = 0;
    for (remote::Kind k : kinds) n += k == kind;
    return n;
}

size_t requests_made(const LogBackend& backend) {
    return backend.count(Call::Font) + backend.count(Call::CanvasFont) + backend.count(Call::TextWidth) + backend.count(Call::Image) + backend.count(Call::CanvasRead);
}

/// html(2) head(3) body(4); a text (11) in a div (10), a PNG (12) and a canvas (13) that draws text in its own font.
const char* PAGE = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[]},
 {"op":"text","id":11,"data":"two ends"},
 {"op":"el","id":12,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgo="]]},
 {"op":"el","id":13,"tag":"canvas","attrs":[["width","40"],["height","20"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},
 {"op":"ins","parent":4,"id":13,"before":null},
 {"op":"canvas","id":13,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10],["s","fillStyle","#336699"],["c","fillRect",0,0,5,5]]}
]})";

const char* PIXELS = R"({"type":"query","qid":1,"query":{"q":"imageData","id":13,"x":0,"y":0,"w":2,"h":1}})";

/// The same body with an element that spins for ever: a batch that changes the document then waits for the next frame.
const char* SPINNING = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":20,"tag":"style","attrs":[]},
 {"op":"css","id":20,"text":"@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .spin{width:20px;height:20px;animation:spin 1s linear infinite}"},
 {"op":"ins","parent":3,"id":20,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["class","spin"]]},
 {"op":"ins","parent":4,"id":10,"before":null}
]})";

const char* FRESH_TEXT = R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"div","attrs":[]},{"op":"text","id":31,"data":"words measured at the next frame"},
 {"op":"ins","parent":30,"id":31,"before":null},{"op":"ins","parent":4,"id":30,"before":null}]})";

/// Once stopped the end answers nothing, sends nothing and calls nothing, and goes without a word.
void inert(Loopback& loop, const std::string& what) {
    stop_cases++;
    const std::string reason = loop.end->stop_reason();
    const size_t sent = loop.wire.kinds.size(), calls = loop.backend.calls.size(), heard = loop.host.events.size();
    check(loop.end->stopped() && !reason.empty(), what + ": the end says it stopped");
    check(loop.receive(parse(PAGE)).empty(), what + ": an engine message gets nothing back");
    loop.want_frame();
    loop.end->report({parse(R"({"type":"size","width":1,"height":2})")});
    remote::RendererEnd::Output out = stale_output();
    check(!loop.end->deliver(encoded(remote::FrameRequest{9, true, remote::Paint::Force}), out) && empty(out), what + ": a frame request gets false and an empty output");
    loop.platform.set_width(200);
    loop.platform.touch(0, 5, 5, 1, true);
    loop.platform.request_frame(true, remote::Paint::Force);
    loop.platform.ask_claim(5, 5);
    const size_t delivered = loop.delivered.size();
    loop.out = stale_output();
    check(loop.pump(), what + ": the platform input drains");
    bool refused = loop.delivered.size() == delivered + 4;
    for (size_t i = delivered; refused && i < loop.delivered.size(); i++) refused = !loop.delivered[i] && empty(loop.outputs[i]);
    check(refused, what + ": each platform input gets false and an empty output");
    check(loop.wire.kinds.size() == sent && loop.wire.outbox.empty() && loop.backend.calls.size() == calls && loop.host.events.size() == heard,
          what + ": nothing goes out once stopped");
    check(loop.end->stopped() && loop.end->stop_reason() == reason, what + ": the first reason stays");
    loop.close();
    check(loop.wire.kinds.size() == sent && loop.backend.calls.size() == calls && loop.host.events.size() == heard, what + ": the end goes without a word");
    check(loop.host.refusals.empty() && loop.wire.refused == 0 && !loop.platform.refusing(), what + ": the platform end took all it was sent");
}

/// The spoiled request was the last message of the call, and the end gives the expected reason.
void stopped_at_spoil(const Loopback& loop, remote::Kind kind, const std::string& reason, const std::string& what) {
    const size_t at = loop.wire.spoiled;
    check(at > 0 && loop.wire.kinds[at - 1] == kind, what + ": the spoiled request is the one named");
    check(loop.wire.kinds.size() == at, what + ": nothing goes out after the spoiled request (" + std::to_string(loop.wire.kinds.size() - at) + " messages did)");
    check(loop.end->stopped() && loop.end->stop_reason() == reason, what + ": stops with \"" + reason + "\", not \"" + loop.end->stop_reason() + "\"");
}

const Spoil SPOILS[] = {Spoil::Unanswered, Spoil::Empty, Spoil::WrongKind, Spoil::WrongVersion, Spoil::Cut, Spoil::Trailing};

std::string spoil_name(Spoil how) {
    switch (how) {
    case Spoil::Unanswered: return "unanswered";
    case Spoil::Empty: return "answered empty";
    case Spoil::WrongKind: return "answered as another request";
    case Spoil::WrongVersion: return "answered in another version";
    case Spoil::Cut: return "answered one byte short";
    case Spoil::Trailing: return "answered with a byte after";
    case Spoil::Replaced: return "answered otherwise";
    default: return "answered";
    }
}

std::string reason_for(Spoil how) {
    return how == Spoil::Unanswered ? "a request got no answer" : "an answer did not decode";
}

std::string kind_name(remote::Kind kind) {
    switch (kind) {
    case remote::Kind::FontRequest: return "a font request";
    case remote::Kind::WidthRequest: return "a width request";
    case remote::Kind::ImageRequest: return "an image request";
    case remote::Kind::PixelsRequest: return "a pixels request";
    case remote::Kind::WidthsRequest: return "a widths request";
    default: return "kind " + std::to_string(int(kind));
    }
}

/// A healthy session against a renderer called directly: construction, a page, pixels, a frame the engine asked for, an unchanged frame and destruction.
void session() {
    VirtualClock direct_clock, clock;
    LogBackend direct_backend;
    auto direct = std::make_unique<Renderer>(300, 2, false, direct_backend, &direct_clock);
    Loopback loop;
    loop.open(300, 2, false, &clock);
    std::string why;
    check(!loop.end->stopped() && loop.host.refusals.empty(), "the end starts healthy");
    check(!loop.backend.calls.empty() && same_calls(direct_backend.calls, loop.backend.calls, why), "construction makes the direct renderer's Backend calls through the platform end " + why);
    check(loop.wire.requests > 0 && loop.wire.requests == requests_made(loop.backend), "construction's requests are answered, one Backend call each");
    check(loop.host.states.size() == 1 && same_state(loop.host.states[0], state_of(*direct, false)) && same_state(loop.platform.state(), loop.host.states[0]),
          "the first State is the renderer's state");
    check(loop.wire.kinds.back() == remote::Kind::State && loop.host.events == std::vector<remote::Kind>{remote::Kind::State}, "the State is the constructor's last message and its only report");

    const std::vector<Json> direct_page = direct->receive(parse(PAGE));
    const std::vector<Json> page = loop.receive(parse(PAGE));
    check(!page.empty() && dumps(page) == dumps(direct_page), "receive gives every renderer message of the page");
    check(loop.host.batches.size() == 1 && dumps(loop.host.batches[0]) == dumps(platform_only(direct_page)) && !loop.host.batches[0].empty(), "the platform gets the page's platform messages alone");
    check(same_calls(direct_backend.calls, loop.backend.calls, why), "the page's Backend calls arrive in order " + why);
    check(loop.backend.count(Call::Font) > 0 && loop.backend.count(Call::CanvasFont) == 1 && loop.backend.count(Call::TextWidth) > 0 && loop.backend.count(Call::Image) == 1
              && loop.backend.count(Call::CanvasRun) > 0,
          "the page makes document fonts, a marked canvas font, widths, an image and canvas batches");

    const std::vector<Json> direct_pixels = direct->receive(parse(PIXELS));
    const std::vector<Json> pixels = loop.receive(parse(PIXELS));
    check(pixels.size() == 1 && dumps(pixels) == dumps(direct_pixels) && !pixels[0]["binaryBase64"].asString().empty(), "an imageData query answers with the platform's pixels");
    check(loop.backend.count(Call::CanvasRead) == 1 && same_calls(direct_backend.calls, loop.backend.calls, why), "the pixels are read once, in order " + why);

    loop.want_frame();
    check(!loop.host.states.empty() && loop.host.events.back() == remote::Kind::State && loop.host.states.back().frame_asked, "the engine's want of a frame goes out as State");
    const uint64_t seq = loop.platform.request_frame(true, remote::Paint::IfDirty);
    loop.out = stale_output();
    check(loop.pump() && loop.delivered == std::vector<bool>{true}, "the frame request is delivered");
    const std::vector<Json> ticked = direct->tick();
    const bool dirty = direct->dirty();
    const DisplayList list = direct->paint();
    std::vector<Rect> rects;
    wire::Damage damage;
    if (direct->damage(rects)) damage = rects;
    direct_backend.frame(true);
    check(loop.outputs.size() == 1 && loop.outputs[0].frame && dumps(loop.outputs[0].messages) == dumps(ticked), "the ticking frame gives the tick's messages and says the engine's frame is due");
    check(dirty && loop.host.frames.size() == 1 && loop.host.frames[0].seq == seq && loop.host.frames[0].painted && !list.empty(), "the dirty page is painted");
    if (loop.host.frames.size() == 1) {
        const remote::Frame& frame = loop.host.frames[0];
        check(same(list, frame.list, why), "the frame's list is the direct paint " + why);
        check(same(damage, frame.damage, why), "the frame's damage is the direct damage " + why);
        check(!frame.state.frame_asked && same_state(frame.state, state_of(*direct, false)) && same_state(loop.platform.state(), frame.state),
              "the frame carries the state after it, the engine's want taken");
    }
    check(loop.host.events.back() == remote::Kind::Frame, "no State follows the frame that carried it");
    check(same_calls(direct_backend.calls, loop.backend.calls, why), "the frame's Backend calls come before its marker " + why);

    loop.platform.request_frame(false, remote::Paint::IfDirty);
    check(loop.pump() && loop.delivered.size() == 2 && loop.delivered[1] && empty(loop.outputs[1]), "an unchanged frame reuses the Output with nothing of the frame before");
    direct_backend.frame(false);
    check(loop.host.frames.size() == 2 && !loop.host.frames[1].painted && loop.host.frames[1].list.empty() && !loop.host.frames[1].damage, "an unchanged frame is answered unpainted");
    check(same_calls(direct_backend.calls, loop.backend.calls, why), "the unchanged frame makes no Backend call " + why);
    const size_t heard = loop.host.events.size();
    loop.platform.clear_selection();
    check(loop.pump() && loop.delivered.size() == 3 && loop.delivered[2] && loop.host.events.size() == heard, "after the frames a call that changes nothing sends no report");

    const size_t before = loop.backend.calls.size();
    direct.reset();
    loop.close();
    check(same_calls(direct_backend.calls, loop.backend.calls, why), "destruction releases what the direct renderer releases, in its order " + why);
    std::vector<Call::Op> tail;
    int canvas_font = 0, first_deleted = 0;
    for (size_t i = 0; i < loop.backend.calls.size(); i++) {
        const Call& c = loop.backend.calls[i];
        if (c.op == Call::CanvasFont) canvas_font = c.font;
        if (i < before) continue;
        if (c.op == Call::DeleteFont && !first_deleted) first_deleted = c.font;
        if (tail.empty() || tail.back() != c.op) tail.push_back(c.op);
    }
    check(tail == std::vector<Call::Op>{Call::CanvasRelease, Call::DeleteFont} && first_deleted == canvas_font && loop.backend.live_fonts() == 0,
          "destruction releases the canvas, then its font, then every document font");
    check(loop.host.refusals.empty() && loop.wire.refused == 0 && !loop.platform.refusing(), "the platform end takes the whole session");
}

/// The first request, made while the renderer is constructed, fails every way: the end stops before its first State.
void construction_failures() {
    for (Spoil how : SPOILS) {
        const std::string what = "request 1 " + spoil_name(how);
        VirtualClock clock;
        Loopback loop;
        loop.wire.fault = {how, 1, std::nullopt, {}};
        loop.open(300, 1, false, &clock);
        stopped_at_spoil(loop, remote::Kind::FontRequest, reason_for(how), what);
        check(loop.wire.kinds.size() == 1 && loop.host.events.empty(), what + ": the constructor sends nothing after it");
        inert(loop, what);
    }
}

/// A later request of each kind fails every way inside an engine message, which then gets nothing back.
void later_failures() {
    const remote::Kind kinds[] = {remote::Kind::FontRequest, remote::Kind::WidthRequest, remote::Kind::ImageRequest, remote::Kind::PixelsRequest, remote::Kind::WidthsRequest};
    std::vector<std::pair<Fault, std::string>> faults;
    for (remote::Kind kind : kinds)
        for (Spoil how : SPOILS) faults.push_back({Fault{how, 0, kind, {}}, kind_name(kind) + " " + spoil_name(how)});
    faults.push_back({Fault{Spoil::Replaced, 0, remote::Kind::PixelsRequest, encoded(remote::PixelsAnswer{true, std::vector<uint8_t>(4, 9)})}, "a pixels request answered with 4 bytes for 8"});
    faults.push_back({Fault{Spoil::Replaced, 0, remote::Kind::FontRequest, encoded(remote::WidthAnswer{8})}, "a font request answered with a width"});
    faults.push_back({Fault{Spoil::Replaced, 0, remote::Kind::WidthsRequest, encoded(remote::WidthsAnswer{{8}})}, "a widths request answered with one width for several texts"});
    faults.push_back({Fault{Spoil::Replaced, 0, remote::Kind::WidthsRequest, encoded(remote::WidthsAnswer{})}, "a widths request answered with no widths"});
    faults.push_back({Fault{Spoil::Replaced, 0, remote::Kind::WidthsRequest, encoded(remote::WidthAnswer{8})}, "a widths request answered with a single width"});
    for (const auto& [fault, what] : faults) {
        VirtualClock clock;
        Loopback loop;
        loop.open(300, 1, false, &clock);
        const bool pixels = *fault.kind == remote::Kind::PixelsRequest;
        if (pixels) loop.receive(parse(PAGE));
        const size_t states = loop.host.states.size();
        loop.wire.fault = fault;
        check(loop.receive(parse(pixels ? PIXELS : PAGE)).empty(), what + ": the engine message gets nothing back");
        stopped_at_spoil(loop, *fault.kind, reason_for(fault.how), what);
        check(loop.host.states.size() == states && loop.host.batches.size() == (pixels ? 1 : 0), what + ": no report of the failed call goes out");
        inert(loop, what);
    }
}

/// A frame, a touch, a hover and a hold query each settle a batch that waited for the frame, whose widths request goes unanswered.
void mid_call_failures() {
    const char* names[] = {"a ticking frame", "a touch", "a hover", "a hold query"};
    for (int call = 0; call < 4; call++) {
        const std::string what = std::string(names[call]) + " whose build loses its answer";
        VirtualClock clock;
        Loopback loop;
        loop.open(300, 1, false, &clock);
        loop.receive(parse(SPINNING));
        const size_t widths = count(loop.wire.kinds, remote::Kind::WidthsRequest);
        loop.receive(parse(FRESH_TEXT));
        check(count(loop.wire.kinds, remote::Kind::WidthsRequest) == widths && loop.end->renderer().wants_tick() && !loop.end->stopped(),
              what + ": the batch waits for the frame");
        loop.want_frame();
        loop.wire.fault = {Spoil::Unanswered, 0, remote::Kind::WidthsRequest, {}};
        if (call == 0) loop.platform.request_frame(true, remote::Paint::Force);
        if (call == 1) loop.platform.touch(0, 5, 5, 1, true);
        if (call == 2) loop.platform.hover(0, 5, 5);
        if (call == 3) loop.platform.ask_claim(5, 5);
        loop.out = stale_output();
        const size_t frames = loop.backend.count(Call::Frame), runs = loop.backend.count(Call::CanvasRun);
        check(loop.pump() && loop.delivered == std::vector<bool>{false} && empty(loop.outputs[0]) && empty(loop.out), what + ": the delivery gets false and an empty output, the engine's frame not due");
        stopped_at_spoil(loop, remote::Kind::WidthsRequest, "a request got no answer", what);
        check(loop.host.frames.empty() && loop.host.pointers.empty() && loop.host.claims.empty() && loop.backend.count(Call::Frame) == frames && loop.backend.count(Call::CanvasRun) == runs,
              what + ": no paint, frame, pointer or claim follows");
        check(call != 0 || loop.end->renderer().dirty(), what + ": the frame leaves its paint undone");
        inert(loop, what);
    }
}

/// What a platform end never sends, or sends broken, stops the end at once.
void bad_input() {
    const std::string frame_request = encoded(remote::FrameRequest{1, true, remote::Paint::Force});
    const std::string none = "a platform message of no known kind or version", broken = "a platform message that did not decode";
    std::string version = frame_request, paint = frame_request, theme = encoded(remote::SetTheme{true});
    version[1] = char(remote::VERSION + 1);
    paint.back() = char(3);
    theme.back() = char(2);
    struct Bad {
        std::string what, bytes, reason;
    };
    const std::vector<Bad> cases = {
        {"an empty message", "", none},
        {"a kind byte alone", frame_request.substr(0, 1), none},
        {"an unknown kind", std::string("\x09\x01", 2), none},
        {"a frame request of another version", version, none},
        {"a frame request one byte short", frame_request.substr(0, frame_request.size() - 1), broken},
        {"a frame request with a byte after it", frame_request + '\0', broken},
        {"a frame request painting past Force", paint, broken},
        {"a theme flag of 2", theme, broken},
        {"a clear selection with a byte after it", encoded(remote::ClearSelection{}) + '\0', broken},
        {"a State", encoded(remote::State{}), "a message of kind 65 where platform input belongs"},
        {"a CanvasRelease", encoded(remote::CanvasRelease{1}), "a message of kind 72 where platform input belongs"},
        {"a FontAnswer outside a request", encoded(remote::FontAnswer{}), "a message of kind 32 where platform input belongs"},
        {"a PixelsAnswer outside a request", encoded(remote::PixelsAnswer{}), "a message of kind 35 where platform input belongs"},
        {"a FontRequest", encoded(remote::FontRequest{}), "a message of kind 96 where platform input belongs"},
    };
    for (const Bad& bad : cases) {
        VirtualClock clock;
        Loopback loop;
        loop.open(300, 1, false, &clock);
        const size_t sent = loop.wire.kinds.size();
        remote::RendererEnd::Output out = stale_output();
        check(!loop.end->deliver(bad.bytes, out) && empty(out), bad.what + ": false and an empty output");
        check(loop.end->stop_reason() == bad.reason, bad.what + ": stops with \"" + bad.reason + "\", not \"" + loop.end->stop_reason() + "\"");
        check(loop.wire.kinds.size() == sent, bad.what + ": nothing goes out");
        inert(loop, bad.what);
    }
    VirtualClock clock;
    Loopback loop;
    loop.open(300, 1, false, &clock);
    remote::RendererEnd::Output out = stale_output();
    check(loop.end->deliver(frame_request, out) && !out.frame && out.messages.empty() && !loop.end->stopped(), "the same frame request whole is taken, into a fresh Output");
}

/// A value past the caps stops the end: a painted list during a frame the engine asked for, a report, a request's text.
void cap_failures() {
    {
        const std::string what = "a painted list past the command cap";
        remote::Caps caps;
        caps.values.commands = 1;
        VirtualClock clock;
        Loopback loop(caps);
        loop.open(300, 1, false, &clock);
        loop.receive(parse(PAGE));
        loop.want_frame();
        loop.platform.request_frame(true, remote::Paint::Force);
        loop.out = stale_output();
        check(!loop.end->stopped() && loop.pump() && loop.delivered == std::vector<bool>{false} && empty(loop.outputs[0]), what + ": the frame gets false and an empty output, the engine's frame not due");
        check(loop.end->stop_reason() == "a message past the caps" && count(loop.wire.kinds, remote::Kind::Frame) == 0 && loop.host.frames.empty(), what + ": stops past the caps with no frame sent");
        inert(loop, what);
    }
    {
        const std::string what = "a report past the count cap";
        remote::Caps caps;
        caps.messages = 0;
        VirtualClock clock;
        Loopback loop(caps);
        loop.open(300, 1, false, &clock);
        check(!loop.end->stopped() && loop.receive(parse(PAGE)).empty(), what + ": the page gets nothing back");
        check(loop.end->stop_reason() == "a message past the caps" && loop.host.batches.empty() && count(loop.wire.kinds, remote::Kind::Messages) == 0, what + ": stops past the caps with no report sent");
        inert(loop, what);
    }
    {
        remote::Caps caps;
        caps.url = 16;
        VirtualClock clock;
        Loopback loop(caps);
        loop.open(300, 1, false, &clock);
        const std::vector<Json> page = loop.receive(parse(PAGE));
        bool failed = false;
        for (const Json& m : page) failed = failed || (m["type"].asString() == "event" && m["ev"]["type"].asString() == "error" && m["ev"]["targetId"].asInt() == 12);
        check(!loop.end->stopped() && failed && count(loop.wire.kinds, remote::Kind::ImageRequest) == 0 && loop.backend.count(Call::Image) == 0,
              "an image URL past the cap fails to load without a request, and the end goes on");
    }
    {
        remote::Caps pixels, room;
        pixels.values.pixels = 4;
        room.message = size_t(64) << 10;
        const struct {
            std::string what;
            remote::Caps caps;
            std::string query;
            size_t bytes;
        } cases[] = {
            {"pixels past the pixel cap", pixels, PIXELS, 8},
            {"pixels past the message room", room, R"({"type":"query","qid":1,"query":{"q":"imageData","id":13,"x":0,"y":0,"w":120,"h":140}})", 120 * 140 * 4},
        };
        for (const auto& c : cases) {
            VirtualClock clock;
            Loopback loop(c.caps);
            loop.open(300, 1, false, &clock);
            loop.receive(parse(PAGE));
            const size_t asked = count(loop.wire.kinds, remote::Kind::PixelsRequest);
            const std::vector<Json> answer = loop.receive(parse(c.query));
            check(!loop.end->stopped() && !loop.platform.refusing() && loop.host.refusals.empty(), c.what + ": both ends go on");
            check(count(loop.wire.kinds, remote::Kind::PixelsRequest) == asked + 1 && loop.backend.count(Call::CanvasRead) == 0, c.what + ": the request goes out and the platform reads nothing");
            check(answer.size() == 1 && answer[0]["binaryBase64"].asString() == zero_base64(c.bytes), c.what + ": the renderer zero-fills the pixels it was not given");
        }
    }
    {
        const std::string what = "a width request past the text cap";
        remote::Caps caps;
        caps.text = 64;
        VirtualClock clock;
        Loopback loop(caps);
        loop.open(300, 1, false, &clock);
        const std::string word(100, 'w');
        check(!loop.end->stopped(), what + ": the end starts");
        const std::string ops = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},)"
            R"({"op":"text","id":11,"data":")" + word + R"("},{"op":"ins","parent":4,"id":11,"before":null}]})";
        check(loop.receive(parse(ops)).empty(), what + ": the batch gets nothing back");
        bool measured = false;
        for (const Call& c : loop.backend.calls) measured = measured || c.text == word;
        check(loop.end->stop_reason() == "a request past the caps" && !measured, what + ": stops past the caps before the platform measures it");
        inert(loop, what);
    }
}

// ───────────────────────── a renderer called directly against the two ends ─────────────────────────

bool has_event(const std::vector<Json>& messages, const std::string& type) {
    for (const Json& m : messages)
        if (m["type"].asString() == "event" && m["ev"]["type"].asString() == type) return true;
    return false;
}

const Json* message_of(const std::vector<Json>& messages, const std::string& type) {
    for (const Json& m : messages)
        if (m["type"].asString() == type) return &m;
    return nullptr;
}

std::string query(int qid, const std::string& q) {
    return R"({"type":"query","qid":)" + std::to_string(qid) + R"(,"query":)" + q + "}";
}

const twidget::WidgetGestureClaim NO_CLAIM{}, HOLD_CLAIM{true, false, true}, DRAG_CLAIM{true, false, false};

const char* CALL_NAMES[] = {"document fonts", "canvas fonts", "font releases", "widths", "images", "canvas runs", "pixel reads", "canvas releases"};

/// What a session made happen: Backend calls by kind, those of construction, of destruction and the one-way ones between frames, the host's reports, frames and the helper's own messages.
struct Tally {
    size_t steps = 0, construction = 0, destruction = 0, painted = 0, unchanged = 0, helper_only = 0;
    size_t calls[Call::Frame] = {}, between[Call::Frame] = {};
    size_t messages = 0, pointers = 0, selections = 0, frames = 0, claims = 0, states = 0;

    void add(const Tally& t) {
        steps += t.steps;
        construction += t.construction;
        destruction += t.destruction;
        painted += t.painted;
        unchanged += t.unchanged;
        helper_only += t.helper_only;
        for (int op = 0; op < Call::Frame; op++) {
            calls[op] += t.calls[op];
            between[op] += t.between[op];
        }
        messages += t.messages;
        pointers += t.pointers;
        selections += t.selections;
        frames += t.frames;
        claims += t.claims;
        states += t.states;
    }

    size_t one_way_between() const { return between[Call::DeleteFont] + between[Call::CanvasRun] + between[Call::CanvasRelease]; }

    void print(const std::string& name) const {
        std::printf("%s: %zu steps; Backend", name.c_str(), steps);
        for (int op = 0; op < Call::Frame; op++) std::printf(" %zu %s,", calls[op], CALL_NAMES[op]);
        std::printf(" %zu in construction, %zu in destruction, between frames %zu font releases, %zu canvas runs, %zu canvas releases;", construction, destruction, between[Call::DeleteFont],
                    between[Call::CanvasRun], between[Call::CanvasRelease]);
        std::printf(" host %zu Messages, %zu Pointer, %zu Selection, %zu Frame (%zu painted, %zu unchanged), %zu ClaimAnswer, %zu State; %zu messages for the helper alone\n", messages, pointers,
                    selections, frames, painted, unchanged, claims, states, helper_only);
    }
};

Tally total;

/// One focused sequence made of calls, each on a renderer called directly, which gives what is expected, and then through the two ends; every step compares the two at once.
struct Arms {
    using Step = Expected;

    explicit Arms(std::string session, const remote::Caps& caps = {}) : name(std::move(session)), loop(caps) {
        label = "construction";
        direct = std::make_unique<Renderer>(300, 2, false, direct_backend, &direct_clock);
        loop.open(300, 2, false, &clock);
        tally.construction = direct_backend.calls.size();
        std::string why;
        ok(!loop.backend.calls.empty() && same_calls(direct_backend.calls, loop.backend.calls, why), "construction makes the direct renderer's Backend calls " + why);
        sent = state_of(*direct, false);
        ok(loop.host.events == std::vector<remote::Kind>{remote::Kind::State} && same_state(loop.host.states.back(), sent) && same_state(loop.platform.state(), sent),
           "the constructor's one report is the direct renderer's State");
        healthy();
    }

    void ok(bool cond, const std::string& what) { check(cond, name + " step " + std::to_string(tally.steps) + " (" + label + "): " + what); }

    void need(size_t n, const std::string& what) { check(n > 0, name + ": the session has " + what); }

    void healthy() { ::healthy([&](bool cond, const std::string& what) { ok(cond, what); }, loop); }

    /// The reads finish() makes after the call, in its order, then the frame's marker; and a real pause before the other arm when the session asks for one.
    void expect(Step& s) {
        s.selection = direct->may_have_selection() ? direct->selected_text() : std::string();
        s.state = state_of(*direct, asked);
        predict(s, selection, sent);
        if (s.frame) direct_backend.frame(s.frame->painted);
        if (lag_ms > 0) std::this_thread::sleep_for(std::chrono::duration<double, std::milli>(lag_ms));
    }

    void compare(const Step& s, size_t heard_before, const std::vector<Json>& returned, const remote::RendererEnd::Output* out) {
        std::string why;
        healthy();
        compare_reports([&](bool cond, const std::string& what) { ok(cond, what); }, s, loop, heard_before, returned, out);
        ok(same_calls(direct_backend.calls, loop.backend.calls, why), "the Backend calls are the direct renderer's, in order " + why);
        selection = s.selection;
        sent = s.state;
        for (const Json& m : s.messages) tally.helper_only += !platform_type(m);
    }

    void begin(const std::string& what) {
        tally.steps++;
        label = what;
    }

    /// Platform input: `call` makes the direct call and its reads into the step, `send` hands the same input to the platform end and returns its seq.
    template <class Call, class Send> Step input(const std::string& what, Call call, Send send) {
        begin(what);
        Step s;
        call(s);
        expect(s);
        const size_t heard = loop.host.events.size(), delivered = loop.delivered.size();
        s.seq = send();
        ok(loop.pump() && loop.delivered.size() == delivered + 1 && loop.delivered.back(), "the platform input is delivered and taken");
        const remote::RendererEnd::Output out = loop.outputs.empty() ? remote::RendererEnd::Output() : loop.outputs.back();
        compare(s, heard, out.messages, &out);
        return s;
    }

    Step receive(const std::string& json, const std::string& what) {
        begin(what);
        const Json message = parse(json);
        Step s;
        s.messages = direct->receive(message);
        expect(s);
        const size_t heard = loop.host.events.size();
        const std::vector<Json> got = loop.receive(message);
        compare(s, heard, got, nullptr);
        return s;
    }

    Step want_frame(const std::string& what) {
        begin(what);
        Step s;
        asked = true;
        expect(s);
        const size_t heard = loop.host.events.size(), frames = loop.host.frames.size();
        loop.want_frame();
        compare(s, heard, {}, nullptr);
        ok(loop.host.frames.size() == frames, "the engine's want of a frame makes no frame for the host");
        return s;
    }

    Step frame(const std::string& what, bool tick, remote::Paint paint) {
        requested++;
        return input(what, [&](Step& s) {
            if (tick) {
                s.messages = direct->tick();
                s.engine = std::exchange(asked, false);
            }
            remote::Frame f;
            if (paint == remote::Paint::Force || (paint == remote::Paint::IfDirty && direct->dirty())) {
                f.list = direct->paint();
                std::vector<Rect> rects;
                if (direct->damage(rects)) f.damage = std::move(rects);
                f.painted = true;
            }
            s.frame = std::move(f);
        }, [&] { return loop.platform.request_frame(tick, paint); });
    }

    Step touch(const std::string& what, int action, double x, double y, int pointer, bool primary, bool mouse = false, int modifiers = 0, int button = 0) {
        return input(what, [&](Step& s) {
            s.messages = direct->touch(action, x, y, pointer, primary, mouse, modifiers, button);
            s.pointer = remote::Pointer{direct->cursor(), gesture_of(s.messages)};
        }, [&] {
            loop.platform.touch(action, x, y, pointer, primary, mouse, modifiers, button);
            return uint64_t(0);
        });
    }

    Step hover(const std::string& what, int action, double x, double y, int modifiers = 0) {
        return input(what, [&](Step& s) {
            s.messages = direct->hover(action, x, y, modifiers);
            s.pointer = remote::Pointer{direct->cursor(), std::nullopt};
        }, [&] {
            loop.platform.hover(action, x, y, modifiers);
            return uint64_t(0);
        });
    }

    Step set_width(const std::string& what, double width) {
        return input(what, [&](Step& s) { direct->set_width(width, s.messages); }, [&] {
            loop.platform.set_width(width);
            return uint64_t(0);
        });
    }

    Step set_max_height(const std::string& what, double height) {
        return input(what, [&](Step& s) { direct->set_max_height(height, s.messages); }, [&] {
            loop.platform.set_max_height(height);
            return uint64_t(0);
        });
    }

    Step set_theme(const std::string& what, bool dark) {
        return input(what, [&](Step& s) { direct->set_theme(dark, s.messages); }, [&] {
            loop.platform.set_theme(dark);
            return uint64_t(0);
        });
    }

    Step clear_selection(const std::string& what) {
        return input(what, [&](Step&) { direct->clear_selection(); }, [&] {
            loop.platform.clear_selection();
            return uint64_t(0);
        });
    }

    Step claim(const std::string& what, double x, double y) {
        return input(what, [&](Step& s) { s.claim = direct->claim_at(x, y); }, [&] { return loop.platform.ask_claim(x, y); });
    }

    /// A call the channel does not carry, made on each renderer by the helper, which then hands the end's renderer messages to report().
    template <class Fn> Step helper(const std::string& what, Fn fn) {
        begin(what);
        Step s;
        s.messages = fn(*direct);
        expect(s);
        const size_t heard = loop.host.events.size();
        ok(!loop.end->stopped(), "the helper calls the renderer of a healthy end");
        const std::vector<Json> got = fn(loop.end->renderer());
        loop.end->report(got);
        loop.wire.flush();
        compare(s, heard, got, nullptr);
        return s;
    }

    /// Both clocks move; that alone is no call, so neither renderer does or reports anything.
    void advance(double ms) {
        const size_t sent_before = loop.wire.kinds.size(), heard = loop.host.events.size(), calls = loop.backend.calls.size(), direct_calls = direct_backend.calls.size();
        direct_clock.ms += ms;
        clock.ms += ms;
        ok(loop.wire.kinds.size() == sent_before && loop.host.events.size() == heard && loop.backend.calls.size() == calls && direct_backend.calls.size() == direct_calls,
           "time moving on alone sends, reports and calls nothing");
    }

    template <class Fn> void both(Fn fn) {
        fn(direct_backend);
        fn(loop.backend);
    }

    int builds() {
        const int a = direct->stats().builds, b = loop.end->renderer().stats().builds;
        ok(a == b, "both renderers built as often (" + std::to_string(a) + " and " + std::to_string(b) + ")");
        return a;
    }

    /// Drops both renderers while their Backends live, compares what they released, and counts the session.
    void close() {
        begin("destruction");
        const size_t before = direct_backend.calls.size(), heard = loop.host.events.size();
        direct.reset();
        loop.close();
        tally.destruction = direct_backend.calls.size() - before;
        std::string why;
        ok(same_calls(direct_backend.calls, loop.backend.calls, why), "destruction releases what the direct renderer releases, in its order " + why);
        ok(loop.host.events.size() == heard && loop.host.refusals.empty() && loop.wire.refused == 0 && !loop.platform.refusing(), "destruction reports nothing and the platform end takes all of it");
        ok(loop.backend.live_fonts() == 0 && direct_backend.live_fonts() == 0, "no font outlives its renderer");
        ok(loop.host.frames.size() == requested, "the host got a frame for each request and no other (" + std::to_string(loop.host.frames.size()) + " for " + std::to_string(requested) + ")");
        const std::vector<Call>& calls = loop.backend.calls;
        size_t first = calls.size(), last = 0;
        for (size_t i = 0; i < calls.size(); i++)
            if (calls[i].op == Call::Frame) {
                first = std::min(first, i);
                last = i;
            }
        for (size_t i = 0; i < calls.size(); i++) {
            const Call::Op op = calls[i].op;
            if (op == Call::Frame) continue;
            tally.calls[op]++;
            if ((op == Call::DeleteFont || op == Call::CanvasRun || op == Call::CanvasRelease) && i > first && i < last) tally.between[op]++;
        }
        for (const remote::Frame& f : loop.host.frames) (f.painted ? tally.painted : tally.unchanged)++;
        for (remote::Kind kind : loop.host.events) {
            tally.messages += kind == remote::Kind::Messages;
            tally.pointers += kind == remote::Kind::Pointer;
            tally.selections += kind == remote::Kind::Selection;
            tally.frames += kind == remote::Kind::Frame;
            tally.claims += kind == remote::Kind::ClaimAnswer;
            tally.states += kind == remote::Kind::State;
        }
        tally.print(name);
        total.add(tally);
    }

    std::string name, label;
    VirtualClock direct_clock, clock;
    LogBackend direct_backend;
    std::unique_ptr<Renderer> direct;
    Loopback loop;
    bool asked = false;
    std::string selection;
    remote::State sent;
    size_t requested = 0;
    double lag_ms = 0;
    Tally tally;
};

/// Text, a PNG and two canvases (13 and 14), the first with a clip its two fills share and a putImageData.
const char* RESOURCES = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[]},
 {"op":"text","id":11,"data":"two ends"},
 {"op":"el","id":12,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgo="]]},
 {"op":"el","id":13,"tag":"canvas","attrs":[["width","40"],["height","20"]]},
 {"op":"el","id":14,"tag":"canvas","attrs":[["width","30"],["height","10"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},
 {"op":"ins","parent":4,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},
 {"op":"canvas","id":13,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10],["c","beginPath"],["c","rect",0,0,20,10],["c","clip"],["s","fillStyle","#336699"],
  ["c","fillRect",0,0,5,5],["c","fillRect",6,0,5,5],["c","putImageData",{"b64":"AQIDBAUGBwg=","w":2,"h":1},1,1]]},
 {"op":"canvas","id":14,"cmds":[["s","font","10px monospace"],["c","fillText","ok",0,8]]}
]})";

/// A canvas batch that sets `count` font spellings of its own, each drawn once.
std::string many_fonts(int canvas, int count) {
    std::string cmds;
    for (int i = 1; i <= count; i++) cmds += std::string(i > 1 ? "," : "") + R"(["s","font",")" + std::to_string(i) + R"(px serif"],["c","fillText","f",0,10])";
    return R"({"type":"ops","ops":[{"op":"canvas","id":)" + std::to_string(canvas) + R"(,"cmds":[)" + cmds + "]}]}";
}

bool shares(const Sharing& sharing) {
    for (const std::vector<long>& users : sharing)
        for (size_t i = 0; i < users.size(); i++)
            if (users[i] >= 0 && size_t(users[i]) != i) return true;
    return false;
}

/// Fonts, widths, an image, canvases, pixels, the canvas-font purge between frames, a canvas removed and one kept until destruction, and false or wrong-sized answers.
void resources() {
    Arms a("resources");
    const Arms::Step page = a.receive(RESOURCES, "a page with a text, a PNG and two canvases");
    a.ok(!platform_only(page.messages).empty(), "the page reports its size");
    a.receive(query(1, R"({"q":"measureText","font":"italic 14px serif","text":"wide"})"), "measureText in a font of its own");
    const Arms::Step pixels = a.receive(query(2, R"({"q":"imageData","id":13,"x":0,"y":0,"w":2,"h":1})"), "imageData of the first canvas");
    a.ok(pixels.messages.size() == 1 && !pixels.messages[0]["binaryBase64"].asString().empty() && pixels.messages[0]["binaryBase64"].asString() != zero_base64(8), "the answer holds the platform's pixels");
    a.ok(a.frame("the first frame", true, remote::Paint::IfDirty).frame->painted, "the page is painted");
    a.receive(many_fonts(13, 65), "65 more canvas font spellings in one batch");
    a.frame("the frame that draws them", true, remote::Paint::IfDirty);
    const size_t released = a.loop.backend.count(Call::DeleteFont);
    a.receive(R"({"type":"ops","ops":[{"op":"canvas","id":14,"cmds":[["s","font","9px monospace"],["c","fillText","again",0,8]]}]})", "the next batch, past 64 cached canvas fonts");
    a.ok(a.loop.backend.count(Call::DeleteFont) >= released + 65, "the batch first releases every cached canvas font");
    a.frame("the frame after the purge", true, remote::Paint::IfDirty);
    a.receive(R"({"type":"ops","ops":[{"op":"rm","id":14}]})", "the second canvas removed");
    a.ok(a.loop.backend.count(Call::CanvasRelease) == 1, "its canvas is released at once");
    a.frame("the frame without it", true, remote::Paint::IfDirty);
    a.ok(!a.frame("a frame with nothing new", false, remote::Paint::IfDirty).frame->painted, "an unchanged frame is unpainted");
    a.both([](LogBackend& b) { b.images_load = false; });
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":15,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgoAAAA="]]},{"op":"ins","parent":4,"id":15,"before":null}]})",
              "a PNG the platform does not load");
    a.receive(query(3, R"({"q":"rect","id":15})"), "the box of the image that did not load");
    a.both([](LogBackend& b) { b.reads = LogBackend::Read::WrongSize; });
    const Arms::Step wrong = a.receive(query(4, R"({"q":"imageData","id":13,"x":0,"y":0,"w":2,"h":1})"), "imageData answered with the wrong size");
    a.ok(wrong.messages.size() == 1 && wrong.messages[0]["binaryBase64"].asString() == zero_base64(8), "wrong-sized pixels are zero-filled");
    a.both([](LogBackend& b) { b.reads = LogBackend::Read::Unsupported; });
    const Arms::Step unsupported = a.receive(query(5, R"({"q":"imageData","id":13,"x":1,"y":0,"w":1,"h":2})"), "imageData the platform cannot read");
    a.ok(unsupported.messages.size() == 1 && unsupported.messages[0]["binaryBase64"].asString() == zero_base64(8), "unread pixels are zero-filled");
    a.frame("the last frame", true, remote::Paint::IfDirty);

    std::set<int> canvas_fonts, document_fonts;
    bool images_failed = false, clipped = false, put = false;
    for (const Call& c : a.loop.backend.calls) {
        if (c.op == Call::CanvasFont) canvas_fonts.insert(c.font);
        if (c.op == Call::Font) document_fonts.insert(c.font);
        if (c.op == Call::Image && !c.ok) images_failed = c.w_in == 0 && c.h_in == 0 && c.image_w == 2 && c.image_h == 3;
        if (c.op == Call::CanvasRun) {
            clipped = clipped || shares(sharing(c.ops));
            for (const CanvasOp& o : c.ops) put = put || (o.kind == CanvasOp::PutImage && o.rgba && o.rgba->size() == 8);
        }
    }
    bool marked = !canvas_fonts.empty() && !document_fonts.empty();
    for (const Call& c : a.loop.backend.calls)
        if (c.op == Call::CanvasRun)
            for (const CanvasOp& o : c.ops)
                if (o.kind == CanvasOp::Text) marked = marked && canvas_fonts.count(o.font);
    for (const remote::Frame& f : a.loop.host.frames)
        for (const Command& c : f.list)
            if (c.kind == Cmd::Text) marked = marked && document_fonts.count(c.font);
    a.ok(marked, "canvas text is in canvas fonts alone and the document's in document fonts alone");
    a.ok(clipped && put, "a batch's fills share its clip and its putImageData carries its pixels");
    a.ok(images_failed, "a false image answer leaves the sizes the platform wrote into initialized outputs");
    a.close();
    for (int op = 0; op < Call::Frame; op++) a.need(a.tally.calls[op], CALL_NAMES[op]);
    a.need(a.tally.construction, "Backend calls in construction");
    a.need(a.tally.destruction, "Backend calls in destruction");
    a.need(a.tally.between[Call::DeleteFont], "font releases between frames");
    a.need(a.tally.between[Call::CanvasRun], "canvas runs between frames");
    a.need(a.tally.between[Call::CanvasRelease], "a canvas release between frames");
    a.need(a.tally.painted, "painted frames");
    a.need(a.tally.unchanged, "unchanged frames");
    a.need(a.tally.messages, "Messages reports");
    a.need(a.tally.states, "State reports");
}

/// A finite opacity animation: html(2) head(3) body(4), its style (5) and the box (10).
const char* FADE = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
 {"op":"el","id":5,"tag":"style","attrs":[]},
 {"op":"css","id":5,"text":"@keyframes fade{from{opacity:0}to{opacity:1}} .a{width:20px;height:20px;background:#336699;animation:fade 100ms linear}"},
 {"op":"ins","parent":3,"id":5,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["class","a"]]},
 {"op":"ins","parent":4,"id":10,"before":null}
]})";

/// Frames as the host asks for them: ticking or not, painted if dirty, forced or skipped, over a finite animation in virtual 16 ms steps, and the engine's own frame request.
void frames() {
    Arms a("frames");
    a.receive(FADE, "a box that fades in for 100 ms");
    a.receive(R"({"type":"listen","types":["animationstart","animationend"]})", "the widget listens to its animation");
    const Arms::Step first = a.frame("the first ticking frame", true, remote::Paint::IfDirty);
    a.ok(first.frame->painted && !first.engine && first.state.wants_tick, "it is painted and the animation wants more");
    const Arms::Step clean = a.frame("a clean frame without a tick", false, remote::Paint::IfDirty);
    a.ok(!clean.frame->painted, "a clean IfDirty frame is unpainted");
    const Arms::Step wanted = a.want_frame("the engine asks for a frame");
    a.ok(wanted.state.frame_asked && wanted.events == std::vector<remote::Kind>{remote::Kind::State}, "the want goes out as State alone");
    const Arms::Step forced = a.frame("a forced frame without a tick", false, remote::Paint::Force);
    a.ok(forced.frame->painted && !forced.engine && forced.state.frame_asked, "Force paints and the engine's want survives a frame that does not tick");
    const Arms::Step skipped = a.frame("a ticking frame that skips the paint", true, remote::Paint::Skip);
    a.ok(!skipped.frame->painted && skipped.engine && !skipped.state.frame_asked, "Skip paints nothing and the tick takes the engine's want once");
    size_t painted = 0, frames = 0;
    for (; frames < 20 && a.direct->wants_tick(); frames++) {
        a.advance(16);
        const Arms::Step f = a.frame("an animation frame at +" + std::to_string(16 * (frames + 1)) + " ms", true, remote::Paint::IfDirty);
        painted += f.frame->painted;
        a.ok(!f.engine, "a tick with no engine request owes the engine no frame");
    }
    a.ok(frames > 3 && painted > 3 && !a.direct->wants_tick(), "the animation runs over several painted frames and ends (" + std::to_string(painted) + " of " + std::to_string(frames) + ")");
    const Arms::Step ended = a.frame("a frame after the animation ended", true, remote::Paint::IfDirty);
    a.ok(!ended.frame->painted && !ended.state.wants_tick && std::isinf(ended.state.next_tick_in), "nothing is left to tick or paint");
    a.ok(a.clear_selection("a call that changes nothing after the frame").events.empty(), "no State repeats the one the frame carried");
    a.advance(16);
    a.ok(a.clear_selection("the same call later").events.empty(), "with nothing scheduled, time changes nothing to report");
    const Arms::Step again = a.want_frame("the engine asks again");
    a.ok(again.events == std::vector<remote::Kind>{remote::Kind::State}, "a changed state is reported again");
    a.ok(a.frame("the ticking frame that takes it", true, remote::Paint::IfDirty).engine, "the engine's frame is due after it");
    a.ok(!a.frame("the next ticking frame", true, remote::Paint::IfDirty).engine, "and only once");
    a.close();
    a.need(a.tally.painted, "painted frames");
    a.need(a.tally.unchanged, "unchanged frames");
    a.need(a.tally.frames, "Frame reports");
    a.need(a.tally.states, "State reports");
    a.need(a.tally.helper_only, "animation events for the helper alone");
}

/// From core_tests: cursor:pointer with a span in it (10, 11), a plain div (12), touch-action:none (13), two selectable paragraphs (14, 16) and a plain one (18).
const char* POINTING = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
 {"op":"el","id":5,"tag":"style","attrs":[]},
 {"op":"css","id":5,"text":".s{user-select:text} p{margin:0;font:10px/20px sans-serif}"},
 {"op":"ins","parent":3,"id":5,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["style","height:20px;cursor:pointer"]]},
 {"op":"el","id":11,"tag":"span","attrs":[["style","display:block;height:20px"]]},
 {"op":"el","id":12,"tag":"div","attrs":[["style","height:20px"]]},
 {"op":"el","id":13,"tag":"div","attrs":[["style","height:40px;touch-action:none"]]},
 {"op":"el","id":14,"tag":"p","attrs":[["class","s"]]},
 {"op":"text","id":15,"data":"Hello world"},
 {"op":"el","id":16,"tag":"p","attrs":[["class","s"]]},
 {"op":"text","id":17,"data":"Second"},
 {"op":"el","id":18,"tag":"p","attrs":[]},
 {"op":"text","id":19,"data":"Plain"},
 {"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},{"op":"ins","parent":4,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},{"op":"ins","parent":14,"id":15,"before":null},
 {"op":"ins","parent":4,"id":16,"before":null},{"op":"ins","parent":16,"id":17,"before":null},
 {"op":"ins","parent":4,"id":18,"before":null},{"op":"ins","parent":18,"id":19,"before":null}
]})";

/// Both arms' answers to the public selection queries, made the same way on each so that neither renderer lays text out alone.
void selection_queries(Arms& a, const std::string& when) {
    struct Seen {
        bool may = false, has = false;
        std::string text;
    };
    std::vector<Seen> seen;
    a.helper("the selection queries " + when, [&](Renderer& r) {
        Seen s;
        s.may = r.may_have_selection();
        s.has = r.has_selection();
        s.text = r.selected_text();
        seen.push_back(s);
        return std::vector<Json>();
    });
    const bool alike = seen.size() == 2 && seen[0].may == seen[1].may && seen[0].has == seen[1].has && seen[0].text == seen[1].text;
    a.ok(alike, "both renderers answer alike");
    if (alike) a.ok((seen[0].may || !seen[0].has) && seen[0].has == !seen[0].text.empty(), "has_selection is nonempty text, and false whenever may_have_selection is");
}

/// Cursor, press claims, holds, hold queries against real presses, input while frozen, and the selection reported exactly when its text changes.
void pointing() {
    Arms a("pointer");
    a.receive(POINTING, "a page to point at");
    a.receive(R"({"type":"listen","types":["pointerdown","pointerup","pointercancel","contextmenu","click"]})", "the widget listens to presses");
    a.frame("the first frame", true, remote::Paint::IfDirty);
    a.ok(a.hover("the mouse over cursor:pointer", 0, 5, 10).pointer->cursor == "pointer", "the cursor there is pointer");
    a.ok(a.hover("the mouse over a plain div", 0, 5, 30).pointer->cursor == "auto", "the cursor there is auto");
    a.hover("the mouse back over cursor:pointer", 0, 5, 10);
    a.ok(a.hover("the mouse leaves", 1, 0, 0).pointer->cursor == "auto", "the cursor is auto once the mouse left");

    const Arms::Step query_only = a.claim("a hold query on touch-action:none", 60, 60);
    a.advance(600);
    const Arms::Step after_query = a.frame("a ticking frame 600 ms after the query", true, remote::Paint::IfDirty);
    a.ok(query_only.messages.empty() && std::isinf(query_only.state.next_tick_in) && !has_event(after_query.messages, "contextmenu") && !gesture_of(after_query.messages),
         "the query presses nothing: no event, no gesture, no hold timer");
    const struct {
        const char* what;
        double x, y;
        twidget::WidgetGestureClaim claim;
    } spots[] = {
        {"cursor:pointer", 60, 10, NO_CLAIM},
        {"a plain div", 60, 30, NO_CLAIM},
        {"touch-action:none", 60, 60, HOLD_CLAIM},
        {"selectable text", 60, 90, NO_CLAIM},
    };
    for (const auto& spot : spots) {
        const std::string on = std::string(" on ") + spot.what;
        const Arms::Step asked = a.claim("a hold query" + on, spot.x, spot.y);
        a.ok(asked.claim == spot.claim && asked.messages.empty() && asked.selection.empty(), "the query answers " + claim_name(spot.claim) + " (got " + claim_name(asked.claim) + ") and selects nothing");
        const Arms::Step pressed = a.touch("a finger pressed" + on, 0, spot.x, spot.y, 0, true);
        a.ok(pressed.pointer->claim == asked.claim, "the press claims what the query answered (got " + claim_name(pressed.pointer->claim) + ")");
        a.ok(asked.claim != HOLD_CLAIM || (!pressed.state.wants_tick && pressed.state.next_tick_in > 0.4 && pressed.state.next_tick_in <= 0.5), "the press on none starts the hold timer the query did not");
        a.touch("the press cancelled" + on, 3, spot.x, spot.y, 0, true);
    }

    a.ok(!a.frame("a frame that drains what waits", true, remote::Paint::IfDirty).state.wants_tick, "nothing waits before the hold");
    const Arms::Step held = a.touch("a finger held on touch-action:none", 0, 60, 60, 0, true);
    a.ok(held.pointer->claim == HOLD_CLAIM && !held.state.wants_tick && held.state.next_tick_in > 0.4 && held.state.next_tick_in <= 0.5, "the press claims the hold, due at the next tick");
    a.advance(520);
    const Arms::Step due = a.claim("a hold query past the hold's deadline", 60, 60);
    a.ok(due.state.wants_tick && due.state.next_tick_in == 0, "the next call reports the hold due");
    a.ok(has_event(a.frame("the frame that takes the hold", true, remote::Paint::Skip).messages, "contextmenu"), "the held finger gets the widget's contextmenu");
    a.touch("the held finger lifted", 2, 60, 60, 0, true);
    a.ok(has_event(a.touch("a right click on touch-action:none", 0, 60, 60, 1, true, true, 0, 2).messages, "contextmenu"), "a right click there is the widget's contextmenu");
    a.touch("the right button released", 2, 60, 60, 1, true, true, 0, 0);
    a.touch("a first finger", 0, 60, 30, 0, true);
    a.ok(!a.touch("a second finger, not primary", 0, 100, 30, 5, false).pointer->claim, "a press that is not primary claims nothing");
    a.touch("the second finger lifted", 2, 100, 30, 5, false);
    a.touch("the first finger lifted", 2, 60, 30, 0, true);

    selection_queries(a, "with nothing selected");
    const Arms::Step start = a.touch("a mouse press on selectable text", 0, 1, 90, 1, true, true);
    a.ok(start.pointer->claim == DRAG_CLAIM && start.selection.empty(), "a press that starts a selection claims horizontal drags and selects nothing yet");
    a.ok(a.touch("the mouse dragged over the line", 1, 200, 90, 1, true, true).selection == "Hello world", "the drag selects the line");
    const Arms::Step released = a.touch("the mouse released", 2, 200, 90, 1, true, true);
    a.ok(released.selection == "Hello world" && std::count(released.events.begin(), released.events.end(), remote::Kind::Selection) == 0, "the same selection is not reported again");
    const Arms::Step over = a.claim("a hold query over the selection", 60, 90);
    a.ok(over.selection == "Hello world" && over.messages.empty() && std::count(over.events.begin(), over.events.end(), remote::Kind::Selection) == 0, "the query keeps the selection and emits nothing");
    selection_queries(a, "with a selection");
    a.ok(a.touch("a press inside the selection", 0, 1, 90, 1, true, true).selection.empty(), "a press drops the selection");
    const Arms::Step part = a.touch("dragged part of the way", 1, 33, 90, 1, true, true);
    a.ok(!part.selection.empty() && part.selection != "Hello world", "part of the line is selected (got '" + part.selection + "')");
    a.ok(a.touch("dragged to the line's end", 1, 200, 90, 1, true, true).selection == "Hello world", "a changed selection is reported again");
    a.ok(a.touch("dragged into the next block", 1, 200, 110, 1, true, true).selection == "Hello world\nSecond", "a selection across blocks keeps a line break");
    a.touch("released over the next block", 2, 200, 110, 1, true, true);
    a.ok(a.clear_selection("the host clears the selection").selection.empty(), "the cleared selection is reported");
    selection_queries(a, "after clear_selection");
    a.ok(a.clear_selection("cleared again").events.empty(), "clearing nothing reports nothing");
    a.touch("a new selection pressed", 0, 1, 90, 1, true, true);
    a.touch("a new selection dragged", 1, 200, 90, 1, true, true);
    a.touch("a new selection released", 2, 200, 90, 1, true, true);
    a.ok(a.touch("a press on plain text", 0, 2, 130, 1, true, true).selection.empty(), "a press elsewhere drops it");
    a.touch("the press on plain text released", 2, 2, 130, 1, true, true);

    a.receive(R"({"type":"ops","ops":[{"op":"el","id":20,"tag":"style","attrs":[]},{"op":"css","id":20,"text":"@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .spin{position:absolute;right:0;top:0;width:10px;height:10px;animation:spin 1s linear infinite}"},{"op":"ins","parent":3,"id":20,"before":null},{"op":"el","id":21,"tag":"div","attrs":[["class","spin"]]},{"op":"ins","parent":4,"id":21,"before":null}]})",
              "a spinner, so that a batch waits for the frame");
    a.advance(300);
    const int built = a.builds();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":22,"tag":"div","attrs":[["style","position:absolute;left:0;top:200px;width:200px;height:40px;touch-action:none"]]},{"op":"ins","parent":4,"id":22,"before":null}]})",
              "a batch with touch-action:none in it, which waits for the frame");
    a.ok(a.builds() == built, "the batch waits");
    const Arms::Step late = a.claim("a hold query on what the waiting batch adds", 60, 220);
    a.ok(a.builds() > built && late.claim == HOLD_CLAIM, "the query builds the batch first and answers for what it added (got " + claim_name(late.claim) + ")");
    a.ok(a.touch("a finger pressed there", 0, 60, 220, 0, true).pointer->claim == late.claim, "the press claims what the query answered");
    a.touch("the press there cancelled", 3, 60, 220, 0, true);

    a.receive(R"({"type":"frozen","reason":"paused","fatal":false})", "the widget is frozen");
    a.ok(a.hover("the mouse over cursor:pointer while frozen", 0, 5, 10).pointer->cursor == "auto", "a frozen widget's cursor is auto");
    const Arms::Step frozen = a.touch("a finger on touch-action:none while frozen", 0, 60, 60, 0, true);
    a.ok(!frozen.pointer->claim && !has_event(frozen.messages, "pointerdown"), "a frozen widget takes no press");
    a.ok(a.claim("a hold query while frozen", 60, 60).claim == NO_CLAIM, "and claims nothing");
    a.touch("the finger lifted while frozen", 2, 60, 60, 0, true);
    a.close();
    a.need(a.tally.pointers, "Pointer reports");
    a.need(a.tally.selections, "Selection reports");
    a.need(a.tally.claims, "ClaimAnswer reports");
    a.need(a.tally.messages, "Messages reports");
}

/// A page 300 px tall: html(2) head(3) body(4), a div (10) with a text (11).
const char* TALL = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","height:300px"]]},
 {"op":"text","id":11,"data":"tall"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]})";

std::vector<Json> mixture() {
    std::vector<Json> out;
    for (const char* m : {R"({"type":"size","width":200,"height":300,"innerWidth":200,"innerHeight":100,"dpr":2})", R"({"type":"event","ev":{"type":"click","targetId":10}})",
                          R"({"type":"gesture","wants":false,"x":true,"y":false,"hold":true})", R"({"type":"result","qid":9,"result":{}})", R"({"type":"limit","detail":"paint"})",
                          R"({"type":"resize","entries":[]})", R"({"type":"violation","detail":"ops limit"})", R"({"type":"bubble","action":"menu","gesture":"hold"})"})
        out.push_back(parse(m));
    return out;
}

/// Size and theme input, the viewport against the document's height, work left unpainted until the host asks, and which messages cross to the platform.
void routing() {
    Arms a("routing");
    const Arms::Step page = a.receive(TALL, "a page 300 px tall");
    a.ok(page.state.height == 300, "the viewport follows the document (" + std::to_string(page.state.height) + ")");
    a.receive(R"({"type":"listen","types":["pointerdown","pointerup"]})", "the widget listens to presses");
    const Arms::Step capped = a.set_max_height("the height capped at 100 px", 100);
    const Json* size = message_of(capped.messages, "size");
    a.ok(size && (*size)["height"].asNumber() == 300 && (*size)["innerHeight"].asNumber() == 100 && capped.state.height == 100,
         "State has the viewport's height and the size message the document's as well");
    const Arms::Step narrow = a.set_width("a narrower view", 200);
    a.ok(narrow.state.width == 200 && message_of(narrow.messages, "size"), "the width is in State and the size message");
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":4,"name":"font","value":"18px serif"}]})", "a batch that changes the body's font");
    a.set_theme("the dark theme, which builds the document anew", true);
    a.hover("the mouse over the page", 0, 5, 50);
    a.want_frame("the engine asks for a frame");
    a.ok(a.loop.host.frames.empty() && a.direct->dirty() && a.loop.end->renderer().dirty() && a.loop.platform.state().dirty, "without request_frame nothing is painted or framed");
    const size_t released = a.loop.backend.count(Call::DeleteFont);
    a.helper("the helper trims", [](Renderer& r) {
        r.trim();
        return std::vector<Json>();
    });
    a.ok(a.loop.backend.count(Call::DeleteFont) > released, "trim releases the fonts no document holds");
    const Arms::Step mixed = a.helper("the helper reports a mixture of messages", [](Renderer&) { return mixture(); });
    std::string crossed;
    if (!a.loop.host.batches.empty())
        for (const Json& m : a.loop.host.batches.back()) crossed += m["type"].asString() + " ";
    a.ok(mixed.events == std::vector<remote::Kind>{remote::Kind::Messages} && crossed == "size gesture limit violation bubble ", "the five platform messages cross alone (got " + crossed + ")");
    const Arms::Step result = a.receive(query(1, R"({"q":"rect","id":10})"), "a query");
    a.ok(message_of(result.messages, "result") && result.events.empty(), "its result is the helper's alone");
    const Arms::Step press = a.touch("a mouse press", 0, 5, 50, 1, true, true);
    a.ok(has_event(press.messages, "pointerdown") && gesture_of(press.messages) && press.events.front() == remote::Kind::Messages, "the helper gets the press's event, the platform its gesture");
    a.touch("the mouse released", 2, 5, 50, 1, true, true);
    const Arms::Step painted = a.frame("the frame the host asks for", true, remote::Paint::IfDirty);
    a.ok(painted.engine && painted.frame->painted && !painted.frame->damage, "the requested frame paints the waiting work, whole after trim, and owes the engine its frame");
    a.ok(!a.frame("an unchanged frame", false, remote::Paint::IfDirty).frame->painted, "the next is unpainted");
    a.close();
    a.need(a.tally.messages, "Messages reports");
    a.need(a.tally.helper_only, "messages for the helper alone");
}

/// Overrides create_font alone, as every backend did before the canvas mark, so canvas fonts reach it through the default.
struct PlainBackend : Backend {
    struct Made {
        std::string families, features;
        float size = 0;
        int weight = 0;
        bool italic = false;
    };

    int create_font(const std::string& families, float size_px, int weight, bool italic, const std::string& features, FontMetrics& out) override {
        out = metrics_for(size_px, weight, italic);
        made.push_back({families, features, size_px, weight, italic});
        sizes[int(made.size())] = size_px;
        return int(made.size());
    }
    void delete_font(int font) override { sizes.erase(font); }
    float text_width(int font, const std::string& text) override {
        const auto it = sizes.find(font);
        return it == sizes.end() ? 0 : it->second * 0.5f * float(text.size());
    }
    bool load_image(int, const std::string&, float&, float&) override { return false; }

    std::vector<Made> made;
    std::map<int, float> sizes;
};

const char* CANVAS_FONTS = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[]},
 {"op":"text","id":11,"data":"words"},
 {"op":"el","id":13,"tag":"canvas","attrs":[["width","40"],["height","20"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":13,"before":null},
 {"op":"canvas","id":13,"cmds":[["s","font","bold 13px monospace"],["c","fillText","hi",0,10],["s","font","italic 15px monospace"],["c","fillText","hi",0,10]]}
]})";

/// The time seam and the defaults: two clocks that move apart, time standing still across real delays, the throttle on virtual time, the static clock, and canvas fonts on a backend that does not tell them apart.
void clocks() {
    {
        VirtualClock a_clock, b_clock;
        LogBackend a_backend, b_backend;
        Renderer a(300, 1, false, a_backend, &a_clock), b(300, 1, false, b_backend, &b_clock);
        for (Renderer* r : {&a, &b}) {
            r->receive(parse(POINTING));
            r->receive(parse(R"({"type":"listen","types":["contextmenu"]})"));
            r->paint();
            r->tick();
        }
        const bool idle = !a.wants_tick() && !b.wants_tick();
        a.touch(0, 60, 60, 0, true);
        b.touch(0, 60, 60, 0, true);
        const double wake = b.next_tick_in();
        check(idle && bits64(a.next_tick_in()) == bits64(wake) && wake > 0.4 && wake <= 0.5, "two renderers pressed at the same virtual time hold until the same deadline");
        a_clock.ms += 520;
        check(a.wants_tick() && a.next_tick_in() == 0, "moving one renderer's clock brings its hold due");
        check(!b.wants_tick() && bits64(b.next_tick_in()) == bits64(wake), "the other's clock and hold stand still");
        check(has_event(a.tick(), "contextmenu") && !has_event(b.tick(), "contextmenu"), "only the one whose time moved takes the hold");
        b.touch(3, 60, 60, 0, true);
        a.touch(2, 60, 60, 0, true);
    }
    {
        SteppingClock moving, still;
        LogBackend moving_backend, still_backend;
        Renderer m(300, 1, false, moving_backend, &moving), s(300, 1, false, still_backend, &still);
        m.receive(parse(TALL));
        s.receive(parse(TALL));
        moving.ms += 1100;
        still.ms += 1100;
        moving.step = 40;
        const char* batch = R"({"type":"ops","ops":[{"op":"style","id":4,"name":"padding-top","value":"3px"}]})";
        const int built = m.stats().builds;
        m.receive(parse(batch));
        s.receive(parse(batch));
        check(m.stats().builds > built && m.stats().throttled > 0 && s.stats().throttled == 0, "a build is charged the time its renderer's clock moved inside it, and a clock that stands still charges nothing");
    }
    {
        using namespace std::chrono;
        const double steady = duration<double>(steady_clock::now().time_since_epoch()).count();
        const double before = Renderer::clock();
        std::this_thread::sleep_for(milliseconds(20));
        const double after = Renderer::clock();
        check(std::fabs(before - steady) < 0.05 && after - before >= 0.015, "the static renderer clock is the steady clock in seconds and moves across a real 20 ms");
    }
    {
        Arms a("real delays");
        a.lag_ms = 20;
        a.receive(R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
 {"op":"el","id":5,"tag":"style","attrs":[]},{"op":"css","id":5,"text":"@keyframes fade{from{opacity:0}to{opacity:1}} .a{height:10px;background:#336699;animation:fade 200ms linear}"},
 {"op":"ins","parent":3,"id":5,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["class","a"]]},{"op":"ins","parent":4,"id":10,"before":null},
 {"op":"el","id":11,"tag":"div","attrs":[["style","height:40px;touch-action:none"]]},{"op":"ins","parent":4,"id":11,"before":null}]})",
                  "a fade over a touch-action:none strip, each arm 20 ms of real time after the other");
        a.receive(R"({"type":"listen","types":["contextmenu"]})", "the widget listens to contextmenu");
        a.frame("the first frame", true, remote::Paint::IfDirty);
        size_t painted = 0, frames = 0;
        for (; frames < 30 && a.direct->wants_tick(); frames++) {
            a.advance(16);
            painted += a.frame("a fade frame at +" + std::to_string(16 * (frames + 1)) + " ms", true, remote::Paint::IfDirty).frame->painted;
        }
        a.ok(painted > 5 && !a.direct->wants_tick(), "the fade's frames match at equal virtual times whatever real time passed (" + std::to_string(painted) + " painted)");
        const Arms::Step held = a.touch("a finger held on the strip", 0, 60, 30, 0, true);
        a.ok(held.pointer->claim == HOLD_CLAIM && !held.state.wants_tick, "the press claims the hold");
        std::this_thread::sleep_for(std::chrono::milliseconds(600));
        const Arms::Step still = a.clear_selection("a call after 600 ms of real time");
        a.ok(still.events.empty() && !still.state.wants_tick && bits64(still.state.next_tick_in) == bits64(held.state.next_tick_in), "real time alone brings no hold due");
        a.advance(520);
        a.ok(a.clear_selection("a call past the hold's virtual deadline").state.wants_tick, "virtual time does");
        a.ok(has_event(a.frame("the frame that takes the hold", true, remote::Paint::Skip).messages, "contextmenu"), "and the hold is taken in both arms");
        a.touch("the finger lifted", 2, 60, 30, 0, true);
        a.close();
    }
    {
        Arms a("throttle");
        a.receive(TALL, "a page");
        a.frame("the first frame", true, remote::Paint::IfDirty);
        a.advance(1100);
        a.helper("50 ms charged past the load grace", [](Renderer& r) {
            r.charge(0.05);
            return std::vector<Json>();
        });
        const int built = a.builds();
        const Arms::Step waiting = a.receive(R"({"type":"ops","ops":[{"op":"style","id":4,"name":"padding-top","value":"3px"}]})", "a batch under the throttle");
        a.ok(a.builds() == built && waiting.state.wants_tick && waiting.state.next_tick_in > 0.1 && waiting.state.next_tick_in <= 0.15, "the batch waits out the throttle, 150 ms of virtual time");
        a.frame("a frame under the throttle", true, remote::Paint::IfDirty);
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        const Arms::Step later = a.frame("a frame after 200 ms of real time", true, remote::Paint::IfDirty);
        a.ok(a.builds() == built && later.state.wants_tick, "real time does not end the throttle");
        a.advance(200);
        const Arms::Step over = a.frame("a frame past the throttle's virtual end", true, remote::Paint::IfDirty);
        a.ok(a.builds() > built && over.frame->painted, "virtual time does: the batch is built and painted");
        a.close();
    }
    {
        VirtualClock plain_clock, marked_clock;
        PlainBackend plain;
        LogBackend marked;
        Renderer p(300, 1, false, plain, &plain_clock), m(300, 1, false, marked, &marked_clock);
        p.receive(parse(CANVAS_FONTS));
        m.receive(parse(CANVAS_FONTS));
        const std::string bold = query(1, R"({"q":"measureText","font":"bold 13px monospace","text":"wide"})"), italic = query(2, R"({"q":"measureText","font":"italic 15px monospace","text":"wide"})");
        const std::vector<Json> plain_bold = p.receive(parse(bold)), plain_italic = p.receive(parse(italic));
        check(dumps(plain_bold) == dumps(m.receive(parse(bold))) && dumps(plain_italic) == dumps(m.receive(parse(italic))), "a backend without the mark measures canvas text as one with it");
        size_t canvas_made = 0;
        bool plain_ok = true;
        for (const PlainBackend::Made& f : plain.made)
            if (f.families == "monospace") {
                canvas_made++;
                plain_ok = plain_ok && f.features.empty() && (f.size == 13 || f.size == 15);
            }
        check(canvas_made == 2 && plain_ok, "create_font alone gets each canvas font once, with empty features");
        const FontMetrics bold_metrics = metrics_for(13, 700, false);
        check(plain_bold.size() == 1 && plain_bold[0]["result"]["fontBoundingBoxAscent"].asNumber() == bold_metrics.ascent && plain_bold[0]["result"]["emHeightAscent"].asNumber() == bold_metrics.em_ascent,
              "the canvas has the metrics create_font gave");
        check(plain_italic.size() == 1 && plain_italic[0]["result"]["emHeightAscent"].asNumber() > 0, "and an em box of its own where create_font gave none");
        bool marked_ok = marked.count(Call::CanvasFont) == 2;
        for (const Call& c : marked.calls) {
            if (c.op == Call::CanvasFont) marked_ok = marked_ok && c.families == "monospace" && c.features.empty();
            if (c.op == Call::Font) marked_ok = marked_ok && c.families != "monospace";
        }
        check(marked_ok, "a backend with the mark gets the canvas fonts there alone");
    }
}


/// A litehtml container that lays text out at 8 px a byte and draws nothing, logging each measure; it lets texts owe their widths when `later`.
class PlainContainer final : public litehtml::document_container {
public:
    explicit PlainContainer(bool later) : later_(later) {}

    litehtml::uint_ptr create_font(const litehtml::font_description& descr, const litehtml::document*, litehtml::font_metrics* fm) override {
        if (fm) {
            fm->font_size = descr.size;
            fm->ascent = litehtml::pixel_t(float(descr.size) * 0.75f);
            fm->descent = litehtml::pixel_t(float(descr.size) * 0.25f);
            fm->height = litehtml::pixel_t(float(descr.size) * 1.25f);
            fm->x_height = litehtml::pixel_t(float(descr.size) * 0.5f);
            fm->ch_width = litehtml::pixel_t(8);
        }
        return ++fonts_;
    }
    void delete_font(litehtml::uint_ptr) override {}
    litehtml::pixel_t text_width(const char* text, litehtml::uint_ptr) override {
        measured.emplace_back(text);
        return litehtml::pixel_t(8.0f * float(std::strlen(text)));
    }
    bool measures_text_later() const override { return later_; }
    void draw_text(litehtml::uint_ptr, const char*, litehtml::uint_ptr, litehtml::web_color, const litehtml::position&) override {}
    litehtml::pixel_t pt_to_px(float pt) const override { return litehtml::pixel_t(pt * 96.0f / 72.0f); }
    litehtml::pixel_t get_default_font_size() const override { return 16; }
    const char* get_default_font_name() const override { return "sans-serif"; }
    void draw_list_marker(litehtml::uint_ptr, const litehtml::list_marker&) override {}
    void load_image(const char*, const char*, bool) override {}
    void get_image_size(const char*, const char*, litehtml::size& sz) override { sz = litehtml::size(); }
    void draw_image(litehtml::uint_ptr, const litehtml::background_layer&, const std::string&, const std::string&) override {}
    void draw_solid_fill(litehtml::uint_ptr, const litehtml::background_layer&, const litehtml::web_color&) override {}
    void draw_linear_gradient(litehtml::uint_ptr, const litehtml::background_layer&, const litehtml::background_layer::linear_gradient&) override {}
    void draw_radial_gradient(litehtml::uint_ptr, const litehtml::background_layer&, const litehtml::background_layer::radial_gradient&) override {}
    void draw_conic_gradient(litehtml::uint_ptr, const litehtml::background_layer&, const litehtml::background_layer::conic_gradient&) override {}
    void draw_borders(litehtml::uint_ptr, const litehtml::borders&, const litehtml::position&, bool) override {}
    void set_caption(const char*) override {}
    void set_base_url(const char*) override {}
    void link(const std::shared_ptr<litehtml::document>&, const litehtml::element::ptr&) override {}
    void on_anchor_click(const char*, const litehtml::element::ptr&) override {}
    void on_mouse_event(const litehtml::element::ptr&, litehtml::mouse_event) override {}
    void set_cursor(const char*) override {}
    void transform_text(std::string& text, litehtml::text_transform tt) override {
        if (tt == litehtml::text_transform_uppercase)
            for (char& c : text) c = char(std::toupper(static_cast<unsigned char>(c)));
    }
    void import_css(std::string&, const std::string&, std::string&) override {}
    void set_clip(const litehtml::position&, const litehtml::border_radiuses&) override {}
    void del_clip() override {}
    void get_viewport(litehtml::position& viewport) const override { viewport = litehtml::position(0, 0, 300, 600); }
    litehtml::element::ptr create_element(const char*, const litehtml::string_map&, const std::shared_ptr<litehtml::document>&) override { return nullptr; }
    void get_media_features(litehtml::media_features& media) const override {
        media = litehtml::media_features();
        media.type = litehtml::media_type_screen;
        media.width = media.device_width = 300;
        media.height = media.device_height = 600;
        media.color = 8;
        media.resolution = 96;
    }
    void get_language(std::string& language, std::string& culture) const override {
        language = "en";
        culture.clear();
    }

    std::vector<std::string> measured;

private:
    bool later_ = false;
    litehtml::uint_ptr fonts_ = 0;
};

/// What litehtml's layout reads of an element's size.
litehtml::size size_of(litehtml::element* el) {
    litehtml::size sz;
    if (el) el->get_content_size(sz, 0);
    return sz;
}

litehtml::el_text* first_text(const litehtml::document::ptr& doc, const std::string& selector) {
    const litehtml::element::ptr el = doc->root()->select_one(selector);
    return el && !el->children().empty() ? dynamic_cast<litehtml::el_text*>(el->children().front().get()) : nullptr;
}

/// A container that does not opt in measures every text in the style pass, as litehtml always did; one that opts in measures each when it is read, the same texts at the same widths, and set_text drops what a text owed.
void containers() {
    const std::string html = R"(<html><body><p>alpha beta</p><p style="text-transform:uppercase">gamma</p><pre>a	b</pre></body></html>)";
    PlainContainer now(false), later(true);
    const litehtml::document::ptr a = litehtml::document::createFromString(litehtml::estring(html), &now);
    const litehtml::document::ptr b = litehtml::document::createFromString(litehtml::estring(html), &later);
    const std::set<std::string> styled(now.measured.begin(), now.measured.end());
    check(styled.count("alpha") && styled.count("GAMMA") && styled.count("    "), "a container that does not opt in measures what the texts show in the style pass");
    check(later.measured.empty(), "one that opts in measures nothing there");
    litehtml::el_text* alpha = first_text(b, "p");
    check(alpha && alpha->width_owed() && !first_text(a, "p")->width_owed(), "its texts owe their widths, the other's none");
    const size_t before = now.measured.size();
    a->render(300);
    b->render(300);
    check(now.measured.size() == before, "the container that does not opt in measures nothing more in the layout");
    const std::set<std::string> read(later.measured.begin(), later.measured.end());
    check(read == styled && later.measured.size() == before, "the one that opts in measures the same texts once each as the layout reads them");
    check(a->height() == b->height() && a->width() == b->width() && !alpha->width_owed(), "both lay out alike, and no width is owed after the layout");

    PlainContainer early(true);
    const litehtml::document::ptr c = litehtml::document::createFromString(litehtml::estring(html), &early);
    litehtml::el_text* text = first_text(c, "p");
    const litehtml::size sz = size_of(text);
    check(text && early.measured == std::vector<std::string>{"alpha"} && float(sz.width) == 40 && !text->width_owed(), "a size read before the layout measures that text alone, exactly");
    size_of(text);
    check(early.measured.size() == 1, "and reading it again measures nothing");
    PlainContainer reset(true);
    const litehtml::document::ptr d = litehtml::document::createFromString(litehtml::estring(html), &reset);
    litehtml::el_text* changed = first_text(d, "p");
    if (changed) changed->set_text("omega");
    check(changed && !changed->width_owed(), "set_text drops the width a text owed");
    check(reset.measured.empty() && float(size_of(changed).width) == 0, "and its size reads as nothing until it is styled again, as before");
}

// ───────────────────────── a layout's unknown words in batches ─────────────────────────

std::vector<remote::Kind> kinds_since(const Loopback& loop, size_t from) {
    return std::vector<remote::Kind>(loop.wire.kinds.begin() + std::ptrdiff_t(from), loop.wire.kinds.end());
}

std::vector<Call> widths_since(const LogBackend& backend, size_t from) {
    std::vector<Call> out;
    for (size_t i = from; i < backend.calls.size(); i++)
        if (backend.calls[i].op == Call::TextWidth) out.push_back(backend.calls[i]);
    return out;
}

/// No font and text measured twice.
bool measured_once(const std::vector<Call>& widths) {
    std::set<std::pair<int, std::string>> seen;
    for (const Call& c : widths)
        if (!seen.emplace(c.font, c.text).second) return false;
    return true;
}

/// Exactly `n` widths requests, the first after every font request, the last before every single width request.
bool batches_of(const std::vector<remote::Kind>& kinds, size_t n) {
    const auto first = std::find(kinds.begin(), kinds.end(), remote::Kind::WidthsRequest);
    const auto last = std::find(kinds.rbegin(), kinds.rend(), remote::Kind::WidthsRequest).base();
    if (first == kinds.end() || count(kinds, remote::Kind::WidthsRequest) != n) return false;
    return std::find(first, kinds.end(), remote::Kind::FontRequest) == kinds.end() && std::find(kinds.begin(), last, remote::Kind::WidthRequest) == last;
}

bool one_batch(const std::vector<remote::Kind>& kinds) {
    return batches_of(kinds, 1);
}

std::set<std::string> texts_of(const std::vector<Call>& widths) {
    std::set<std::string> out;
    for (const Call& c : widths) out.insert(c.text);
    return out;
}

/// Some fifty words in five faces: repeated words, a bold copy of five, an uppercase line, a pre block with a tab, spaced words, and a word broken anywhere in a narrow box.
const char* WORDS = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen alpha beta gamma delta pi rho tau mu"},
 {"op":"el","id":12,"tag":"p","attrs":[["style","font-weight:bold"]]},
 {"op":"text","id":13,"data":"alpha beta gamma delta epsilon"},
 {"op":"el","id":14,"tag":"p","attrs":[["style","text-transform:uppercase"]]},
 {"op":"text","id":15,"data":"Mixed case words"},
 {"op":"el","id":16,"tag":"pre","attrs":[["style","font:13px monospace"]]},
 {"op":"text","id":17,"data":"a\tb  c\nd"},
 {"op":"el","id":18,"tag":"p","attrs":[["style","letter-spacing:2px;word-spacing:3px"]]},
 {"op":"text","id":19,"data":"spaced words here"},
 {"op":"el","id":20,"tag":"div","attrs":[["style","width:40px;overflow-wrap:anywhere"]]},
 {"op":"text","id":21,"data":"supercalifragilisticexpialidocious"},
 {"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},{"op":"ins","parent":12,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},{"op":"ins","parent":14,"id":15,"before":null},
 {"op":"ins","parent":4,"id":16,"before":null},{"op":"ins","parent":16,"id":17,"before":null},
 {"op":"ins","parent":4,"id":18,"before":null},{"op":"ins","parent":18,"id":19,"before":null},
 {"op":"ins","parent":4,"id":20,"before":null},{"op":"ins","parent":20,"id":21,"before":null}
]})";

/// A build's unknown words go in one widths request after its fonts and the breaks its layout takes in one more, a relayout finds them cached, later builds ask for their new words alone, transforms and spacing measure what they show, and after trim the words are asked for again; each call as the direct renderer makes it.
void batches() {
    Arms a("batches");
    size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(WORDS, "a page of some fifty words in five faces");
    std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    std::vector<Call> widths = widths_since(a.loop.backend, calls);
    const size_t singles = count(kinds, remote::Kind::WidthRequest), batched = widths.size() - singles;
    a.ok(batches_of(kinds, 2), "the build asks for its words in one widths request after its fonts, and its layout for the breaks in one more, before any single width");
    a.ok(batched > 50, "the requests hold the page's distinct words and spaces, and their letters (" + std::to_string(batched) + ")");
    bool letters_alone = false;
    for (size_t i = widths.size() - singles; i < widths.size(); i++) letters_alone = letters_alone || widths[i].text.size() < 2;
    a.ok(singles > 0 && !letters_alone, "the word broken anywhere is measured in longer pieces in the layout, after the batches, never a letter alone (" + std::to_string(singles) + ")");
    a.ok(measured_once(widths), "no font and text is measured twice");
    bool upper = false, tab = false;
    for (const Call& c : widths) {
        upper = upper || c.text == "MIXED";
        tab = tab || c.text == "    ";
    }
    a.ok(upper && tab, "the batch measures what the texts show: the uppercase words and a tab's spaces");
    a.frame("the first frame", true, remote::Paint::IfDirty);

    sent = a.loop.wire.kinds.size();
    a.set_width("a narrower view", 200);
    a.ok(count(kinds_since(a.loop, sent), remote::Kind::WidthsRequest) == 0, "a relayout at another width finds every word cached");

    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"p","attrs":[]},{"op":"text","id":31,"data":"alpha omega seventeen eighteen alpha nineteen"},{"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":30,"id":31,"before":null}]})",
              "a paragraph of known and new words");
    kinds = kinds_since(a.loop, sent);
    widths = widths_since(a.loop.backend, calls);
    std::set<std::string> asked;
    for (const Call& c : widths) asked.insert(c.text);
    a.ok(one_batch(kinds) && asked == std::set<std::string>{"seventeen", "eighteen", "nineteen"}, "only the new words are asked for, in one widths request");

    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":18,"name":"letter-spacing","value":"5px"}]})", "a new letter spacing");
    bool spaced = false;
    for (const Call& c : widths_since(a.loop.backend, calls)) spaced = spaced || c.text == "spaced" || c.text == "here";
    a.ok(!spaced, "the spaced words keep their face's widths, the spacing added where they are used");
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":14,"name":"text-transform","value":"lowercase"}]})", "the uppercase line turned lowercase");
    a.frame("the frame after the restyles", true, remote::Paint::IfDirty);

    a.helper("the helper trims", [](Renderer& r) {
        r.trim();
        return std::vector<Json>();
    });
    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":32,"tag":"p","attrs":[]},{"op":"text","id":33,"data":"alpha beta gamma"},{"op":"ins","parent":4,"id":32,"before":null},{"op":"ins","parent":32,"id":33,"before":null}]})",
              "words measured before the trim");
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthsRequest) == 2 && !widths_since(a.loop.backend, calls).empty(),
         "the words the trim forgot are asked for again in one request, and the breaks of the word broken anywhere in one more");
    a.frame("the last frame", true, remote::Paint::IfDirty);
    a.close();
}

/// A letter-spaced line (11), a word-spaced number with a no-break space (13), and a letter-spaced word (16) under a display:none box (14).
const char* SPACED = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[["style","letter-spacing:2px"]]},
 {"op":"text","id":11,"data":"Spaced CAPS"},
 {"op":"el","id":12,"tag":"p","attrs":[["style","word-spacing:4px"]]},
 {"op":"text","id":13,"data":"10 000 apart"},
 {"op":"el","id":14,"tag":"div","attrs":[["style","display:none"]]},
 {"op":"el","id":15,"tag":"p","attrs":[["style","letter-spacing:3px"]]},
 {"op":"text","id":16,"data":"QXZ"},
 {"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},{"op":"ins","parent":12,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},{"op":"ins","parent":14,"id":15,"before":null},{"op":"ins","parent":15,"id":16,"before":null}
]})";

bool none_measured(const std::vector<Call>& widths, const std::set<std::string>& texts) {
    for (const Call& c : widths)
        if (texts.count(c.text)) return false;
    return true;
}

/// A run painted with letter or word spacing is drawn unit by unit: the build's one request holds the units of every run it can see, and the paint measures nothing.
void spaced_units() {
    Arms a("spaced units");
    size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(SPACED, "spaced runs, one under a display:none box");
    std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    std::vector<Call> widths = widths_since(a.loop.backend, calls);
    std::set<std::string> asked = texts_of(widths);
    a.ok(one_batch(kinds) && count(kinds, remote::Kind::WidthRequest) == 0, "the build makes one widths request and no single one (" + reports_name(kinds) + ")");
    bool letters = true;
    for (const char* unit : {"S", "p", "a", "c", "e", "d", "C", "A", "P"}) letters = letters && asked.count(unit);
    a.ok(letters, "it holds each letter of the letter-spaced line");
    a.ok(asked.count("10") && asked.count("\xC2\xA0") && asked.count("000"), "and the words and the space the word-spaced number is drawn in");
    a.ok(asked.count("QXZ") && none_measured(widths, {"Q", "X", "Z"}), "the hidden word's own width, but not its letters");
    a.ok(measured_once(widths), "no font and text is measured twice");
    sent = a.loop.wire.kinds.size();
    a.frame("the first frame", true, remote::Paint::IfDirty);
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 0 && count(kinds, remote::Kind::WidthsRequest) == 0, "the paint measures nothing (" + reports_name(kinds) + ")");

    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":14,"name":"display","value":"block"}]})", "the hidden box shown");
    kinds = kinds_since(a.loop, sent);
    asked = texts_of(widths_since(a.loop.backend, calls));
    a.ok(one_batch(kinds) && count(kinds, remote::Kind::WidthRequest) == 0 && asked == std::set<std::string>{"Q", "X", "Z"}, "its letters come in the layout's one request, and nothing else");
    sent = a.loop.wire.kinds.size();
    a.frame("the frame that shows it", true, remote::Paint::IfDirty);
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 0 && count(kinds, remote::Kind::WidthsRequest) == 0, "and its paint measures nothing");

    sent = a.loop.wire.kinds.size();
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":10,"name":"letter-spacing","value":"5px"}]})", "a wider letter spacing");
    a.frame("the frame after it", true, remote::Paint::IfDirty);
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 0 && count(kinds, remote::Kind::WidthsRequest) == 0, "the letters keep their face's widths, the spacing added where they are drawn");
    a.close();
}

/// Two words in a flex row (12, 14), whose items' smallest widths break them, a paragraph that fits (16), and a word in a flex row under a display:none box (20).
const char* BREAKING = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":11,"tag":"div","attrs":[]},
 {"op":"text","id":12,"data":"breakable"},
 {"op":"el","id":13,"tag":"div","attrs":[]},
 {"op":"text","id":14,"data":"pieces"},
 {"op":"el","id":15,"tag":"p","attrs":[]},
 {"op":"text","id":16,"data":"plain fits"},
 {"op":"el","id":17,"tag":"div","attrs":[["style","display:none"]]},
 {"op":"el","id":18,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":19,"tag":"div","attrs":[]},
 {"op":"text","id":20,"data":"VWK"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},{"op":"ins","parent":11,"id":12,"before":null},
 {"op":"ins","parent":10,"id":13,"before":null},{"op":"ins","parent":13,"id":14,"before":null},
 {"op":"ins","parent":4,"id":15,"before":null},{"op":"ins","parent":15,"id":16,"before":null},
 {"op":"ins","parent":4,"id":17,"before":null},{"op":"ins","parent":17,"id":18,"before":null},
 {"op":"ins","parent":18,"id":19,"before":null},{"op":"ins","parent":19,"id":20,"before":null}
]})";

std::string flex_item(int row, int id, const std::string& text) {
    return R"({"op":"el","id":)" + std::to_string(id) + R"(,"tag":"div","attrs":[]},{"op":"text","id":)" + std::to_string(id + 1) + R"(,"data":")" + text +
           R"("},{"op":"ins","parent":)" + std::to_string(id) + R"(,"id":)" + std::to_string(id + 1) + R"(,"before":null},{"op":"ins","parent":)" + std::to_string(row) +
           R"(,"id":)" + std::to_string(id) + R"(,"before":null})";
}

/// The breaks a layout measures: its first unmeasured one asks, in one request, for the break points of every text it may wrap that it can see; each is measured once and none alone, a relayout asks for nothing, and a later layout for its new letters alone.
void breaks() {
    Arms a("breaks");
    size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(BREAKING, "words in a flex row, a paragraph and a hidden row");
    std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    std::vector<Call> widths = widths_since(a.loop.backend, calls);
    std::set<std::string> asked = texts_of(widths);
    a.ok(batches_of(kinds, 2) && count(kinds, remote::Kind::WidthRequest) == 0,
         "the build asks for its words in one widths request and its layout for the breaks in one more, with no single width (" + reports_name(kinds) + ")");
    bool letters = true;
    for (const char* letter : {"b", "r", "e", "a", "k", "l", "p", "i", "c", "s"}) letters = letters && asked.count(letter);
    a.ok(letters, "the breaks hold every letter of the row's words");
    a.ok(asked.count("f") && asked.count("t"), "and of the paragraph, which the layout may wrap too");
    a.ok(asked.count("VWK") && none_measured(widths, {"V", "W", "K"}), "the hidden word's own width, but not its letters");
    a.ok(measured_once(widths), "no font and text is measured twice");
    sent = a.loop.wire.kinds.size();
    a.frame("the first frame", true, remote::Paint::IfDirty);
    a.set_width("a narrower view", 200);
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 0 && count(kinds, remote::Kind::WidthsRequest) == 0, "the paint and a relayout at another width measure nothing");

    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[)" + flex_item(10, 30, "QXZ") + "]}", "a new word in the row");
    kinds = kinds_since(a.loop, sent);
    asked = texts_of(widths_since(a.loop.backend, calls));
    a.ok(batches_of(kinds, 2) && count(kinds, remote::Kind::WidthRequest) == 0 && asked == std::set<std::string>{"QXZ", "Q", "X", "Z"},
         "the word in one request, and its new letters alone in the layout's one more");

    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":40,"tag":"div","attrs":[["style","display:flex"]]},{"op":"ins","parent":4,"id":40,"before":null},)" + flex_item(40, 41, "JJ") + "," +
                  flex_item(10, 43, "YY") + "]}",
              "new words in two rows");
    kinds = kinds_since(a.loop, sent);
    asked = texts_of(widths_since(a.loop.backend, calls));
    a.ok(batches_of(kinds, 2) && count(kinds, remote::Kind::WidthRequest) == 0 && asked == std::set<std::string>{"JJ", "YY", "J", "Y"},
         "two boxes that break in one layout share its one request for breaks");
    a.frame("the last frame", true, remote::Paint::IfDirty);
    a.close();
}

/// html(2) head(3) body(4), a paragraph (10) whose text (11) is all it holds.
const char* SOLE_TEXT = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"alpha beta"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]})";

std::string data_op(const std::string& text) {
    return R"({"type":"ops","ops":[{"op":"data","id":11,"data":")" + text + R"("}]})";
}

/// A sole text patched in place: the document reads the new words' sizes before any layout, one request each, until a size differs; the rest wait for the layout's batch.
void patched_words() {
    Arms a("patches");
    a.receive(SOLE_TEXT, "a paragraph whose text is all it holds");
    a.frame("the first frame", true, remote::Paint::IfDirty);
    size_t sent = a.loop.wire.kinds.size();
    a.receive(data_op("gamma zeta"), "new words of the same widths");
    std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 2 && count(kinds, remote::Kind::WidthsRequest) == 0,
         "each new word is read before any layout, one width request each (" + reports_name(kinds) + ")");
    a.frame("the frame after it", true, remote::Paint::IfDirty);
    sent = a.loop.wire.kinds.size();
    a.receive(data_op("epsilons iota"), "a longer first word");
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 1 && count(kinds, remote::Kind::WidthsRequest) == 1,
         "the first word is read, and the layout its new width needs asks for the rest in one request");
    a.frame("the frame after the longer word", true, remote::Paint::IfDirty);
    sent = a.loop.wire.kinds.size();
    a.receive(data_op("one two three"), "a text of more words");
    kinds = kinds_since(a.loop, sent);
    a.ok(count(kinds, remote::Kind::WidthRequest) == 0 && count(kinds, remote::Kind::WidthsRequest) == 1, "new text elements are measured in the layout's one request");
    a.frame("the last frame", true, remote::Paint::IfDirty);
    a.close();
}

/// More different words in one face than its cache keeps: the batch takes what the cache keeps, the rest are measured one by one, none twice.
void cache_pressure() {
    Arms a("cache pressure");
    std::string words;
    for (int i = 0; i < 4200; i++) words += "w" + std::to_string(i) + " ";
    const size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},)"
              R"({"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},{"op":"el","id":10,"tag":"p","attrs":[]},)"
              R"({"op":"text","id":11,"data":")" + words + R"("},{"op":"ins","parent":4,"id":10,"before":null},{"op":"ins","parent":10,"id":11,"before":null}]})",
              "a paragraph of 4,200 different words in one face");
    const std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    const std::vector<Call> widths = widths_since(a.loop.backend, calls);
    const size_t singles = count(kinds, remote::Kind::WidthRequest), batches = count(kinds, remote::Kind::WidthsRequest), batched = widths.size() - singles;
    const size_t per = remote::Caps().widths;
    a.ok(measured_once(widths) && widths.size() == 4201, "each word and the space measured once (" + std::to_string(widths.size()) + " measured)");
    a.ok(batched == 4096 && batches == (batched + per - 1) / per && singles == 4201 - 4096,
         "the batches take what the face keeps, in requests of at most " + std::to_string(per) + ", the rest one by one (" + std::to_string(batched) + " in " + std::to_string(batches) +
             " requests, " + std::to_string(singles) + " alone)");
    a.frame("the frame", true, remote::Paint::IfDirty);
    a.close();
}

/// Twenty words of many lengths, one of 30 bytes, and the space between them, in the body's one face, none wider than the line.
const char* MANY_WORDS = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"p","attrs":[]},
 {"op":"text","id":11,"data":"a bb ccc dddd eeeee ffffff ggggggg hhhhhhhh iiiiiiiii jjjjjjjjjj kk lll mmmm nnnnn oooooo ppppppp qqqqqqqqqqqqqqqqqqqqqqqqqqqqqq rr sss tttt"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]})";

struct BatchLog : LogBackend {
    std::vector<float> text_widths(const std::vector<TextInFont>& texts) override {
        batches.push_back(texts.size());
        return LogBackend::text_widths(texts);
    }

    std::vector<size_t> batches;
};

/// A renderer end's channel straight into a platform end on a Backend that notes each batch it is handed, keeping every request and answer.
struct Straight : remote::RendererChannel {
    explicit Straight(const remote::Caps& caps) : caps(caps), platform(backend, host, queue, caps) {}

    void send(std::string message) override {
        if (!platform.receive(message)) refused++;
    }
    bool request(std::string message, std::string& answer) override {
        requests.push_back(message);
        if (!platform.answer(message, answer)) {
            refused++;
            return false;
        }
        answers.push_back(answer);
        return true;
    }

    remote::Caps caps;
    BatchLog backend;
    Heard host{backend};
    Queue queue;
    remote::PlatformEnd platform;
    std::vector<std::string> requests, answers;
    size_t refused = 0;
};

/// Under caps of each kind, the words go in requests and answers within every cap, a text no batch can hold alone goes as a single width request, and the platform end makes the direct renderer's Backend calls, each request in one Backend call.
void bounded_batches() {
    struct Case {
        std::string what;
        size_t widths, widths_text, message;
        size_t requests, singles;
    };
    const remote::Caps defaults;
    const Case cases[] = {
        {"the default caps", defaults.widths, defaults.widths_text, defaults.message, 1, 0},
        {"4 texts", 4, defaults.widths_text, defaults.message, 6, 0},
        {"1 text", 1, defaults.widths_text, defaults.message, 21, 0},
        {"0 texts", 0, defaults.widths_text, defaults.message, 0, 21},
        {"16 bytes of texts, less than the long word's", defaults.widths, 16, defaults.message, 8, 1},
        {"a message of 96 bytes", defaults.widths, defaults.widths_text, 96, 4, 0},
        {"40 bytes of texts", defaults.widths, 40, defaults.message, 4, 0},
        {"21 texts, all of them", 21, defaults.widths_text, defaults.message, 1, 0},
        {"20 texts, one fewer", 20, defaults.widths_text, defaults.message, 2, 0},
        {"122 bytes of texts, theirs exactly", defaults.widths, 122, defaults.message, 1, 0},
        {"121 bytes of texts, a byte short", defaults.widths, 121, defaults.message, 2, 0},
        {"a message of 296 bytes, the request of all 21 exactly", defaults.widths, defaults.widths_text, 296, 1, 0},
        {"a message of 295 bytes, a byte short", defaults.widths, defaults.widths_text, 295, 2, 0},
    };
    for (const Case& c : cases) {
        const std::string what = "under a cap of " + c.what;
        remote::Caps caps;
        caps.widths = c.widths;
        caps.widths_text = c.widths_text;
        caps.message = c.message;
        VirtualClock direct_clock, clock;
        LogBackend direct_backend;
        Renderer direct(300, 1, false, direct_backend, &direct_clock);
        Straight channel(caps);
        remote::RendererEnd end(300, 1, false, channel, caps, &clock);
        const std::vector<Json> want = direct.receive(parse(MANY_WORDS));
        const std::vector<Json> got = end.receive(parse(MANY_WORDS));
        std::string why;
        check(!end.stopped() && channel.refused == 0 && dumps(got) == dumps(want), what + ": both ends stay healthy and give the direct renderer's messages");
        check(same_calls(direct_backend.calls, channel.backend.calls, why), what + ": the platform end makes the direct renderer's Backend calls " + why);
        size_t requests = 0, singles = 0, within = 0, items = 0;
        std::vector<size_t> sizes;
        for (size_t i = 0; i < channel.requests.size() && i < channel.answers.size(); i++) {
            const std::optional<remote::Kind> kind = remote::kind_of(channel.requests[i]);
            if (kind == remote::Kind::WidthRequest) singles++;
            if (kind != remote::Kind::WidthsRequest) continue;
            requests++;
            remote::WidthsRequest request;
            remote::WidthsAnswer answer;
            size_t text = 0;
            if (remote::decode(channel.requests[i], caps, request) && remote::decode(channel.answers[i], caps, request, answer)) {
                for (const TextInFont& t : request.texts) text += t.text.size();
                within += request.texts.size() <= c.widths && text <= c.widths_text && channel.requests[i].size() <= c.message && channel.answers[i].size() <= c.message;
            }
            items += request.texts.size();
            sizes.push_back(request.texts.size());
        }
        check(within == requests, what + ": every request and answer is within the caps");
        check(sizes == channel.backend.batches, what + ": each request reaches the Backend in one call of its texts");
        const size_t measured = widths_since(channel.backend, 0).size();
        check(items + singles == measured, what + ": every word goes in a batch or alone, once");
        check(requests == c.requests && singles == c.singles && items + singles == 21,
              what + ": the 21 texts go in " + std::to_string(c.requests) + " requests and " + std::to_string(c.singles) + " single widths, not " + std::to_string(requests) + " and " +
                  std::to_string(singles));
    }
}

/// With batches of 4, a build's second widths request fails every way: the first was answered, nothing goes out after the second, and the end stops.
void batch_failures() {
    for (Spoil how : SPOILS) {
        const std::string what = "the second of three widths requests " + spoil_name(how);
        remote::Caps caps;
        caps.widths = 4;
        VirtualClock clock;
        Loopback loop(caps);
        loop.open(300, 1, false, &clock);
        loop.receive(parse(TALL));
        const size_t batches = count(loop.wire.kinds, remote::Kind::WidthsRequest), states = loop.host.states.size();
        loop.wire.fault = {how, loop.wire.requests + 2, std::nullopt, {}};
        const std::vector<Json> got = loop.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"div","attrs":[]},{"op":"text","id":31,"data":"one two three four five six seven eight nine ten"},{"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":30,"id":31,"before":null}]})"));
        check(got.empty(), what + ": the engine message gets nothing back");
        stopped_at_spoil(loop, remote::Kind::WidthsRequest, reason_for(how), what);
        check(count(loop.wire.kinds, remote::Kind::WidthsRequest) == batches + 2 && loop.host.states.size() == states, what + ": the first request went, no third and no report");
        inert(loop, what);
    }
}

/// Under batches of 4 the layout's breaks go in several requests, each within the caps, and the platform end makes the direct renderer's Backend calls.
void bounded_breaks() {
    remote::Caps caps;
    caps.widths = 4;
    VirtualClock direct_clock, clock;
    LogBackend direct_backend;
    Renderer direct(300, 1, false, direct_backend, &direct_clock);
    Straight channel(caps);
    remote::RendererEnd end(300, 1, false, channel, caps, &clock);
    const std::vector<Json> want = direct.receive(parse(BREAKING));
    const std::vector<Json> got = end.receive(parse(BREAKING));
    std::string why;
    check(!end.stopped() && channel.refused == 0 && dumps(got) == dumps(want), "breaks in batches of 4: both ends stay healthy and give the direct renderer's messages");
    check(same_calls(direct_backend.calls, channel.backend.calls, why), "breaks in batches of 4: the platform end makes the direct renderer's Backend calls " + why);
    size_t requests = 0, within = 0, singles = 0;
    for (const std::string& request : channel.requests) {
        const std::optional<remote::Kind> kind = remote::kind_of(request);
        singles += kind == remote::Kind::WidthRequest;
        if (kind != remote::Kind::WidthsRequest) continue;
        requests++;
        remote::WidthsRequest decoded;
        within += remote::decode(request, caps, decoded) && decoded.texts.size() <= caps.widths;
    }
    check(requests > 4 && within == requests && singles == 0, "breaks in batches of 4: the words and the breaks go in " + std::to_string(requests) + " requests within the cap, none alone");
}

/// The layout's request for breaks fails every way: the words' request was answered, nothing goes out after it, and the end stops.
void break_failures() {
    for (Spoil how : SPOILS) {
        const std::string what = "the request for breaks " + spoil_name(how);
        VirtualClock clock;
        Loopback loop;
        loop.open(300, 1, false, &clock);
        loop.receive(parse(BREAKING));
        const size_t batches = count(loop.wire.kinds, remote::Kind::WidthsRequest), states = loop.host.states.size();
        loop.wire.fault = {how, loop.wire.requests + 2, std::nullopt, {}};
        const std::vector<Json> got = loop.receive(parse(R"({"type":"ops","ops":[)" + flex_item(10, 30, "QXZ") + "]}"));
        check(got.empty(), what + ": the engine message gets nothing back");
        stopped_at_spoil(loop, remote::Kind::WidthsRequest, reason_for(how), what);
        check(count(loop.wire.kinds, remote::Kind::WidthsRequest) == batches + 2 && count(loop.wire.kinds, remote::Kind::WidthRequest) == 0 && loop.host.states.size() == states,
              what + ": the words' request went, then the failed one, and nothing after");
        inert(loop, what);
    }
}

// ───────────────────────── painted frames as deltas ─────────────────────────

/// The kind of the last frame the renderer end sent, whole or as a delta.
remote::Kind last_frame(const Loopback& loop) {
    for (auto it = loop.wire.kinds.rbegin(); it != loop.wire.kinds.rend(); ++it)
        if (*it == remote::Kind::Frame || *it == remote::Kind::FrameDelta) return *it;
    return remote::Kind(0);
}

std::string frame_name(remote::Kind kind) {
    return kind == remote::Kind::FrameDelta ? "a delta" : kind == remote::Kind::Frame ? "whole" : "nothing";
}

/// A painted frame of `a` that crossed as `kind`.
void crossed_as(Arms& a, const std::string& what, remote::Kind kind) {
    const Arms::Step f = a.frame(what, true, remote::Paint::IfDirty);
    a.ok(f.frame->painted, "it is painted");
    a.ok(last_frame(a.loop) == kind, "it crossed " + frame_name(kind) + ", not " + frame_name(last_frame(a.loop)));
}

/// The bytes of the list the direct renderer painted last.
size_t list_size(Renderer& r) {
    std::string bytes;
    wire::encode(r.paint(), {}, bytes);
    return bytes.size();
}

/// `count` divs of a word each from element `first`, which stay still beside an animation, or their removal.
std::string words(bool add, int count, int first = 100) {
    std::string ops;
    for (int i = 0; i < count; i++) {
        const std::string id = std::to_string(first + 2 * i), text = std::to_string(first + 1 + 2 * i);
        ops += std::string(ops.empty() ? "" : ",")
            + (add ? R"({"op":"el","id":)" + id + R"(,"tag":"div","attrs":[]},{"op":"text","id":)" + text + R"(,"data":"word )" + id + R"("},{"op":"ins","parent":)" + id
                       + R"(,"id":)" + text + R"(,"before":null},{"op":"ins","parent":4,"id":)" + id + R"(,"before":null})"
                   : R"({"op":"rm","id":)" + id + "}");
    }
    return R"({"type":"ops","ops":[)" + ops + "]}";
}

/// `count` boxes from element `first`, all painted alike in one place, or their removal.
std::string boxes(bool add, int count, int first) {
    std::string ops;
    for (int i = 0; i < count; i++) {
        const std::string id = std::to_string(first + i);
        ops += std::string(ops.empty() ? "" : ",")
            + (add ? R"({"op":"el","id":)" + id + R"(,"tag":"div","attrs":[["style","position:absolute;left:0;top:0;width:10px;height:10px;background:#c03"]]},{"op":"ins","parent":4,"id":)"
                       + id + R"(,"before":null})"
                   : R"({"op":"rm","id":)" + id + "}");
    }
    return R"({"type":"ops","ops":[)" + ops + "]}";
}

/// An animation's painted frames cross as deltas that rebuild the direct renderer's lists and damage, the first whole, an unpainted frame keeping the base; a list past delta_base goes whole and drops it, and 0 sends every frame whole.
void deltas() {
    {
        Arms a("deltas");
        a.receive(SPINNING, "a spinner");
        a.receive(words(true, 10), "10 words beside it");
        crossed_as(a, "the first frame", remote::Kind::Frame);
        for (int i = 1; i <= 8; i++) {
            a.advance(16);
            crossed_as(a, "a spinner frame at +" + std::to_string(16 * i) + " ms", remote::Kind::FrameDelta);
        }
        a.advance(16);
        a.ok(!a.frame("a ticking frame that skips the paint", true, remote::Paint::Skip).frame->painted, "it is unpainted");
        crossed_as(a, "the frame after it", remote::Kind::FrameDelta);
        a.ok(!a.frame("a clean frame", false, remote::Paint::IfDirty).frame->painted, "a clean frame is unpainted");
        a.ok(a.frame("a forced frame of the same list", false, remote::Paint::Force).frame->painted && last_frame(a.loop) == remote::Kind::FrameDelta, "a forced repaint crosses as a delta");
        a.receive(FRESH_TEXT, "a text added");
        a.advance(16);
        crossed_as(a, "the frame with the text", remote::Kind::FrameDelta);
        a.receive(boxes(true, 2, 300), "two boxes painted alike");
        a.advance(16);
        crossed_as(a, "the frame with them", remote::Kind::FrameDelta);
        a.receive(boxes(true, 60, 400), "60 more boxes painted alike, more than the last list holds");
        a.advance(16);
        a.frame("the frame with them, which copies no more than the last list holds", true, remote::Paint::IfDirty);
        a.advance(16);
        crossed_as(a, "the frame after it", remote::Kind::FrameDelta);
        a.close();
        a.need(count(a.loop.wire.kinds, remote::Kind::FrameDelta), "frames as deltas");
    }
    for (const bool nested : {false, true}) {
        Arms a(nested ? "a spinner in a spinner" : "a spinner alone");
        a.receive(SPINNING, "a spinner");
        if (nested) a.receive(R"({"type":"ops","ops":[{"op":"el","id":11,"tag":"div","attrs":[["class","spin"]]},{"op":"ins","parent":10,"id":11,"before":null}]})", "a spinner in it");
        crossed_as(a, "the first frame", remote::Kind::Frame);
        for (int i = 1; i <= 3; i++) {
            a.advance(16);
            crossed_as(a, nested ? "a frame whose delta would copy its pops and still be longer" : "a frame with nothing worth copying", remote::Kind::Frame);
        }
        a.close();
    }
    size_t spinner = 0;
    {
        VirtualClock clock;
        LogBackend backend;
        Renderer r(300, 2, false, backend, &clock);
        r.receive(parse(SPINNING));
        r.receive(parse(words(true, 10)));
        r.tick();
        spinner = list_size(r);
    }
    {
        remote::Caps caps;
        caps.delta_base = spinner + 64;
        Arms a("deltas past delta_base", caps);
        a.receive(SPINNING, "a spinner");
        a.receive(words(true, 10), "10 words beside it");
        crossed_as(a, "the first frame", remote::Kind::Frame);
        a.advance(16);
        crossed_as(a, "a spinner frame", remote::Kind::FrameDelta);
        a.receive(words(true, 40, 200), "40 words more, which take the list past delta_base");
        a.advance(16);
        crossed_as(a, "the first frame past delta_base", remote::Kind::Frame);
        a.advance(16);
        crossed_as(a, "the next frame past it", remote::Kind::Frame);
        a.receive(words(false, 40, 200), "those words removed");
        a.advance(16);
        crossed_as(a, "the first frame back within delta_base, with no list kept", remote::Kind::Frame);
        a.advance(16);
        crossed_as(a, "the next", remote::Kind::FrameDelta);
        a.close();
    }
    for (size_t base : {size_t(0), size_t(1)}) {
        remote::Caps caps;
        caps.delta_base = base;
        Arms a("deltas under a delta_base of " + std::to_string(base), caps);
        a.receive(SPINNING, "a spinner");
        a.receive(words(true, 10), "10 words beside it");
        for (int i = 0; i < 4; i++) {
            a.advance(16);
            crossed_as(a, "a spinner frame", remote::Kind::Frame);
        }
        a.close();
        a.ok(count(a.loop.wire.kinds, remote::Kind::FrameDelta) == 0, "no frame crossed as a delta");
    }
}

/// Delivers the queued platform input to the renderer end and keeps what it sends last in the outbox, for a test to change before it reaches the platform end.
void deliver_held(Loopback& loop) {
    loop.wire.flush();
    while (!loop.queue.messages.empty()) {
        const std::string message = std::move(loop.queue.messages.front());
        loop.queue.messages.pop_front();
        loop.delivered.push_back(loop.end->deliver(message, loop.out));
        loop.outputs.push_back(loop.out);
    }
}

/// A spinner whose renderer end has painted twice, so that its next painted frame is a delta, asked for and held in the outbox.
void spinning_held(Loopback& loop, VirtualClock& clock, const char* page = SPINNING) {
    loop.open(300, 2, false, &clock);
    loop.receive(parse(page));
    loop.receive(parse(words(true, 10)));
    for (int i = 0; i < 2; i++) {
        clock.ms += 16;
        loop.platform.request_frame(true, remote::Paint::IfDirty);
        loop.pump();
    }
    clock.ms += 16;
    loop.platform.request_frame(true, remote::Paint::IfDirty);
    deliver_held(loop);
}

std::string* held_delta(Loopback& loop) {
    for (std::string& message : loop.wire.outbox)
        if (Wire::kind(message) == remote::Kind::FrameDelta) return &message;
    return nullptr;
}

void put_u64(std::string& bytes, size_t at, uint64_t v) {
    for (size_t i = 0; i < 8; i++) bytes[at + i] = char((v >> (8 * i)) & 0xff);
}

/// Both ends and the clock their renderer reads, made first so that it outlives the renderer.
struct Route {
    VirtualClock clock;
    Loopback loop;
};

/// A delta changed on its way stops the platform end with its reason before the host or the Backend hears of it, and a delta of one route is refused by another, whose base it is not.
void tampered_deltas() {
    const struct {
        const char* what;
        void (*change)(std::string&);
        const char* reason;
    } changes[] = {
        {"a run of tag 2", [](std::string& m) { m[49] = 2; }, "a message of kind 73 did not rebuild a frame"},
        {"a byte cut", [](std::string& m) { m.pop_back(); }, "a message of kind 73 did not rebuild a frame"},
        {"a byte after it", [](std::string& m) { m.push_back('\0'); }, "a message of kind 73 did not rebuild a frame"},
        {"a declared length one longer", [](std::string& m) { m[41] = char(uint8_t(m[41]) + 1); }, "a message of kind 73 did not rebuild a frame"},
        {"the next request's seq", [](std::string& m) { put_u64(m, 2, 4); }, "a message of kind 73 for request 4 where 3 was due"},
        {"the first frame as its base", [](std::string& m) { put_u64(m, 33, 1); }, "a message of kind 73 copying from frame 1 where 2 was painted last"},
        {"cut before its base", [](std::string& m) { m.resize(40); }, "a message of kind 73 did not decode"},
    };
    for (const auto& c : changes) {
        const std::string what = std::string("a delta with ") + c.what;
        Route route;
        Loopback& loop = route.loop;
        spinning_held(loop, route.clock);
        std::string* delta = held_delta(loop);
        check(delta != nullptr && !loop.end->stopped(), what + ": the third painted frame is a delta");
        if (!delta) continue;
        c.change(*delta);
        const size_t frames = loop.host.frames.size(), calls = loop.backend.calls.size();
        loop.wire.flush();
        check(loop.platform.refusing() && loop.host.refusals.size() == 1 && loop.host.refusals[0] == c.reason,
            what + ": stops the platform end with \"" + c.reason + "\", not \"" + (loop.host.refusals.empty() ? std::string() : loop.host.refusals[0]) + "\"");
        check(loop.host.frames.size() == frames && loop.backend.calls.size() == calls, what + ": no frame reaches the host and no Backend call is made");
        check(loop.wire.refused == 1, what + ": the platform end refused that message alone");
    }
    Route spin_route, fade_route;
    Loopback &spin = spin_route.loop, &fade = fade_route.loop;
    spinning_held(spin, spin_route.clock);
    spinning_held(fade, fade_route.clock, FADE);
    std::string* from_spin = held_delta(spin);
    std::string* from_fade = held_delta(fade);
    check(from_spin && from_fade, "both routes' third painted frames are deltas");
    if (!from_spin || !from_fade) return;
    const std::string own_fade = *from_fade;
    *from_fade = *from_spin;
    const size_t frames = fade.host.frames.size(), calls = fade.backend.calls.size();
    fade.wire.flush();
    check(fade.platform.refusing() && fade.host.refusals.size() == 1 && fade.host.frames.size() == frames && fade.backend.calls.size() == calls,
        "a delta of one route reaching another is refused there, nothing heard: " + (fade.host.refusals.empty() ? std::string("no reason") : fade.host.refusals[0]));
    spin.wire.flush();
    check(!spin.platform.refusing() && spin.host.refusals.empty() && spin.host.frames.size() == 3 && spin.host.frames.back().painted, "the route it came from takes it");
    Route again_route;
    Loopback& again = again_route.loop;
    spinning_held(again, again_route.clock, FADE);
    std::string* own = held_delta(again);
    check(own && *own == own_fade, "the other route's own delta is the same each time");
    again.wire.flush();
    check(!again.platform.refusing() && again.host.frames.size() == 3, "and its own route takes it");
}

/// The bytes of a painted Frame's list, and its units: its head and each command.
struct ListSize {
    size_t bytes = 0, units = 0;
};

ListSize list_size_of(const DisplayList& list) {
    std::string bytes;
    std::vector<size_t> starts;
    wire::encode(list, {}, bytes, starts);
    return {bytes.size(), starts.size()};
}

static_assert(noexcept(std::declval<remote::PlatformEnd&>().close()), "a platform end closes without throwing");

/// Runs `call` with every allocation of this thread refused, and gives how many it asked for.
size_t denied_in(const std::function<void()>& call) {
    watch.denied = 0;
    watch.deny = true;
    call();
    watch.deny = false;
    return watch.denied;
}

// Single-threaded WebAssembly cannot start a thread.
#if !defined(__EMSCRIPTEN__) || defined(__EMSCRIPTEN_PTHREADS__)
#define TW_THREADS
#endif

#ifdef TW_THREADS
/// What a watch has counted so far.
struct Counted {
    size_t used = 0, live = 0, alive = 0, asked = 0, made = 0;

    bool operator==(const Counted&) const = default;
};

Counted counted() {
    return {watch.used, watch.live, watch.alive, watch.asked, watch.made};
}
#endif

/// A spinner's first painted frame, held whole in the outbox before it reaches the platform end; the bytes of its list.
size_t first_frame_held(Loopback& loop, VirtualClock& clock) {
    loop.open(300, 2, false, &clock);
    loop.receive(parse(SPINNING));
    loop.receive(parse(words(true, 40)));
    loop.platform.request_frame(true, remote::Paint::IfDirty);
    deliver_held(loop);
    const std::string* held = loop.wire.outbox.empty() ? nullptr : &loop.wire.outbox.back();
    remote::Frame frame;
    std::string_view view;
    check(held && remote::decode(*held, {}, frame, view) && frame.painted && !view.empty(), "the first painted frame waits whole in the outbox");
    return view.size();
}

/// A platform end its embedder closes, with a painted list kept and a reply due, gives the list back without a word to its host and without asking for memory, and closing again changes nothing; meanwhile, where a thread can start, another thread's block passes the watch by.
void closed_platform() {
    Route route;
    Loopback& loop = route.loop;
    const size_t list = first_frame_held(loop, route.clock);
#ifdef TW_THREADS
    std::atomic<int> stage = 0;
    std::atomic<void*> foreign = nullptr;
    std::thread other([&] {
        while (stage.load() != 1) std::this_thread::yield();
        foreign.store(::operator new(4096));
        ::operator delete(foreign.load());
        stage.store(2);
    });
#endif
    watch.start();
    loop.wire.flush();
#ifdef TW_THREADS
    const Counted before = counted();
    stage.store(1);
    while (stage.load() != 2) std::this_thread::yield();
    const Counted after = counted();
    watch.stop();
    other.join();
    check(foreign.load() != nullptr && after == before, "another thread's block, made and freed while the watch is on, leaves its counts as they were");
#else
    watch.stop();
#endif
    const size_t kept = watch.live;
    check(!watch.full && loop.host.frames.size() == 1 && !loop.platform.refusing() && kept >= list, "the platform end takes it and keeps its list");
    check(loop.platform.request_frame(true, remote::Paint::IfDirty) == 2 && loop.queue.messages.size() == 1, "and asks for the next frame");
    const size_t events = loop.host.events.size();
    check(denied_in([&] { loop.platform.close(); }) == 0, "closing with a list kept and a reply due asks for no memory");
    const size_t freed = kept - watch.live;
    check(freed >= list && freed <= list + 64, "closing gives back the kept list's " + std::to_string(list) + " bytes (" + std::to_string(freed) + " freed)");
    check(loop.platform.refusing() && loop.host.refusals.empty() && loop.host.events.size() == events, "the host hears nothing of it");
    const size_t live = watch.live;
    check(denied_in([&] { loop.platform.close(); }) == 0 && watch.live == live && loop.host.refusals.empty() && loop.host.events.size() == events, "closing again asks for nothing and changes nothing");
    const size_t sent = loop.wire.kinds.size(), refused = loop.wire.refused;
    check(loop.pump() && loop.wire.kinds.size() > sent && loop.wire.refused - refused == loop.wire.kinds.size() - sent, "the platform end refuses all the renderer end sends for the frame it asked for");
    check(loop.host.refusals.empty() && loop.host.events.size() == events, "which reaches no host");
    check(loop.platform.request_frame(true, remote::Paint::IfDirty) == 0 && loop.host.refusals.empty(), "a closed end asks for no frame");
    loop.close();
    check(loop.host.refusals.empty() && loop.host.events.size() == events, "and the renderer end's last messages reach no host");
}

/// A message that does not decode stops a platform end with a painted list kept and a reply due: it gives the list back and its host hears the reason once, whatever comes after.
void malformed_platform() {
    Route route;
    Loopback& loop = route.loop;
    const size_t list = first_frame_held(loop, route.clock);
    watch.start();
    loop.wire.flush();
    watch.stop();
    const size_t kept = watch.live;
    check(!watch.full && loop.host.frames.size() == 1 && kept >= list && loop.platform.request_frame(true, remote::Paint::IfDirty) == 2, "the platform end keeps the list and asks for the next frame");
    const size_t events = loop.host.events.size();
    const std::string cut{char(remote::Kind::Frame), char(remote::VERSION)};
    check(!loop.platform.receive(cut) && loop.platform.refusing(), "a frame cut after its version stops the platform end");
    const size_t live = watch.live, freed = kept - live;
    check(freed >= list && freed <= list + 64, "the stop gives back the kept list's " + std::to_string(list) + " bytes (" + std::to_string(freed) + " freed)");
    check(loop.host.refusals == std::vector<std::string>{"a message of kind 64 did not decode"} && loop.host.events.size() == events, "the host hears the reason once and nothing else");
    const size_t sent = loop.wire.kinds.size(), refused = loop.wire.refused;
    check(!loop.platform.receive(cut) && loop.pump() && loop.wire.kinds.size() > sent && loop.wire.refused - refused == loop.wire.kinds.size() - sent,
          "the same message again and all the renderer end sends for the frame asked for are refused");
    check(loop.host.refusals.size() == 1 && loop.host.events.size() == events && watch.live == live, "quietly, with nothing more to give back");
    loop.close();
    check(loop.host.refusals.size() == 1 && loop.host.events.size() == events, "and the renderer end's last messages reach no host");
}

/// A renderer end's channel whose requests a platform end answers and whose messages it keeps, the last one only, so that a paint's allocations are its own.
struct Kept : remote::RendererChannel {
    explicit Kept(const remote::Caps& caps) : platform(backend, host, queue, caps) {}

    void send(std::string message) override { last = std::move(message); }
    bool request(std::string message, std::string& answer) override { return platform.answer(message, answer); }

    LogBackend backend;
    Heard host{backend};
    Queue queue;
    remote::PlatformEnd platform;
    std::string last;
};

/// What one watched call asked for: every byte, and the blocks of at least the size the watch was given.
struct Asked {
    size_t bytes = 0, big = 0, live = 0;
};

Asked asked_by(size_t big, const std::function<void()>& call) {
    watch.start(big);
    call();
    watch.stop();
    check(!watch.full, "the watch keeps every block");
    return {watch.asked, watch.bigs, watch.live};
}

/// A spinner beside a paragraph of 1,500 words at 4 px, all of them in view.
std::vector<std::string> busy_page() {
    std::string text;
    for (int i = 0; i < 1500; i++) text += "w" + std::to_string(i) + " ";
    return {SPINNING, R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"p","attrs":[["style","font:4px sans-serif"]]},{"op":"text","id":31,"data":")" + text
                          + R"("},{"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":30,"id":31,"before":null}]})"};
}

/// With deltas off or past delta_base, every painted frame goes out as the ordinary Frame's bytes, written once: the end asks for no more than the direct renderer's paint and the Frame's own encoding, list-sized blocks included.
void ordinary_frames() {
    for (size_t base : {size_t(0), size_t(64)}) {
        const std::string what = "frames under a delta_base of " + std::to_string(base);
        remote::Caps caps;
        caps.delta_base = base;
        VirtualClock direct_clock, clock;
        LogBackend direct_backend;
        Renderer direct(300, 2, false, direct_backend, &direct_clock);
        Kept channel(caps);
        remote::RendererEnd end(300, 2, false, channel, caps, &clock);
        for (const std::string& page : busy_page()) {
            direct.receive(parse(page));
            end.receive(parse(page));
        }
        size_t list = 0;
        for (uint64_t seq = 1; seq <= 4; seq++) {
            direct_clock.ms += 16;
            clock.ms += 16;
            remote::Frame frame;
            frame.seq = seq;
            std::string ordinary;
            const Asked direct_asked = asked_by(list / 2, [&] {
                direct.tick();
                frame.list = direct.paint();
                std::vector<Rect> damage;
                if (direct.damage(damage)) frame.damage = std::move(damage);
                frame.painted = true;
                frame.state = state_of(direct, false);
                remote::encode(frame, caps, ordinary);
            });
            remote::RendererEnd::Output out;
            const std::string request = encoded(remote::FrameRequest{seq, true, remote::Paint::IfDirty});
            const Asked end_asked = asked_by(list / 2, [&] { end.deliver(request, out); });
            const std::string at = what + ", frame " + std::to_string(seq) + ": ";
            check(!end.stopped() && !ordinary.empty() && channel.last == ordinary, at + "the end sends the ordinary Frame's bytes");
            if (list) {
                check(end_asked.big == direct_asked.big, at + "the end makes as many list-sized blocks as the direct paint and its Frame (" + std::to_string(end_asked.big) + " and " + std::to_string(direct_asked.big) + ")");
                check(end_asked.bytes <= direct_asked.bytes + 512, at + "and asks for no more (" + std::to_string(end_asked.bytes) + " bytes against " + std::to_string(direct_asked.bytes) + ")");
            }
            list = list_size_of(frame.list).bytes;
        }
        check(list > 16384, what + ": the list is " + std::to_string(list) + " bytes");
    }
}

/// After a paint the end keeps the painted list for the next delta: its bytes, the bounds of its units and its index, and nothing more than an end with deltas off keeps.
void kept_storage() {
    remote::Caps off;
    off.delta_base = 0;
    VirtualClock clock, off_clock;
    Kept channel({}), off_channel(off);
    remote::RendererEnd end(300, 2, false, channel, {}, &clock), off_end(300, 2, false, off_channel, off, &off_clock);
    VirtualClock direct_clock;
    LogBackend direct_backend;
    Renderer direct(300, 2, false, direct_backend, &direct_clock);
    for (const std::string& page : busy_page()) {
        direct.receive(parse(page));
        end.receive(parse(page));
        off_end.receive(parse(page));
    }
    for (uint64_t seq = 1; seq <= 3; seq++) {
        direct_clock.ms += 16;
        clock.ms += 16;
        off_clock.ms += 16;
        direct.tick();
        const ListSize list = list_size_of(direct.paint());
        const std::string request = encoded(remote::FrameRequest{seq, true, remote::Paint::IfDirty});
        remote::RendererEnd::Output out, off_out;
        const auto kept_by = [&](remote::RendererEnd& e, Kept& c, remote::RendererEnd::Output& o) {
            return asked_by(SIZE_MAX, [&] {
                e.deliver(request, o);
                o = {};
                std::string().swap(c.last);
            }).live;
        };
        const size_t kept = kept_by(end, channel, out), kept_off = kept_by(off_end, off_channel, off_out);
        const size_t base = kept - kept_off, most = list.bytes + 4 * (list.units + 1) + 16 * (list.units - 1) + 128;
        const std::string at = "frame " + std::to_string(seq) + " of a list of " + std::to_string(list.bytes) + " bytes in " + std::to_string(list.units) + " units: ";
        check(!end.stopped() && !off_end.stopped() && kept >= kept_off, at + "both ends paint");
        check(base >= list.bytes && base <= most, at + "the end keeps the list, its bounds and its index alone (" + std::to_string(base) + " bytes, at most " + std::to_string(most) + ")");
    }
}

/// A word of 700 letters in a box 300 px wide, at 1 px a letter.
const char* LONG_WORD = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:2px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","width:300px"]]},
 {"op":"text","id":11,"data":"LONGWORD"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null}
]})";

/// A warm relayout fits the long word again with probes of hundreds of letters, longer than the width cache keeps: each is measured alone, as before the batches, and none asks for the letters of the texts the layout may break, which a new short text's letters show.
void warm_fit_probes() {
    std::string page = LONG_WORD;
    page.replace(page.find("LONGWORD"), 8, std::string(700, 'x'));
    Arms a("warm fit probes");
    size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(page, "a word of 700 letters in a box 300 px wide");
    std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    std::set<std::string> asked = texts_of(widths_since(a.loop.backend, calls));
    a.ok(count(kinds, remote::Kind::WidthsRequest) == 1 && asked.count("x"), "the first layout asks for the word's letter in a request for breaks (" + reports_name(kinds) + ")");
    a.frame("the first frame", true, remote::Paint::IfDirty);
    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"el","id":20,"tag":"p","attrs":[]},{"op":"text","id":21,"data":"qz"},{"op":"ins","parent":4,"id":20,"before":null},{"op":"ins","parent":20,"id":21,"before":null}]})",
              "a short text after it, which fits its line");
    kinds = kinds_since(a.loop, sent);
    const std::vector<Call> widths = widths_since(a.loop.backend, calls);
    asked = texts_of(widths);
    bool long_probes = !widths.empty();
    for (const Call& c : widths) long_probes = long_probes && (c.text == "qz" || c.text.size() > 256);
    a.ok(count(kinds, remote::Kind::WidthsRequest) == 1 && asked.count("qz") && !asked.count("q") && !asked.count("z"),
         "the relayout asks for the new word in one request and for no breaks (" + reports_name(kinds) + ")");
    a.ok(long_probes && count(kinds, remote::Kind::WidthRequest) > 0, "its fit probes past the cache's longest key are measured alone (" + std::to_string(widths.size()) + " widths)");
    a.frame("the frame after it", true, remote::Paint::IfDirty);
    sent = a.loop.wire.kinds.size();
    calls = a.loop.backend.calls.size();
    a.receive(R"({"type":"ops","ops":[{"op":"style","id":10,"name":"width","value":"100px"},{"op":"el","id":22,"tag":"p","attrs":[]},{"op":"text","id":23,"data":"jk"},{"op":"ins","parent":4,"id":22,"before":null},{"op":"ins","parent":22,"id":23,"before":null}]})",
              "the box 100 px wide and another short text");
    kinds = kinds_since(a.loop, sent);
    asked = texts_of(widths_since(a.loop.backend, calls));
    bool fresh_probes = false;
    for (const std::string& t : asked) fresh_probes = fresh_probes || (t.size() > 1 && t.size() <= 256 && t.find_first_not_of('x') == std::string::npos);
    a.ok(count(kinds, remote::Kind::WidthsRequest) == 1 && asked.count("jk") && !asked.count("j") && !asked.count("k") && fresh_probes,
         "probes the cache could keep but has not measured yet ask for no breaks either, only for themselves (" + reports_name(kinds) + ")");
    a.frame("the last frame", true, remote::Paint::IfDirty);
    a.close();
}

/// A flex item sized at its narrowest whose preserved tab breaks anywhere, in a face of its own: the layout's request for breaks holds the tab's space.
const char* PRESERVED_TAB = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","display:flex"]]},
 {"op":"el","id":11,"tag":"div","attrs":[["style","white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif"]]},
 {"op":"text","id":12,"data":"ab\tcd"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":11,"id":12,"before":null}
]})";

/// The narrowest a content-sized box can be measures the characters of its preserved spaces too: the layout's one request for breaks asks for them with the letters, and no width goes alone.
void preserved_tab() {
    Arms a("a preserved tab");
    const size_t sent = a.loop.wire.kinds.size(), calls = a.loop.backend.calls.size();
    a.receive(PRESERVED_TAB, "a flex item of two words and a tab, preserved and broken anywhere");
    const std::vector<remote::Kind> kinds = kinds_since(a.loop, sent);
    const std::vector<Call> widths = widths_since(a.loop.backend, calls);
    int face = 0;
    for (const Call& c : a.loop.backend.calls)
        if (c.op == Call::Font && c.size == 17) face = c.font;
    bool space = false, letters = true;
    for (const Call& c : widths) space = space || (c.font == face && c.text == " ");
    for (const char* letter : {"a", "b", "c", "d"}) letters = letters && texts_of(widths).count(letter);
    a.ok(batches_of(kinds, 2) && count(kinds, remote::Kind::WidthRequest) == 0, "the build asks for its words and for its breaks in two requests, with no single width (" + reports_name(kinds) + ")");
    a.ok(face != 0 && space && letters, "the breaks hold the letters and the tab's space in the item's face");
    a.ok(measured_once(widths), "no font and text is measured twice");
    a.frame("the first frame", true, remote::Paint::IfDirty);
    a.close();
}

/// A narrow box whose word breaks anywhere in a 13 px face, a box of `tabs` preserved tabs between two letters that breaks anywhere in a 17 px face, and a box whose preserved line break shows nothing in a 19 px face.
std::string tabs_page(int tabs) {
    std::string text = "a";
    for (int i = 0; i < tabs; i++) text += "\\t";
    text += "b";
    return R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font:16px sans-serif"]]},
 {"op":"el","id":10,"tag":"div","attrs":[["style","width:20px;overflow-wrap:anywhere;font:13px monospace"]]},
 {"op":"text","id":11,"data":"abcdefgh"},
 {"op":"el","id":12,"tag":"div","attrs":[["style","width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:17px serif"]]},
 {"op":"text","id":13,"data":")" + text + R"("},
 {"op":"el","id":14,"tag":"div","attrs":[["style","width:300px;white-space:pre-wrap;overflow-wrap:anywhere;font:19px serif"]]},
 {"op":"text","id":15,"data":"ab\ncd"},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null},
 {"op":"ins","parent":12,"id":13,"before":null},
 {"op":"ins","parent":4,"id":14,"before":null},
 {"op":"ins","parent":14,"id":15,"before":null}
]})";
}

/// Another narrow box in the 13 px face, before the tabs, whose word's letters no face has measured.
const char* NEW_LETTERS = R"({"type":"ops","ops":[{"op":"el","id":20,"tag":"div","attrs":[["style","width:20px;overflow-wrap:anywhere;font:13px monospace"]]},{"op":"text","id":21,"data":"mnopqrst"},
 {"op":"ins","parent":4,"id":20,"before":12},{"op":"ins","parent":20,"id":21,"before":null}]})";

/// A Backend that notes, for each widths batch, how many blocks the watch had seen made when it began and when it ended, and where its calls lie in the log.
struct Marking : LogBackend {
    struct Batch {
        size_t began = 0, ended = 0, from = 0, to = 0;
    };

    std::vector<float> text_widths(const std::vector<TextInFont>& texts) override {
        Batch batch{watch.made, 0, calls.size(), 0};
        std::vector<float> widths = LogBackend::text_widths(texts);
        batch.ended = watch.made;
        batch.to = calls.size();
        batches.push_back(batch);
        return widths;
    }

    std::vector<Batch> batches;
};

int font_of_size(const LogBackend& backend, float size) {
    for (const Call& c : backend.calls)
        if (c.op == Call::Font && c.size == size) return c.font;
    return 0;
}

bool asked_in(const LogBackend& backend, const Marking::Batch& batch, int font, const std::string& text) {
    for (size_t i = batch.from; i < batch.to; i++)
        if (backend.calls[i].font == font && backend.calls[i].text == text) return true;
    return false;
}

/// How many blocks a warm relayout makes between its words' request and its breaks' request: the layout up to the first character of the new word, then the scan of every text it may break, the tabs' spaces among them.
size_t discovery_blocks(int tabs) {
    const std::string what = std::to_string(tabs) + " preserved tabs: ";
    VirtualClock clock;
    Marking backend;
    Renderer direct(300, 2, false, backend, &clock);
    backend.batches.reserve(16);
    direct.receive(parse(tabs_page(tabs)));
    const int monospace = font_of_size(backend, 13), tab_face = font_of_size(backend, 17), line_face = font_of_size(backend, 19);
    check(monospace && tab_face && line_face && backend.batches.size() == 2, what + "the cold build asks for its words and its breaks in two requests");
    if (backend.batches.size() != 2) return 0;
    const Marking::Batch cold = backend.batches[1];
    bool letters = true;
    for (const char* letter : {"a", "b", "c", "d"}) letters = letters && asked_in(backend, cold, line_face, letter);
    check(asked_in(backend, cold, tab_face, " ") && asked_in(backend, cold, monospace, "a"), what + "the cold breaks hold the tabs' space and the narrow word's letters");
    check(letters && !asked_in(backend, cold, line_face, " ") && !asked_in(backend, cold, line_face, ""), what + "and the letters around the preserved line break, which adds nothing");
    backend.batches.clear();
    watch.start();
    direct.receive(parse(NEW_LETTERS));
    watch.stop();
    check(!watch.full && backend.batches.size() == 2, what + "the warm relayout asks for its new word and then for its breaks");
    if (backend.batches.size() != 2) return 0;
    const Marking::Batch warm = backend.batches[1];
    bool fresh = warm.to > warm.from;
    for (size_t i = warm.from; i < warm.to; i++) fresh = fresh && backend.calls[i].font == monospace && backend.calls[i].text.size() == 1 && backend.calls[i].text[0] >= 'm' && backend.calls[i].text[0] <= 't';
    check(fresh && asked_in(backend, warm, monospace, "m"), what + "its breaks hold the new letters alone");
    return warm.began - backend.batches[0].ended;
}

/// Break discovery in a warm face asks once for a run of spaces' one character: a relayout that misses a letter in another face makes as many blocks beside 200 preserved tabs as beside 100, and the ends make the direct renderer's calls.
void warm_tabs() {
    const size_t fewer = discovery_blocks(100), more = discovery_blocks(200);
    check(fewer > 0 && more == fewer, "the discovery makes no block for a tab (" + std::to_string(fewer) + " blocks beside 100 tabs, " + std::to_string(more) + " beside 200)");
    Arms a("many preserved tabs");
    a.receive(tabs_page(100), "a narrow word, 100 preserved tabs and a preserved line break");
    a.frame("the first frame", true, remote::Paint::IfDirty);
    a.receive(NEW_LETTERS, "a narrow word of new letters before the tabs");
    a.frame("the frame after it", true, remote::Paint::IfDirty);
    a.close();
}

} // namespace

int main() {
    session();
    construction_failures();
    later_failures();
    mid_call_failures();
    bad_input();
    cap_failures();
    resources();
    frames();
    pointing();
    routing();
    clocks();
    containers();
    batches();
    spaced_units();
    breaks();
    patched_words();
    cache_pressure();
    bounded_batches();
    batch_failures();
    bounded_breaks();
    break_failures();
    deltas();
    tampered_deltas();
    closed_platform();
    malformed_platform();
    ordinary_frames();
    kept_storage();
    warm_fit_probes();
    preserved_tab();
    warm_tabs();
    total.print("all sessions");
    for (int op = 0; op < Call::Frame; op++) check(total.calls[op] > 0, std::string("the sessions compare ") + CALL_NAMES[op]);
    check(total.construction && total.destruction && total.one_way_between() && total.painted && total.unchanged && total.helper_only, "the sessions compare construction, destruction, one-way calls between frames, painted and unchanged frames and the helper's own messages");
    check(total.messages && total.pointers && total.selections && total.frames && total.claims && total.states, "the sessions compare every kind of report");
    std::printf("renderer end: %zu stopped ends, each inert after its stop\n", stop_cases);
    std::printf("%d checks, %d failure(s)\n", checks, failures);
    return failures ? 1 : 0;
}
