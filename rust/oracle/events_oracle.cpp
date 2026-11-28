// The whole renderer with the probes of events_probe.h at its input code (events_oracle.cmake's copy of renderer.cpp, included whole so that its Node is complete here), driven by input scripts for rust/crates/events: scripts, the tape and the transcript are laid out in that crate's tests/common/mod.rs.
#include "renderer.cpp"
#include "wire.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <memory>
#include <string>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

namespace eo {

using twidget::Json;
using twlite::Backend;
using twlite::Clock;
using twlite::FontMetrics;
using twlite::Rect;
using twlite::Renderer;

bool read_exact(void* to, size_t n) {
    return n == 0 || std::fread(to, 1, n, stdin) == n;
}

struct In {
    std::string_view s;
    size_t at = 0;

    template <typename T> T raw() {
        T v{};
        if (at + sizeof v <= s.size()) std::memcpy(&v, s.data() + at, sizeof v);
        at += sizeof v;
        return v;
    }
    uint8_t u8() { return raw<uint8_t>(); }
    int32_t i32() { return raw<int32_t>(); }
    uint32_t u32() { return raw<uint32_t>(); }
    uint64_t u64() { return raw<uint64_t>(); }
    float f32() { return raw<float>(); }
    double f64() { return raw<double>(); }
    std::string str() {
        size_t n = u32();
        n = std::min(n, at <= s.size() ? s.size() - at : 0);
        std::string out(s.substr(std::min(at, s.size()), n));
        at += n;
        return out;
    }
};

struct Out {
    std::string s;
    template <typename T> void raw(T v) { s.append(reinterpret_cast<const char*>(&v), sizeof v); }
    void u8(uint8_t v) { raw(v); }
    void str(std::string_view v) {
        raw(uint32_t(v.size()));
        s.append(v);
    }
    void rect(const Rect& r) {
        raw(r.x);
        raw(r.y);
        raw(r.w);
        raw(r.h);
    }
};

uint64_t fnv64(std::string_view v, uint64_t h = 1469598103934665603ull) {
    for (unsigned char c : v) h = (h ^ c) * 1099511628211ull;
    return h;
}

struct StepClock : Clock {
    double ms = 0;
    double now_ms() override { return ms; }
};

// core_tests.cpp's StubBackend, with images that decode as their URL says: "fail" never, "async" later (the size comes with image_ready), any other at 24 x 16.
struct Stub : Backend {
    int next = 0, next_canvas_font = 1000;
    void metrics(float size, FontMetrics& out) {
        out.ascent = size * 0.8f;
        out.descent = size * 0.2f;
        out.height = size * 1.2f;
        out.x_height = size * 0.5f;
        out.zero_width = 8;
    }
    int create_font(const std::string&, float size, int, bool, const std::string&, FontMetrics& out) override {
        metrics(size, out);
        return ++next;
    }
    int create_canvas_font(const std::string&, float size, int, bool, FontMetrics& out) override {
        metrics(size, out);
        return ++next_canvas_font;
    }
    void delete_font(int) override {}
    float text_width(int, const std::string& text) override { return 8.0f * float(text.size()); }
    bool load_image(int, const std::string& url, float& w, float& h) override {
        if (url.find("fail") != std::string::npos) return false;
        if (url.find("async") != std::string::npos) {
            w = h = 0;
            return true;
        }
        w = 24;
        h = 16;
        return true;
    }
};

} // namespace eo

namespace twlite {

// Renderer's friend in the oracle's copy of renderer.h: reads what the input holds.
struct EventsOracle {
    static void sync(const Renderer& r) {
        twp::u8('S');
        twp::sync(r.stale_, r.layout_stopped_, r.frozen_, r.frozen_reason_, r.frozen_fatal_, r.pending_violations_, r.paint_stopped_);
    }

    static uint64_t tree(const Renderer& r, uint32_t max_id) {
        uint64_t h = 1469598103934665603ull;
        std::string b;
        for (uint32_t id = 1; id <= max_id; id++) {
            const Renderer::Node* n = r.node(id);
            if (!n) continue;
            b.clear();
            b.append(reinterpret_cast<const char*>(&id), sizeof id);
            b.push_back(char(n->kind));
            b += n->tag;
            b.push_back('\0');
            for (const auto& [name, value] : n->attrs) {
                b += name;
                b.push_back('=');
                b += value;
                b.push_back('\0');
            }
            b.push_back('\1');
            b += n->data;
            h = eo::fnv64(b, h);
        }
        return h;
    }

    static void state(Renderer& r, eo::Out& o, uint32_t max_id) {
        o.raw(uint32_t(r.touches_.size()));
        for (const auto& [pointer, t] : r.touches_) {
            o.raw(int32_t(pointer));
            o.raw(t.x0);
            o.raw(t.y0);
            o.raw(t.x);
            o.raw(t.y);
            o.raw(int64_t(t.target));
            o.rect(t.target_rect);
            o.u8(t.moved);
            o.u8(t.primary);
            o.raw(int32_t(t.pointer));
            o.u8(t.mouse);
            o.raw(int32_t(t.modifiers));
            o.raw(int32_t(t.button));
            o.u8(t.held);
            o.raw(int64_t(t.capture));
            o.raw(t.sent_x);
            o.raw(t.sent_y);
            o.u8(t.finger);
        }
        o.raw(int64_t(r.hover_));
        o.raw(r.hover_x_);
        o.raw(r.hover_y_);
        o.raw(int32_t(r.click_count_));
        o.raw(int32_t(r.arbiter_pointer_));
        auto deadline = r.hold_deadline();
        o.u8(deadline.has_value());
        o.raw(deadline.value_or(0.0));
        o.u8(r.bubble_);
        o.u8(r.bubble_touch_);
        o.u8(r.rtl_);
        o.raw(uint32_t(r.pending_moves_.size()));
        for (const auto& [key, ev] : r.pending_moves_) {
            o.str(key);
            o.str(ev.dump());
        }
        for (const auto* set : {&r.interest_, &r.prevent_}) {
            std::vector<std::string> sorted(set->begin(), set->end());
            std::sort(sorted.begin(), sorted.end());
            o.raw(uint32_t(sorted.size()));
            for (const std::string& k : sorted) o.str(k);
        }
        o.str(r.editor_json());
        o.str(r.editing_initial_);
        o.raw(uint64_t(r.editor_generation_));
        o.raw(int64_t(r.selection_.anchor.owner));
        o.raw(uint64_t(r.selection_.anchor.offset));
        o.raw(int64_t(r.selection_.focus.owner));
        o.raw(uint64_t(r.selection_.focus.offset));
        o.raw(int32_t(r.selection_.pointer));
        o.raw(r.last_input_);
        o.raw(uint32_t(r.resize_watch_.size()));
        for (const auto& [id, size] : r.resize_watch_) {
            o.raw(int64_t(id));
            o.raw(size.first);
            o.raw(size.second);
        }
        std::map<int, std::vector<int64_t>> waiting(r.waiting_images_.begin(), r.waiting_images_.end());
        o.raw(uint32_t(waiting.size()));
        for (const auto& [image, ids] : waiting) {
            o.raw(int32_t(image));
            o.raw(uint32_t(ids.size()));
            for (int64_t id : ids) o.raw(id);
        }
        const auto& key = r.runs_key_;
        o.raw(uint64_t(uintptr_t(std::get<0>(key))));
        o.raw(uint64_t(std::get<1>(key)));
        o.raw(uint64_t(std::get<2>(key)));
        o.raw(uint64_t(std::get<3>(key)));
        o.raw(uint32_t(r.runs_.size()));
        for (const auto& run : r.runs_) {
            o.u8(run.drawn);
            o.raw(int64_t(run.layer));
            for (float v : {run.shown.a, run.shown.b, run.shown.c, run.shown.d, run.shown.e, run.shown.f}) o.raw(v);
            o.raw(uint32_t(run.advances.size()));
        }
        o.raw(uint64_t(uintptr_t(r.selectable_key_.first)));
        o.raw(uint64_t(r.selectable_key_.second));
        o.u8(r.selectable_);
        o.raw(tree(r, max_id));
    }

    static uint64_t generation(const Renderer& r) { return r.editor() ? r.editor()->generation : 0; }
};

} // namespace twlite

namespace eo {

using twlite::EventsOracle;

void messages(const std::vector<Json>& out, Out& o) {
    std::vector<std::string> kept;
    for (const Json& m : out) {
        const std::string& type = m["type"].asString();
        if (type == "size" || type == "result") continue;
        kept.push_back(m.dump());
    }
    o.raw(uint32_t(kept.size()));
    for (const std::string& k : kept) o.str(k);
}

void script(In& in, Out& o) {
    const double width = in.f64(), dpr = in.f64();
    const bool dark = in.u8() != 0;
    const double t0 = in.f64();
    const uint32_t max_id = in.u32();
    Stub backend;
    StepClock clock;
    clock.ms = t0;
    twp::tape = twp::Tape{};
    twp::tape.on = true;
    auto r = std::make_unique<Renderer>(width, dpr, dark, backend, &clock);
    r->receive(Json::object().set("type", "policy").set("policy", "compat"));
    Out transcript;
    const uint32_t steps = in.u32();
    for (uint32_t i = 0; i < steps && in.at < in.s.size(); i++) {
        const uint8_t kind = in.u8();
        clock.ms = in.f64();
        EventsOracle::sync(*r);
        transcript.u8(kind);
        std::vector<Json> out;
        std::string extra;
        auto port = [&](auto f) {
            twp::Port guard;
            return f();
        };
        switch (kind) {
        case 'o':
        case 'm': {
            auto parsed = Json::parse(in.str());
            if (!parsed) break;
            Json msg = kind == 'o' ? Json::object().set("type", "ops").set("ops", std::move(*parsed)) : std::move(*parsed);
            out = r->receive(msg);
            break;
        }
        case 'd': r->set_theme(in.u8() != 0, out); break;
        case 'w': r->set_width(in.f64(), out); break;
        case 'M': r->set_max_height(in.f64(), out); break;
        case 'C': r->set_compositing(in.u8() != 0); break;
        case 't': {
            const int action = in.i32();
            const double x = in.f64(), y = in.f64();
            const int pointer = in.i32();
            const bool primary = in.u8() != 0, mouse = in.u8() != 0;
            const int modifiers = in.i32(), button = in.i32();
            out = port([&] { return r->touch(action, x, y, pointer, primary, mouse, modifiers, button); });
            break;
        }
        case 'h': {
            const int action = in.i32();
            const double x = in.f64(), y = in.f64();
            const int modifiers = in.i32();
            out = port([&] { return r->hover(action, x, y, modifiers); });
            break;
        }
        case 'c': {
            const double x = in.f64(), y = in.f64();
            auto claim = port([&] { return r->claim_at(x, y); });
            extra.push_back(char((claim.x ? 1 : 0) | (claim.y ? 2 : 0) | (claim.hold ? 4 : 0)));
            break;
        }
        case 'e': {
            const uint8_t mode = in.u8();
            const uint64_t literal = in.u64();
            const std::string text = in.str(), type = in.str();
            const uint64_t generation = mode == 0 ? EventsOracle::generation(*r) : literal;
            out = port([&] { return r->edit(generation, text, type); });
            break;
        }
        case 'E': out = port([&] { return r->end_editing(); }); break;
        case 'x': extra.push_back(char(port([&] { return r->clear_selection(); }) ? 1 : 0)); break;
        case 'i': {
            const int id = in.i32();
            const float w = in.f32(), h = in.f32();
            out = port([&] { return r->image_ready(id, w, h); });
            break;
        }
        case 'W': {
            const double x = in.f64(), y = in.f64(), dx = in.f64(), dy = in.f64();
            const int mode = in.i32(), modifiers = in.i32();
            out = port([&] { return r->wheel(x, y, dx, dy, mode, modifiers); });
            break;
        }
        case 'k': {
            const std::string type = in.str(), key = in.str(), code = in.str();
            const bool repeat = in.u8() != 0;
            const int modifiers = in.i32();
            out = port([&] { return r->key(type, key, code, repeat, modifiers); });
            break;
        }
        case 'f': out = r->tick(); break;
        case 'p': r->paint(); break;
        case 'l': r->last_html(); break;
        case 'q': {
            Out q;
            twp::Port guard;
            q.str(r->cursor());
            q.u8(r->has_selection());
            q.str(r->selected_text());
            q.str(r->selectable_text_json());
            extra = q.s;
            break;
        }
        default: std::exit(4);
        }
        messages(out, transcript);
        transcript.s += extra;
        EventsOracle::state(*r, transcript, max_id);
    }
    twp::tape.on = false;
    r.reset();
    o.raw(uint32_t(twp::tape.bytes.size()));
    o.s += twp::tape.bytes;
    o.s += transcript.s;
    twp::tape = twp::Tape{};
}

} // namespace eo

int main() {
    using namespace eo;
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
        if (op != 'S') return 3;
        In in{bytes};
        Out o;
        script(in, o);
        const uint32_t size = uint32_t(o.s.size());
        const unsigned char head[5] = {1, uint8_t(size), uint8_t(size >> 8), uint8_t(size >> 16), uint8_t(size >> 24)};
        std::fwrite(head, 1, 5, stdout);
        if (size) std::fwrite(o.s.data(), 1, size, stdout);
    }
    std::fflush(stdout);
    return 0;
}
