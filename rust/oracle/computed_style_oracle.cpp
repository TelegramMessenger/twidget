// core/src/computed_style.cpp on stdin for rust/crates/computed_style: records of a u8 op, a u32 length and fields; crates/computed_style/tests/oracle.rs lays out each.
#include "computed_style.cpp"

#include <litehtml/document.h>
#include <litehtml/document_container.h>
#include <litehtml/css_parser.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace litehtml;

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
    void f32(float v) { raw(v); }
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
};

// The keyword tables a length's from_token may take, by number.
css_values keywords(uint8_t which) {
    switch (which) {
    case 1: return css_values(background_size_strings);
    case 2: return css_values(flex_basis_strings);
    case 3: return css_values(font_size_strings);
    case 4: return css_values(line_height_strings);
    default: return {};
    }
}

// The tables Keyword reads, by number.
css_values table(uint8_t which) {
    switch (which) {
    case 0: return css_values(style_display_strings);
    case 1: return css_values(css_units_strings);
    case 2: return css_values(border_style_strings);
    case 3: return css_values(flex_align_items_strings);
    case 4: return css_values(overflow_strings);
    default: return css_values(background_size_strings);
    }
}

// A document's container: fonts 16 px with fixed metrics, 8 px a byte of text, a 375 x 667 screen.
class Container final : public document_container {
public:
    uint_ptr create_font(const font_description& d, const document*, font_metrics* fm) override {
        if (fm) {
            *fm = font_metrics();
            fm->font_size = d.size;
            fm->ascent = d.size * 0.75f;
            fm->descent = d.size * 0.25f;
            fm->height = d.size * 1.25f;
            fm->x_height = d.size * 0.5f;
            fm->ch_width = d.size * 0.5f;
        }
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
    void get_image_size(const char*, const char*, litehtml::size& sz) override { sz = litehtml::size(20, 10); }
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
    void import_css(std::string& text, const std::string&, std::string&) override { text.clear(); }
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

void write_color(Out& o, const web_color& c) {
    o.u8(c.red);
    o.u8(c.green);
    o.u8(c.blue);
    o.u8(c.alpha);
}

void write_len(Out& o, const css_length& l) {
    o.u8(l.is_predefined());
    o.i32(l.predef());
    o.i32(int(l.units()));
    o.f32(l.val());
}

void write_border(Out& o, const css_border& b) {
    o.i32(int(b.style));
    write_len(o, b.width);
    write_color(o, b.color);
}

void write_px(Out& o, pixel_t v) {
    o.f32(float(v));
}

const char* RAW_NAMES[] = {"touch-action", "font-variant-numeric", "filter", "backdrop-filter", "box-shadow", "translate", "rotate", "scale", "transform", "fill", "stroke"};

// Everything computed_style reads of an element, in crates/computed_style's Element order.
void write_element(Out& o, const element::ptr& el, int depth) {
    auto tag = std::dynamic_pointer_cast<html_tag>(el);
    const css_properties& css = el->css();
    o.u8(tag != nullptr);
    for (int v : {int(css.get_display()), int(css.get_position()), int(css.get_float()), int(css.get_clear()), int(css.get_box_sizing()), int(css.get_visibility()), css.get_z_index(),
                  int(css.get_direction())})
        o.i32(v);
    const css_borders& b = css.get_borders();
    for (const css_border* side : {&b.top, &b.right, &b.bottom, &b.left}) write_border(o, *side);
    for (const css_length* r : {&b.radius.top_left_x, &b.radius.top_left_y, &b.radius.top_right_x, &b.radius.top_right_y, &b.radius.bottom_right_x, &b.radius.bottom_right_y, &b.radius.bottom_left_x,
                                &b.radius.bottom_left_y})
        write_len(o, *r);
    const auto& m = css.get_margins();
    for (const css_length* l : {&m.top, &m.right, &m.bottom, &m.left}) write_len(o, *l);
    const auto& p = css.get_padding();
    for (const css_length* l : {&p.top, &p.right, &p.bottom, &p.left}) write_len(o, *l);
    write_len(o, css.get_width());
    write_len(o, css.get_height());
    o.i32(int(css.get_border_collapse()));
    write_len(o, css.get_border_spacing_x());
    write_len(o, css.get_border_spacing_y());
    o.i32(int(css.get_caption_side()));
    write_len(o, css.get_min_width());
    write_len(o, css.get_min_height());
    write_len(o, css.get_max_width());
    write_len(o, css.get_max_height());
    const auto& off = css.get_offsets();
    for (const css_length* l : {&off.top, &off.right, &off.bottom, &off.left}) write_len(o, *l);
    for (int v : {int(css.get_overflow_x()), int(css.get_overflow_y()), int(css.get_white_space()), int(css.get_overflow_wrap()), int(css.get_word_break())}) o.i32(v);
    write_color(o, css.get_color());
    write_px(o, css.get_font_size());
    o.f32(css.font_weight().val());
    o.i32(int(css.font_style_value()));
    o.str(css.font_family());
    o.u8(css.line_height().css_value.is_predefined());
    write_px(o, css.line_height().computed_value);
    o.i32(int(css.get_text_align()));
    write_len(o, css.get_text_indent());
    o.i32(int(css.get_text_transform()));
    write_px(o, css.get_letter_spacing());
    write_px(o, css.get_word_spacing());
    o.i32(int(css.get_vertical_align()));
    o.i32(css.get_text_decoration_line());
    o.i32(int(css.get_text_decoration_style()));
    write_color(o, css.get_text_decoration_color());
    o.i32(int(css.get_user_select()));
    o.i32(int(css.get_list_style_type()));
    o.i32(int(css.get_list_style_position()));
    o.str(css.get_list_style_image());
    const background& bg = css.get_bg();
    write_color(o, bg.m_color);
    o.u32(uint32_t(bg.m_image.size()));
    for (const image& im : bg.m_image) {
        o.i32(int(im.type));
        o.str(im.url);
        o.str(im.type == image::type_gradient ? _s(im.m_gradient.m_type) : std::string());
    }
    o.u32(uint32_t(bg.m_size.size()));
    for (const css_size& s : bg.m_size) {
        write_len(o, s.width);
        write_len(o, s.height);
    }
    o.u32(uint32_t(bg.m_position_x.size()));
    for (const css_length& l : bg.m_position_x) write_len(o, l);
    o.u32(uint32_t(bg.m_position_y.size()));
    for (const css_length& l : bg.m_position_y) write_len(o, l);
    o.u32(uint32_t(bg.m_repeat.size()));
    for (int r : bg.m_repeat) o.i32(r);
    o.f32(css.get_opacity());
    o.str(css.get_cursor());
    write_len(o, css.get_transform_origin_x());
    write_len(o, css.get_transform_origin_y());
    write_border(o, css.get_outline());
    write_px(o, css.get_outline_offset());
    for (int v : {int(css.get_flex_direction()), int(css.get_flex_wrap()), int(css.get_flex_justify_content()), int(css.get_flex_align_items()), int(css.get_flex_align_self()),
                  int(css.get_flex_align_content())})
        o.i32(v);
    o.f32(css.get_flex_grow());
    o.f32(css.get_flex_shrink());
    write_len(o, css.get_flex_basis());
    o.i32(css.get_order());
    write_len(o, css.get_row_gap());
    write_len(o, css.get_column_gap());
    o.f32(css.get_aspect_ratio());
    o.str(css.get_content());
    auto ri = el->get_render_item();
    o.u8(ri != nullptr);
    if (ri) {
        const auto& rm = ri->get_margins();
        const auto& rp = ri->get_paddings();
        const auto& rb = ri->get_borders();
        for (pixel_t v : {rm.top, rm.right, rm.bottom, rm.left, rp.top, rp.right, rp.bottom, rp.left, rb.top, rb.right, rb.bottom, rb.left}) write_px(o, v);
    }
    const position placement = el->get_placement();
    write_px(o, placement.width);
    write_px(o, placement.height);
    for (const char* name : RAW_NAMES) o.str(tag ? tag->raw_declaration(name) : std::string());
    std::vector<element::ptr> chain;
    for (auto e = el; e; e = e->parent()) chain.push_back(e);
    o.u32(uint32_t(chain.size()));
    for (const auto& e : chain) {
        auto t = std::dynamic_pointer_cast<html_tag>(e);
        o.u8(t != nullptr);
        o.str(t ? t->raw_declaration("pointer-events") : std::string());
        o.str(t ? t->raw_declaration("quotes") : std::string());
    }
    for (bool fill : {true, false}) {
        web_color painted;
        const bool ok = svg_paint_color(el, fill, painted, el->get_document()->container());
        o.u8(ok);
        write_color(o, painted);
    }
    element::ptr before, after;
    if (tag && depth == 0)
        for (const auto& child : tag->children()) {
            if (!before && std::dynamic_pointer_cast<el_before>(child)) before = child;
            if (!after && std::dynamic_pointer_cast<el_after>(child)) after = child;
        }
    for (const element::ptr& pseudo : {before, after}) {
        o.u8(pseudo != nullptr);
        if (pseudo) write_element(o, pseudo, depth + 1);
    }
}

void collect(const element::ptr& el, std::vector<element::ptr>& out, size_t cap) {
    if (out.size() >= cap) return;
    out.push_back(el);
    for (const auto& child : el->children()) collect(child, out, cap);
}

const char* PSEUDOS[] = {"::before", ":after", "::marker", ":before", "::after"};

// The animated values an element gets: every third one, from its index, with a later one winning.
std::vector<std::pair<std::string, std::string>> animated_for(size_t index) {
    if (index % 3 != 1) return {};
    return {{"filter", "blur(1px)"}, {"transform", " rotate(45deg) "}, {"filter", "brightness(2)"}, {"fill", "#f00"}, {"box-shadow", "0 0 3px red"}};
}

bool run(int op, std::string_view input) {
    In in{input};
    Out o;
    switch (op) {
    case 'p': {
        // 'p' f64: px of it.
        answer(1, twlite::px(in.f64()));
        return true;
    }
    case 'c': {
        // 'c' r, g, b, a: color of them.
        const uint8_t r = in.u8(), g = in.u8(), b = in.u8(), a = in.u8();
        answer(1, twlite::color(web_color(r, g, b, a)));
        return true;
    }
    case 'k': {
        // 'k' u8 table, i32 index: Keyword.
        const css_values names = table(in.u8());
        answer(1, twlite::Keyword(names, in.i32()));
        return true;
    }
    case 'l': {
        // 'l' f64 font size, str predefined, i32 options, u8 keywords, then the text: length() of the single token from_token takes, status 0 when it takes none.
        const double font_size = in.f64();
        const std::string predefined = in.str();
        const int options = in.i32();
        const css_values words = keywords(in.u8());
        const css_token_vector tokens = normalize(in.rest(), f_componentize | f_remove_whitespace);
        css_length len;
        if (tokens.size() != 1 || !len.from_token(tokens[0], options, words)) {
            answer(0, {});
            return true;
        }
        answer(1, twlite::length(len, font_size, predefined.c_str()));
        return true;
    }
    case 'a': {
        // 'a' i32: Alignment.
        answer(1, twlite::Alignment(flex_align_items(in.i32())));
        return true;
    }
    case 'b': {
        // 'b' four str: box_shorthand.
        const std::string t = in.str(), r = in.str(), b = in.str(), l = in.str();
        answer(1, twlite::box_shorthand(t, r, b, l));
        return true;
    }
    case 'd': {
        // 'd' i32: text_decoration_line.
        answer(1, twlite::text_decoration_line(in.i32()));
        return true;
    }
    case 'v': {
        // 'v' the text: computed_custom_property's join of it normalized as a style keeps a custom property.
        const css_token_vector tokens = normalize(in.rest(), f_componentize | f_remove_whitespace);
        std::string out;
        for (size_t i = 0; i < tokens.size(); i++) {
            if (i && tokens[i - 1].type < 0 && tokens[i].type < 0) out += ' ';
            out += tokens[i].get_repr(false);
        }
        answer(1, twlite::css::trim(out));
        return true;
    }
    case 'S': {
        // 'S' i32 width, then the html: per element (at most 300, document order) its inputs as write_element writes them, then computed_style of it and of each of PSEUDOS, touch-action and pointer-events.
        static Container container;
        const int width = in.i32();
        const document::ptr doc = document::createFromString(in.rest(), &container);
        if (!doc->root()) {
            answer(0, {});
            return true;
        }
        doc->render(pixel_t(float(width)));
        std::vector<element::ptr> nodes;
        collect(doc->root(), nodes, 300);
        o.u32(uint32_t(nodes.size()));
        for (size_t i = 0; i < nodes.size(); i++) {
            Out inputs;
            write_element(inputs, nodes[i], 0);
            o.str(inputs.s);
            const auto animated = animated_for(i);
            twlite::ComputedExtras extras;
            if (!animated.empty()) extras.animated = &animated;
            o.str(twlite::computed_style(nodes[i], "", extras).dump());
            for (const char* pseudo : PSEUDOS) o.str(twlite::computed_style(nodes[i], pseudo, extras).dump());
            o.str(twlite::computed_touch_action(nodes[i]));
            o.str(twlite::computed_pointer_events(nodes[i]));
        }
        answer(1, o.s);
        return true;
    }
    default:
        return false;
    }
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
        if (!run(op, bytes)) return 3;
    }
    std::fflush(stdout);
    return 0;
}
