// The whole renderer (core/src/renderer.cpp on litehtml) driven through its messages for rust/crates/shadow to compare with: records and answers are laid out in that crate's tests/common/mod.rs.
#include "renderer.h"
#include "wire.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <map>
#include <memory>
#include <string>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite;
using twidget::Json;

namespace {

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
    uint32_t u32() { return raw<uint32_t>(); }
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
    void u32(uint32_t v) { raw(v); }
    void i32(int32_t v) { raw(v); }
    void u64(uint64_t v) { raw(v); }
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
};

uint64_t fnv64(std::string_view v) {
    uint64_t h = 1469598103934665603ull;
    for (unsigned char c : v) h = (h ^ c) * 1099511628211ull;
    return h;
}

void canonical(float& v) {
    if (std::isnan(v)) v = std::numeric_limits<float>::quiet_NaN();
}

void canonical(std::vector<float>& list) {
    for (float& v : list) canonical(v);
}

// As canvas_oracle.cpp writes a batch: every NaN as one, shared clip lists still shared.
std::vector<CanvasOp> canonical(std::vector<CanvasOp> ops) {
    std::map<const void*, std::shared_ptr<const std::vector<CanvasClip>>> clips;
    for (CanvasOp& op : ops) {
        canonical(op.path.points);
        for (float& v : op.paint.args) canonical(v);
        for (Stop& s : op.paint.stops) canonical(s.offset);
        for (float& v : op.matrix) canonical(v);
        canonical(op.dashes);
        for (float* v : {&op.line_width, &op.miter_limit, &op.dash_offset, &op.alpha, &op.shadow_blur, &op.shadow_x, &op.shadow_y, &op.x, &op.y,
                         &op.scale_x, &op.text_dx, &op.sx, &op.sy, &op.sw, &op.sh, &op.dx, &op.dy, &op.dw, &op.dh})
            canonical(*v);
        if (op.clips) {
            auto& made = clips[op.clips.get()];
            if (!made) {
                auto copy = std::make_shared<std::vector<CanvasClip>>(*op.clips);
                for (CanvasClip& c : *copy) canonical(c.path.points);
                made = copy;
            }
            op.clips = made;
        }
    }
    return ops;
}

struct FixedClock : Clock {
    double now_ms() override { return 0; }
};

// core_tests.cpp's StubBackend, its canvas fonts numbered on their own from 1001 and every canvas call written down.
struct Stub : Backend {
    int next = 0, next_canvas_font = 1000;
    Out* log = nullptr;
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
    bool load_image(int, const std::string&, float&, float&) override { return false; }
    void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override {
        if (!log) return;
        std::string bytes;
        const bool encoded = wire::encode(canonical(ops), wire::Caps{}, bytes);
        log->u8('R');
        log->i32(id);
        log->i32(w);
        log->i32(h);
        log->u32(uint32_t(ops.size()));
        log->u8(encoded ? 1 : 0);
        log->u64(fnv64(bytes));
    }
    void canvas_release(int id) override {
        if (!log) return;
        log->u8('X');
        log->i32(id);
    }
};

bool outside(const std::string& detail) {
    return detail.rfind("layout limit", 0) == 0 || detail.rfind("name limit", 0) == 0;
}

void script(In& in, Out& o) {
    const double width = in.f64(), dpr = in.f64();
    const bool dark = in.u8() != 0;
    const bool digest = (in.u8() & 1) != 0;
    Stub backend;
    FixedClock clock;
    Out calls;
    backend.log = &calls;
    auto r = std::make_unique<Renderer>(width, dpr, dark, backend, &clock);
    r->receive(Json::object().set("type", "policy").set("policy", "compat"));
    auto tail = [&](const std::vector<Json>& out) {
        std::vector<std::string> tree, stops;
        for (const Json& m : out) {
            if (m["type"].asString() != "violation") continue;
            const std::string& detail = m["detail"].asString();
            if (outside(detail)) stops.push_back(detail);
            else if (detail.rfind("image ", 0) != 0) tree.push_back(detail);
        }
        o.u32(uint32_t(tree.size()));
        for (const std::string& d : tree) o.str(d);
        const std::string& html = r->last_html();
        o.u32(uint32_t(calls.s.size()));
        o.s += calls.s;
        calls.s.clear();
        o.u32(uint32_t(stops.size()));
        for (const std::string& d : stops) o.str(d);
        // The document of a renderer stopped past a limit is its badge alone, which names the reason.
        static constexpr std::string_view STOPPED = "<!DOCTYPE html><html data-tw-frozen=\"fatal\"><head><style>", BADGE = "<div data-tw-badge=\"fatal\">";
        const size_t badge = html.rfind(BADGE);
        const bool stopped = html.rfind(STOPPED, 0) == 0 && badge != std::string::npos;
        o.u8(stopped ? 1 : 0);
        if (stopped) o.str(std::string_view(html).substr(badge + BADGE.size(), html.size() - badge - BADGE.size() - std::string_view("</div></body></html>").size()));
        if (digest) {
            o.u32(uint32_t(html.size()));
            o.u64(fnv64(html));
        } else {
            o.str(html);
        }
    };
    o.u8('c');
    tail({});
    const uint32_t steps = in.u32();
    for (uint32_t i = 0; i < steps && in.at < in.s.size(); i++) {
        const uint8_t kind = in.u8();
        o.u8(kind);
        switch (kind) {
        case 'o':
        case 'm': {
            auto parsed = Json::parse(in.str());
            o.u8(parsed ? 1 : 0);
            if (!parsed) break;
            Json msg = kind == 'o' ? Json::object().set("type", "ops").set("ops", std::move(*parsed)) : std::move(*parsed);
            tail(r->receive(msg));
            break;
        }
        case 'd': {
            std::vector<Json> out;
            r->set_theme(in.u8() != 0, out);
            tail(out);
            break;
        }
        default: std::exit(4);
        }
    }
    backend.log = nullptr;
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
