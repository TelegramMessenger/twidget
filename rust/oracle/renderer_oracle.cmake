# renderer_oracle: renderer.cpp and renderer.h copied with the probes of renderer_probe.h at every call the ported renderer makes into litehtml's document, the container's stage 4.2 parts and stage 4.5's code (the input's from input_probes.cmake, the rest here; each pattern found exactly as often as written, or the configure stops), the copy included whole by the driver over the rest of the core.
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
tw_probe(RENDERER_CPP 1 [=[class Container : public litehtml::document_container {
public:]=] [=[class Container : public litehtml::document_container {
    friend struct RendererOracle;
public:]=])

# The document's life in Container: the layout budget, patches, a new document, its height and what a new viewport height asks.
tw_probe(RENDERER_CPP 1 [=[        doc_->set_layout_budget(kLayoutBudgetBase + kLayoutBudgetPerElement * elements);
        doc_->render(litehtml::pixel_t(width));]=] [=[        twp::count(elements);
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
tw_probe(RENDERER_CPP 2 [=[const auto usage = litehtml::_id_usage();]=] [=[const auto usage = twp::usage(litehtml::_id_usage());]=])
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

# Stage 4.5's code: what each call left, recorded as the outermost one returns, and its answers.
tw_probe(RENDERER_CPP 1 [=[bool fast = scripted_only(ops, scripted);]=] [=[bool fast = twp::scripted(scripted_only(ops, scripted), scripted);]=])
tw_probe(RENDERER_CPP 1 [=[if (fast && apply_scripted(scripted)) {]=] [=[if (fast && twp::applied(apply_scripted(scripted), dirty_)) {]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::update_composition(double now) {]=] [=[void Renderer::update_composition(double now) {
    TWP_STAGE45('U', [&] { twp::raw(now); }, [this] {
        twp::u8(dirty_);
        twp::u8(touched_all_);
        twp::raw(wake_at_);
        twp::ids(layer_ids_);
        twp::promoted(promoted_);
    });]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::strip_layers(StyleOverrides& sampled) const {]=] [=[void Renderer::strip_layers(StyleOverrides& sampled) const {
    TWP_STAGE45('T', [this] { twp::promoted(promoted_); }, nullptr);]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::count_paint() {]=] [=[void Renderer::count_paint() {
    const size_t twp_events = pending_events_.size();
    TWP_STAGE45('P', nullptr, [this, twp_events] {
        twp::messages(pending_events_, twp_events);
        twp::u8(paint_stopped_);
    });]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::recount_boxes(const std::vector<int64_t>& ids) {]=] [=[void Renderer::recount_boxes(const std::vector<int64_t>& ids) {
    const size_t twp_events = pending_events_.size();
    TWP_STAGE45('B', nullptr, [this, twp_events] {
        twp::messages(pending_events_, twp_events);
        twp::u8(paint_stopped_);
    });]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::note_scripted(const Json::Array& ops) {]=] [=[void Renderer::note_scripted(const Json::Array& ops) {
    TWP_STAGE45('G', nullptr, nullptr);]=])
tw_probe(RENDERER_CPP 1 [=[void Renderer::note_svg(const Json::Array& ops) {]=] [=[void Renderer::note_svg(const Json::Array& ops) {
    TWP_STAGE45('W', nullptr, nullptr);]=])

file(WRITE "${RENDERER_SEAM}/renderer.cpp.tmp" "${RENDERER_CPP}")
file(WRITE "${RENDERER_SEAM}/renderer.h.tmp" "${RENDERER_H}")
configure_file("${RENDERER_SEAM}/renderer.cpp.tmp" "${RENDERER_SEAM}/renderer.cpp" COPYONLY)
configure_file("${RENDERER_SEAM}/renderer.h.tmp" "${RENDERER_SEAM}/renderer.h" COPYONLY)
# renderer_probe.h and the probe lists are dependencies MSBuild does not see: the copy is written again when they change.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/renderer_probe.h" "${CMAKE_CURRENT_SOURCE_DIR}/renderer_oracle.cmake")
add_executable(renderer_oracle renderer_oracle.cpp)
target_include_directories(renderer_oracle BEFORE PRIVATE "${RENDERER_SEAM}" "${CMAKE_CURRENT_SOURCE_DIR}" "${TW_GENERATED}")
target_link_libraries(renderer_oracle PRIVATE events_core)
