// lhdump's 'W' record (stage L16): hit testing and the mouse state between patches, layouts and paints; lhdump.cpp includes it after patching(), and rust/crates/litehtml/tests/common/hit.rs lays it out.
#pragma once

namespace lhdump_dom {
#define LHDUMP_ROB_P(Name, Member) \
    struct Name {}; \
    auto robbed(Name); \
    template struct Rob<&Member, Name>;
LHDUMP_ROB_P(DocActiveElement, document::m_active_element)
#undef LHDUMP_ROB_P
} // namespace lhdump_dom

namespace {

// The renderer's hittable (core/src/renderer.cpp) through computed_pointer_events (core/src/computed_style.cpp), copied.
bool hit_hittable(const element::ptr& el) {
    for (auto e = el; e; e = e->parent()) {
        auto tag = std::dynamic_pointer_cast<html_tag>(e);
        std::string v = tag ? twlite::css::trim(tag->raw_declaration("pointer-events")) : std::string();
        if (!v.empty() && v != "inherit" && v != "unset") return (v == "initial" ? "auto" : v) != "none";
    }
    return true;
}

void write_hit_box(Out& o, const position& p) {
    for (pixel_t v : {p.x, p.y, p.width, p.height}) o.f32(lhdump_paint::nan1(v.value()));
}

// The hovered and active elements in the walk's numbering (-1 none, -2 out of the tree).
void write_hit_state(Out& o, document& doc) {
    const ElementIndex w(doc);
    o.i32(w.of(doc.get_over_element().get()));
    o.i32(w.of(lhdump_dom::field<lhdump_dom::DocActiveElement>(doc).get()));
}

// f32 width, height and dpr, u8 flags (bit 0: the container sanitizes elements), the master sheet, the html, then u32 steps: 'u', 's', 'H', 'm', 'w' as 'b'; 'p' a paint step as 'Q'; 'd' the cascade, the layout, what painting keeps and the hovered and active elements; 'g' get_element_by_point (u8 check: 0 none, 1 the renderer's hittable, 2 v-scrollable, 3 h-scrollable, 4 any, 5 the items of even number; f32 its delta; u8 from: 0 the root, 1 the k-th element as the renderer's element_at_within, 2 the k-th item, 3 get_child_by_point of the k-th item; u32 k; u8 flag and i32 z-index for 3; f32 x, y, client x, client y) answered with i32 the element; 'o', 'b', 'U' f32 x, y, client x, client y: on_mouse_over, on_lbutton_down, on_lbutton_up, 'c' on_button_cancel, 'l' on_mouse_leave, each answered with u8, the redraw boxes (u32, 4 f32 each) and the hovered and active elements; 'S' f32 dx, dy, x, y, client x, client y: on_scroll (u8 0 without a render tree, else 1, u32 values, each f32 dx, dy and the box); 'T' u8 what, u32 k, f32 a, b on the k-th item (0 scroll_to, 3 is_h_scrollable, 4 is_v_scrollable, 5 scroll_box of a, b, 6 is_point_inside) or element (1 h_scroll, 2 v_scroll), u8 found then its answer; 'v' u32 k, u8 at (0 its placement, 1 the 4 f32 that follow), f32 x, y: the k-th element's el_svg::element_at (i32, -3 not an svg): the document as createUpdatableFromString makes it, each step's answer and the names the case interned, then the container's calls by phase.
void pointing(In& in, Out& o) {
    const size_t names_before = _id_usage().names;
    CascadeContainer container;
    container.width = in.f32();
    container.height = in.f32();
    container.dpr = in.f32();
    const uint8_t flags = in.u8();
    container.sanitizing = (flags & 1) != 0;
    const std::string master = in.str();
    std::string current = in.str();
    auto doc = document::createUpdatableFromString(current, &container, master);
    o.u8(uint8_t(doc->mode()));
    std::vector<size_t> phases{container.calls.size()};
    const uint32_t steps = in.u32();
    uint8_t last_op = 0;
    for (uint32_t s = 0; s < steps && in.at < in.s.size(); s++, phases.push_back(container.calls.size())) {
        const uint8_t op = in.u8();
        if (last_op != 'w') lhdump_grid::trace().clear();
        last_op = op;
        const ElementIndex w(*doc);
        container.hit_elements = &w.index;
        if (op == 'u') {
            const std::string html = in.str();
            bool changed = true;
            const bool ok = doc->update_from_string(html, &changed);
            if (ok) current = html;
            write_patch_answer(o, *doc, ok, changed);
        } else if (op == 's' || op == 'H') {
            const uint32_t n = in.u32();
            std::vector<std::pair<std::shared_ptr<element>, std::string>> styles;
            std::vector<document::hole_change> changes;
            for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) {
                const element::ptr el = w.pick(in.u32());
                if (op == 's') {
                    styles.emplace_back(el, in.str());
                    continue;
                }
                document::hole_change change;
                change.owner = el;
                change.text = in.u8() != 0;
                change.name = in.str();
                change.value = in.str();
                changes.push_back(std::move(change));
            }
            bool changed = true;
            const bool ok = op == 's' ? doc->update_styles(styles, &changed) : doc->update_holes(changes, &changed);
            std::string html;
            if (ok && doc->template_html(html)) current = html;
            write_patch_answer(o, *doc, ok, changed);
        } else if (op == 'm') {
            container.width = in.f32();
            container.height = in.f32();
            o.u8(doc->root() && doc->media_changed() ? 1 : 0);
        } else if (op == 'w') {
            const float width = in.f32();
            const uint8_t how = in.u8();
            doc->set_layout_budget((how & 1) ? in.u64() : renderer_layout_budget(*doc));
            o.f32(doc->render(pixel_t(width), render_type(how >> 1)).value());
            std::vector<position> regions;
            const bool dirty = doc->layout_dirty_regions(regions);
            o.u8(dirty ? 1 : 0);
            o.u32(uint32_t(regions.size()));
            for (const auto& r : regions) write_pos(o, r);
        } else if (op == 'p') {
            container.hit_elements = nullptr;
            paint_step(in, o, *doc, container);
        } else if (op == 'd') {
            write_cascade(o, *doc);
            write_layout(o, *doc);
            write_paint(o, *doc);
            write_hit_state(o, *doc);
        } else if (op == 'g') {
            const uint8_t check = in.u8();
            const float delta = in.f32();
            const uint8_t from = in.u8();
            const uint32_t k = in.u32();
            const uint8_t flag = in.u8();
            const int32_t zindex = in.i32();
            const float x = in.f32(), y = in.f32(), client_x = in.f32(), client_y = in.f32();
            PaintIndex index(*doc);
            std::function<bool(const std::shared_ptr<render_item>&)> checker;
            if (check != 0) {
                checker = [&](const std::shared_ptr<render_item>& ri) {
                    bool ok = true;
                    if (check == 1) ok = hit_hittable(ri->src_el());
                    if (check == 2) ok = ri->is_v_scrollable(pixel_t(delta));
                    if (check == 3) ok = ri->is_h_scrollable(pixel_t(delta));
                    if (check == 5) ok = index.t.of(ri.get()) % 2 == 0;
                    Out b;
                    b.i32(index.t.of(ri.get()));
                    container.calls.push_back({'?', {}, b.s, ok ? 1 : 0});
                    return ok;
                };
            }
            element::ptr hit;
            if (from == 0) {
                if (doc->root_render()) hit = doc->root_render()->get_element_by_point(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), checker);
            } else if (from == 1) {
                const element::ptr el = w.pick(k);
                auto ri = el ? el->get_render_item() : nullptr;
                if (ri) {
                    auto placement = ri->get_placement();
                    auto sticky = ri->sticky_offset();
                    auto px = pixel_t(x) - (placement.x - ri->pos().x - sticky.x);
                    auto py = pixel_t(y) - (placement.y - ri->pos().y - sticky.y);
                    hit = ri->get_element_by_point(px, py, pixel_t(client_x), pixel_t(client_y), checker);
                }
            } else if (!index.t.items.empty()) {
                render_item* ri = index.t.items[k % index.t.items.size()];
                if (from == 2)
                    hit = ri->get_element_by_point(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), checker);
                else
                    hit = ri->get_child_by_point(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), draw_flag(flag % 5), zindex, checker);
            }
            o.i32(w.of(hit.get()));
        } else if (op == 'o' || op == 'b' || op == 'U' || op == 'c' || op == 'l') {
            float x = 0, y = 0, client_x = 0, client_y = 0;
            if (op == 'o' || op == 'b' || op == 'U') {
                x = in.f32();
                y = in.f32();
                client_x = in.f32();
                client_y = in.f32();
            }
            std::vector<position> boxes;
            auto redraw = [&](const position& p) { boxes.push_back(p); };
            bool ret = false;
            if (op == 'o') ret = doc->on_mouse_over(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), redraw);
            if (op == 'b') ret = doc->on_lbutton_down(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), redraw);
            if (op == 'U') ret = doc->on_lbutton_up(pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y), redraw);
            if (op == 'c') ret = doc->on_button_cancel(redraw);
            if (op == 'l') ret = doc->on_mouse_leave(redraw);
            o.u8(ret ? 1 : 0);
            o.u32(uint32_t(boxes.size()));
            for (const auto& b : boxes) write_hit_box(o, b);
            write_hit_state(o, *doc);
        } else if (op == 'S') {
            const float dx = in.f32(), dy = in.f32(), x = in.f32(), y = in.f32(), client_x = in.f32(), client_y = in.f32();
            if (!doc->root_render()) {
                o.u8(0);
                continue;
            }
            const auto values = doc->on_scroll(pixel_t(dx), pixel_t(dy), pixel_t(x), pixel_t(y), pixel_t(client_x), pixel_t(client_y));
            o.u8(1);
            o.u32(uint32_t(values.size()));
            for (const auto& v : values) {
                o.f32(lhdump_paint::nan1(v.dx.value()));
                o.f32(lhdump_paint::nan1(v.dy.value()));
                write_hit_box(o, v.scroll_box);
            }
        } else if (op == 'T') {
            const uint8_t what = in.u8();
            const uint32_t k = in.u32();
            const float a = in.f32(), b = in.f32();
            if (what == 1 || what == 2) {
                const element::ptr el = w.pick(k);
                o.u8(el ? 1 : 0);
                if (el) o.f32(lhdump_paint::nan1((what == 1 ? el->h_scroll(pixel_t(a)) : el->v_scroll(pixel_t(a))).value()));
                continue;
            }
            PaintIndex index(*doc);
            o.u8(index.t.items.empty() ? 0 : 1);
            if (index.t.items.empty()) continue;
            render_item* ri = index.t.items[k % index.t.items.size()];
            if (what == 0) ri->scroll_to(pixel_t(a), pixel_t(b));
            if (what == 3) o.u8(ri->is_h_scrollable(pixel_t(a)) ? 1 : 0);
            if (what == 4) o.u8(ri->is_v_scrollable(pixel_t(a)) ? 1 : 0);
            if (what == 5) {
                position box(pixel_t(a), pixel_t(b), 0, 0);
                ri->scroll_box(box);
                write_hit_box(o, box);
            }
            if (what == 6) o.u8(ri->is_point_inside(pixel_t(a), pixel_t(b)) ? 1 : 0);
        } else if (op == 'v') {
            const element::ptr el = w.pick(in.u32());
            const uint8_t at = in.u8();
            position pos;
            if (at == 1) {
                pos.x = in.f32();
                pos.y = in.f32();
                pos.width = in.f32();
                pos.height = in.f32();
            }
            const float x = in.f32(), y = in.f32();
            auto* svg = dynamic_cast<el_svg*>(el.get());
            if (!svg) {
                o.i32(-3);
                continue;
            }
            if (at == 0) pos = svg->get_placement();
            o.i32(w.of(svg->element_at(pos, pixel_t(x), pixel_t(y)).get()));
        }
    }
    container.hit_elements = nullptr;
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

} // namespace
