// core/src/png.cpp on stdin for rust/crates/png: records of a u8 op, a u32 length and the bytes; answers of a u8 status, a u32 length and bytes.
#include "png.h"

#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

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

void put(std::string& out, uint64_t v, int bytes) {
    for (int i = 0; i < bytes; i++) out.push_back(char(uint8_t(v >> (8 * i))));
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::vector<uint8_t> bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        unsigned char b[4];
        if (!read_exact(b, 4)) return 2;
        const uint32_t n = uint32_t(b[0]) | (uint32_t(b[1]) << 8) | (uint32_t(b[2]) << 16) | (uint32_t(b[3]) << 24);
        // One byte more than the data, so that an empty input still has a pointer to give.
        bytes.resize(size_t(n) + 1);
        if (!read_exact(bytes.data(), n)) return 2;
        std::string out;
        if (op == 'r') {
            // 'r' the file: status 1 and png_intact_rows as a u64 (SIZE_MAX for none).
            put(out, uint64_t(twlite::png_intact_rows(bytes.data(), n)), 8);
            answer(1, out);
        } else if (op == 'd') {
            // 'd' the file: image_dimensions' status, then w and h as u32 (what they held, 7 and 9, when it fails).
            uint32_t w = 7, h = 9;
            const bool ok = twlite::image_dimensions(bytes.data(), n, w, h);
            put(out, w, 4);
            put(out, h, 4);
            answer(ok ? 1 : 0, out);
        } else {
            return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
