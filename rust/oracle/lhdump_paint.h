// lhdump's 'Q' record: every paint call a container gets with its arguments, and what painting keeps (rust/crates/litehtml/tests/common/paint.rs lays them out).
#pragma once

#include "document.h"
#include "document_container.h"
#include "el_form.h"
#include "lhdump_dom.h"
#include "render_item.h"

#include <cstdint>
#include <cstring>
#include <functional>
#include <map>
#include <string>
#include <vector>

namespace lhdump_dom {
#define LHDUMP_ROB_P(Name, Member) \
    struct Name {}; \
    auto robbed(Name); \
    template struct Rob<&Member, Name>;
LHDUMP_ROB_P(RiInk, render_item::m_ink)
LHDUMP_ROB_P(RiDrawnInk, render_item::m_drawn_ink)
LHDUMP_ROB_P(RiInkGeneration, render_item::m_ink_generation)
LHDUMP_ROB_P(InputLookP, el_input::m_look)
LHDUMP_ROB_P(SelectLookP, el_select::m_look)
LHDUMP_ROB_P(ButtonLookP, el_button::m_look)
LHDUMP_ROB_P(TextareaLookP, el_textarea::m_look)
LHDUMP_ROB_P(MeterLookP, el_meter::m_look)
#undef LHDUMP_ROB_P
} // namespace lhdump_dom

namespace lhdump_paint {

using namespace litehtml;

// A paint call's arguments as bytes, every float as its bits.
struct Bytes {
    std::string s;

    template <typename T> Bytes& raw(T v) {
        s.append(reinterpret_cast<const char*>(&v), sizeof v);
        return *this;
    }
    Bytes& u8(uint8_t v) { return raw(v); }
    Bytes& i32(int32_t v) { return raw(v); }
    Bytes& u32(uint32_t v) { return raw(v); }
    Bytes& u64(uint64_t v) { return raw(v); }
    Bytes& f32(float v) { return raw(v); }
    Bytes& str(const std::string& v) {
        u32(uint32_t(v.size()));
        s.append(v);
        return *this;
    }
    Bytes& color(const web_color& c) {
        for (byte v : {c.red, c.green, c.blue, c.alpha}) u8(v);
        return u8(c.is_current_color ? 1 : 0);
    }
    Bytes& pos(const position& p) {
        for (pixel_t v : {p.x, p.y, p.width, p.height}) f32(v.value());
        return *this;
    }
    Bytes& radius(const border_radiuses& r) {
        for (pixel_t v : {r.top_left_x, r.top_left_y, r.top_right_x, r.top_right_y, r.bottom_right_x, r.bottom_right_y, r.bottom_left_x, r.bottom_left_y})
            f32(v.value());
        return *this;
    }
    Bytes& layer(const background_layer& l) {
        pos(l.border_box);
        radius(l.border_radius);
        pos(l.clip_box);
        pos(l.origin_box);
        i32(int32_t(l.attachment));
        i32(int32_t(l.repeat));
        return u8(l.is_root ? 1 : 0);
    }
    Bytes& points(const background_layer::gradient_base& g) {
        u32(uint32_t(g.color_points.size()));
        for (const auto& p : g.color_points) {
            f32(p.offset);
            color(p.color);
            u8(p.hint ? 1 : 0);
            f32(p.hint ? *p.hint : 0.f);
        }
        i32(int32_t(g.color_space));
        return i32(int32_t(g.hue_interpolation));
    }
    Bytes& borders(const litehtml::borders& b) {
        for (const border* side : {&b.left, &b.top, &b.right, &b.bottom}) {
            f32(side->width.value());
            i32(int32_t(side->style));
            color(side->color);
        }
        return radius(b.radius);
    }
    Bytes& length(const css_length& len) {
        u8(len.is_predefined() ? 1 : 0);
        i32(len.predef());
        u8(uint8_t(len.units()));
        return f32(len.val());
    }
    Bytes& svg_gradient(const litehtml::svg_gradient& g) {
        i32(g.type);
        for (float v : {g.x1, g.y1, g.x2, g.y2, g.r, g.fx, g.fy}) f32(v);
        u32(uint32_t(g.stops.size()));
        for (const auto& [offset, c] : g.stops) {
            f32(offset);
            color(c);
        }
        return *this;
    }
    Bytes& path(const vector_path& p) {
        u32(uint32_t(p.ops.size()));
        for (auto op : p.ops) u8(uint8_t(op));
        u32(uint32_t(p.points.size()));
        for (float v : p.points) f32(v);
        u8(p.fill ? 1 : 0);
        color(p.fill_color);
        u8(p.evenodd ? 1 : 0);
        u8(p.stroke ? 1 : 0);
        color(p.stroke_color);
        f32(p.stroke_width);
        i32(p.line_cap);
        i32(p.line_join);
        f32(p.miter_limit);
        u32(uint32_t(p.dashes.size()));
        for (float v : p.dashes) f32(v);
        f32(p.dash_offset);
        svg_gradient(p.fill_gradient);
        return svg_gradient(p.stroke_gradient);
    }
};

// The paint calls of a container, each logged through log() (kind, text, arguments, the hdc); a filtered or isolated paint draws into hdc * 2 + 1.
template <class Base> class Painting : public Base {
public:
    bool active = false;
    web_color accent = web_color(0, 122, 255);
    const std::map<const element*, int32_t>* owners = nullptr;

    virtual void log(char kind, const std::string& a, const std::string& b, int64_t n) const = 0;

    int32_t owner_of(const element* el) const {
        if (!el) return -1;
        if (owners)
            if (auto it = owners->find(el); it != owners->end()) return it->second;
        return -2;
    }
    Bytes& filter(Bytes& b, const filter_paint& f) const {
        b.u32(f.steps ? uint32_t(f.steps->size()) : 0);
        if (f.steps)
            for (const auto& step : *f.steps) {
                b.u8(uint8_t(step.kind));
                b.f32(step.amount);
                b.length(step.x);
                b.length(step.y);
                b.length(step.radius);
                b.color(step.color);
            }
        b.pos(f.bounds);
        b.pos(f.box);
        b.radius(f.radius);
        return b.i32(owner_of(static_cast<const element*>(f.owner)));
    }

    void draw_text(uint_ptr hdc, const char* text, uint_ptr font, web_color color, const position& pos) override {
        Bytes b;
        b.u64(uint64_t(font)).color(color).pos(pos);
        log('t', text ? text : "", b.s, int64_t(hdc));
    }
    void draw_list_marker(uint_ptr hdc, const list_marker& m) override {
        Bytes b;
        b.u8(m.baseurl ? 1 : 0).str(m.baseurl ? m.baseurl : "").i32(int32_t(m.marker_type)).color(m.color).pos(m.pos).i32(m.index).u64(uint64_t(m.font));
        log('k', m.image, b.s, int64_t(hdc));
    }
    void draw_image(uint_ptr hdc, const background_layer& layer, const std::string& url, const std::string& base_url) override {
        Bytes b;
        b.layer(layer).str(base_url);
        log('g', url, b.s, int64_t(hdc));
    }
    void draw_solid_fill(uint_ptr hdc, const background_layer& layer, const web_color& color) override {
        Bytes b;
        b.layer(layer).color(color);
        log('f', {}, b.s, int64_t(hdc));
    }
    void draw_linear_gradient(uint_ptr hdc, const background_layer& layer, const background_layer::linear_gradient& g) override {
        Bytes b;
        b.layer(layer).points(g).f32(g.start.x).f32(g.start.y).f32(g.end.x).f32(g.end.y);
        log('l', {}, b.s, int64_t(hdc));
    }
    void draw_radial_gradient(uint_ptr hdc, const background_layer& layer, const background_layer::radial_gradient& g) override {
        Bytes b;
        b.layer(layer).points(g).f32(g.position.x).f32(g.position.y).f32(g.radius.x).f32(g.radius.y);
        log('r', {}, b.s, int64_t(hdc));
    }
    void draw_conic_gradient(uint_ptr hdc, const background_layer& layer, const background_layer::conic_gradient& g) override {
        Bytes b;
        b.layer(layer).points(g).f32(g.position.x).f32(g.position.y).f32(g.angle).f32(g.radius);
        log('c', {}, b.s, int64_t(hdc));
    }
    void draw_borders(uint_ptr hdc, const litehtml::borders& borders, const position& draw_pos, bool root) override {
        Bytes b;
        b.borders(borders).pos(draw_pos).u8(root ? 1 : 0);
        log('d', {}, b.s, int64_t(hdc));
    }
    void set_clip(const position& pos, const border_radiuses& radius) override {
        Bytes b;
        b.pos(pos).radius(radius);
        log('C', {}, b.s, 0);
    }
    void del_clip() override { log('D', {}, {}, 0); }
    void suspend_clips() override { log('U', {}, {}, 0); }
    void resume_clips() override { log('R', {}, {}, 0); }
    void draw_box_shadow(uint_ptr hdc, const position& box, const border_radiuses& radius, const shadow_paint& shadow) override {
        Bytes b;
        b.pos(box).radius(radius);
        for (pixel_t v : {shadow.x, shadow.y, shadow.blur, shadow.spread}) b.f32(v.value());
        b.color(shadow.color).u8(shadow.inset ? 1 : 0);
        log('h', {}, b.s, int64_t(hdc));
    }
    void set_clip_path(const vector_path& path) override {
        Bytes b;
        b.path(path);
        log('P', {}, b.s, 0);
    }
    void begin_group(uint_ptr hdc, const paint_group& g) override {
        Bytes b;
        b.f32(g.opacity);
        for (float v : {g.matrix.a, g.matrix.b, g.matrix.c, g.matrix.d, g.matrix.tx, g.matrix.ty}) b.f32(v);
        b.u8(g.backface_hidden ? 1 : 0).pos(g.bounds).u8(g.exact_bounds ? 1 : 0).i32(owner_of(g.owner));
        log('G', {}, b.s, int64_t(hdc));
    }
    void draw_filtered(uint_ptr hdc, const filter_paint& f, const std::function<void(uint_ptr)>& paint) override {
        Bytes b;
        log('X', {}, filter(b, f).s, int64_t(hdc));
        paint(hdc * 2 + 1);
        log('x', {}, {}, int64_t(hdc));
    }
    void draw_backdrop(uint_ptr hdc, const filter_paint& f) override {
        Bytes b;
        log('K', {}, filter(b, f).s, int64_t(hdc));
    }
    void draw_path(uint_ptr hdc, const vector_path& path) override {
        Bytes b;
        b.path(path);
        log('p', {}, b.s, int64_t(hdc));
    }
    void end_group(uint_ptr hdc) override { log('E', {}, {}, int64_t(hdc)); }
    bool draw_cached(uint_ptr hdc, const position& box, uint64_t key, const std::function<void(uint_ptr)>&) override {
        Bytes b;
        b.pos(box).u64(key);
        log('A', {}, b.s, int64_t(hdc));
        return false;
    }
    void draw_isolated(uint_ptr hdc, const position& box, const std::function<void(uint_ptr)>& paint) override {
        Bytes b;
        b.pos(box);
        log('O', {}, b.s, int64_t(hdc));
        paint(hdc * 2 + 1);
        log('o', {}, {}, int64_t(hdc));
    }
    bool controls_active() const override {
        log('Q', {}, {}, 0);
        return active;
    }
    web_color control_accent() const override {
        log('q', {}, {}, 0);
        return accent;
    }
};

template <class Out> void write_paint_ink(Out& o, const paint_ink& ink) {
    for (float v : {ink.l, ink.t, ink.r, ink.b}) o.f32(v);
    o.u8(ink.empty ? 1 : 0);
    o.u8(ink.unbounded ? 1 : 0);
}

template <class Out> void write_paint_look(Out& o, const form_look& look) {
    o.u8(look.active ? 1 : 0);
    for (byte v : {look.accent.red, look.accent.green, look.accent.blue, look.accent.alpha}) o.u8(v);
}

// What painting keeps, as the 'Q' record's dump step writes it: the paint generation and culling, every item's ink cache and visual transform (in the numbering given), each control's look as its last paint left it.
template <class Out> void write_paint_state(Out& o, const document& doc, const std::vector<render_item*>& items, const std::vector<element*>& nodes) {
    using lhdump_dom::field;
    o.u32(doc.paint_generation());
    o.u8(doc.culls_paint() ? 1 : 0);
    o.u32(uint32_t(items.size()));
    for (render_item* ri : items) {
        o.u32(field<lhdump_dom::RiInkGeneration>(*ri));
        lhdump_paint::write_paint_ink(o, field<lhdump_dom::RiInk>(*ri));
        lhdump_paint::write_paint_ink(o, field<lhdump_dom::RiDrawnInk>(*ri));
        const transform_matrix m = ri->get_visual_transform();
        for (float v : {m.a, m.b, m.c, m.d, m.tx, m.ty}) o.f32(v);
    }
    o.u32(uint32_t(nodes.size()));
    for (element* el : nodes) {
        const int kind = lhdump_dom::kind_of(*el);
        o.u8(uint8_t(kind));
        if (kind == 15) lhdump_paint::write_paint_look(o, field<lhdump_dom::InputLookP>(static_cast<el_input&>(*el)));
        if (kind == 16) lhdump_paint::write_paint_look(o, field<lhdump_dom::SelectLookP>(static_cast<el_select&>(*el)));
        if (kind == 17) lhdump_paint::write_paint_look(o, field<lhdump_dom::ButtonLookP>(static_cast<el_button&>(*el)));
        if (kind == 18) lhdump_paint::write_paint_look(o, field<lhdump_dom::TextareaLookP>(static_cast<el_textarea&>(*el)));
        if (kind == 20) lhdump_paint::write_paint_look(o, field<lhdump_dom::MeterLookP>(static_cast<el_meter&>(*el)));
    }
}

} // namespace lhdump_paint
