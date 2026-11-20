// svg_oracle: el_svg.cpp's geometry on stdin for rust/crates/litehtml (stage L14); rust/crates/litehtml/tests/common/svg.rs lays out every record and answer.
#include "el_svg.cpp"

#include "document.h"
#include "document_container.h"
#include "html_tag.h"

#include <cstdint>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <limits>
#include <map>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace litehtml;

namespace {

// The painter's private members, reached by explicit instantiation (which may name any member) without changing the fork.
template <auto Member, typename Tag> struct Rob {
    friend auto robbed(Tag) { return Member; }
};

#define SVG_ROB(Name, Member) \
    struct Name {}; \
    auto robbed(Name); \
    template struct Rob<&Member, Name>;

SVG_ROB(PainterDrawUse, litehtml::svg_painter::draw_use)
SVG_ROB(PainterUseDepth, litehtml::svg_painter::m_use_depth)
SVG_ROB(PainterView, litehtml::svg_painter::m_view)
SVG_ROB(SvgViewBox, litehtml::el_svg::view_box)

#undef SVG_ROB

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
    float f32() { return raw<float>(); }
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
    // Every NaN as one: which NaN an operation gives where two meet is the compiler's choice (rust/PLAN.md, Findings).
    void f32(float v) { raw(std::isnan(v) ? std::numeric_limits<float>::quiet_NaN() : v); }
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
};

// What begin_group throws while a <use>'s placement is asked: the group's matrix, before the painter draws the target.
struct UseGroup {
    transform_matrix matrix;
};

// A document's container: fonts 16 px with fixed metrics, 8 px a byte of text, a 375 x 667 screen; begin_group throws when armed.
class Container final : public document_container {
public:
    bool armed = false;

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
    void begin_group(uint_ptr, const paint_group& group) override {
        if (armed) throw UseGroup{group.matrix};
    }
};

void write_matrix(Out& o, const transform_matrix& m) {
    for (float v : {m.a, m.b, m.c, m.d, m.tx, m.ty}) o.f32(v);
}

void write_path(Out& o, const vector_path& path) {
    o.u32(uint32_t(path.ops.size()));
    for (auto op : path.ops) o.u8(uint8_t(op));
    o.u32(uint32_t(path.points.size()));
    for (float v : path.points) o.f32(v);
}

// The point svg_path_length writes is marked unwritten by these bits.
constexpr uint32_t UNWRITTEN = 0x7fc12345u;

void write_length_at(Out& o, const vector_path& path, float at) {
    float x, y;
    std::memcpy(&x, &UNWRITTEN, 4);
    std::memcpy(&y, &UNWRITTEN, 4);
    const float total = svg_path_length(path, at, &x, &y);
    uint32_t bits;
    std::memcpy(&bits, &x, 4);
    o.f32(total);
    o.u8(bits != UNWRITTEN ? 1 : 0);
    o.f32(x);
    o.f32(y);
}

// A builder's path, its state, then svg_path_length at -1 and at each of the ats.
void write_built(Out& o, const path_builder& b, const std::vector<float>& ats) {
    write_path(o, b.path);
    for (float v : {b.cx, b.cy, b.sx, b.sy, b.qx, b.qy, b.kx, b.ky}) o.f32(v);
    o.u8(uint8_t(b.last));
    write_length_at(o, b.path, -1);
    for (float at : ats) write_length_at(o, b.path, at);
}

std::vector<float> read_ats(In& in) {
    std::vector<float> ats(std::min<size_t>(in.u32(), 64));
    for (auto& at : ats) at = in.f32();
    return ats;
}

void write_len(Out& o, const css_length& l) {
    o.u8(l.is_predefined() ? 1 : 0);
    o.i32(l.predef());
    o.i32(int(l.units()));
    o.f32(l.is_predefined() ? 0.f : l.val());
    o.u8(l.calc() ? 1 : 0);
}

void collect(const element::ptr& el, int parent, std::vector<std::pair<element::ptr, int>>& nodes) {
    const int index = int(nodes.size());
    nodes.emplace_back(el, parent);
    for (const auto& child : el->children()) collect(child, index, nodes);
}

const char* const PSEUDO_FLAGS[] = {"root", "hover", "active", "focus", "focus-visible", "focus-within", "link", "visited", "any-link", "local-link", "target", "target-within", "scope", "host"};

// lhdump's write_dom: each node in tree order with its parent, kind, tag, id, classes, attributes and pseudo-class flags.
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

// What SvgElements gives of an element that the DOM does not: el_svg or not, the cascade's own transform and origin, the computed transform, the placement.
std::string element_inputs(const element::ptr& el) {
    Out o;
    const auto tag = std::dynamic_pointer_cast<html_tag>(el);
    const css_properties& css = el->css();
    o.u8(std::dynamic_pointer_cast<el_svg>(el) ? 1 : 0);
    o.u8(tag && tag->has_own_property(_transform_) ? 1 : 0);
    o.u8(tag && tag->has_own_property(_transform_origin_) ? 1 : 0);
    o.u8(css.get_transform_fill_box() ? 1 : 0);
    write_len(o, css.get_transform_origin_x());
    write_len(o, css.get_transform_origin_y());
    const auto& functions = css.get_transform();
    o.u32(uint32_t(functions.size()));
    for (const auto& f : functions) {
        o.i32(int(f.kind));
        write_len(o, f.x);
        write_len(o, f.y);
        for (float v : f.v) o.f32(v);
    }
    const position p = el->get_placement();
    for (pixel_t v : {p.x, p.y, p.width, p.height}) o.f32(v.value());
    return o.s;
}

// The view a <use> under el resolves its lengths against: the nearest <svg>'s view at its placement, 0 0 300 150 without one.
void use_view(const element::ptr& el, float view[4]) {
    view[0] = 0;
    view[1] = 0;
    view[2] = 300;
    view[3] = 150;
    for (auto p = el->parent(); p; p = p->parent())
        if (auto svg = std::dynamic_pointer_cast<el_svg>(p)) {
            paint_group group;
            svg->view_group(svg->get_placement(), view, group);
            return;
        }
}


// What the port computes of an element: svg_geometry with its lengths, svg_ctm, svg_css_transform in a fixed box and the transform attribute, an <svg>'s view box, content sizes and view groups.
std::string element_answers(const element::ptr& el) {
    Out o;
    vector_path path;
    const bool geometry = svg_geometry(el, path);
    o.u8(geometry ? 1 : 0);
    if (geometry) {
        write_path(o, path);
        const float total = svg_path_length(path, -1, nullptr, nullptr);
        write_length_at(o, path, -1);
        for (float at : {0.f, total * 0.37f, total * 0.5f, total, total + 1, 1e30f}) write_length_at(o, path, at);
    }
    transform_matrix ctm;
    o.u8(svg_ctm(el, ctm) ? 1 : 0);
    write_matrix(o, ctm);
    if (std::dynamic_pointer_cast<html_tag>(el)) {
        write_matrix(o, svg_css_transform(el, position(pixel_t(3.f), pixel_t(-7.f), pixel_t(120.f), pixel_t(80.f))));
        write_matrix(o, parse_svg_transform(el->get_attr("transform")));
    }
    if (const auto svg = std::dynamic_pointer_cast<el_svg>(el)) {
        float box[4] = {0, 0, 0, 0};
        o.u8(((*svg).*robbed(SvgViewBox{}))(box) ? 1 : 0);
        for (float v : box) o.f32(v);
        for (float max_width : {0.f, 375.f, 123.5f}) {
            litehtml::size sz;
            svg->get_content_size(sz, pixel_t(max_width));
            o.f32(sz.width.value());
            o.f32(sz.height.value());
        }
        const position places[] = {svg->get_placement(), position(pixel_t(10.f), pixel_t(20.f), pixel_t(0.f), pixel_t(0.f)),
                                   position(pixel_t(-5.f), pixel_t(2.5f), pixel_t(333.f), pixel_t(77.f))};
        for (const position& pos : places) {
            float view[4];
            paint_group group;
            svg->view_group(pos, view, group);
            for (float v : view) o.f32(v);
            write_matrix(o, group.matrix);
            for (pixel_t v : {group.bounds.x, group.bounds.y, group.bounds.width, group.bounds.height}) o.f32(v.value());
        }
    }
    return o.s;
}

// Each <use> in order through two painters (the whole budget and 40), its depth and the painter's from a schedule; draw_use stops at its group, which begin_group throws.
void write_uses(Out& o, Container& container, const std::vector<std::pair<element::ptr, int>>& nodes) {
    static const int depths[] = {0, 0, 1, 8, 9, 3, 2};
    static const int use_depths[] = {0, 1, 0, 2, 0, 1, 3};
    Out calls;
    uint32_t count = 0;
    for (int budget : {int(svg_painter::use_budget), 40}) {
        const float none[4] = {0, 0, 0, 0};
        svg_painter painter(&container, 0, none, nullptr, budget);
        size_t k = 0;
        for (size_t i = 0; i < nodes.size() && k < 1000; i++) {
            const element::ptr& el = nodes[i].first;
            if (lowcase(el->get_tagName()) != "use") continue;
            float view[4];
            use_view(el, view);
            std::copy(view, view + 4, painter.*robbed(PainterView{}));
            painter.*robbed(PainterUseDepth{}) = use_depths[k % 7];
            const int depth = depths[k % 7];
            k++;
            calls.u32(uint32_t(i));
            calls.i32(depth);
            calls.i32(painter.*robbed(PainterUseDepth{}));
            calls.i32(painter.use_left());
            for (float v : view) calls.f32(v);
            bool resolved = false;
            transform_matrix matrix;
            container.armed = true;
            try {
                (painter.*robbed(PainterDrawUse{}))(el, -1, depth);
            } catch (const UseGroup& group) {
                resolved = true;
                matrix = group.matrix;
            }
            container.armed = false;
            calls.u8(resolved ? 1 : 0);
            write_matrix(calls, matrix);
            calls.i32(painter.use_left());
            count++;
        }
    }
    o.u32(count);
    o.s += calls.s;
}

// A stack of hosts (each <use> whose href selects a target, at most 12, then six pairs of nodes by a formula) and every node's svg_parent chain under it (at most 400).
void write_hosts(Out& o, const std::vector<std::pair<element::ptr, int>>& nodes, const document::ptr& doc) {
    std::map<const element*, uint32_t> index;
    for (size_t i = 0; i < nodes.size(); i++) index[nodes[i].first.get()] = uint32_t(i);
    auto& hosts = use_hosts();
    hosts.clear();
    for (size_t i = 0; i < nodes.size() && hosts.size() < 12; i++) {
        const element::ptr& el = nodes[i].first;
        if (lowcase(el->get_tagName()) != "use") continue;
        const char* href = el->get_attr("href");
        if (!href) href = el->get_attr("xlink:href");
        if (!href || href[0] != '#' || !svg_reference_id(href + 1)) continue;
        if (element::ptr target = doc->root()->select_one(std::string("#") + (href + 1))) hosts.emplace_back(target.get(), el);
    }
    const size_t n = nodes.size();
    for (size_t k = 0; k < 6 && n; k++) hosts.emplace_back(nodes[(7 * k + 3) % n].first.get(), nodes[(13 * k + 1) % n].first);
    o.u32(uint32_t(hosts.size()));
    for (const auto& [target, host] : hosts) {
        o.u32(index[target]);
        o.u32(index[host.get()]);
    }
    const size_t chains = std::min<size_t>(n, 400);
    o.u32(uint32_t(chains));
    for (size_t i = 0; i < chains; i++) {
        size_t hosts_above = hosts.size();
        std::vector<uint32_t> chain;
        for (element::ptr cur = nodes[i].first; cur && chain.size() < 4096; cur = svg_parent(cur, hosts_above))
            chain.push_back(index.count(cur.get()) ? index[cur.get()] : UINT32_MAX);
        o.u32(uint32_t(chain.size()));
        for (uint32_t c : chain) o.u32(c);
    }
    hosts.clear();
}

// Each element with an outline (at most 24) against each gradient element (at most 6), its fill attribute made url(#ref): svg_gradient_paint's answer and the gradient's fields.
void write_gradients(Out& o, Container& container, const std::vector<std::pair<element::ptr, int>>& nodes) {
    std::vector<size_t> shapes, gradients;
    for (size_t i = 0; i < nodes.size(); i++) {
        const std::string name = lowcase(nodes[i].first->get_tagName());
        vector_path path;
        if (name == "lineargradient" || name == "radialgradient") {
            if (gradients.size() < 6) gradients.push_back(i);
        } else if (shapes.size() < 24 && svg_geometry(nodes[i].first, path)) {
            shapes.push_back(i);
        }
    }
    o.u32(uint32_t(shapes.size() * gradients.size()));
    for (size_t s : shapes) {
        const element::ptr& shape = nodes[s].first;
        vector_path path;
        svg_geometry(shape, path);
        if (const auto tag = std::dynamic_pointer_cast<html_tag>(shape)) tag->set_attr("fill", "url(#ref)");
        for (size_t g : gradients) {
            const element::ptr& gradient = nodes[g].first;
            svg_gradient out;
            const bool ok = svg_gradient_paint(shape, _fill_, path, out, 1, &container, [&](const std::string&) { return gradient; });
            o.u32(uint32_t(s));
            o.u32(uint32_t(g));
            o.u8(ok ? 1 : 0);
            o.i32(out.type);
            for (float v : {out.x1, out.y1, out.x2, out.y2, out.r, out.fx, out.fy}) o.f32(v);
        }
    }
}

// The element the 'n' record writes its text into, as an attribute: a <rect> in an <svg>.
element::ptr attribute_element(Container& container) {
    static document::ptr doc = document::createFromString("<svg><rect></rect></svg>", &container);
    std::vector<std::pair<element::ptr, int>> nodes;
    collect(doc->root(), -1, nodes);
    for (const auto& [el, parent] : nodes)
        if (lowcase(el->get_tagName()) == "rect") return el;
    return nullptr;
}

bool run(int op, std::string_view input) {
    static Container container;
    In in{input};
    Out o;
    switch (op) {
    case 'n': { // the text: numbers(), next_number from its start until it fails (at most 64) and next_flag likewise (at most 16); then as an attribute, attr_length at five bases, gradient_coordinate, svg_reference_id
        const std::string text = in.rest();
        const char* s = text.c_str();
        const std::vector<float> list = numbers(s);
        o.u32(uint32_t(list.size()));
        for (float v : list) o.f32(v);
        const char* p = s;
        for (int i = 0; i < 64; i++) {
            float v = 0;
            const bool ok = next_number(p, v);
            o.u8(ok ? 1 : 0);
            o.f32(v);
            o.u32(uint32_t(p - s));
            if (!ok) break;
        }
        p = s;
        for (int i = 0; i < 16; i++) {
            int flag = -1;
            const bool ok = next_flag(p, flag);
            o.u8(ok ? 1 : 0);
            o.i32(flag);
            o.u32(uint32_t(p - s));
            if (!ok) break;
        }
        const element::ptr el = attribute_element(container);
        std::dynamic_pointer_cast<html_tag>(el)->set_attr("x", s);
        for (const auto& [base, def] : {std::pair{100.f, -1.f}, std::pair{0.f, 7.f}, std::pair{-37.5f, 0.f}, std::pair{1e30f, 1.f}, std::pair{3.3f, 0.5f}})
            o.f32(attr_length(el, "x", base, def));
        o.f32(gradient_coordinate(el, "x", 0.5f));
        o.u8(svg_reference_id(std::string(s)) ? 1 : 0);
        answer(1, o.s);
        return true;
    }
    case 'p': { // u32 ats and their f32s, then path data: path_builder::parse of it, the builder after it and the lengths (write_built)
        const std::vector<float> ats = read_ats(in);
        const std::string text = in.rest();
        vector_path path;
        path_builder b(path);
        b.parse(text.c_str());
        write_built(o, b, ats);
        answer(1, o.s);
        return true;
    }
    case 'b': { // u32 ats and their f32s, u32 steps, each a u8 op and its operands (0 move x y, 1 line x y, 2 cubic six, 3 quad four, 4 close, 5 arc rx ry rotation i32 large i32 sweep x y, 6 ellipse four, 7 rect six, 8 parse str): the builder after them (write_built)
        const std::vector<float> ats = read_ats(in);
        vector_path path;
        path_builder b(path);
        const uint32_t steps = std::min<uint32_t>(in.u32(), 4096);
        for (uint32_t i = 0; i < steps && in.at < in.s.size(); i++) {
            switch (in.u8()) {
            case 0: {
                const float x = in.f32(), y = in.f32();
                b.move(x, y);
                break;
            }
            case 1: {
                const float x = in.f32(), y = in.f32();
                b.line(x, y);
                break;
            }
            case 2: {
                float v[6];
                for (float& f : v) f = in.f32();
                b.cubic(v[0], v[1], v[2], v[3], v[4], v[5]);
                break;
            }
            case 3: {
                float v[4];
                for (float& f : v) f = in.f32();
                b.quad(v[0], v[1], v[2], v[3]);
                break;
            }
            case 4: b.close(); break;
            case 5: {
                const float rx = in.f32(), ry = in.f32(), rotation = in.f32();
                const int large = in.i32(), sweep = in.i32();
                const float x = in.f32(), y = in.f32();
                b.arc(rx, ry, rotation, large, sweep, x, y);
                break;
            }
            case 6: {
                float v[4];
                for (float& f : v) f = in.f32();
                b.ellipse(v[0], v[1], v[2], v[3]);
                break;
            }
            case 7: {
                float v[6];
                for (float& f : v) f = in.f32();
                b.rect(v[0], v[1], v[2], v[3], v[4], v[5]);
                break;
            }
            default: {
                const std::string text = in.str();
                b.parse(text.c_str());
                break;
            }
            }
        }
        write_built(o, b, ats);
        answer(1, o.s);
        return true;
    }
    case 't': { // u8 null, then the text: parse_svg_transform of it, or of a null text
        const bool null = in.u8() != 0;
        const std::string text = in.rest();
        write_matrix(o, parse_svg_transform(null ? nullptr : text.c_str()));
        answer(1, o.s);
        return true;
    }
    case 'm': { // two matrices of six f32: the first then the second, then is_identity of each
        transform_matrix a, b;
        for (float* f : {&a.a, &a.b, &a.c, &a.d, &a.tx, &a.ty, &b.a, &b.b, &b.c, &b.d, &b.tx, &b.ty}) *f = in.f32();
        write_matrix(o, a.then(b));
        o.u8(a.is_identity() ? 1 : 0);
        o.u8(b.is_identity() ? 1 : 0);
        answer(1, o.s);
        return true;
    }
    case 'D': { // i32 width, then the html, laid out: its DOM as lhdump's 'h' writes it, each node's inputs and answers as strings, the <use> placements, the host chains and the gradients
        const int width = in.i32();
        const document::ptr doc = document::createFromString(in.rest(), &container);
        if (!doc->root()) {
            answer(0, {});
            return true;
        }
        doc->render(pixel_t(float(width)));
        std::vector<std::pair<element::ptr, int>> nodes;
        collect(doc->root(), -1, nodes);
        o.u8(uint8_t(doc->mode()));
        write_dom(o, nodes);
        for (const auto& node : nodes) o.str(element_inputs(node.first));
        for (const auto& node : nodes) o.str(element_answers(node.first));
        write_uses(o, container, nodes);
        write_hosts(o, nodes, doc);
        write_gradients(o, container, nodes);
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
