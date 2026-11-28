// The probes renderer_oracle compiles into its copy of core/src/renderer.cpp (renderer_oracle.cmake puts them at the calls into the document and the container, stage 4.5's among them): the tape rust/crates/renderer/tests/common/tape.rs reads.
#pragma once

#include "display_list.h"
#include "wire.h"
#include "twidget/json.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <string_view>
#include <tuple>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

namespace twp {

// Every answer of the document to the renderer, in order.
struct Tape {
    std::string bytes;
    bool on = false;
};
inline Tape tape;

inline bool recording() {
    return tape.on;
}
inline void u8(uint8_t v) { tape.bytes.push_back(char(v)); }
template <class T> void raw(T v) { tape.bytes.append(reinterpret_cast<const char*>(&v), sizeof v); }
inline void str(std::string_view s) {
    raw(uint32_t(s.size()));
    tape.bytes.append(s);
}
inline void rect(const twlite::Rect& r) {
    raw(r.x);
    raw(r.y);
    raw(r.w);
    raw(r.h);
}
inline uint64_t fnv64(std::string_view v) {
    uint64_t h = 1469598103934665603ull;
    for (unsigned char c : v) h = (h ^ c) * 1099511628211ull;
    return h;
}
inline bool question(char tag) {
    if (!recording()) return false;
    u8(uint8_t(tag));
    return true;
}
inline void step(uint32_t index) {
    if (!tape.on) return;
    u8('S');
    raw(index);
}

// ── the input's questions, as events_probe.h names them ──

inline int64_t hit_answer(double x, double y, twlite::Rect* p, int64_t id) {
    if (question('h')) {
        raw(x);
        raw(y);
        u8(p != nullptr);
        raw(id);
        if (p) rect(*p);
    }
    return id;
}
inline twlite::Rect box(int64_t id, twlite::Rect r) {
    if (question('b')) {
        raw(id);
        rect(r);
    }
    return r;
}
inline bool exists(int64_t id, bool yes) {
    if (question('x')) {
        raw(id);
        u8(yes);
    }
    return yes;
}
inline void touch_action(int64_t id, const std::string* value) {
    if (question('a')) {
        raw(id);
        u8(value != nullptr);
        if (value) str(*value);
    }
}
inline bool user_select(int64_t id, bool yes) {
    if (question('u')) {
        raw(id);
        u8(yes);
    }
    return yes;
}
inline bool mouse(int action, double x, double y, bool hovers, bool redraw) {
    if (question('m')) {
        raw(int32_t(action));
        raw(x);
        raw(y);
        u8(hovers);
        u8(redraw);
    }
    return redraw;
}
inline bool leave(bool redraw) {
    if (question('v')) u8(redraw);
    return redraw;
}
template <class Key> void runs_key(const Key& key) {
    if (question('K')) {
        raw(uint64_t(uintptr_t(std::get<0>(key))));
        raw(uint64_t(std::get<1>(key)));
        raw(uint64_t(std::get<2>(key)));
        raw(uint64_t(std::get<3>(key)));
    }
}
template <class Runs> void runs(const Runs& list) {
    if (!question('R')) return;
    raw(uint32_t(list.size()));
    for (const auto& r : list) {
        raw(int64_t(r.owner));
        raw(uint64_t(r.offset));
        str(r.text);
        rect(r.box);
        raw(uint64_t(r.font));
        raw(int32_t(r.block));
        u8(r.selectable);
        u8(r.line_break);
    }
}
inline float width(const char* text, uintptr_t font, float w) {
    if (question('w')) {
        str(text);
        raw(uint64_t(font));
        raw(w);
    }
    return w;
}
inline bool font_info(uintptr_t font, int& handle, float& descent, float& letter, float& word, bool ok) {
    if (question('f')) {
        raw(uint64_t(font));
        u8(ok);
        if (ok) {
            raw(int32_t(handle));
            raw(descent);
            raw(letter);
            raw(word);
        }
    }
    return ok;
}
// The renderer's own (stage 4.5): ported, not answered.
inline bool layer(int64_t, double, const twlite::Affine&, float, bool ok) {
    return ok;
}
template <class Key> void selectable_key(const Key& key) {
    if (question('k')) {
        raw(uint64_t(uintptr_t(key.first)));
        raw(uint64_t(key.second));
    }
}
inline void selectable(bool yes) {
    if (question('j')) u8(yes);
}
template <class Spec> bool editor_style(int64_t id, const Spec& s, bool applied) {
    if (question('e')) {
        raw(id);
        u8(applied);
        if (applied) {
            rect(s.frame);
            str(s.font_family);
            raw(s.font_size);
            raw(int32_t(s.font_weight));
            u8(s.italic);
            u8(s.color.r);
            u8(s.color.g);
            u8(s.color.b);
            u8(s.color.a);
            raw(int32_t(s.align));
            raw(int32_t(s.font));
            u8(s.has_clip);
            rect(s.clip);
        }
    }
    return applied;
}
inline bool image_of(const std::string& src, int& image, float& w, float& h, bool ok) {
    if (question('i')) {
        str(src);
        u8(ok);
        if (ok) {
            raw(int32_t(image));
            raw(w);
            raw(h);
        }
    }
    return ok;
}
inline bool image_ready(int id, float w, float h, bool changed) {
    if (question('I')) {
        raw(int32_t(id));
        raw(w);
        raw(h);
        u8(changed);
    }
    return changed;
}
inline void content(int64_t id, const std::pair<float, float>& size) {
    if (question('p')) {
        raw(id);
        raw(size.first);
        raw(size.second);
    }
}
inline const std::string& cursor(const std::string& c) {
    if (question('c')) str(c);
    return c;
}
// The renderer's own: ported, not answered.
inline void pending(const std::vector<twidget::Json>&) {}
inline double width_of(double w) { return w; }

// ── the document's life ──

inline void count(uint64_t elements) {
    if (question('n')) raw(elements);
}
inline void render(float width, uint64_t budget) {
    if (question('r')) {
        raw(width);
        raw(budget);
    }
}
inline bool exhausted(bool yes) {
    if (question('E')) u8(yes);
    return yes;
}
inline uint64_t spent(uint64_t boxes) {
    if (question('s')) raw(boxes);
    return boxes;
}
inline bool viewport_units(const std::string& html, bool yes) {
    if (question('z')) {
        raw(fnv64(html));
        raw(uint32_t(html.size()));
        u8(yes);
    }
    return yes;
}
inline bool update(const std::string& html, bool ok, const bool& changed) {
    if (question('D')) {
        raw(fnv64(html));
        raw(uint32_t(html.size()));
        u8(ok);
        u8(changed);
    }
    return ok;
}
inline void media_changed() { question('M'); }
inline bool paint_only(bool yes) {
    if (question('y')) u8(yes);
    return yes;
}
inline void created(const std::string& html, const std::string& master, bool made) {
    if (question('C')) {
        raw(fnv64(html));
        raw(uint32_t(html.size()));
        raw(fnv64(master));
        raw(uint32_t(master.size()));
        u8(made);
    }
}
inline void command(char tag) { question(tag); }
inline double height(double h) {
    if (question('H')) raw(h);
    return h;
}
inline bool root(bool yes) {
    if (question('o')) u8(yes);
    return yes;
}
template <class F> bool reads_height(F f) {
    bool yes = f();
    if (question('q')) u8(yes);
    return yes;
}
inline size_t fonts(size_t n) {
    if (question('F')) raw(uint64_t(n));
    return n;
}
template <class El> const char* twid(const El& el) {
    const char* a = el ? el->get_attr("data-twid") : nullptr;
    return a ? a : "";
}
template <class Change> bool holes(const std::vector<Change>& changes, bool ok, const bool& changed) {
    if (question('J')) {
        raw(uint32_t(changes.size()));
        for (const Change& c : changes) {
            raw(int64_t(std::strtoll(twid(c.owner), nullptr, 10)));
            str(c.name);
            str(c.value);
            u8(c.text);
        }
        u8(ok);
        u8(changed);
    }
    return ok;
}
template <class Patch> bool styles(const std::vector<Patch>& patches, bool ok, const bool& changed) {
    if (question('Z')) {
        raw(uint32_t(patches.size()));
        for (const Patch& p : patches) {
            raw(int64_t(std::strtoll(twid(p.first), nullptr, 10)));
            str(p.second);
        }
        u8(ok);
        u8(changed);
    }
    return ok;
}
template <class Usage> Usage usage(Usage u) {
    if (question('N')) {
        raw(uint64_t(u.names));
        raw(uint64_t(u.bytes));
    }
    return u;
}
inline const uint64_t no_generation = 0;
template <class Map> const Map& declared(const Map& found, const std::vector<std::string>& names, const std::vector<std::string>& gates, const std::vector<int64_t>& always,
                                         const uint64_t& generation) {
    if (question('d')) {
        raw(uint32_t(names.size()));
        for (const auto& n : names) str(n);
        raw(uint32_t(gates.size()));
        for (const auto& g : gates) str(g);
        raw(uint32_t(always.size()));
        for (int64_t a : always) raw(a);
        std::map<int64_t, std::map<std::string, std::string>> sorted;
        for (const auto& [id, values] : found) sorted[id].insert(values.begin(), values.end());
        raw(uint32_t(sorted.size()));
        for (const auto& [id, values] : sorted) {
            raw(id);
            raw(uint32_t(values.size()));
            for (const auto& [k, v] : values) {
                str(k);
                str(v);
            }
        }
        raw(generation);
    }
    return found;
}
template <class Elements, class Box> void edges(const Elements& elements, Box box) {
    if (!question('g')) return;
    std::map<int64_t, twlite::Rect> sorted;
    for (const auto& [id, el] : elements) sorted.emplace(id, box(el));
    raw(uint32_t(sorted.size()));
    for (const auto& [id, r] : sorted) {
        raw(id);
        rect(r);
    }
}
// A violation the container pushes, recorded where it pushes it: the next answer brings it.
inline void push(std::vector<std::string>& violations, std::string v) {
    if (question('V')) str(v);
    violations.push_back(std::move(v));
}

// ── the queries' reads ──

inline twlite::Rect visual(int64_t id, twlite::Rect r) {
    if (question('1')) {
        raw(id);
        rect(r);
    }
    return r;
}
inline std::vector<twlite::Rect> lines(int64_t id, std::vector<twlite::Rect> list) {
    if (question('2')) {
        raw(id);
        raw(uint32_t(list.size()));
        for (const auto& r : list) rect(r);
    }
    return list;
}
inline twlite::Rect transformed(int64_t id, const twlite::Rect& in, twlite::Rect r) {
    if (question('3')) {
        raw(id);
        rect(in);
        rect(r);
    }
    return r;
}
inline twlite::Rect border(int64_t id, twlite::Rect r) {
    if (question('4')) {
        raw(id);
        rect(r);
    }
    return r;
}
inline void borders(int64_t id, bool has, float l, float t, float r, float b) {
    if (question('5')) {
        raw(id);
        u8(has);
        if (has) {
            raw(l);
            raw(t);
            raw(r);
            raw(b);
        }
    }
}
inline void offsets(int64_t id, double left, double top, int64_t parent) {
    if (question('6')) {
        raw(id);
        raw(left);
        raw(top);
        raw(parent);
    }
}
inline void extent(int64_t id, bool scroller, float pad_right, float pad_bottom, float right, float bottom) {
    if (question('7')) {
        raw(id);
        u8(scroller);
        raw(pad_right);
        raw(pad_bottom);
        raw(right);
        raw(bottom);
    }
}
inline void textarea(int64_t id, bool has, float w, float h, float l, float t, float r, float b) {
    if (question('8')) {
        raw(id);
        u8(has);
        if (has) {
            for (float v : {w, h, l, t, r, b}) raw(v);
        }
    }
}
inline twidget::Json computed(int64_t id, const std::string& pseudo, const std::vector<std::pair<std::string, std::string>>& animated, twidget::Json answer) {
    if (question('9')) {
        raw(id);
        str(pseudo);
        raw(uint32_t(animated.size()));
        for (const auto& [k, v] : animated) {
            str(k);
            str(v);
        }
        str(answer.dump());
    }
    return answer;
}
inline std::string custom(int64_t id, const std::string& name, std::string answer) {
    if (question('0')) {
        raw(id);
        str(name);
        str(answer);
    }
    return answer;
}
template <class Position> bool svg_text(int64_t id, const Position& text, bool ok) {
    if (question('!')) {
        raw(id);
        u8(ok);
        if (ok) {
            for (float v : {float(text.x), float(text.y), float(text.width), float(text.height)}) raw(v);
        }
    }
    return ok;
}
inline bool svg_geometry(int64_t id, bool ok) {
    if (question('@')) {
        raw(id);
        u8(ok);
    }
    return ok;
}
inline float path_length(int64_t id, float at, float length, float x, float y) {
    if (question('#')) {
        raw(id);
        raw(at);
        raw(length);
        raw(x);
        raw(y);
    }
    return length;
}
inline bool bounds(int64_t id, bool ok, const float& x0, const float& y0, const float& x1, const float& y1) {
    if (question('$')) {
        raw(id);
        u8(ok);
        if (ok) {
            raw(x0);
            raw(y0);
            raw(x1);
            raw(y1);
        }
    }
    return ok;
}

// ── stage 4.5's questions: the elements, boxes, clips, coverage and paint of the container ──

template <class El> El element(int64_t id, El el) {
    if (question('x')) {
        raw(id);
        u8(el != nullptr);
    }
    return el;
}
inline bool local(int64_t id, bool yes) {
    if (question('A')) {
        raw(id);
        u8(yes);
    }
    return yes;
}
// A set of litehtml elements by their widget ids, sorted.
template <class Set> void id_set(const Set& set) {
    std::vector<int64_t> ids;
    for (const auto* e : set) ids.push_back(std::strtoll(twid(e), nullptr, 10));
    std::sort(ids.begin(), ids.end());
    raw(uint32_t(ids.size()));
    for (int64_t id : ids) raw(id);
}
// A list of litehtml elements by their widget ids, in order.
template <class List> void id_list(const List& list) {
    raw(uint32_t(list.size()));
    for (const auto& e : list) raw(int64_t(std::strtoll(twid(e), nullptr, 10)));
}
template <class Set> bool candidate(int64_t id, const Set* movers, const twlite::Rect& box, bool ok) {
    if (question('[')) {
        raw(id);
        u8(movers != nullptr);
        if (movers) id_set(*movers);
        u8(ok);
        if (ok) rect(box);
    }
    return ok;
}
inline bool candidate(int64_t id, std::nullptr_t, const twlite::Rect& box, bool ok) {
    if (question('[')) {
        raw(id);
        u8(0);
        u8(ok);
        if (ok) rect(box);
    }
    return ok;
}
inline void radii(const twlite::Radii& r) {
    for (float v : {r.tlx, r.tly, r.trx, r.try_, r.brx, r.bry, r.blx, r.bly}) raw(v);
}
inline bool clip(int64_t id, const bool& has, const twlite::Rect& r, const twlite::Radii& k, bool ok) {
    if (question(']')) {
        raw(id);
        u8(ok);
        if (ok) {
            u8(has);
            if (has) {
                rect(r);
                radii(k);
            }
        }
    }
    return ok;
}
// A display list in wire form, and whether the wire takes it.
inline void list(const twlite::DisplayList& l) {
    std::string bytes;
    u8(twlite::wire::encode(l, twlite::wire::Caps{}, bytes) ? 1 : 0);
    str(bytes);
}
inline bool tile(int64_t id, const twlite::Rect& t, const twlite::Rect& area, const twlite::Rect& c, const twlite::Radii& k, const twlite::DisplayList& content, bool ok) {
    if (question('~')) {
        raw(id);
        u8(ok);
        if (ok) {
            rect(t);
            rect(area);
            rect(c);
            radii(k);
            list(content);
        }
    }
    return ok;
}
template <class Map> Map declared(int64_t id, Map found) {
    if (question('{')) {
        raw(id);
        std::map<std::string, std::string> sorted(found.begin(), found.end());
        raw(uint32_t(sorted.size()));
        for (const auto& [k, v] : sorted) {
            str(k);
            str(v);
        }
    }
    return found;
}
inline size_t order(int64_t id, size_t index) {
    if (question('<')) {
        raw(id);
        raw(uint64_t(index));
    }
    return index;
}
template <class Set> bool covered(int64_t id, const twlite::Rect& area, const Set& ignore, bool yes) {
    if (question('?')) {
        raw(id);
        rect(area);
        id_set(ignore);
        u8(yes);
    }
    return yes;
}
template <class Set, class Found> Found obscurers(int64_t id, const twlite::Rect& area, const Set& ignore, size_t limit, Found found) {
    if (question('&')) {
        raw(id);
        rect(area);
        id_set(ignore);
        raw(uint64_t(limit));
        u8(found.has_value());
        if (found) {
            raw(uint32_t(found->size()));
            for (int64_t v : *found) raw(v);
        }
    }
    return found;
}
template <class List> twlite::DisplayList drawn(const List& skip, const List& backgrounds, twlite::DisplayList l) {
    if (question('+')) {
        id_list(skip);
        id_list(backgrounds);
        list(l);
    }
    return l;
}
template <class List> twlite::DisplayList layer_drawn(int64_t id, const List& skip, const twlite::Rect& bounds, twlite::DisplayList l) {
    if (question('*')) {
        raw(id);
        id_list(skip);
        rect(bounds);
        list(l);
    }
    return l;
}
inline twlite::DisplayList plain(twlite::DisplayList l) {
    if (question('=')) list(l);
    return l;
}
template <class Key> Key paint_key(Key key) {
    if (question('^')) {
        raw(uint64_t(uintptr_t(std::get<0>(key))));
        raw(uint32_t(std::get<1>(key)));
        raw(uint64_t(std::get<2>(key)));
    }
    return key;
}
inline int64_t hit_within(int64_t id, float x, float y, twlite::Rect* p, int64_t hit) {
    if (question('/')) {
        raw(id);
        raw(x);
        raw(y);
        u8(p != nullptr);
        raw(hit);
        if (p && hit) rect(*p);
    }
    return hit;
}

} // namespace twp

// The input's probes (events_oracle.cmake's patterns) where the renderer is ported whole: its builds and markers are plain calls.
#define TWP_PORT ((void)0)
#define TWP_ENTRY(TAG) ((void)0)
#define TWP_PAINT(LIST, LAYER, TINT) ((void)0)
#define TWP_ACT(TAG, CALL) \
    do {                   \
        CALL;              \
    } while (0)
#define TWP_CMD(TAG, CALL) \
    do {                   \
        CALL;              \
    } while (0)
#define TWP_HIT(X, Y, P) widget_id_at(X, Y, P)
