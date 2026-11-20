// The probes events_oracle compiles into its copy of core/src/renderer.cpp (events_oracle.cmake puts them at the input code's calls): the tape rust/crates/events/tests/common/tape.rs reads.
#pragma once

#include "display_list.h"
#include "wire.h"
#include "twidget/json.h"

#include <cstdint>
#include <cstring>
#include <optional>
#include <string>
#include <string_view>
#include <tuple>
#include <vector>

namespace twp {

// Depth counts the input's own code on the stack: its questions are recorded there, a host's call into it is a marker at depth 0, and a build it asks for runs at depth 0 again.
struct Tape {
    std::string bytes;
    int depth = 0;
    bool on = false;
};
inline Tape tape;

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
inline bool question(char tag) {
    if (!tape.on) return false;
    // A question outside the input's code: a call the oracle does not mark.
    if (tape.depth == 0) u8('X');
    u8(uint8_t(tag));
    return true;
}

inline void sync(bool stale, bool layout_stopped, bool frozen, const std::string& reason, bool fatal, const std::vector<std::string>& violations, bool paint_stopped) {
    u8(stale);
    u8(layout_stopped);
    u8(frozen);
    str(reason);
    u8(fatal);
    raw(uint32_t(violations.size()));
    for (const std::string& v : violations) str(v);
    u8(paint_stopped);
}

struct Port {
    Port() { tape.depth++; }
    ~Port() { tape.depth--; }
};

struct Entry {
    template <class... A> Entry(char tag, const A&... state) {
        if (tape.on && tape.depth == 0) {
            u8(uint8_t(tag));
            sync(state...);
        }
        tape.depth++;
    }
    ~Entry() { tape.depth--; }
};

// paint_selection from the paint: the list it is given, then a digest of the list it leaves.
struct Paint {
    const twlite::DisplayList& list;
    bool marked = false;
    template <class... A> Paint(const twlite::DisplayList& l, int64_t layer, bool tint, const A&... state) : list(l) {
        if (tape.on && tape.depth == 0) {
            marked = true;
            u8('Q');
            sync(state...);
            raw(layer);
            u8(tint);
            std::string bytes;
            twlite::wire::encode(list, twlite::wire::Caps{}, bytes);
            str(bytes);
        }
        tape.depth++;
    }
    ~Paint() {
        tape.depth--;
        if (!marked) return;
        std::string bytes;
        bool encoded = twlite::wire::encode(list, twlite::wire::Caps{}, bytes);
        uint64_t h = 1469598103934665603ull;
        for (unsigned char c : bytes) h = (h ^ c) * 1099511628211ull;
        u8('q');
        u8(encoded);
        raw(uint32_t(list.size()));
        raw(h);
    }
};

inline void begin_action(char tag) {
    u8('A');
    u8(uint8_t(tag));
}

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
inline bool layer(int64_t id, double now, const twlite::Affine& shown, float opacity, bool ok) {
    if (question('L')) {
        raw(id);
        raw(now);
        u8(ok);
        if (ok) {
            for (float v : {shown.a, shown.b, shown.c, shown.d, shown.e, shown.f}) raw(v);
            raw(opacity);
        }
    }
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
inline void pending(const std::vector<twidget::Json>& events) {
    if (!question('P')) return;
    raw(uint32_t(events.size()));
    for (const twidget::Json& m : events) str(m.dump());
}
inline const std::string& cursor(const std::string& c) {
    if (question('c')) str(c);
    return c;
}
inline double width_of(double w) {
    if (question('W')) raw(w);
    return w;
}
inline void command(char tag) {
    question('C');
    if (tape.on) u8(uint8_t(tag));
}

} // namespace twp

#define TWP_STATE stale_, layout_stopped_, frozen_, frozen_reason_, frozen_fatal_, pending_violations_, paint_stopped_
#define TWP_PORT twp::Port twp_port
#define TWP_ENTRY(TAG) twp::Entry twp_entry(TAG, TWP_STATE)
#define TWP_PAINT(LIST, LAYER, TINT) twp::Paint twp_paint(LIST, LAYER, TINT, TWP_STATE)
#define TWP_ACT(TAG, CALL)                                  \
    do {                                                    \
        if (twp::tape.on && twp::tape.depth > 0) {          \
            twp::begin_action(TAG);                         \
            const int twp_depth = twp::tape.depth;          \
            twp::tape.depth = 0;                            \
            CALL;                                           \
            twp::tape.depth = twp_depth;                    \
            twp::u8('Z');                                   \
            twp::sync(TWP_STATE);                           \
        } else {                                            \
            CALL;                                           \
        }                                                   \
    } while (0)
#define TWP_CMD(TAG, CALL) \
    do {                   \
        CALL;              \
        twp::command(TAG); \
    } while (0)
#define TWP_HIT(X, Y, P) twp::hit_answer(X, Y, P, widget_id_at(X, Y, P))
