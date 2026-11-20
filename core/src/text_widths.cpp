// LayoutWidths: a layout's unknown widths asked for in one Backend::text_widths call each, in the order the layout meets them, within what the face caches keep.
#include "text_widths.h"

#ifdef TW_LITEHTML_TWIDGET
#include <litehtml/el_text.h>

#include <algorithm>
#include <map>
#include <set>
#include <utility>

namespace twlite {
namespace {

/// One Backend call for widths no face has, in the order asked, each face within what its cache keeps.
struct Asking {
    std::vector<TextInFont> texts;
    std::vector<FaceWidths*> faces;
    std::set<std::pair<const FaceWidths*, std::string_view>> asked;
    std::map<const FaceWidths*, size_t> room;
    size_t pool = 0;

    void add(FaceWidths* face, std::string_view text) {
        text = text.substr(0, text.find('\0'));  // every reader measures the text as a C string
        if (!face || text.size() > MAX_WIDTH_KEY || face->widths.count(text) || asked.count({face, text})) return;
        // Past what the caches keep, a batch would forget its first answers before they are read.
        size_t& left = room.try_emplace(face, MAX_WIDTH_ENTRIES - std::min(face->widths.size(), MAX_WIDTH_ENTRIES)).first->second;
        if (!left || text.size() > MAX_WIDTH_BYTES - pool) return;
        left--;
        pool += text.size();
        asked.emplace(face, text);
        texts.push_back(TextInFont{face->handle, std::string(text)});
        faces.push_back(face);
    }
    void send(Backend& backend) {
        if (texts.empty()) return;
        const std::vector<float> widths = backend.text_widths(texts);
        for (size_t i = 0; i < texts.size() && i < widths.size(); i++) faces[i]->remember(texts[i].text, widths[i]);
    }
};

} // namespace

void LayoutWidths::before(const std::vector<litehtml::el_text*>& owed, const std::vector<litehtml::el_text*>& shown) {
    breakable_.clear();
    Asking asking{.pool = pool_};
    for (litehtml::el_text* text : owed) asking.add(fonts_.face(text->measured_font()), text->display_text());
    for (litehtml::el_text* text : shown) {
        const litehtml::uint_ptr font = text->css().get_font();
        FaceWidths* face = fonts_.face(font);
        if (!face) continue;
        for (std::string_view unit : fonts_.painted_units(font, text->display_text())) asking.add(face, unit);
        if (text->breaks_in_line() || text->breaks_at_narrowest()) breakable_.push_back(text);
    }
    asking.send(backend_);
    for (litehtml::el_text* text : owed) {
        const FaceWidths* face = fonts_.face(text->measured_font());
        if (face && face->widths.count(text->display_text())) text->measure_width();
    }
    for (litehtml::el_text* text : owed) text->measure_width();
}

void LayoutWidths::character(litehtml::uint_ptr font, std::string_view text) {
    if (breakable_.empty() || text.size() > MAX_WIDTH_KEY) return;
    const FaceWidths* face = fonts_.face(font);
    if (face && !face->widths.count(text)) measure_breaks();
}

void LayoutWidths::measure_breaks() {
    Asking asking{.pool = pool_};
    for (litehtml::el_text* text : std::exchange(breakable_, {})) {
        FaceWidths* face = fonts_.face(text->css().get_font());
        const std::string& t = text->display_text();
        if (!face || t.empty()) continue;
        // Every character of a run of spaces, as a preserved tab shows, is the one space.
        if (t.find_first_not_of(' ') == std::string::npos) {
            asking.add(face, " ");
            continue;
        }
        const std::vector<size_t> points = fonts_.break_points(t);
        for (size_t i = 1; i < points.size(); i++) asking.add(face, std::string_view(t).substr(points[i - 1], points[i] - points[i - 1]));
    }
    asking.send(backend_);
}

} // namespace twlite
#endif
