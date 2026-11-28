// twlite::remote on stdin: each message decoded by the kind its first byte names and encoded again by the C++, for the tests of
// rust/crates/remote to compare with the Rust codec.
//
// Records, little-endian: 'C' then eighteen u64 caps (the ten wire caps as wire_oracle takes them, then message, text, url, messages, widths, widths_text, frame, delta_base)
// sets the caps of the cases after it; 'm' then a u32 length and the message decodes it under those caps and encodes it again under
// them, 'M' decodes under the default caps; 'x' and 'X' do the same for a PixelsAnswer, five i32 of the request it answers (id, x, y,
// w, h) before the length; 'k' then a length and bytes answers kind_of. Each case answers a u8 (0 refused, 1 encoded, 2 the encode
// refused, 3 a PixelsAnswer without its request), a u32 length and the bytes.
#include "remote.h"

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

void put_u32(std::string& out, uint32_t v) {
    for (int i = 0; i < 4; i++) out.push_back(char(uint8_t(v >> (8 * i))));
}

void put_u64(std::string& out, uint64_t v) {
    for (int i = 0; i < 8; i++) out.push_back(char(uint8_t(v >> (8 * i))));
}

void put_f64(std::string& out, double v) {
    uint64_t b = 0;
    std::memcpy(&b, &v, 8);
    put_u64(out, b);
}

// The texts of a WidthsRequest, as 'y', 'Y' and 'w' give them: a u32 count, each an i32 font and a u32 length and bytes; 'y' and 'Y' then decode a WidthsAnswer to them as 'x' and 'X' do, 'w' a u32 from and no message, answering widths_in_request as a u64.
bool read_texts(std::vector<TextInFont>& texts) {
    uint32_t n = 0;
    if (!read_u32(n)) return false;
    for (uint32_t i = 0; i < n; i++) {
        uint32_t font = 0, length = 0;
        if (!read_u32(font) || !read_u32(length)) return false;
        std::string text(length, '\0');
        if (!read_exact(text.data(), length)) return false;
        texts.push_back({int32_t(font), std::move(text)});
    }
    return true;
}

// 'p': a u64 `most` before the length; the Frame decoded and encoded again after "xy" with its list's place, answering those bytes, u64 at, u64 size, u32 starts and each u64.
void placed(std::string_view message, const remote::Caps& caps, uint64_t most) {
    remote::Frame frame;
    if (!remote::decode(message, caps, frame)) return answer(0, {});
    std::string out = "xy";
    remote::WrittenList list{1, 2, {3}};
    if (!remote::encode(frame, caps, out, size_t(most), list)) {
        out += std::to_string(list.at) + "," + std::to_string(list.size) + "," + std::to_string(list.starts.size());
        return answer(2, out);
    }
    put_u64(out, list.at);
    put_u64(out, list.size);
    put_u32(out, uint32_t(list.starts.size()));
    for (size_t s : list.starts) put_u64(out, s);
    answer(1, out);
}

// 'v': a Frame decoded with its list's view, answering the view's u64 offset in the message and u64 length.
void viewed(std::string_view message, const remote::Caps& caps) {
    remote::Frame frame;
    std::string_view list = "kept";
    if (!remote::decode(message, caps, frame, list)) return answer(0, list == "kept" ? std::string() : std::string("moved"));
    std::string out;
    put_u64(out, list.empty() ? 0 : uint64_t(list.data() - message.data()));
    put_u64(out, list.size());
    answer(1, out);
}

// 'i': a message's delta_ids, answering u64 seq and u64 base.
void ids(std::string_view message) {
    uint64_t seq = 7, base = 7;
    if (!remote::delta_ids(message, seq, base)) return answer(0, seq == 7 && base == 7 ? std::string() : std::string("moved"));
    std::string out;
    put_u64(out, seq);
    put_u64(out, base);
    answer(1, out);
}

// 's': a message's delta_scan under the caps, answering u64 seq, u32 length and the damage view's u64 offset in the message and u64 length; a refusal answers "moved" when it changed an output.
void scanned(std::string_view message, const remote::Caps& caps) {
    const std::string_view kept = "kept";
    uint64_t seq = 7;
    uint32_t length = UINT32_MAX;
    std::string_view damage = kept;
    if (!remote::delta_scan(message, caps, seq, length, damage)) {
        const bool alone = seq == 7 && length == UINT32_MAX && damage.data() == kept.data() && damage.size() == kept.size();
        return answer(0, alone ? std::string() : std::string("moved"));
    }
    std::string out;
    put_u64(out, seq);
    put_u32(out, length);
    put_u64(out, uint64_t(damage.data() - message.data()));
    put_u64(out, damage.size());
    answer(1, out);
}

// 'r': a u64 base seq and a u32 length and the base before the length; the FrameDelta rebuilt on it, answering u64 seq, the state's fields, the painted flag, the damage's wire bytes after a u32 length and the list's bytes.
void rebuilt(std::string_view message, const remote::Caps& caps, uint64_t base_seq, std::string_view base) {
    remote::Frame frame;
    std::string list = "kept";
    if (!remote::rebuild(message, caps, base_seq, base, frame, list)) return answer(0, list == "kept" ? std::string() : std::string("moved"));
    std::string out;
    put_u64(out, frame.seq);
    put_f64(out, frame.state.width);
    put_u32(out, uint32_t(frame.state.height));
    out.push_back(char(frame.state.dirty));
    out.push_back(char(frame.state.wants_tick));
    put_f64(out, frame.state.next_tick_in);
    out.push_back(char(frame.state.frame_asked));
    out.push_back(char(frame.painted));
    std::string damage;
    wire::encode(frame.damage, wire::Caps{}, damage);
    put_u32(out, uint32_t(damage.size()));
    out += damage;
    out += list;
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
    case Kind::WidthsAnswer: return answer(3, {});
    case Kind::FrameDelta: return again<remote::FrameDelta>(message, decode_caps, encode_caps);
    case Kind::WidthsRequest: return again<remote::WidthsRequest>(message, decode_caps, encode_caps);
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
            uint64_t v[18];
            for (uint64_t& x : v)
                if (!read_u64(x)) return 2;
            size_t* const fields[] = {&caps.values.commands, &caps.values.canvas_ops, &caps.values.path_ops, &caps.values.floats, &caps.values.stops,
                &caps.values.filters, &caps.values.clips, &caps.values.text, &caps.values.pixels, &caps.values.rects, &caps.message, &caps.text,
                &caps.url, &caps.messages, &caps.widths, &caps.widths_text, &caps.frame, &caps.delta_base};
            for (size_t i = 0; i < 18; i++) *fields[i] = size_t(v[i]);
            continue;
        }
        remote::PixelsRequest asked;
        if (op == 'x' || op == 'X') {
            uint32_t f[5];
            for (uint32_t& x : f)
                if (!read_u32(x)) return 2;
            asked = remote::PixelsRequest{int32_t(f[0]), int32_t(f[1]), int32_t(f[2]), int32_t(f[3]), int32_t(f[4])};
        }
        remote::WidthsRequest widths_asked;
        uint32_t from = 0;
        if ((op == 'y' || op == 'Y' || op == 'w') && !read_texts(widths_asked.texts)) return 2;
        if (op == 'w' && !read_u32(from)) return 2;
        uint64_t most = 0, base_seq = 0;
        std::string base;
        if (op == 'p' && !read_u64(most)) return 2;
        if (op == 'r') {
            uint32_t length = 0;
            if (!read_u64(base_seq) || !read_u32(length)) return 2;
            base.resize(length);
            if (!read_exact(base.data(), length)) return 2;
        }
        if (op == 'w') {
            std::string out;
            put_u64(out, remote::widths_in_request(widths_asked.texts, from, caps));
            answer(1, out);
            continue;
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
        } else if (op == 'y' || op == 'Y') {
            remote::WidthsAnswer value;
            if (!remote::decode(bytes, op == 'y' ? caps : defaults, widths_asked, value)) {
                answer(0, {});
            } else {
                std::string out;
                if (!remote::encode(value, caps, out)) answer(2, {});
                else answer(1, out);
            }
        } else if (op == 'p') {
            placed(bytes, caps, most);
        } else if (op == 'v') {
            viewed(bytes, caps);
        } else if (op == 'i') {
            ids(bytes);
        } else if (op == 's') {
            scanned(bytes, caps);
        } else if (op == 'r') {
            rebuilt(bytes, caps, base_seq, base);
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
