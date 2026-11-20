// core/src/filters.cpp on stdin for rust/crates/filters: records of a u8 op, a u32 length and fields; answers of a u8 status, a u32 length and bytes.
#include "filters.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <string>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite;

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

struct Reader {
    const std::string& s;
    size_t at = 0;
    bool bad = false;

    const unsigned char* take(size_t n) {
        if (s.size() - at < n) {
            bad = true;
            static const unsigned char zeros[16] = {};
            return zeros;
        }
        const unsigned char* p = reinterpret_cast<const unsigned char*>(s.data()) + at;
        at += n;
        return p;
    }
    uint8_t u8() { return take(1)[0]; }
    uint32_t u32() {
        const unsigned char* b = take(4);
        return uint32_t(b[0]) | (uint32_t(b[1]) << 8) | (uint32_t(b[2]) << 16) | (uint32_t(b[3]) << 24);
    }
    int32_t i32() { return int32_t(u32()); }
    float f32() {
        const uint32_t bits = u32();
        float f;
        std::memcpy(&f, &bits, 4);
        return f;
    }
    // A filter: kind (u8), amount, x and y (f32 bits), colour r, g, b, a.
    Filter filter() {
        Filter f;
        f.kind = Filter::Kind(u8());
        f.amount = f32();
        f.x = f32();
        f.y = f32();
        f.color.r = u8();
        f.color.g = u8();
        f.color.b = u8();
        f.color.a = u8();
        return f;
    }
    // A list: a u32 count, then its filters.
    Filters list() {
        Filters out;
        for (uint32_t n = u32(); n > 0 && !bad; n--) out.push_back(filter());
        return out;
    }
};

// Every NaN as 0x7fc00000: which NaN an operation gives is the compiler's choice.
void put_f32(std::string& out, float v) {
    if (v != v) v = std::numeric_limits<float>::quiet_NaN();
    uint32_t bits;
    std::memcpy(&bits, &v, 4);
    for (int i = 0; i < 4; i++) out.push_back(char(uint8_t(bits >> (8 * i))));
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
        Reader r{bytes};
        std::string out;
        // 'm' a filter: color_matrix's status, then rgb, offset and alpha as f32 bits.
        if (op == 'm') {
            const Filter f = r.filter();
            if (r.bad) return 4;
            ColorMatrix m;
            m.offset = 7;
            const bool ok = color_matrix(f, m);
            for (float v : m.rgb) put_f32(out, v);
            put_f32(out, m.offset);
            put_f32(out, m.alpha);
            answer(ok ? 1 : 0, out);
        } else if (op == 'r') {
            // 'r' a list: filter_reach's left, top, right and bottom as f32 bits.
            const Filters list = r.list();
            if (r.bad) return 4;
            const FilterReach reach = filter_reach(list);
            put_f32(out, reach.left);
            put_f32(out, reach.top);
            put_f32(out, reach.right);
            put_f32(out, reach.bottom);
            answer(1, out);
        } else if (op == 'p') {
            // 'p' w, h, stride, bgra, scale, a list and the pixels: filter_pixels' bytes after.
            const int32_t w = r.i32(), h = r.i32();
            const uint32_t stride = r.u32();
            const bool bgra = r.u8() != 0;
            const float scale = r.f32();
            const Filters list = r.list();
            if (r.bad) return 4;
            std::vector<uint8_t> pixels(bytes.begin() + long(r.at), bytes.end());
            filter_pixels(pixels.data(), w, h, stride, bgra, list, scale);
            answer(1, std::string(pixels.begin(), pixels.end()));
        } else {
            return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
