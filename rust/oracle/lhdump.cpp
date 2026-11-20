// lhdump: the litehtml fork's answers on stdin, for each stage of its Rust port (rust/PLAN.md, "L. The litehtml port"); rust/crates/litehtml/tests/common/mod.rs lays out every record and answer.
#include "codepoint.h"
#include "css.h"
#include "css_length.h"
#include "css_parser.h"
#include "css_tokenizer.h"
#include "document.h"
#include "document_container.h"
#include "encodings.h"
#include "html.h"
#include "html_tag.h"
#include "master_css.h"
#include "num_cvt.h"
#include "string_id.h"
#include "style.h"
#include "stylesheet.h"
#include "utf8_strings.h"
#include "web_color.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <map>
#include <memory>
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

// The document of the stylesheet cases: the helper's container as far as a stylesheet asks it (pt, the default font size, the media features of a 375 x 667 screen at 2x), with a few sheets to @import.
class OracleContainer final : public document_container {
public:
    uint_ptr create_font(const font_description&, const document*, font_metrics* fm) override {
        if (fm) *fm = font_metrics();
        return 1;
    }
    void delete_font(uint_ptr) override {}
    pixel_t text_width(const char* text, uint_ptr) override { return pixel_t(float(std::strlen(text) * 8)); }
    void draw_text(uint_ptr, const char*, uint_ptr, web_color, const position&) override {}
    pixel_t pt_to_px(float pt) const override { return pixel_t(pt * 96.0f / 72.0f); }
    pixel_t get_default_font_size() const override { return 16; }
    const char* get_default_font_name() const override { return "sans-serif"; }
    void draw_list_marker(uint_ptr, const list_marker&) override {}
    void load_image(const char*, const char*, bool) override {}
    void get_image_size(const char*, const char*, litehtml::size& sz) override { sz = litehtml::size(); }
    void draw_image(uint_ptr, const background_layer&, const std::string&, const std::string&) override {}
    void draw_solid_fill(uint_ptr, const background_layer&, const web_color&) override {}
    void draw_linear_gradient(uint_ptr, const background_layer&, const background_layer::linear_gradient&) override {}
    void draw_radial_gradient(uint_ptr, const background_layer&, const background_layer::radial_gradient&) override {}
    void draw_conic_gradient(uint_ptr, const background_layer&, const background_layer::conic_gradient&) override {}
    void draw_borders(uint_ptr, const borders&, const position&, bool) override {}
    void set_caption(const char*) override {}
    void set_base_url(const char*) override {}
    void link(const std::shared_ptr<document>&, const element::ptr&) override {}
    void on_anchor_click(const char*, const element::ptr&) override {}
    void on_mouse_event(const element::ptr&, mouse_event) override {}
    void set_cursor(const char*) override {}
    void transform_text(std::string&, text_transform) override {}
    void import_css(std::string& text, const std::string& url, std::string& baseurl) override {
        static const std::map<std::string, std::string> sheets = {
            {"a.css", "i{color:red}"},
            {"m.css", "@media print{p{x:y}} q{z:w}"},
            {"nested.css", "@import 'a.css' screen; r{s:t} @import 'm.css';"},
            {"layer.css", "@layer{u{v:w}} @supports (color:red){x{y:z}}"},
        };
        auto it = sheets.find(url);
        text = it == sheets.end() ? std::string() : it->second;
        if (it != sheets.end()) baseurl = "imported/" + url;
    }
    void set_clip(const position&, const border_radiuses&) override {}
    void del_clip() override {}
    void get_viewport(position& viewport) const override { viewport = position(0, 0, 375, 667); }
    element::ptr create_element(const char*, const string_map&, const std::shared_ptr<document>&) override { return nullptr; }
    void get_media_features(media_features& m) const override {
        m.type = media_type_screen;
        m.width = m.device_width = 375;
        m.height = m.device_height = 667;
        m.color = 8;
        m.color_index = 256;
        m.monochrome = 0;
        m.resolution = 192;
    }
    void get_language(std::string& language, std::string& culture) const override {
        language = "en";
        culture.clear();
    }
};

const document::ptr& stylesheet_document() {
    static OracleContainer container;
    static const document::ptr doc = document::createFromString("<!DOCTYPE html><html><head></head><body></body></html>", &container, "", "");
    return doc;
}

// The grid media queries are evaluated against, set by a 'g' record.
std::vector<media_features>& grid() {
    static std::vector<media_features> features;
    return features;
}

css_token_vector read_tokens(In& in) {
    css_token_vector tokens;
    const uint32_t n = in.u32();
    for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) tokens.push_back(read_token(in, 0));
    return tokens;
}

void write_declarations(Out& o, const raw_declaration::vector& decls) {
    o.u32(uint32_t(decls.size()));
    for (const auto& d : decls) {
        o.str(d.name);
        write_tokens(o, d.value);
        o.u8(d.important ? 1 : 0);
    }
}

// A rule, then what is inside its block: a style rule's declarations and rules, an at-rule's block as a sheet of rules.
void write_rules(Out& o, const raw_rule::vector& rules) {
    o.u32(uint32_t(rules.size()));
    for (const auto& rule : rules) {
        o.u8(uint8_t(rule->type));
        o.str(rule->name);
        write_tokens(o, rule->prelude);
        write_token(o, rule->block);
        if (rule->type == raw_rule::qualified) {
            raw_declaration::vector decls;
            raw_rule::vector nested;
            css_parser(rule->block.value).consume_style_block_contents(decls, nested);
            write_declarations(o, decls);
            write_rules(o, nested);
        } else if (rule->block.type == CURLY_BLOCK) {
            write_rules(o, css_parser::parse_stylesheet(rule->block.value, false));
        }
    }
}

void write_condition(Out& o, const media_condition& condition);

void write_in_parens(Out& o, const media_in_parens& item) {
    if (item.is<media_condition>()) {
        o.u8(0);
        write_condition(o, item.get<media_condition>());
    } else if (item.is<media_feature>()) {
        const auto& f = item.get<media_feature>();
        o.u8(1);
        o.str(f.name);
        o.f32(f.value);
        o.f32(f.value2);
        o.u32(f.op);
        o.u32(f.op2);
    } else {
        o.u8(2);
    }
}

void write_condition(Out& o, const media_condition& condition) {
    o.u32(uint32_t(condition.op));
    o.u32(uint32_t(condition.m_conditions.size()));
    for (const auto& item : condition.m_conditions) write_in_parens(o, item);
}

// The renderer's rewrite_color_scheme (core/src/renderer.cpp), copied as it is: litehtml knows no prefers-color-scheme.
std::string rewrite_color_scheme(const std::string& css, bool dark) {
    static constexpr std::string_view KEY = "prefers-color-scheme";
    std::string out;
    out.reserve(css.size());
    size_t pos = 0;
    for (size_t i = 0; i + KEY.size() <= css.size();) {
        if (size_t skip = twlite::css::skip_opaque(css, i); skip != i) {
            i = skip;
            continue;
        }
        if ((i > 0 && twlite::css::ident_char(css[i - 1])) || !twlite::css::iequals(std::string_view(css).substr(i, KEY.size()), KEY) ||
            (i + KEY.size() < css.size() && twlite::css::ident_char(css[i + KEY.size()]))) {
            i++;
            continue;
        }
        out.append(css, pos, i - pos);
        size_t j = i + KEY.size();
        while (j < css.size() && (css[j] == ' ' || css[j] == '\t' || css[j] == '\n' || css[j] == ':')) j++;
        size_t k = j;
        while (k < css.size() && std::isalpha((unsigned char)css[k])) k++;
        std::string value = twlite::css::lower(css.substr(j, k - j));
        bool matches = value.empty() ? true : (value == "dark") == dark;
        out += matches ? "min-width:0px" : "min-width:999999px";
        pos = i = k;
    }
    out.append(css, pos, std::string::npos);
    return out;
}

void write_selector(Out& o, const css_selector& sel);

void write_selector_list(Out& o, const css_selector::vector& list) {
    o.u32(uint32_t(list.size()));
    for (const auto& sel : list) write_selector(o, *sel);
}

void write_selector(Out& o, const css_selector& sel) {
    for (int v : {sel.m_specificity.a, sel.m_specificity.b, sel.m_specificity.c, sel.m_specificity.d, sel.m_order}) o.i32(v);
    o.u8(sel.m_layered ? 1 : 0);
    o.u8(uint8_t(sel.m_combinator));
    o.str(_s(sel.m_right.m_prefix));
    o.str(_s(sel.m_right.m_tag));
    o.u32(uint32_t(sel.m_right.m_attrs.size()));
    for (const auto& attr : sel.m_right.m_attrs) {
        o.u8(uint8_t(attr.type));
        o.str(_s(attr.prefix));
        o.str(_s(attr.name));
        o.str(attr.value);
        o.u8(uint8_t(attr.matcher));
        o.u8(attr.caseless_match ? 1 : 0);
        o.i32(attr.a);
        o.i32(attr.b);
        write_selector_list(o, attr.selector_list);
    }
    o.u8(sel.m_left ? 1 : 0);
    if (sel.m_left) write_selector(o, *sel.m_left);
}

// Whether a selector reads :hover or :active anywhere, nested lists included.
bool is_dynamic(const css_selector& sel) {
    for (const auto& attr : sel.m_right.m_attrs) {
        if (attr.type == select_pseudo_class && (attr.name == _hover_ || attr.name == _active_)) return true;
        for (const auto& nested : attr.selector_list)
            if (is_dynamic(*nested)) return true;
    }
    return sel.m_left && is_dynamic(*sel.m_left);
}

void collect_nodes(const element::ptr& el, int parent, std::vector<std::pair<element::ptr, int>>& nodes) {
    const int index = int(nodes.size());
    nodes.emplace_back(el, parent);
    for (const auto& child : el->children()) collect_nodes(child, index, nodes);
}

// The pseudo-classes a document may set on an element, as names.
const char* const PSEUDO_FLAGS[] = {"root", "hover", "active", "focus", "focus-visible", "focus-within", "link", "visited", "any-link", "local-link", "target", "target-within", "scope", "host"};

// The fork's DOM of a document: each node in tree order with its parent, kind, tag, id, classes, attributes and pseudo-class flags.
void write_dom(Out& o, const std::vector<std::pair<element::ptr, int>>& nodes) {
    o.u32(uint32_t(nodes.size()));
    for (const auto& [el, parent] : nodes) {
        const auto* tag = dynamic_cast<const html_tag*>(el.get());
        o.i32(parent);
        o.u8(tag ? 1 : 0);
        o.u8(el->is_comment() ? 1 : 0);
        o.u8(el->css().get_display() == display_inline_text ? 1 : 0);
        o.str(_s(el->tag()));
        o.str(tag ? _s(tag->id()) : std::string());
        o.u32(tag ? uint32_t(tag->classes().size()) : 0);
        if (tag)
            for (string_id cls : tag->classes()) o.str(_s(cls));
        o.u32(tag ? uint32_t(tag->attributes().size()) : 0);
        if (tag)
            for (const auto& [name, value] : tag->attributes()) {
                o.str(name);
                o.str(value);
            }
        std::vector<std::string> flags;
        for (const char* name : PSEUDO_FLAGS)
            if (el->has_pseudo_class(_id(name))) flags.emplace_back(name);
        o.u32(uint32_t(flags.size()));
        for (const auto& flag : flags) o.str(flag);
    }
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
    case 'g': { // a setting: the media features grid, u32 count, each i32 type, f32 width, height, device width and height, i32 color, color index, monochrome, f32 resolution
        grid().clear();
        const uint32_t n = in.u32();
        for (uint32_t i = 0; i < n; i++) {
            media_features m;
            m.type = media_type(in.i32());
            m.width = in.f32();
            m.height = in.f32();
            m.device_width = in.f32();
            m.device_height = in.f32();
            m.color = in.i32();
            m.color_index = in.i32();
            m.monochrome = in.i32();
            m.resolution = in.f32();
            grid().push_back(m);
        }
        return true;
    }
    case 'r': { // css_parser::parse_stylesheet of the text: u8 top level, the text; every rule and what its block holds
        const bool top_level = in.u8() != 0;
        write_rules(o, css_parser::parse_stylesheet(in.rest(), top_level));
        answer(1, o.s);
        return true;
    }
    case 'B': { // a style block's contents from text componentized: its raw declarations and rules, then the declarations as style::add takes them
        raw_declaration::vector decls;
        raw_rule::vector rules;
        css_parser(normalize(std::string(input), f_componentize)).consume_style_block_contents(decls, rules);
        write_declarations(o, decls);
        write_rules(o, rules);
        for (auto& decl : decls) {
            remove_whitespace(decl.value);
            if (decl.name.substr(0, 2) != "--") decl.name = lowcase(decl.name);
        }
        write_declarations(o, decls);
        answer(1, o.s);
        return true;
    }
    case 'e': { // parse_media_query_list of the text: every query's structure, then each grid point's trileans and the list's answer
        const media_query_list list = parse_media_query_list(std::string(input), stylesheet_document());
        o.u32(uint32_t(list.m_queries.size()));
        for (const auto& q : list.m_queries) {
            o.u8(q.m_not ? 1 : 0);
            o.i32(int32_t(q.m_media_type));
            o.u32(uint32_t(q.m_conditions.size()));
            for (const auto& c : q.m_conditions) write_condition(o, c);
        }
        for (const auto& features : grid()) {
            for (const auto& q : list.m_queries) o.u8(uint8_t(q.check(features)));
            o.u8(list.check(features) ? 1 : 0);
        }
        answer(1, o.s);
        return true;
    }
    case 'w': { // css::parse_css_stylesheet of the text: u8 scheme (0 as is, 1 light, 2 dark), the text; every selector's order, layered flag, rule and media list, then each media list over the grid
        const uint8_t scheme = in.u8();
        std::string text = in.rest();
        if (scheme) text = rewrite_color_scheme(text, scheme == 2);
        css sheet;
        sheet.parse_css_stylesheet(text, "", stylesheet_document());
        std::vector<const style*> rules;
        std::vector<media_query_list_list::ptr> lists;
        o.u32(uint32_t(sheet.selectors().size()));
        for (const auto& sel : sheet.selectors()) {
            auto rule = std::find(rules.begin(), rules.end(), sel->m_style.get());
            if (rule == rules.end()) rule = rules.insert(rules.end(), sel->m_style.get());
            int32_t list = -1;
            if (sel->m_media_query) {
                auto it = std::find(lists.begin(), lists.end(), sel->m_media_query);
                if (it == lists.end()) it = lists.insert(lists.end(), sel->m_media_query);
                list = int32_t(it - lists.begin());
            }
            o.i32(sel->m_order);
            o.u8(sel->m_layered ? 1 : 0);
            o.i32(int32_t(rule - rules.begin()));
            o.i32(list);
        }
        o.u32(uint32_t(lists.size()));
        for (const auto& list : lists) {
            for (const auto& features : grid()) {
                list->apply_media_features(features);
                o.u8(list->is_used() ? 1 : 0);
            }
        }
        answer(1, o.s);
        return true;
    }
    case 'o': { // the renderer's rewrite_color_scheme: u8 dark, the text
        const bool dark = in.u8() != 0;
        answer(1, rewrite_color_scheme(in.rest(), dark));
        return true;
    }
    case 'Y': { // whether style::add of tokens (u32 count, tokens) keeps a property, as @supports asks it of a declaration
        style st;
        st.add(read_tokens(in), "", stylesheet_document()->container());
        answer(st.empty() ? 0 : 1, {});
        return true;
    }
    case 'U': { // parse_url of a token: the url
        std::string url;
        const bool ok = parse_url(read_token(in, 0), url);
        answer(ok ? 1 : 0, url);
        return true;
    }
    case 'X': { // decode of the bytes as UTF-8 (a BOM may say otherwise)
        answer(1, decode(std::string(input), encoding::utf_8));
        return true;
    }
    case 'V': { // a length from a token in px: from_token with f_length_percentage, then the document's to_pixels and cvt_units against 16px metrics and a size of 200
        const css_token tok = read_token(in, 0);
        css_length len;
        const bool ok = len.from_token(tok, f_length_percentage);
        font_metrics fm;
        fm.font_size = fm.x_height = 16;
        fm.ch_width = 7;
        o.f32(stylesheet_document()->to_pixels(len, fm, 200).value());
        stylesheet_document()->cvt_units(len, fm, 200);
        write_length(o, len);
        answer(ok ? 1 : 0, o.s);
        return true;
    }
    case 'h': { // u8 flips, the html, u32 sheets and their texts: the fork's DOM of the html, then each sheet's selectors (structure, specificity, order) and each selector against each node as select with and without pseudo-classes; with flips, :hover and :active set on every k-th element (at most 48) and the selectors that read them against each node
        static OracleContainer container;
        const bool flips = in.u8() != 0;
        const std::string html = in.str();
        std::vector<std::string> sheets(in.u32());
        for (auto& sheet : sheets) sheet = in.str();
        const document::ptr doc = document::createFromString(html, &container);
        std::vector<std::pair<element::ptr, int>> nodes;
        if (doc->root()) collect_nodes(doc->root(), -1, nodes);
        o.u8(uint8_t(doc->mode()));
        write_dom(o, nodes);
        std::vector<css_selector::ptr> selectors;
        for (const auto& text : sheets) {
            css sheet;
            sheet.parse_css_stylesheet(text, "", doc);
            o.u32(uint32_t(sheet.selectors().size()));
            for (const auto& sel : sheet.selectors()) {
                write_selector(o, *sel);
                selectors.push_back(sel);
            }
        }
        for (const auto& sel : selectors)
            for (const auto& [el, parent] : nodes) {
                o.u8(uint8_t(el->select(*sel, true)));
                o.u8(uint8_t(el->select(*sel, false)));
            }
        if (flips) {
            std::vector<css_selector::ptr> dynamic;
            for (const auto& sel : selectors)
                if (is_dynamic(*sel)) dynamic.push_back(sel);
            std::vector<element::ptr> tags;
            for (const auto& [el, parent] : nodes)
                if (dynamic_cast<const html_tag*>(el.get())) tags.push_back(el);
            const size_t step = std::max<size_t>(1, (tags.size() + 47) / 48);
            for (size_t t = 0; t < tags.size(); t += step) {
                tags[t]->on_mouse_over();
                for (int pass = 0; pass < 2; pass++) {
                    if (pass == 1) tags[t]->on_lbutton_down();
                    for (const auto& sel : dynamic)
                        for (const auto& [el, parent] : nodes) o.u8(uint8_t(el->select(*sel, true)));
                }
                tags[t]->on_mouse_leave();
            }
        }
        answer(1, o.s);
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
