// core/src/css.cpp and the C library's strtod on stdin, for the tests of rust/crates/css and rust/crates/cstd to compare with.
//
// Each record is a u8 operation, a u32 length and that many bytes of its input; each answer a u8 status, a u32 length and bytes.
// Operations (the input's layout before the text, then what the answer holds):
//   's' strtod                         -> f64 bits, u32 bytes taken
//   'N' number                         -> status 1 and f64 bits, or 0
//   't' trim, 'l' lower, 'Z' normalize_colors, 'W' unwrap_where -> the text
//   'i' iequals: u32 length of a, a, b -> status
//   'S' split_top_level: separator     -> u32 count, each part as u32 length and bytes
//   'D' parse_declarations             -> u32 count, each name and value as u32 length and bytes, then a u8 important
//   'n' is_tag_name, 'a' is_attribute_name, 'p' is_property_name, 'v' is_theme_variable -> status
//   'L' resolve_light_dark: u8 dark    -> the text
//   'O' skip_opaque: u32 at            -> u32
//   'T' parse_time                     -> status, f64 bits
//   'C' parse_color                    -> status, r g b a
//   'F' format_number: f64 bits, i32 digits (no text) -> the text
//   'f' parse_font                     -> u32 length and family, f32 size bits, i32 weight, u8 italic
//   'X' a transform: u8 which (0 transform, 1 translate, 2 rotate, 3 scale, 4 origin, 5 opacity), f32 w, f32 h -> status, the f32 bits
//   'H' theme_variables, 'A' content_safe_area_variables: JSON text -> status 2 when it does not parse, else 1 and the pairs
//   'V' substitute_variables: u32 count, each name and value as u32 length and bytes -> the text
#include "css.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
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

    uint32_t u32() {
        uint32_t v = 0;
        for (int i = 0; i < 4 && at < s.size(); i++) v |= uint32_t(uint8_t(s[at++])) << (8 * i);
        return v;
    }
    uint8_t u8() { return at < s.size() ? uint8_t(s[at++]) : 0; }
    double f64() {
        uint64_t v = 0;
        for (int i = 0; i < 8 && at < s.size(); i++) v |= uint64_t(uint8_t(s[at++])) << (8 * i);
        double d;
        std::memcpy(&d, &v, 8);
        return d;
    }
    float f32() {
        uint32_t v = u32();
        float f;
        std::memcpy(&f, &v, 4);
        return f;
    }
    std::string_view bytes(size_t n) {
        n = std::min(n, s.size() - at);
        auto out = s.substr(at, n);
        at += n;
        return out;
    }
    std::string_view rest() { return bytes(s.size() - at); }
};

struct Out {
    std::string s;

    void u8(uint8_t v) { s.push_back(char(v)); }
    void u32(uint32_t v) {
        for (int i = 0; i < 4; i++) s.push_back(char(uint8_t(v >> (8 * i))));
    }
    void f64(double d) {
        uint64_t v;
        std::memcpy(&v, &d, 8);
        for (int i = 0; i < 8; i++) s.push_back(char(uint8_t(v >> (8 * i))));
    }
    void f32(float f) {
        uint32_t v;
        std::memcpy(&v, &f, 4);
        u32(v);
    }
    void str(std::string_view t) {
        u32(uint32_t(t.size()));
        s.append(t);
    }
};

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

void pairs(Out& o, const std::vector<std::pair<std::string, std::string>>& vars) {
    o.u32(uint32_t(vars.size()));
    for (const auto& [k, v] : vars) {
        o.str(k);
        o.str(v);
    }
}

void affine(Out& o, const Affine& m) {
    for (float f : {m.a, m.b, m.c, m.d, m.e, m.f}) o.f32(f);
}

void run(int op, std::string_view input) {
    In in{input};
    Out o;
    switch (op) {
    case 's': {
        const std::string text(input);
        char* end = nullptr;
        const double v = std::strtod(text.c_str(), &end);
        o.f64(v);
        o.u32(uint32_t(end - text.c_str()));
        return answer(1, o.s);
    }
    case 'N': {
        auto v = css::number(input);
        if (!v) return answer(0, {});
        o.f64(*v);
        return answer(1, o.s);
    }
    case 't': return answer(1, css::trim(input));
    case 'l': return answer(1, css::lower(std::string(input)));
    case 'Z': return answer(1, css::normalize_colors(input));
    case 'W': return answer(1, css::unwrap_where(input));
    case 'i': {
        const uint32_t n = in.u32();
        auto a = in.bytes(n);
        return answer(css::iequals(a, in.rest()) ? 1 : 0, {});
    }
    case 'S': {
        const char sep = char(in.u8());
        auto parts = css::split_top_level(in.rest(), sep);
        o.u32(uint32_t(parts.size()));
        for (const auto& p : parts) o.str(p);
        return answer(1, o.s);
    }
    case 'D': {
        auto ds = css::parse_declarations(input);
        o.u32(uint32_t(ds.size()));
        for (const auto& d : ds) {
            o.str(d.name);
            o.str(d.value);
            o.u8(d.important ? 1 : 0);
        }
        return answer(1, o.s);
    }
    case 'n': return answer(css::is_tag_name(input) ? 1 : 0, {});
    case 'a': return answer(css::is_attribute_name(input) ? 1 : 0, {});
    case 'p': return answer(css::is_property_name(input) ? 1 : 0, {});
    case 'v': return answer(css::is_theme_variable(input) ? 1 : 0, {});
    case 'L': {
        const bool dark = in.u8() != 0;
        return answer(1, css::resolve_light_dark(in.rest(), dark));
    }
    case 'O': {
        const uint32_t at = in.u32();
        o.u32(uint32_t(css::skip_opaque(in.rest(), at)));
        return answer(1, o.s);
    }
    case 'T': {
        auto v = css::parse_time(input);
        if (!v) return answer(0, {});
        o.f64(*v);
        return answer(1, o.s);
    }
    case 'C': {
        Color c;
        if (!css::parse_color(input, c)) return answer(0, {});
        for (uint8_t v : {c.r, c.g, c.b, c.a}) o.u8(v);
        return answer(1, o.s);
    }
    case 'F': {
        const double d = in.f64();
        const int digits = int(int32_t(in.u32()));
        return answer(1, css::format_number(d, digits));
    }
    case 'f': {
        auto f = css::parse_font(input);
        o.str(f.family);
        o.f32(f.size);
        o.u32(uint32_t(f.weight));
        o.u8(f.italic ? 1 : 0);
        return answer(1, o.s);
    }
    case 'X': {
        const uint8_t which = in.u8();
        const float w = in.f32(), h = in.f32();
        auto text = in.rest();
        if (which == 4) {
            auto v = css::parse_origin(text, w, h);
            if (!v) return answer(0, {});
            o.f32(v->first);
            o.f32(v->second);
            return answer(1, o.s);
        }
        if (which == 5) {
            auto v = css::parse_opacity(text);
            if (!v) return answer(0, {});
            o.f32(*v);
            return answer(1, o.s);
        }
        std::optional<Affine> m = which == 0 ? css::parse_transform(text, w, h)
            : which == 1                     ? css::parse_translate(text, w, h)
            : which == 2                     ? css::parse_rotate(text)
                                             : css::parse_scale(text);
        if (!m) return answer(0, {});
        affine(o, *m);
        return answer(1, o.s);
    }
    case 'H':
    case 'A': {
        auto json = twidget::Json::parse(input);
        if (!json) return answer(2, {});
        pairs(o, op == 'H' ? css::theme_variables(*json) : css::content_safe_area_variables(*json));
        return answer(1, o.s);
    }
    case 'V': {
        const uint32_t n = in.u32();
        std::vector<std::pair<std::string, std::string>> vars;
        for (uint32_t i = 0; i < n; i++) {
            std::string k(in.bytes(in.u32()));
            std::string v(in.bytes(in.u32()));
            vars.emplace_back(std::move(k), std::move(v));
        }
        return answer(1, css::substitute_variables(in.rest(), vars));
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
