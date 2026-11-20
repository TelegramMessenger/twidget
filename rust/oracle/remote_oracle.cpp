// twlite::remote on stdin: each message decoded by the kind its first byte names and encoded again by the C++, for the tests of
// rust/crates/remote to compare with the Rust codec.
//
// Records, little-endian: 'C' then fourteen u64 caps (the ten wire caps as wire_oracle takes them, then message, text, url, messages)
// sets the caps of the cases after it; 'm' then a u32 length and the message decodes it under those caps and encodes it again under
// them, 'M' decodes under the default caps; 'x' and 'X' do the same for a PixelsAnswer, five i32 of the request it answers (id, x, y,
// w, h) before the length; 'k' then a length and bytes answers kind_of. Each case answers a u8 (0 refused, 1 encoded, 2 the encode
// refused, 3 a PixelsAnswer without its request), a u32 length and the bytes.
#include "remote.h"

#include <cstdint>
#include <cstdio>
#include <string>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite;
using remote::Kind;

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

template <class T> void again(std::string_view message, const remote::Caps& decode_caps, const remote::Caps& encode_caps) {
    T value;
    if (!remote::decode(message, decode_caps, value)) return answer(0, {});
    std::string out;
    if (!remote::encode(value, encode_caps, out)) return answer(2, {});
    answer(1, out);
}

void run(std::string_view message, const remote::Caps& decode_caps, const remote::Caps& encode_caps) {
    if (message.empty()) return answer(0, {});
    switch (Kind(uint8_t(message[0]))) {
    case Kind::FrameRequest: return again<remote::FrameRequest>(message, decode_caps, encode_caps);
    case Kind::SetWidth: return again<remote::SetWidth>(message, decode_caps, encode_caps);
    case Kind::SetMaxHeight: return again<remote::SetMaxHeight>(message, decode_caps, encode_caps);
    case Kind::SetTheme: return again<remote::SetTheme>(message, decode_caps, encode_caps);
    case Kind::Touch: return again<remote::Touch>(message, decode_caps, encode_caps);
    case Kind::Hover: return again<remote::Hover>(message, decode_caps, encode_caps);
    case Kind::ClearSelection: return again<remote::ClearSelection>(message, decode_caps, encode_caps);
    case Kind::ClaimQuery: return again<remote::ClaimQuery>(message, decode_caps, encode_caps);
    case Kind::FontAnswer: return again<remote::FontAnswer>(message, decode_caps, encode_caps);
    case Kind::WidthAnswer: return again<remote::WidthAnswer>(message, decode_caps, encode_caps);
    case Kind::ImageAnswer: return again<remote::ImageAnswer>(message, decode_caps, encode_caps);
    case Kind::PixelsAnswer: return answer(3, {});
    case Kind::Frame: return again<remote::Frame>(message, decode_caps, encode_caps);
    case Kind::State: return again<remote::State>(message, decode_caps, encode_caps);
    case Kind::Messages: return again<remote::Messages>(message, decode_caps, encode_caps);
    case Kind::Pointer: return again<remote::Pointer>(message, decode_caps, encode_caps);
    case Kind::Selection: return again<remote::Selection>(message, decode_caps, encode_caps);
    case Kind::ClaimAnswer: return again<remote::ClaimAnswer>(message, decode_caps, encode_caps);
    case Kind::FontRelease: return again<remote::FontRelease>(message, decode_caps, encode_caps);
    case Kind::CanvasRun: return again<remote::CanvasRun>(message, decode_caps, encode_caps);
    case Kind::CanvasRelease: return again<remote::CanvasRelease>(message, decode_caps, encode_caps);
    case Kind::FontRequest: return again<remote::FontRequest>(message, decode_caps, encode_caps);
    case Kind::WidthRequest: return again<remote::WidthRequest>(message, decode_caps, encode_caps);
    case Kind::ImageRequest: return again<remote::ImageRequest>(message, decode_caps, encode_caps);
    case Kind::PixelsRequest: return again<remote::PixelsRequest>(message, decode_caps, encode_caps);
    }
    answer(0, {});
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    remote::Caps caps;
    const remote::Caps defaults;
    std::string bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        if (op == 'C') {
            uint64_t v[14];
            for (uint64_t& x : v)
                if (!read_u64(x)) return 2;
            size_t* const fields[] = {&caps.values.commands, &caps.values.canvas_ops, &caps.values.path_ops, &caps.values.floats, &caps.values.stops,
                &caps.values.filters, &caps.values.clips, &caps.values.text, &caps.values.pixels, &caps.values.rects, &caps.message, &caps.text,
                &caps.url, &caps.messages};
            for (size_t i = 0; i < 14; i++) *fields[i] = size_t(v[i]);
            continue;
        }
        remote::PixelsRequest asked;
        if (op == 'x' || op == 'X') {
            uint32_t f[5];
            for (uint32_t& x : f)
                if (!read_u32(x)) return 2;
            asked = remote::PixelsRequest{int32_t(f[0]), int32_t(f[1]), int32_t(f[2]), int32_t(f[3]), int32_t(f[4])};
        }
        uint32_t n = 0;
        if (!read_u32(n)) return 2;
        bytes.resize(n);
        if (!read_exact(bytes.data(), n)) return 2;
        if (op == 'm') {
            run(bytes, caps, caps);
        } else if (op == 'M') {
            run(bytes, defaults, caps);
        } else if (op == 'x' || op == 'X') {
            remote::PixelsAnswer value;
            if (!remote::decode(bytes, op == 'x' ? caps : defaults, asked, value)) {
                answer(0, {});
            } else {
                std::string out;
                if (!remote::encode(value, caps, out)) answer(2, {});
                else answer(1, out);
            }
        } else if (op == 'k') {
            const auto kind = remote::kind_of(bytes);
            if (kind) answer(1, std::string(1, char(uint8_t(*kind))));
            else answer(0, {});
        } else {
            return 3;
        }
    }
    std::fflush(stdout);
    return 0;
}
