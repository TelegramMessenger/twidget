// twidget::wire on stdin: each case is decoded and encoded again by the C++, for rust/crates/wire's tests to compare with the Rust codec.
//
// Records, little-endian: 'C' then ten u64 caps (commands, canvas_ops, path_ops, floats, stops, filters, clips, text, pixels, rects)
// sets the caps of the cases after it; 'l', 'd', 'b' or 'm' then a u32 length and the bytes decodes them under those caps and encodes
// the value again under them; 'L', 'D' or 'B' decodes under the default caps and encodes under the set ones.
// Each case answers a u8 (0 the decode refused, 1 encoded, 2 the encode refused), a u32 length and the encoded bytes.
#include "wire.h"

#include <cstdint>
#include <cstdio>
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

bool read_u32(uint32_t& v) {
    unsigned char b[4];
    if (!read_exact(b, 4)) return false;
    v = uint32_t(b[0]) | (uint32_t(b[1]) << 8) | (uint32_t(b[2]) << 16) | (uint32_t(b[3]) << 24);
    return true;
}

bool read_u64(uint64_t& v) {
    unsigned char b[8];
    if (!read_exact(b, 8)) return false;
    v = 0;
    for (int i = 7; i >= 0; i--) v = (v << 8) | b[i];
    return true;
}

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

template <class T> void run(std::string_view bytes, const wire::Caps& decode_caps, const wire::Caps& encode_caps) {
    T value;
    if (!wire::decode(bytes, decode_caps, value)) return answer(0, {});
    std::string out;
    if (!wire::encode(value, encode_caps, out)) return answer(2, {});
    answer(1, out);
}

// 's': a u64 `most` before the length; the list decoded under the set caps is encoded after "xy" with its starts within `most`, answering those bytes, a u32 count and each start as a u64.
void run_starts(std::string_view bytes, const wire::Caps& caps, uint64_t most) {
    DisplayList value;
    if (!wire::decode(bytes, caps, value)) return answer(0, {});
    std::string out = "xy";
    std::vector<size_t> starts{7, 8, 9};
    if (!wire::encode(value, caps, out, starts, size_t(most))) {
        out += std::to_string(starts.size());
        return answer(2, out);
    }
    const uint32_t n = uint32_t(starts.size());
    out.append(reinterpret_cast<const char*>(&n), 4);
    for (size_t s : starts) {
        const uint64_t v = s;
        out.append(reinterpret_cast<const char*>(&v), 8);
    }
    answer(1, out);
}

void run_metrics(std::string_view bytes) {
    FontMetrics value;
    if (!wire::decode(bytes, value)) return answer(0, {});
    std::string out;
    wire::encode(value, out);
    answer(1, out);
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    wire::Caps caps;
    const wire::Caps defaults;
    std::string bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        if (op == 'C') {
            uint64_t v[10];
            for (uint64_t& x : v)
                if (!read_u64(x)) return 2;
            caps.commands = size_t(v[0]);
            caps.canvas_ops = size_t(v[1]);
            caps.path_ops = size_t(v[2]);
            caps.floats = size_t(v[3]);
            caps.stops = size_t(v[4]);
            caps.filters = size_t(v[5]);
            caps.clips = size_t(v[6]);
            caps.text = size_t(v[7]);
            caps.pixels = size_t(v[8]);
            caps.rects = size_t(v[9]);
            continue;
        }
        uint64_t most = 0;
        if (op == 's' && !read_u64(most)) return 2;
        uint32_t n = 0;
        if (!read_u32(n)) return 2;
        bytes.resize(n);
        if (!read_exact(bytes.data(), n)) return 2;
        switch (op) {
        case 's': run_starts(bytes, caps, most); break;
        case 'l': run<DisplayList>(bytes, caps, caps); break;
        case 'd': run<wire::Damage>(bytes, caps, caps); break;
        case 'b': run<std::vector<CanvasOp>>(bytes, caps, caps); break;
        case 'm': run_metrics(bytes); break;
        case 'L': run<DisplayList>(bytes, defaults, caps); break;
        case 'D': run<wire::Damage>(bytes, defaults, caps); break;
        case 'B': run<std::vector<CanvasOp>>(bytes, defaults, caps); break;
        default: return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
