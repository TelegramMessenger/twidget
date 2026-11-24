# The input code's probes (stage 4.4) over renderer.cpp and renderer.h held in `cpp_var` and `h_var`: renderer.h includes `header`, which defines them, and makes the oracle's `friend` its friend. events_oracle's header records the input's questions and its builds as actions; renderer_oracle's, where the renderer is ported whole, records them as the document's answers.
function(tw_input_probes cpp_var h_var header friend)
set(EVENTS_CPP "${${cpp_var}}")
set(EVENTS_H "${${h_var}}")
# renderer.h: the probes, the driver as a friend, the width an RTL arbiter reads.
tw_probe(EVENTS_H 1 [=[#include "twidget/json.h"]=] "#include \"twidget/json.h\"
#include \"${header}\"")
tw_probe(EVENTS_H 1 [=[    friend class Container;]=] "    friend class Container;
    friend struct ${friend};")
tw_probe(EVENTS_H 1 [=[return rtl_ ? width_ - x : x;]=] [=[return rtl_ ? twp::width_of(width_) - x : x;]=])

# Questions: hit tests, boxes, elements, the cascade's touch-action and user-select.
tw_probe(EVENTS_CPP 1 [=[widget_id_at(x, y, &t.target_rect)]=] [=[TWP_HIT(x, y, &t.target_rect)]=])
tw_probe(EVENTS_CPP 1 [=[widget_id_at(x, y, &rect)]=] [=[TWP_HIT(x, y, &rect)]=])
tw_probe(EVENTS_CPP 1 [=[widget_id_at(x, y, &up_rect)]=] [=[TWP_HIT(x, y, &up_rect)]=])
tw_probe(EVENTS_CPP 2 [=[widget_id_at(x, y, nullptr)]=] [=[TWP_HIT(x, y, nullptr)]=])
tw_probe(EVENTS_CPP 1 [=[    auto el = container_->element(id);
    return el ? border_box_of(el) : Rect{};]=] [=[    auto el = container_->element(id);
    return twp::box(id, el ? border_box_of(el) : Rect{});]=])
tw_probe(EVENTS_CPP 1 [=[!container_->element(t.capture)]=] [=[!twp::exists(t.capture, container_->element(t.capture) != nullptr)]=])
tw_probe(EVENTS_CPP 1 [=[!container_->element(done.capture)]=] [=[!twp::exists(done.capture, container_->element(done.capture) != nullptr)]=])
tw_probe(EVENTS_CPP 1 [=[&& container_->element(n->id)) set_capture]=] [=[&& twp::exists(n->id, container_->element(n->id) != nullptr)) set_capture]=])
tw_probe(EVENTS_CPP 1 [=[if (auto el = n->id ? container_->element(n->id) : nullptr) values.push_back(computed_touch_action(el));]=] [=[if (n->id) {
            auto el = container_->element(n->id);
            if (el) values.push_back(computed_touch_action(el));
            twp::touch_action(n->id, el ? &values.back() : nullptr);
        }]=])
tw_probe(EVENTS_CPP 1 [=[    for (auto el = container_->element(target); el; el = el->parent()) {]=] [=[    bool twp_verdict = [&] {
    for (auto el = container_->element(target); el; el = el->parent()) {]=])
tw_probe(EVENTS_CPP 1 [=[        if (value != litehtml::user_select_auto) return value == litehtml::user_select_text;
    }
#endif]=] [=[        if (value != litehtml::user_select_auto) return value == litehtml::user_select_text;
    }
    return false; }();
    return twp::user_select(target, twp_verdict);
#endif]=])

# The mouse state behind :hover and :active, and the cursor.
tw_probe(EVENTS_CPP 1 [=[container_->mouse(action, x, y, done.mouse && action == 2)]=] [=[twp::mouse(action, x, y, done.mouse && action == 2, container_->mouse(action, x, y, done.mouse && action == 2))]=])
tw_probe(EVENTS_CPP 1 [=[container_->mouse(0, x, y)]=] [=[twp::mouse(0, x, y, false, container_->mouse(0, x, y))]=])
tw_probe(EVENTS_CPP 2 [=[container_->mouse(1, x, y)]=] [=[twp::mouse(1, x, y, false, container_->mouse(1, x, y))]=])
tw_probe(EVENTS_CPP 1 [=[container_->mouse(3, done.x, done.y)]=] [=[twp::mouse(3, done.x, done.y, false, container_->mouse(3, done.x, done.y))]=])
tw_probe(EVENTS_CPP 1 [=[container_->leave()]=] [=[twp::leave(container_->leave())]=])
tw_probe(EVENTS_CPP 1 [=[const std::string& c = container_->cursor();]=] [=[const std::string& c = twp::cursor(container_->cursor());]=])

# The text as laid out: its key and runs, widths, fonts, layers, whether any is selectable.
tw_probe(EVENTS_CPP 1 [=[    auto key = std::make_tuple(painted, generation, layouts, doc ? doc->style_epoch() : 0);]=] [=[    auto key = std::make_tuple(painted, generation, layouts, doc ? doc->style_epoch() : 0);
    twp::runs_key(key);]=])
tw_probe(EVENTS_CPP 1 [=[    if (!doc || !doc->root()) return runs;]=] [=[    if (!doc || !doc->root()) {
        twp::runs(runs);
        return runs;
    }]=])
tw_probe(EVENTS_CPP 1 [=[    walk(doc->root(), 0, 0);]=] [=[    walk(doc->root(), 0, 0);
    twp::runs(runs);]=])
tw_probe(EVENTS_CPP 1 [=[float w = float(container_->text_width(run.text.substr(0, bytes).c_str(), litehtml::uint_ptr(run.font)));]=] [=[float w = twp::width(run.text.substr(0, bytes).c_str(), run.font, float(container_->text_width(run.text.substr(0, bytes).c_str(), litehtml::uint_ptr(run.font))));]=])
tw_probe(EVENTS_CPP 1 [=[!container_->font_info(r.font, font, descent, letter, word)]=] [=[!twp::font_info(r.font, font, descent, letter, word, container_->font_info(r.font, font, descent, letter, word))]=])
tw_probe(EVENTS_CPP 1 [=[layer_shown(r.layer, now, shown, opacity) ?]=] [=[twp::layer(r.layer, now, shown, opacity, layer_shown(r.layer, now, shown, opacity)) ?]=])
tw_probe(EVENTS_CPP 1 [=[    std::pair<uint64_t, uint64_t> key{container_->document_serial(), doc ? doc->style_epoch() : 0};]=] [=[    std::pair<uint64_t, uint64_t> key{container_->document_serial(), doc ? doc->style_epoch() : 0};
    twp::selectable_key(key);]=])
tw_probe(EVENTS_CPP 1 [=[    if (doc && doc->root()) visit(doc->root());
    return selectable_;]=] [=[    if (doc && doc->root()) visit(doc->root());
    twp::selectable(selectable_);
    return selectable_;]=])

# The editor's style, images, resize observers, the renderer's own messages.
tw_probe(EVENTS_CPP 1 [=[container_->editor_style(id, editing_);]=] [=[twp::editor_style(id, editing_, container_->editor_style(id, editing_));]=])
tw_probe(EVENTS_CPP 1 [=[container_->editor_style(editing_.id, editing_);]=] [=[twp::editor_style(editing_.id, editing_, container_->editor_style(editing_.id, editing_));]=])
tw_probe(EVENTS_CPP 1 [=[!container_->image_of(*src, image, w, h)]=] [=[!twp::image_of(*src, image, w, h, container_->image_of(*src, image, w, h))]=])
tw_probe(EVENTS_CPP 1 [=[!container_->image_ready(id, w, h)]=] [=[!twp::image_ready(id, w, h, container_->image_ready(id, w, h))]=])
tw_probe(EVENTS_CPP 1 [=[        std::pair<float, float> size{float(content.width), float(content.height)};]=] [=[        std::pair<float, float> size{float(content.width), float(content.height)};
        twp::content(id, size);]=])
tw_probe(EVENTS_CPP 1 [=[    for (auto& ev : pending_events_) out.push_back(std::move(ev));]=] [=[    twp::pending(pending_events_);
    for (auto& ev : pending_events_) out.push_back(std::move(ev));]=])

# Calls with no answer, and builds: recorded where the input's code makes them, the rest of the renderer's left alone.
tw_probe(EVENTS_CPP 3 [=[selection_changed();]=] [=[TWP_CMD('C', selection_changed());]=])
tw_probe(EVENTS_CPP 6 [=[restyled_by_input();]=] [=[TWP_ACT('y', restyled_by_input());]=])
tw_probe(EVENTS_CPP 5 [=[settle(out);]=] [=[TWP_ACT('s', settle(out));]=])
tw_probe(EVENTS_CPP 2 [=[settle(pending_events_);]=] [=[TWP_ACT('p', settle(pending_events_));]=])
tw_probe(EVENTS_CPP 5 [=[rebuild(true);]=] [=[TWP_ACT('r', rebuild(true));]=])
tw_probe(EVENTS_CPP 10 [=[rebuild();]=] [=[TWP_ACT('R', rebuild());]=])

# The input's code the rest of the renderer calls: a call op, and markers for the calls a replay makes itself.
tw_probe(EVENTS_CPP 1 [=[    } else if (kind == "call") {
        const std::string& method = str("method");]=] [=[    } else if (kind == "call") {
        const std::string& method = str("method");
        TWP_PORT;]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::flush_pending(std::vector<Json>& out) {]=] [=[void Renderer::flush_pending(std::vector<Json>& out) {
    TWP_ENTRY('F');]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::flush_moves(std::vector<Json>& out) {]=] [=[void Renderer::flush_moves(std::vector<Json>& out) {
    TWP_ENTRY('M');]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::check_hold(std::vector<Json>& out) {]=] [=[void Renderer::check_hold(std::vector<Json>& out) {
    TWP_ENTRY('H');]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::load_images(std::vector<Json>& out) {]=] [=[void Renderer::load_images(std::vector<Json>& out) {
    TWP_ENTRY('G');]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::refresh_editor() {]=] [=[void Renderer::refresh_editor() {
    TWP_ENTRY('E');]=])
tw_probe(EVENTS_CPP 1 [=[void Renderer::paint_selection(DisplayList& list, int64_t layer, bool tint) const {]=] [=[void Renderer::paint_selection(DisplayList& list, int64_t layer, bool tint) const {
    TWP_PAINT(list, layer, tint);]=])
tw_probe(EVENTS_CPP 1 [=[    if (selectable_text())
        for (const TextRun& r : text_runs()) r.drawn = false;]=] [=[    {
        TWP_ENTRY('D');
        if (selectable_text())
            for (const TextRun& r : text_runs()) r.drawn = false;
    }]=])
set(${cpp_var} "${EVENTS_CPP}" PARENT_SCOPE)
set(${h_var} "${EVENTS_H}" PARENT_SCOPE)
endfunction()
