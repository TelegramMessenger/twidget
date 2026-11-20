// twidget::wire alone, linked without litehtml or the renderer: samples that cover every kind and member come back bit for bit and shared as they were, their bytes follow docs/TWIDGET_CORE.md "Binary forms", hostile bytes are refused with the output untouched, and no decode asks the allocator for more than a bound linear in its bytes.
#include "wire.h"

#include "decoder_checks.h"
#include "wire_compare.h"

#include <algorithm>
#include <climits>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <deque>
#include <initializer_list>
#include <iterator>
#include <memory>
#include <new>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>

using namespace twlite;

namespace {

void put_bits(float& f, uint32_t b) {
    std::memcpy(&f, &b, sizeof f);
}

/// Float bits a copy through a float could change: NaNs with payloads and either sign, a signalling NaN, -0, both infinities, a subnormal, the largest float.
constexpr uint32_t SPECIALS[] = {0x7fc12345, 0xffc54321, 0x7f800abc, 0x80000000, 0x7f800000, 0xff800000, 0x00000003, 0x7f7fffff};
constexpr size_t SPECIAL_COUNT = std::size(SPECIALS);
/// 1/3, which the JSON form's hundredths would change.
constexpr uint32_t THIRD = 0x3eaaaaab;

/// What a sample sets apart from a default value: the leaves that differ (list items as []), and every float's bits by the kind of place it is in.
struct Cover {
    std::set<std::string> leaves;
    std::set<uint32_t> scalars, slots, items;

    void f32(const At& at, const float& a, const float& b) {
        (in_item(at) ? items : at.kind == At::Slot ? slots : scalars).insert(bits(a));
        if (bits(a) != bits(b)) leaves.insert(where(at, false));
    }
    void integer(const At& at, uint64_t a, uint64_t b) {
        if (a != b) leaves.insert(where(at, false));
    }
    void raw(const At& at, std::string_view a, std::string_view b) {
        if (!same_bytes(a, b)) leaves.insert(where(at, false));
    }
};

/// The objects an output holds, to tell that a refusal kept them rather than equal copies.
std::vector<const void*> held(const DisplayList& list) {
    std::vector<const void*> out{list.data()};
    for (const Command& c : list) {
        out.push_back(c.path.get());
        out.push_back(c.filter.get());
    }
    return out;
}

std::vector<const void*> held(const Batch& ops) {
    std::vector<const void*> out{ops.data()};
    for (const CanvasOp& o : ops) {
        out.push_back(o.clips.get());
        out.push_back(o.rgba.get());
    }
    return out;
}

std::vector<const void*> held(const wire::Damage& damage) {
    return {damage ? damage->data() : nullptr};
}

std::vector<const void*> held(const FontMetrics&) {
    return {};
}

/// A used output for a decode to replace or leave alone; built afresh for each comparison, so that no copy shares its payloads.
void make_sentinel(DisplayList& list) {
    auto path = std::make_shared<Path>();
    path->ops = {Path::Move, Path::Line, Path::Close};
    path->points = {1, 2, 30, 40};
    path->fill = true;
    path->fill_color = {200, 100, 50, 25};
    auto filter = std::make_shared<Filters>(2);
    (*filter)[0].amount = 3;
    (*filter)[1] = Filter{Filter::DropShadow, 1.5f, -2, 4, {9, 9, 9, 99}};
    list.resize(3);
    list[0].kind = Cmd::Text;
    list[0].text = "a sentinel that a refused decode leaves alone";
    list[0].font = 7;
    list[0].x = 1.25f;
    list[0].key = 99;
    list[0].path = path;
    list[0].filter = filter;
    list[1].kind = Cmd::Path;
    list[1].path = path;
    list[1].filter = filter;
    list[1].opacity = 0.75f;
    list[2].rect = {1, 2, 3, 4};
    list[2].color = {1, 2, 3, 4};
}

void make_sentinel(Batch& ops) {
    auto clips = std::make_shared<Clips>(1);
    (*clips)[0].path.ops = {Path::Move, Path::Line, Path::Line, Path::Close};
    (*clips)[0].path.points = {0, 0, 8, 0, 8, 8};
    auto rgba = std::make_shared<Pixels>(16, uint8_t(0x5a));
    ops.resize(2);
    ops[0].path.ops = {Path::Move, Path::Line};
    ops[0].path.points = {1, 1, 5, 5};
    ops[0].paint.color = {10, 20, 30, 40};
    ops[0].clips = clips;
    ops[0].rgba = rgba;
    ops[0].text = "sentinel";
    ops[1].kind = CanvasOp::PutImage;
    ops[1].clips = clips;
    ops[1].rgba = rgba;
    ops[1].dx = 3;
    ops[1].w = 2;
    ops[1].h = 2;
}

void make_sentinel(wire::Damage& damage) {
    damage = std::vector<Rect>{{1, 2, 3, 4}, {5, 6, 7, 8}};
}

void make_sentinel(FontMetrics& m) {
    m.ascent = 11;
    m.descent = 12;
    m.height = 13;
    m.x_height = 14;
    m.zero_width = 15;
    m.em_ascent = 16;
    m.em_descent = 17;
}

template <class T> bool encoded(const T& value, const wire::Caps& caps, std::string& out) {
    return wire::encode(value, caps, out);
}

bool encoded(const FontMetrics& value, const wire::Caps&, std::string& out) {
    wire::encode(value, out);
    return true;
}

/// Every decode these checks make goes through here, and so through the allocation scope.
template <class T> bool decoded(std::string_view bytes, const wire::Caps& caps, T& out) {
    return in_scope(bytes, [&] { return wire::decode(bytes, caps, out); });
}

bool decoded(std::string_view bytes, const wire::Caps&, FontMetrics& out) {
    return in_scope(bytes, [&] { return wire::decode(bytes, out); });
}

/// `value` encodes and decodes back the same leaf by leaf and shared alike: into an empty output and a used one, after a prefix, and owning what it holds once the bytes are gone.
template <class T> void round_trip(const std::string& what, const T& value, const wire::Caps& caps = {}) {
    std::string bytes;
    if (!encoded(value, caps, bytes)) {
        check(false, what + ": encodes");
        return;
    }
    std::string why;
    T fresh;
    check(decoded(bytes, caps, fresh) && same(value, fresh, why), what + ": decodes the same " + why);
    check(sharing(fresh) == sharing(value), what + ": decodes shared alike");
    T used;
    make_sentinel(used);
    check(decoded(bytes, caps, used) && same(value, used, why) && sharing(used) == sharing(value), what + ": replaces a used output " + why);
    std::string again;
    check(encoded(fresh, caps, again) && again == bytes, what + ": what it decodes to encodes to the same bytes");
    const std::string prefix("an earlier value\0\xff", 18);
    std::string after = prefix;
    check(encoded(value, caps, after) && after.compare(0, prefix.size(), prefix) == 0 && after.compare(prefix.size(), std::string::npos, bytes) == 0,
        what + ": appends after what the string held");
    T suffix;
    check(decoded(std::string_view(after).substr(prefix.size()), caps, suffix) && same(value, suffix, why), what + ": decodes from after a prefix " + why);
    auto buffer = std::make_unique<char[]>(bytes.size());
    std::memcpy(buffer.get(), bytes.data(), bytes.size());
    T owned;
    const bool ok = decoded(std::string_view(buffer.get(), bytes.size()), caps, owned);
    std::memset(buffer.get(), 0xa5, bytes.size());
    check(ok && same(value, owned, why), what + ": keeps its own copy once the bytes are overwritten " + why);
    buffer.reset();
    check(same(value, owned, why) && sharing(owned) == sharing(value), what + ": keeps its own copy once the bytes are freed " + why);
}

/// `bytes` refused, and the output the decoder was given still the same value holding the same objects.
template <class T> void refused(const std::string& what, std::string_view bytes, const wire::Caps& caps = {}) {
    T out;
    make_sentinel(out);
    T expected;
    make_sentinel(expected);
    const std::vector<const void*> before = held(out);
    const bool accepted = decoded(bytes, caps, out);
    std::string why;
    check(!accepted, what + ": refused");
    check(same(expected, out, why) && held(out) == before, what + ": the output untouched " + why);
}

/// Decodes into one output in turn: each success replaces it, each refusal leaves what the last success put there.
template <class T> void alternate(const std::string& what, const T& a, const T& b) {
    std::string x, y, why;
    if (!encoded(a, {}, x) || !encoded(b, {}, y)) {
        check(false, what + ": encodes");
        return;
    }
    T out;
    check(decoded(x, {}, out) && same(a, out, why), what + ": decodes the first " + why);
    std::vector<const void*> kept = held(out);
    check(!decoded(std::string_view(y).substr(0, y.size() - 1), {}, out), what + ": refuses the second cut short");
    check(same(a, out, why) && held(out) == kept, what + ": keeps the first " + why);
    check(decoded(y, {}, out) && same(b, out, why) && sharing(out) == sharing(b), what + ": decodes the second " + why);
    kept = held(out);
    check(!decoded(x + '\0', {}, out), what + ": refuses the first with a byte after it");
    check(same(b, out, why) && held(out) == kept, what + ": keeps the second " + why);
    check(decoded(x, {}, out) && same(a, out, why) && sharing(out) == sharing(a), what + ": decodes the first again " + why);
}

/// Bytes as docs/TWIDGET_CORE.md lays them out, put together here without the codec.
struct Bytes {
    std::string s;

    Bytes& u8(uint32_t v) {
        s.push_back(char(v & 0xff));
        return *this;
    }
    Bytes& u32(uint32_t v) {
        for (int shift = 0; shift < 32; shift += 8) u8(v >> shift);
        return *this;
    }
    Bytes& u64(uint64_t v) {
        for (int shift = 0; shift < 64; shift += 8) u8(uint32_t((v >> shift) & 0xff));
        return *this;
    }
    Bytes& i32(int v) { return u32(uint32_t(v)); }
    Bytes& f32(const float& v) { return u32(bits(v)); }
    /// `count` floats from `first` up by ones.
    Bytes& run(float first, int count) {
        for (int i = 0; i < count; i++) {
            const float f = first + float(i);
            f32(f);
        }
        return *this;
    }
    Bytes& color(uint32_t r, uint32_t g, uint32_t b, uint32_t a) { return u8(r).u8(g).u8(b).u8(a); }
    Bytes& head(uint32_t form) { return u8(form).u8(1); }
};

/// Bytes from hex digits, spaces between them ignored.
std::string unhex(std::string_view digits) {
    std::string out;
    int high = -1;
    for (char c : digits) {
        const int d = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : -1;
        if (d < 0) continue;
        if (high < 0) {
            high = d;
        } else {
            out.push_back(char(high * 16 + d));
            high = -1;
        }
    }
    return out;
}

size_t first_difference(const std::string& a, const std::string& b) {
    size_t i = 0;
    while (i < a.size() && i < b.size() && a[i] == b[i]) i++;
    return i;
}

/// `value` encodes to exactly `expected` and `expected` decodes back to it.
template <class T> void golden(const std::string& what, const T& value, const std::string& expected) {
    std::string bytes;
    const bool written = encoded(value, {}, bytes);
    check(written && bytes == expected, what + ": encodes to the documented bytes, the first difference at byte " + std::to_string(first_difference(bytes, expected)));
    T out;
    std::string why;
    check(decoded(expected, {}, out) && same(value, out, why), what + ": the documented bytes decode to it " + why);
    check(sharing(out) == sharing(value), what + ": the documented bytes decode shared alike");
}

template <size_t N> void spread(float (&to)[N], float first, float step) {
    for (size_t i = 0; i < N; i++) to[i] = first + step * float(i);
}

/// A Path with each of its members set and every op, its numbers counted from `base`.
std::shared_ptr<Path> full_path(float base) {
    auto p = std::make_shared<Path>();
    p->ops = {Path::Move, Path::Line, Path::Quad, Path::Cubic, Path::Close};
    p->points.resize(14);
    for (size_t i = 0; i < p->points.size(); i++) p->points[i] = base + 0.5f * float(i);
    p->fill = true;
    p->evenodd = true;
    p->fill_color = {1, 2, 3, 4};
    p->gradient = 2;
    p->x1 = base + 1;
    p->y1 = base + 2;
    p->x2 = base + 3;
    p->y2 = base + 4;
    p->r = base + 5;
    p->fx = base + 6;
    p->fy = base + 7;
    p->stops = {Stop{0.25f, {5, 6, 7, 8}}, Stop{0.75f, {9, 10, 11, 12}}};
    p->stroke = true;
    p->stroke_color = {13, 14, 15, 16};
    p->stroke_width = base + 0.25f;
    p->line_cap = 1;
    p->line_join = 2;
    p->miter_limit = base + 8;
    p->dashes = {base + 9, base + 10};
    p->dash_offset = base + 11;
    p->stroke_gradient = 1;
    spread(p->sg, base + 12, 1);
    p->stroke_stops = {Stop{0.5f, {17, 18, 19, 20}}};
    return p;
}

/// A Path whose floats are the special patterns, in its scalars, its sg slots and its lists alike.
std::shared_ptr<Path> special_path() {
    auto p = std::make_shared<Path>();
    p->ops = {Path::Move, Path::Cubic};
    p->points.resize(SPECIAL_COUNT);
    p->dashes.resize(SPECIAL_COUNT);
    p->stops.resize(SPECIAL_COUNT);
    p->stroke_stops.resize(SPECIAL_COUNT);
    for (size_t i = 0; i < SPECIAL_COUNT; i++) {
        put_bits(p->points[i], SPECIALS[i]);
        put_bits(p->dashes[i], SPECIALS[SPECIAL_COUNT - 1 - i]);
        put_bits(p->stops[i].offset, SPECIALS[i]);
        put_bits(p->stroke_stops[i].offset, SPECIALS[SPECIAL_COUNT - 1 - i]);
    }
    float* const scalars[] = {&p->x1, &p->y1, &p->x2, &p->y2, &p->r, &p->fx, &p->fy, &p->stroke_width};
    for (size_t i = 0; i < SPECIAL_COUNT; i++) put_bits(*scalars[i], SPECIALS[i]);
    put_bits(p->miter_limit, SPECIALS[2]);
    put_bits(p->dash_offset, SPECIALS[3]);
    for (size_t i = 0; i < std::size(p->sg); i++) put_bits(p->sg[i], SPECIALS[SPECIAL_COUNT - 1 - i]);
    return p;
}

/// One filter of each kind, every member set, numbers from `base`.
std::shared_ptr<Filters> every_filter(float base) {
    auto list = std::make_shared<Filters>(size_t(Filter::DropShadow) + 1);
    for (size_t k = 0; k < list->size(); k++) {
        Filter& f = (*list)[k];
        f.kind = Filter::Kind(k);
        f.amount = base + float(k);
        f.x = base - float(k);
        f.y = base + 0.5f * float(k) + 0.25f;
        f.color = {uint8_t(k + 1), uint8_t(k + 2), uint8_t(k + 3), uint8_t(100 + k)};
    }
    return list;
}

std::shared_ptr<Filters> special_filters() {
    auto list = std::make_shared<Filters>(SPECIAL_COUNT);
    for (size_t i = 0; i < SPECIAL_COUNT; i++) {
        Filter& f = (*list)[i];
        f.kind = Filter::Kind(i);
        put_bits(f.amount, SPECIALS[i]);
        put_bits(f.x, SPECIALS[(i + 3) % SPECIAL_COUNT]);
        put_bits(f.y, SPECIALS[(i + 5) % SPECIAL_COUNT]);
    }
    return list;
}

/// Every member of a command set, on ClipPop, which paints none of them.
void every_member(Command& c) {
    c.kind = Cmd::ClipPop;
    c.rect = {1.25f, 2.5f, 3.75f, 5};
    c.radii = {0.5f, 1, 1.5f, 2, 2.5f, 3, 3.5f, 4};
    c.clip = {6, 7, 8, 9};
    c.has_clip = true;
    c.color = {10, 20, 30, 40};
    c.width = 1.0f / 3;
    c.font = 11;
    c.text = "text a ClipPop does not paint";
    c.x = 12;
    c.y = 13;
    c.image = 14;
    c.canvas = 15;
    c.ax = 16;
    c.ay = 17;
    c.bx = 18;
    c.by = 19;
    c.stops = {Stop{0.25f, {1, 2, 3, 4}}, Stop{0.75f, {5, 6, 7, 8}}};
    c.path = full_path(100);
    c.filter = every_filter(200);
    c.opacity = 0.5f;
    spread(c.matrix, 2, 0.5f);
    c.hidden = true;
    c.inset = true;
    c.key = 0x0123456789abcdefULL;
}

/// A clip list of two clips with every op between them, numbers from `base`.
std::shared_ptr<Clips> clip_list(float base) {
    auto clips = std::make_shared<Clips>(2);
    Geometry& first = (*clips)[0].path;
    first.ops = {Path::Move, Path::Line, Path::Quad, Path::Cubic, Path::Close};
    first.points.resize(14);
    for (size_t i = 0; i < first.points.size(); i++) first.points[i] = base + float(i);
    (*clips)[1].path.ops = {Path::Move, Path::Line, Path::Close};
    (*clips)[1].path.points = {base, -base, base + 0.125f, base};
    (*clips)[1].evenodd = true;
    return clips;
}

/// 1024 pixel bytes holding every byte value, in an order `step` (odd) sets.
std::shared_ptr<Pixels> every_byte(size_t step) {
    auto px = std::make_shared<Pixels>(1024);
    for (size_t i = 0; i < px->size(); i++) (*px)[i] = uint8_t(i * step + 3);
    return px;
}

/// Every member of an op set, on Reset, which paints none of them.
void every_member(CanvasOp& o) {
    o.kind = CanvasOp::Reset;
    o.path.ops = {Path::Move, Path::Line, Path::Quad, Path::Cubic, Path::Close};
    o.path.points.resize(14);
    for (size_t i = 0; i < o.path.points.size(); i++) o.path.points[i] = 0.5f + float(i);
    o.evenodd = true;
    o.paint.kind = CanvasPaint::Conic;
    o.paint.color = {1, 2, 3, 4};
    spread(o.paint.args, 0.25f, 1.5f);
    o.paint.stops = {Stop{0.125f, {5, 6, 7, 8}}, Stop{0.875f, {9, 10, 11, 12}}};
    spread(o.matrix, 2, 0.5f);
    o.line_width = 1.0f / 3;
    o.line_cap = 2;
    o.line_join = 1;
    o.miter_limit = 3.5f;
    o.dashes = {4, 2, 1};
    o.dash_offset = 0.75f;
    o.alpha = 0.5f;
    o.composite = Composite::Xor;
    o.shadow_color = {13, 14, 15, 16};
    o.shadow_blur = 5;
    o.shadow_x = 6;
    o.shadow_y = -7;
    o.clips = clip_list(20);
    o.font = 8;
    o.text = "text a Reset does not paint";
    o.x = 9;
    o.y = 10;
    o.scale_x = 0.25f;
    o.text_dx = 11;
    o.stroke_text = true;
    o.image = 12;
    o.canvas = 13;
    o.sx = 14;
    o.sy = 15;
    o.sw = 16;
    o.sh = 17;
    o.dx = 18;
    o.dy = 19;
    o.dw = 20;
    o.dh = 21;
    o.smoothing = false;
    o.rgba = every_byte(7);
    o.w = 32;
    o.h = 32;
}

template <class T> struct Named {
    std::string name;
    T value;
};
template <class T> using Samples = std::deque<Named<T>>;

template <class T> T& add(Samples<T>& samples, const char* name) {
    samples.push_back(Named<T>{name, T()});
    return samples.back().value;
}

template <class T> const T& named(const Samples<T>& samples, const std::string& name) {
    for (const Named<T>& s : samples)
        if (s.name == name) return s.value;
    check(false, "a sample named " + name);
    static const T none{};
    return none;
}

const char* const EVERY_MEMBER = "every member, on a kind that paints none of them";
const char* const SPECIAL_FLOATS = "special floats in scalars, arrays and lists";
const char* const PAYLOADS = "payloads: none, empty, shared, equal but apart, referred back to";

Samples<DisplayList> list_samples() {
    Samples<DisplayList> out;
    add(out, "empty list");

    DisplayList& kinds = add(out, "every command kind");
    kinds.resize(size_t(Cmd::Backdrop) + 1);
    for (size_t k = 0; k < kinds.size(); k++) {
        kinds[k].kind = Cmd(k);
        kinds[k].rect = {float(k) + 0.5f, 1, 2, 3};
        kinds[k].key = k + 1;
    }

    every_member(add(out, EVERY_MEMBER).emplace_back());

    DisplayList& specials = add(out, SPECIAL_FLOATS);
    specials.resize(2);
    Command& s = specials[0];
    s.kind = Cmd::Text;
    float* const scalars[] = {&s.width, &s.x, &s.y, &s.ax, &s.ay, &s.bx, &s.by, &s.opacity};
    float* const rects[] = {&s.rect.x, &s.rect.y, &s.rect.w, &s.rect.h, &s.clip.x, &s.clip.y, &s.clip.w, &s.clip.h};
    float* const radii[] = {&s.radii.tlx, &s.radii.tly, &s.radii.trx, &s.radii.try_, &s.radii.brx, &s.radii.bry, &s.radii.blx, &s.radii.bly};
    s.stops.resize(SPECIAL_COUNT);
    for (size_t i = 0; i < SPECIAL_COUNT; i++) {
        put_bits(*scalars[i], SPECIALS[i]);
        put_bits(*rects[i], SPECIALS[SPECIAL_COUNT - 1 - i]);
        put_bits(*radii[i], SPECIALS[(i + 4) % SPECIAL_COUNT]);
        put_bits(s.stops[i].offset, SPECIALS[i]);
        s.stops[i].color = {uint8_t(i), uint8_t(2 * i), 0, 0};
    }
    for (size_t i = 0; i < std::size(s.matrix); i++) put_bits(s.matrix[i], SPECIALS[i]);
    s.path = special_path();
    specials[1].kind = Cmd::FilterPush;
    specials[1].filter = special_filters();

    DisplayList& ints = add(out, "ints and keys at their extremes");
    ints.resize(4);
    ints[0].kind = Cmd::Image;
    ints[0].font = INT_MIN;
    ints[0].image = INT_MAX;
    ints[0].canvas = -1;
    ints[0].key = UINT64_MAX;
    ints[1].kind = Cmd::Text;
    ints[1].font = INT_MAX;
    ints[1].image = INT_MIN;
    ints[1].canvas = INT_MIN;
    ints[1].key = 0x8000000000000001ULL;
    ints[2].kind = Cmd::Image;
    ints[2].font = -1;
    ints[2].image = -2;
    ints[2].canvas = INT_MAX;
    ints[2].key = 0;
    auto enums = std::make_shared<Path>();
    enums->gradient = INT_MIN;
    enums->line_cap = INT_MAX;
    enums->line_join = -1;
    enums->stroke_gradient = 3;
    ints[3].kind = Cmd::Path;
    ints[3].path = enums;
    ints[3].key = 1;

    DisplayList& text = add(out, "text as bytes: a NUL, invalid UTF-8, a long run");
    text.resize(3);
    for (Command& c : text) c.kind = Cmd::Text;
    text[0].text = std::string("nul\0inside", 10);
    text[1].text = "\xff\xfe\xc3\x28 is not UTF-8";
    text[2].text.resize(300);
    for (size_t i = 0; i < text[2].text.size(); i++) text[2].text[i] = char(255 - i % 256);

    DisplayList& shared = add(out, PAYLOADS);
    shared.resize(8);
    const auto empty_path = std::make_shared<Path>();
    const auto path = full_path(1), equal_path = full_path(1), other_path = full_path(50);
    const std::shared_ptr<const Path> paths[] = {nullptr, empty_path, path, path, equal_path, other_path, empty_path, other_path};
    const auto empty_filters = std::make_shared<Filters>();
    const auto filters = every_filter(1), equal_filters = every_filter(1), other_filters = every_filter(9);
    const std::shared_ptr<const Filters> filter_users[] = {filters, nullptr, empty_filters, filters, equal_filters, other_filters, other_filters, empty_filters};
    for (size_t i = 0; i < shared.size(); i++) {
        shared[i].kind = i % 2 ? Cmd::Path : Cmd::FilterPush;
        shared[i].path = paths[i];
        shared[i].filter = filter_users[i];
    }
    return out;
}

Samples<Batch> batch_samples() {
    Samples<Batch> out;
    add(out, "empty batch");

    Batch& kinds = add(out, "every op kind");
    kinds.resize(size_t(CanvasOp::PutImage) + 1);
    for (size_t k = 0; k < kinds.size(); k++) {
        kinds[k].kind = CanvasOp::Kind(k);
        kinds[k].x = float(k) + 0.25f;
    }

    every_member(add(out, EVERY_MEMBER).emplace_back());

    Batch& paints = add(out, "every paint kind");
    paints.resize(size_t(CanvasPaint::Conic) + 1);
    for (size_t k = 0; k < paints.size(); k++) {
        CanvasPaint& p = paints[k].paint;
        p.kind = CanvasPaint::Kind(k);
        p.color = {uint8_t(k), 50, 60, 70};
        spread(p.args, float(k) + 1, 0.5f);
        p.stops = {Stop{0.5f, {1, 2, 3, uint8_t(k)}}};
    }

    Batch& composites = add(out, "every composite");
    composites.resize(size_t(Composite::Luminosity) + 1);
    for (size_t k = 0; k < composites.size(); k++) composites[k].composite = Composite(k);

    Batch& settings = add(out, "line, image, shadow and matrix settings beside their defaults");
    settings.resize(6);
    settings[1].kind = CanvasOp::Stroke;
    settings[1].line_width = 3;
    settings[1].line_cap = 1;
    settings[1].line_join = 2;
    settings[1].miter_limit = 2;
    settings[1].dashes = {1, 2, 3};
    settings[1].dash_offset = 0.5f;
    settings[2].kind = CanvasOp::Image;
    settings[2].image = 3;
    settings[2].sx = 1;
    settings[2].sy = 2;
    settings[2].sw = 30;
    settings[2].sh = 40;
    settings[2].dx = 5;
    settings[2].dy = 6;
    settings[2].dw = 70;
    settings[2].dh = 80;
    settings[2].smoothing = false;
    settings[3].kind = CanvasOp::Image;
    settings[3].canvas = 4;
    spread(settings[3].matrix, 0.5f, 0.25f);
    settings[4].kind = CanvasOp::PutImage;
    settings[4].rgba = std::make_shared<Pixels>(16, uint8_t(0x80));
    settings[4].dx = 1;
    settings[4].dy = 1;
    settings[4].w = 2;
    settings[4].h = 2;
    settings[5].shadow_color = {0, 0, 0, 128};
    settings[5].shadow_blur = 4;
    settings[5].shadow_x = 2;
    settings[5].shadow_y = -2;
    settings[5].alpha = 0.25f;

    Batch& specials = add(out, SPECIAL_FLOATS);
    specials.resize(1);
    CanvasOp& s = specials[0];
    s.kind = CanvasOp::Stroke;
    s.paint.kind = CanvasPaint::Linear;
    s.path.ops = {Path::Move, Path::Line, Path::Line, Path::Line};
    s.path.points.resize(SPECIAL_COUNT);
    s.dashes.resize(SPECIAL_COUNT);
    s.paint.stops.resize(SPECIAL_COUNT);
    auto clips = std::make_shared<Clips>(1);
    (*clips)[0].path.ops = {Path::Move, Path::Line, Path::Line, Path::Close};
    (*clips)[0].path.points.resize(SPECIAL_COUNT);
    float* const scalars[] = {&s.line_width, &s.miter_limit, &s.dash_offset, &s.alpha, &s.shadow_blur, &s.shadow_x, &s.shadow_y, &s.x};
    float* const image[] = {&s.sx, &s.sy, &s.sw, &s.sh, &s.dx, &s.dy, &s.dw, &s.dh};
    for (size_t i = 0; i < SPECIAL_COUNT; i++) {
        put_bits(*scalars[i], SPECIALS[i]);
        put_bits(*image[i], SPECIALS[SPECIAL_COUNT - 1 - i]);
        put_bits(s.path.points[i], SPECIALS[i]);
        put_bits(s.dashes[i], SPECIALS[SPECIAL_COUNT - 1 - i]);
        put_bits(s.paint.stops[i].offset, SPECIALS[i]);
        put_bits((*clips)[0].path.points[i], SPECIALS[(i + 2) % SPECIAL_COUNT]);
    }
    for (size_t i = 0; i < std::size(s.matrix); i++) {
        put_bits(s.matrix[i], SPECIALS[i]);
        put_bits(s.paint.args[i], SPECIALS[i + 2]);
    }
    put_bits(s.y, SPECIALS[1]);
    put_bits(s.scale_x, SPECIALS[2]);
    put_bits(s.text_dx, SPECIALS[3]);
    s.clips = clips;

    Batch& shared = add(out, PAYLOADS);
    shared.resize(8);
    const auto empty_clips = std::make_shared<Clips>();
    const auto clip = clip_list(1), equal_clip = clip_list(1), other_clip = clip_list(40);
    const std::shared_ptr<const Clips> clip_users[] = {nullptr, empty_clips, clip, clip, equal_clip, other_clip, empty_clips, other_clip};
    const auto empty_pixels = std::make_shared<Pixels>();
    const auto pixels = every_byte(1), equal_pixels = every_byte(1), other_pixels = every_byte(5);
    const std::shared_ptr<const Pixels> pixel_users[] = {pixels, nullptr, empty_pixels, pixels, equal_pixels, other_pixels, other_pixels, empty_pixels};
    for (size_t i = 0; i < shared.size(); i++) {
        shared[i].kind = i % 2 ? CanvasOp::PutImage : CanvasOp::Fill;
        shared[i].clips = clip_users[i];
        shared[i].rgba = pixel_users[i];
    }
    return out;
}

Samples<wire::Damage> damage_samples() {
    Samples<wire::Damage> out;
    add(out, "the whole frame");
    add(out, "nothing moved").emplace();
    std::vector<Rect>& rects = add(out, "rects, special floats among them").emplace(5);
    rects[0] = {1.5f, 2.25f, 30, 40};
    rects[1] = {-8, 0.125f, 1.0f / 3, 7};
    rects[2] = {100, 200, 0.5f, 0.75f};
    for (size_t i = 0; i < SPECIAL_COUNT; i++) {
        Rect& r = rects[3 + i / 4];
        float* const fields[] = {&r.x, &r.y, &r.w, &r.h};
        put_bits(*fields[i % 4], SPECIALS[i]);
    }
    return out;
}

Samples<FontMetrics> metrics_samples() {
    Samples<FontMetrics> out;
    add(out, "default metrics");
    FontMetrics& ordinary = add(out, "ordinary metrics");
    ordinary.ascent = 12.5f;
    ordinary.descent = 3.25f;
    ordinary.height = 17;
    ordinary.x_height = 6.5f;
    ordinary.zero_width = 7.125f;
    ordinary.em_ascent = 1.0f / 3;
    ordinary.em_descent = 4.75f;
    for (size_t shift : {size_t(0), size_t(7)}) {
        FontMetrics& m = add(out, shift ? "special metrics from the largest float" : "special metrics");
        float* const fields[] = {&m.ascent, &m.descent, &m.height, &m.x_height, &m.zero_width, &m.em_ascent, &m.em_descent};
        for (size_t i = 0; i < std::size(fields); i++) put_bits(*fields[i], SPECIALS[(i + shift) % SPECIAL_COUNT]);
    }
    return out;
}

// The members of each value, read from display_list.h, canvas_ops.h and backend.h rather than from the codec, as the leaves walk() names.
namespace shape {

using Leaves = std::set<std::string>;
using Shape = void (*)(Leaves&, const std::string&);
struct Member {
    const char* name;
    Shape shape;
};

template <size_t N> void members(Leaves& out, const std::string& at, const Member (&list)[N]) {
    for (const Member& m : list) m.shape(out, at.empty() ? std::string(m.name) : at + "." + m.name);
}

void each(Leaves& out, const std::string& at, std::initializer_list<const char*> names) {
    for (const char* n : names) out.insert(at + "." + n);
}

void scalar(Leaves& out, const std::string& at) {
    out.insert(at);
}

void color(Leaves& out, const std::string& at) {
    each(out, at, {"r", "g", "b", "a"});
}

void rect(Leaves& out, const std::string& at) {
    each(out, at, {"x", "y", "w", "h"});
}

void radii(Leaves& out, const std::string& at) {
    each(out, at, {"tlx", "tly", "trx", "try_", "brx", "bry", "blx", "bly"});
}

template <int N> void slots(Leaves& out, const std::string& at) {
    for (int i = 0; i < N; i++) out.insert(at + "[" + std::to_string(i) + "]");
}

/// A list of floats or path ops: its length and its items.
void numbers(Leaves& out, const std::string& at) {
    out.insert(at + ".size");
    out.insert(at + "[]");
}

void stops(Leaves& out, const std::string& at) {
    out.insert(at + ".size");
    out.insert(at + "[].offset");
    color(out, at + "[].color");
}

void geometry(Leaves& out, const std::string& at) {
    numbers(out, at + ".ops");
    numbers(out, at + ".points");
}

constexpr Member PATH[] = {
    {"ops", numbers}, {"points", numbers}, {"fill", scalar}, {"evenodd", scalar}, {"fill_color", color}, {"gradient", scalar},
    {"x1", scalar}, {"y1", scalar}, {"x2", scalar}, {"y2", scalar}, {"r", scalar}, {"fx", scalar}, {"fy", scalar}, {"stops", stops},
    {"stroke", scalar}, {"stroke_color", color}, {"stroke_width", scalar}, {"line_cap", scalar}, {"line_join", scalar},
    {"miter_limit", scalar}, {"dashes", numbers}, {"dash_offset", scalar}, {"stroke_gradient", scalar}, {"sg", slots<7>},
    {"stroke_stops", stops},
};
static_assert(std::size(PATH) == 25);

void path(Leaves& out, const std::string& at) {
    out.insert(at);
    members(out, at, PATH);
}

constexpr Member FILTER[] = {{"amount", scalar}, {"x", scalar}, {"y", scalar}, {"color", color}};
static_assert(std::size(FILTER) == 4);

void filters(Leaves& out, const std::string& at) {
    out.insert(at);
    out.insert(at + ".size");
    out.insert(at + "[].kind");
    members(out, at + "[]", FILTER);
}

constexpr Member COMMAND[] = {
    {"rect", rect}, {"radii", radii}, {"clip", rect}, {"has_clip", scalar}, {"color", color}, {"width", scalar}, {"font", scalar},
    {"text", scalar}, {"x", scalar}, {"y", scalar}, {"image", scalar}, {"canvas", scalar}, {"ax", scalar}, {"ay", scalar},
    {"bx", scalar}, {"by", scalar}, {"stops", stops}, {"path", path}, {"filter", filters}, {"opacity", scalar}, {"matrix", slots<6>},
    {"hidden", scalar}, {"inset", scalar}, {"key", scalar},
};
static_assert(std::size(COMMAND) == 24);

constexpr Member PAINT[] = {{"color", color}, {"args", slots<6>}, {"stops", stops}};
static_assert(std::size(PAINT) == 3);

void paint(Leaves& out, const std::string& at) {
    out.insert(at + ".kind");
    members(out, at, PAINT);
}

void clips(Leaves& out, const std::string& at) {
    out.insert(at);
    out.insert(at + ".size");
    geometry(out, at + "[].path");
    out.insert(at + "[].evenodd");
}

void pixels(Leaves& out, const std::string& at) {
    out.insert(at);
    out.insert(at + ".bytes");
}

constexpr Member OP[] = {
    {"path", geometry}, {"evenodd", scalar}, {"paint", paint}, {"matrix", slots<6>}, {"line_width", scalar}, {"line_cap", scalar},
    {"line_join", scalar}, {"miter_limit", scalar}, {"dashes", numbers}, {"dash_offset", scalar}, {"alpha", scalar},
    {"composite", scalar}, {"shadow_color", color}, {"shadow_blur", scalar}, {"shadow_x", scalar}, {"shadow_y", scalar},
    {"clips", clips}, {"font", scalar}, {"text", scalar}, {"x", scalar}, {"y", scalar}, {"scale_x", scalar}, {"text_dx", scalar},
    {"stroke_text", scalar}, {"image", scalar}, {"canvas", scalar}, {"sx", scalar}, {"sy", scalar}, {"sw", scalar}, {"sh", scalar},
    {"dx", scalar}, {"dy", scalar}, {"dw", scalar}, {"dh", scalar}, {"smoothing", scalar}, {"rgba", pixels}, {"w", scalar},
    {"h", scalar},
};
static_assert(std::size(OP) == 38);

constexpr Member METRICS[] = {
    {"ascent", scalar}, {"descent", scalar}, {"height", scalar}, {"x_height", scalar}, {"zero_width", scalar}, {"em_ascent", scalar},
    {"em_descent", scalar},
};
static_assert(std::size(METRICS) == 7);

template <size_t N> Leaves of(const char* kind, const Member (&list)[N]) {
    Leaves out;
    if (*kind) out.insert(kind);
    members(out, "", list);
    return out;
}

Leaves damage() {
    Leaves out{"valid"};
    out.insert("rects.size");
    rect(out, "rects[]");
    return out;
}

} // namespace shape

/// Every leaf the inventory names some sample sets apart from its default, and the samples set no leaf the inventory lacks.
void inventory(const std::string& what, const shape::Leaves& want, const Cover& cover) {
    for (const std::string& leaf : want) check(cover.leaves.count(leaf) != 0, what + ": a sample sets " + leaf);
    for (const std::string& leaf : cover.leaves) check(want.count(leaf) != 0, what + ": " + leaf + " is a member the inventory knows");
}

void specials_in(const std::string& what, const std::set<uint32_t>& seen) {
    for (uint32_t b : SPECIALS) check(seen.count(b) != 0, what + " hold " + hex(b));
}

void list_coverage(const Samples<DisplayList>& lists) {
    Cover cover;
    std::set<int> kinds, filter_kinds, ops;
    std::set<long long> ids, path_ints;
    std::set<uint64_t> keys;
    bool nul = false, invalid = false, long_text = false;
    for (const auto& s : lists)
        for (const Command& c : s.value) {
            walk(cover, At{}, c, Command{});
            kinds.insert(int(c.kind));
            ids.insert({c.font, c.image, c.canvas});
            keys.insert(c.key);
            nul = nul || c.text.find('\0') != std::string::npos;
            invalid = invalid || c.text.find('\xff') != std::string::npos;
            long_text = long_text || c.text.size() > 64;
            if (c.path) {
                ops.insert(c.path->ops.begin(), c.path->ops.end());
                path_ints.insert({c.path->gradient, c.path->line_cap, c.path->line_join, c.path->stroke_gradient});
            }
            if (c.filter)
                for (const Filter& f : *c.filter) filter_kinds.insert(f.kind);
        }
    check(kinds.size() == size_t(Cmd::Backdrop) + 1, "lists: all 18 command kinds");
    check(filter_kinds.size() == size_t(Filter::DropShadow) + 1, "lists: all 10 filter kinds");
    check(ops.size() == size_t(Path::Close) + 1, "lists: all 5 path ops");
    inventory("lists", shape::of("kind", shape::COMMAND), cover);
    specials_in("lists: scalars", cover.scalars);
    specials_in("lists: array slots", cover.slots);
    specials_in("lists: list items", cover.items);
    check(cover.scalars.count(THIRD) != 0, "lists: a float no hundredth rounds to");
    check(ids.count(INT_MIN) && ids.count(INT_MAX) && ids.count(-1), "lists: ids at INT_MIN, INT_MAX and below 0");
    check(path_ints.count(INT_MIN) && path_ints.count(INT_MAX) && path_ints.count(-1), "lists: a path's enum-like ints at their extremes");
    check(keys.count(0) && keys.count(UINT64_MAX) && keys.count(0x8000000000000001ULL), "lists: keys 0, UINT64_MAX and 0x8000000000000001");
    check(nul && invalid && long_text, "lists: text with a NUL, with invalid UTF-8 and past any small-string buffer");
    std::string why;
    const DisplayList& shared = named(lists, PAYLOADS);
    check(sharing(shared) == Sharing{{-1, 1, 2, 2, 4, 5, 1, 5}, {0, -1, 2, 0, 4, 5, 5, 2}}, "lists: payloads none, shared and referred back to the first and the last");
    check(shared.size() == 8 && shared[2].path != shared[4].path && same(*shared[2].path, *shared[4].path, why), "lists: two equal paths apart " + why);
    check(shared.size() == 8 && shared[0].filter != shared[4].filter && same(*shared[0].filter, *shared[4].filter, why), "lists: two equal filter lists apart " + why);
    check(shared.size() == 8 && same(*shared[1].path, Path{}, why) && shared[2].filter->empty(), "lists: an empty path and an empty filter list " + why);
}

void batch_coverage(const Samples<Batch>& batches) {
    Cover cover;
    std::set<int> kinds, paints, composites, op_ops, clip_ops, bytes;
    for (const auto& s : batches)
        for (const CanvasOp& o : s.value) {
            walk(cover, At{}, o, CanvasOp{});
            kinds.insert(o.kind);
            paints.insert(o.paint.kind);
            composites.insert(int(o.composite));
            op_ops.insert(o.path.ops.begin(), o.path.ops.end());
            if (o.clips)
                for (const CanvasClip& c : *o.clips) clip_ops.insert(c.path.ops.begin(), c.path.ops.end());
            if (o.rgba) bytes.insert(o.rgba->begin(), o.rgba->end());
        }
    check(kinds.size() == size_t(CanvasOp::PutImage) + 1, "batches: all 7 op kinds");
    check(paints.size() == size_t(CanvasPaint::Conic) + 1, "batches: all 4 paint kinds");
    check(composites.size() == size_t(Composite::Luminosity) + 1, "batches: all 26 composites");
    check(op_ops.size() == size_t(Path::Close) + 1, "batches: all 5 path ops in an op's path");
    check(clip_ops.size() == size_t(Path::Close) + 1, "batches: all 5 path ops in a clip's path");
    check(bytes.size() == 256, "batches: pixels with every byte value");
    shape::Leaves want = shape::of("kind", shape::OP);
    inventory("batches", want, cover);
    specials_in("batches: scalars", cover.scalars);
    specials_in("batches: array slots", cover.slots);
    specials_in("batches: list items", cover.items);
    check(cover.scalars.count(THIRD) != 0, "batches: a float no hundredth rounds to");
    std::string why;
    const Batch& shared = named(batches, PAYLOADS);
    check(sharing(shared) == Sharing{{-1, 1, 2, 2, 4, 5, 1, 5}, {0, -1, 2, 0, 4, 5, 5, 2}}, "batches: payloads none, shared and referred back to the first and the last");
    check(shared.size() == 8 && shared[2].clips != shared[4].clips && same(*shared[2].clips, *shared[4].clips, why), "batches: two equal clip lists apart " + why);
    check(shared.size() == 8 && shared[0].rgba != shared[4].rgba && *shared[0].rgba == *shared[4].rgba, "batches: two equal pixel payloads apart");
    check(shared.size() == 8 && shared[1].clips->empty() && shared[2].rgba->empty(), "batches: an empty clip list and empty pixels");
}

void damage_and_metrics_coverage(const Samples<wire::Damage>& damages, const Samples<FontMetrics>& metrics) {
    Cover damage;
    bool whole = false, nothing = false, rects = false;
    for (const auto& s : damages) {
        walk(damage, At{}, s.value, wire::Damage());
        whole = whole || !s.value;
        nothing = nothing || (s.value && s.value->empty());
        rects = rects || (s.value && !s.value->empty());
    }
    check(whole && nothing && rects, "damage: the whole frame, nothing moved, and rects");
    inventory("damage", shape::damage(), damage);
    specials_in("damage: rects", damage.items);
    check(damage.items.count(THIRD) != 0, "damage: a float no hundredth rounds to");
    Cover font;
    for (const auto& s : metrics) walk(font, At{}, s.value, FontMetrics{});
    inventory("metrics", shape::of("", shape::METRICS), font);
    specials_in("metrics", font.scalars);
    check(font.scalars.count(THIRD) != 0, "metrics: a float no hundredth rounds to");
}

/// Version 1's defaults written out member by member, over a value that has all of its members set, so that one left out shows.
void version_1(Command& c) {
    c.kind = Cmd::FillRect;
    c.rect = Rect{0, 0, 0, 0};
    c.radii = Radii{0, 0, 0, 0, 0, 0, 0, 0};
    c.clip = Rect{0, 0, 0, 0};
    c.has_clip = false;
    c.color = Color{0, 0, 0, 255};
    c.width = 0;
    c.font = 0;
    c.text.clear();
    c.x = 0;
    c.y = 0;
    c.image = 0;
    c.canvas = 0;
    c.ax = 0;
    c.ay = 0;
    c.bx = 0;
    c.by = 0;
    c.stops.clear();
    c.path = nullptr;
    c.filter = nullptr;
    c.opacity = 1;
    spread(c.matrix, 0, 0);
    c.matrix[0] = 1;
    c.matrix[3] = 1;
    c.hidden = false;
    c.inset = false;
    c.key = 0;
}

void version_1(Path& p) {
    p.ops.clear();
    p.points.clear();
    p.fill = false;
    p.evenodd = false;
    p.fill_color = Color{0, 0, 0, 255};
    p.gradient = 0;
    p.x1 = 0;
    p.y1 = 0;
    p.x2 = 0;
    p.y2 = 0;
    p.r = 0;
    p.fx = 0;
    p.fy = 0;
    p.stops.clear();
    p.stroke = false;
    p.stroke_color = Color{0, 0, 0, 255};
    p.stroke_width = 1;
    p.line_cap = 0;
    p.line_join = 0;
    p.miter_limit = 4;
    p.dashes.clear();
    p.dash_offset = 0;
    p.stroke_gradient = 0;
    spread(p.sg, 0, 0);
    p.stroke_stops.clear();
}

void version_1(CanvasPaint& p) {
    p.kind = CanvasPaint::Solid;
    p.color = Color{0, 0, 0, 255};
    spread(p.args, 0, 0);
    p.stops.clear();
}

void version_1(CanvasOp& o) {
    o.kind = CanvasOp::Fill;
    o.path.ops.clear();
    o.path.points.clear();
    o.evenodd = false;
    version_1(o.paint);
    spread(o.matrix, 0, 0);
    o.matrix[0] = 1;
    o.matrix[3] = 1;
    o.line_width = 1;
    o.line_cap = 0;
    o.line_join = 0;
    o.miter_limit = 10;
    o.dashes.clear();
    o.dash_offset = 0;
    o.alpha = 1;
    o.composite = Composite::SourceOver;
    o.shadow_color = Color{0, 0, 0, 0};
    o.shadow_blur = 0;
    o.shadow_x = 0;
    o.shadow_y = 0;
    o.clips = nullptr;
    o.font = 0;
    o.text.clear();
    o.x = 0;
    o.y = 0;
    o.scale_x = 1;
    o.text_dx = 0;
    o.stroke_text = false;
    o.image = 0;
    o.canvas = 0;
    o.sx = 0;
    o.sy = 0;
    o.sw = -1;
    o.sh = -1;
    o.dx = 0;
    o.dy = 0;
    o.dw = 0;
    o.dh = 0;
    o.smoothing = true;
    o.rgba = nullptr;
    o.w = 0;
    o.h = 0;
}

void documented_numbers() {
    check(uint8_t(wire::Form::List) == 1 && uint8_t(wire::Form::Damage) == 2 && uint8_t(wire::Form::Batch) == 3 && uint8_t(wire::Form::Metrics) == 4,
        "forms: 1 list, 2 damage, 3 batch, 4 metrics");
    check(wire::VERSION == 1, "version 1");
    const wire::Caps caps;
    check(caps.commands == size_t(1) << 20, "cap commands: 2^20");
    check(caps.canvas_ops == size_t(1) << 20, "cap canvas_ops: 2^20");
    check(caps.path_ops == size_t(1) << 22, "cap path_ops: 2^22");
    check(caps.floats == size_t(1) << 22, "cap floats: 2^22");
    check(caps.stops == size_t(1) << 16, "cap stops: 2^16");
    check(caps.filters == size_t(1) << 12, "cap filters: 2^12");
    check(caps.clips == size_t(1) << 16, "cap clips: 2^16");
    check(caps.text == size_t(4) << 20, "cap text: 4 MiB");
    check(caps.pixels == size_t(64) << 20, "cap pixels: 64 MiB");
    check(caps.rects == size_t(1) << 12, "cap rects: 2^12");
}

/// The structs' initializers still are version 1's defaults, and an empty mask decodes to those defaults.
void version_1_defaults() {
    std::string why;
    Command command;
    every_member(command);
    version_1(command);
    check(same(command, Command{}, why), "Command's initializers are version 1's defaults " + why);
    Path path = *full_path(3);
    version_1(path);
    check(same(path, Path{}, why), "Path's initializers are version 1's defaults " + why);
    CanvasOp op;
    every_member(op);
    CanvasPaint paint = op.paint;
    version_1(paint);
    version_1(op);
    check(same(paint, CanvasPaint{}, why), "CanvasPaint's initializers are version 1's defaults " + why);
    check(same(op, CanvasOp{}, why), "CanvasOp's initializers are version 1's defaults " + why);
    check(same(Filter{Filter::Blur, 0, 0, 0, Color{0, 0, 0, 255}}, Filter{}, why), "Filter's initializers are version 1's defaults " + why);
    check(same(Stop{0, Color{0, 0, 0, 255}}, Stop{}, why), "Stop's initializers are version 1's defaults " + why);
    check(same(CanvasClip{Geometry{}, false}, CanvasClip{}, why), "CanvasClip's initializers are version 1's defaults " + why);
    check(same(FontMetrics{0, 0, 0, 0, 0, 0, 0}, FontMetrics{}, why), "FontMetrics' initializers are version 1's defaults " + why);

    const std::string list_bytes = Bytes().head(1).u32(2).u8(0).u32(0).u8(11).u32(0x60000).u32(0).u32(0).u32(0).u32(0).s;
    DisplayList list;
    check(decoded(list_bytes, {}, list) && list.size() == 2, "a list of empty masks and empty payloads decodes");
    if (list.size() == 2) {
        check(same(command, list[0], why), "an empty command mask decodes to version 1's defaults " + why);
        Command payloads = command;
        payloads.kind = Cmd::Path;
        payloads.path = list[1].path;
        payloads.filter = list[1].filter;
        check(same(payloads, list[1], why), "a command with only its payloads decodes the rest to version 1's defaults " + why);
        check(list[1].path && same(path, *list[1].path, why), "an empty path mask decodes to version 1's defaults " + why);
        check(list[1].filter && list[1].filter->empty(), "an empty filter list decodes to an empty one, not to none");
    }
    const std::string batch_bytes = Bytes().head(3).u32(2).u8(1).u64(0).u8(1).u64(uint64_t(1) << 2).u8(0).u8(0).s;
    Batch batch;
    check(decoded(batch_bytes, {}, batch) && batch.size() == 2, "a batch of empty masks decodes");
    if (batch.size() == 2) {
        check(same(op, batch[0], why), "an empty op mask decodes to version 1's defaults " + why);
        check(same(op, batch[1], why), "an empty paint mask decodes to version 1's paint " + why);
    }
}

/// Every member of a command, its path and its filters, with a second command that refers back to both.
DisplayList full_list() {
    DisplayList l(2);
    Command& c = l[0];
    c.kind = Cmd::Backdrop;
    c.rect = {1, 2, 3, 4};
    c.radii = {5, 6, 7, 8, 9, 10, 11, 12};
    c.clip = {13, 14, 15, 16};
    c.has_clip = true;
    c.color = {17, 18, 19, 20};
    c.width = 21;
    c.font = -22;
    c.text = "ab";
    c.x = 23;
    c.y = 24;
    c.image = -25;
    c.canvas = 26;
    c.ax = 27;
    c.ay = 28;
    c.bx = 29;
    c.by = 30;
    c.stops = {Stop{0.5f, {31, 32, 33, 34}}};
    auto p = std::make_shared<Path>();
    p->ops = {Path::Cubic, Path::Close};
    p->points = {35, 36};
    p->fill = true;
    p->evenodd = true;
    p->fill_color = {37, 38, 39, 40};
    p->gradient = 2;
    p->x1 = 41;
    p->y1 = 42;
    p->x2 = 43;
    p->y2 = 44;
    p->r = 45;
    p->fx = 46;
    p->fy = 47;
    p->stops = {Stop{0.25f, {48, 49, 50, 51}}};
    p->stroke = true;
    p->stroke_color = {52, 53, 54, 55};
    p->stroke_width = 56;
    p->line_cap = 1;
    p->line_join = 2;
    p->miter_limit = 57;
    p->dashes = {58};
    p->dash_offset = 59;
    p->stroke_gradient = 1;
    spread(p->sg, 60, 1);
    p->stroke_stops = {Stop{0.75f, {67, 68, 69, 70}}};
    c.path = p;
    c.filter = std::make_shared<Filters>(Filters{Filter{Filter::DropShadow, 71, 72, 73, {74, 75, 76, 77}}});
    c.opacity = 0.125f;
    spread(c.matrix, 78, 1);
    c.hidden = true;
    c.inset = true;
    c.key = 0x0123456789abcdefULL;
    l[1].kind = Cmd::Path;
    l[1].path = c.path;
    l[1].filter = c.filter;
    return l;
}

std::string joined(const std::vector<std::string>& fields) {
    std::string out;
    for (const std::string& f : fields) out += f;
    return out;
}

/// full_list()'s path as the documentation lays out each member: bit i's bytes at i, none for a flag.
std::vector<std::string> full_path_fields() {
    return {
        Bytes().u32(2).u8(3).u8(4).s, Bytes().u32(2).f32(35).f32(36).s, "", "", Bytes().color(37, 38, 39, 40).s, Bytes().i32(2).s,
        Bytes().f32(41).s, Bytes().f32(42).s, Bytes().f32(43).s, Bytes().f32(44).s, Bytes().f32(45).s, Bytes().f32(46).s, Bytes().f32(47).s,
        Bytes().u32(1).f32(0.25f).color(48, 49, 50, 51).s, "", Bytes().color(52, 53, 54, 55).s, Bytes().f32(56).s, Bytes().i32(1).s,
        Bytes().i32(2).s, Bytes().f32(57).s, Bytes().u32(1).f32(58).s, Bytes().f32(59).s, Bytes().i32(1).s, Bytes().run(60, 7).s,
        Bytes().u32(1).f32(0.75f).color(67, 68, 69, 70).s,
    };
}

/// full_list()'s first command, each member's bytes at its bit, the path and the filter list new payloads.
std::vector<std::string> full_command_fields() {
    return {
        Bytes().run(1, 4).s, Bytes().run(5, 8).s, Bytes().run(13, 4).s, "", Bytes().color(17, 18, 19, 20).s, Bytes().f32(21).s,
        Bytes().i32(-22).s, Bytes().u32(2).u8('a').u8('b').s, Bytes().f32(23).s, Bytes().f32(24).s, Bytes().i32(-25).s,
        Bytes().i32(26).s, Bytes().f32(27).s, Bytes().f32(28).s, Bytes().f32(29).s, Bytes().f32(30).s,
        Bytes().u32(1).f32(0.5f).color(31, 32, 33, 34).s, Bytes().u32(0).u32(0x01ffffff).s + joined(full_path_fields()),
        Bytes().u32(0).u32(1).u8(9).f32(71).f32(72).f32(73).color(74, 75, 76, 77).s, Bytes().f32(0.125f).s, Bytes().run(78, 6).s, "", "",
        Bytes().u64(0x0123456789abcdefULL).s,
    };
}

std::string full_list_bytes() {
    return Bytes().head(1).u32(2).u8(17).u32(0x00ffffff).s + joined(full_command_fields()) + Bytes().u8(11).u32(0x00060000).u32(1).u32(1).s;
}

/// Every member of an op, its paint, clips and pixels, with a second op that refers back to both payloads.
Batch full_batch() {
    Batch ops(2);
    CanvasOp& o = ops[0];
    o.kind = CanvasOp::PutImage;
    o.path.ops = {Path::Move, Path::Line};
    o.path.points = {1, 2, 3, 4};
    o.evenodd = true;
    o.paint.kind = CanvasPaint::Radial;
    o.paint.color = {5, 6, 7, 8};
    spread(o.paint.args, 9, 1);
    o.paint.stops = {Stop{0.5f, {15, 16, 17, 18}}};
    spread(o.matrix, 19, 1);
    o.line_width = 25;
    o.line_cap = 1;
    o.line_join = 2;
    o.miter_limit = 26;
    o.dashes = {27, 28};
    o.dash_offset = 29;
    o.alpha = 0.5f;
    o.composite = Composite::Luminosity;
    o.shadow_color = {30, 31, 32, 33};
    o.shadow_blur = 34;
    o.shadow_x = 35;
    o.shadow_y = 36;
    auto clips = std::make_shared<Clips>(1);
    (*clips)[0].path.ops = {Path::Move, Path::Close};
    (*clips)[0].path.points = {37, 38};
    (*clips)[0].evenodd = true;
    o.clips = clips;
    o.font = -39;
    o.text = "hi";
    o.x = 40;
    o.y = 41;
    o.scale_x = 42;
    o.text_dx = 43;
    o.stroke_text = true;
    o.image = 44;
    o.canvas = -45;
    o.sx = 46;
    o.sy = 47;
    o.sw = 48;
    o.sh = 49;
    o.dx = 50;
    o.dy = 51;
    o.dw = 52;
    o.dh = 53;
    o.smoothing = false;
    o.rgba = std::make_shared<Pixels>(Pixels{0x00, 0x7f, 0x80, 0xff});
    o.w = 1;
    o.h = 1;
    ops[1].clips = o.clips;
    ops[1].rgba = o.rgba;
    return ops;
}

/// full_batch()'s paint, each member's bytes at its bit.
std::vector<std::string> full_paint_fields() {
    return {Bytes().color(5, 6, 7, 8).s, Bytes().run(9, 6).s, Bytes().u32(1).f32(0.5f).color(15, 16, 17, 18).s};
}

/// full_batch()'s first op, each member's bytes at its bit, the clip list and the pixels new payloads.
std::vector<std::string> full_op_fields() {
    std::vector<std::string> f = {
        Bytes().u32(2).u8(0).u8(1).u32(4).run(1, 4).s, "", Bytes().u8(2).u8(7).s + joined(full_paint_fields()), Bytes().run(19, 6).s,
        Bytes().f32(25).s, Bytes().i32(1).s, Bytes().i32(2).s, Bytes().f32(26).s, Bytes().u32(2).f32(27).f32(28).s, Bytes().f32(29).s,
        Bytes().f32(0.5f).s, Bytes().u8(25).s, Bytes().color(30, 31, 32, 33).s, Bytes().f32(34).s, Bytes().f32(35).s, Bytes().f32(36).s,
        Bytes().u32(0).u32(1).u32(2).u8(0).u8(4).u32(2).f32(37).f32(38).u8(1).s, Bytes().i32(-39).s, Bytes().u32(2).u8('h').u8('i').s,
        Bytes().f32(40).s, Bytes().f32(41).s, Bytes().f32(42).s, Bytes().f32(43).s, "", Bytes().i32(44).s, Bytes().i32(-45).s,
    };
    for (int i = 0; i < 8; i++) f.push_back(Bytes().f32(float(46 + i)).s);
    f.push_back("");
    f.push_back(Bytes().u32(0).u32(4).u8(0x00).u8(0x7f).u8(0x80).u8(0xff).s);
    f.push_back(Bytes().i32(1).s);
    f.push_back(Bytes().i32(1).s);
    return f;
}

std::string full_batch_bytes() {
    return Bytes().head(3).u32(2).u8(6).u64(0x3fffffffffULL).s + joined(full_op_fields())
        + Bytes().u8(1).u64((uint64_t(1) << 16) | (uint64_t(1) << 35)).u32(1).u32(1).s;
}

void golden_bytes() {
    DisplayList sparse(1);
    sparse[0].rect = {1, 2, 3, 4};
    sparse[0].color = {10, 20, 30, 40};
    golden("a FillRect with a rect and a colour", sparse, unhex("0101 01000000 00 11000000 0000803f 00000040 00004040 00008040 0a141e28"));

    DisplayList pair(2);
    auto filled = std::make_shared<Path>();
    filled->fill = true;
    for (Command& c : pair) {
        c.kind = Cmd::Path;
        c.path = filled;
    }
    golden("two Path commands sharing one filled path", pair, unhex("0101 02000000 0b 00000200 00000000 04000000 0b 00000200 01000000"));

    golden("damage of the whole frame", wire::Damage(), unhex("0201 00"));
    golden("damage with nothing moved", wire::Damage(std::vector<Rect>()), unhex("0201 01 00000000"));
    golden("damage of one rect", wire::Damage(std::vector<Rect>{{1, 2, 3, 4}}), unhex("0201 01 01000000 0000803f 00000040 00004040 00008040"));

    Batch multiply(1);
    multiply[0].paint.color = {1, 2, 3, 4};
    multiply[0].composite = Composite::Multiply;
    golden("a Fill in a solid colour, multiplied", multiply, unhex("0301 01000000 01 0408000000000000 00 01 01020304 0b"));

    FontMetrics metrics;
    metrics.ascent = 1;
    metrics.descent = 2;
    metrics.height = 3;
    metrics.x_height = 4;
    metrics.zero_width = 5;
    metrics.em_ascent = 6;
    metrics.em_descent = 7;
    golden("metrics 1 to 7", metrics, unhex("0401 0000803f 00000040 00004040 00008040 0000a040 0000c040 0000e040"));

    golden("every member of a command, its path and its filters", full_list(), full_list_bytes());
    golden("every member of an op, its paint, its clips and its pixels", full_batch(), full_batch_bytes());
}

/// The members a value's leaves name, without what is below them.
std::set<std::string> tops(const std::set<std::string>& leaves) {
    std::set<std::string> out;
    for (const std::string& leaf : leaves) out.insert(leaf.substr(0, leaf.find_first_of(".[")));
    return out;
}

/// `bytes` decode to a value whose `part` sets `member` and nothing else, and the value encodes back to `bytes`.
template <class T, class Part> void lone(const std::string& what, const std::string& bytes, const char* member, Part part) {
    T value;
    Cover cover;
    const bool ok = decoded(bytes, {}, value);
    if (ok) part(cover, value);
    std::string set;
    for (const std::string& top : tops(cover.leaves)) set += " " + top;
    check(ok && tops(cover.leaves) == std::set<std::string>{member}, what + " is " + member + " alone, not" + set);
    std::string again;
    check(ok && encoded(value, {}, again) && again == bytes, what + " encodes back to its documented bytes");
}

/// Each member's documented bytes under its documented bit alone: the inventory lists members in declaration order, which is the documented bit order, so a swap of two bits shows.
void lone_members() {
    const std::vector<std::string> command = full_command_fields(), path = full_path_fields(), op = full_op_fields(), paint = full_paint_fields();
    check(command.size() == std::size(shape::COMMAND) && path.size() == std::size(shape::PATH) && op.size() == std::size(shape::OP)
            && paint.size() == std::size(shape::PAINT),
        "a documented field for every member");
    for (size_t i = 0; i < command.size() && i < std::size(shape::COMMAND); i++)
        lone<DisplayList>("command bit " + std::to_string(i), Bytes().head(1).u32(1).u8(0).u32(uint32_t(1) << i).s + command[i], shape::COMMAND[i].name,
            [](Cover& c, const DisplayList& l) {
                if (l.size() == 1) walk(c, At{}, l[0], Command{});
            });
    for (size_t i = 0; i < path.size() && i < std::size(shape::PATH); i++)
        lone<DisplayList>("path bit " + std::to_string(i), Bytes().head(1).u32(1).u8(11).u32(1 << 17).u32(0).u32(uint32_t(1) << i).s + path[i],
            shape::PATH[i].name, [](Cover& c, const DisplayList& l) {
                if (l.size() == 1 && l[0].path) walk(c, At{}, *l[0].path, Path{});
            });
    for (size_t i = 0; i < op.size() && i < std::size(shape::OP); i++)
        lone<Batch>("op bit " + std::to_string(i), Bytes().head(3).u32(1).u8(1).u64(uint64_t(1) << i).s + op[i], shape::OP[i].name,
            [](Cover& c, const Batch& b) {
                if (b.size() == 1) walk(c, At{}, b[0], CanvasOp{});
            });
    for (size_t i = 0; i < paint.size() && i < std::size(shape::PAINT); i++)
        lone<Batch>("paint bit " + std::to_string(i), Bytes().head(3).u32(1).u8(1).u64(uint64_t(1) << 2).u8(0).u8(1 << i).s + paint[i],
            shape::PAINT[i].name, [](Cover& c, const Batch& b) {
                if (b.size() == 1) walk(c, At{}, b[0].paint, CanvasPaint{});
            });
}

/// A value's payload tables are its own: encoding it again writes its payloads inline again, and a reference into an earlier value's payloads is refused.
void tables_per_value(const DisplayList& list, const Batch& batch) {
    std::string first, second;
    check(encoded(list, {}, first) && encoded(list, {}, second) && first == second, "a list encoded twice writes its payloads inline both times");
    std::string third, fourth;
    check(encoded(batch, {}, third) && encoded(batch, {}, fourth) && third == fourth, "a batch encoded twice writes its payloads inline both times");
    DisplayList earlier;
    check(decoded(Bytes().head(1).u32(2).u8(11).u32(1 << 17).u32(0).u32(0).u8(15).u32(1 << 18).u32(0).u32(0).s, {}, earlier) && earlier.size() == 2,
        "a list with a path and a filter list inline decodes");
    refused<DisplayList>("a path that refers to the list before's", Bytes().head(1).u32(1).u8(11).u32(1 << 17).u32(1).s);
    refused<DisplayList>("a filter list that refers to the list before's", Bytes().head(1).u32(1).u8(15).u32(1 << 18).u32(1).s);
    Batch before;
    check(decoded(Bytes().head(3).u32(2).u8(1).u64(uint64_t(1) << 16).u32(0).u32(0).u8(6).u64(uint64_t(1) << 35).u32(0).u32(0).s, {}, before) && before.size() == 2,
        "a batch with a clip list and pixels inline decodes");
    refused<Batch>("a clip list that refers to the batch before's", Bytes().head(3).u32(1).u8(1).u64(uint64_t(1) << 16).u32(1).s);
    refused<Batch>("pixels that refer to the batch before's", Bytes().head(3).u32(1).u8(6).u64(uint64_t(1) << 35).u32(1).s);
}

/// The comparisons themselves tell apart a key, -0 from 0, NaN payloads, a point or an op deep in a clip, and a copied payload from a shared one.
void comparisons_notice() {
    std::string why;
    DisplayList a(1), b(1);
    b[0].key = 1;
    check(!same(a, b, why) && why.starts_with("[0].key:"), "a different key shows: " + why);
    b = a;
    put_bits(b[0].width, 0x80000000);
    check(!same(a, b, why) && why.starts_with("[0].width:"), "-0 shows against 0: " + why);
    put_bits(a[0].x, 0x7fc00001);
    b = a;
    put_bits(b[0].x, 0x7fc00002);
    check(!same(a, b, why) && why.starts_with("[0].x:"), "another NaN payload shows: " + why);
    b = a;
    b[0].path = full_path(1);
    check(!same(a, b, why) && why.starts_with("[0].path:"), "a path where there was none shows: " + why);

    Batch c(1);
    c[0].clips = clip_list(1);
    Batch d = c;
    auto moved = std::make_shared<Clips>(*c[0].clips);
    (*moved)[1].path.points[2] = 99;
    d[0].clips = moved;
    check(!same(c, d, why) && why.starts_with("[0].clips[1].path.points[2]:"), "a point deep in a clip shows: " + why);
    auto turned = std::make_shared<Clips>(*c[0].clips);
    (*turned)[0].path.ops[1] = Path::Quad;
    d[0].clips = turned;
    check(!same(c, d, why) && why.starts_with("[0].clips[0].path.ops[1]:"), "an op deep in a clip shows: " + why);

    DisplayList shared(2), copied(2);
    shared[0].path = shared[1].path = full_path(1);
    copied[0].path = full_path(1);
    copied[1].path = full_path(1);
    check(same(shared, copied, why) && sharing(shared) != sharing(copied), "a copied payload is equal to a shared one but not shared alike " + why);
    Batch one(2), two(2);
    one[0].rgba = one[1].rgba = every_byte(1);
    two[0].rgba = every_byte(1);
    two[1].rgba = every_byte(1);
    check(same(one, two, why) && sharing(one) != sharing(two), "copied pixels are equal to shared ones but not shared alike " + why);
}

/// The most a decode asks for per input byte, by form, as this build's sizes and its library's bookkeeping give it.
struct Bound {
    double list = 0, batch = 0, damage = 0;
};

/// One request made of operator new itself, which no container's bookkeeping can change, freed however the run ends.
struct Block {
    void* p = nullptr;

    explicit Block(size_t n) : p(::operator new(n)) {}
    Block(const Block&) = delete;
    Block& operator=(const Block&) = delete;
    ~Block() { ::operator delete(p); }
};

/// What building a `T` from nothing asks for, as a decoder's initial value of a payload: the bookkeeping, if any, of the containers in it.
template <class T> Measured built() {
    std::optional<T> value;
    return measure(0, [&] {
        value.emplace();
        return value.has_value();
    });
}

/// What putting a `T` made from `from` in room a vector already has asks for, as a decoder's records and clips: the bookkeeping, if any, of the containers in it.
template <class T, class... From> Measured placed(const From&... from) {
    std::vector<T> room;
    room.reserve(1);
    return measure(0, [&] {
        room.emplace_back(from...);
        return room.size() == 1;
    });
}

/// What one new payload of `T` asks for, as a decoder's: make_shared's control block and a copy of an empty initial value made before it, with the copy's bookkeeping.
template <class T> Measured new_payload() {
    const T initial{};
    return measure(0, [&] { return std::make_shared<T>(initial) != nullptr; });
}

/// The slots a payload table asks for per entry: the most a table of any size up to 65536 entries asks for, or the g*g/(g-1) its growth by g heads to, whichever is more.
double table_slots() {
    std::vector<std::shared_ptr<const Path>> table;
    const auto entry = std::make_shared<const Path>();
    double most = 0, growth = 0;
    const Measured grown = measure(SIZE_MAX, [&] {
        for (size_t k = 1; k <= 65536; k++) {
            const size_t room = table.capacity();
            table.push_back(entry);
            if (room && table.capacity() != room) growth = double(table.capacity()) / double(room);
            most = std::max(most, double(scope.asked) / double(k * sizeof(entry)));
        }
        return true;
    });
    check(grown.ok && growth > 1, "calibration: a payload table grows by a factor, seen " + std::to_string(growth));
    return growth > 1 ? std::max(most, growth * growth / (growth - 1)) : most;
}

/// The most the library asks for per byte of storage made room for, as for MSVC's aligned large blocks on x86 and x64: what scales the derived bound.
double storage_slack() {
    double most = 1;
    for (size_t k = 1; k <= (size_t(1) << 20); k *= 2) {
        for (const size_t n : {k - 1, k, k + 1}) {
            std::vector<char> room;
            const Measured m = measure(SIZE_MAX, [&] {
                room.reserve(n);
                return true;
            });
            if (n) most = std::max(most, double(m.scope.asked) / double(n));
        }
    }
    return most;
}

/// The scope sees what vectors, strings, make_shared and operator new ask for, refuses before malloc a request past its budget and counts nothing outside a run; from what it sees, the library's bookkeeping included, the bound a decode keeps to.
Bound calibrate_allocations() {
    std::vector<Command> command_room;
    const Measured commands = measure(0, [&] {
        command_room.reserve(3);
        return true;
    });
    check(commands.ok && commands.scope.asked == 3 * sizeof(Command) && commands.scope.count == 1,
        "calibration: room for 3 commands in a vector built before is one request of " + std::to_string(3 * sizeof(Command)) + " bytes, seen " + std::to_string(commands.scope.asked));
    std::vector<float> float_room;
    const Measured floats = measure(0, [&] {
        float_room.resize(10);
        return true;
    });
    check(floats.ok && floats.scope.asked == 10 * sizeof(float) && floats.scope.count == 1,
        "calibration: 10 floats in a vector built before are one request of 40 bytes, seen " + std::to_string(floats.scope.asked));
    const std::string source(4096, 's');
    for (size_t n : {size_t(23), size_t(24), size_t(31), size_t(47), size_t(48), size_t(64), size_t(100), size_t(1000), size_t(4096)}) {
        std::string s;
        const Measured text = measure(n, [&] {
            s.assign(source.data(), n);
            return s.size() == n;
        });
        check(text.ok && text.scope.count == 1 && text.scope.asked >= n && text.scope.asked <= 2 * (n + 4),
            "calibration: a string of " + std::to_string(n) + " bytes in a string built before is one request of at least that and at most twice its str field, seen "
                + std::to_string(text.scope.asked));
    }
    const Measured exact = measure(0, [] {
        const Block b(10);
        return b.p != nullptr;
    });
    check(exact.ok && exact.scope.asked == 10 && exact.scope.granted == 10 && exact.scope.count == 1, "calibration: a request of operator new is counted to the byte");
    const Measured nothrow = measure(0, [] {
        char* p = new (std::nothrow) char[10];
        const bool got = p != nullptr;
        delete[] p;
        return got;
    });
    check(nothrow.ok && nothrow.scope.asked == 10 && nothrow.scope.count == 1, "calibration: a nothrow new[] is counted");
    const Measured path = new_payload<Path>(), filters = new_payload<Filters>(), clips = new_payload<Clips>(), pixels = new_payload<Pixels>();
    check(path.ok && path.scope.asked > sizeof(Path) && filters.ok && filters.scope.asked > sizeof(Filters) && clips.ok && clips.scope.asked > sizeof(Clips) && pixels.ok
            && pixels.scope.asked > sizeof(Pixels),
        "calibration: a new payload asks for more than its value: its control block too");
    const Measured command = placed<Command>(Command()), op = placed<CanvasOp>(CanvasOp()), clip = placed<CanvasClip>();
    const Measured no_filters = built<Filters>(), no_clips = built<Clips>(), no_pixels = built<Pixels>();
    check(command.ok && op.ok && clip.ok && no_filters.ok && no_clips.ok && no_pixels.ok, "calibration: what making a record, a clip or an initial payload value asks for is measured");

    const Measured over = measure(0, [] {
        const Block b(FIXED + 1);
        return true;
    });
    check(over.threw && !over.ok && over.scope.over && over.scope.asked == FIXED + 1 && over.scope.granted == 0 && over.scope.count == 1,
        "calibration: a request past the budget is refused before malloc, and throws");
    const Measured quiet = measure(0, [] {
        char* p = new (std::nothrow) char[FIXED + 1];
        const bool none = p == nullptr;
        delete[] p;
        return none;
    });
    check(quiet.ok && !quiet.threw && quiet.scope.over && quiet.scope.granted == 0, "calibration: a nothrow request past the budget gets null");
    const Measured summed = measure(1, [] {
        const Block a(1000);
        const Block b(PER_BYTE + FIXED - 999);
        return true;
    });
    check(summed.threw && summed.scope.over && summed.scope.granted == 1000 && summed.scope.asked == PER_BYTE + FIXED + 1 && summed.scope.count == 2,
        "calibration: the budget bounds what a run asks for in all, not each request");
    const Measured freed = measure(1, [] {
        {
            const Block a(1000);
        }
        const Block b(200);
        return true;
    });
    check(freed.threw && freed.scope.over && freed.scope.granted == 1000 && freed.scope.asked == 1200, "calibration: what a run frees does not make room for more");
    const size_t seen = scope.count;
    const std::string outside(1000, 'o');
    check(!scope.armed && scope.count == seen && !outside.empty(), "calibration: nothing is counted outside a run");
    const size_t widest = (SIZE_MAX - FIXED) / PER_BYTE;
    check(budget(0) == FIXED && budget(1) == PER_BYTE + FIXED && budget(widest) == PER_BYTE * widest + FIXED && budget(widest + 1) == SIZE_MAX && budget(SIZE_MAX) == SIZE_MAX,
        "calibration: the budget saturates rather than wraps");

    // A record costs its room, its copy of the defaults and the initial value it builds for each payload it may read (a filter list's, a clip list's, pixels'; a path starts from version 1's own), per fewest wire bytes of one; a new payload its request and its table slots per 8 bytes it takes; each nested list level may charge what is left twice; the library's slack scales it all.
    const double slots = table_slots(), slack = storage_slack();
    const double slot_bytes = slots * sizeof(std::shared_ptr<const Path>);
    const double lists = std::max({2.0, sizeof(Stop) / 8.0, sizeof(float) / 4.0, double(sizeof(Path::Op)), sizeof(Filter) / 17.0});
    const double nested = std::max({lists, double(sizeof(CanvasClip) + clip.scope.asked) / 9.0, double(sizeof(uint8_t))});
    Bound b;
    b.list = slack
        * (double(sizeof(Command) + command.scope.asked + no_filters.scope.asked) / 5.0 + (double(std::max(path.scope.asked, filters.scope.asked)) + slot_bytes) / 8
            + 2 * lists);
    b.batch = slack
        * (double(sizeof(CanvasOp) + op.scope.asked + no_clips.scope.asked + no_pixels.scope.asked) / 9.0
            + (double(std::max(clips.scope.asked, pixels.scope.asked)) + slot_bytes) / 8 + 2 * 2 * nested);
    b.damage = slack * 2 * sizeof(Rect) / 16.0;
    check(b.list <= PER_BYTE && b.batch <= PER_BYTE && b.damage <= PER_BYTE, "the derived bound fits the budget of 128 bytes for each input byte");
    std::printf("allocation: derived bound per input byte: list %.1f, batch %.1f, damage %.1f; from Command %zu, CanvasOp %zu, CanvasClip %zu, Filter %zu, "
                "made in room Command %zu, CanvasOp %zu, CanvasClip %zu, initial Filters %zu, clips %zu, pixels %zu, new Path %zu, Filters %zu, clips %zu, pixels %zu, "
                "shared_ptr %zu, %.4f table slots each, slack %.4f; budget %zu per byte and %zu\n",
        b.list, b.batch, b.damage, sizeof(Command), sizeof(CanvasOp), sizeof(CanvasClip), sizeof(Filter), command.scope.asked, op.scope.asked, clip.scope.asked,
        no_filters.scope.asked, no_clips.scope.asked, no_pixels.scope.asked, path.scope.asked, filters.scope.asked, clips.scope.asked, pixels.scope.asked,
        sizeof(std::shared_ptr<const Path>), slots, slack, PER_BYTE, FIXED);
    return b;
}

/// A member of wire::Caps, named so that MSVC parses each declaration of one.
using CapField = size_t wire::Caps::*;

constexpr CapField CAPS[] = {&wire::Caps::commands, &wire::Caps::canvas_ops, &wire::Caps::path_ops, &wire::Caps::floats, &wire::Caps::stops,
    &wire::Caps::filters, &wire::Caps::clips, &wire::Caps::text, &wire::Caps::pixels, &wire::Caps::rects};
const char* const CAP_NAMES[] = {"commands", "canvas_ops", "path_ops", "floats", "stops", "filters", "clips", "text", "pixels", "rects"};

/// The caps every sample just fits: each the most items any sample has in that field.
wire::Caps tight() {
    wire::Caps c;
    c.commands = 18;
    c.canvas_ops = 26;
    c.path_ops = 5;
    c.floats = 14;
    c.stops = 8;
    c.filters = 10;
    c.clips = 2;
    c.text = 300;
    c.pixels = 1024;
    c.rects = 5;
    return c;
}

wire::Caps open_caps() {
    wire::Caps c;
    for (CapField cap : CAPS) c.*cap = SIZE_MAX;
    return c;
}

template <class T> size_t refusing(const Samples<T>& samples, const wire::Caps& caps) {
    size_t n = 0;
    for (const auto& s : samples) {
        std::string bytes;
        if (!encoded(s.value, caps, bytes)) n++;
    }
    return n;
}

/// Some sample reaches each of tight()'s caps: one less, and it no longer encodes.
void tight_caps_reached(const Samples<DisplayList>& lists, const Samples<Batch>& batches, const Samples<wire::Damage>& damages) {
    for (size_t i = 0; i < std::size(CAPS); i++) {
        wire::Caps caps = tight();
        caps.*CAPS[i] -= 1;
        check(refusing(lists, caps) + refusing(batches, caps) + refusing(damages, caps) > 0, std::string("a sample reaches the tight cap on ") + CAP_NAMES[i]);
    }
}

/// The sharing pattern of what `bytes` decode to as form `form`, nothing when they are refused.
template <class T> std::optional<Sharing> shared_by(std::string_view bytes, const wire::Caps& caps) {
    T value;
    if (!decoded(bytes, caps, value)) return std::nullopt;
    return sharing(value);
}

std::optional<Sharing> shared_as(int form, std::string_view bytes, const wire::Caps& caps) {
    switch (form) {
    case 1: return shared_by<DisplayList>(bytes, caps);
    case 2: return shared_by<wire::Damage>(bytes, caps);
    case 3: return shared_by<Batch>(bytes, caps);
    }
    return shared_by<FontMetrics>(bytes, caps);
}

void refused_as(int form, const std::string& what, std::string_view bytes, const wire::Caps& caps) {
    switch (form) {
    case 1: return refused<DisplayList>(what, bytes, caps);
    case 2: return refused<wire::Damage>(what, bytes, caps);
    case 3: return refused<Batch>(what, bytes, caps);
    }
    refused<FontMetrics>(what, bytes, caps);
}

/// Bytes a value is appended after, to show that an encoder keeps them.
const std::string EARLIER("an earlier value\0\xff", 18);

/// `bytes` decode under `caps` to a value that encodes again under them after an earlier value: to `bytes` themselves when `exact`, else to bytes that decode to the same; under `below`, its encoding is refused with the earlier value kept.
template <class T> void accepted(const std::string& what, const std::string& bytes, const wire::Caps& caps, bool exact, const wire::Caps* below = nullptr) {
    T out;
    const bool ok = decoded(bytes, caps, out);
    check(ok, what + ": decodes");
    if (!ok) return;
    std::string again = EARLIER, why;
    const bool written = encoded(out, caps, again) && again.compare(0, EARLIER.size(), EARLIER) == 0;
    const std::string mine = written ? again.substr(EARLIER.size()) : std::string();
    if (exact) {
        check(written && mine == bytes, what + ": encodes back to the same bytes after an earlier value");
    } else {
        T back;
        check(written && decoded(mine, caps, back) && same(out, back, why) && sharing(out) == sharing(back), what + ": encodes to bytes that decode to the same " + why);
    }
    if (below) {
        std::string kept = EARLIER;
        check(!encoded(out, *below, kept) && kept == EARLIER, what + ": encoding it one cap lower is refused, the earlier value kept");
    }
}

void accepted_as(int form, const std::string& what, const std::string& bytes, const wire::Caps& caps, bool exact, const wire::Caps* below = nullptr) {
    switch (form) {
    case 1: return accepted<DisplayList>(what, bytes, caps, exact, below);
    case 2: return accepted<wire::Damage>(what, bytes, caps, exact, below);
    case 3: return accepted<Batch>(what, bytes, caps, exact, below);
    }
    accepted<FontMetrics>(what, bytes, caps, exact, below);
}

/// `n` records after a count of `declared`, as form `form`.
std::string repeated(uint32_t form, size_t declared, size_t n, const std::string& record) {
    Bytes b;
    b.head(form).u32(uint32_t(declared));
    for (size_t i = 0; i < n; i++) b.s += record;
    return b.s;
}

std::string new_path() {
    return Bytes().u8(11).u32(1 << 17).u32(0).u32(0).s;
}

std::string new_clips_and_pixels() {
    return Bytes().u8(1).u64((uint64_t(1) << 16) | (uint64_t(1) << 35)).u32(0).u32(0).u32(0).u32(0).s;
}

std::string four_clips() {
    Bytes b;
    b.u8(1).u64(uint64_t(1) << 16).u32(0).u32(4);
    for (int i = 0; i < 4; i++) b.u32(3).u8(0).u8(1).u8(4).u32(6).run(1, 6).u8(1);
    return b.s;
}

/// Records repeated to a size: whether the value is whole, and the row of sharing() whose payload all its records share, if one does.
struct Growth {
    const char* name;
    int form;
    bool whole;
    std::string (*bytes)(size_t n);
    int shared_row;
};

const Growth GROWTH[] = {
    {"commands at their defaults", 1, true, [](size_t n) { return repeated(1, n, n, Bytes().u8(0).u32(0).s); }, -1},
    {"commands each with a new empty path", 1, true, [](size_t n) { return repeated(1, n, n, new_path()); }, -1},
    {"commands each with a new empty filter list", 1, true, [](size_t n) { return repeated(1, n, n, Bytes().u8(15).u32(1 << 18).u32(0).u32(0).s); }, -1},
    {"commands each with a new path and filter list", 1, true,
        [](size_t n) { return repeated(1, n, n, Bytes().u8(11).u32((1 << 17) | (1 << 18)).u32(0).u32(0).u32(0).u32(0).s); }, -1},
    {"commands that all share one path of 4096 points", 1, true,
        [](size_t n) {
            Bytes b;
            b.head(1).u32(uint32_t(n + 1)).u8(11).u32(1 << 17).u32(0).u32(1 << 1).u32(4096).run(0, 4096);
            for (size_t i = 0; i < n; i++) b.u8(11).u32(1 << 17).u32(1);
            return b.s;
        },
        0},
    {"as many commands declared as the bytes could hold, each with a new path, cut short", 1, false, [](size_t n) { return repeated(1, 13 * n / 5, n, new_path()); }, -1},
    {"ops at their defaults", 3, true, [](size_t n) { return repeated(3, n, n, Bytes().u8(1).u64(0).s); }, -1},
    {"ops each with a new empty clip list and new empty pixels", 3, true, [](size_t n) { return repeated(3, n, n, new_clips_and_pixels()); }, -1},
    {"ops each with a new list of four clips", 3, true, [](size_t n) { return repeated(3, n, n, four_clips()); }, -1},
    {"ops each with a new list of four clips, cut at the last byte", 3, false,
        [](size_t n) {
            std::string s = repeated(3, n, n, four_clips());
            s.pop_back();
            return s;
        },
        -1},
    {"ops that all share one pixel payload of 64 KiB", 3, true,
        [](size_t n) {
            Bytes b;
            b.head(3).u32(uint32_t(n + 1)).u8(6).u64(uint64_t(1) << 35).u32(0).u32(65536);
            for (uint32_t i = 0; i < 65536; i++) b.u8(i);
            for (size_t i = 0; i < n; i++) b.u8(6).u64(uint64_t(1) << 35).u32(1);
            return b.s;
        },
        1},
    {"as many ops declared as the bytes could hold, each with new clips and pixels, cut short", 3, false,
        [](size_t n) { return repeated(3, 25 * n / 9, n, new_clips_and_pixels()); }, -1},
    {"damage of many rects", 2, true,
        [](size_t n) {
            Bytes b;
            b.head(2).u8(1).u32(uint32_t(n));
            for (size_t i = 0; i < n; i++) b.f32(float(i)).f32(1).f32(2).f32(3);
            return b.s;
        },
        -1},
};

/// At every size, whole or refused at its end, a decode asks for no more than the derived bound per input byte, under caps that bound nothing.
void allocation_growth(const Bound& bound) {
    const wire::Caps open = open_caps();
    for (const Growth& g : GROWTH) {
        const double per_byte = g.form == 1 ? bound.list : g.form == 3 ? bound.batch : bound.damage;
        const size_t record = g.form == 1 ? sizeof(Command) : g.form == 3 ? sizeof(CanvasOp) : sizeof(Rect);
        for (size_t n : {size_t(1), size_t(16), size_t(256), size_t(4096), size_t(32768)}) {
            const std::string bytes = g.bytes(n);
            const std::string what = std::string(g.name) + ", " + std::to_string(n) + " records";
            context = "allocation growth: " + what;
            std::optional<Sharing> shared;
            if (g.whole) {
                shared = shared_as(g.form, bytes, open);
                check(shared.has_value(), what + ": decodes");
            } else {
                refused_as(g.form, what, bytes, open);
            }
            const Measured m = last_decode;
            const double per = double(m.scope.asked) / double(bytes.size());
            check(double(m.scope.asked) <= per_byte * double(bytes.size()) + double(FIXED),
                what + ": asks for " + std::to_string(m.scope.asked) + " bytes for " + std::to_string(bytes.size()) + ", past the derived bound");
            check(m.scope.asked >= n * record, what + ": the scope sees the room made for each record");
            if (g.shared_row >= 0) check(shared && (*shared)[size_t(g.shared_row)] == std::vector<long>(n + 1, 0), what + ": every record shares the one payload");
            if (n == 32768)
                std::printf("allocation: %s: %zu input bytes, %zu asked, %.1f per byte against a bound of %.1f\n", what.c_str(), bytes.size(), m.scope.asked, per, per_byte);
        }
    }
}

/// Every proper prefix of every sample is refused, and so are bytes after one, one value after another, and no bytes at all.
template <class T> void cuts(const std::string& form, const Samples<T>& samples) {
    for (const auto& s : samples) {
        const std::string what = form + " " + s.name;
        std::string bytes;
        if (!encoded(s.value, {}, bytes)) {
            check(false, what + ": encodes");
            continue;
        }
        context = "cuts of " + what;
        for (size_t n = 0; n < bytes.size(); n++) refused<T>(what + " cut to " + std::to_string(n) + " bytes", std::string_view(bytes).substr(0, n));
        refused<T>(what + " with a NUL after it", bytes + '\0');
        refused<T>(what + " with 0xff after it", bytes + '\xff');
        refused<T>(what + " twice over", bytes + bytes);
    }
    context = "no bytes at all";
    refused<T>(form + ": no bytes, from no pointer", std::string_view());
    const char one = 'x';
    refused<T>(form + ": no bytes, from a pointer", std::string_view(&one, 0));
}

/// Random changes to every sample under default and tight caps: a refusal leaves the output alone, and what is taken encodes again under the same caps to what decodes the same, shared alike.
template <class T> void mutations(const std::string& form, const Samples<T>& samples, uint64_t seed) {
    Tally tally;
    const wire::Caps under[] = {wire::Caps(), tight()};
    const char* const names[] = {"default caps", "tight caps"};
    T expected;
    make_sentinel(expected);
    for (size_t c = 0; c < std::size(under); c++) {
        size_t index = 0;
        for (const auto& s : samples) {
            const uint64_t start = seed + 0x100 * c + index++;
            std::string bytes;
            if (!encoded(s.value, under[c], bytes)) {
                check(false, form + " " + s.name + ": encodes under " + names[c]);
                continue;
            }
            context = "changes to " + form + " " + s.name + " under " + names[c] + " from seed " + hex(start);
            Lcg random{start};
            for (int round = 0; round < ROUNDS; round++) {
                std::string changed = bytes;
                const std::string edits = mutate(random, changed);
                const auto where = [&] { return context + ", round " + std::to_string(round) + ":" + edits; };
                T out;
                make_sentinel(out);
                const std::vector<const void*> before = held(out);
                std::string why;
                if (decoded(changed, under[c], out)) {
                    tally.accepted++;
                    std::string again;
                    T back;
                    const bool closed = encoded(out, under[c], again) && decoded(again, under[c], back) && same(out, back, why) && sharing(out) == sharing(back);
                    verify(closed, [&] { return where() + ": what it decodes to does not come back the same " + why; });
                } else {
                    tally.refused++;
                    verify(same(expected, out, why) && held(out) == before, [&] { return where() + ": the refusal changed the output " + why; });
                }
            }
        }
    }
    check(tally.accepted > 0 && tally.refused > 0, form + ": random changes both decode and are refused");
    std::printf("changes: %s, seeds from %s, %d rounds on each of %zu samples under default and tight caps: %zu decoded, %zu refused\n", form.c_str(), hex(seed).c_str(), ROUNDS,
        samples.size(), tally.accepted, tally.refused);
}

/// A count or a length in a complete value: the cap that bounds it, the value's bytes before it and after its items, and its i-th item.
struct Site {
    std::string name;
    CapField cap;
    /// The fewest bytes one item takes, as docs/TWIDGET_CORE.md gives them.
    size_t least;
    int form;
    std::string before, after;
    void (*item)(Bytes&, size_t);

    std::string bytes(uint32_t declared, size_t items, bool cut = false) const {
        Bytes b{before};
        b.u32(declared);
        for (size_t i = 0; i < items; i++) item(b, i);
        if (cut) b.s.pop_back();
        return b.s + after;
    }
};

void stop_item(Bytes& b, size_t i) {
    b.f32(0.125f * float(i)).color(uint32_t(i), 2, 3, 255);
}

void byte_item(Bytes& b, size_t i) {
    b.u8(uint32_t('a' + i % 26));
}

void op_item(Bytes& b, size_t i) {
    b.u8(uint32_t(i % 5));
}

void float_item(Bytes& b, size_t i) {
    b.f32(float(i) + 0.5f);
}

std::vector<Site> sites() {
    const uint32_t path = 1 << 17, filter = 1 << 18, opacity = 1 << 19, key = 1 << 23, stroke_width = 1 << 16;
    const std::string list = Bytes().head(1).u32(1).s, batch = Bytes().head(3).u32(1).s;
    const std::string in_path = list + Bytes().u8(11).u32(path).u32(0).s;
    const std::string clips = batch + Bytes().u8(1).u64((1 << 16) | (1 << 17)).u32(0).s;
    return {
        {"a list's commands", &wire::Caps::commands, 5, 1, Bytes().head(1).s, "", [](Bytes& b, size_t i) { b.u8(uint32_t(i % 18)).u32(0); }},
        {"a command's stops", &wire::Caps::stops, 8, 1, list + Bytes().u8(8).u32((1 << 16) | key).s, Bytes().u64(7).s, stop_item},
        {"a command's text", &wire::Caps::text, 1, 1, list + Bytes().u8(4).u32((1 << 7) | key).s, Bytes().u64(7).s, byte_item},
        {"a path's ops", &wire::Caps::path_ops, 1, 1, in_path + Bytes().u32(1 | stroke_width).s, Bytes().f32(2).s, op_item},
        {"a path's points", &wire::Caps::floats, 4, 1, in_path + Bytes().u32((1 << 1) | stroke_width).s, Bytes().f32(2).s, float_item},
        {"a path's stops", &wire::Caps::stops, 8, 1, in_path + Bytes().u32((1 << 13) | stroke_width).s, Bytes().f32(2).s, stop_item},
        {"a path's dashes", &wire::Caps::floats, 4, 1, in_path + Bytes().u32((1 << 20) | (1 << 21)).s, Bytes().f32(0.5f).s, float_item},
        {"a path's stroke stops", &wire::Caps::stops, 8, 1, list + Bytes().u8(11).u32(path | opacity).u32(0).u32(1 << 24).s, Bytes().f32(0.5f).s, stop_item},
        {"a filter list's filters", &wire::Caps::filters, 17, 1, list + Bytes().u8(15).u32(filter | opacity).u32(0).s, Bytes().f32(0.5f).s,
            [](Bytes& b, size_t i) { b.u8(uint32_t(i % 10)).f32(float(i)).f32(1).f32(2).color(1, 2, 3, 4); }},
        {"a batch's ops", &wire::Caps::canvas_ops, 9, 3, Bytes().head(3).s, "", [](Bytes& b, size_t i) { b.u8(uint32_t(i % 7)).u64(0); }},
        {"an op's path ops", &wire::Caps::path_ops, 1, 3, batch + Bytes().u8(1).u64(1 | (1 << 4)).s, Bytes().u32(0).f32(2).s, op_item},
        {"an op's points", &wire::Caps::floats, 4, 3, batch + Bytes().u8(1).u64(1 | (1 << 4)).u32(0).s, Bytes().f32(2).s, float_item},
        {"an op's dashes", &wire::Caps::floats, 4, 3, batch + Bytes().u8(1).u64((1 << 8) | (1 << 9)).s, Bytes().f32(0.5f).s, float_item},
        {"a paint's stops", &wire::Caps::stops, 8, 3, batch + Bytes().u8(1).u64((1 << 2) | (1 << 3)).u8(1).u8(1 << 2).s, Bytes().f32(2).f32(0).f32(0).f32(2).f32(0).f32(0).s,
            stop_item},
        {"an op's text", &wire::Caps::text, 1, 3, batch + Bytes().u8(4).u64((1 << 18) | (1 << 19)).s, Bytes().f32(3).s, byte_item},
        {"a clip list's clips", &wire::Caps::clips, 9, 3, clips, Bytes().i32(5).s, [](Bytes& b, size_t i) { b.u32(0).u32(0).u8(uint32_t(i % 2)); }},
        {"a clip's path ops", &wire::Caps::path_ops, 1, 3, clips + Bytes().u32(1).s, Bytes().u32(0).u8(1).i32(5).s, op_item},
        {"a clip's points", &wire::Caps::floats, 4, 3, clips + Bytes().u32(1).u32(0).s, Bytes().u8(1).i32(5).s, float_item},
        {"an op's pixels", &wire::Caps::pixels, 1, 3, batch + Bytes().u8(6).u64((uint64_t(1) << 35) | (uint64_t(1) << 36)).u32(0).s, Bytes().i32(1).s,
            [](Bytes& b, size_t i) { b.u8(uint32_t(i)); }},
        {"damage's rects", &wire::Caps::rects, 16, 2, Bytes().head(2).u8(1).s, "", [](Bytes& b, size_t i) { b.f32(float(i)).f32(1).f32(2).f32(3); }},
    };
}

/// Counts no small input holds: around INT32_MAX, the largest, and those whose product with 4, 8 or 16 bytes wraps in 32 bits.
constexpr uint32_t VAST[] = {0x7fffffff, 0x80000000, 0xffffffff, 0x40000000, 0x40000001, 0x20000000, 0x20000001, 0x10000000, 0x10000001};

/// Each count and length with only its own cap restrictive: complete at its cap and refused one past it, at caps 0, 1 and 3, both ways; refused cut inside its items or declaring one more; refused before making room when declaring more than the bytes left hold, under a cap of SIZE_MAX too; taken under a cap past UINT32_MAX.
void count_sites() {
    for (const Site& s : sites()) {
        context = "the count of " + s.name;
        for (size_t cap : {size_t(0), size_t(1), size_t(3)}) {
            wire::Caps caps, below;
            caps.*s.cap = cap;
            below.*s.cap = cap ? cap - 1 : 0;
            accepted_as(s.form, s.name + " at a cap of " + std::to_string(cap), s.bytes(uint32_t(cap), cap), caps, cap > 0, cap ? &below : nullptr);
            refused_as(s.form, s.name + " one past a cap of " + std::to_string(cap), s.bytes(uint32_t(cap + 1), cap + 1), caps);
        }
        refused_as(s.form, s.name + " cut inside its last item", s.bytes(3, 3, true), {});
        refused_as(s.form, s.name + " declaring one item more than it has", s.bytes(3, 2), {});
        accepted_as(s.form, s.name + " holding one item", s.bytes(1, 1), {}, true);
        const size_t one = last_decode.scope.asked, left = s.bytes(1, 1).size() - s.before.size() - 4;
        const auto refused_at_count = [&](const std::string& what, const std::string& bytes, const wire::Caps& caps) {
            refused_as(s.form, what, bytes, caps);
            check(last_decode.scope.asked <= one, what + ": refused before making room, asking for " + std::to_string(last_decode.scope.asked) + " bytes where one item asks for " + std::to_string(one));
        };
        refused_at_count(s.name + " declaring one item more than the bytes left could hold", s.bytes(uint32_t(left / s.least + 1), 1), {});
        wire::Caps open;
        open.*s.cap = SIZE_MAX;
        accepted_as(s.form, s.name + " under a cap of SIZE_MAX", s.bytes(3, 3), open, true);
        for (uint32_t n : VAST) refused_at_count(s.name + " declaring " + hex(n) + " under a cap of SIZE_MAX", s.bytes(n, 1), open);
        wire::Caps widest;
        widest.*s.cap = UINT32_MAX;
        accepted_as(s.form, s.name + " under a cap of UINT32_MAX", s.bytes(1, 1), widest, true);
        if constexpr (SIZE_MAX > UINT32_MAX) {
            wire::Caps wide;
            wide.*s.cap = size_t(UINT32_MAX) + 1;
            accepted_as(s.form, s.name + " under a cap of 2^32", s.bytes(1, 1), wide, true);
        }
    }
}

/// A payload kind's references: records each writing a new payload of it, the i-th distinct, and a record naming the k-th.
struct Reference {
    const char* name;
    int form;
    size_t row;
    std::string (*fresh)(size_t i);
    std::string (*naming)(uint32_t k);
};

/// Every reference kind names its first, a middle and its last payload so far, and is refused past them, at UINT32_MAX, and before any payload is written.
void references() {
    const Reference kinds[] = {
        {"a path", 1, 0, [](size_t i) { return Bytes().u8(11).u32(1 << 17).u32(0).u32(1 << 16).f32(float(i) + 2).s; },
            [](uint32_t k) { return Bytes().u8(11).u32(1 << 17).u32(k).s; }},
        {"a filter list", 1, 1, [](size_t i) { return Bytes().u8(15).u32(1 << 18).u32(0).u32(1).u8(0).f32(float(i) + 1).f32(0).f32(0).color(0, 0, 0, 255).s; },
            [](uint32_t k) { return Bytes().u8(15).u32(1 << 18).u32(k).s; }},
        {"a clip list", 3, 0, [](size_t i) { return Bytes().u8(1).u64(1 << 16).u32(0).u32(1).u32(0).u32(1).f32(float(i)).u8(0).s; },
            [](uint32_t k) { return Bytes().u8(1).u64(1 << 16).u32(k).s; }},
        {"pixels", 3, 1, [](size_t i) { return Bytes().u8(6).u64(uint64_t(1) << 35).u32(0).u32(1).u8(uint32_t(i)).s; },
            [](uint32_t k) { return Bytes().u8(6).u64(uint64_t(1) << 35).u32(k).s; }},
    };
    for (const Reference& r : kinds) {
        context = std::string("references to ") + r.name;
        const auto with = [&](uint32_t k) { return Bytes().head(uint32_t(r.form)).u32(4).s + r.fresh(0) + r.fresh(1) + r.fresh(2) + r.naming(k); };
        for (uint32_t k : {1u, 2u, 3u}) {
            const std::string what = std::string(r.name) + " named by reference " + std::to_string(k) + " of 3";
            accepted_as(r.form, what, with(k), {}, true);
            const std::optional<Sharing> s = shared_as(r.form, with(k), {});
            check(s && (*s)[r.row] == std::vector<long>{0, 1, 2, long(k) - 1}, what + ": shares that payload");
        }
        for (uint32_t k : {4u, 5u, 0x80000000u, 0xffffffffu}) refused_as(r.form, std::string(r.name) + " named by reference " + hex(k) + " of 3", with(k), {});
        refused_as(r.form, std::string(r.name) + " named before any is written", Bytes().head(uint32_t(r.form)).u32(1).s + r.naming(1), {});
        refused_as(r.form, std::string(r.name) + " named before the first is written", Bytes().head(uint32_t(r.form)).u32(2).s + r.naming(1) + r.fresh(0), {});
    }
}

/// An enum in a value: bytes holding one value of it, and the last value version 1 knows.
struct EnumPlace {
    const char* name;
    int form;
    uint32_t last;
    std::string (*bytes)(uint32_t v);
};

/// Every enum, wherever it is and whether or not the kind around it paints it, takes its last value and refuses the next one and above.
void enum_places() {
    const EnumPlace places[] = {
        {"a command's kind", 1, 17, [](uint32_t v) { return Bytes().head(1).u32(1).u8(v).u32(0).s; }},
        {"a filter's kind, on a FillRect", 1, 9, [](uint32_t v) { return Bytes().head(1).u32(1).u8(0).u32(1 << 18).u32(0).u32(1).u8(v).f32(1).f32(2).f32(3).color(1, 2, 3, 4).s; }},
        {"an op of a path, on a FillRect", 1, 4, [](uint32_t v) { return Bytes().head(1).u32(1).u8(0).u32(1 << 17).u32(0).u32(1).u32(2).u8(0).u8(v).s; }},
        {"an op's kind", 3, 6, [](uint32_t v) { return Bytes().head(3).u32(1).u8(v).u64(0).s; }},
        {"a paint's kind, on a Reset", 3, 3, [](uint32_t v) { return Bytes().head(3).u32(1).u8(0).u64(1 << 2).u8(v).u8(0).s; }},
        {"a composite, on a Reset", 3, 25, [](uint32_t v) { return Bytes().head(3).u32(1).u8(0).u64(1 << 11).u8(v).s; }},
        {"an op of an op's path, on a Text op", 3, 4, [](uint32_t v) { return Bytes().head(3).u32(1).u8(4).u64(1).u32(2).u8(0).u8(v).u32(0).s; }},
        {"an op of a clip's path, on a Clear", 3, 4, [](uint32_t v) { return Bytes().head(3).u32(1).u8(3).u64(1 << 16).u32(0).u32(1).u32(2).u8(0).u8(v).u32(0).u8(0).s; }},
    };
    for (const EnumPlace& p : places) {
        context = std::string("the enum in ") + p.name;
        accepted_as(p.form, std::string(p.name) + " at " + std::to_string(p.last), p.bytes(p.last), {}, true);
        for (uint32_t v : {p.last + 1, p.last + 2, 0x80u, 0xffu}) refused_as(p.form, std::string(p.name) + " at " + std::to_string(v), p.bytes(v), {});
    }
}

/// Each mask bit past the last member is refused, next to the same record with an empty mask.
void mask_bits() {
    context = "mask bits";
    const std::string command = Bytes().head(1).u32(1).u8(0).s, path = Bytes().head(1).u32(1).u8(11).u32(1 << 17).u32(0).s;
    const std::string op = Bytes().head(3).u32(1).u8(1).s, paint = op + Bytes().u64(1 << 2).u8(0).s;
    accepted<DisplayList>("a command with an empty mask", command + Bytes().u32(0).s, {}, true);
    for (int bit = 24; bit < 32; bit++) refused<DisplayList>("a command with mask bit " + std::to_string(bit), command + Bytes().u32(uint32_t(1) << bit).s);
    refused<DisplayList>("a command with every mask bit", command + Bytes().u32(0xffffffff).s);
    accepted<DisplayList>("a path with an empty mask", path + Bytes().u32(0).s, {}, true);
    for (int bit = 25; bit < 32; bit++) refused<DisplayList>("a path with mask bit " + std::to_string(bit), path + Bytes().u32(uint32_t(1) << bit).s);
    accepted<Batch>("an op with an empty mask", op + Bytes().u64(0).s, {}, true);
    for (int bit = 38; bit < 64; bit++) refused<Batch>("an op with mask bit " + std::to_string(bit), op + Bytes().u64(uint64_t(1) << bit).s);
    refused<Batch>("an op with every mask bit", op + Bytes().u64(UINT64_MAX).s);
    accepted<Batch>("a paint with an empty mask", paint + Bytes().u8(0).s, {}, false);
    for (int bit = 3; bit < 8; bit++) refused<Batch>("a paint with mask bit " + std::to_string(bit), paint + Bytes().u8(uint32_t(1) << bit).s);
}

/// The two flags take 0 and 1 and refuse anything else.
void flags() {
    context = "flags";
    accepted<wire::Damage>("damage flag 0", Bytes().head(2).u8(0).s, {}, true);
    accepted<wire::Damage>("damage flag 1", Bytes().head(2).u8(1).u32(0).s, {}, true);
    const std::string clip = Bytes().head(3).u32(1).u8(1).u64(1 << 16).u32(0).u32(1).u32(0).u32(0).s;
    accepted<Batch>("a clip's evenodd 0", clip + Bytes().u8(0).s, {}, true);
    accepted<Batch>("a clip's evenodd 1", clip + Bytes().u8(1).s, {}, true);
    for (uint32_t v : {2u, 0x80u, 0xffu}) {
        refused<wire::Damage>("damage flag " + std::to_string(v), Bytes().head(2).u8(v).s);
        refused<wire::Damage>("damage flag " + std::to_string(v) + " before an empty count", Bytes().head(2).u8(v).u32(0).s);
        refused<Batch>("a clip's evenodd " + std::to_string(v), clip + Bytes().u8(v).s);
    }
}

/// A value marked with any other form or version is refused.
template <class T> std::string heads(const std::string& form, const T& value) {
    context = "the form and version of " + form;
    std::string bytes;
    check(encoded(value, {}, bytes) && bytes.size() >= 2, form + ": encodes");
    if (bytes.size() < 2) return bytes;
    accepted<T>(form + " as it is", bytes, {}, true);
    for (uint32_t f : {0u, 1u, 2u, 3u, 4u, 5u, 0x81u, 0xffu}) {
        if (f == uint8_t(bytes[0])) continue;
        std::string other = bytes;
        other[0] = char(f);
        refused<T>(form + " marked form " + std::to_string(f), other);
    }
    for (uint32_t v : {0u, 2u, 0x81u, 0xffu}) {
        std::string other = bytes;
        other[1] = char(v);
        refused<T>(form + " marked version " + std::to_string(v), other);
    }
    return bytes;
}

/// A value of one form is refused by the decoders of the other three.
void forms_apart(const std::string (&bytes)[4]) {
    context = "forms apart";
    for (int as = 1; as <= 4; as++)
        for (int of = 1; of <= 4; of++)
            if (as != of) refused_as(as, "a form " + std::to_string(of) + " value read as form " + std::to_string(as), bytes[of - 1], {});
}

void zeros(Bytes& b, int n) {
    for (int i = 0; i < n; i++) b.f32(0);
}

/// Every member written at its version-1 default and every list written empty decodes to the defaults, and encodes again without them.
void present_defaults() {
    context = "members written at their defaults";
    std::string why;
    Bytes c;
    c.head(1).u32(1).u8(0).u32(0x00ffffff & ~((1u << 3) | (1u << 17) | (1u << 18) | (1u << 21) | (1u << 22)));
    zeros(c, 16);
    c.color(0, 0, 0, 255).f32(0).i32(0).u32(0).f32(0).f32(0).i32(0).i32(0);
    zeros(c, 4);
    c.u32(0).f32(1).f32(1).f32(0).f32(0).f32(1).f32(0).f32(0).u64(0);
    DisplayList list;
    std::string again;
    check(decoded(c.s, {}, list) && list.size() == 1 && same(Command{}, list[0], why), "a command with every member at its default decodes to the defaults " + why);
    check(encoded(list, {}, again) && again == Bytes().head(1).u32(1).u8(0).u32(0).s, "a command with every member at its default encodes again without them");

    Bytes p;
    p.head(1).u32(1).u8(11).u32(1 << 17).u32(0).u32(0x01ffffff & ~((1u << 2) | (1u << 3) | (1u << 14)));
    p.u32(0).u32(0).color(0, 0, 0, 255).i32(0);
    zeros(p, 7);
    p.u32(0).color(0, 0, 0, 255).f32(1).i32(0).i32(0).f32(4).u32(0).f32(0).i32(0);
    zeros(p, 7);
    p.u32(0);
    list.clear();
    again.clear();
    check(decoded(p.s, {}, list) && list.size() == 1 && list[0].path && same(Path{}, *list[0].path, why), "a path with every member at its default decodes to the defaults " + why);
    check(encoded(list, {}, again) && again == Bytes().head(1).u32(1).u8(11).u32(1 << 17).u32(0).u32(0).s, "a path with every member at its default encodes again without them");

    Bytes o;
    const uint64_t bools_and_payloads = (uint64_t(1) << 1) | (uint64_t(1) << 16) | (uint64_t(1) << 23) | (uint64_t(1) << 34) | (uint64_t(1) << 35);
    o.head(3).u32(1).u8(1).u64(((uint64_t(1) << 38) - 1) & ~bools_and_payloads);
    o.u32(0).u32(0).u8(0).u8(7).color(0, 0, 0, 255);
    zeros(o, 6);
    o.u32(0).f32(1).f32(0).f32(0).f32(1).f32(0).f32(0).f32(1).i32(0).i32(0).f32(10).u32(0).f32(0).f32(1).u8(0).color(0, 0, 0, 0);
    zeros(o, 3);
    o.i32(0).u32(0).f32(0).f32(0).f32(1).f32(0).i32(0).i32(0).f32(0).f32(0).f32(-1).f32(-1);
    zeros(o, 4);
    o.i32(0).i32(0);
    Batch batch;
    again.clear();
    check(decoded(o.s, {}, batch) && batch.size() == 1 && same(CanvasOp{}, batch[0], why), "an op with every member at its default decodes to the defaults " + why);
    check(encoded(batch, {}, again) && again == Bytes().head(3).u32(1).u8(1).u64(0).s, "an op with every member at its default encodes again without them");
}

/// Encoders refuse an enum value version 1 does not know wherever it is, the earlier value kept; the last value it knows encodes and decodes back.
void encoder_enums() {
    context = "encoder enums";
    const auto refuses = [](const std::string& what, const auto& value) {
        std::string out = EARLIER;
        check(!encoded(value, {}, out) && out == EARLIER, what + ": refused, the earlier value kept");
    };
    const auto takes = [](const std::string& what, const auto& value) {
        std::string out = EARLIER, why;
        std::decay_t<decltype(value)> back;
        check(encoded(value, {}, out) && out.compare(0, EARLIER.size(), EARLIER) == 0 && decoded(std::string_view(out).substr(EARLIER.size()), {}, back) && same(value, back, why),
            what + ": encodes after the earlier value and decodes back " + why);
    };
    DisplayList list(2);
    list[0].kind = Cmd::Text;
    list[0].text = std::string(40, 't');
    list[0].path = full_path(1);
    for (int kind : {18, 255, -1, INT_MIN, INT_MAX}) {
        DisplayList l = list;
        l[1].kind = Cmd(kind);
        refuses("a second command of kind " + std::to_string(kind), l);
    }
    DisplayList last = list;
    last[1].kind = Cmd::Backdrop;
    takes("a second command of kind Backdrop", last);
    for (uint32_t op : {5u, 0x80u, 0xffu, 4u}) {
        DisplayList l = list;
        auto p = std::make_shared<Path>();
        p->ops = {Path::Move, Path::Op(op)};
        l[1].path = p;
        if (op == Path::Close) takes("a FillRect's path ending in Close", l);
        else refuses("a FillRect's path with op " + std::to_string(op), l);
    }
    for (uint32_t kind : {10u, 0x80u, 0xffu, 9u}) {
        DisplayList l = list;
        auto f = std::make_shared<Filters>(2);
        (*f)[1].kind = Filter::Kind(kind);
        l[1].filter = f;
        if (kind == Filter::DropShadow) takes("a FillRect's filter of kind DropShadow", l);
        else refuses("a FillRect's filter of kind " + std::to_string(kind), l);
    }

    Batch batch(2);
    batch[0].text = std::string(40, 'b');
    batch[0].rgba = every_byte(3);
    for (uint32_t kind : {7u, 0x80u, 0xffu, 6u}) {
        Batch b = batch;
        b[1].kind = CanvasOp::Kind(kind);
        if (kind == CanvasOp::PutImage) takes("a second op of kind PutImage", b);
        else refuses("a second op of kind " + std::to_string(kind), b);
    }
    for (uint32_t kind : {4u, 0x80u, 0xffu, 3u}) {
        Batch b = batch;
        b[1].kind = CanvasOp::Reset;
        b[1].paint.kind = CanvasPaint::Kind(kind);
        if (kind == CanvasPaint::Conic) takes("a Reset's paint of kind Conic", b);
        else refuses("a Reset's paint of kind " + std::to_string(kind), b);
    }
    for (uint32_t composite : {26u, 0x80u, 0xffu, 25u}) {
        Batch b = batch;
        b[1].kind = CanvasOp::Reset;
        b[1].composite = Composite(composite);
        if (composite == uint32_t(Composite::Luminosity)) takes("a Reset's composite Luminosity", b);
        else refuses("a Reset's composite " + std::to_string(composite), b);
    }
    for (uint32_t op : {5u, 0x80u, 0xffu, 4u}) {
        Batch b = batch;
        b[1].kind = CanvasOp::Text;
        b[1].path.ops = {Path::Move, Path::Op(op)};
        if (op == Path::Close) takes("a Text op's path ending in Close", b);
        else refuses("a Text op's path with op " + std::to_string(op), b);
        Batch c = batch;
        c[1].kind = CanvasOp::Clear;
        auto clips = std::make_shared<Clips>(2);
        (*clips)[1].path.ops = {Path::Move, Path::Op(op)};
        c[1].clips = clips;
        if (op == Path::Close) takes("a Clear's clip path ending in Close", c);
        else refuses("a Clear's clip path with op " + std::to_string(op), c);
    }
}

/// The starts an encoder notes are where each command begins in what it wrote, counted from the list's first byte after a prefix too: the first commands alone encode to the same bytes up to the next start, shared payloads and references back to them included; a refusal leaves the bytes and the starts as they were.
void command_starts(const Samples<DisplayList>& lists) {
    context = "command starts";
    DisplayList repeated(6);
    const auto path = full_path(3);
    for (size_t i = 0; i < repeated.size(); i++) {
        repeated[i].kind = i % 3 ? Cmd::Path : Cmd::FillRect;
        repeated[i].rect = {1, 2, 3, 4};
        if (i % 3) repeated[i].path = path;
    }
    std::vector<std::pair<std::string, DisplayList>> all{{"the same commands again, sharing a path", repeated}};
    for (const auto& s : lists) all.push_back({s.name, s.value});
    for (const auto& [name, list] : all) {
        const std::string what = "list " + name;
        std::string plain;
        if (!wire::encode(list, {}, plain)) {
            check(false, what + ": encodes");
            continue;
        }
        for (const std::string& before : {std::string(), EARLIER}) {
            const std::string where = before.empty() ? "" : " after a prefix";
            std::string out = before;
            std::vector<size_t> starts{99};
            const bool ok = wire::encode(list, {}, out, starts);
            check(ok && out == before + plain, what + ": writes the same bytes" + where);
            const bool counted = starts.size() == list.size() + 1 && starts.front() == 6 && starts.back() == plain.size();
            check(counted, what + ": a start for each command, the first after the list's head, and the list's end" + where);
            for (size_t i = 0; ok && counted && i < list.size(); i++) {
                std::string first;
                wire::encode(DisplayList(list.begin(), list.begin() + long(i + 1)), {}, first);
                verify(starts[i + 1] == first.size() && plain.compare(starts[i], starts[i + 1] - starts[i], first, starts[i], first.size() - starts[i]) == 0,
                    [&] { return what + ": command " + std::to_string(i) + " lies between its starts as the first commands alone encode it" + where; });
            }
            std::vector<size_t> within{99}, past{99}, head{99};
            std::string at_most = before, short_by_one = before, headless = before;
            check(wire::encode(list, {}, at_most, within, plain.size()) && at_most == before + plain && within == starts, what + ": a list as long as the bytes given keeps its starts" + where);
            check(wire::encode(list, {}, short_by_one, past, plain.size() - 1) && short_by_one == before + plain && past.empty(),
                what + ": a list a byte longer is written the same with no starts" + where);
            check(wire::encode(list, {}, headless, head, 5) && headless == before + plain && head.empty(), what + ": so is any list given fewer bytes than its head" + where);
        }
    }
    wire::Caps one;
    one.commands = 1;
    DisplayList bad(2);
    bad[1].kind = Cmd(int(Cmd::Backdrop) + 1);
    const std::vector<size_t> sentinel{7, 8};
    std::string out = EARLIER;
    std::vector<size_t> starts = sentinel;
    check(!wire::encode(DisplayList(2), one, out, starts) && out == EARLIER && starts == sentinel, "a list past its command cap is refused, the bytes and the starts kept");
    check(!wire::encode(bad, {}, out, starts) && out == EARLIER && starts == sentinel, "a command of a kind version 1 does not know is refused, the bytes and the starts kept");
}

} // namespace

int main() {
    context = "allocation calibration";
    const Bound bound = calibrate_allocations();

    const Samples<DisplayList> lists = list_samples();
    const Samples<Batch> batches = batch_samples();
    const Samples<wire::Damage> damages = damage_samples();
    const Samples<FontMetrics> metrics = metrics_samples();

    context = "documented bytes, coverage and round trips";
    documented_numbers();
    version_1_defaults();
    golden_bytes();
    lone_members();
    list_coverage(lists);
    batch_coverage(batches);
    damage_and_metrics_coverage(damages, metrics);
    comparisons_notice();

    for (const auto& s : lists) round_trip("list " + s.name, s.value);
    for (const auto& s : batches) round_trip("batch " + s.name, s.value);
    for (const auto& s : damages) round_trip("damage " + s.name, s.value);
    for (const auto& s : metrics) round_trip("metrics " + s.name, s.value);
    command_starts(lists);

    tables_per_value(named(lists, PAYLOADS), named(batches, PAYLOADS));
    alternate("lists", named(lists, EVERY_MEMBER), named(lists, PAYLOADS));
    alternate("batches", named(batches, EVERY_MEMBER), named(batches, PAYLOADS));
    alternate("damage", named(damages, "rects, special floats among them"), named(damages, "the whole frame"));
    alternate("metrics", named(metrics, "ordinary metrics"), named(metrics, "special metrics"));

    context = "round trips under tight caps";
    for (const auto& s : lists) round_trip("list " + s.name + " under tight caps", s.value, tight());
    for (const auto& s : batches) round_trip("batch " + s.name + " under tight caps", s.value, tight());
    for (const auto& s : damages) round_trip("damage " + s.name + " under tight caps", s.value, tight());
    tight_caps_reached(lists, batches, damages);

    allocation_growth(bound);
    cuts("list", lists);
    cuts("batch", batches);
    cuts("damage", damages);
    cuts("metrics", metrics);
    count_sites();
    references();
    enum_places();
    mask_bits();
    flags();
    const std::string each_form[] = {heads("list", full_list()), heads("damage", named(damages, "rects, special floats among them")), heads("batch", full_batch()),
        heads("metrics", named(metrics, "ordinary metrics"))};
    forms_apart(each_form);
    present_defaults();
    encoder_enums();

    mutations("list", lists, 0x11570000);
    mutations("batch", batches, 0xba7c0000);
    mutations("damage", damages, 0xda3a0000);
    mutations("metrics", metrics, 0x3e7c0000);

    std::printf("allocation: %zu decodes measured against a budget of %zu bytes for each input byte and %zu more\n", measured_decodes, PER_BYTE, FIXED);
    print_extreme("the nearest its budget", nearest_budget);
    print_extreme("the most per input byte of 256 or more", most_per_byte);
    std::printf("%d checks, %d failure(s)\n", checks, failures);
    return failures ? 1 : 0;
}
