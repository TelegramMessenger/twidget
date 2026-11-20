// core/src/encode.cpp's JSON forms on stdin for rust/crates/encode: records of a u8 op, a u32 length and fields; answers of a u8 status, a u32 length and the JSON.
#include "canvas_ops.h"
#include "compositor.h"
#include "display_list.h"
#include "wire.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
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

    uint64_t le(int n) {
        if (s.size() - at < size_t(n)) {
            bad = true;
            return 0;
        }
        uint64_t v = 0;
        for (int i = n - 1; i >= 0; i--) v = (v << 8) | uint8_t(s[at + size_t(i)]);
        at += size_t(n);
        return v;
    }
    float f32() {
        const uint32_t bits = uint32_t(le(4));
        float f;
        std::memcpy(&f, &bits, 4);
        return f;
    }
    double f64() {
        const uint64_t bits = le(8);
        double d;
        std::memcpy(&d, &bits, 8);
        return d;
    }
    Rect rect() {
        Rect r;
        r.x = f32();
        r.y = f32();
        r.w = f32();
        r.h = f32();
        return r;
    }
};

// A composition: version, then its layers with every member to_json reads, in their order.
Composition composition(Reader& r) {
    Composition c;
    c.version = r.le(8);
    for (uint64_t n = r.le(4); n > 0 && !r.bad; n--) {
        Layer l;
        l.id = int64_t(r.le(8));
        l.bounds = r.rect();
        l.content_version = r.le(8);
        l.raster_scale = r.f32();
        l.has_clip = r.le(1) != 0;
        l.clip = r.rect();
        float* q[] = {&l.clip_radii.tlx, &l.clip_radii.tly, &l.clip_radii.trx, &l.clip_radii.try_, &l.clip_radii.brx, &l.clip_radii.bry, &l.clip_radii.blx, &l.clip_radii.bly};
        for (float* v : q) *v = r.f32();
        l.track_version = r.le(8);
        l.track.start = r.f64();
        l.track.duration = r.f64();
        l.track.repeat = r.le(1) != 0;
        for (uint64_t k = r.le(4); k > 0 && !r.bad; k--) {
            LayerKey key;
            key.t = r.f32();
            key.matrix.a = r.f32();
            key.matrix.b = r.f32();
            key.matrix.c = r.f32();
            key.matrix.d = r.f32();
            key.matrix.e = r.f32();
            key.matrix.f = r.f32();
            key.opacity = r.f32();
            l.track.keys.push_back(key);
        }
        l.parent = int64_t(r.le(8));
        c.layers.push_back(std::move(l));
    }
    return c;
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
        if (op == 'l') {
            // 'l' a display list in its wire form: status 1 and to_json of it, 0 when it does not decode.
            DisplayList list;
            if (!wire::decode(bytes, wire::Caps{}, list)) answer(0, {});
            else answer(1, to_json(list));
        } else if (op == 'b') {
            // 'b' a canvas batch in its wire form: status 1 and to_json of it, 0 when it does not decode.
            std::vector<CanvasOp> ops;
            if (!wire::decode(bytes, wire::Caps{}, ops)) answer(0, {});
            else answer(1, to_json(ops));
        } else if (op == 'c') {
            // 'c' a composition as composition() reads it: status 1 and to_json of it.
            Reader r{bytes};
            const Composition c = composition(r);
            if (r.bad || r.at != bytes.size()) return 4;
            answer(1, to_json(c));
        } else {
            return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
