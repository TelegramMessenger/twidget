// bench_oracle [--step|--trace] <mode> [count] [frames]: tests/bench.cpp unchanged on a step clock, traced call by call unless --step, as twidget_bench takes them (rust/PLAN.md, T9).

// What bench.cpp includes comes first, so the two macros below rename its own text only.
#include "compositor.h"
#include "renderer.h"
#include "wire.h"

#include <algorithm>
#include <chrono>
#include <cinttypes>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace bench_trace {

bool tracing = true;
bool original_css_setup = false;

struct Hash {
    uint64_t h = 14695981039346656037ull;
    void bytes(const void* data, size_t n) {
        const auto* p = static_cast<const unsigned char*>(data);
        for (size_t i = 0; i < n; i++) {
            h ^= p[i];
            h *= 1099511628211ull;
        }
    }
    void u8(uint8_t v) { bytes(&v, 1); }
    void u32(uint32_t v) {
        const unsigned char b[4] = {uint8_t(v), uint8_t(v >> 8), uint8_t(v >> 16), uint8_t(v >> 24)};
        bytes(b, 4);
    }
    void u64(uint64_t v) {
        u32(uint32_t(v));
        u32(uint32_t(v >> 32));
    }
    void i32(int32_t v) { u32(uint32_t(v)); }
    void f32(float v) {
        uint32_t b;
        std::memcpy(&b, &v, 4);
        u32(b);
    }
    void str(const std::string& s) {
        u64(s.size());
        bytes(s.data(), s.size());
    }
    void metrics(const twlite::FontMetrics& m) {
        for (float v : {m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent}) f32(v);
    }
};

std::string hex(uint64_t v) {
    char buf[17];
    std::snprintf(buf, sizeof buf, "%016" PRIx64, v);
    return buf;
}

std::string outs(const std::vector<twidget::Json>& out) {
    Hash h;
    for (const auto& m : out) h.str(m.dump());
    return " out=" + std::to_string(out.size()) + ":" + hex(h.h);
}

std::string original_css_outs(std::vector<twidget::Json> out) {
    const auto invalid = [] {
        std::fputs("Expected a size followed by animation starts on distinct targets.\n", stderr);
        std::exit(2);
    };
    if (out.empty() || out.front()["type"].asString() != "size") invalid();
    const auto id = [](const twidget::Json& message) { return message["ev"]["targetId"].asNumber(); };
    for (auto it = out.begin() + 1; it != out.end(); ++it) {
        const auto& event = (*it)["ev"];
        const double target = id(*it);
        if ((*it)["type"].asString() != "event" || event["type"].asString() != "animationstart"
            || !event["targetId"].isNumber() || !std::isfinite(target) || std::trunc(target) != target) invalid();
    }
    const auto first = out.begin() + 1;
    std::sort(first, out.end(), [&](const auto& a, const auto& b) { return id(a) < id(b); });
    if (std::adjacent_find(first, out.end(), [&](const auto& a, const auto& b) { return id(a) == id(b); }) != out.end()) invalid();
    return outs(out);
}

struct StepClock : twlite::Clock {
    double now = 1000;
    double now_ms() override { return now; }
};

/// The bench's backend behind one that writes down each call, its arguments and its answers.
struct Tap : twlite::Backend {
    twlite::Backend* inner = nullptr;
    Hash hash;
    uint64_t calls = 0;

    int create_font(const std::string& families, float size_px, int weight, bool italic, const std::string& features, twlite::FontMetrics& out) override {
        int font = inner->create_font(families, size_px, weight, italic, features, out);
        calls++;
        hash.u8('F');
        hash.str(families);
        hash.f32(size_px);
        hash.i32(weight);
        hash.u8(italic);
        hash.str(features);
        hash.i32(font);
        hash.metrics(out);
        return font;
    }
    int create_canvas_font(const std::string& families, float size_px, int weight, bool italic, twlite::FontMetrics& out) override {
        int font = inner->create_canvas_font(families, size_px, weight, italic, out);
        calls++;
        hash.u8('C');
        hash.str(families);
        hash.f32(size_px);
        hash.i32(weight);
        hash.u8(italic);
        hash.i32(font);
        hash.metrics(out);
        return font;
    }
    void delete_font(int font) override {
        inner->delete_font(font);
        calls++;
        hash.u8('D');
        hash.i32(font);
    }
    float text_width(int font, const std::string& text) override {
        float width = inner->text_width(font, text);
        calls++;
        hash.u8('W');
        hash.i32(font);
        hash.str(text);
        hash.f32(width);
        return width;
    }
    std::vector<float> text_widths(const std::vector<twlite::TextInFont>& texts) override {
        auto widths = inner->text_widths(texts);
        calls++;
        hash.u8('T');
        hash.u64(texts.size());
        for (const auto& t : texts) {
            hash.i32(t.font);
            hash.str(t.text);
        }
        hash.u64(widths.size());
        for (float w : widths) hash.f32(w);
        return widths;
    }
    bool load_image(int id, const std::string& url, float& w, float& h) override {
        bool ok = inner->load_image(id, url, w, h);
        calls++;
        hash.u8('I');
        hash.i32(id);
        hash.str(url);
        hash.u8(ok);
        hash.f32(w);
        hash.f32(h);
        return ok;
    }
    void canvas_run(int id, int w, int h, const std::vector<twlite::CanvasOp>& ops) override {
        inner->canvas_run(id, w, h, ops);
        std::string bytes;
        bool ok = twlite::wire::encode(ops, twlite::wire::Caps{}, bytes);
        calls++;
        hash.u8('R');
        hash.i32(id);
        hash.i32(w);
        hash.i32(h);
        hash.u8(ok);
        hash.str(bytes);
    }
    bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) override {
        bool ok = inner->canvas_read(id, x, y, w, h, rgba);
        calls++;
        hash.u8('Q');
        for (int v : {id, x, y, w, h}) hash.i32(v);
        hash.u8(ok);
        hash.u64(rgba.size());
        hash.bytes(rgba.data(), rgba.size());
        return ok;
    }
    void canvas_release(int id) override {
        inner->canvas_release(id);
        calls++;
        hash.u8('X');
        hash.i32(id);
    }
};

/// Made before the renderer, which takes the tap and the clock.
struct Parts {
    StepClock trace_clock_;
    Tap trace_tap_;
    explicit Parts(twlite::Backend& backend) { trace_tap_.inner = &backend; }
};

class TracingRenderer : private Parts, public twlite::Renderer {
public:
    TracingRenderer(double width, double dpr, bool dark, twlite::Backend& backend)
        : Parts(backend), Renderer(width, dpr, dark, tracing ? static_cast<twlite::Backend&>(trace_tap_) : backend, &trace_clock_) {
        line("open", "");
    }

    std::vector<twidget::Json> receive(const twidget::Json& msg) {
        auto out = Renderer::receive(msg);
        if (!tracing) return out;
        Hash in;
        in.str(msg.dump());
        line("receive", " in=" + hex(in.h) + (original_css_setup ? original_css_outs(out) : outs(out)));
        original_css_setup = false;
        return out;
    }
    std::vector<twidget::Json> hover(int action, double x, double y, int modifiers = 0) {
        auto out = Renderer::hover(action, x, y, modifiers);
        if (tracing) line("hover", outs(out));
        return out;
    }
    std::vector<twidget::Json> tick() {
        trace_clock_.now += 1000.0 / 60;
        auto out = Renderer::tick();
        if (tracing) line("tick", outs(out));
        return out;
    }
    twlite::DisplayList paint() {
        auto list = Renderer::paint();
        if (!tracing) return list;
        std::string bytes;
        bool ok = twlite::wire::encode(list, twlite::wire::Caps{}, bytes);
        Hash l;
        l.bytes(bytes.data(), bytes.size());
        std::vector<twlite::Rect> rects;
        bool damaged = damage(rects);
        Hash d;
        d.u8(damaged);
        d.u64(rects.size());
        for (const auto& r : rects) {
            d.f32(r.x);
            d.f32(r.y);
            d.f32(r.w);
            d.f32(r.h);
        }
        const auto& comp = composition();
        Hash c;
        c.str(twlite::to_json(comp));
        for (const auto& layer : comp.layers) {
            std::string content;
            bool encoded = twlite::wire::encode(layer.content, twlite::wire::Caps{}, content);
            c.u8(encoded);
            c.str(content);
        }
        line("paint", " n=" + std::to_string(list.size()) + " list=" + std::to_string(int(ok)) + ":" + hex(l.h) + " damage=" + hex(d.h) + " layers=" +
                          std::to_string(comp.version) + ":" + std::to_string(comp.layers.size()) + ":" + hex(c.h));
        return list;
    }
    void set_compositing(bool enabled) {
        Renderer::set_compositing(enabled);
        line("compositing", "");
    }

private:
    void line(const char* what, const std::string& rest) {
        if (!tracing) return;
        const auto& s = stats();
        std::string text = what + rest + " be=" + std::to_string(trace_tap_.calls) + ":" + hex(trace_tap_.hash.h);
        text += " L=" + std::to_string(layouts()) + " B=" + std::to_string(s.builds) + " P=" + std::to_string(s.patched) + " I=" + std::to_string(s.in_place);
        text += " S=" + std::to_string(s.scripted) + " R=" + std::to_string(s.reused) + " T=" + std::to_string(s.throttled) + " N=" + std::to_string(s.passes);
        text += " H=" + std::to_string(s.html_bytes) + " M=" + std::to_string(s.names) + " Y=" + std::to_string(s.name_bytes) + "\n";
        std::fputs(text.c_str(), stdout);
        trace_tap_.hash = Hash{};
        trace_tap_.calls = 0;
    }
};

} // namespace bench_trace

using bench_trace::TracingRenderer;

#define Renderer TracingRenderer
#define main bench_main
#include "bench.cpp"
#undef main
#undef Renderer

int main(int argc, char** argv) {
    if (argc > 1 && std::strcmp(argv[1], "--trace-original-css") == 0) {
        if (argc < 3 || std::strcmp(argv[2], "css") != 0) {
            std::fputs("--trace-original-css requires the css benchmark.\n", stderr);
            return 2;
        }
        bench_trace::original_css_setup = true;
    }
    if (argc > 1 && (bench_trace::original_css_setup || std::strcmp(argv[1], "--step") == 0 || std::strcmp(argv[1], "--trace") == 0)) {
        bench_trace::tracing = argv[1][2] == 't';
        argv[1] = argv[0];
        return bench_main(argc - 1, argv + 1);
    }
    return bench_main(argc, argv);
}
