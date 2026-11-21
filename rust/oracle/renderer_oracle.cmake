# renderer_oracle: renderer.cpp and renderer.h copied with the probes of renderer_probe.h at every call the ported renderer makes into litehtml's document and the container (stage 4.2's parts, the paint and what stage 4.5 reads among them; the input's from input_probes.cmake, the rest here; each pattern found exactly as often as written, or the configure stops), the copy included whole by the driver over the rest of the core.
include("${CMAKE_CURRENT_SOURCE_DIR}/probe.cmake")
include("${CMAKE_CURRENT_SOURCE_DIR}/input_probes.cmake")
set(TW_PROBE_WHAT renderer_oracle)
set(RENDERER_SEAM "${CMAKE_CURRENT_BINARY_DIR}/renderer_seam")
file(READ "${TW_CORE}/renderer.cpp" RENDERER_CPP)
file(READ "${TW_CORE}/renderer.h" RENDERER_H)
string(REPLACE "\r\n" "\n" RENDERER_CPP "${RENDERER_CPP}")
string(REPLACE "\r\n" "\n" RENDERER_H "${RENDERER_H}")
tw_input_probes(RENDERER_CPP RENDERER_H renderer_probe.h RendererOracle)

# The driver reads the container's state too.
tw_probe(RENDERER_CPP 1 [=[#endif
{
public:
    Container(Renderer& r, Backend& backend)]=] [=[#endif
{
    friend struct RendererOracle;
public:
    Container(Renderer& r, Backend& backend)]=])

# The document's life in Container: the walk's words measured, the layout budget, patches, a new document, its height and what a new viewport height asks.
tw_probe(RENDERER_CPP 1 [=[        widths_.before(owed, shown);
#endif
        doc_->set_layout_budget(kLayoutBudgetBase + kLayoutBudgetPerElement * elements);
        doc_->render(litehtml::pixel_t(width));]=] [=[        twp::count(elements);
        widths_.before(owed, shown);
        twp::command('W');
#endif
        doc_->set_layout_budget(kLayoutBudgetBase + kLayoutBudgetPerElement * elements);
        doc_->render(litehtml::pixel_t(width));
        twp::render(width, kLayoutBudgetBase + kLayoutBudgetPerElement * elements);]=])
tw_probe(RENDERER_CPP 2 [=[doc_->layout_exhausted()]=] [=[twp::exhausted(doc_->layout_exhausted())]=])
tw_probe(RENDERER_CPP 1 [=[r_.stop_layout(doc_->layout_spent())]=] [=[r_.stop_layout(twp::spent(doc_->layout_spent()))]=])
tw_probe(RENDERER_CPP 1 [=[bool viewport_units = litehtml::has_viewport_units(html);]=] [=[bool viewport_units = twp::viewport_units(html, litehtml::has_viewport_units(html));]=])
tw_probe(RENDERER_CPP 1 [=[doc_->update_from_string(html, &changed)]=] [=[twp::update(html, doc_->update_from_string(html, &changed), changed)]=])
tw_probe(RENDERER_CPP 1 [=[if (viewport_changed) doc_->media_changed();]=] [=[if (viewport_changed) {
                doc_->media_changed();
                twp::media_changed();
            }]=])
tw_probe(RENDERER_CPP 4 [=[doc_->last_patch_paint_only()]=] [=[twp::paint_only(doc_->last_patch_paint_only())]=])
tw_probe(RENDERER_CPP 1 [=[        doc_ = litehtml::document::createUpdatableFromString(html, this, master_css_);]=] [=[        doc_ = litehtml::document::createUpdatableFromString(html, this, master_css_);
        twp::created(html, master_css_, doc_ != nullptr);]=])
tw_probe(RENDERER_CPP 1 [=[    void index() {
        elements_.clear();]=] [=[    void index() {
        twp::command('X');
        elements_.clear();]=])
tw_probe(RENDERER_CPP 1 [=[return doc_ ? double(doc_->height()) : 0.0;]=] [=[return doc_ ? twp::height(double(doc_->height())) : 0.0;]=])
tw_probe(RENDERER_CPP 1 [=[        if (!doc_ || !doc_->root()) return false;
        if (viewport_units_) return true;]=] [=[        if (!doc_ || !twp::root(doc_->root() != nullptr)) return false;
        if (viewport_units_) return true;]=])
tw_probe(RENDERER_CPP 1 [=[        litehtml::position::vector fixed;]=] [=[        return twp::reads_height([&] {
        litehtml::position::vector fixed;]=])
tw_probe(RENDERER_CPP 1 [=[            if (!contained) return true;
        }
        return false;
    }]=] [=[            if (!contained) return true;
        }
        return false;
        });
    }]=])
tw_probe(RENDERER_CPP 1 [=[return fonts_.size() > built_fonts_ + MAX_PATCHED_FONTS;]=] [=[return twp::fonts(fonts_.size()) > built_fonts_ + MAX_PATCHED_FONTS;]=])
tw_probe(RENDERER_CPP 1 [=[        built_fonts_ = fonts_.size();]=] [=[        built_fonts_ = twp::fonts(fonts_.size());]=])
tw_probe(RENDERER_CPP 1 [=[if (!doc_->update_holes(changes, &changed)) return false;]=] [=[if (!twp::holes(changes, doc_->update_holes(changes, &changed), changed)) return false;]=])
tw_probe(RENDERER_CPP 1 [=[if (!doc_->update_styles(patches, &changed)) return false;]=] [=[if (!twp::styles(patches, doc_->update_styles(patches, &changed), changed)) return false;]=])
tw_probe(RENDERER_CPP 1 [=[    void trim() {
        for (auto it = faces_.begin(); it != faces_.end();) {]=] [=[    void trim() {
        twp::command('t');
        for (auto it = faces_.begin(); it != faces_.end();) {]=])
# The container's images: a violation recorded where it is pushed.
tw_probe(RENDERER_CPP 3 [=[r_.pending_violations_.push_back(]=] [=[twp::push(r_.pending_violations_, ]=])
tw_probe(RENDERER_CPP 1 [=[return container_->image_of(v, image, w, h);]=] [=[return twp::image_of(v, image, w, h, container_->image_of(v, image, w, h));]=])

# The name table, the declaration scans, the elements' edges.
tw_probe(RENDERER_CPP 2 [=[const auto usage = litehtml::_id_thread_usage();]=] [=[const auto usage = twp::usage(litehtml::_id_thread_usage());]=])
tw_probe(RENDERER_CPP 1 [=[const auto& found = container_->declarations(Animator::watched(), Animator::entry_properties(), {}, &generation);]=] [=[const auto& found = twp::declared(container_->declarations(Animator::watched(), Animator::entry_properties(), {}, &generation), Animator::watched(), Animator::entry_properties(), {}, generation);]=])
tw_probe(RENDERER_CPP 1 [=[for (auto& [id, values] : container_->declarations(names, {}, targets)) {]=] [=[for (auto& [id, values] : twp::declared(container_->declarations(names, {}, targets), names, {}, targets, twp::no_generation)) {]=])
tw_probe(RENDERER_CPP 1 [=[    for (const auto& [id, el] : container_->elements()) {
        Rect r = border_box_of(el);]=] [=[    twp::edges(container_->elements(), [](const litehtml::element::ptr& e) { return border_box_of(e); });
    for (const auto& [id, el] : container_->elements()) {
        Rect r = border_box_of(el);]=])

# The elements the renderer's own code looks up.
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(n->id);
        if (!el) return false;]=] [=[        auto el = container_->element(n->id);
        twp::exists(n->id, el != nullptr);
        if (!el) return false;]=])
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(n->id);
        if (!el || n->tag == "style"]=] [=[        auto el = container_->element(n->id);
        twp::exists(n->id, el != nullptr);
        if (!el || n->tag == "style"]=])
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(id);
        if (!n || !el) {]=] [=[        auto el = container_->element(id);
        twp::exists(id, el != nullptr);
        if (!n || !el) {]=])
tw_probe(RENDERER_CPP 1 [=[        el = container_->element(id);
        if (!shadow]=] [=[        el = container_->element(id);
        twp::exists(id, el != nullptr);
        if (!shadow]=])

# What the queries read of the layout and the cascade.
tw_probe(RENDERER_CPP 1 [=[if (kind == "rect") return rect_json(el ? visual_box_of(el) : Rect{});]=] [=[if (kind == "rect") return rect_json(el ? twp::visual(shadow->id, visual_box_of(el)) : Rect{});]=])
tw_probe(RENDERER_CPP 1 [=[for (const Rect& line : inline_boxes_of(el)) rects.push(rect_json(transformed_box(el, line)));]=] [=[for (const Rect& line : twp::lines(shadow->id, inline_boxes_of(el))) rects.push(rect_json(twp::transformed(shadow->id, line, transformed_box(el, line))));]=])
tw_probe(RENDERER_CPP 1 [=[Json::array().push(rect_json(visual_box_of(el)))]=] [=[Json::array().push(rect_json(twp::visual(shadow->id, visual_box_of(el))))]=])
tw_probe(RENDERER_CPP 1 [=[auto placement = [&]() -> Rect { return el ? border_box_of(el) : Rect{}; };]=] [=[auto placement = [&]() -> Rect { return el ? twp::border(shadow->id, border_box_of(el)) : Rect{}; };]=])
tw_probe(RENDERER_CPP 1 [=[            bl = float(bd.left); bt = float(bd.top); br = float(bd.right); bb = float(bd.bottom);
        }]=] [=[            bl = float(bd.left); bt = float(bd.top); br = float(bd.right); bb = float(bd.bottom);
        }
        if (el) twp::borders(shadow->id, ri != nullptr, bl, bt, br, bb);]=])
tw_probe(RENDERER_CPP 1 [=[if (ri) offsets_of(ri, left, top, parent);]=] [=[if (ri) {
            offsets_of(ri, left, top, parent);
            twp::offsets(shadow->id, left, top, parent);
        }]=])
tw_probe(RENDERER_CPP 1 [=[            content_extent(ri, right, bottom, scroller);]=] [=[            content_extent(ri, right, bottom, scroller);
            twp::extent(shadow->id, scroller, float(pad.right), float(pad.bottom), right, bottom);]=])
tw_probe(RENDERER_CPP 1 [=[        if (auto text = std::dynamic_pointer_cast<litehtml::render_item_inline_context>(ri); text && shadow && shadow->tag == "textarea") {
            const auto& pad = ri->get_paddings();
            auto content = text->text_content_size();]=] [=[        if (ri && shadow && shadow->tag == "textarea" && !std::dynamic_pointer_cast<litehtml::render_item_inline_context>(ri)) twp::textarea(shadow->id, false, 0, 0, 0, 0, 0, 0);
        if (auto text = std::dynamic_pointer_cast<litehtml::render_item_inline_context>(ri); text && shadow && shadow->tag == "textarea") {
            const auto& pad = ri->get_paddings();
            auto content = text->text_content_size();
            twp::textarea(shadow->id, true, float(content.width), float(content.height), float(pad.left), float(pad.top), float(pad.right), float(pad.bottom));]=])
tw_probe(RENDERER_CPP 1 [=[if (q.find("prop")) return computed_custom_property(el, q["prop"].asString());]=] [=[if (q.find("prop")) return twp::custom(shadow->id, q["prop"].asString(), computed_custom_property(el, q["prop"].asString()));]=])
tw_probe(RENDERER_CPP 1 [=[return computed_style(el, q["pseudo"].asString(), extras);]=] [=[return twp::computed(shadow->id, q["pseudo"].asString(), animated, computed_style(el, q["pseudo"].asString(), extras));]=])
tw_probe(RENDERER_CPP 1 [=[return double(widget_id_at(q["x"].asNumber(), q["y"].asNumber(), nullptr));]=] [=[return double(TWP_HIT(q["x"].asNumber(), q["y"].asNumber(), nullptr));]=])
tw_probe(RENDERER_CPP 1 [=[if (el && litehtml::svg_text_box(el, container_.get(), text)) {]=] [=[if (el && twp::svg_text(shadow->id, text, litehtml::svg_text_box(el, container_.get(), text))) {]=])
tw_probe(RENDERER_CPP 1 [=[if (!el || !litehtml::svg_geometry(el, path)) return]=] [=[if (!el || !twp::svg_geometry(shadow->id, litehtml::svg_geometry(el, path))) return]=])
tw_probe(RENDERER_CPP 1 [=[if (method == "getTotalLength") return double(litehtml::svg_path_length(path, -1, nullptr, nullptr));]=] [=[if (method == "getTotalLength") return double(twp::path_length(shadow->id, -1.0f, litehtml::svg_path_length(path, -1, nullptr, nullptr), 0, 0));]=])
tw_probe(RENDERER_CPP 1 [=[            litehtml::svg_path_length(path, std::isfinite(at) ? float(std::max(0.0, at)) : 0.0f, &x, &y);]=] [=[            litehtml::svg_path_length(path, std::isfinite(at) ? float(std::max(0.0, at)) : 0.0f, &x, &y);
            twp::path_length(shadow->id, std::isfinite(at) ? float(std::max(0.0, at)) : 0.0f, 0, x, y);]=])
tw_probe(RENDERER_CPP 1 [=[if (!path_bounds(path, x0, y0, x1, y1)) return Json::object()]=] [=[if (!twp::bounds(shadow->id, path_bounds(path, x0, y0, x1, y1), x0, y0, x1, y1)) return Json::object()]=])

# Stage 4.5's questions: the elements its code looks up, the boxes the paint policy counts, the clips, tiles and coverage the composition reads, the paints it asks for, the hits through layers.
tw_probe(RENDERER_CPP 1 [=[        if (auto el = container_->element(p.id)) mover_elements.insert(el.get());]=] [=[        if (auto el = twp::element(p.id, container_->element(p.id))) mover_elements.insert(el.get());]=])
tw_probe(RENDERER_CPP 1 [=[&& container_->background_is_local(ancestor);]=] [=[&& twp::local(ancestor, container_->background_is_local(ancestor));]=])
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(id);
        Rect box;
        if (!el || !container_->layer_candidate(el, box, &mover_elements)) continue;]=] [=[        auto el = twp::element(id, container_->element(id));
        Rect box;
        if (!el || !twp::candidate(id, &mover_elements, box, container_->layer_candidate(el, box, &mover_elements))) continue;]=])
tw_probe(RENDERER_CPP 1 [=[if (!container_->clip_of(want.el, p.has_clip, p.clip, p.clip_radii)) continue;]=] [=[if (!twp::clip(id, p.has_clip, p.clip, p.clip_radii, container_->clip_of(want.el, p.has_clip, p.clip, p.clip_radii))) continue;]=])
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(id);
        Promotion p;
        Rect area;
        if (blocked || !el || !container_->background_tile(el, p.box, area, p.clip, p.clip_radii, p.tile)) continue;]=] [=[        auto el = twp::element(id, container_->element(id));
        Promotion p;
        Rect area;
        if (blocked || !el || !twp::tile(id, p.box, area, p.clip, p.clip_radii, p.tile, container_->background_tile(el, p.box, area, p.clip, p.clip_radii, p.tile))) continue;]=])
tw_probe(RENDERER_CPP 1 [=[if (!container_->clip_of(el, outer, outer_clip, outer_radii)) continue;]=] [=[if (!twp::clip(id, outer, outer_clip, outer_radii, container_->clip_of(el, outer, outer_clip, outer_radii))) continue;]=])
tw_probe(RENDERER_CPP 1 [=[    const auto& trace = container_->paint_order();]=] [=[    const auto& trace = container_->paint_order();
    twp::command('|');]=])
tw_probe(RENDERER_CPP 1 [=[            auto el = container_->element(id);
            if (!alone || !el) continue;]=] [=[            auto el = twp::element(id, container_->element(id));
            if (!alone || !el) continue;]=])
tw_probe(RENDERER_CPP 3 [=[container_->layer_candidate(el, p.box)]=] [=[twp::candidate(id, nullptr, p.box, container_->layer_candidate(el, p.box))]=])
tw_probe(RENDERER_CPP 3 [=[container_->clip_of(el, p.has_clip, p.clip, p.clip_radii)]=] [=[twp::clip(id, p.has_clip, p.clip, p.clip_radii, container_->clip_of(el, p.has_clip, p.clip, p.clip_radii))]=])
tw_probe(RENDERER_CPP 2 [=[container_->declared(id, {"transform", "translate", "rotate", "scale", "opacity", "transform-origin"})]=] [=[twp::declared(id, container_->declared(id, {"transform", "translate", "rotate", "scale", "opacity", "transform-origin"}))]=])
tw_probe(RENDERER_CPP 1 [=[if (!container_->covered(el, p.reach, trace, others)) break;]=] [=[if (!twp::covered(id, p.reach, others, container_->covered(el, p.reach, trace, others))) break;]=])
tw_probe(RENDERER_CPP 2 [=[        auto el = container_->element(id);
        Promotion p;
        if (!alone || !el]=] [=[        auto el = twp::element(id, container_->element(id));
        Promotion p;
        if (!alone || !el]=])
tw_probe(RENDERER_CPP 1 [=[!container_->covered(elements[id], wanted_layer.reach, trace, ignore_all)) continue;]=] [=[!twp::covered(id, wanted_layer.reach, ignore_all, container_->covered(elements[id], wanted_layer.reach, trace, ignore_all))) continue;]=])
tw_probe(RENDERER_CPP 1 [=[auto found = container_->obscurers(elements[id], wanted_layer.reach, trace, ignore_all, MAX_OBSCURERS);]=] [=[auto found = twp::obscurers(id, wanted_layer.reach, ignore_all, MAX_OBSCURERS, container_->obscurers(elements[id], wanted_layer.reach, trace, ignore_all, MAX_OBSCURERS));]=])
tw_probe(RENDERER_CPP 1 [=[                auto el = container_->element(other);]=] [=[                auto el = twp::element(other, container_->element(other));]=])
tw_probe(RENDERER_CPP 1 [=[!container_->layer_candidate(el, s.box) || !container_->clip_of(el, s.has_clip, s.clip, s.clip_radii)) {]=] [=[!twp::candidate(other, nullptr, s.box, container_->layer_candidate(el, s.box)) || !twp::clip(other, s.has_clip, s.clip, s.clip_radii, container_->clip_of(el, s.has_clip, s.clip, s.clip_radii))) {]=])
tw_probe(RENDERER_CPP 1 [=[        p.order = size_t(std::find(trace.begin(), trace.end(), ri.get()) - trace.begin());]=] [=[        p.order = twp::order(id, size_t(std::find(trace.begin(), trace.end(), ri.get()) - trace.begin()));]=])
tw_probe(RENDERER_CPP 1 [=[if (p.reach.empty() || container_->covered(elements[id], p.reach, trace, ignore)) continue;]=] [=[if (p.reach.empty() || twp::covered(id, p.reach, ignore, container_->covered(elements[id], p.reach, trace, ignore))) continue;]=])

# The paint: the document drawn with or without its layers, each layer's content, the key of a list kept.
tw_probe(RENDERER_CPP 1 [=[        painted_key_ = container_->paint_key();]=] [=[        painted_key_ = twp::paint_key(container_->paint_key());]=])
tw_probe(RENDERER_CPP 1 [=[    if (container_->paint_key() != painted_key_ ||]=] [=[    if (twp::paint_key(container_->paint_key()) != painted_key_ ||]=])
tw_probe(RENDERER_CPP 1 [=[            if (auto el = container_->element(id)) {
                order.emplace_back(p.order, id);]=] [=[            if (auto el = twp::element(id, container_->element(id))) {
                order.emplace_back(p.order, id);]=])
tw_probe(RENDERER_CPP 1 [=[        DisplayList list = container_->draw(skip, backgrounds);]=] [=[        DisplayList list = twp::drawn(skip, backgrounds, container_->draw(skip, backgrounds));]=])
tw_probe(RENDERER_CPP 1 [=[                layer.content = container_->draw_layer(container_->element(id), skip, layer.bounds);]=] [=[                layer.content = twp::layer_drawn(id, skip, layer.bounds, container_->draw_layer(container_->element(id), skip, layer.bounds));]=])
tw_probe(RENDERER_CPP 1 [=[    return container_->draw();
}]=] [=[    return twp::plain(container_->draw());
}]=])

# The hits: inside a layer where its track shows it, then the document.
tw_probe(RENDERER_CPP 1 [=[if (int64_t hit = container_->widget_id(container_->element_at_within(container_->element(id), mx, my), placement)) return hit;]=] [=[if (int64_t hit = twp::hit_within(id, mx, my, placement, container_->widget_id(container_->element_at_within(container_->element(id), mx, my), placement))) return hit;]=])
tw_probe(RENDERER_CPP 1 [=[    return container_->widget_id(container_->element_at(x, y), placement);]=] [=[    return twp::hit_answer(x, y, placement, container_->widget_id(container_->element_at(x, y), placement));]=])

# The heavy boxes: the walk between its markers, a box's facts where it is set, the no-effects switch past the limit.
tw_probe(RENDERER_CPP 1 [=[std::optional<float> Renderer::layer_opacity(int64_t id) const {]=] [=[// What box_paint_of reads of a box, recorded where the walk or a recount sets it.
int64_t twp_box(int64_t key, const litehtml::element& el, bool pseudo) {
    if (twp::question('%')) {
        twp::raw(key);
        twp::u8(pseudo);
        const auto& css = el.css();
        twp::u8(!css.get_backdrop_filter().empty());
        twp::u8(!css.get_filter().empty());
        bool blurred = false;
        for (const auto& shadow : css.get_box_shadows()) {
            if (!(shadow.blur.val() <= 0)) {
                blurred = true;
                break;
            }
        }
        twp::u8(blurred);
        twp::u8(!pseudo && el.tag() == litehtml::_button_ && (el.get_attr("disabled") || el.has_pseudo_class(litehtml::_active_)));
        twp::raw(float(css.get_opacity()));
    }
    return key;
}

std::optional<float> Renderer::layer_opacity(int64_t id) const {]=])
tw_probe(RENDERER_CPP 1 [=[    paint_.begin_scan();]=] [=[    paint_.begin_scan();
    twp::command('Q');]=])
tw_probe(RENDERER_CPP 1 [=[    if (paint_.end_scan()) motion_stale_ = true;]=] [=[    twp::command(';');
    if (paint_.end_scan()) motion_stale_ = true;]=])
tw_probe(RENDERER_CPP 1 [=[paint_.set_box(parent * 4 + (tag == litehtml::__tag_before_ ? 1 : 2), box_paint_of(*el, true, std::nullopt));]=] [=[paint_.set_box(twp_box(parent * 4 + (tag == litehtml::__tag_before_ ? 1 : 2), *el, true), box_paint_of(*el, true, std::nullopt));]=])
tw_probe(RENDERER_CPP 2 [=[paint_.set_box(id * 4, box_paint_of(*el, false, layer_opacity(id)));]=] [=[paint_.set_box(twp_box(id * 4, *el, false), box_paint_of(*el, false, layer_opacity(id)));]=])
tw_probe(RENDERER_CPP 1 [=[        auto el = container_->element(id);
        if (el && paint_.has_box(id * 4))]=] [=[        auto el = twp::element(id, container_->element(id));
        if (el && paint_.has_box(id * 4))]=])
tw_probe(RENDERER_CPP 1 [=[    container_->set_no_effects();]=] [=[    container_->set_no_effects();
    twp::command('-');]=])

file(WRITE "${RENDERER_SEAM}/renderer.cpp.tmp" "${RENDERER_CPP}")
file(WRITE "${RENDERER_SEAM}/renderer.h.tmp" "${RENDERER_H}")
configure_file("${RENDERER_SEAM}/renderer.cpp.tmp" "${RENDERER_SEAM}/renderer.cpp" COPYONLY)
configure_file("${RENDERER_SEAM}/renderer.h.tmp" "${RENDERER_SEAM}/renderer.h" COPYONLY)
# renderer_probe.h and the probe lists are dependencies MSBuild does not see: the copy is written again when they change.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/renderer_probe.h" "${CMAKE_CURRENT_SOURCE_DIR}/renderer_oracle.cmake")
add_executable(renderer_oracle renderer_oracle.cpp)
target_include_directories(renderer_oracle BEFORE PRIVATE "${RENDERER_SEAM}" "${CMAKE_CURRENT_SOURCE_DIR}" "${TW_GENERATED}")
target_link_libraries(renderer_oracle PRIVATE events_core)
