// core/src/paint_policy on stdin, for the tests of rust/crates/paint to compare with.
//
// Each record is a u8 operation, a u32 length and its input; each answer a u8 status, a u32 length and bytes. Strings are a u32
// length and bytes; numbers little-endian.
//   'b' box_shadow_blurred, 'h' hides_subtree: the text -> status
//   'w' box_shadow_without_blur: the text -> the text
//   'p' box_paint: u8 pseudo, u8 button (0 none, 1 disabled, 2 pressed), six strings (display, content, box-shadow, filter,
//       backdrop-filter, opacity) -> status 0 for no box, else 1 and heavy, backdrop
//   'a' AnimationPaint: u32 keyframes, each u8 has an offset, f64 offset, u32 properties, each a name, u8 has a value, a value
//       -> status 0 for nothing, else 1 and heavy, backdrop
//   'c' a PaintCounter<std::string> script: u32 steps, each u8 kind and a key string, then for 0 (set_box) u8 has paint, heavy,
//       backdrop; 1 remove_box; 2 (start_animation) an id string, heavy, backdrop; 3 (end_animation) an id string; 4 check;
//       5 begin_scan; 6 end_scan -> status 1 and, for each check, i32 heavy, backdrop, peak heavy, peak backdrop, u8 stopped,
//       u8 code, i32 count; for each end_scan a u8; for each has_box and animating after a step, nothing
#include "paint_policy.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite::paint;

namespace {

bool read_exact(void* to, size_t n) {
    return n == 0 || std::fread(to, 1, n, stdin) == n;
}

struct In {
    std::string_view s;
    size_t at = 0;

    uint8_t u8() { return at < s.size() ? uint8_t(s[at++]) : 0; }
    uint32_t u32() {
        uint32_t v = 0;
        for (int i = 0; i < 4 && at < s.size(); i++) v |= uint32_t(uint8_t(s[at++])) << (8 * i);
        return v;
    }
    double f64() {
        uint64_t v = 0;
        for (int i = 0; i < 8 && at < s.size(); i++) v |= uint64_t(uint8_t(s[at++])) << (8 * i);
        double d;
        std::memcpy(&d, &v, 8);
        return d;
    }
    std::string str() {
        size_t n = std::min<size_t>(u32(), s.size() - at);
        std::string out(s.substr(at, n));
        at += n;
        return out;
    }
};

struct Out {
    std::string s;
    void u8(uint8_t v) { s.push_back(char(v)); }
    void i32(int32_t v) {
        for (int i = 0; i < 4; i++) s.push_back(char(uint8_t(uint32_t(v) >> (8 * i))));
    }
};

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

void paint_answer(const std::optional<BoxPaint>& p) {
    if (!p) return answer(0, {});
    Out o;
    o.u8(p->heavy ? 1 : 0);
    o.u8(p->backdrop ? 1 : 0);
    answer(1, o.s);
}

void run(int op, std::string_view input) {
    In in{input};
    switch (op) {
    case 'b': return answer(box_shadow_blurred(input) ? 1 : 0, {});
    case 'h': return answer(hides_subtree(input) ? 1 : 0, {});
    case 'w': return answer(1, box_shadow_without_blur(input));
    case 'p': {
        const bool pseudo = in.u8() != 0;
        const uint8_t button = in.u8();
        std::string f[6];
        for (auto& s : f) s = in.str();
        Style style{f[0], f[1], f[2], f[3], f[4], f[5], button == 1 ? Style::Button::Disabled : button == 2 ? Style::Button::Pressed : Style::Button::None};
        return paint_answer(box_paint(style, pseudo));
    }
    case 'a': {
        AnimationPaint paint;
        const uint32_t frames = in.u32();
        for (uint32_t k = 0; k < frames; k++) {
            const bool has = in.u8() != 0;
            const double offset = in.f64();
            paint.keyframe(has ? std::optional<double>(offset) : std::nullopt);
            const uint32_t props = in.u32();
            for (uint32_t p = 0; p < props; p++) {
                std::string name = in.str();
                const bool valued = in.u8() != 0;
                std::string value = in.str();
                paint.property(name, valued ? std::optional<std::string_view>(value) : std::nullopt);
            }
        }
        return paint_answer(paint.result());
    }
    case 'c': {
        PaintCounter<std::string> counter;
        Out o;
        const uint32_t steps = in.u32();
        for (uint32_t i = 0; i < steps; i++) {
            const uint8_t kind = in.u8();
            const std::string key = in.str();
            switch (kind) {
            case 0: {
                const bool has = in.u8() != 0;
                BoxPaint p{in.u8() != 0, in.u8() != 0};
                counter.set_box(key, has ? std::optional<BoxPaint>(p) : std::nullopt);
                break;
            }
            case 1: counter.remove_box(key); break;
            case 2: {
                const std::string id = in.str();
                BoxPaint p{in.u8() != 0, in.u8() != 0};
                counter.start_animation(key, id, p);
                break;
            }
            case 3: counter.end_animation(key, in.str()); break;
            case 4: {
                auto v = counter.check();
                o.i32(counter.heavy());
                o.i32(counter.backdrop());
                o.i32(counter.peak_heavy());
                o.i32(counter.peak_backdrop());
                o.u8(v ? 1 : 0);
                o.u8(v ? uint8_t(v->code) : 0);
                o.i32(v ? v->count : 0);
                o.u8(counter.has_box(key) ? 1 : 0);
                break;
            }
            case 5: counter.begin_scan(); break;
            case 6: o.u8(counter.end_scan() ? 1 : 0); break;
            }
        }
        return answer(1, o.s);
    }
    }
    std::exit(3);
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
        run(op, bytes);
    }
    std::fflush(stdout);
    return 0;
}
