// core/src/compositor.cpp on stdin, for the tests of rust/crates/compositor to compare with. Display lists come in their
// binary form (wire.cpp), so every float keeps its bits.
//
// Each record is a u8 operation, a u32 length and its input; each answer a u8 status, a u32 length and bytes. Numbers are
// little-endian; a summary is a u32 count and per entry a u64 key, four f32 ink, an i32 canvas and a u8 backdrop; a key is
// eight f32 (t, a, b, c, d, e, f, opacity). A float the functions compute is written with any NaN as 0x7fc00000: when two NaNs
// meet in one operation, which payload the result keeps is the compiler's operand order, which IEEE leaves open.
//   'd' paint_digest: a list -> status 1 and the u64 digest, 0 when the list does not decode
//   's' summarize: a list -> status 1 and the summary
//   'i' drop_inert_clips: a list -> status 1, the u64 count and the list after in its binary form (status 2 when it does not encode)
//   'D' damage_between: u8 callback (0 none), u32 changed canvases and their i32 ids, two summaries -> status 1, u8 result,
//       u32 rectangles of four f32, u32 canvases asked and their i32 ids in order
//   'L' damage_between of two summarized lists: u8 callback, the changed canvases, u32 length and a list, twice -> as 'D'
//   'b' bake_track: four f32 box, u8 sample kind, eight f64 parameters, u32 the call that fails (0 none) -> status 1, u8 result,
//       u32 keys (copies of samples, so with their bits), the u32 count of samples and each t as f64
//   'r' resume_track: f64 start, f64 duration, u8 repeat, u32 keys, f64 elapsed -> status 1, u32 keys, f64 duration
#include "compositor.h"
#include "wire.h"

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <set>
#include <string>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite;

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
    uint64_t u64() { return raw<uint64_t>(); }
    float f32() { return raw<float>(); }
    double f64() { return raw<double>(); }
    std::string_view bytes() {
        size_t n = u32();
        n = std::min(n, at <= s.size() ? s.size() - at : 0);
        std::string_view out = s.substr(std::min(at, s.size()), n);
        at += n;
        return out;
    }
    LayerKey key() {
        LayerKey k;
        k.t = f32();
        k.matrix = Affine{f32(), f32(), f32(), f32(), f32(), f32()};
        k.opacity = f32();
        return k;
    }
    PaintSummary summary() {
        PaintSummary out;
        uint32_t n = u32();
        for (uint32_t i = 0; i < n && at < s.size(); i++) {
            PaintSummary::Entry e;
            e.key = u64();
            e.ink = Rect{f32(), f32(), f32(), f32()};
            e.canvas = i32();
            e.backdrop = u8() != 0;
            out.entries.push_back(e);
        }
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
    void f32(float v) { raw(v); }
    void f64(double v) { raw(v); }
    void computed(float v) {
        if (std::isnan(v)) u32(0x7fc00000u);
        else f32(v);
    }
    void rect(const Rect& r) {
        computed(r.x); computed(r.y); computed(r.w); computed(r.h);
    }
    void key(const LayerKey& k, bool exact) {
        for (float v : {k.t, k.matrix.a, k.matrix.b, k.matrix.c, k.matrix.d, k.matrix.e, k.matrix.f, k.opacity}) exact ? f32(v) : computed(v);
    }
    void keys(const std::vector<LayerKey>& list, bool exact) {
        u32(uint32_t(list.size()));
        for (const LayerKey& k : list) key(k, exact);
    }
    void summary(const PaintSummary& p) {
        u32(uint32_t(p.entries.size()));
        for (const auto& e : p.entries) {
            u64(e.key);
            rect(e.ink);
            i32(e.canvas);
            u8(e.backdrop ? 1 : 0);
        }
    }
};

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

bool list_of(std::string_view bytes, DisplayList& out) {
    return wire::decode(bytes, wire::Caps{}, out);
}

uint64_t fnv(uint64_t v) {
    uint64_t h = 1469598103934665603ull;
    for (int i = 0; i < 8; i++) h = (h ^ ((v >> (8 * i)) & 0xff)) * 1099511628211ull;
    return h;
}

/// The scripted layer the tests bake: a family of key functions of t, the same in the Rust test.
bool sample_of(uint8_t kind, const double* p, double t, LayerKey& key) {
    key.t = float(p[7]);
    switch (kind % 8) {
    case 0: {
        double angle = p[2] + p[3] * t, c = std::cos(angle), s = std::sin(angle);
        key.matrix = Affine{float(c), float(s), float(-s), float(c), float(p[0] - c * p[0] + s * p[1]), float(p[1] - s * p[0] - c * p[1])};
        break;
    }
    case 1:
        key.matrix = Affine::translate(float(p[0] + p[1] * t), float(p[2] * t * t));
        key.opacity = float(p[3] + p[4] * t);
        break;
    case 2: {
        double n = p[0] < 1 ? 1 : p[0], v = std::floor(t * n) / n;
        key.matrix = Affine{float(1 + p[1] * v), 0, 0, float(1 + p[1] * v), float(p[2] * v), 0};
        break;
    }
    case 3: {
        double u = t * t * (3 - 2 * t);
        key.matrix = Affine{float(1 + p[0] * u), 0, 0, float(1 + p[1] * u), 0, 0};
        key.opacity = float(1 - u * p[2]);
        break;
    }
    case 4:
        if (t >= p[0]) {
            key.matrix = Affine::translate(float(p[1]), float(p[2]));
            key.opacity = float(p[3]);
        }
        break;
    case 5:
        key.matrix = Affine::translate(t > p[0] ? std::numeric_limits<float>::quiet_NaN() : float(t * p[1]), 0);
        break;
    case 6: {
        uint64_t bits;
        std::memcpy(&bits, &t, 8);
        uint64_t h = fnv(bits);
        key.matrix = Affine::translate(float(h % 1000) / 10.0f, float((h >> 10) % 1000) / 10.0f);
        key.opacity = float((h >> 20) % 1000) / 1000.0f;
        break;
    }
    default:
        key.matrix = Affine::translate(float(p[0]), float(p[1]));
        break;
    }
    // Its NaNs as one: two NaNs met in the arithmetic above keep the payload the compiler's operand order picks.
    for (float* v : {&key.matrix.a, &key.matrix.b, &key.matrix.c, &key.matrix.d, &key.matrix.e, &key.matrix.f, &key.opacity})
        if (std::isnan(*v)) *v = std::numeric_limits<float>::quiet_NaN();
    return true;
}

void damage_answer(uint8_t mode, const std::set<int>& changed, const PaintSummary& a, const PaintSummary& b) {
    std::vector<int> asked;
    std::function<bool(int)> callback;
    if (mode) callback = [&](int canvas) {
        asked.push_back(canvas);
        return changed.count(canvas) != 0;
    };
    std::vector<Rect> rects{Rect{1, 2, 3, 4}};
    Out o;
    o.u8(damage_between(a, b, callback, rects) ? 1 : 0);
    o.u32(uint32_t(rects.size()));
    for (const Rect& r : rects) o.rect(r);
    o.u32(uint32_t(asked.size()));
    for (int c : asked) o.i32(c);
    answer(1, o.s);
}

void run(int op, std::string_view input) {
    In in{input};
    switch (op) {
    case 'd': {
        DisplayList list;
        if (!list_of(input, list)) return answer(0, {});
        Out o;
        o.u64(paint_digest(list));
        return answer(1, o.s);
    }
    case 's': {
        DisplayList list;
        if (!list_of(input, list)) return answer(0, {});
        Out o;
        o.summary(summarize(list));
        return answer(1, o.s);
    }
    case 'i': {
        DisplayList list;
        if (!list_of(input, list)) return answer(0, {});
        Out o;
        o.u64(drop_inert_clips(list));
        if (!wire::encode(list, wire::Caps{}, o.s)) return answer(2, {});
        return answer(1, o.s);
    }
    case 'D':
    case 'L': {
        uint8_t mode = in.u8();
        std::set<int> changed;
        for (uint32_t n = in.u32(), i = 0; i < n && in.at < input.size(); i++) changed.insert(in.i32());
        PaintSummary a, b;
        if (op == 'D') {
            a = in.summary();
            b = in.summary();
        } else {
            DisplayList la, lb;
            if (!list_of(in.bytes(), la) || !list_of(in.bytes(), lb)) return answer(0, {});
            a = summarize(la);
            b = summarize(lb);
        }
        return damage_answer(mode, changed, a, b);
    }
    case 'b': {
        Rect box{in.f32(), in.f32(), in.f32(), in.f32()};
        uint8_t kind = in.u8();
        double p[8];
        for (double& v : p) v = in.f64();
        uint32_t fail_at = in.u32();
        std::vector<double> asked;
        auto sample = [&](double t, LayerKey& key) {
            asked.push_back(t);
            if (fail_at && asked.size() == fail_at) return false;
            return sample_of(kind, p, t, key);
        };
        std::vector<LayerKey> keys{LayerKey{}};
        Out o;
        o.u8(bake_track(sample, box, keys) ? 1 : 0);
        o.keys(keys, true);
        o.u32(uint32_t(asked.size()));
        for (double t : asked) o.f64(t);
        return answer(1, o.s);
    }
    case 'r': {
        LayerTrack track;
        track.start = in.f64();
        track.duration = in.f64();
        track.repeat = in.u8() != 0;
        for (uint32_t n = in.u32(), i = 0; i < n && in.at < input.size(); i++) track.keys.push_back(in.key());
        double elapsed = in.f64();
        double span = -1;
        auto keys = resume_track(track, elapsed, span);
        Out o;
        o.keys(keys, false);
        o.f64(span);
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
