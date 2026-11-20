// lhdump: the litehtml fork's answers on stdin, for each stage of its Rust port (rust/PLAN.md, "L. The litehtml port"); rust/crates/litehtml/tests/common/mod.rs lays out every record and answer.
#include "codepoint.h"
#include "css_length.h"
#include "css_parser.h"
#include "css_tokenizer.h"
#include "html.h"
#include "master_css.h"
#include "num_cvt.h"
#include "string_id.h"
#include "utf8_strings.h"
#include "web_color.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

#ifdef _WIN32
#define LH_ENVIRON _environ
#else
extern char** environ;
#define LH_ENVIRON environ
#endif

namespace litehtml {
bool parse_angle(const css_token& tok, float& angle, bool percents_allowed);
bool parse_hash_color(const css_token& tok, web_color& color);
bool parse_rgb_func(const css_token& tok, web_color& color);
bool parse_hsl_func(const css_token& tok, web_color& color);
bool parse_func_color(const css_token& tok, web_color& color);
void hsl_to_rgb(float hue, float sat, float light, float& r, float& g, float& b);
float clamp(float x, float min, float max);
byte calc_percent_and_clamp(const css_length& val, float max);
std::string resolve_name(const std::string& name, document_container* container);
void filter_code_points(std::string& input);
} // namespace litehtml

using namespace litehtml;

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
        at = std::min(at + sizeof v, s.size());
        return v;
    }
    uint8_t u8() { return raw<uint8_t>(); }
    uint32_t u32() { return raw<uint32_t>(); }
    int32_t i32() { return raw<int32_t>(); }
    float f32() { return raw<float>(); }
    double f64() { return raw<double>(); }
    std::string str() {
        const size_t n = std::min<size_t>(u32(), s.size() - at);
        std::string out(s.substr(at, n));
        at += n;
        return out;
    }
    std::string rest() {
        std::string out(s.substr(at));
        at = s.size();
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
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
};

void answer(uint8_t status, const std::string& bytes) {
    const uint32_t n = uint32_t(bytes.size());
    const unsigned char head[5] = {status, uint8_t(n), uint8_t(n >> 8), uint8_t(n >> 16), uint8_t(n >> 24)};
    std::fwrite(head, 1, 5, stdout);
    if (n) std::fwrite(bytes.data(), 1, n, stdout);
}

bool is_numeric(int type) {
    return type == NUMBER || type == PERCENTAGE || type == DIMENSION;
}

// A token as a case gives it: its fields set as they come, its value whatever kind it is.
css_token read_token(In& in, int depth) {
    css_token t;
    t.ch = in.i32();
    const float number = in.f32();
    const uint8_t number_type = in.u8();
    const uint8_t hash_type = in.u8();
    if (is_numeric(t.type)) t.n = {number, css_number_type(number_type)};
    if (t.type == HASH) t.hash_type = css_hash_type(hash_type);
    t.str() = in.str();
    t.repr = in.str();
    const uint32_t children = in.u32();
    for (uint32_t i = 0; i < children && depth < 64 && in.at < in.s.size(); i++) t.value.push_back(read_token(in, depth + 1));
    return t;
}

void write_token(Out& o, const css_token& t) {
    o.i32(t.type);
    o.f32(is_numeric(t.type) ? t.n.number : 0.f);
    o.u8(is_numeric(t.type) ? uint8_t(t.n.number_type) : 0);
    o.u8(t.type == HASH ? uint8_t(t.hash_type) : 0);
    o.str(t.str());
    o.str(t.repr);
    o.u32(uint32_t(t.value.size()));
    for (const auto& v : t.value) write_token(o, v);
}

void write_color(Out& o, const web_color& c) {
    for (byte v : {c.red, c.green, c.blue, c.alpha}) o.u8(v);
    o.u8(c.is_current_color ? 1 : 0);
}

#define TABLES(T) \
    T(style_display_strings) T(font_size_strings) T(line_height_strings) T(font_style_strings) T(font_system_family_name_strings) T(font_variant_strings) \
    T(font_weight_strings) T(list_style_type_strings) T(list_style_position_strings) T(vertical_align_strings) T(border_width_strings) \
    T(border_style_strings) T(element_float_strings) T(element_clear_strings) T(css_units_strings) T(background_attachment_strings) \
    T(background_repeat_strings) T(background_box_strings) T(background_position_strings) T(element_position_strings) T(direction_strings) \
    T(text_align_strings) T(text_transform_strings) T(font_smoothing_strings) T(white_space_strings) T(overflow_strings) T(background_size_strings) \
    T(visibility_strings) T(border_collapse_strings) T(table_layout_strings) T(content_property_strings) T(user_select_strings) \
    T(overflow_wrap_strings) T(word_break_strings) T(resize_strings) T(appearance_strings) T(box_sizing_strings) T(media_type_strings) \
    T(flex_direction_strings) T(flex_wrap_strings) T(flex_justify_content_strings) T(self_position_strings) T(flex_align_items_strings) \
    T(flex_align_content_strings) T(flex_basis_strings) T(caption_side_strings) T(style_text_decoration_line_strings) \
    T(style_text_decoration_style_strings) T(style_text_decoration_thickness_strings) T(style_text_emphasis_position_strings) \
    T(radial_extent_strings) T(color_space_strings) T(hue_interpolation_strings)

struct Table {
    const char* name;
    css_values values;
};

const std::vector<Table>& tables() {
    static const std::vector<Table> all = {
#define ENTRY(name) {#name, css_values(name)},
        TABLES(ENTRY)
#undef ENTRY
    };
    return all;
}

css_values keywords(uint8_t index) {
    return index < tables().size() ? tables()[index].values : css_values();
}

// The convert function of the calc cases: each unit its own scale, so a leaf converted shows.
float scale_unit(float value, css_units unit) {
    return value * (float(int(unit)) + 0.5f);
}

void write_length(Out& o, const css_length& len) {
    o.u8(len.is_predefined() ? 1 : 0);
    o.i32(len.predef());
    o.u8(uint8_t(len.units()));
    o.f32(len.val());
    o.u8(len.calc() ? 1 : 0);
    o.str(len.to_string());
    for (float w : {0.f, 100.f, -37.5f, 1e9f, 3.3f}) o.f32(len.calc_percent(w).value());
    for (float base : {0.f, 250.f, -1.f}) {
        o.f32(calc_evaluate(len.calc().get(), base).value());
        o.f32(calc_evaluate(len.calc().get(), base, scale_unit).value());
    }
    float px = -1;
    auto resolved = calc_resolve_units(len.calc(), scale_unit, px);
    o.f32(px);
    o.u8(!resolved ? 0 : resolved == len.calc() ? 1 : 2);
    o.f32(calc_evaluate(resolved.get(), 300).value());
}

void write_tokens(Out& o, const css_token_vector& tokens) {
    o.u32(uint32_t(tokens.size()));
    for (const auto& t : tokens) write_token(o, t);
}

// The keep_whitespace of the cases: none, or media queries' (whitespace stays between < or > and =).
keep_whitespace_fn keeper(uint8_t mode) {
    if (mode == 0) return nullptr;
    return [](const css_token& left, const css_token& right) { return is_one_of(left.ch, '<', '>') && right.ch == '='; };
}

// The LH_NO_* switches are off unless a case sets one: none comes in from the environment.
void clear_switches() {
    std::vector<std::string> names;
    for (char** e = LH_ENVIRON; e && *e; e++)
        if (std::strncmp(*e, "LH_NO_", 6) == 0) names.emplace_back(*e, std::strcspn(*e, "="));
#ifdef _WIN32
    for (const auto& name : names) _putenv_s(name.c_str(), "");
#else
    for (const auto& name : names) unsetenv(name.c_str());
#endif
}

bool run(int op, std::string_view input) {
    In in{input};
    Out o;
    switch (op) {
    case 'E': { // a setting: one LH_* switch as NAME=VALUE, before the case that first reads it
        const std::string text(input);
        const size_t eq = text.find('=');
        if (eq == std::string::npos) return false;
#ifdef _WIN32
        _putenv_s(text.substr(0, eq).c_str(), text.substr(eq + 1).c_str());
#else
        setenv(text.substr(0, eq).c_str(), text.substr(eq + 1).c_str(), 1);
#endif
        return true;
    }
    case 'd': { // t_strtod and t_strtof of the text: f64 bits, u32 end, f32 bits
        const std::string text(input);
        char* end = nullptr;
        const double v = t_strtod(text.c_str(), &end);
        o.f64(v);
        o.u32(uint32_t(end - text.c_str()));
        o.f32(t_strtof(text));
        answer(1, o.s);
        return true;
    }
    case 'c': { // convert_string_to_number of the text: f64 bits, and as the tokenizer narrows it to float
        const double v = css_tokenizer::convert_string_to_number(std::string(input));
        o.f64(v);
        o.f32(static_cast<float>(v));
        answer(1, o.s);
        return true;
    }
    case 'p': { // pixel_t: f32 a, f32 b, i32 k through every operator
        const pixel_t a = in.f32(), b = in.f32();
        const int k = in.i32();
        o.u8(uint8_t(int(a == b) | int(a != b) << 1 | int(a < b) << 2 | int(a > b) << 3 | int(a <= b) << 4 | int(a >= b) << 5));
        for (pixel_t v : {std::max(a, b), std::min(a, b), a + b, a - b, a * b, a / b, -a, a + k, a - k, a * k, a / k, k + a, k - a, k * a, k / a})
            o.f32(v.value());
        pixel_t c = a;
        c += b;
        o.f32(c.value());
        c -= k;
        o.f32(c.value());
        c *= b;
        o.f32(c.value());
        c /= k;
        o.f32(c.value());
        o.f32((++c).value());
        o.f32((c--).value());
        o.f32(c.value());
        o.i32(int(a));
        o.i32(round_f(a.value()));
        o.i32(round_d(double(a.value())));
        o.f32(pixel_t(k).value());
        o.f32(clamp_length(a.value()));
        answer(1, o.s);
        return true;
    }
    case 'n': { // a script on the name table: u32 steps, each 'i' str (_id), 'f' str (_find_id), 's' u32 (_s) or 'u' (_id_usage)
        const uint32_t steps = in.u32();
        for (uint32_t i = 0; i < steps; i++) {
            switch (in.u8()) {
            case 'i': o.u32(uint32_t(_id(in.str()))); break;
            case 'f': {
                string_id id = string_id(0);
                const bool found = _find_id(in.str(), id);
                o.u8(found ? 1 : 0);
                o.u32(found ? uint32_t(id) : 0);
                break;
            }
            case 's': o.str(_s(string_id(in.u32()))); break;
            case 'u': {
                const auto usage = _id_usage();
                o.u64(usage.names);
                o.u64(usage.bytes);
                break;
            }
            default: return false;
            }
        }
        answer(1, o.s);
        return true;
    }
    case 'N': { // the whole name table in id order
        const auto usage = _id_usage();
        o.u32(uint32_t(usage.names));
        for (size_t i = 0; i < usage.names; i++) o.str(_s(string_id(i)));
        o.u32(uint32_t(empty_id));
        o.u32(uint32_t(star_id));
        answer(1, o.s);
        return true;
    }
    case 't': { // the keyword tables and the float tables beside them
        o.u32(uint32_t(tables().size()));
        for (const auto& t : tables()) {
            o.str(t.name);
            o.u32(uint32_t(t.values.size));
            for (size_t i = 0; i < t.values.size; i++) o.str(t.values.data[i]);
        }
        for (float v : border_width_values) o.f32(v);
        for (float v : background_position_percentages) o.f32(v);
        answer(1, o.s);
        return true;
    }
    case 'v': { // u8 table, u32 index, text: value_index of the text, value_by_index of the index
        const css_values values = keywords(in.u8());
        const uint32_t index = in.u32();
        const std::string text = in.rest();
        const auto found = values.value_index(text);
        o.u8(found ? 1 : 0);
        o.i32(found ? *found : 0);
        o.str(values.value_by_index(index));
        o.u8(values.has(text) ? 1 : 0);
        answer(1, o.s);
        return true;
    }
    case 'C': { // u8 which (0 hash, 1 rgb, 2 hsl, 3 either function), a token: the colour or status 0
        const uint8_t which = in.u8();
        const css_token tok = read_token(in, 0);
        web_color color(1, 2, 3, 4);
        const bool ok = which == 0 ? parse_hash_color(tok, color)
            : which == 1           ? parse_rgb_func(tok, color)
            : which == 2           ? parse_hsl_func(tok, color)
                                   : parse_func_color(tok, color);
        write_color(o, color);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'H': { // hsl_to_rgb of f32 hue, saturation, lightness
        const float h = in.f32(), s = in.f32(), l = in.f32();
        float r = 0, g = 0, b = 0;
        hsl_to_rgb(h, s, l, r, g, b);
        for (float v : {r, g, b}) o.f32(v);
        answer(1, o.s);
        return true;
    }
    case 'q': { // clamp of f32 x, min, max; calc_percent_and_clamp of f32 value, u8 units, f32 max
        const float x = in.f32(), lo = in.f32(), hi = in.f32();
        o.f32(litehtml::clamp(x, lo, hi));
        const float value = in.f32();
        const css_units units = css_units(in.u8());
        o.u8(calc_percent_and_clamp(css_length(value, units), in.f32()));
        answer(1, o.s);
        return true;
    }
    case 'D': { // darken: r, g, b, a, f64 fraction
        const byte r = in.u8(), g = in.u8(), b = in.u8(), a = in.u8();
        write_color(o, web_color(r, g, b, a).darken(in.f64()));
        answer(1, o.s);
        return true;
    }
    case 'S': { // to_string of r, g, b, a
        const byte r = in.u8(), g = in.u8(), b = in.u8(), a = in.u8();
        answer(1, web_color(r, g, b, a).to_string());
        return true;
    }
    case 'R': { // resolve_name of the text, no container
        answer(1, resolve_name(std::string(input), nullptr));
        return true;
    }
    case 'L': { // css_length::from_token: u8 start (0 default, 1 2.5em, 2 predef 7, 3 a calc), i32 options, u8 keywords table, a token
        const uint8_t start = in.u8();
        const int options = in.i32();
        const css_values words = keywords(in.u8());
        const css_token tok = read_token(in, 0);
        css_length len;
        if (start == 1) len.set_value(2.5f, css_units_em);
        if (start == 2) len.predef(7);
        if (start == 3) {
            css_token calc(CV_FUNCTION, "calc");
            calc.value = {css_token(PERCENTAGE, 50.f), css_token(WHITESPACE), css_token(css_token_type('-')), css_token(WHITESPACE), css_token(DIMENSION, 3.f, css_number_integer, "em")};
            len.from_token(calc, f_length_percentage);
        }
        const bool ok = len.from_token(tok, options, words);
        write_length(o, len);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'A': { // parse_angle: u8 percents_allowed, a token
        const bool percents = in.u8() != 0;
        const css_token tok = read_token(in, 0);
        float angle = -7.f;
        const bool ok = parse_angle(tok, angle, percents);
        o.f32(angle);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'k': { // num_cvt: u8 style (0 latin lower, 1 latin upper, 2 greek, 3 roman lower, 4 roman upper), i32 value
        const uint8_t style = in.u8();
        const int value = in.i32();
        answer(1, style == 0 ? num_cvt::to_latin_lower(value)
                : style == 1 ? num_cvt::to_latin_upper(value)
                : style == 2 ? num_cvt::to_greek_lower(value)
                : style == 3 ? num_cvt::to_roman_lower(value)
                             : num_cvt::to_roman_upper(value));
        return true;
    }
    case 'u': { // UTF-8 of the text: every read_utf8_char from 0, prev_utf8_char from the end, is_valid_utf8, utf8_to_utf32 and back
        const std::string text(input);
        std::vector<std::pair<uint32_t, uint32_t>> reads;
        for (int index = 0;;) {
            const int before = index;
            const char32_t ch = read_utf8_char(text, index);
            reads.emplace_back(uint32_t(ch), uint32_t(index));
            if (index == before) break;
        }
        o.u32(uint32_t(reads.size()));
        for (auto [ch, index] : reads) {
            o.u32(ch);
            o.u32(index);
        }
        std::vector<uint32_t> backs;
        for (int index = int(text.size()); index > 0;) {
            prev_utf8_char(text, index);
            backs.push_back(uint32_t(index));
        }
        o.u32(uint32_t(backs.size()));
        for (uint32_t index : backs) o.u32(index);
        o.u8(is_valid_utf8(text) ? 1 : 0);
        const std::u32string wide(static_cast<const char32_t*>(utf8_to_utf32(text)));
        o.u32(uint32_t(wide.size()));
        for (char32_t ch : wide) o.u32(uint32_t(ch));
        o.str(utf32_to_utf8(wide).c_str());
        answer(1, o.s);
        return true;
    }
    case 'a': { // append_char of each u32 code point
        std::string out;
        while (in.at + 4 <= in.s.size()) append_char(out, char32_t(in.u32()));
        answer(1, out);
        return true;
    }
    case 's': { // split_string: delims, delims_preserve, quote, then the text
        const std::string delims = in.str(), preserve = in.str(), quote = in.str();
        string_vector tokens;
        split_string(in.rest(), tokens, delims, preserve, quote);
        o.u32(uint32_t(tokens.size()));
        for (const auto& t : tokens) o.str(t);
        answer(1, o.s);
        return true;
    }
    case 'm': { // html.cpp's helpers on a and b
        const std::string a = in.str(), b = in.str();
        o.str(trim(a));
        o.str(trim(a, b));
        std::string lower = a;
        o.str(lcase(lower));
        o.i32(t_strcasecmp(a.c_str(), b.c_str()));
        for (size_t n : {size_t(0), size_t(1), size_t(3), a.size(), b.size()}) o.i32(t_strncasecmp(a.c_str(), b.c_str(), n));
        o.u8(equal_i(a, b) ? 1 : 0);
        o.u8(is_number(a, true) ? 1 : 0);
        o.u8(is_number(a, false) ? 1 : 0);
        o.str(get_escaped_string(a));
        for (char open : {'(', '[', '{'}) {
            const auto at = find_close_bracket(a, 0, open, open == '(' ? ')' : open == '[' ? ']' : '}');
            o.i32(at == std::string::npos ? -1 : int32_t(at));
        }
        for (int index : {0, std::min(1, int(a.size())), -1, -2, int(a.size())}) {
            o.u8(match(a, index, b) ? 1 : 0);
            o.u8(match_i(a, index, b) ? 1 : 0);
        }
        std::string joined;
        join_string(joined, {a, b, a}, b);
        o.str(joined);
        answer(1, o.s);
        return true;
    }
    case 'T': { // tokenize of the text as it is: the tokens
        write_tokens(o, tokenize(std::string(input)));
        answer(1, o.s);
        return true;
    }
    case 'Z': { // normalize: u8 options, u8 keep (0 none, 1 media queries'), the text: the tokens, get_repr of them without and with spaces
        const int options = in.u8();
        const keep_whitespace_fn keep = keeper(in.u8());
        const css_token_vector tokens = normalize(in.rest(), options, keep);
        write_tokens(o, tokens);
        o.str(get_repr(tokens, 0, -1, false));
        o.str(get_repr(tokens, 0, -1, true));
        answer(1, o.s);
        return true;
    }
    case 'F': { // filter_code_points of the text
        std::string text(input);
        filter_code_points(text);
        answer(1, text);
        return true;
    }
    case 'K': { // a colour from text: normalize componentized without whitespace, u32 tokens, then parse_color of a single one and the colour
        const css_token_vector tokens = normalize(std::string(input), f_componentize | f_remove_whitespace);
        o.u32(uint32_t(tokens.size()));
        web_color color(1, 2, 3, 4);
        const bool ok = tokens.size() == 1 && parse_color(tokens[0], color, nullptr);
        write_color(o, color);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'G': { // a length from text: i32 options, u8 keywords table, the text normalized as for 'K', from_token of a single token
        const int options = in.i32();
        const css_values words = keywords(in.u8());
        const css_token_vector tokens = normalize(in.rest(), f_componentize | f_remove_whitespace);
        o.u32(uint32_t(tokens.size()));
        css_length len;
        const bool ok = tokens.size() == 1 && len.from_token(tokens[0], options, words);
        write_length(o, len);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'P': { // the text componentized: its comma-separated lists, is_any_value, is_declaration_value, skip_whitespace from 0
        const css_token_vector tokens = normalize(std::string(input), f_componentize);
        const auto lists = parse_comma_separated_list(tokens);
        o.u32(uint32_t(lists.size()));
        for (const auto& list : lists) write_tokens(o, list);
        o.u8(is_any_value(tokens) ? 1 : 0);
        o.u8(is_declaration_value(tokens, 0) ? 1 : 0);
        int index = 0;
        o.u8(skip_whitespace(tokens, index) ? 1 : 0);
        o.i32(index);
        for (const auto& t : tokens) o.str(t.ident());
        answer(1, o.s);
        return true;
    }
    case 'M': { // litehtml's master_css
        answer(1, master_css);
        return true;
    }
    case 'x': { // every byte's URL classes: bit 0 reserved, bit 1 scheme, bit 2 ASCII
        for (int c = 0; c < 256; c++) {
            const char ch = char(c);
            o.u8(uint8_t(int(is_url_reserved_codepoint(ch)) | int(is_url_scheme_codepoint(ch)) << 1 | int(is_ascii_codepoint(ch)) << 2));
        }
        answer(1, o.s);
        return true;
    }
    }
    return false;
}

} // namespace

int main() {
    clear_switches();
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
        if (!run(op, bytes)) return 3;
    }
    std::fflush(stdout);
    return 0;
}
