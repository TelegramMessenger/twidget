#pragma once

#include "wire.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <map>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

using namespace twlite;

namespace {

uint32_t bits(const float& f) {
    uint32_t b = 0;
    std::memcpy(&b, &f, sizeof b);
    return b;
}

std::string hex(uint64_t v) {
    char s[24];
    std::snprintf(s, sizeof s, "0x%llx", static_cast<unsigned long long>(v));
    return s;
}

using Batch = std::vector<CanvasOp>;
using Clips = std::vector<CanvasClip>;
using Pixels = std::vector<uint8_t>;

/// Where a leaf of a value is: a member, an array slot or a list item under its parent.
struct At {
    enum Kind { Member, Slot, Item };
    const At* up = nullptr;
    const char* name = "";
    size_t index = 0;
    Kind kind = Member;
};

/// Its path, with list items numbered or, for coverage, all `[]`.
std::string where(const At& at, bool numbered) {
    const std::string up = at.up ? where(*at.up, numbered) : std::string();
    if (at.kind == At::Member) return up.empty() ? std::string(at.name) : up + "." + at.name;
    if (at.kind == At::Item && !numbered) return up + "[]";
    return up + "[" + std::to_string(at.index) + "]";
}

[[maybe_unused]] bool in_item(const At& at) {
    for (const At* a = &at; a; a = a->up)
        if (a->kind == At::Item) return true;
    return false;
}

std::string_view view(const Pixels& v) {
    return v.empty() ? std::string_view() : std::string_view(reinterpret_cast<const char*>(v.data()), v.size());
}

bool same_bytes(std::string_view a, std::string_view b) {
    if (a.size() != b.size()) return false;
    for (size_t i = 0; i < a.size(); i++)
        if (a[i] != b[i]) return false;
    return true;
}

/// Compares two values leaf by leaf, floats by their bits, and names the first leaf that differs.
struct Equal {
    int differences = 0;
    std::string first;

    void differ(const At& at, const std::string& a, const std::string& b) {
        if (!differences++) first = where(at, true) + ": " + a + " against " + b;
    }
    void f32(const At& at, const float& a, const float& b) {
        if (bits(a) != bits(b)) differ(at, hex(bits(a)), hex(bits(b)));
    }
    void integer(const At& at, uint64_t a, uint64_t b) {
        if (a != b) differ(at, hex(a), hex(b));
    }
    void raw(const At& at, std::string_view a, std::string_view b) {
        if (!same_bytes(a, b)) differ(at, std::to_string(a.size()) + " bytes", std::to_string(b.size()) + " bytes");
    }
};

template <class V, class T> void walk(V& v, const At& at, const std::vector<T>& a, const std::vector<T>& b);
template <class V, class T> void walk(V& v, const At& at, const std::shared_ptr<const T>& a, const std::shared_ptr<const T>& b);

template <class V> void walk(V& v, const At& at, const float& a, const float& b) {
    v.f32(at, a, b);
}

template <class V> void walk(V& v, const At& at, const Path::Op& a, const Path::Op& b) {
    v.integer(at, a, b);
}

template <class V, size_t N> void walk(V& v, const At& at, const float (&a)[N], const float (&b)[N]) {
    for (size_t i = 0; i < N; i++) v.f32(At{&at, "", i, At::Slot}, a[i], b[i]);
}

template <class V> void walk(V& v, const At& at, const Pixels& a, const Pixels& b) {
    v.raw(At{&at, "bytes"}, view(a), view(b));
}

template <class V> void walk(V& v, const At& at, const Color& a, const Color& b) {
    v.integer(At{&at, "r"}, a.r, b.r);
    v.integer(At{&at, "g"}, a.g, b.g);
    v.integer(At{&at, "b"}, a.b, b.b);
    v.integer(At{&at, "a"}, a.a, b.a);
}

template <class V> void walk(V& v, const At& at, const Rect& a, const Rect& b) {
    v.f32(At{&at, "x"}, a.x, b.x);
    v.f32(At{&at, "y"}, a.y, b.y);
    v.f32(At{&at, "w"}, a.w, b.w);
    v.f32(At{&at, "h"}, a.h, b.h);
}

template <class V> void walk(V& v, const At& at, const Radii& a, const Radii& b) {
    v.f32(At{&at, "tlx"}, a.tlx, b.tlx);
    v.f32(At{&at, "tly"}, a.tly, b.tly);
    v.f32(At{&at, "trx"}, a.trx, b.trx);
    v.f32(At{&at, "try_"}, a.try_, b.try_);
    v.f32(At{&at, "brx"}, a.brx, b.brx);
    v.f32(At{&at, "bry"}, a.bry, b.bry);
    v.f32(At{&at, "blx"}, a.blx, b.blx);
    v.f32(At{&at, "bly"}, a.bly, b.bly);
}

template <class V> void walk(V& v, const At& at, const Stop& a, const Stop& b) {
    v.f32(At{&at, "offset"}, a.offset, b.offset);
    walk(v, At{&at, "color"}, a.color, b.color);
}

template <class V> void walk(V& v, const At& at, const Filter& a, const Filter& b) {
    v.integer(At{&at, "kind"}, a.kind, b.kind);
    v.f32(At{&at, "amount"}, a.amount, b.amount);
    v.f32(At{&at, "x"}, a.x, b.x);
    v.f32(At{&at, "y"}, a.y, b.y);
    walk(v, At{&at, "color"}, a.color, b.color);
}

template <class V> void walk(V& v, const At& at, const Path& a, const Path& b) {
    walk(v, At{&at, "ops"}, a.ops, b.ops);
    walk(v, At{&at, "points"}, a.points, b.points);
    v.integer(At{&at, "fill"}, a.fill, b.fill);
    v.integer(At{&at, "evenodd"}, a.evenodd, b.evenodd);
    walk(v, At{&at, "fill_color"}, a.fill_color, b.fill_color);
    v.integer(At{&at, "gradient"}, a.gradient, b.gradient);
    v.f32(At{&at, "x1"}, a.x1, b.x1);
    v.f32(At{&at, "y1"}, a.y1, b.y1);
    v.f32(At{&at, "x2"}, a.x2, b.x2);
    v.f32(At{&at, "y2"}, a.y2, b.y2);
    v.f32(At{&at, "r"}, a.r, b.r);
    v.f32(At{&at, "fx"}, a.fx, b.fx);
    v.f32(At{&at, "fy"}, a.fy, b.fy);
    walk(v, At{&at, "stops"}, a.stops, b.stops);
    v.integer(At{&at, "stroke"}, a.stroke, b.stroke);
    walk(v, At{&at, "stroke_color"}, a.stroke_color, b.stroke_color);
    v.f32(At{&at, "stroke_width"}, a.stroke_width, b.stroke_width);
    v.integer(At{&at, "line_cap"}, a.line_cap, b.line_cap);
    v.integer(At{&at, "line_join"}, a.line_join, b.line_join);
    v.f32(At{&at, "miter_limit"}, a.miter_limit, b.miter_limit);
    walk(v, At{&at, "dashes"}, a.dashes, b.dashes);
    v.f32(At{&at, "dash_offset"}, a.dash_offset, b.dash_offset);
    v.integer(At{&at, "stroke_gradient"}, a.stroke_gradient, b.stroke_gradient);
    walk(v, At{&at, "sg"}, a.sg, b.sg);
    walk(v, At{&at, "stroke_stops"}, a.stroke_stops, b.stroke_stops);
}

template <class V> void walk(V& v, const At& at, const Command& a, const Command& b) {
    v.integer(At{&at, "kind"}, int(a.kind), int(b.kind));
    walk(v, At{&at, "rect"}, a.rect, b.rect);
    walk(v, At{&at, "radii"}, a.radii, b.radii);
    walk(v, At{&at, "clip"}, a.clip, b.clip);
    v.integer(At{&at, "has_clip"}, a.has_clip, b.has_clip);
    walk(v, At{&at, "color"}, a.color, b.color);
    v.f32(At{&at, "width"}, a.width, b.width);
    v.integer(At{&at, "font"}, a.font, b.font);
    v.raw(At{&at, "text"}, a.text, b.text);
    v.f32(At{&at, "x"}, a.x, b.x);
    v.f32(At{&at, "y"}, a.y, b.y);
    v.integer(At{&at, "image"}, a.image, b.image);
    v.integer(At{&at, "canvas"}, a.canvas, b.canvas);
    v.f32(At{&at, "ax"}, a.ax, b.ax);
    v.f32(At{&at, "ay"}, a.ay, b.ay);
    v.f32(At{&at, "bx"}, a.bx, b.bx);
    v.f32(At{&at, "by"}, a.by, b.by);
    walk(v, At{&at, "stops"}, a.stops, b.stops);
    walk(v, At{&at, "path"}, a.path, b.path);
    walk(v, At{&at, "filter"}, a.filter, b.filter);
    v.f32(At{&at, "opacity"}, a.opacity, b.opacity);
    walk(v, At{&at, "matrix"}, a.matrix, b.matrix);
    v.integer(At{&at, "hidden"}, a.hidden, b.hidden);
    v.integer(At{&at, "inset"}, a.inset, b.inset);
    v.integer(At{&at, "key"}, a.key, b.key);
}

template <class V> void walk(V& v, const At& at, const Geometry& a, const Geometry& b) {
    walk(v, At{&at, "ops"}, a.ops, b.ops);
    walk(v, At{&at, "points"}, a.points, b.points);
}

template <class V> void walk(V& v, const At& at, const CanvasPaint& a, const CanvasPaint& b) {
    v.integer(At{&at, "kind"}, a.kind, b.kind);
    walk(v, At{&at, "color"}, a.color, b.color);
    walk(v, At{&at, "args"}, a.args, b.args);
    walk(v, At{&at, "stops"}, a.stops, b.stops);
}

template <class V> void walk(V& v, const At& at, const CanvasClip& a, const CanvasClip& b) {
    walk(v, At{&at, "path"}, a.path, b.path);
    v.integer(At{&at, "evenodd"}, a.evenodd, b.evenodd);
}

template <class V> void walk(V& v, const At& at, const CanvasOp& a, const CanvasOp& b) {
    v.integer(At{&at, "kind"}, a.kind, b.kind);
    walk(v, At{&at, "path"}, a.path, b.path);
    v.integer(At{&at, "evenodd"}, a.evenodd, b.evenodd);
    walk(v, At{&at, "paint"}, a.paint, b.paint);
    walk(v, At{&at, "matrix"}, a.matrix, b.matrix);
    v.f32(At{&at, "line_width"}, a.line_width, b.line_width);
    v.integer(At{&at, "line_cap"}, a.line_cap, b.line_cap);
    v.integer(At{&at, "line_join"}, a.line_join, b.line_join);
    v.f32(At{&at, "miter_limit"}, a.miter_limit, b.miter_limit);
    walk(v, At{&at, "dashes"}, a.dashes, b.dashes);
    v.f32(At{&at, "dash_offset"}, a.dash_offset, b.dash_offset);
    v.f32(At{&at, "alpha"}, a.alpha, b.alpha);
    v.integer(At{&at, "composite"}, uint8_t(a.composite), uint8_t(b.composite));
    walk(v, At{&at, "shadow_color"}, a.shadow_color, b.shadow_color);
    v.f32(At{&at, "shadow_blur"}, a.shadow_blur, b.shadow_blur);
    v.f32(At{&at, "shadow_x"}, a.shadow_x, b.shadow_x);
    v.f32(At{&at, "shadow_y"}, a.shadow_y, b.shadow_y);
    walk(v, At{&at, "clips"}, a.clips, b.clips);
    v.integer(At{&at, "font"}, a.font, b.font);
    v.raw(At{&at, "text"}, a.text, b.text);
    v.f32(At{&at, "x"}, a.x, b.x);
    v.f32(At{&at, "y"}, a.y, b.y);
    v.f32(At{&at, "scale_x"}, a.scale_x, b.scale_x);
    v.f32(At{&at, "text_dx"}, a.text_dx, b.text_dx);
    v.integer(At{&at, "stroke_text"}, a.stroke_text, b.stroke_text);
    v.integer(At{&at, "image"}, a.image, b.image);
    v.integer(At{&at, "canvas"}, a.canvas, b.canvas);
    v.f32(At{&at, "sx"}, a.sx, b.sx);
    v.f32(At{&at, "sy"}, a.sy, b.sy);
    v.f32(At{&at, "sw"}, a.sw, b.sw);
    v.f32(At{&at, "sh"}, a.sh, b.sh);
    v.f32(At{&at, "dx"}, a.dx, b.dx);
    v.f32(At{&at, "dy"}, a.dy, b.dy);
    v.f32(At{&at, "dw"}, a.dw, b.dw);
    v.f32(At{&at, "dh"}, a.dh, b.dh);
    v.integer(At{&at, "smoothing"}, a.smoothing, b.smoothing);
    walk(v, At{&at, "rgba"}, a.rgba, b.rgba);
    v.integer(At{&at, "w"}, a.w, b.w);
    v.integer(At{&at, "h"}, a.h, b.h);
}

template <class V> void walk(V& v, const At& at, const FontMetrics& a, const FontMetrics& b) {
    v.f32(At{&at, "ascent"}, a.ascent, b.ascent);
    v.f32(At{&at, "descent"}, a.descent, b.descent);
    v.f32(At{&at, "height"}, a.height, b.height);
    v.f32(At{&at, "x_height"}, a.x_height, b.x_height);
    v.f32(At{&at, "zero_width"}, a.zero_width, b.zero_width);
    v.f32(At{&at, "em_ascent"}, a.em_ascent, b.em_ascent);
    v.f32(At{&at, "em_descent"}, a.em_descent, b.em_descent);
}

template <class V> void walk(V& v, const At& at, const wire::Damage& a, const wire::Damage& b) {
    v.integer(At{&at, "valid"}, a.has_value(), b.has_value());
    const std::vector<Rect> none;
    walk(v, At{&at, "rects"}, a ? *a : none, b ? *b : none);
}

/// A list's length, then each item against the other's or, past its end, against a default item.
template <class V, class T> void walk(V& v, const At& at, const std::vector<T>& a, const std::vector<T>& b) {
    v.integer(At{&at, "size"}, a.size(), b.size());
    const T none{};
    for (size_t i = 0; i < std::max(a.size(), b.size()); i++)
        walk(v, At{&at, "", i, At::Item}, i < a.size() ? a[i] : none, i < b.size() ? b[i] : none);
}

/// Whether a payload is there, then what it holds; who else holds it is sharing()'s.
template <class V, class T> void walk(V& v, const At& at, const std::shared_ptr<const T>& a, const std::shared_ptr<const T>& b) {
    v.integer(at, a != nullptr, b != nullptr);
    const T none{};
    walk(v, at, a ? *a : none, b ? *b : none);
}

/// Whether `b` is `a` leaf by leaf; `why` names the first leaf that differs.
template <class T> bool same(const T& a, const T& b, std::string& why) {
    Equal e;
    walk(e, At{}, a, b);
    why = e.first;
    return e.differences == 0;
}

/// Each user of a payload as the index of the payload's first user, -1 without one: two values share alike when these match.
template <class T, class Get> std::vector<long> users(const std::vector<T>& values, Get get) {
    std::vector<long> out;
    std::map<const void*, long> first;
    for (size_t i = 0; i < values.size(); i++) {
        const void* p = get(values[i]);
        out.push_back(p ? first.try_emplace(p, long(i)).first->second : -1);
    }
    return out;
}

using Sharing = std::vector<std::vector<long>>;

Sharing sharing(const DisplayList& list) {
    return {users(list, [](const Command& c) -> const void* { return c.path.get(); }),
        users(list, [](const Command& c) -> const void* { return c.filter.get(); })};
}

Sharing sharing(const Batch& ops) {
    return {users(ops, [](const CanvasOp& o) -> const void* { return o.clips.get(); }),
        users(ops, [](const CanvasOp& o) -> const void* { return o.rgba.get(); })};
}

[[maybe_unused]] Sharing sharing(const wire::Damage&) {
    return {};
}

[[maybe_unused]] Sharing sharing(const FontMetrics&) {
    return {};
}

} // namespace
