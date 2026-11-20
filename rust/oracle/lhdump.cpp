// lhdump: the litehtml fork's answers on stdin, for each stage of its Rust port (rust/PLAN.md, "L. The litehtml port"); rust/crates/litehtml/tests/common/mod.rs lays out every record and answer.
#include "codepoint.h"
#include "css.h"
#include "css_effects.h"
#include "css_length.h"
#include "css_parser.h"
#include "css_tokenizer.h"
#include "document.h"
#include "document_container.h"
#include "encodings.h"
#include "html.h"
#include "html_tag.h"
#include "lhdump_dom.h"
#include "lhdump_grid.h"
#include "lhdump_gumbo.h"
#include "master_css.h"
#include "num_cvt.h"
#include "render_block.h"
#include "render_block_context.h"
#include "render_flex.h"
#include "render_grid.h"
#include "render_image.h"
#include "render_inline.h"
#include "render_inline_context.h"
#include "render_item.h"
#include "render_table.h"
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
#include <set>
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

// The members the 'l' record dumps that the fork keeps private, robbed as lhdump_dom.h robs its own.
namespace lhdump_dom {
#define LHDUMP_ROB_L(Name, Member) \
    struct Name {}; \
    auto robbed(Name); \
    template struct Rob<&Member, Name>;
LHDUMP_ROB_L(CssZIndex, css_properties::m_z_index)
LHDUMP_ROB_L(CssLineHeightRaw, css_properties::m_css_line_height)
LHDUMP_ROB_L(CssFontSize, css_properties::m_font_size)
LHDUMP_ROB_L(CssFontSmoothing, css_properties::m_font_smoothing)
LHDUMP_ROB_L(CssLetterSpacing, css_properties::m_letter_spacing)
LHDUMP_ROB_L(CssWordSpacing, css_properties::m_word_spacing)
LHDUMP_ROB_L(CssVariantNumeric, css_properties::m_font_variant_numeric)
LHDUMP_ROB_L(CssFeatureSettings, css_properties::m_font_feature_settings)
LHDUMP_ROB_L(CssBackface, css_properties::m_backface_hidden)
LHDUMP_ROB_L(CssOutlineOffset, css_properties::m_outline_offset)
LHDUMP_ROB_L(CssDarkScheme, css_properties::m_dark_scheme)
LHDUMP_ROB_L(CssKeywordSize, css_properties::m_keyword_size)
LHDUMP_ROB_L(CssGenericMono, css_properties::m_generic_mono)
LHDUMP_ROB_L(ElCounterValues, element::m_counter_values)
LHDUMP_ROB_L(ElUsedStyles, element::m_used_styles)
LHDUMP_ROB_L(TagStyleBase, html_tag::m_style_base)
LHDUMP_ROB_L(TagHasStyleBase, html_tag::m_has_style_base)
LHDUMP_ROB_L(TagCounterChanges, html_tag::m_counter_changes)
LHDUMP_ROB_L(TagAnonymous, html_tag::m_anonymous_style)
LHDUMP_ROB_L(TagAttrs, html_tag::m_attrs)
LHDUMP_ROB_L(TextTransformed, el_text::m_transformed_text)
LHDUMP_ROB_L(TextUseTransformed, el_text::m_use_transformed)
LHDUMP_ROB_L(TextDrawSpaces, el_text::m_draw_spaces)
LHDUMP_ROB_L(TextSize, el_text::m_size)
LHDUMP_ROB_L(TextMeasuredFont, el_text::m_measured_font)
LHDUMP_ROB_L(TextMeasuredTransform, el_text::m_measured_transform)
LHDUMP_ROB_L(GenGenerations, el_before_after_base::m_generations)
LHDUMP_ROB_L(GenBoxes, el_before_after_base::m_generated_boxes)
LHDUMP_ROB_L(GenBytes, el_before_after_base::m_generated_bytes)
LHDUMP_ROB_L(InputLook, el_input::m_look)
LHDUMP_ROB_L(SelectLook, el_select::m_look)
LHDUMP_ROB_L(SelectArrow, el_select::m_arrow)
LHDUMP_ROB_L(ButtonLook, el_button::m_look)
LHDUMP_ROB_L(TextareaLook, el_textarea::m_look)
LHDUMP_ROB_L(MeterLook, el_meter::m_look)
LHDUMP_ROB_L(DocFonts, document::m_fonts)
LHDUMP_ROB_L(DocShareCache, document::m_share_cache)
LHDUMP_ROB_L(DocMediaLists, document::m_media_lists)
LHDUMP_ROB_L(DocIdsSelectable, document::m_ids_selectable)
LHDUMP_ROB_L(DocGeneratedBoxes, document::m_generated_boxes)
LHDUMP_ROB_L(DocGeneratedBytes, document::m_generated_bytes)
LHDUMP_ROB_L(DocTabular, document::m_tabular_elements)
LHDUMP_ROB_L(DocTableCells, document::m_table_cells)
LHDUMP_ROB_L(DocTableLimited, document::m_table_limited)
LHDUMP_ROB_L(DocRenderSerial, document::m_render_tree_serial)
LHDUMP_ROB_L(ElRenders, element::m_renders)
LHDUMP_ROB_L(TableGridOf, render_item_table::m_grid)
LHDUMP_ROB_L(TableSpacingX, render_item_table::m_border_spacing_x)
LHDUMP_ROB_L(TableSpacingY, render_item_table::m_border_spacing_y)
LHDUMP_ROB_L(RiLayoutDirty, render_item::m_layout_dirty)
LHDUMP_ROB_L(RiRelayoutPending, render_item::m_relayout_pending)
LHDUMP_ROB_L(RiOutOfFlow, render_item::m_out_of_flow)
LHDUMP_ROB_L(RiLayoutEpoch, render_item::m_layout_epoch)
LHDUMP_ROB_L(RiLastLayout, render_item::m_last_layout)
LHDUMP_ROB_L(RiLastPos, render_item::m_last_pos)
LHDUMP_ROB_L(RiLastX, render_item::m_last_x)
LHDUMP_ROB_L(RiLastY, render_item::m_last_y)
LHDUMP_ROB_L(RiLastMargins, render_item::m_last_margins)
LHDUMP_ROB_L(RiLastPadding, render_item::m_last_padding)
LHDUMP_ROB_L(RiLastBorders, render_item::m_last_borders)
LHDUMP_ROB_L(RiMeasured, render_item::m_measured)
LHDUMP_ROB_L(RiMeasuredNext, render_item::m_measured_next)
LHDUMP_ROB_L(RiResolvedSafe, render_item::m_resolved_layout_safe)
LHDUMP_ROB_L(RiPositioned, render_item::m_positioned)
LHDUMP_ROB_L(InlineBoxes, render_item_inline::m_boxes)
LHDUMP_ROB_L(BcVaApplied, render_item_block_context::m_va_applied)
LHDUMP_ROB_L(IcTrailing, render_item_inline_context::m_trailing)
LHDUMP_ROB_L(IcFragmented, render_item_inline_context::m_fragmented_texts)
LHDUMP_ROB_L(IcLines, render_item_inline_context::m_line_boxes)
LHDUMP_ROB_L(IcRendered, render_item_inline_context::m_rendered_width)
LHDUMP_ROB_L(IcVaApplied, render_item_inline_context::m_va_applied)
LHDUMP_ROB_L(IcEllipses, render_item_inline_context::m_ellipses)
LHDUMP_ROB_L(IcCut, render_item_inline_context::m_cut)
LHDUMP_ROB_L(FlexLines, render_item_flex::m_lines)
LHDUMP_ROB_L(LbDefaultLineHeight, line_box::m_default_line_height)
LHDUMP_ROB_L(LbFontMetrics, line_box::m_font_metrics)
LHDUMP_ROB_L(LbTextAlign, line_box::m_text_align)
LHDUMP_ROB_L(LbRtl, line_box::m_rtl)
LHDUMP_ROB_L(LbiMinWidth, line_box_item::m_rendered_min_width)
LHDUMP_ROB_L(TextBreaker, el_text::m_breaker)
LHDUMP_ROB_L(DocSize, document::m_size)
#undef LHDUMP_ROB_L
} // namespace lhdump_dom

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
    uint64_t u64() { return raw<uint64_t>(); }
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
class OracleContainer : public document_container {
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

// The containers of the declaration cases: lhdump's own (375 x 667), the renderer's (its sanitize_declaration copied as core/src/renderer.cpp has it, untrusted, 412 x 915), and one that resolves a few colour names.
class RendererLikeContainer final : public OracleContainer {
public:
    bool sanitize_declaration(const std::string& name, const css_token_vector& value) override {
        if (value.size() != 1 || (name != "position" && name != "display")) return true;
        const std::string keyword = value[0].ident();
        return name == "position" ? keyword != "sticky" : keyword != "list-item" && keyword != "inline-text";
    }
    void get_media_features(media_features& m) const override {
        OracleContainer::get_media_features(m);
        m.width = m.device_width = 412;
        m.height = m.device_height = 915;
    }
};

class ColorNamesContainer final : public OracleContainer {
public:
    std::string resolve_color(const std::string& color) const override {
        if (color == "brand") return "#0a0b0c";
        if (color == "Accentx") return "rgb(1 2 3 / 50%)";
        if (color == "twotok") return "red blue";
        if (color == "alias") return "brand";
        return {};
    }
};

document_container* declaration_container(uint8_t mode) {
    static OracleContainer plain;
    static RendererLikeContainer renderer;
    static ColorNamesContainer names;
    if (mode == 1) return &plain;
    if (mode == 2) return &renderer;
    if (mode == 3) return &names;
    return nullptr;
}

void write_gradient(Out& o, const gradient& g) {
    o.str(_s(g.m_type));
    o.u32(g.m_side);
    o.f32(g.angle);
    o.u32(uint32_t(g.m_colors.size()));
    for (const auto& stop : g.m_colors) {
        o.u8(stop.is_color_hint ? 1 : 0);
        write_color(o, stop.color);
        o.u8(stop.length ? 1 : 0);
        if (stop.length) write_length(o, *stop.length);
        o.u8(stop.angle ? 1 : 0);
        if (stop.angle) o.f32(*stop.angle);
    }
    write_length(o, g.position_x);
    write_length(o, g.position_y);
    o.i32(int32_t(g.radial_shape));
    o.i32(int32_t(g.radial_extent));
    write_length(o, g.radial_radius_x);
    write_length(o, g.radial_radius_y);
    o.f32(g.conic_from_angle);
    o.i32(int32_t(g.color_space));
    o.i32(int32_t(g.hue_interpolation));
}

// A property's value: its flags and order, the variant's index, then what that alternative holds.
void write_property(Out& o, const property_value& p) {
    o.u8(p.m_important ? 1 : 0);
    o.u8(p.m_has_var ? 1 : 0);
    o.i32(p.m_order);
    o.u8(uint8_t(p.index()));
    if (p.is<css_wide_keyword>()) {
        o.u8(uint8_t(p.get<css_wide_keyword>().kind));
    } else if (p.is<int>()) {
        o.i32(p.get<int>());
    } else if (p.is<int_vector>()) {
        o.u32(uint32_t(p.get<int_vector>().size()));
        for (int v : p.get<int_vector>()) o.i32(v);
    } else if (p.is<css_length>()) {
        write_length(o, p.get<css_length>());
    } else if (p.is<length_vector>()) {
        o.u32(uint32_t(p.get<length_vector>().size()));
        for (const auto& v : p.get<length_vector>()) write_length(o, v);
    } else if (p.is<float>()) {
        o.f32(p.get<float>());
    } else if (p.is<web_color>()) {
        write_color(o, p.get<web_color>());
    } else if (p.is<std::vector<image>>()) {
        o.u32(uint32_t(p.get<std::vector<image>>().size()));
        for (const auto& img : p.get<std::vector<image>>()) {
            o.u8(uint8_t(img.type));
            o.str(img.url);
            write_gradient(o, img.m_gradient);
        }
    } else if (p.is<std::string>()) {
        o.str(p.get<std::string>());
    } else if (p.is<string_vector>()) {
        o.u32(uint32_t(p.get<string_vector>().size()));
        for (const auto& v : p.get<string_vector>()) o.str(v);
    } else if (p.is<size_vector>()) {
        o.u32(uint32_t(p.get<size_vector>().size()));
        for (const auto& v : p.get<size_vector>()) {
            write_length(o, v.width);
            write_length(o, v.height);
        }
    } else if (p.is<css_token_vector>()) {
        write_tokens(o, p.get<css_token_vector>());
    }
}

// Every property a style holds, in the map's (id) order, then the name table's size.
void write_style(Out& o, const style& st) {
    std::vector<string_id> held;
    const size_t names = _id_usage().names;
    for (size_t id = 0; id < names; id++)
        if (!st.get_property(string_id(id)).is<invalid>()) held.push_back(string_id(id));
    o.u32(uint32_t(held.size()));
    for (string_id id : held) {
        o.u32(uint32_t(id));
        o.str(_s(id));
        write_property(o, st.get_property(id));
    }
    o.u32(uint32_t(_id_usage().names));
}

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

// ---- Stage L5: the cascade ('l') ----

// The cascade's container: the renderer's answers where the fork asks (fonts with fixed metrics per description, widths per byte, ASCII text transforms, the trusted switch and its sanitize_declaration), its viewport set by the case, every font, width, image and transform call logged; for layout the viewport, image sizes from the source's bytes and text breakers that break between any two characters, logged too.
class CascadeContainer final : public OracleContainer {
public:
    float width = 375, height = 667, dpr = 2;
    bool trusted = false;
    struct Call {
        char kind;
        std::string a, b;
        int64_t n = 0;
    };
    mutable std::vector<Call> calls;
    std::map<uint_ptr, float> sizes;
    uint_ptr next = 0;
    uint64_t breakers = 0;

    // A breaker of a text in a font: every character, by its bytes, as wide as text_width makes a byte, and a break before any of them.
    class Breaker final : public text_breaker {
    public:
        CascadeContainer* owner;
        uint64_t id;
        std::string text;
        float size;
        Breaker(CascadeContainer* o, uint64_t i, std::string t, float s) : owner(o), id(i), text(std::move(t)), size(s) {}
        size_t fit(size_t start, pixel_t width) override {
            std::string at(8, '\0');
            const uint32_t s32 = uint32_t(start);
            const float w = width.value();
            std::memcpy(at.data(), &s32, 4);
            std::memcpy(at.data() + 4, &w, 4);
            owner->calls.push_back({'b', at, {}, int64_t(id)});
            if (start >= text.size()) return start;
            size_t end = start;
            for (size_t i = start; i < text.size();) {
                size_t next = i + 1;
                while (next < text.size() && (static_cast<unsigned char>(text[next]) & 0xC0) == 0x80) next++;
                if (float(next - start) * size * 0.5f > width.value()) {
                    if (end == start) end = next;
                    break;
                }
                end = next;
                i = next;
            }
            return end;
        }
        pixel_t minimum_width() override {
            owner->calls.push_back({'m', {}, {}, int64_t(id)});
            float widest = 0;
            for (size_t i = 0; i < text.size();) {
                size_t next = i + 1;
                while (next < text.size() && (static_cast<unsigned char>(text[next]) & 0xC0) == 0x80) next++;
                widest = std::max(widest, float(next - i) * size * 0.5f);
                i = next;
            }
            return pixel_t(widest);
        }
    };

    std::shared_ptr<text_breaker> create_text_breaker(const std::string& text, uint_ptr font) override {
        calls.push_back({'B', text, {}, int64_t(font)});
        auto it = sizes.find(font);
        return std::make_shared<Breaker>(this, ++breakers, text, it == sizes.end() ? 0.f : it->second);
    }
    void get_viewport(position& viewport) const override {
        calls.push_back({'V', {}, {}, 0});
        viewport = position(0, 0, pixel_t(width), pixel_t(height));
    }
    void get_image_size(const char* src, const char* baseurl, litehtml::size& sz) override {
        const std::string s = src ? src : "";
        calls.push_back({'S', s, baseurl ? baseurl : "", baseurl ? 1 : 0});
        if (s.empty()) return;
        uint32_t sum = 0;
        for (char c : s) sum += static_cast<unsigned char>(c);
        sz.width = pixel_t(float(sum % 200));
        sz.height = pixel_t(float((sum * 7 + uint32_t(s.size())) % 150));
    }

    uint_ptr create_font(const font_description& d, const document*, font_metrics* fm) override {
        calls.push_back({'F', d.hash(), {}, 0});
        const uint_ptr id = ++next;
        sizes[id] = d.size.value();
        if (fm) {
            const float size = d.size.value();
            const float ascent = size * (d.family.size() % 3 == 0 ? 0.75f : 0.8f);
            const float descent = size * 0.25f;
            const float height = size * 1.15f + (d.weight >= 600 ? 1.0f : 0.0f);
            const float x_height = d.style == font_style_italic ? 0.f : ascent * 0.6f;
            const float zero_width = size * 0.5f + float(d.weight) / 1000.f;
            fm->font_size = d.size;
            fm->ascent = pixel_t(ascent);
            fm->descent = pixel_t(descent);
            fm->height = pixel_t(std::max(height, ascent + descent));
            fm->x_height = pixel_t(x_height > 0 ? x_height : ascent * 0.5f);
            fm->ch_width = pixel_t(zero_width);
            fm->draw_spaces = d.decoration_line != text_decoration_line_none;
            fm->sub_shift = pixel_t(size / 5);
            fm->super_shift = pixel_t(size / 3);
        }
        return id;
    }
    pixel_t text_width(const char* text, uint_ptr font) override {
        calls.push_back({'W', text ? text : "", {}, int64_t(font)});
        auto it = sizes.find(font);
        if (it == sizes.end() || !text) return 0;
        return pixel_t(float(std::strlen(text)) * it->second * 0.5f);
    }
    const char* get_default_font_name() const override { return "-apple-system, system-ui, sans-serif"; }
    void load_image(const char* src, const char* baseurl, bool redraw_on_ready) override {
        calls.push_back({'I', src ? src : "", baseurl ? baseurl : "", (baseurl ? 2 : 0) | (redraw_on_ready ? 1 : 0)});
    }
    void transform_text(std::string& text, text_transform tt) override {
        calls.push_back({'T', text, {}, int64_t(tt)});
        switch (tt) {
        case text_transform_uppercase:
            for (auto& c : text) c = char(std::toupper((unsigned char)c));
            break;
        case text_transform_lowercase:
            for (auto& c : text) c = char(std::tolower((unsigned char)c));
            break;
        case text_transform_capitalize: {
            bool start = true;
            for (auto& c : text) {
                if (start) c = char(std::toupper((unsigned char)c));
                start = c == ' ';
            }
            break;
        }
        default: break;
        }
    }
    void trusted_styles(bool t) override { trusted = t; }
    void sanitizer_limit(const char* what) override { calls.push_back({'L', what ? what : "", {}, 0}); }
    bool sanitize_declaration(const std::string& name, const css_token_vector& value) override {
        if (trusted || value.size() != 1 || (name != "position" && name != "display")) return true;
        const std::string keyword = value[0].ident();
        return name == "position" ? keyword != "sticky" : keyword != "list-item" && keyword != "inline-text";
    }
    void get_media_features(media_features& m) const override {
        m.type = media_type_screen;
        m.width = m.device_width = pixel_t(width);
        m.height = m.device_height = pixel_t(height);
        m.color = 8;
        m.color_index = 256;
        m.monochrome = 0;
        m.resolution = pixel_t(96.0f * dpr);
    }
};

// A computed length, shorter than write_length: keyword or value, unit, val(), and a calc's value against two sizes and its text.
void write_len(Out& o, const css_length& len) {
    o.u8(len.is_predefined() ? 1 : 0);
    o.i32(len.predef());
    o.u8(uint8_t(len.units()));
    o.f32(len.val());
    o.u8(len.calc() ? 1 : 0);
    if (len.calc()) {
        o.f32(len.calc_percent(100).value());
        o.f32(len.calc_percent(-37.5f).value());
        o.str(len.to_string());
    }
}

void write_margins(Out& o, const css_margins& m) {
    for (const css_length* l : {&m.left, &m.right, &m.top, &m.bottom}) write_len(o, *l);
}

void write_border(Out& o, const css_border& b) {
    write_len(o, b.width);
    o.i32(int32_t(b.style));
    write_color(o, b.color);
}

void write_track_list(Out& o, const grid_track_list& list) {
    for (const auto* sizes : {&list.tracks, &list.repeat}) {
        o.u32(uint32_t(sizes->size()));
        for (const auto& t : *sizes) {
            o.i32(int32_t(t.min_type));
            o.i32(int32_t(t.max_type));
            write_len(o, t.min_len);
            write_len(o, t.max_len);
            o.f32(t.fr);
        }
    }
    o.i32(list.repeat_at);
    o.u8(list.auto_fit ? 1 : 0);
}

void write_shadows(Out& o, const box_shadow_vector& shadows) {
    o.u32(uint32_t(shadows.size()));
    for (const auto& s : shadows) {
        for (const css_length* len : {&s.x, &s.y, &s.blur, &s.spread}) write_len(o, *len);
        write_color(o, s.color);
        o.u8(s.inset ? 1 : 0);
    }
}

void write_filters(Out& o, const filter_vector& filters) {
    o.u32(uint32_t(filters.size()));
    for (const auto& f : filters) {
        o.i32(int32_t(f.kind));
        o.f32(f.amount);
        for (const css_length* len : {&f.x, &f.y, &f.radius}) write_len(o, *len);
        write_color(o, f.color);
    }
}

void write_metrics(Out& o, const font_metrics& fm) {
    for (pixel_t v : {fm.font_size, fm.height, fm.ascent, fm.descent, fm.x_height, fm.ch_width}) o.f32(v.value());
    o.u8(fm.draw_spaces ? 1 : 0);
    o.f32(fm.sub_shift.value());
    o.f32(fm.super_shift.value());
}

// Every field of a computed style, in css_properties.h's order; the share id as the walk's first element that has it.
void write_css(Out& o, const css_properties& css, int32_t shared) {
    using lhdump_dom::field;
    auto& c = const_cast<css_properties&>(css);
    for (int v : {int(css.get_position()), int(css.get_text_align()), int(css.get_direction()), int(css.get_overflow()), int(css.get_overflow_x()), int(css.get_overflow_y()),
                  int(css.get_white_space()), int(css.get_overflow_wrap()), int(css.get_word_break()), int(css.get_user_select()), int(css.get_display()), int(css.get_visibility()),
                  int(css.get_appearance()), int(css.get_resize()), int(css.get_box_sizing())})
        o.i32(v);
    write_len(o, field<lhdump_dom::CssZIndex>(c));
    for (int v : {int(css.get_vertical_align()), int(css.get_float()), int(css.get_clear())}) o.i32(v);
    write_margins(o, css.get_margins());
    write_margins(o, css.get_padding());
    const css_borders& b = css.get_borders();
    for (const css_border* side : {&b.left, &b.top, &b.right, &b.bottom}) write_border(o, *side);
    const auto& r = b.radius;
    for (const css_length* l : {&r.top_left_x, &r.top_left_y, &r.top_right_x, &r.top_right_y, &r.bottom_right_x, &r.bottom_right_y, &r.bottom_left_x, &r.bottom_left_y}) write_len(o, *l);
    for (const css_length* l : {&css.get_width(), &css.get_cell_width(), &css.get_height(), &css.get_min_width(), &css.get_min_height(), &css.get_max_width(), &css.get_max_height()})
        write_len(o, *l);
    const css_offsets& off = css.get_offsets();
    for (const css_length* l : {&off.left, &off.right, &off.top, &off.bottom}) write_len(o, *l);
    write_len(o, css.get_text_indent());
    write_len(o, field<lhdump_dom::CssLineHeightRaw>(c));
    write_len(o, css.line_height().css_value);
    o.f32(css.line_height().computed_value.value());
    o.i32(int32_t(css.get_list_style_type()));
    o.i32(int32_t(css.get_list_style_position()));
    o.str(css.get_list_style_image());
    o.str(css.get_list_style_image_baseurl());
    const background& bg = css.get_bg();
    o.u32(uint32_t(bg.m_image.size()));
    for (const auto& img : bg.m_image) {
        o.u8(uint8_t(img.type));
        o.str(img.url);
        write_gradient(o, img.m_gradient);
    }
    o.str(bg.m_baseurl);
    write_color(o, bg.m_color);
    o.u32(uint32_t(bg.m_attachment.size()));
    for (int v : bg.m_attachment) o.i32(v);
    for (const auto* list : {&bg.m_position_x, &bg.m_position_y}) {
        o.u32(uint32_t(list->size()));
        for (const auto& l : *list) write_len(o, l);
    }
    o.u32(uint32_t(bg.m_size.size()));
    for (const auto& s : bg.m_size) {
        write_len(o, s.width);
        write_len(o, s.height);
    }
    for (const auto* list : {&bg.m_repeat, &bg.m_clip, &bg.m_origin}) {
        o.u32(uint32_t(list->size()));
        for (int v : *list) o.i32(v);
    }
    o.u64(uint64_t(css.get_font()));
    write_len(o, field<lhdump_dom::CssFontSize>(c));
    o.str(css.font_family());
    write_len(o, css.font_weight());
    o.i32(int32_t(css.font_style_value()));
    o.i32(css.get_text_decoration_line());
    o.i32(int32_t(css.get_text_decoration_style()));
    write_len(o, css.get_text_decoration_thickness());
    write_color(o, css.get_text_decoration_color());
    o.str(css.get_text_emphasis_style());
    write_color(o, css.get_text_emphasis_color());
    o.i32(css.get_text_emphasis_position());
    write_metrics(o, css.get_font_metrics());
    o.i32(shared);
    o.i32(int32_t(css.get_text_transform()));
    o.i32(int32_t(field<lhdump_dom::CssFontSmoothing>(c)));
    write_color(o, css.get_color());
    o.str(css.get_cursor());
    o.str(css.get_content());
    o.i32(int32_t(css.get_border_collapse()));
    o.i32(int32_t(css.get_table_layout()));
    write_len(o, css.get_border_spacing_x());
    write_len(o, css.get_border_spacing_y());
    o.f32(css.get_flex_grow());
    o.f32(css.get_flex_shrink());
    write_len(o, css.get_flex_basis());
    for (int v : {int(css.get_flex_direction()), int(css.get_flex_wrap()), int(css.get_flex_justify_content()), int(css.get_flex_align_items()), int(css.get_flex_align_self()),
                  int(css.get_flex_align_content()), int(css.get_caption_side()), css.get_order()})
        o.i32(v);
    write_len(o, css.get_row_gap());
    write_len(o, css.get_column_gap());
    o.i32(int32_t(css.get_justify_items()));
    o.i32(int32_t(css.get_justify_self()));
    write_track_list(o, css.get_grid_template_columns());
    write_track_list(o, css.get_grid_template_rows());
    write_track_list(o, css.get_grid_auto_rows());
    for (const grid_placement* p : {&css.get_grid_column(), &css.get_grid_row()}) {
        o.i32(p->start);
        o.i32(p->end);
        o.i32(p->span);
    }
    o.f32(css.get_opacity());
    o.f32(css.get_aspect_ratio());
    o.u8(css.get_center_content() ? 1 : 0);
    write_len(o, field<lhdump_dom::CssLetterSpacing>(c));
    write_len(o, field<lhdump_dom::CssWordSpacing>(c));
    o.str(field<lhdump_dom::CssVariantNumeric>(c));
    o.str(field<lhdump_dom::CssFeatureSettings>(c));
    write_shadows(o, css.get_text_shadows());
    write_shadows(o, css.get_box_shadows());
    write_filters(o, css.get_filter());
    write_filters(o, css.get_backdrop_filter());
    const clip_path_shape& clip = css.get_clip_path();
    o.i32(int32_t(clip.kind));
    o.u32(uint32_t(clip.values.size()));
    for (const auto& v : clip.values) write_len(o, v);
    o.u8(clip.evenodd ? 1 : 0);
    o.u32(uint32_t(css.get_transform().size()));
    for (const auto& f : css.get_transform()) {
        o.i32(int32_t(f.kind));
        write_len(o, f.x);
        write_len(o, f.y);
        for (float v : f.v) o.f32(v);
    }
    write_len(o, css.get_transform_origin_x());
    write_len(o, css.get_transform_origin_y());
    o.u8(css.get_transform_fill_box() ? 1 : 0);
    o.i32(field<lhdump_dom::CssBackface>(c));
    o.i32(css.get_contain());
    write_border(o, css.get_outline());
    write_len(o, field<lhdump_dom::CssOutlineOffset>(c));
    write_color(o, css.get_accent_color());
    o.i32(field<lhdump_dom::CssDarkScheme>(c));
    o.u8(field<lhdump_dom::CssKeywordSize>(c) ? 1 : 0);
    o.u8(field<lhdump_dom::CssGenericMono>(c) ? 1 : 0);
}

// A style's properties in id order: u32 count, each u32 id, its name and write_property's value.
void write_style_map(Out& o, const style& st) {
    const auto& props = lhdump_dom::field<lhdump_dom::StyleProperties>(const_cast<style&>(st));
    o.u32(uint32_t(props.size()));
    for (const auto& [name, value] : props) {
        o.u32(uint32_t(name));
        o.str(_s(name));
        write_property(o, value);
    }
}

void write_look(Out& o, const form_look& look) {
    o.u8(look.native ? 1 : 0);
    o.u8(look.dark ? 1 : 0);
    o.u8(look.disabled ? 1 : 0);
}

void cascade_walk(const element::ptr& el, int32_t parent, std::vector<std::pair<element::ptr, int32_t>>& out) {
    const int32_t index = int32_t(out.size());
    out.emplace_back(el, parent);
    for (const auto& child : el->children()) cascade_walk(child, index, out);
}

// The document after a step: every element of the tree from the root (its class, parent, tag, computed style, a tag's matched selectors and styles and counters, and its class's computed state), then the document's fonts, sharing, media lists and generated content.
void write_cascade(Out& o, document& doc) {
    using lhdump_dom::field;
    std::vector<std::pair<element::ptr, int32_t>> nodes;
    if (doc.root()) cascade_walk(doc.root(), -1, nodes);
    std::map<int, int32_t> shared;
    for (size_t k = 0; k < nodes.size(); k++) shared.emplace(nodes[k].first->css().get_shared_id(), int32_t(k));
    o.u32(uint32_t(nodes.size()));
    for (const auto& [el, parent] : nodes) {
        const int kind = lhdump_dom::kind_of(*el);
        o.u8(uint8_t(kind));
        o.i32(parent);
        o.str(_s(el->tag()));
        write_css(o, el->css(), shared[el->css().get_shared_id()]);
        auto* tag = dynamic_cast<html_tag*>(el.get());
        if (tag) {
            const auto& used = field<lhdump_dom::ElUsedStyles>(static_cast<element&>(*tag));
            o.u32(uint32_t(used.size()));
            for (const auto& us : used) {
                o.u64(us->m_selector->place());
                o.u8(us->m_used ? 1 : 0);
            }
            write_style_map(o, field<lhdump_dom::TagStyle>(*tag));
            o.u8(field<lhdump_dom::TagHasStyleBase>(*tag) ? 1 : 0);
            write_style_map(o, field<lhdump_dom::TagStyleBase>(*tag));
            o.i32(field<lhdump_dom::TagCounterChanges>(*tag));
            o.u8(field<lhdump_dom::TagAnonymous>(*tag) ? 1 : 0);
        }
        const auto& counters = field<lhdump_dom::ElCounterValues>(*el);
        o.u32(uint32_t(counters.size()));
        for (const auto& [name, value] : counters) {
            o.str(_s(name));
            o.i32(value);
        }
        if (kind == 25 || kind == 26) {
            auto& text = static_cast<el_text&>(*el);
            o.str(field<lhdump_dom::TextTransformed>(text));
            o.u8(field<lhdump_dom::TextUseTransformed>(text) ? 1 : 0);
            o.u8(field<lhdump_dom::TextDrawSpaces>(text) ? 1 : 0);
            o.f32(field<lhdump_dom::TextSize>(text).width.value());
            o.f32(field<lhdump_dom::TextSize>(text).height.value());
            o.u64(uint64_t(field<lhdump_dom::TextMeasuredFont>(text)));
            o.i32(field<lhdump_dom::TextMeasuredTransform>(text));
            o.u8(el->is_white_space() ? 1 : 0);
            o.u8(el->is_break() ? 1 : 0);
        } else if (kind == 21 || kind == 22) {
            auto& gen = static_cast<el_before_after_base&>(*el);
            o.i32(field<lhdump_dom::GenGenerations>(gen));
            o.i32(field<lhdump_dom::GenBoxes>(gen));
            o.u64(uint64_t(field<lhdump_dom::GenBytes>(gen)));
        } else if (kind == 15) {
            write_look(o, field<lhdump_dom::InputLook>(static_cast<el_input&>(*el)));
        } else if (kind == 16) {
            write_look(o, field<lhdump_dom::SelectLook>(static_cast<el_select&>(*el)));
            o.u8(field<lhdump_dom::SelectArrow>(static_cast<el_select&>(*el)) ? 1 : 0);
        } else if (kind == 17) {
            write_look(o, field<lhdump_dom::ButtonLook>(static_cast<el_button&>(*el)));
        } else if (kind == 18) {
            write_look(o, field<lhdump_dom::TextareaLook>(static_cast<el_textarea&>(*el)));
        } else if (kind == 20) {
            write_look(o, field<lhdump_dom::MeterLook>(static_cast<el_meter&>(*el)));
        } else if (kind == 7) {
            o.u8(field<lhdump_dom::ImageLoaded>(static_cast<el_image&>(*el)) ? 1 : 0);
        }
    }
    const auto& fonts = field<lhdump_dom::DocFonts>(doc);
    o.u32(uint32_t(fonts.size()));
    for (const auto& [key, item] : fonts) {
        o.str(key);
        o.u64(uint64_t(item.font));
        write_metrics(o, item.metrics);
    }
    o.u32(uint32_t(field<lhdump_dom::DocShareCache>(doc).size()));
    const auto& lists = field<lhdump_dom::DocMediaLists>(doc);
    o.u32(uint32_t(lists.size()));
    for (const auto& list : lists) o.u8(list->is_used() ? 1 : 0);
    o.i32(field<lhdump_dom::DocIdsSelectable>(doc));
    o.u8(doc.has_backdrop ? 1 : 0);
    o.i32(field<lhdump_dom::DocGeneratedBoxes>(doc));
    o.u64(uint64_t(field<lhdump_dom::DocGeneratedBytes>(doc)));
    o.u8(doc.generated_limited() ? 1 : 0);
}


// ---- Stage L6: the render tree ('o' with 'r') ----

// The render item classes, as the port numbers them.
int render_kind(const render_item& ri) {
    const std::type_info& t = typeid(ri);
    if (t == typeid(render_item_block)) return 1;
    if (t == typeid(render_item_block_context)) return 2;
    if (t == typeid(render_item_inline_context)) return 3;
    if (t == typeid(render_item_inline)) return 4;
    if (t == typeid(render_text)) return 5;
    if (t == typeid(render_item_image)) return 6;
    if (t == typeid(render_item_table)) return 7;
    if (t == typeid(render_item_table_part)) return 8;
    if (t == typeid(render_item_table_row)) return 9;
    if (t == typeid(render_item_flex)) return 10;
    if (t == typeid(render_item_grid)) return 11;
    if (t == typeid(render_item)) return 0;
    return 255;
}

// The items the document keeps, numbered: the tree from the root in preorder, then each tabular item the tree does not reach with the items under it; the DOM's elements by their walk, the anonymous ones the tree made by first use.
struct TreeIndex {
    std::vector<render_item*> items;
    std::map<const render_item*, int32_t> item_index;
    std::map<const element*, int32_t> dom;
    std::vector<element*> anon;
    std::map<const element*, int32_t> anon_index;

    void visit(render_item* ri) {
        std::vector<render_item*> pending{ri};
        while (!pending.empty()) {
            render_item* cur = pending.back();
            pending.pop_back();
            if (item_index.count(cur)) continue;
            item_index[cur] = int32_t(items.size());
            items.push_back(cur);
            const auto& kids = cur->children();
            for (auto it = kids.rbegin(); it != kids.rend(); ++it) pending.push_back(it->get());
        }
    }
    int32_t of(const render_item* ri) const {
        if (!ri) return -1;
        auto it = item_index.find(ri);
        return it == item_index.end() ? -3 : it->second;
    }
    int32_t of_el(element* el) {
        if (!el) return -1;
        if (auto it = dom.find(el); it != dom.end()) return it->second;
        if (auto it = anon_index.find(el); it != anon_index.end()) return it->second;
        const int32_t k = -2 - int32_t(anon.size());
        anon_index[el] = k;
        anon.push_back(el);
        return k;
    }
};

void write_renders(Out& o, TreeIndex& t, element& el) {
    const auto& renders = lhdump_dom::field<lhdump_dom::ElRenders>(el);
    o.u32(uint32_t(renders.size()));
    for (const auto& weak : renders) o.i32(t.of(weak.lock().get()));
    o.i32(t.of(el.get_render_item().get()));
}

void write_grid(Out& o, TreeIndex& t, render_item_table& table) {
    const auto& grid = lhdump_dom::field<lhdump_dom::TableGridOf>(table);
    o.f32(lhdump_dom::field<lhdump_dom::TableSpacingX>(table).value());
    o.f32(lhdump_dom::field<lhdump_dom::TableSpacingY>(table).value());
    o.u8(grid ? 1 : 0);
    if (!grid) return;
    o.i32(grid->rows_count());
    o.i32(grid->cols_count());
    for (int r = 0; r < grid->rows_count(); r++) {
        const table_row& row = grid->row(r);
        o.i32(t.of(row.el_row.get()));
        for (pixel_t v : {row.height, row.border_top, row.border_bottom, row.top, row.bottom, row.min_height}) o.f32(v.value());
        write_len(o, row.css_height);
    }
    for (int r = 0; r < grid->rows_count(); r++) {
        for (int c = 0; c < grid->cols_count(); c++) {
            const table_cell* cell = grid->cell(c, r);
            o.u8(cell ? 1 : 0);
            if (!cell) continue;
            o.i32(t.of(cell->el.get()));
            o.i32(cell->colspan);
            o.i32(cell->rowspan);
            for (pixel_t v : {cell->min_width, cell->min_height, cell->max_width, cell->max_height, cell->width, cell->height}) o.f32(v.value());
            for (pixel_t v : {cell->borders.left, cell->borders.right, cell->borders.top, cell->borders.bottom}) o.f32(v.value());
        }
    }
    for (int c = 0; c < grid->cols_count(); c++) {
        const table_column& col = grid->column(c);
        for (pixel_t v : {col.min_width, col.max_width, col.width, col.border_left, col.border_right, col.left, col.right}) o.f32(v.value());
        write_len(o, col.css_width);
    }
    o.u32(uint32_t(grid->captions().size()));
    for (const auto& cap : grid->captions()) o.i32(t.of(cap.get()));
    o.f32(grid->top_captions_height().value());
    o.f32(grid->bottom_captions_height().value());
}

// The render tree after a step: every item the document keeps (class, element, parent as its weak pointer has it, children, the constructor's margins, padding and borders, skip, a table's spacing and grid), the root and the tabular list, the table cells taken, then each element of the DOM (its list_index, computed width and render items) and each anonymous element the tree made (its class, parent, tag, computed style, styles, counters and render items).
void write_render_tree(Out& o, document& doc) {
    using lhdump_dom::field;
    TreeIndex t;
    std::vector<std::pair<element::ptr, int32_t>> nodes;
    if (doc.root()) cascade_walk(doc.root(), -1, nodes);
    for (size_t k = 0; k < nodes.size(); k++) t.dom[nodes[k].first.get()] = int32_t(k);
    if (doc.root_render()) t.visit(doc.root_render().get());
    const auto& tabular = field<lhdump_dom::DocTabular>(doc);
    for (const auto& item : tabular) t.visit(item.get());
    o.u32(uint32_t(t.items.size()));
    for (render_item* ri : t.items) {
        o.u8(uint8_t(render_kind(*ri)));
        o.i32(t.of_el(ri->src_el().get()));
        o.i32(t.of(ri->parent().get()));
        o.u32(uint32_t(ri->children().size()));
        for (const auto& child : ri->children()) o.i32(t.of(child.get()));
        for (const margins* m : {&ri->get_margins(), &ri->get_paddings(), &ri->get_borders()})
            for (pixel_t v : {m->left, m->right, m->top, m->bottom}) o.f32(v.value());
        o.u8(ri->skip() ? 1 : 0);
        if (auto* table = dynamic_cast<render_item_table*>(ri)) write_grid(o, t, *table);
    }
    o.i32(t.of(doc.root_render().get()));
    o.u32(uint32_t(tabular.size()));
    for (const auto& item : tabular) o.i32(t.of(item.get()));
    o.i32(field<lhdump_dom::DocTableCells>(doc));
    o.u8(field<lhdump_dom::DocTableLimited>(doc) ? 1 : 0);
    o.u64(field<lhdump_dom::DocRenderSerial>(doc));
    o.u32(uint32_t(nodes.size()));
    for (const auto& [el, parent] : nodes) {
        const char* index = el->get_attr("list_index");
        o.u8(index ? 1 : 0);
        if (index) o.str(index);
        write_len(o, el->css().get_width());
        write_renders(o, t, *el);
    }
    for (size_t k = 0; k < t.anon.size(); k++) {
        element* el = t.anon[k];
        o.u8(1);
        o.u8(uint8_t(lhdump_dom::kind_of(*el)));
        o.i32(t.of_el(el->parent().get()));
        o.str(_s(el->tag()));
        write_css(o, el->css(), el->css().get_shared_id());
        if (auto* tag = dynamic_cast<html_tag*>(el)) {
            o.u32(uint32_t(field<lhdump_dom::ElUsedStyles>(static_cast<element&>(*tag)).size()));
            write_style_map(o, field<lhdump_dom::TagStyle>(*tag));
            o.u8(field<lhdump_dom::TagHasStyleBase>(*tag) ? 1 : 0);
            write_style_map(o, field<lhdump_dom::TagStyleBase>(*tag));
            o.i32(field<lhdump_dom::TagCounterChanges>(*tag));
            o.u8(field<lhdump_dom::TagAnonymous>(*tag) ? 1 : 0);
            o.u32(uint32_t(field<lhdump_dom::TagAttrs>(*tag).size()));
        }
        const auto& counters = field<lhdump_dom::ElCounterValues>(*el);
        o.u32(uint32_t(counters.size()));
        for (const auto& [name, value] : counters) {
            o.str(_s(name));
            o.i32(value);
        }
        write_renders(o, t, *el);
    }
    o.u8(0);
}

// ---- Stages L7 and L8: layout ('Y') ----

void write_typed(Out& o, const containing_block_context::typed_pixel& t) {
    o.f32(t.value.value());
    o.u8(uint8_t(t.type));
}

void write_cb(Out& o, const containing_block_context& cb) {
    for (const auto* t : {&cb.width, &cb.render_width, &cb.min_width, &cb.max_width, &cb.height, &cb.min_height, &cb.max_height}) write_typed(o, *t);
    o.i32(cb.context_idx);
    o.u32(cb.size_mode);
}

template <typename Entry> void write_entry(Out& o, const Entry& e) {
    o.u8(e.valid ? 1 : 0);
    o.u8(e.second_pass ? 1 : 0);
    write_cb(o, e.cb);
    o.f32(e.ret.natural_width.value());
    o.f32(e.ret.min_width.value());
    o.f32(e.outer_height.value());
}

void write_pos(Out& o, const position& p) {
    for (pixel_t v : {p.x, p.y, p.width, p.height}) o.f32(v.value());
}

void write_sides(Out& o, const margins& m) {
    for (pixel_t v : {m.left, m.right, m.top, m.bottom}) o.f32(v.value());
}

// Stage L10: a flex's first and last baselines, then its lines (cross start, main, cross and base sizes, auto margin counts, both baselines with their types, reversed axes, the main gap) and each line's items (the class, the item, base, min and max sizes, main size, factors, scaled shrink factor, frozen, clamp state, order, source order, main auto margins, cross auto margins, alignment).
void write_flex(Out& o, TreeIndex& t, render_item_flex& flex) {
    o.f32(flex.get_first_baseline().value());
    o.f32(flex.get_last_baseline().value());
    const auto& lines = lhdump_dom::field<lhdump_dom::FlexLines>(flex);
    o.u32(uint32_t(lines.size()));
    for (const flex_line& line : lines) {
        for (pixel_t v : {line.cross_start, line.main_size, line.cross_size, line.base_size}) o.f32(v.value());
        o.i32(line.num_auto_margin_main_start);
        o.i32(line.num_auto_margin_main_end);
        for (const baseline* b : {&line.first_baseline, &line.last_baseline}) {
            o.f32(b->value().value());
            o.u8(uint8_t(b->type()));
        }
        o.u8(line.reverse_main ? 1 : 0);
        o.u8(line.reverse_cross ? 1 : 0);
        o.f32(line.main_gap.value());
        o.u32(uint32_t(line.items.size()));
        for (const auto& item : line.items) {
            o.u8(dynamic_cast<const flex_item_row_direction*>(item.get()) ? 1 : 0);
            o.i32(t.of(item->el.get()));
            for (pixel_t v : {item->base_size, item->min_size}) o.f32(v.value());
            o.f32(item->max_size.value().value());
            o.u8(item->max_size.is_default() ? 1 : 0);
            o.f32(item->main_size.value());
            o.i32(item->grow);
            o.i32(item->shrink);
            o.f32(item->scaled_flex_shrink_factor.value());
            o.u8(item->frozen ? 1 : 0);
            o.u8(uint8_t(item->clamp_state));
            o.i32(item->order);
            o.i32(item->src_order);
            for (const def_value<pixel_t>* m : {&item->auto_margin_main_start, &item->auto_margin_main_end}) {
                o.f32(m->value().value());
                o.u8(m->is_default() ? 1 : 0);
            }
            o.u8(item->auto_margin_cross_start ? 1 : 0);
            o.u8(item->auto_margin_cross_end ? 1 : 0);
            o.i32(int32_t(item->align));
        }
    }
}

// Stage L11: the grid passes of the layout steps right before the dump, in order, each the grid, its available width, column gap, definite height and that height, row gap, explicit column and row counts, its columns and rows (the size's types and fr, base, limit, infinite, repeated, occupied, collapsed, content and min-content widths), its items (the item, column, row, spans, min- and max-content widths, automatic minimum, outer width and height), the columns' and rows' starts, the rows' start and spacing, and the widths it reported.
void write_grid_passes(Out& o, TreeIndex& t) {
    const auto& passes = lhdump_grid::trace();
    o.u32(uint32_t(passes.size()));
    for (const auto& pass : passes) {
        o.i32(t.of(pass.grid));
        o.f32(pass.available);
        o.f32(pass.column_gap);
        o.u8(pass.definite_height ? 1 : 0);
        o.f32(pass.container_height);
        o.f32(pass.row_gap);
        o.i32(pass.explicit_columns);
        o.i32(pass.explicit_rows);
        for (const auto* tracks : {&pass.columns, &pass.rows}) {
            o.u32(uint32_t(tracks->size()));
            for (const auto& track : *tracks) {
                o.i32(track.min_type);
                o.i32(track.max_type);
                for (float v : {track.fr, track.base, track.limit}) o.f32(v);
                for (bool b : {track.infinite, track.repeated, track.occupied, track.collapsed}) o.u8(b ? 1 : 0);
                o.f32(track.content);
                o.f32(track.min_content);
            }
        }
        o.u32(uint32_t(pass.cells.size()));
        for (const auto& cell : pass.cells) {
            o.i32(t.of(cell.el));
            for (int v : {cell.col, cell.row, cell.col_span, cell.row_span}) o.i32(v);
            o.f32(cell.min_content);
            o.f32(cell.max_content);
            o.u8(cell.auto_min ? 1 : 0);
            o.f32(cell.width);
            o.f32(cell.height);
        }
        for (const auto* starts : {&pass.column_x, &pass.row_y}) {
            o.u32(uint32_t(starts->size()));
            for (float v : *starts) o.f32(v);
        }
        for (float v : {pass.row_start, pass.row_between, pass.natural_width, pass.min_width}) o.f32(v);
    }
}

// The layout after a step: the document's (a root item, its size, the boxes laid out of the budget, exhausted, positioned boxes, the epoch, relayout roots left, rendered once, the fixed boxes' redraw boxes), then every item as the render tree numbers them (class, element, parent, children, its place, margins, padding, borders, skip, the sides a collapsed table leaves it; its cache and marks; its positioned items in z-order; its scroll view; a sticky box's offset; its placement; an inline's boxes, a block context's vertical-align shift, a text's pieces, an inline context's lines and what it keeps, a flex's baselines and lines with their items, a table's spacing and grid as laid out), the grid passes, then each DOM element's and anonymous element's line height (an image's is its height), first render item and, for a text, whether it keeps a breaker.
void write_layout(Out& o, document& doc) {
    using lhdump_dom::field;
    TreeIndex t;
    std::vector<std::pair<element::ptr, int32_t>> nodes;
    if (doc.root()) cascade_walk(doc.root(), -1, nodes);
    for (size_t k = 0; k < nodes.size(); k++) t.dom[nodes[k].first.get()] = int32_t(k);
    if (doc.root_render()) t.visit(doc.root_render().get());
    const auto& tabular = field<lhdump_dom::DocTabular>(doc);
    for (const auto& item : tabular) t.visit(item.get());
    std::map<const render_item*, std::pair<int32_t, int32_t>> pieces;
    for (size_t k = 0; k < t.items.size(); k++)
        if (auto* text = dynamic_cast<render_text*>(t.items[k]))
            for (size_t f = 0; f < text->fragments.size(); f++) pieces[text->fragments[f].get()] = {int32_t(k), int32_t(f)};
    auto item_ref = [&](const render_item* ri) {
        if (auto it = pieces.find(ri); it != pieces.end()) {
            o.i32(it->second.first);
            o.i32(it->second.second);
        } else {
            o.i32(t.of(ri));
            o.i32(-1);
        }
    };
    o.u8(doc.root_render() ? 1 : 0);
    const litehtml::size& size = field<lhdump_dom::DocSize>(doc);
    o.f32(size.width.value());
    o.f32(size.height.value());
    o.u64(doc.layout_spent());
    o.u8(doc.layout_exhausted() ? 1 : 0);
    o.u8(doc.m_has_positioned ? 1 : 0);
    o.u64(doc.layout_epoch());
    o.u32(uint32_t(doc.m_relayout_roots.size()));
    o.u8(doc.m_layout_after_render ? 1 : 0);
    position::vector fixed_boxes;
    doc.get_fixed_boxes(fixed_boxes);
    o.u32(uint32_t(fixed_boxes.size()));
    for (const auto& b : fixed_boxes) write_pos(o, b);
    o.u32(uint32_t(t.items.size()));
    auto write_inline_boxes = [&](render_item& ri) {
        const auto& boxes = field<lhdump_dom::InlineBoxes>(static_cast<render_item_inline&>(ri));
        o.u32(uint32_t(boxes.size()));
        for (const auto& b : boxes) write_pos(o, b);
    };
    for (size_t k = 0; k < t.items.size(); k++) {
        render_item* ri = t.items[k];
        const int kind = render_kind(*ri);
        o.u8(uint8_t(kind));
        o.i32(t.of_el(ri->src_el().get()));
        o.i32(t.of(ri->parent().get()));
        o.u32(uint32_t(ri->children().size()));
        for (const auto& child : ri->children()) o.i32(t.of(child.get()));
        write_pos(o, ri->pos());
        for (const margins* m : {&ri->get_margins(), &ri->get_paddings(), &ri->get_borders()}) write_sides(o, *m);
        o.u8(ri->skip() ? 1 : 0);
        o.u8(ri->collapse_skip);
        o.u8(field<lhdump_dom::RiLayoutDirty>(*ri) ? 1 : 0);
        o.u8(field<lhdump_dom::RiRelayoutPending>(*ri) ? 1 : 0);
        o.u8(field<lhdump_dom::RiOutOfFlow>(*ri) ? 1 : 0);
        o.u64(field<lhdump_dom::RiLayoutEpoch>(*ri));
        write_entry(o, field<lhdump_dom::RiLastLayout>(*ri));
        write_pos(o, field<lhdump_dom::RiLastPos>(*ri));
        o.f32(field<lhdump_dom::RiLastX>(*ri).value());
        o.f32(field<lhdump_dom::RiLastY>(*ri).value());
        write_sides(o, field<lhdump_dom::RiLastMargins>(*ri));
        write_sides(o, field<lhdump_dom::RiLastPadding>(*ri));
        write_sides(o, field<lhdump_dom::RiLastBorders>(*ri));
        for (const auto& entry : field<lhdump_dom::RiMeasured>(*ri)) write_entry(o, entry);
        o.i32(field<lhdump_dom::RiMeasuredNext>(*ri));
        o.i32(field<lhdump_dom::RiResolvedSafe>(*ri));
        const auto& positioned = field<lhdump_dom::RiPositioned>(*ri);
        o.u32(uint32_t(positioned.size()));
        for (const auto& p : positioned) o.i32(t.of(p.get()));
        const scroll_view* view = ri->get_scroll_view();
        o.u8(view ? 1 : 0);
        if (view) {
            write_pos(o, view->get_viewport());
            o.f32(view->get_scroll_size().width.value());
            o.f32(view->get_scroll_size().height.value());
            o.f32(view->get_left().value());
            o.f32(view->get_top().value());
        }
        if (ri->css().get_position() == element_position_sticky) {
            const position sticky = ri->sticky_offset();
            o.f32(sticky.x.value());
            o.f32(sticky.y.value());
        }
        write_pos(o, ri->get_placement());
        if (kind == 4 || kind == 5) write_inline_boxes(*ri);
        if (kind == 2) o.f32(field<lhdump_dom::BcVaApplied>(static_cast<render_item_block_context&>(*ri)).value());
        if (kind == 5) {
            auto& text = static_cast<render_text&>(*ri);
            o.u8(text.is_fragment ? 1 : 0);
            o.str(text.fragment_text);
            o.i32(text.cut);
            o.u32(uint32_t(text.fragments.size()));
            for (const auto& f : text.fragments) {
                write_pos(o, f->pos());
                for (const margins* m : {&f->get_margins(), &f->get_paddings(), &f->get_borders()}) write_sides(o, *m);
                o.u8(f->skip() ? 1 : 0);
                o.u8(f->is_fragment ? 1 : 0);
                o.str(f->fragment_text);
                o.i32(f->cut);
                o.i32(t.of(f->parent().get()));
                o.u32(uint32_t(f->fragments.size()));
                write_inline_boxes(*f);
            }
        }
        if (kind == 3) {
            auto& ic = static_cast<render_item_inline_context&>(*ri);
            o.f32(field<lhdump_dom::IcTrailing>(ic).value());
            const rendered_width& rendered = field<lhdump_dom::IcRendered>(ic);
            o.f32(rendered.natural_width.value());
            o.f32(rendered.min_width.value());
            o.f32(field<lhdump_dom::IcVaApplied>(ic).value());
            o.u8(field<lhdump_dom::IcCut>(ic) ? 1 : 0);
            const auto& ellipses = field<lhdump_dom::IcEllipses>(ic);
            o.u32(uint32_t(ellipses.size()));
            for (const auto& e : ellipses) write_pos(o, e);
            const auto& texts = field<lhdump_dom::IcFragmented>(ic);
            o.u32(uint32_t(texts.size()));
            for (const auto& text : texts) item_ref(text.get());
            auto& lines = field<lhdump_dom::IcLines>(ic);
            o.u32(uint32_t(lines.size()));
            for (auto& line : lines) {
                for (pixel_t v : {line->top(), line->left(), line->line_right(), line->height(), line->width(), line->baseline()}) o.f32(v.value());
                o.i32(int32_t(field<lhdump_dom::LbTextAlign>(*line)));
                o.u8(field<lhdump_dom::LbRtl>(*line) ? 1 : 0);
                o.f32(line->get_rendered_width().natural_width.value());
                o.f32(line->get_rendered_width().min_width.value());
                const css_line_height_t& lh = field<lhdump_dom::LbDefaultLineHeight>(*line);
                o.f32(lh.computed_value.value());
                o.u8(lh.css_value.is_predefined() ? 1 : 0);
                write_metrics(o, field<lhdump_dom::LbFontMetrics>(*line));
                o.u32(uint32_t(line->items().size()));
                for (auto& item : line->items()) {
                    const auto type = item->get_type();
                    o.u8(uint8_t(type));
                    o.u8(type == line_box_item::type_text_part && typeid(*item) != typeid(line_box_item) ? 1 : 0);
                    item_ref(item->get_el().get());
                    o.f32(field<lhdump_dom::LbiMinWidth>(*item).value());
                    o.f32(item->get_items_top().value());
                    o.f32(item->get_items_bottom().value());
                    if (type != line_box_item::type_text_part) write_pos(o, item->pos());
                    for (pixel_t v : {item->left(), item->right(), item->top(), item->bottom(), item->width(), item->height(), item->get_rendered_min_width(), item->get_rendered_natural_width()})
                        o.f32(v.value());
                }
            }
        }
        if (kind == 10) write_flex(o, t, static_cast<render_item_flex&>(*ri));
        // Stage L12: a table's grid with its rows' heights and places, its cells' widths and its columns' widths and places.
        if (kind == 7) write_grid(o, t, static_cast<render_item_table&>(*ri));
    }
    write_grid_passes(o, t);
    o.u32(uint32_t(nodes.size()));
    for (const auto& [el, parent] : nodes) {
        o.f32(el->css().line_height().computed_value.value());
        o.i32(t.of(el->get_render_item().get()));
        const int kind = lhdump_dom::kind_of(*el);
        if (kind == 25 || kind == 26) o.u8(field<lhdump_dom::TextBreaker>(static_cast<el_text&>(*el)) ? 1 : 0);
    }
    o.u32(uint32_t(t.anon.size()));
    for (element* el : t.anon) {
        o.f32(el->css().line_height().computed_value.value());
        o.i32(t.of(el->get_render_item().get()));
    }
}

// The renderer's layout budget for the document: 100000 boxes and 50 more per element of the DOM.
uint64_t renderer_layout_budget(document& doc) {
    uint64_t elements = 0;
    std::vector<const element*> pending;
    if (auto root = doc.root()) pending.push_back(root.get());
    while (!pending.empty()) {
        const element* el = pending.back();
        pending.pop_back();
        elements++;
        for (const auto& child : el->children()) pending.push_back(child.get());
    }
    return 100000 + 50 * elements;
}

// f32 width, height and dpr, the master and user sheets, the html, then u32 steps (u8 'd' dump (the cascade, with mode 1 the render tree, with mode 2 the layout), 'm' f32 width and height then media_changed, 'h', 'a' or 'x' u32 k for :hover, :active or leaving on the k-th tag then find_styles_changes, 'r' refresh_styles and compute_styles of the root, 'w' f32 width and u8 how (bit 0: the renderer's budget or a u64 that follows; the rest the render_type) then render, answered with its f32): the document as createUpdatableFromString finalizes it with the user sheet, each step's answer and the names the case interned (as one string), then the container's calls by phase (the finalization, then each step).
void cascade(In& in, Out& o, int mode) {
    const size_t names_before = _id_usage().names;
    CascadeContainer container;
    container.width = in.f32();
    container.height = in.f32();
    container.dpr = in.f32();
    const std::string master = in.str();
    const std::string user = in.str();
    const std::string html = in.str();
    auto doc = std::make_shared<document>(&container);
    lhdump_dom::field<lhdump_dom::DocText>(*doc) = html;
    const std::string& text = lhdump_dom::field<lhdump_dom::DocText>(*doc);
    GumboOutput* output = gumbo_parse_with_options(&kGumboDefaultOptions, text.data(), text.size());
    if (output->truncated) {
        container.sanitizer_limit("elements");
        lhdump_dom::field<lhdump_dom::DocBuildLimited>(*doc) = true;
    }
    switch (output->document->v.document.doc_type_quirks_mode) {
    case GUMBO_DOCTYPE_NO_QUIRKS: doc->set_document_mode(no_quirks_mode); break;
    case GUMBO_DOCTYPE_QUIRKS: doc->set_document_mode(quirks_mode); break;
    case GUMBO_DOCTYPE_LIMITED_QUIRKS: doc->set_document_mode(limited_quirks_mode); break;
    }
    lhdump_dom::field<lhdump_dom::DocTrackNodes>(*doc) = true;
    elements_list root_elements;
    ((*doc).*robbed(lhdump_dom::DocCreateNode{}))(output->root, root_elements, true, true);
    lhdump_dom::field<lhdump_dom::DocTrackNodes>(*doc) = false;
    element::ptr root = root_elements.empty() ? nullptr : root_elements.back();
    lhdump_dom::field<lhdump_dom::DocGumbo>(*doc) = output;
    doc->finalize_from_external_root(root, master, user);
    ((*doc).*robbed(lhdump_dom::DocBuildTemplate{}))();
    o.u8(uint8_t(doc->mode()));
    std::vector<size_t> phases{container.calls.size()};
    const uint32_t steps = in.u32();
    uint8_t last_op = 0;
    for (uint32_t s = 0; s < steps && in.at < in.s.size(); s++, phases.push_back(container.calls.size())) {
        const uint8_t op = in.u8();
        // The grid passes a dump writes are those of the layouts right before it.
        if (last_op != 'w') lhdump_grid::trace().clear();
        last_op = op;
        if (op == 'd') {
            if (mode == 2)
                write_layout(o, *doc);
            else if (mode == 1)
                write_render_tree(o, *doc);
            else
                write_cascade(o, *doc);
            continue;
        }
        if (op == 'w') {
            const float width = in.f32();
            const uint8_t how = in.u8();
            doc->set_layout_budget((how & 1) ? in.u64() : renderer_layout_budget(*doc));
            o.f32(doc->render(pixel_t(width), render_type(how >> 1)).value());
            continue;
        }
        if (op == 'm') {
            container.width = in.f32();
            container.height = in.f32();
            o.u8(doc->root() && doc->media_changed() ? 1 : 0);
            continue;
        }
        if (op == 'r') {
            if (doc->root()) {
                doc->root()->refresh_styles();
                doc->root()->compute_styles();
            }
            o.u8(1);
            continue;
        }
        const uint32_t k = in.u32();
        std::vector<std::pair<element::ptr, int32_t>> nodes;
        if (doc->root()) cascade_walk(doc->root(), -1, nodes);
        std::vector<element::ptr> tags;
        for (const auto& [el, parent] : nodes)
            if (dynamic_cast<html_tag*>(el.get())) tags.push_back(el);
        if (tags.empty()) {
            o.u8(0);
            continue;
        }
        const element::ptr& el = tags[k % tags.size()];
        bool changed = op == 'h' ? el->on_mouse_over() : op == 'a' ? el->on_lbutton_down() : el->on_mouse_leave();
        const bool styles = doc->root()->find_styles_changes([](const position&) {});
        o.u8(uint8_t((changed ? 1 : 0) | (styles ? 2 : 0)));
    }
    const size_t names_after = _id_usage().names;
    o.u32(uint32_t(names_after - names_before));
    for (size_t k = names_before; k < names_after; k++) o.str(_s(string_id(k)));
    Out log;
    log.u32(uint32_t(phases.size()));
    for (size_t p = 0; p < phases.size(); p++) {
        const size_t from = p == 0 ? 0 : phases[p - 1];
        log.u32(uint32_t(phases[p] - from));
        for (size_t k = from; k < phases[p]; k++) {
            const auto& call = container.calls[k];
            log.u8(uint8_t(call.kind));
            log.str(call.a);
            log.str(call.b);
            log.u64(uint64_t(call.n));
        }
    }
    Out all;
    all.str(o.s);
    all.s += log.s;
    o.s = std::move(all.s);
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
    case 'o': { // the renderer's rewrite_color_scheme: u8 dark (0 or 1), the text; or, after an 'r', the render tree: as 'l', each dump step the tree (write_render_tree)
        if (!input.empty() && input[0] == 'r') {
            in.u8();
            cascade(in, o, 1);
            answer(1, o.s);
            return true;
        }
        const bool dark = in.u8() != 0;
        answer(1, rewrite_color_scheme(in.rest(), dark));
        return true;
    }
    case 'f': { // a style block through style.cpp: u8 container (0 none, 1 lhdump's, 2 the renderer's, 3 colour names), u8 entry (0 add of text, 1 add of its tokens componentized, 2 add_property of a name and text, 3 that and important), baseurl, the name for 2 and 3, then the text; every property the style holds
        document_container* container = declaration_container(in.u8());
        const uint8_t entry = in.u8();
        const std::string baseurl = in.str();
        const std::string name = entry >= 2 ? in.str() : std::string();
        const std::string text = in.rest();
        style st;
        if (entry == 0) st.add(text, baseurl, container);
        else if (entry == 1) st.add(normalize(text, f_componentize), baseurl, container);
        else st.add_property(_id(name), text, baseurl, entry == 3, container);
        write_style(o, st);
        answer(st.empty() ? 0 : 1, o.s);
        return true;
    }
    case 'y': { // css_effects.cpp on a value: u8 which (0 box-shadow, 1 filter, 2 transform, 3 transform-origin, 4 clip-path, 5 grid track list, 6 grid placement, 7 the fast transform tokens of the text, 8 fold_math), u8 container as for 'f', the text componentized without whitespace
        const uint8_t which = in.u8();
        document_container* container = declaration_container(in.u8());
        const std::string text = in.rest();
        css_token_vector tokens = normalize(text, f_componentize | f_remove_whitespace);
        bool ok = false;
        switch (which) {
        case 0: {
            box_shadow_vector shadows;
            ok = parse_box_shadow(tokens, shadows, container);
            o.u32(uint32_t(shadows.size()));
            for (const auto& s : shadows) {
                for (const css_length* len : {&s.x, &s.y, &s.blur, &s.spread}) write_length(o, *len);
                write_color(o, s.color);
                o.u8(s.inset ? 1 : 0);
            }
            break;
        }
        case 1: {
            filter_vector filters;
            ok = parse_filter(tokens, filters, container);
            o.u32(uint32_t(filters.size()));
            for (const auto& f : filters) {
                o.i32(int32_t(f.kind));
                o.f32(f.amount);
                for (const css_length* len : {&f.x, &f.y, &f.radius}) write_length(o, *len);
                write_color(o, f.color);
            }
            o.f32(filter_reach(filters));
            break;
        }
        case 2: {
            transform_vector functions;
            ok = parse_transform(tokens, functions);
            o.u32(uint32_t(functions.size()));
            for (const auto& f : functions) {
                o.i32(int32_t(f.kind));
                write_length(o, f.x);
                write_length(o, f.y);
                for (float v : f.v) o.f32(v);
            }
            break;
        }
        case 3: {
            css_length x(1, css_units_em), y(2, css_units_em);
            ok = parse_transform_origin(tokens, x, y);
            write_length(o, x);
            write_length(o, y);
            break;
        }
        case 4: {
            clip_path_shape shape;
            ok = parse_clip_path(tokens, shape);
            o.i32(int32_t(shape.kind));
            o.u32(uint32_t(shape.values.size()));
            for (const auto& v : shape.values) write_length(o, v);
            o.u8(shape.evenodd ? 1 : 0);
            break;
        }
        case 5: {
            grid_track_list list;
            ok = parse_grid_track_list(tokens, list);
            for (const auto* sizes : {&list.tracks, &list.repeat}) {
                o.u32(uint32_t(sizes->size()));
                for (const auto& t : *sizes) {
                    o.i32(int32_t(t.min_type));
                    o.i32(int32_t(t.max_type));
                    write_length(o, t.min_len);
                    write_length(o, t.max_len);
                    o.f32(t.fr);
                }
            }
            o.i32(list.repeat_at);
            o.u8(list.auto_fit ? 1 : 0);
            break;
        }
        case 6: {
            grid_placement placement;
            ok = parse_grid_placement(tokens, placement);
            o.i32(placement.start);
            o.i32(placement.end);
            o.i32(placement.span);
            break;
        }
        case 7: {
            css_token_vector fast;
            ok = fast_transform_tokens(text, fast);
            write_tokens(o, fast);
            break;
        }
        case 8: {
            o.u8(has_foldable_math(tokens) ? 1 : 0);
            o.u8(has_viewport_units(tokens) ? 1 : 0);
            o.u8(has_viewport_units(text) ? 1 : 0);
            ok = fold_math(tokens, container);
            write_tokens(o, tokens);
            break;
        }
        default: return false;
        }
        answer(ok ? 1 : 0, o.s);
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
    case 'i': { // the fork's DOM of a text as the renderer's documents are built: lhdump_dom.h
        lhdump_dom::dom<In, Out, OracleContainer>(in, o);
        answer(1, o.s);
        return true;
    }
    case 'I': { // the declarations parse_attributes adds to each tag's style, against the port's: lhdump_dom.h
        lhdump_dom::replay<In, Out, OracleContainer>(in, o, [](In& in) { return read_token(in, 0); });
        answer(1, o.s);
        return true;
    }
    case 'l': { // the cascade: a document finalized under a viewport with master and user sheets, then steps (media changes, :hover and :active flips, refreshes) and every element's computed style after each dump step: cascade() above
        cascade(in, o, 0);
        answer(1, o.s);
        return true;
    }
    case 'Y': { // layout: as 'l', with layout steps ('w'), and each dump step the layout (write_layout)
        cascade(in, o, 2);
        answer(1, o.s);
        return true;
    }
    case 'j': { // Gumbo's parse of a text: lhdump_gumbo.h
        lhdump_gumbo::parse(in, o);
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
