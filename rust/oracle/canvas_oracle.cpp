// core/src/canvas.cpp on stdin, for the tests of rust/crates/canvas to compare with: scripts of steps through one Canvas and a
// scripted CanvasHost, which writes down every call it gets where it gets it. Batches come out in their binary form (wire.cpp),
// so every float keeps its bits (every NaN as one) and shared clip lists stay shared.
//
// Each record is a u8 operation, a u32 length and its input; each answer a u8 status, a u32 length and bytes. Numbers are
// little-endian; a string is a u32 length and bytes.
//   'S' a script: i32 id, i32 width, i32 height, i64 allowance, u64 the other canvases' path floats, u8 flags (1: batches and
//       pixels as their u32 length and u64 FNV-1a instead of their bytes), u32 steps, each a u8 kind and its input:
//       'r' cmds JSON text -> 'R', u8 parsed, then the batch and the state;   'z' i32 w, i32 h -> 'Z', the batch and the state;
//       'c' method, args JSON text -> 'C', u8 parsed, the answer's dump;     't' trim -> 'T';
//       'a' i64 allowance, 'o' u64 path floats: the host's from then on;    'm' font, text -> 'M', the measureText answer;
//       'i' query JSON text, u8 read (0 no canvas, 1 pixels, 2 refused, 3 a byte short) -> 'I', u8 parsed, the dump, the pixels.
//   A batch is a u8 (1 encoded) and its bytes; the state width, height, i64 pixels and u64 path floats. The host writes 'F' font
//   css, i32 handle and seven f32 metrics; 'W' i32 font, text, f32 width; 'N' i64 node, u8 found, i32 image, i32 canvas, f32 w,
//   f32 h; 'A' i32 id, i64 allowance; 'P' i32 id, u64 floats; 'B' i32 x, y, w, h, u8 read. -> status 1 and all of it.
// measureText and imageData are the renderer's answers (renderer.cpp: Renderer::measure_text, the imageData query), copied
// here as they are, the renderer's font cache aside.
#include "canvas.h"
#include "wire.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <map>
#include <string>

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
    int32_t i32() { return raw<int32_t>(); }
    int64_t i64() { return raw<int64_t>(); }
    uint64_t u64() { return raw<uint64_t>(); }
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
    bool digest = false;
    template <typename T> void raw(T v) { s.append(reinterpret_cast<const char*>(&v), sizeof v); }
    void u8(uint8_t v) { raw(v); }
    void u32(uint32_t v) { raw(v); }
    void i32(int32_t v) { raw(v); }
    void i64(int64_t v) { raw(v); }
    void u64(uint64_t v) { raw(v); }
    void f32(float v) { raw(v); }
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
    void payload(std::string_view v) {
        if (!digest) return str(v);
        uint64_t h = 1469598103934665603ull;
        for (unsigned char c : v) h = (h ^ c) * 1099511628211ull;
        u32(uint32_t(v.size()));
        u64(h);
    }
};

uint32_t fnv32(std::string_view s) {
    uint32_t h = 2166136261u;
    for (unsigned char c : s) h = (h ^ c) * 16777619u;
    return h;
}

/// The scripted host, the same in the Rust test: fonts and widths from the bytes they are asked about, sources from the node
/// id, the allowance and the other canvases' floats as the script sets them.
struct Host : CanvasHost {
    Out* log = nullptr;
    int64_t allowance = 0;
    uint64_t others = 0;

    int canvas_font(const std::string& css, FontMetrics& m) override {
        const uint32_t h = fnv32(css);
        m.ascent = float(h % 17) * 0.75f + 1.0f;
        m.descent = float((h >> 5) % 7) * 0.5f;
        m.height = m.ascent + m.descent + 1.0f;
        m.x_height = float((h >> 9) % 5);
        m.zero_width = float((h >> 12) % 9) * 0.5f;
        if ((h >> 3) & 1) {
            m.em_ascent = m.ascent * 0.875f;
            m.em_descent = m.descent * 0.625f + 0.25f;
        }
        const int handle = css.empty() || h % 11 == 0 ? 0 : int(1 + (h >> 16) % 9);
        log->u8('F');
        log->str(css);
        log->i32(handle);
        for (float v : {m.ascent, m.descent, m.height, m.x_height, m.zero_width, m.em_ascent, m.em_descent}) log->f32(v);
        return handle;
    }
    float canvas_text_width(int font, const std::string& text) override {
        float w = 0;
        if (!text.empty() && text[0] == '!') w = std::numeric_limits<float>::quiet_NaN();
        else if (!text.empty() && text[0] == '#') w = std::numeric_limits<float>::infinity();
        else if (!text.empty() && text[0] == '~') w = -5.0f;
        else w = float(text.size()) * (float(font) * 0.5f + 3.0f) + float(text.empty() ? 0 : uint8_t(text[0]) % 5) * 0.25f;
        log->u8('W');
        log->i32(font);
        log->str(text);
        log->f32(w);
        return w;
    }
    bool canvas_source(int64_t node, int& image, int& canvas, float& w, float& h) override {
        const uint64_t u = uint64_t(node);
        bool found = true;
        switch (u % 8) {
        case 0: found = false; break;
        case 1: image = 1 + int(u % 997); w = float(u % 50) + 1; h = float(u % 30) + 2; break;
        case 2: canvas = 1 + int(u % 89); w = float(u % 64) + 1; h = float(u % 16) + 1; break;
        case 3: w = float(u % 100) + 1; h = float(u % 70) + 1; break;
        case 4: w = 0; h = 10; break;
        case 5: image = 7; w = std::numeric_limits<float>::quiet_NaN(); h = 5; break;
        case 6: w = 1e30f; h = 0.5f; break;
        default: w = 48; h = 48; break;
        }
        log->u8('N');
        log->i64(node);
        log->u8(found ? 1 : 0);
        log->i32(image);
        log->i32(canvas);
        log->f32(w);
        log->f32(h);
        return found;
    }
    int64_t canvas_allowance(int id) override {
        log->u8('A');
        log->i32(id);
        log->i64(allowance);
        return allowance;
    }
    size_t canvas_path_floats(int id) override {
        log->u8('P');
        log->i32(id);
        log->u64(others);
        return size_t(others);
    }
};

// renderer.cpp's Renderer::measure_text, its font cache aside.
Json measure_text(CanvasHost& host, const std::string& font, const std::string& text) {
    FontMetrics fm;
    int handle = host.canvas_font(font, fm);
    float width = handle ? host.canvas_text_width(handle, text) : 0.0f;
    Json o = Json::object();
    o.set("width", width);
    o.set("actualBoundingBoxLeft", 0).set("actualBoundingBoxRight", width);
    o.set("actualBoundingBoxAscent", fm.ascent * 0.8f).set("actualBoundingBoxDescent", fm.descent * 0.6f);
    o.set("fontBoundingBoxAscent", fm.ascent).set("fontBoundingBoxDescent", fm.descent);
    o.set("emHeightAscent", fm.em_ascent).set("emHeightDescent", fm.em_descent);
    o.set("hangingBaseline", fm.ascent * 0.8f).set("alphabeticBaseline", 0).set("ideographicBaseline", -fm.descent);
    return o;
}

// renderer.cpp's imageData query, the backend's canvas_read scripted by `mode`.
Json image_data(const Json& q, uint8_t mode, Out& log, std::string& binary, bool& has_binary) {
    int w = std::clamp(q["w"].asInt(), 0, Canvas::MAX_SIDE), h = std::clamp(q["h"].asInt(), 0, Canvas::MAX_SIDE);
    if (int64_t(w) * h > Canvas::MAX_PIXELS) return Json::object().set("error", "too large");
    std::vector<uint8_t> rgba;
    auto canvas_read = [&](int x, int y, int rw, int rh, std::vector<uint8_t>& out) {
        size_t n = size_t(rw) * rh * 4;
        if (mode == 3) n = n ? n - 1 : 1;
        if (mode != 2)
            for (size_t i = 0; i < n; i++) out.push_back(uint8_t(i * 7 + size_t(x) + size_t(y)));
        log.u8('B');
        log.i32(x);
        log.i32(y);
        log.i32(rw);
        log.i32(rh);
        log.u8(mode != 2 ? 1 : 0);
        return mode != 2;
    };
    if (mode == 0 || !canvas_read(q["x"].asInt(), q["y"].asInt(), w, h, rgba) || rgba.size() != size_t(w) * h * 4)
        rgba.assign(size_t(w) * h * 4, 0);
    binary = base64(rgba);
    has_binary = true;
    return Json::object().set("width", w).set("height", h);
}

void state(Out& o, const Canvas& c) {
    o.i32(c.width());
    o.i32(c.height());
    o.i64(c.pixels());
    o.u64(c.path_floats());
}

void canonical(float& v) {
    if (std::isnan(v)) v = std::numeric_limits<float>::quiet_NaN();
}

void canonical(std::vector<float>& list) {
    for (float& v : list) canonical(v);
}

/// `ops` with every NaN as 0x7fc00000, shared clip lists still shared: which NaN an operation gives is the compiler's choice
/// (operand order, and LLVM folds -x / 2 into x / -2, which keeps x's sign), not the source's.
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

void batch(Out& o, const std::vector<CanvasOp>& ops) {
    std::string bytes;
    if (!wire::encode(canonical(ops), wire::Caps{}, bytes)) return o.u8(0);
    o.u8(1);
    o.payload(bytes);
}

void script(std::string_view input) {
    In in{input};
    Out o;
    Host host;
    host.log = &o;
    const int id = in.i32(), width = in.i32(), height = in.i32();
    host.allowance = in.i64();
    host.others = in.u64();
    o.digest = (in.u8() & 1) != 0;
    Canvas canvas(id, width, height, host);
    state(o, canvas);
    const uint32_t steps = in.u32();
    for (uint32_t i = 0; i < steps && in.at < input.size(); i++) {
        const uint8_t kind = in.u8();
        switch (kind) {
        case 'r': {
            std::string text = in.str();
            auto cmds = Json::parse(text);
            o.u8('R');
            o.u8(cmds ? 1 : 0);
            if (!cmds) break;
            std::vector<CanvasOp> ops;
            canvas.run(*cmds, ops);
            batch(o, ops);
            state(o, canvas);
            break;
        }
        case 'z': {
            const int w = in.i32(), h = in.i32();
            std::vector<CanvasOp> ops;
            o.u8('Z');
            canvas.resize(w, h, ops);
            batch(o, ops);
            state(o, canvas);
            break;
        }
        case 'c': {
            std::string method = in.str(), text = in.str();
            auto args = Json::parse(text);
            o.u8('C');
            o.u8(args ? 1 : 0);
            if (args) o.str(canvas.call(method, *args).dump());
            break;
        }
        case 't':
            canvas.trim();
            o.u8('T');
            break;
        case 'a': host.allowance = in.i64(); break;
        case 'o': host.others = in.u64(); break;
        case 'm': {
            std::string font = in.str(), text = in.str();
            o.u8('M');
            o.str(measure_text(host, font, text).dump());
            break;
        }
        case 'i': {
            std::string text = in.str();
            const uint8_t mode = in.u8();
            auto q = Json::parse(text);
            o.u8('I');
            o.u8(q ? 1 : 0);
            if (!q) break;
            std::string binary;
            bool has_binary = false;
            o.str(image_data(*q, mode, o, binary, has_binary).dump());
            o.u8(has_binary ? 1 : 0);
            if (has_binary) o.payload(binary);
            break;
        }
        default: std::exit(4);
        }
    }
    const uint32_t n = uint32_t(o.s.size());
    const unsigned char head[5] = {1, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(o.s.data(), 1, n, stdout);
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
        script(bytes);
    }
    std::fflush(stdout);
    return 0;
}
