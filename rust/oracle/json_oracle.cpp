// widget-sandbox's twidget::Json on stdin, for rust/crates/json's tests to compare with the Rust port.
//
// Records, little-endian: 'p' then a u8 mode (0 Surrogates::Replace, 1 Keep), an i32 maxDepth, a u32 length and the text parses it
// and answers its dump; 'n', 'g' or 'i' then a double's u64 bits answers Json::number, the Apple fallback of Json::number (the
// shortest of 15, 16 and 17 significant digits; copied below, since json.h picks its branch by platform) or asInt as an i32;
// 'q' then a u32 length and bytes answers Json::quote. Each answer is a u8 (0 refused, 1 taken), a u32 length and the bytes.
#include "twidget/json.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <sstream>
#include <string>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using twidget::Json;

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

bool read_double(double& d) {
    unsigned char b[8];
    if (!read_exact(b, 8)) return false;
    uint64_t v = 0;
    for (int i = 7; i >= 0; i--) v = (v << 8) | b[i];
    std::memcpy(&d, &v, sizeof d);
    return true;
}

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

// json.h's branch for Apple deployment targets without floating std::to_chars, as it is there.
std::string fallback_number(double d) {
    if (!std::isfinite(d)) return "null";
    if (d == std::floor(d) && std::fabs(d) < 1e15) {
        char buf[32];
        auto r = std::to_chars(buf, buf + sizeof buf, (long long)d);
        return std::string(buf, r.ptr);
    }
    std::string out;
    for (int precision = 15; precision <= 17; precision++) {
        std::ostringstream stream;
        stream.imbue(std::locale::classic());
        stream.precision(precision);
        stream << d;
        out = stream.str();
        if (Json::parseDouble(out.c_str()) == d) break;
    }
    return out;
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::string bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        if (op == 'p') {
            const int mode = std::fgetc(stdin);
            uint32_t depth = 0, n = 0;
            if (mode == EOF || !read_u32(depth) || !read_u32(n)) return 2;
            bytes.resize(n);
            if (!read_exact(bytes.data(), n)) return 2;
            const auto v = Json::parse(bytes, mode ? Json::Surrogates::Keep : Json::Surrogates::Replace, int32_t(depth));
            if (v) answer(1, v->dump());
            else answer(0, {});
        } else if (op == 'n' || op == 'g' || op == 'i') {
            double d = 0;
            if (!read_double(d)) return 2;
            if (op == 'n') answer(1, Json::number(d));
            else if (op == 'g') answer(1, fallback_number(d));
            else {
                const int32_t i = Json(d).asInt(-7);
                const unsigned char b[4] = {uint8_t(i), uint8_t(i >> 8), uint8_t(i >> 16), uint8_t(uint32_t(i) >> 24)};
                answer(1, std::string(reinterpret_cast<const char*>(b), 4));
            }
        } else if (op == 'q') {
            uint32_t n = 0;
            if (!read_u32(n)) return 2;
            bytes.resize(n);
            if (!read_exact(bytes.data(), n)) return 2;
            answer(1, Json::quote(bytes));
        } else {
            return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
