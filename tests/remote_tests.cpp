// twidget::remote alone, linked without litehtml or the renderer: every message kind comes back member for member, its bytes follow docs/TWIDGET_CORE.md "Two ends over a byte channel", hostile bytes are refused with the output untouched, no decode asking the allocator for more than a bound linear in its bytes, and a platform end serves a session, widths batches included, and stops at what an honest renderer end never sends.
#include "platform_end.h"
#include "remote.h"

#include "decoder_checks.h"
#include "log_backend.h"
#include "wire_compare.h"

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <limits>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace {

using remote::Kind;

uint64_t bits64(const double& d) {
    uint64_t b = 0;
    std::memcpy(&b, &d, sizeof b);
    return b;
}

void put_bits(float& f, uint32_t b) {
    std::memcpy(&f, &b, sizeof f);
}

void put_bits(double& d, uint64_t b) {
    std::memcpy(&d, &b, sizeof d);
}

/// Bits a copy through a float or a double could change: NaNs with payloads and either sign, a signalling NaN, -0, both infinities, a subnormal, the largest.
constexpr uint32_t SPECIAL_FLOATS[] = {0x7fc12345, 0xffc54321, 0x7f800abc, 0x80000000, 0x7f800000, 0xff800000, 0x00000003, 0x7f7fffff};
constexpr uint64_t SPECIAL_DOUBLES[] = {0x7ff8000000012345, 0xfff8000000054321, 0x7ff00000000abcde, 0x8000000000000000, 0x7ff0000000000000, 0xfff0000000000000,
    0x0000000000000003, 0x7fefffffffffffff};
constexpr size_t SPECIALS = std::size(SPECIAL_FLOATS);
static_assert(std::size(SPECIAL_DOUBLES) == SPECIALS);

const Kind KINDS[] = {Kind::FrameRequest, Kind::SetWidth, Kind::SetMaxHeight, Kind::SetTheme, Kind::Touch, Kind::Hover, Kind::ClearSelection, Kind::ClaimQuery,
    Kind::FontAnswer, Kind::WidthAnswer, Kind::ImageAnswer, Kind::PixelsAnswer, Kind::WidthsAnswer, Kind::Frame, Kind::State, Kind::Messages, Kind::Pointer,
    Kind::Selection, Kind::ClaimAnswer, Kind::FontRelease, Kind::CanvasRun, Kind::CanvasRelease, Kind::FrameDelta, Kind::FontRequest, Kind::WidthRequest,
    Kind::ImageRequest, Kind::PixelsRequest, Kind::WidthsRequest};

bool listed(uint32_t kind) {
    for (Kind k : KINDS)
        if (uint32_t(k) == kind) return true;
    return false;
}

/// Bytes a value is appended after, to show that an encoder keeps them.
const std::string EARLIER("an earlier value\0\xff", 18);

/// Counts no small input holds: around INT32_MAX, the largest, and those whose product with 4, 8 or 16 bytes wraps in 32 bits.
constexpr uint32_t VAST[] = {0x7fffffff, 0x80000000, 0xffffffff, 0x40000000, 0x40000001, 0x20000000, 0x20000001, 0x10000000, 0x10000001};

template <class V> void f64(V& v, const At& at, const double& a, const double& b) {
    v.integer(at, bits64(a), bits64(b));
}

template <class V> void walk_claim(V& v, const At& at, const twidget::WidgetGestureClaim& a, const twidget::WidgetGestureClaim& b) {
    v.integer(At{&at, "x"}, a.x, b.x);
    v.integer(At{&at, "y"}, a.y, b.y);
    v.integer(At{&at, "hold"}, a.hold, b.hold);
}

template <class V> void walk_state(V& v, const At& at, const remote::State& a, const remote::State& b) {
    f64(v, At{&at, "width"}, a.width, b.width);
    v.integer(At{&at, "height"}, uint32_t(a.height), uint32_t(b.height));
    v.integer(At{&at, "dirty"}, a.dirty, b.dirty);
    v.integer(At{&at, "wants_tick"}, a.wants_tick, b.wants_tick);
    f64(v, At{&at, "next_tick_in"}, a.next_tick_in, b.next_tick_in);
    v.integer(At{&at, "frame_asked"}, a.frame_asked, b.frame_asked);
}

template <class V> void walk(V& v, const At& at, const remote::FrameRequest& a, const remote::FrameRequest& b) {
    v.integer(At{&at, "seq"}, a.seq, b.seq);
    v.integer(At{&at, "tick"}, a.tick, b.tick);
    v.integer(At{&at, "paint"}, uint8_t(a.paint), uint8_t(b.paint));
}

template <class V> void walk(V& v, const At& at, const remote::SetWidth& a, const remote::SetWidth& b) {
    f64(v, At{&at, "width"}, a.width, b.width);
}

template <class V> void walk(V& v, const At& at, const remote::SetMaxHeight& a, const remote::SetMaxHeight& b) {
    f64(v, At{&at, "height"}, a.height, b.height);
}

template <class V> void walk(V& v, const At& at, const remote::SetTheme& a, const remote::SetTheme& b) {
    v.integer(At{&at, "dark"}, a.dark, b.dark);
}

template <class V> void walk(V& v, const At& at, const remote::Touch& a, const remote::Touch& b) {
    v.integer(At{&at, "action"}, uint32_t(a.action), uint32_t(b.action));
    f64(v, At{&at, "x"}, a.x, b.x);
    f64(v, At{&at, "y"}, a.y, b.y);
    v.integer(At{&at, "pointer"}, uint32_t(a.pointer), uint32_t(b.pointer));
    v.integer(At{&at, "primary"}, a.primary, b.primary);
    v.integer(At{&at, "mouse"}, a.mouse, b.mouse);
    v.integer(At{&at, "modifiers"}, uint32_t(a.modifiers), uint32_t(b.modifiers));
    v.integer(At{&at, "button"}, uint32_t(a.button), uint32_t(b.button));
}

template <class V> void walk(V& v, const At& at, const remote::Hover& a, const remote::Hover& b) {
    v.integer(At{&at, "action"}, uint32_t(a.action), uint32_t(b.action));
    f64(v, At{&at, "x"}, a.x, b.x);
    f64(v, At{&at, "y"}, a.y, b.y);
    v.integer(At{&at, "modifiers"}, uint32_t(a.modifiers), uint32_t(b.modifiers));
}

template <class V> void walk(V&, const At&, const remote::ClearSelection&, const remote::ClearSelection&) {}

template <class V> void walk(V& v, const At& at, const remote::ClaimQuery& a, const remote::ClaimQuery& b) {
    v.integer(At{&at, "seq"}, a.seq, b.seq);
    f64(v, At{&at, "x"}, a.x, b.x);
    f64(v, At{&at, "y"}, a.y, b.y);
}

template <class V> void walk(V& v, const At& at, const remote::FontAnswer& a, const remote::FontAnswer& b) {
    v.integer(At{&at, "font"}, uint32_t(a.font), uint32_t(b.font));
    walk(v, At{&at, "metrics"}, a.metrics, b.metrics);
}

template <class V> void walk(V& v, const At& at, const remote::WidthAnswer& a, const remote::WidthAnswer& b) {
    v.f32(At{&at, "width"}, a.width, b.width);
}

template <class V> void walk(V& v, const At& at, const remote::ImageAnswer& a, const remote::ImageAnswer& b) {
    v.integer(At{&at, "ok"}, a.ok, b.ok);
    v.f32(At{&at, "w"}, a.w, b.w);
    v.f32(At{&at, "h"}, a.h, b.h);
}

template <class V> void walk(V& v, const At& at, const remote::PixelsAnswer& a, const remote::PixelsAnswer& b) {
    v.integer(At{&at, "ok"}, a.ok, b.ok);
    walk(v, At{&at, "rgba"}, a.rgba, b.rgba);
}

template <class V> void walk(V& v, const At& at, const remote::WidthsAnswer& a, const remote::WidthsAnswer& b) {
    walk(v, At{&at, "widths"}, a.widths, b.widths);
}

template <class V> void walk(V& v, const At& at, const remote::State& a, const remote::State& b) {
    walk_state(v, at, a, b);
}

template <class V> void walk(V& v, const At& at, const remote::Frame& a, const remote::Frame& b) {
    v.integer(At{&at, "seq"}, a.seq, b.seq);
    walk_state(v, At{&at, "state"}, a.state, b.state);
    v.integer(At{&at, "painted"}, a.painted, b.painted);
    walk(v, At{&at, "list"}, a.list, b.list);
    walk(v, At{&at, "damage"}, a.damage, b.damage);
}

template <class V> void walk(V& v, const At& at, const remote::Messages& a, const remote::Messages& b) {
    const At texts{&at, "texts"};
    v.integer(At{&texts, "size"}, a.texts.size(), b.texts.size());
    const std::string none;
    for (size_t i = 0; i < std::max(a.texts.size(), b.texts.size()); i++)
        v.raw(At{&texts, "", i, At::Item}, i < a.texts.size() ? a.texts[i] : none, i < b.texts.size() ? b.texts[i] : none);
}

template <class V> void walk(V& v, const At& at, const remote::Pointer& a, const remote::Pointer& b) {
    v.raw(At{&at, "cursor"}, a.cursor, b.cursor);
    const At claim{&at, "claim"};
    v.integer(claim, a.claim.has_value(), b.claim.has_value());
    walk_claim(v, claim, a.claim.value_or(twidget::WidgetGestureClaim()), b.claim.value_or(twidget::WidgetGestureClaim()));
}

template <class V> void walk(V& v, const At& at, const remote::Selection& a, const remote::Selection& b) {
    v.raw(At{&at, "text"}, a.text, b.text);
}

template <class V> void walk(V& v, const At& at, const remote::ClaimAnswer& a, const remote::ClaimAnswer& b) {
    v.integer(At{&at, "seq"}, a.seq, b.seq);
    walk_claim(v, At{&at, "claim"}, a.claim, b.claim);
}

template <class V> void walk(V& v, const At& at, const remote::FontRelease& a, const remote::FontRelease& b) {
    v.integer(At{&at, "font"}, uint32_t(a.font), uint32_t(b.font));
}

template <class V> void walk(V& v, const At& at, const remote::CanvasRun& a, const remote::CanvasRun& b) {
    v.integer(At{&at, "id"}, uint32_t(a.id), uint32_t(b.id));
    v.integer(At{&at, "w"}, uint32_t(a.w), uint32_t(b.w));
    v.integer(At{&at, "h"}, uint32_t(a.h), uint32_t(b.h));
    walk(v, At{&at, "ops"}, a.ops, b.ops);
}

template <class V> void walk(V& v, const At& at, const remote::CanvasRelease& a, const remote::CanvasRelease& b) {
    v.integer(At{&at, "id"}, uint32_t(a.id), uint32_t(b.id));
}

template <class V> void walk(V& v, const At& at, const remote::FontRequest& a, const remote::FontRequest& b) {
    v.integer(At{&at, "canvas"}, a.canvas, b.canvas);
    v.raw(At{&at, "families"}, a.families, b.families);
    v.f32(At{&at, "size"}, a.size, b.size);
    v.integer(At{&at, "weight"}, uint32_t(a.weight), uint32_t(b.weight));
    v.integer(At{&at, "italic"}, a.italic, b.italic);
    v.raw(At{&at, "features"}, a.features, b.features);
}

template <class V> void walk(V& v, const At& at, const remote::WidthRequest& a, const remote::WidthRequest& b) {
    v.integer(At{&at, "font"}, uint32_t(a.font), uint32_t(b.font));
    v.raw(At{&at, "text"}, a.text, b.text);
}

template <class V> void walk(V& v, const At& at, const remote::ImageRequest& a, const remote::ImageRequest& b) {
    v.integer(At{&at, "id"}, uint32_t(a.id), uint32_t(b.id));
    v.raw(At{&at, "url"}, a.url, b.url);
}

template <class V> void walk(V& v, const At& at, const remote::PixelsRequest& a, const remote::PixelsRequest& b) {
    v.integer(At{&at, "id"}, uint32_t(a.id), uint32_t(b.id));
    v.integer(At{&at, "x"}, uint32_t(a.x), uint32_t(b.x));
    v.integer(At{&at, "y"}, uint32_t(a.y), uint32_t(b.y));
    v.integer(At{&at, "w"}, uint32_t(a.w), uint32_t(b.w));
    v.integer(At{&at, "h"}, uint32_t(a.h), uint32_t(b.h));
}

template <class V> void walk(V& v, const At& at, const TextInFont& a, const TextInFont& b) {
    v.integer(At{&at, "font"}, uint32_t(a.font), uint32_t(b.font));
    v.raw(At{&at, "text"}, a.text, b.text);
}

template <class V> void walk(V& v, const At& at, const remote::WidthsRequest& a, const remote::WidthsRequest& b) {
    walk(v, At{&at, "texts"}, a.texts, b.texts);
}

template <class V> void walk(V& v, const At& at, const remote::DeltaRun& a, const remote::DeltaRun& b) {
    v.integer(At{&at, "offset"}, a.offset, b.offset);
    v.integer(At{&at, "length"}, a.length, b.length);
    v.raw(At{&at, "literal"}, a.literal, b.literal);
}

template <class V> void walk(V& v, const At& at, const remote::FrameDelta& a, const remote::FrameDelta& b) {
    v.integer(At{&at, "seq"}, a.seq, b.seq);
    walk_state(v, At{&at, "state"}, a.state, b.state);
    v.integer(At{&at, "base"}, a.base, b.base);
    walk(v, At{&at, "runs"}, a.runs, b.runs);
    walk(v, At{&at, "damage"}, a.damage, b.damage);
}

/// The sharing patterns of the values a message carries, each within its own value.
template <class T> Sharing shares(const T&) {
    return {};
}

Sharing shares(const remote::Frame& m) {
    Sharing s = sharing(m.list);
    for (const std::vector<long>& row : sharing(m.damage)) s.push_back(row);
    return s;
}

Sharing shares(const remote::CanvasRun& m) {
    return sharing(m.ops);
}

Sharing shares(const remote::FontAnswer& m) {
    return sharing(m.metrics);
}

/// Whether `b` is `a` member by member, doubles and floats by their bits, and its values shared alike; `why` names the first difference.
template <class T> bool equal(const T& a, const T& b, std::string& why) {
    Equal e;
    walk(e, At{}, a, b);
    why = e.first;
    if (e.differences) return false;
    if (shares(a) == shares(b)) return true;
    why = "the payloads are shared otherwise";
    return false;
}

/// Which members a sample sets apart from a default message, and which it has, each named up to its first list item or the value it carries.
struct Cover {
    std::set<std::string> differ, all;

    void note(const At& at, bool differs) {
        std::string leaf = where(at, false);
        if (in_item(at)) leaf.resize(leaf.find('['));
        for (const char* value : {"list", "damage", "ops", "metrics", "rgba"})
            if (leaf.rfind(value, 0) == 0) leaf = value;
        all.insert(leaf);
        if (differs) differ.insert(leaf);
    }
    void f32(const At& at, const float& a, const float& b) { note(at, bits(a) != bits(b)); }
    void integer(const At& at, uint64_t a, uint64_t b) { note(at, a != b); }
    void raw(const At& at, std::string_view a, std::string_view b) { note(at, !same_bytes(a, b)); }
};

/// The objects an output holds, to tell that a refusal kept them rather than equal copies.
template <class T> std::vector<const void*> held(const T&) {
    return {};
}

std::vector<const void*> held(const remote::Frame& m) {
    std::vector<const void*> out{m.list.data(), m.damage ? m.damage->data() : nullptr};
    for (const Command& c : m.list) {
        out.push_back(c.path.get());
        out.push_back(c.filter.get());
    }
    return out;
}

std::vector<const void*> held(const remote::CanvasRun& m) {
    std::vector<const void*> out{m.ops.data()};
    for (const CanvasOp& o : m.ops) {
        out.push_back(o.clips.get());
        out.push_back(o.rgba.get());
    }
    return out;
}

std::vector<const void*> held(const remote::Messages& m) {
    std::vector<const void*> out{m.texts.data()};
    for (const std::string& t : m.texts) out.push_back(t.data());
    return out;
}

std::vector<const void*> held(const remote::PixelsAnswer& m) {
    return {m.rgba.data()};
}

std::vector<const void*> held(const remote::Pointer& m) {
    return {m.cursor.data()};
}

std::vector<const void*> held(const remote::Selection& m) {
    return {m.text.data()};
}

std::vector<const void*> held(const remote::FontRequest& m) {
    return {m.families.data(), m.features.data()};
}

std::vector<const void*> held(const remote::WidthRequest& m) {
    return {m.text.data()};
}

std::vector<const void*> held(const remote::ImageRequest& m) {
    return {m.url.data()};
}

std::vector<const void*> held(const remote::WidthsAnswer& m) {
    return {m.widths.data()};
}

std::vector<const void*> held(const remote::WidthsRequest& m) {
    std::vector<const void*> out{m.texts.data()};
    for (const TextInFont& t : m.texts) out.push_back(t.text.data());
    return out;
}

std::vector<const void*> held(const remote::FrameDelta& m) {
    std::vector<const void*> out{m.runs.data(), m.damage ? m.damage->data() : nullptr};
    for (const remote::DeltaRun& run : m.runs) out.push_back(run.literal.data());
    return out;
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

/// A used output for a decode to replace or leave alone: populated, and unlike every sample.
void make_sentinel(remote::FrameRequest& m) {
    m = {0x5e5e5e, true, remote::Paint::Skip};
}

void make_sentinel(remote::SetWidth& m) {
    m.width = -12.5;
}

void make_sentinel(remote::SetMaxHeight& m) {
    m.height = 777.25;
}

void make_sentinel(remote::SetTheme& m) {
    m.dark = true;
}

void make_sentinel(remote::Touch& m) {
    m = {9, -1.5, 2.5, 3, true, false, 4, 1};
}

void make_sentinel(remote::Hover& m) {
    m = {8, 7.5, 6.5, 5};
}

void make_sentinel(remote::ClearSelection&) {}

void make_sentinel(remote::ClaimQuery& m) {
    m = {99, 1.25, 2.5};
}

void make_sentinel(remote::FontAnswer& m) {
    m.font = 77;
    make_sentinel(m.metrics);
}

void make_sentinel(remote::WidthAnswer& m) {
    m.width = 9.75f;
}

void make_sentinel(remote::ImageAnswer& m) {
    m = {true, 5, 6};
}

void make_sentinel(remote::PixelsAnswer& m) {
    m.ok = true;
    m.rgba.assign(12, 0x5a);
}

void make_sentinel(remote::State& m) {
    m = {4.5, 6, true, false, 1.5, true};
}

void make_sentinel(remote::Frame& m) {
    auto path = std::make_shared<Path>();
    path->ops = {Path::Move, Path::Line};
    path->points = {1, 2, 3, 4};
    m.seq = 55;
    m.state = {1, 2, true, true, 3, true};
    m.painted = true;
    m.list.resize(2);
    m.list[0].kind = Cmd::Path;
    m.list[0].path = path;
    m.list[0].text = "a sentinel that a refused decode leaves alone";
    m.list[1].kind = Cmd::Path;
    m.list[1].path = path;
    m.damage = std::vector<Rect>{{1, 2, 3, 4}, {5, 6, 7, 8}};
}

void make_sentinel(remote::Messages& m) {
    m.texts = {"a sentinel that a refused decode leaves alone", "a second sentinel text"};
}

void make_sentinel(remote::Pointer& m) {
    m.cursor = "a sentinel cursor longer than a small string's own room";
    m.claim = twidget::WidgetGestureClaim{true, true, false};
}

void make_sentinel(remote::Selection& m) {
    m.text = "a sentinel selection longer than a small string's own room";
}

void make_sentinel(remote::ClaimAnswer& m) {
    m = {44, {true, true, true}};
}

void make_sentinel(remote::FontRelease& m) {
    m.font = 66;
}

void make_sentinel(remote::CanvasRun& m) {
    auto clips = std::make_shared<Clips>(1);
    (*clips)[0].path.ops = {Path::Move, Path::Line, Path::Close};
    (*clips)[0].path.points = {0, 0, 8, 8};
    auto rgba = std::make_shared<Pixels>(16, uint8_t(0xa5));
    m.id = 5;
    m.w = 6;
    m.h = 7;
    m.ops.resize(2);
    m.ops[0].clips = clips;
    m.ops[0].rgba = rgba;
    m.ops[1].kind = CanvasOp::PutImage;
    m.ops[1].clips = clips;
    m.ops[1].rgba = rgba;
}

void make_sentinel(remote::CanvasRelease& m) {
    m.id = 88;
}

void make_sentinel(remote::FontRequest& m) {
    m = {false, "a sentinel family list longer than a small string's own room", 9, 300, true, "liga=0,and a long tail after it"};
}

void make_sentinel(remote::WidthRequest& m) {
    m = {11, "a sentinel text longer than a small string's own room"};
}

void make_sentinel(remote::ImageRequest& m) {
    m = {12, "data:a sentinel url longer than a small string's own room"};
}

void make_sentinel(remote::PixelsRequest& m) {
    m = {1, 2, 3, 4, 5};
}

void make_sentinel(remote::WidthsAnswer& m) {
    m.widths = {1.5f, -2.25f, 1e9f};
}

void make_sentinel(remote::WidthsRequest& m) {
    m.texts = {{13, "a sentinel text longer than a small string's own room"}, {-14, "and a second"}};
}

void make_sentinel(remote::FrameDelta& m) {
    m.seq = 66;
    m.state = {7, 8, true, false, 9, true};
    m.base = 65;
    m.runs = {{1, 2, {}}, {0, 0, "a sentinel literal longer than a small string's own room"}};
    m.damage = std::vector<Rect>{{1, 2, 3, 4}};
}

template <class T> T sentinel() {
    T value;
    make_sentinel(value);
    return value;
}

/// The request an answer decodes against: a PixelsAnswer's and a WidthsAnswer's own, a pixels request standing in for the others.
template <class T> struct AskedBy {
    using type = remote::PixelsRequest;
};

template <> struct AskedBy<remote::WidthsAnswer> {
    using type = remote::WidthsRequest;
};

template <class T> using AskedFor = typename AskedBy<T>::type;

template <class T> bool decode_as(std::string_view bytes, const remote::Caps& caps, const AskedFor<T>&, T& out) {
    return remote::decode(bytes, caps, out);
}

bool decode_as(std::string_view bytes, const remote::Caps& caps, const remote::PixelsRequest& asked, remote::PixelsAnswer& out) {
    return remote::decode(bytes, caps, asked, out);
}

bool decode_as(std::string_view bytes, const remote::Caps& caps, const remote::WidthsRequest& asked, remote::WidthsAnswer& out) {
    return remote::decode(bytes, caps, asked, out);
}

/// Every decode these checks make goes through here, and so through the allocation scope; `asked` is the request a PixelsAnswer or a WidthsAnswer answers.
template <class T> bool decoded(std::string_view bytes, const remote::Caps& caps, T& out, const AskedFor<T>& asked = {}) {
    return in_scope(bytes, [&] { return decode_as(bytes, caps, asked, out); });
}

/// `bytes` refused, and the output the decoder was given still the same value holding the same objects.
template <class T> void refused(const std::string& what, std::string_view bytes, const remote::Caps& caps, const AskedFor<T>& asked = {}) {
    const T expected = sentinel<T>();
    T out = expected;
    const std::vector<const void*> before = held(out);
    const bool taken = decoded(bytes, caps, out, asked);
    std::string why;
    const bool kept = equal(expected, out, why) && held(out) == before;
    verify(!taken && kept, [&] { return what + (taken ? ": taken" : ": the output changed " + why); });
}

/// Caps each small, that every sample still fits.
remote::Caps tight() {
    remote::Caps c;
    c.message = 4096;
    c.text = 64;
    c.url = 128;
    c.messages = 4;
    c.widths = 4;
    c.widths_text = 32;
    c.values.commands = 8;
    c.values.canvas_ops = 8;
    c.values.path_ops = 16;
    c.values.floats = 32;
    c.values.stops = 4;
    c.values.filters = 4;
    c.values.clips = 4;
    c.values.text = 64;
    c.values.pixels = 64;
    c.values.rects = 4;
    c.frame = 4096;
    c.delta_base = 64;
    return c;
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
    Bytes& i32(int32_t v) { return u32(uint32_t(v)); }
    Bytes& f32(const float& v) { return u32(bits(v)); }
    Bytes& f64(const double& v) { return u64(bits64(v)); }
    Bytes& head(Kind kind) { return u8(uint32_t(kind)).u8(1); }
    Bytes& str(std::string_view text) {
        u32(uint32_t(text.size()));
        s.append(text);
        return *this;
    }
    Bytes& raw(std::string_view bytes) {
        s.append(bytes);
        return *this;
    }
    Bytes& state(const remote::State& m) { return f64(m.width).i32(m.height).u8(m.dirty).u8(m.wants_tick).f64(m.next_tick_in).u8(m.frame_asked); }
    /// A u32 length and a value's bytes.
    Bytes& value(std::string_view bytes) { return u32(uint32_t(bytes.size())).raw(bytes); }
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

template <class T> std::string encoded(const T& value, const remote::Caps& caps = {}) {
    std::string out;
    if (!remote::encode(value, caps, out)) check(false, "a value the checks build encodes");
    return out;
}

// ───────────────────────── Samples ─────────────────────────

template <class T> struct Sample {
    Sample(std::string name, T value, AskedFor<T> asked = {}) : name(std::move(name)), value(std::move(value)), asked(std::move(asked)) {}

    std::string name;
    T value;
    AskedFor<T> asked;
};

template <class T> using Samples = std::vector<Sample<T>>;

/// Five commands, two of them sharing a path, one a signalling NaN in its rect, within tight() caps.
DisplayList compact_list() {
    auto path = std::make_shared<Path>();
    path->ops = {Path::Move, Path::Line, Path::Close};
    path->points = {0, 0, 10, 0};
    path->fill = true;
    auto filter = std::make_shared<Filters>(1);
    (*filter)[0] = Filter{Filter::Blur, 2.5f, 0, 0, {}};
    DisplayList list(5);
    list[0].rect = {1, 2, 30, 40};
    put_bits(list[0].rect.x, SPECIAL_FLOATS[2]);
    list[0].color = {10, 20, 30, 255};
    list[0].key = 7;
    list[1].kind = Cmd::Text;
    list[1].text = "hello";
    list[1].font = 3;
    list[1].x = 1.5f;
    list[1].y = 12;
    list[2].kind = Cmd::Path;
    list[2].path = path;
    list[3].kind = Cmd::Path;
    list[3].path = path;
    list[3].opacity = 0.5f;
    list[4].kind = Cmd::FilterPush;
    list[4].filter = filter;
    list[4].rect = {0, 0, 100, 100};
    return list;
}

/// Five ops: two sharing a clip list, two sharing their pixels, within tight() caps.
Batch compact_batch() {
    auto clips = std::make_shared<Clips>(1);
    (*clips)[0].path.ops = {Path::Move, Path::Line, Path::Line, Path::Close};
    (*clips)[0].path.points = {0, 0, 4, 0, 4, 4};
    auto rgba = std::make_shared<Pixels>(16, uint8_t(0x3c));
    Batch ops(5);
    ops[0].kind = CanvasOp::Reset;
    ops[1].path.ops = {Path::Move, Path::Line, Path::Close};
    ops[1].path.points = {1, 1, 9, 9};
    ops[1].paint.color = {200, 100, 50, 255};
    ops[1].clips = clips;
    ops[2].kind = CanvasOp::Text;
    ops[2].text = "canvas";
    ops[2].font = 4;
    ops[2].clips = clips;
    ops[3].kind = CanvasOp::PutImage;
    ops[3].rgba = rgba;
    ops[3].w = 2;
    ops[3].h = 2;
    ops[4].kind = CanvasOp::PutImage;
    ops[4].rgba = rgba;
    ops[4].w = 2;
    ops[4].h = 2;
    ops[4].dx = 3;
    return ops;
}

FontMetrics ordinary_metrics() {
    FontMetrics m;
    m.ascent = 10.5f;
    m.descent = 3.25f;
    m.height = 16;
    m.x_height = 5.5f;
    m.zero_width = 7.125f;
    m.em_ascent = 9.75f;
    m.em_descent = 2.25f;
    return m;
}

FontMetrics special_metrics() {
    FontMetrics m;
    float* fields[] = {&m.ascent, &m.descent, &m.height, &m.x_height, &m.zero_width, &m.em_ascent, &m.em_descent};
    for (size_t i = 0; i < std::size(fields); i++) put_bits(*fields[i], SPECIAL_FLOATS[i]);
    return m;
}

remote::State ordinary_state() {
    return {320.5, 480, true, true, 0.016, true};
}

remote::State special_state() {
    remote::State s;
    put_bits(s.width, SPECIAL_DOUBLES[2]);
    s.height = INT32_MIN;
    s.wants_tick = true;
    put_bits(s.next_tick_in, SPECIAL_DOUBLES[0]);
    return s;
}

Samples<remote::FrameRequest> frame_requests() {
    return {{"default", {}}, {"a forced tick", {0x0123456789abcdef, true, remote::Paint::Force}}, {"a skipped frame", {UINT64_MAX, false, remote::Paint::Skip}},
        {"a tick painted if dirty", {1, true, remote::Paint::IfDirty}}};
}

Samples<remote::SetWidth> set_widths() {
    Samples<remote::SetWidth> out{{"default", {}}, {"a width", {320.5}}};
    for (size_t i = 0; i < SPECIALS; i++) {
        remote::SetWidth m;
        put_bits(m.width, SPECIAL_DOUBLES[i]);
        out.push_back({"special " + hex(SPECIAL_DOUBLES[i]), m});
    }
    return out;
}

Samples<remote::SetMaxHeight> set_max_heights() {
    Samples<remote::SetMaxHeight> out{{"default", {}}, {"a height", {1e6}}};
    remote::SetMaxHeight m;
    put_bits(m.height, SPECIAL_DOUBLES[5]);
    out.push_back({"minus infinity", m});
    return out;
}

Samples<remote::SetTheme> set_themes() {
    return {{"light", {}}, {"dark", {true}}};
}

Samples<remote::Touch> touches() {
    remote::Touch extreme{INT32_MIN, 0, 0, INT32_MAX, false, true, -1, INT32_MIN};
    put_bits(extreme.x, SPECIAL_DOUBLES[2]);
    put_bits(extreme.y, SPECIAL_DOUBLES[3]);
    return {{"default", {}}, {"a mouse press", {2, 10.25, -3.5, 7, true, true, 0x0f, 2}}, {"extremes", extreme}};
}

Samples<remote::Hover> hovers() {
    remote::Hover extreme{-1, 0, 0, INT32_MAX};
    put_bits(extreme.x, SPECIAL_DOUBLES[4]);
    put_bits(extreme.y, SPECIAL_DOUBLES[6]);
    return {{"default", {}}, {"a move", {1, 5.5, 6.5, 3}}, {"extremes", extreme}};
}

Samples<remote::ClearSelection> clear_selections() {
    return {{"the only form", {}}};
}

Samples<remote::ClaimQuery> claim_queries() {
    remote::ClaimQuery extreme{UINT64_MAX, 0, 0};
    put_bits(extreme.x, SPECIAL_DOUBLES[5]);
    put_bits(extreme.y, SPECIAL_DOUBLES[1]);
    return {{"default", {}}, {"a query", {42, 100.5, 200.25}}, {"extremes", extreme}};
}

Samples<remote::FontAnswer> font_answers() {
    return {{"default", {}}, {"ordinary metrics", {5, ordinary_metrics()}}, {"special metrics", {INT32_MIN, special_metrics()}}};
}

Samples<remote::WidthAnswer> width_answers() {
    Samples<remote::WidthAnswer> out{{"default", {}}, {"a width", {42.25f}}};
    for (size_t i = 0; i < SPECIALS; i++) {
        remote::WidthAnswer m;
        put_bits(m.width, SPECIAL_FLOATS[i]);
        out.push_back({"special " + hex(SPECIAL_FLOATS[i]), m});
    }
    return out;
}

Samples<remote::ImageAnswer> image_answers() {
    remote::ImageAnswer special{false, 3.5f, 0};
    put_bits(special.h, SPECIAL_FLOATS[1]);
    return {{"default", {}}, {"a decoded image", {true, 64, 48}}, {"a refusal that sizes", special}};
}

Samples<remote::PixelsAnswer> pixels_answers() {
    remote::PixelsAnswer six{true, {}};
    for (uint32_t i = 0; i < 24; i++) six.rgba.push_back(uint8_t(i * 11));
    return {{"unsupported", {}, {1, 0, 0, 2, 3}}, {"2 × 3 pixels", six, {1, 4, 5, 2, 3}}, {"no pixels", {true, {}}, {2, 5, 5, 0, 7}},
        {"unsupported for the largest", {}, {3, -5, -6, 8192, 2048}}};
}

/// Four texts in two fonts: what the widths answers below answer.
remote::WidthsRequest four_texts() {
    return {{{1, "a"}, {1, "bb"}, {2, "a"}, {2, ""}}};
}

Samples<remote::WidthsAnswer> widths_answers() {
    remote::WidthsAnswer first, second;
    for (size_t i = 0; i < SPECIALS; i++) put_bits((i < 4 ? first : second).widths.emplace_back(), SPECIAL_FLOATS[i]);
    return {{"none", {}}, {"one width", {{42.25f}}, {{{3, "word"}}}}, {"special widths", first, four_texts()}, {"more special widths", second, four_texts()},
        {"ordinary widths", {{8.f, 0.f, 16.5f, 1e-3f}}, four_texts()}};
}

Samples<remote::Frame> frames() {
    remote::Frame unchanged{9, ordinary_state(), false, {}, std::nullopt};
    remote::Frame whole{10, ordinary_state(), true, compact_list(), std::nullopt};
    remote::Frame still{11, {}, true, {}, std::vector<Rect>()};
    remote::Frame rects{UINT64_MAX, special_state(), true, compact_list(), std::vector<Rect>{{1, 2, 3, 4}, {0, 0, 0, 0}}};
    put_bits((*rects.damage)[1].w, SPECIAL_FLOATS[0]);
    return {{"default", {}}, {"unchanged", unchanged}, {"painted whole", whole}, {"painted with nothing moved", still}, {"painted with rects", rects}};
}

Samples<remote::State> states() {
    return {{"default", {}}, {"ordinary", ordinary_state()}, {"special", special_state()}};
}

Samples<remote::Messages> messages() {
    remote::Messages several{{R"({"type":"gesture","wants":false,"x":true,"y":false,"hold":true})", "", std::string("\0\xff\x01 bytes", 9), R"({"type":"limit"})"}};
    return {{"none", {}}, {"one", {{R"({"type":"size","width":320,"height":96})"}}}, {"several", several}};
}

Samples<remote::Pointer> pointers() {
    return {{"default", {}}, {"a cursor", {"pointer", std::nullopt}}, {"claimed nothing", {"auto", twidget::WidgetGestureClaim{}}},
        {"claimed a hold", {"text", twidget::WidgetGestureClaim{true, false, true}}}, {"claimed all", {"", twidget::WidgetGestureClaim{true, true, true}}}};
}

Samples<remote::Selection> selections() {
    return {{"none", {}}, {"text", {"selected\ntext"}}, {"bytes", {std::string("\0\xff caf\xc3\xa9", 8)}}};
}

Samples<remote::ClaimAnswer> claim_answers() {
    return {{"default", {}}, {"a hold", {7, {true, false, true}}}, {"extremes", {UINT64_MAX, {false, true, false}}}};
}

Samples<remote::FontRelease> font_releases() {
    return {{"default", {}}, {"a font", {12}}, {"the least", {INT32_MIN}}};
}

Samples<remote::CanvasRun> canvas_runs() {
    return {{"default", {}}, {"a batch", {3, 300, 150, compact_batch()}}, {"extremes, no ops", {INT32_MAX, -1, INT32_MIN, {}}}};
}

Samples<remote::CanvasRelease> canvas_releases() {
    return {{"default", {}}, {"a canvas", {3}}, {"the least", {INT32_MIN}}};
}

Samples<remote::FontRequest> font_requests() {
    remote::FontRequest special{false, "", 0, INT32_MIN, false, ""};
    put_bits(special.size, SPECIAL_FLOATS[2]);
    return {{"default", {}}, {"a document font", {false, "\"Inter\", sans-serif", 13.5f, 600, true, "tnum=1,zero=1"}}, {"a canvas font", {true, "serif", 10, 400, true, ""}},
        {"special", special}};
}

Samples<remote::WidthRequest> width_requests() {
    return {{"default", {}}, {"a text", {3, "hello"}}, {"bytes", {-1, std::string("\0\xff\n", 3)}}};
}

Samples<remote::ImageRequest> image_requests() {
    return {{"default", {}}, {"a data URL", {5, "data:image/png;base64,iVBORw0KGgo="}}, {"the least", {INT32_MIN, "x"}}};
}

Samples<remote::WidthsRequest> widths_requests() {
    const remote::WidthsRequest several{{{3, "hello"}, {-1, std::string("\0\xff\n", 3)}, {INT32_MIN, ""}, {3, "hello"}}};
    return {{"none", {}}, {"one text", {{{7, "word"}}}}, {"repeated, empty and bytes", several}, {"the extremes", {{{INT32_MAX, " "}, {0, std::string(28, 'x')}}}}};
}

Samples<remote::PixelsRequest> pixels_requests() {
    return {{"default", {}}, {"a region", {1, 10, 20, 30, 40}}, {"the most pixels", {INT32_MIN, INT32_MIN, INT32_MAX, 8192, 2048}},
        {"the most of both sides", {2, -1, -1, 4096, 4096}}, {"the widest, empty", {3, 0, 0, 8192, 0}}};
}

/// Within tight() caps, and each no shorter than the Frame it stands for, so that a message cap of its size takes it.
Samples<remote::FrameDelta> frame_deltas() {
    remote::FrameDelta literal{10, ordinary_state(), 9, {{0, 0, std::string("\x01\x01\x00\x00\x00\x00", 6)}}, std::nullopt};
    remote::FrameDelta mixed{UINT64_MAX, special_state(), UINT64_MAX - 1, {{0, 6, {}}, {0, 0, "abc"}, {55, 9, {}}, {0, 0, std::string("\0\xff", 2)}},
        std::vector<Rect>{{1, 2, 3, 4}, {0, 0, 0, 0}}};
    put_bits((*mixed.damage)[1].w, SPECIAL_FLOATS[0]);
    remote::FrameDelta still{1, {}, 0, {{3, 1, {}}}, std::vector<Rect>()};
    return {{"one literal", literal}, {"copies and literals", mixed}, {"one byte copied, nothing moved", still}};
}

// ───────────────────────── Every kind ─────────────────────────

/// What the checks did with one decoder.
struct Report {
    std::string name;
    Kind kind = Kind::FrameRequest;
    size_t samples = 0, decodes = 0, cuts = 0, heads = 0, accepted = 0, refused = 0;
};

std::vector<Report> reports;

/// The sample encodes and decodes back the same member by member: into an empty output and a used one, after a prefix, and owning what it holds once the bytes are gone.
template <class T> void round_trip(const std::string& what, Kind kind, const Sample<T>& s, const remote::Caps& caps) {
    std::string bytes;
    if (!remote::encode(s.value, caps, bytes)) {
        check(false, what + ": encodes");
        return;
    }
    check(remote::kind_of(bytes) == kind, what + ": its header names its kind");
    std::string why;
    T fresh;
    check(decoded(bytes, caps, fresh, s.asked) && equal(s.value, fresh, why), what + ": decodes the same " + why);
    T used = sentinel<T>();
    check(decoded(bytes, caps, used, s.asked) && equal(s.value, used, why), what + ": replaces a used output " + why);
    std::string again;
    check(remote::encode(fresh, caps, again) && again == bytes, what + ": what it decodes to encodes to the same bytes");
    std::string after = EARLIER;
    check(remote::encode(s.value, caps, after) && after == EARLIER + bytes, what + ": appends after what the string held");
    T suffix;
    check(decoded(std::string_view(after).substr(EARLIER.size()), caps, suffix, s.asked) && equal(s.value, suffix, why), what + ": decodes from after a prefix " + why);
    auto buffer = std::make_unique<char[]>(bytes.size());
    std::memcpy(buffer.get(), bytes.data(), bytes.size());
    T owned;
    const bool ok = decoded(std::string_view(buffer.get(), bytes.size()), caps, owned, s.asked);
    std::memset(buffer.get(), 0xa5, bytes.size());
    buffer.reset();
    check(ok && equal(s.value, owned, why), what + ": keeps its own copy once the bytes are gone " + why);
}

/// Under a message cap of exactly its size the sample is taken both ways; one byte less and none, it is refused both ways, the earlier value kept; the cap leaves out a prefix.
template <class T> void message_caps(const std::string& what, const Sample<T>& s) {
    const std::string bytes = encoded(s.value);
    remote::Caps exact, short_by_one, none, open;
    exact.message = bytes.size();
    short_by_one.message = bytes.size() - 1;
    none.message = 0;
    open.message = SIZE_MAX;
    T out;
    check(decoded(bytes, exact, out, s.asked), what + ": decodes under a message cap of its size");
    check(decoded(bytes, open, out, s.asked), what + ": decodes under a message cap of SIZE_MAX");
    refused<T>(what + " one byte past the message cap", bytes, short_by_one, s.asked);
    refused<T>(what + " under a message cap of 0", bytes, none, s.asked);
    std::string kept = EARLIER;
    check(!remote::encode(s.value, short_by_one, kept) && kept == EARLIER, what + ": encoding it one byte past the message cap is refused, the earlier value kept");
    kept = EARLIER;
    check(!remote::encode(s.value, none, kept) && kept == EARLIER, what + ": encoding it under a message cap of 0 is refused, the earlier value kept");
    const std::string longer(bytes.size() + 7, 'p');
    std::string after = longer;
    check(remote::encode(s.value, exact, after) && after == longer + bytes, what + ": the message cap leaves out what the string held before");
}

/// Every proper prefix refused, and so are bytes after the message and the message twice over.
template <class T> size_t cuts(const std::string& what, const Sample<T>& s) {
    const std::string bytes = encoded(s.value);
    for (size_t n = 0; n < bytes.size(); n++) refused<T>(what + " cut to " + std::to_string(n) + " bytes", std::string_view(bytes).substr(0, n), {}, s.asked);
    refused<T>(what + " with a NUL after it", bytes + '\0', {}, s.asked);
    refused<T>(what + " with 0xff after it", bytes + '\xff', {}, s.asked);
    refused<T>(what + " twice over", bytes + bytes, {}, s.asked);
    return bytes.size() + 3;
}

/// Every other first byte, known kinds among them, and other versions are refused; kind_of names exactly the known kinds of version 1.
template <class T> size_t heads(const std::string& what, Kind kind, const Sample<T>& s) {
    const std::string bytes = encoded(s.value);
    size_t n = 0;
    for (uint32_t k = 0; k < 256; k++) {
        if (k == uint32_t(kind)) continue;
        std::string changed = bytes;
        changed[0] = char(k);
        const std::optional<Kind> named = remote::kind_of(changed);
        verify(named == (listed(k) ? std::optional<Kind>(Kind(k)) : std::nullopt), [&] { return what + " as kind " + std::to_string(k) + ": kind_of names it otherwise"; });
        refused<T>(what + " as kind " + std::to_string(k), changed, {}, s.asked);
        n++;
    }
    for (uint32_t version : {0u, 2u, 0x7fu, 0x80u, 0xffu}) {
        std::string changed = bytes;
        changed[1] = char(version);
        verify(!remote::kind_of(changed), [&] { return what + " in version " + std::to_string(version) + ": kind_of names a kind"; });
        refused<T>(what + " in version " + std::to_string(version), changed, {}, s.asked);
        n++;
    }
    return n;
}

/// Random changes to every sample under default and tight caps: a refusal leaves the output alone, and what is taken encodes again under the same caps to what decodes the same.
template <class T> void mutations(Report& report, const Samples<T>& samples, uint64_t seed) {
    Tally tally;
    const remote::Caps under[] = {remote::Caps(), tight()};
    const char* const names[] = {"default caps", "tight caps"};
    const T expected = sentinel<T>();
    for (size_t c = 0; c < std::size(under); c++) {
        size_t index = 0;
        for (const Sample<T>& s : samples) {
            const uint64_t start = seed + 0x100 * c + index++;
            std::string bytes;
            if (!remote::encode(s.value, under[c], bytes)) {
                check(false, report.name + " " + s.name + ": encodes under " + names[c]);
                continue;
            }
            context = "changes to " + report.name + " " + s.name + " under " + names[c] + " from seed " + hex(start);
            Lcg random{start};
            for (int round = 0; round < ROUNDS; round++) {
                std::string changed = bytes;
                const std::string edits = mutate(random, changed);
                const auto where = [&] { return context + ", round " + std::to_string(round) + ":" + edits; };
                T out = expected;
                const std::vector<const void*> before = held(out);
                std::string why;
                if (decoded(changed, under[c], out, s.asked)) {
                    tally.accepted++;
                    std::string again;
                    T back;
                    const bool closed = remote::encode(out, under[c], again) && decoded(again, under[c], back, s.asked) && equal(out, back, why);
                    verify(closed, [&] { return where() + ": what it decodes to does not come back the same " + why; });
                } else {
                    tally.refused++;
                    verify(equal(expected, out, why) && held(out) == before, [&] { return where() + ": the refusal changed the output " + why; });
                }
            }
        }
    }
    const bool fixed_empty = report.kind == Kind::ClearSelection;
    check(tally.refused > 0 && (tally.accepted > 0 || fixed_empty), report.name + ": random changes are refused, and decode unless the form is fixed and empty");
    report.accepted = tally.accepted;
    report.refused = tally.refused;
}

/// One decoder's samples: round trips under default and tight caps, the message cap, cuts, headers, changes, and samples that set every member.
template <class T> void suite(const char* name, Kind kind, const Samples<T>& samples, uint64_t seed) {
    Report report;
    report.name = name;
    report.kind = kind;
    report.samples = samples.size();
    const size_t start = measured_decodes;
    Cover cover;
    const T none{};
    for (const Sample<T>& s : samples) {
        const std::string what = std::string(name) + " " + s.name;
        walk(cover, At{}, s.value, none);
        context = "round trips of " + what;
        round_trip(what, kind, s, remote::Caps());
        round_trip(what + " under tight caps", kind, s, tight());
        context = "message caps of " + what;
        message_caps(what, s);
        context = "cuts of " + what;
        report.cuts += cuts(what, s);
        context = "headers of " + what;
        report.heads += heads(what, kind, s);
    }
    std::string missing;
    for (const std::string& leaf : cover.all)
        if (!cover.differ.count(leaf)) missing += " " + leaf;
    check(missing.empty(), std::string(name) + ": the samples set every member apart from its default, but not" + missing);
    mutations(report, samples, seed);
    report.decodes = measured_decodes - start;
    reports.push_back(report);
}

// ───────────────────────── Layouts ─────────────────────────

/// `value` encodes to exactly `expected` and `expected` decodes back to it.
template <class T> void golden(const std::string& what, const T& value, const std::string& expected, const AskedFor<T>& asked = {}) {
    std::string bytes;
    const bool written = remote::encode(value, {}, bytes);
    check(written && bytes == expected, what + ": encodes to the documented bytes, the first difference at byte " + std::to_string(first_difference(bytes, expected)));
    T out;
    std::string why;
    check(decoded(expected, {}, out, asked) && equal(value, out, why), what + ": the documented bytes decode to it " + why);
}

void golden_bytes() {
    context = "documented bytes";
    golden("a forced tick", remote::FrameRequest{0x0102030405060708, true, remote::Paint::Force}, unhex("01 01  08 07 06 05 04 03 02 01  01  02"));
    remote::State state{320.5, -7, true, false, std::numeric_limits<double>::infinity(), true};
    golden("a state", state, unhex("41 01  00 00 00 00 00 08 74 40  f9 ff ff ff  01  00  00 00 00 00 00 00 f0 7f  01"));
    golden("a claimed pointer", remote::Pointer{"pointer", twidget::WidgetGestureClaim{true, false, true}}, unhex("43 01  07 00 00 00 70 6f 69 6e 74 65 72  01  01 00 01"));
    golden("an unclaimed pointer", remote::Pointer{"", std::nullopt}, unhex("43 01  00 00 00 00  00"));
    golden("clearing the selection", remote::ClearSelection{}, unhex("07 01"));
    remote::Frame painted{2, {}, true, {}, std::nullopt};
    golden("a painted frame of nothing",
        painted, unhex("40 01  02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  01  06 00 00 00 01 01 00 00 00 00  03 00 00 00 02 01 00"));
    golden("an unpainted frame", remote::Frame{3, {}, false, {}, std::nullopt},
        unhex("40 01  03 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  00"));
    golden("a canvas font request", remote::FontRequest{true, "a", 1, 2, false, ""}, unhex("60 01  01  01 00 00 00 61  00 00 80 3f  02 00 00 00  00"));
    golden("a document font request", remote::FontRequest{false, "a", 1, 2, true, "b"}, unhex("60 01  00  01 00 00 00 61  00 00 80 3f  02 00 00 00  01  01 00 00 00 62"));
    golden("a pixels request", remote::PixelsRequest{1, -1, 2, 3, 4}, unhex("63 01  01 00 00 00  ff ff ff ff  02 00 00 00  03 00 00 00  04 00 00 00"));
    golden("messages", remote::Messages{{"ab", ""}}, unhex("42 01  02 00 00 00  02 00 00 00 61 62  00 00 00 00"));
    const remote::WidthsRequest two{{{1, "ab"}, {-1, ""}}};
    golden("a widths request", two, unhex("64 01  02 00 00 00  01 00 00 00  02 00 00 00 61 62  ff ff ff ff  00 00 00 00"));
    golden("a widths answer", remote::WidthsAnswer{{1.f, -0.f}}, unhex("24 01  02 00 00 00  00 00 80 3f  00 00 00 80"), two);
    golden("an empty widths request", remote::WidthsRequest{}, unhex("64 01  00 00 00 00"));
    golden("an empty widths answer", remote::WidthsAnswer{}, unhex("24 01  00 00 00 00"));
    std::string metrics;
    wire::encode(ordinary_metrics(), metrics);
    golden("a font answer", remote::FontAnswer{-2, ordinary_metrics()}, Bytes().head(Kind::FontAnswer).i32(-2).value(metrics).s);
    std::string batch;
    check(wire::encode(compact_batch(), {}, batch), "the compact batch encodes");
    golden("a canvas run", remote::CanvasRun{1, 2, 3, compact_batch()}, Bytes().head(Kind::CanvasRun).i32(1).i32(2).i32(3).value(batch).s);
    std::string run;
    const Batch ops = compact_batch();
    check(remote::encode_canvas_run(1, 2, 3, ops, {}, run) && run == encoded(remote::CanvasRun{1, 2, 3, ops}), "encode_canvas_run writes what a CanvasRun encodes to");
    std::string kept = EARLIER;
    remote::Caps small;
    small.values.canvas_ops = 4;
    check(!remote::encode_canvas_run(1, 2, 3, ops, small, kept) && kept == EARLIER, "encode_canvas_run refuses a batch past its cap, the earlier value kept");

    golden("a frame delta", remote::FrameDelta{2, {}, 1, {{6, 4, {}}, {0, 0, "ab"}}, std::nullopt},
        unhex("49 01  02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  00 00 00 00  00  00  00 00 00 00 00 00 f0 7f  00  01 00 00 00 00 00 00 00  06 00 00 00  02 00 00 00"
              "  00  06 00 00 00  04 00 00 00  01  02 00 00 00 61 62  03 00 00 00 02 01 00"));
    std::string list;
    std::vector<size_t> starts;
    check(wire::encode(compact_list(), {}, list, starts), "the compact list encodes with its starts");
    const remote::Frame frame{5, ordinary_state(), true, compact_list(), std::vector<Rect>{{1, 2, 3, 4}}};
    const remote::WrittenList untouched{1, 2, {3}};
    remote::WrittenList where = untouched;
    std::string written = EARLIER;
    check(remote::encode(frame, {}, written, list.size(), where) && written == EARLIER + encoded(frame), "a painted Frame written with its list's place is the Frame's own bytes");
    check(where.at == EARLIER.size() + 2 + 8 + 23 + 1 + 4 && where.size == list.size() && written.compare(where.at, where.size, list) == 0 && where.starts == starts,
        "its list lies where the place says, after the header, seq, state, flag and length, with the list's own starts");
    written.clear();
    check(remote::encode(frame, {}, written, list.size() - 1, where) && written == encoded(frame) && where.size == list.size() && where.starts.empty(),
        "a list a byte past the bytes given is written the same, with no starts");
    remote::Caps one_short;
    one_short.frame = encoded(frame).size() - 1;
    written = EARLIER;
    where = untouched;
    check(!remote::encode(frame, one_short, written, SIZE_MAX, where) && written == EARLIER && where.at == 1 && where.size == 2 && where.starts == untouched.starts,
        "a Frame past the frame cap is refused, the earlier value and the place kept");
    const remote::Frame unpainted{6, {}, false, {}, std::nullopt};
    written.clear();
    check(remote::encode(unpainted, {}, written, SIZE_MAX, where) && written == encoded(unpainted) && where.size == 0 && where.starts.empty(), "an unpainted Frame has no list to place");
    remote::Frame out;
    std::string_view viewed = "kept";
    const std::string bytes = encoded(frame);
    check(remote::decode(bytes, {}, out, viewed) && viewed == list && viewed.data() >= bytes.data() && viewed.data() < bytes.data() + bytes.size(),
        "a painted Frame's list is viewed where it lies in the message");
    viewed = "kept";
    check(remote::decode(encoded(remote::Frame{6, {}, false, {}, std::nullopt}), {}, out, viewed) && viewed.empty(), "an unpainted Frame views no list");
    viewed = "kept";
    check(!remote::decode(bytes, one_short, out, viewed) && viewed == "kept", "a Frame past the frame cap is refused, the view kept");
    uint64_t seq = 0, base = 0;
    const std::string delta = encoded(remote::FrameDelta{UINT64_MAX, special_state(), 0x0102030405060708, {{0, 0, "x"}}, std::nullopt});
    check(remote::delta_ids(delta, seq, base) && seq == UINT64_MAX && base == 0x0102030405060708, "delta_ids reads a FrameDelta's seq and base");
    seq = base = 7;
    check(!remote::delta_ids(delta.substr(0, 40), seq, base) && seq == 7 && base == 7, "delta_ids needs both fields whole");
    check(!remote::delta_ids(bytes, seq, base) && seq == 7 && base == 7, "delta_ids reads no other kind");
}

/// The kinds: 28 of them in four ranges by direction and role, kind_of naming exactly those of version 1 and looking at the header alone.
void kinds() {
    context = "kinds";
    std::set<uint32_t> values;
    size_t platform = 0, answers = 0, renderer = 0, requests = 0;
    for (Kind k : KINDS) {
        const uint32_t v = uint32_t(k);
        values.insert(v);
        platform += v >= 1 && v <= 8;
        answers += v >= 32 && v <= 36;
        renderer += v >= 64 && v <= 73;
        requests += v >= 96 && v <= 100;
    }
    check(values.size() == 28 && platform == 8 && answers == 5 && renderer == 10 && requests == 5, "28 kinds: 8 from the platform, 5 answers, 10 from the renderer, 5 requests");
    check(uint32_t(Kind::WidthsAnswer) - uint32_t(Kind::FontAnswer) == uint32_t(Kind::WidthsRequest) - uint32_t(Kind::FontRequest), "each answer is its request's kind less 64");
    check(remote::VERSION == 1, "version 1");
    for (uint32_t k = 0; k < 256; k++) {
        const std::optional<Kind> named = remote::kind_of(Bytes().u8(k).u8(1).s);
        check(named == (listed(k) ? std::optional<Kind>(Kind(k)) : std::nullopt), "kind_of names kind " + std::to_string(k) + " exactly when it is known");
        for (uint32_t version : {0u, 2u, 0xffu})
            check(!remote::kind_of(Bytes().u8(k).u8(version).s), "kind_of names no kind " + std::to_string(k) + " of version " + std::to_string(version));
        check(!remote::kind_of(Bytes().u8(k).s), "kind_of needs two bytes, given kind " + std::to_string(k) + " alone");
    }
    check(!remote::kind_of(std::string_view()), "kind_of names no kind of no bytes");
    const std::string not_a_frame = Bytes().head(Kind::Frame).raw("not a frame").s;
    remote::Frame out;
    check(remote::kind_of(not_a_frame) == Kind::Frame && !decoded(not_a_frame, {}, out), "kind_of reads the header alone: what it names may still be refused");
}

/// A flag at byte `at` of a sample: any value but 0 and 1 refused; the other of them taken when `plain`, refused when the flag says which fields follow.
template <class T> void flag_at(const std::string& what, const T& value, size_t at, bool plain, const AskedFor<T>& asked = {}) {
    const std::string bytes = encoded(value);
    if (at >= bytes.size() || uint8_t(bytes[at]) > 1) {
        check(false, what + ": the sample has a flag at byte " + std::to_string(at));
        return;
    }
    for (uint32_t b : {2u, 0x80u, 0xffu}) {
        std::string changed = bytes;
        changed[at] = char(b);
        refused<T>(what + " as " + hex(b), changed, {}, asked);
    }
    std::string toggled = bytes;
    toggled[at] = char(toggled[at] ^ 1);
    if (plain) {
        T out;
        std::string again;
        check(decoded(toggled, {}, out, asked) && remote::encode(out, {}, again) && again == toggled, what + ": the other value is taken and written back");
    } else {
        refused<T>(what + " turned without its fields", toggled, {}, asked);
    }
}

void flags() {
    context = "flags";
    flag_at("a frame request's tick", remote::FrameRequest{}, 10, true);
    flag_at("a theme's dark", remote::SetTheme{}, 2, true);
    flag_at("a touch's primary", remote::Touch{}, 26, true);
    flag_at("a touch's mouse", remote::Touch{}, 27, true);
    flag_at("an image answer's ok", remote::ImageAnswer{}, 2, true);
    flag_at("a failed pixels answer's ok", remote::PixelsAnswer{}, 2, false, {0, 0, 0, 1, 1});
    remote::PixelsAnswer four{true, {1, 2, 3, 4}};
    flag_at("a pixels answer's ok", four, 2, false, {0, 0, 0, 1, 1});
    flag_at("a state's dirty", remote::State{}, 14, true);
    flag_at("a state's wants_tick", remote::State{}, 15, true);
    flag_at("a state's frame_asked", remote::State{}, 24, true);
    flag_at("a frame's dirty", remote::Frame{}, 22, true);
    flag_at("a frame's wants_tick", remote::Frame{}, 23, true);
    flag_at("a frame's frame_asked", remote::Frame{}, 32, true);
    flag_at("an unpainted frame's painted", remote::Frame{}, 33, false);
    flag_at("a painted frame's painted", remote::Frame{1, {}, true, {}, std::nullopt}, 33, false);
    const remote::Pointer claimed{"", twidget::WidgetGestureClaim{}};
    flag_at("an unclaimed pointer's claimed", remote::Pointer{}, 6, false);
    flag_at("a claimed pointer's claimed", claimed, 6, false);
    flag_at("a pointer claim's x", claimed, 7, true);
    flag_at("a pointer claim's y", claimed, 8, true);
    flag_at("a pointer claim's hold", claimed, 9, true);
    flag_at("a claim answer's x", remote::ClaimAnswer{}, 10, true);
    flag_at("a claim answer's y", remote::ClaimAnswer{}, 11, true);
    flag_at("a claim answer's hold", remote::ClaimAnswer{}, 12, true);
    flag_at("a document font request's canvas", remote::FontRequest{}, 2, false);
    flag_at("a canvas font request's canvas", remote::FontRequest{true, "", 0, 0, false, ""}, 2, false);
    flag_at("a font request's italic", remote::FontRequest{}, 15, true);
    flag_at("a canvas font request's italic", remote::FontRequest{true, "", 0, 0, false, ""}, 15, true);
    const std::string request = encoded(remote::FrameRequest{});
    for (uint32_t paint = 0; paint < 256; paint++) {
        std::string changed = request;
        changed[11] = char(paint);
        remote::FrameRequest out;
        if (paint <= 2) {
            check(decoded(changed, {}, out) && out.paint == remote::Paint(paint) && encoded(out) == changed, "a frame request's paint " + std::to_string(paint) + " is taken");
        } else {
            refused<remote::FrameRequest>("a frame request's paint " + std::to_string(paint), changed, {});
        }
    }
    std::string kept = EARLIER;
    check(!remote::encode(remote::FrameRequest{1, true, remote::Paint(3)}, {}, kept) && kept == EARLIER, "a paint version 1 does not know is not encoded, the earlier value kept");
}

/// What a message leaves out has its default: encoding a non-default inactive field is refused, and decoding gives the default whatever the output held.
void inactive_fields() {
    context = "inactive fields";
    const auto refuses = [](const std::string& what, const auto& value) {
        std::string kept = EARLIER;
        check(!remote::encode(value, {}, kept) && kept == EARLIER, what + ": not encoded, the earlier value kept");
    };
    remote::Frame with_list;
    with_list.list.resize(1);
    refuses("an unpainted frame with a list", with_list);
    remote::Frame with_damage;
    with_damage.damage.emplace();
    refuses("an unpainted frame with damage", with_damage);
    refuses("a canvas font request with features", remote::FontRequest{true, "serif", 10, 400, false, "tnum=1"});
    refuses("a failed pixels answer with pixels", remote::PixelsAnswer{false, {1}});
    remote::Frame frame = sentinel<remote::Frame>();
    check(decoded(encoded(remote::Frame{4, {}, false, {}, std::nullopt}), {}, frame) && frame.list.empty() && !frame.damage, "an unpainted frame decodes with no list and no damage");
    remote::FontRequest font = sentinel<remote::FontRequest>();
    check(decoded(encoded(remote::FontRequest{true, "serif", 10, 400, false, ""}), {}, font) && font.canvas && font.features.empty(), "a canvas font request decodes with no features");
    remote::PixelsAnswer pixels = sentinel<remote::PixelsAnswer>();
    check(decoded(encoded(remote::PixelsAnswer{}), {}, pixels, {0, 0, 0, 3, 1}) && !pixels.ok && pixels.rgba.empty(), "a failed pixels answer decodes with no pixels");
    remote::Pointer pointer = sentinel<remote::Pointer>();
    check(decoded(encoded(remote::Pointer{"auto", std::nullopt}), {}, pointer) && !pointer.claim && pointer.cursor == "auto", "an unclaimed pointer decodes with no claim");
}

// ───────────────────────── Counts and lengths ─────────────────────────

template <class T> void accepted(const std::string& what, const std::string& bytes, const remote::Caps& caps, const remote::Caps* below) {
    T out;
    const bool ok = decoded(bytes, caps, out);
    check(ok, what + ": decodes");
    if (!ok) return;
    std::string again = EARLIER;
    check(remote::encode(out, caps, again) && again == EARLIER + bytes, what + ": encodes back to the same bytes after an earlier value");
    if (below) {
        std::string kept = EARLIER;
        check(!remote::encode(out, *below, kept) && kept == EARLIER, what + ": encoding it one cap lower is refused, the earlier value kept");
    }
}

void accepted_as(Kind kind, const std::string& what, const std::string& bytes, const remote::Caps& caps, const remote::Caps* below = nullptr) {
    switch (kind) {
    case Kind::Messages: return accepted<remote::Messages>(what, bytes, caps, below);
    case Kind::Pointer: return accepted<remote::Pointer>(what, bytes, caps, below);
    case Kind::Selection: return accepted<remote::Selection>(what, bytes, caps, below);
    case Kind::FontRequest: return accepted<remote::FontRequest>(what, bytes, caps, below);
    case Kind::WidthRequest: return accepted<remote::WidthRequest>(what, bytes, caps, below);
    case Kind::WidthsRequest: return accepted<remote::WidthsRequest>(what, bytes, caps, below);
    case Kind::ImageRequest: return accepted<remote::ImageRequest>(what, bytes, caps, below);
    case Kind::FontAnswer: return accepted<remote::FontAnswer>(what, bytes, caps, below);
    case Kind::Frame: return accepted<remote::Frame>(what, bytes, caps, below);
    case Kind::CanvasRun: return accepted<remote::CanvasRun>(what, bytes, caps, below);
    default: check(false, what + ": a kind the count checks take");
    }
}

void refused_as(Kind kind, const std::string& what, const std::string& bytes, const remote::Caps& caps) {
    switch (kind) {
    case Kind::Messages: return refused<remote::Messages>(what, bytes, caps);
    case Kind::Pointer: return refused<remote::Pointer>(what, bytes, caps);
    case Kind::Selection: return refused<remote::Selection>(what, bytes, caps);
    case Kind::FontRequest: return refused<remote::FontRequest>(what, bytes, caps);
    case Kind::WidthRequest: return refused<remote::WidthRequest>(what, bytes, caps);
    case Kind::WidthsRequest: return refused<remote::WidthsRequest>(what, bytes, caps);
    case Kind::ImageRequest: return refused<remote::ImageRequest>(what, bytes, caps);
    case Kind::FontAnswer: return refused<remote::FontAnswer>(what, bytes, caps);
    case Kind::Frame: return refused<remote::Frame>(what, bytes, caps);
    case Kind::CanvasRun: return refused<remote::CanvasRun>(what, bytes, caps);
    default: check(false, what + ": a kind the count checks take");
    }
}

using Cap = size_t remote::Caps::*;

/// A count or a length in a complete message: the cap that bounds it, the message's bytes before it and after its items, and its i-th item.
struct Site {
    std::string name;
    Kind kind;
    Cap cap;
    size_t least;
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

void byte_item(Bytes& b, size_t i) {
    b.u8(uint32_t('a' + i % 26));
}

std::vector<Site> sites() {
    const std::string font_tail = Bytes().f32(12).i32(400).u8(0).s;
    return {
        {"a Messages report's count", Kind::Messages, &remote::Caps::messages, 4, Bytes().head(Kind::Messages).s, "", [](Bytes& b, size_t) { b.u32(0); }},
        {"a Messages report's text", Kind::Messages, &remote::Caps::text, 1, Bytes().head(Kind::Messages).u32(2).str("").s, "", byte_item},
        {"a Pointer report's cursor", Kind::Pointer, &remote::Caps::text, 1, Bytes().head(Kind::Pointer).s, Bytes().u8(1).u8(1).u8(0).u8(1).s, byte_item},
        {"a Selection report's text", Kind::Selection, &remote::Caps::text, 1, Bytes().head(Kind::Selection).s, "", byte_item},
        {"a FontRequest's families", Kind::FontRequest, &remote::Caps::text, 1, Bytes().head(Kind::FontRequest).u8(0).s, font_tail + Bytes().str("").s, byte_item},
        {"a canvas FontRequest's families", Kind::FontRequest, &remote::Caps::text, 1, Bytes().head(Kind::FontRequest).u8(1).s, font_tail, byte_item},
        {"a FontRequest's features", Kind::FontRequest, &remote::Caps::text, 1, Bytes().head(Kind::FontRequest).u8(0).str("").s + font_tail, "", byte_item},
        {"a WidthRequest's text", Kind::WidthRequest, &remote::Caps::text, 1, Bytes().head(Kind::WidthRequest).i32(3).s, "", byte_item},
        {"an ImageRequest's url", Kind::ImageRequest, &remote::Caps::url, 1, Bytes().head(Kind::ImageRequest).i32(5).s, "", byte_item},
        {"a WidthsRequest's count", Kind::WidthsRequest, &remote::Caps::widths, 8, Bytes().head(Kind::WidthsRequest).s, "", [](Bytes& b, size_t i) { b.i32(int32_t(i)).u32(0); }},
        {"a WidthsRequest's text", Kind::WidthsRequest, &remote::Caps::text, 1, Bytes().head(Kind::WidthsRequest).u32(2).i32(1).str("").i32(2).s, "", byte_item},
        {"a WidthsRequest's texts together", Kind::WidthsRequest, &remote::Caps::widths_text, 1, Bytes().head(Kind::WidthsRequest).u32(2).i32(1).str("").i32(2).s, "",
            byte_item},
    };
}

/// Each count and length with only its own cap restrictive: complete at its cap and refused one past it, at caps 0, 1 and 3, both ways; refused cut inside its items or declaring one more; refused before making room when declaring more than the bytes left hold, under a cap of SIZE_MAX too; taken under a cap past UINT32_MAX.
void count_sites() {
    for (const Site& s : sites()) {
        context = "the count of " + s.name;
        for (size_t cap : {size_t(0), size_t(1), size_t(3)}) {
            remote::Caps caps, below;
            caps.*s.cap = cap;
            below.*s.cap = cap ? cap - 1 : 0;
            accepted_as(s.kind, s.name + " at a cap of " + std::to_string(cap), s.bytes(uint32_t(cap), cap), caps, cap ? &below : nullptr);
            refused_as(s.kind, s.name + " one past a cap of " + std::to_string(cap), s.bytes(uint32_t(cap + 1), cap + 1), caps);
        }
        refused_as(s.kind, s.name + " cut inside its last item", s.bytes(3, 3, true), {});
        refused_as(s.kind, s.name + " declaring one item more than it has", s.bytes(3, 2), {});
        accepted_as(s.kind, s.name + " holding one item", s.bytes(1, 1), {});
        const size_t one = last_decode.scope.asked, left = s.bytes(1, 1).size() - s.before.size() - 4;
        const auto refused_at_count = [&](const std::string& what, const std::string& bytes, const remote::Caps& caps) {
            refused_as(s.kind, what, bytes, caps);
            check(last_decode.scope.asked <= one, what + ": refused before making room, asking for " + std::to_string(last_decode.scope.asked) + " bytes where one item asks for " + std::to_string(one));
        };
        refused_at_count(s.name + " declaring one item more than the bytes left could hold", s.bytes(uint32_t(left / s.least + 1), 1), {});
        remote::Caps open;
        open.*s.cap = SIZE_MAX;
        accepted_as(s.kind, s.name + " under a cap of SIZE_MAX", s.bytes(3, 3), open);
        for (uint32_t n : VAST) refused_at_count(s.name + " declaring " + hex(n) + " under a cap of SIZE_MAX", s.bytes(n, 1), open);
        remote::Caps widest;
        widest.*s.cap = UINT32_MAX;
        accepted_as(s.kind, s.name + " under a cap of UINT32_MAX", s.bytes(1, 1), widest);
        if constexpr (SIZE_MAX > UINT32_MAX) {
            remote::Caps wide;
            wide.*s.cap = size_t(UINT32_MAX) + 1;
            accepted_as(s.kind, s.name + " under a cap of 2^32", s.bytes(1, 1), wide);
        }
    }
}

/// A length-delimited wire value in a complete message: the bytes before its length, the value, the bytes after it, and a whole value of another form.
struct ValueSite {
    std::string name;
    Kind kind;
    std::string before, value, after, other;

    std::string bytes(uint32_t declared, std::string_view inside) const { return Bytes{before}.u32(declared).raw(inside).raw(after).s; }
};

std::vector<ValueSite> value_sites() {
    std::string metrics, list, damage, batch, empty_list;
    wire::encode(ordinary_metrics(), metrics);
    check(wire::encode(compact_list(), {}, list) && wire::encode(wire::Damage(std::vector<Rect>{{1, 2, 3, 4}}), {}, damage) && wire::encode(compact_batch(), {}, batch)
            && wire::encode(DisplayList(), {}, empty_list),
        "the values the length checks carry encode");
    const std::string frame = Bytes().head(Kind::Frame).u64(1).state(ordinary_state()).u8(1).s;
    return {
        {"a FontAnswer's metrics", Kind::FontAnswer, Bytes().head(Kind::FontAnswer).i32(5).s, metrics, "", damage},
        {"a Frame's list", Kind::Frame, frame, list, Bytes().value(damage).s, damage},
        {"a Frame's damage", Kind::Frame, frame + Bytes().value(list).s, damage, "", empty_list},
        {"a CanvasRun's batch", Kind::CanvasRun, Bytes().head(Kind::CanvasRun).i32(1).i32(2).i32(3).s, batch, "", list},
    };
}

/// Each wire value taken whole at its length: refused one byte short or long, empty, past the bytes left, of another form or version, or with a byte after it.
void value_lengths() {
    for (const ValueSite& s : value_sites()) {
        context = "the length of " + s.name;
        const uint32_t n = uint32_t(s.value.size());
        accepted_as(s.kind, s.name + " at its length", s.bytes(n, s.value), {});
        const size_t whole = last_decode.scope.asked;
        refused_as(s.kind, s.name + " a byte shorter", s.bytes(n - 1, s.value), {});
        refused_as(s.kind, s.name + " a byte longer", s.bytes(n + 1, s.value), {});
        refused_as(s.kind, s.name + " a byte shorter, cut to it", s.bytes(n - 1, std::string_view(s.value).substr(0, n - 1)), {});
        refused_as(s.kind, s.name + " with a byte after it inside its length", s.bytes(n + 1, s.value + '\0'), {});
        refused_as(s.kind, s.name + " empty", s.bytes(0, ""), {});
        refused_as(s.kind, s.name + " of another form", s.bytes(uint32_t(s.other.size()), s.other), {});
        std::string version = s.value;
        version[1] = 2;
        refused_as(s.kind, s.name + " of version 2", s.bytes(n, version), {});
        std::string form = s.value;
        form[0] = char(0x7f);
        refused_as(s.kind, s.name + " of no form", s.bytes(n, form), {});
        for (uint32_t vast : VAST) {
            refused_as(s.kind, s.name + " declaring " + hex(vast), s.bytes(vast, s.value), {});
            check(last_decode.scope.asked <= whole, s.name + " declaring " + hex(vast) + ": refused asking for no more than the whole message does");
        }
    }
}

/// A cap of the carried values bounds them inside a message both ways: one less and the message is refused, the earlier value kept.
template <class T> void nested(const std::string& what, const T& value, size_t wire::Caps::*cap, size_t at) {
    const std::string bytes = encoded(value);
    remote::Caps fits, short_by_one;
    fits.values.*cap = at;
    short_by_one.values.*cap = at - 1;
    T out;
    std::string why;
    check(decoded(bytes, fits, out) && equal(value, out, why), what + ": taken at its cap " + why);
    refused<T>(what + " one past its cap", bytes, short_by_one);
    std::string kept = EARLIER;
    check(!remote::encode(value, short_by_one, kept) && kept == EARLIER, what + ": encoding it one past its cap is refused, the earlier value kept");
}

void nested_caps() {
    context = "nested caps";
    const remote::Frame frame{1, {}, true, compact_list(), std::vector<Rect>{{1, 2, 3, 4}, {5, 6, 7, 8}}};
    nested("a frame's commands", frame, &wire::Caps::commands, 5);
    nested("a frame's damage rects", frame, &wire::Caps::rects, 2);
    nested("a frame's command text", frame, &wire::Caps::text, 5);
    nested("a frame's path ops", frame, &wire::Caps::path_ops, 3);
    const remote::CanvasRun run{1, 2, 3, compact_batch()};
    nested("a canvas run's ops", run, &wire::Caps::canvas_ops, 5);
    nested("a canvas run's pixels", run, &wire::Caps::pixels, 16);
    nested("a canvas run's clips", run, &wire::Caps::clips, 1);
    remote::Frame invalid = frame;
    invalid.list[1].kind = Cmd(18);
    std::string kept = EARLIER;
    check(!remote::encode(invalid, {}, kept) && kept == EARLIER, "a frame whose list wire refuses is not encoded, the earlier value kept");
}

// ───────────────────────── Pixels ─────────────────────────

std::string pixels_answer(uint32_t declared, size_t bytes) {
    Bytes b;
    b.head(Kind::PixelsAnswer).u8(1).u32(declared);
    for (size_t i = 0; i < bytes; i++) b.u8(uint32_t(i));
    return b.s;
}

/// A pixels answer holds exactly the w × h × 4 bytes of the request it answers, within the pixel cap; a request asks for no more than a canvas holds.
void pixels() {
    context = "pixels";
    const remote::PixelsRequest six{1, 0, 0, 2, 3};
    const std::string failed = Bytes().head(Kind::PixelsAnswer).u8(0).s;
    remote::PixelsAnswer out;
    check(decoded(pixels_answer(24, 24), {}, out, six) && out.ok && out.rgba.size() == 24, "a 2 × 3 answer of 24 bytes is taken");
    const size_t one = last_decode.scope.asked;
    for (uint32_t n : {0u, 4u, 20u, 23u, 25u, 28u, 48u}) refused<remote::PixelsAnswer>("a 2 × 3 answer of " + std::to_string(n) + " bytes", pixels_answer(n, n), {}, six);
    refused<remote::PixelsAnswer>("a 2 × 3 answer declaring 24 bytes with 23", pixels_answer(24, 23), {}, six);
    refused<remote::PixelsAnswer>("a 2 × 3 answer declaring 24 bytes with 25", pixels_answer(24, 25), {}, six);
    for (uint32_t n : VAST) {
        refused<remote::PixelsAnswer>("a 2 × 3 answer declaring " + hex(n), pixels_answer(n, 24), {}, six);
        check(last_decode.scope.asked <= one, "a 2 × 3 answer declaring " + hex(n) + ": refused before making room");
    }
    refused<remote::PixelsAnswer>("a failed answer with a count after it", failed + Bytes().u32(0).s, {}, six);
    refused<remote::PixelsAnswer>("an answer without its count", Bytes().head(Kind::PixelsAnswer).u8(1).s, {}, six);
    remote::Caps pixel_cap;
    pixel_cap.values.pixels = 24;
    check(decoded(pixels_answer(24, 24), pixel_cap, out, six), "a 2 × 3 answer is taken under a pixel cap of 24");
    pixel_cap.values.pixels = 23;
    refused<remote::PixelsAnswer>("a 2 × 3 answer under a pixel cap of 23", pixels_answer(24, 24), pixel_cap, six);
    std::string kept = EARLIER;
    check(!remote::encode(out, pixel_cap, kept) && kept == EARLIER, "an answer past the pixel cap is not encoded, the earlier value kept");
    remote::Caps message_cap;
    message_cap.message = 3 + 4 + 24;
    check(decoded(pixels_answer(24, 24), message_cap, out, six) && remote::encode(out, message_cap, kept), "a 2 × 3 answer takes 31 bytes");
    message_cap.message = 30;
    refused<remote::PixelsAnswer>("a 2 × 3 answer under a message cap of 30", pixels_answer(24, 24), message_cap, six);
    const remote::PixelsRequest empty{1, 0, 0, 0, 3};
    check(decoded(pixels_answer(0, 0), {}, out, empty) && out.ok && out.rgba.empty(), "an answer of no pixels is taken for a request of none");
    refused<remote::PixelsAnswer>("an answer of 4 bytes for a request of none", pixels_answer(4, 4), {}, empty);
    const remote::PixelsRequest bad[] = {{0, 0, 0, -1, 1}, {0, 0, 0, 1, -1}, {0, 0, 0, 8193, 1}, {0, 0, 0, 1, 8193}, {0, 0, 0, 8192, 2049}, {0, 0, 0, 4097, 4096},
        {0, 0, 0, INT32_MIN, INT32_MIN}, {0, 0, 0, INT32_MAX, 0}};
    for (const remote::PixelsRequest& asked : bad) {
        const std::string dims = std::to_string(asked.w) + " × " + std::to_string(asked.h);
        refused<remote::PixelsAnswer>("a failed answer to a request of " + dims, failed, {}, asked);
        refused<remote::PixelsAnswer>("an empty answer to a request of " + dims, pixels_answer(0, 0), {}, asked);
        refused<remote::PixelsRequest>("a request of " + dims, Bytes().head(Kind::PixelsRequest).i32(1).i32(0).i32(0).i32(asked.w).i32(asked.h).s, {});
        std::string none = EARLIER;
        check(!remote::encode(asked, {}, none) && none == EARLIER, "a request of " + dims + " is not encoded, the earlier value kept");
    }
    const remote::PixelsRequest good[] = {{0, 0, 0, 8192, 2048}, {0, 0, 0, 4096, 4096}, {0, 0, 0, 8192, 0}, {0, 0, 0, 0, 8192}, {0, 0, 0, 0, 0}};
    for (const remote::PixelsRequest& asked : good) {
        const std::string dims = std::to_string(asked.w) + " × " + std::to_string(asked.h);
        check(decoded(failed, {}, out, asked) && !out.ok, "a failed answer to a request of " + dims + " is taken");
        remote::PixelsRequest request;
        check(decoded(encoded(asked), {}, request) && request.w == asked.w && request.h == asked.h, "a request of " + dims + " is taken");
    }
}

// ───────────────────────── Widths ─────────────────────────

std::string widths_answer(uint32_t declared, size_t widths) {
    Bytes b;
    b.head(Kind::WidthsAnswer).u32(declared);
    for (size_t i = 0; i < widths; i++) b.f32(float(i) + 0.5f);
    return b.s;
}

/// `n` texts of `bytes` bytes each, in three fonts.
remote::WidthsRequest texts_of(size_t n, size_t bytes = 1) {
    remote::WidthsRequest r;
    for (size_t i = 0; i < n; i++) r.texts.push_back({int(i % 3) + 1, std::string(bytes, char('a' + i % 26))});
    return r;
}

/// A widths answer holds exactly one width for each text of the request it answers, which must itself be within the caps; a request's texts are bounded by count, one by one and together, both ways.
void widths() {
    context = "widths";
    const remote::WidthsRequest three = texts_of(3);
    remote::WidthsAnswer out;
    check(decoded(widths_answer(3, 3), {}, out, three) && out.widths.size() == 3 && out.widths[0] == 0.5f && out.widths[2] == 2.5f, "an answer of 3 widths to 3 texts is taken in order");
    const size_t one = last_decode.scope.asked;
    for (uint32_t n : {0u, 1u, 2u, 4u, 6u}) refused<remote::WidthsAnswer>("an answer of " + std::to_string(n) + " widths to 3 texts", widths_answer(n, n), {}, three);
    refused<remote::WidthsAnswer>("an answer declaring 3 widths with 2", widths_answer(3, 2), {}, three);
    refused<remote::WidthsAnswer>("an answer declaring 3 widths with 4", widths_answer(3, 4), {}, three);
    refused<remote::WidthsAnswer>("an answer declaring 3 widths with 2 and a byte", widths_answer(3, 2) + '\0', {}, three);
    for (uint32_t n : VAST) {
        refused<remote::WidthsAnswer>("an answer declaring " + hex(n) + " widths", widths_answer(n, 3), {}, three);
        check(last_decode.scope.asked <= one, "an answer declaring " + hex(n) + " widths: refused before making room");
    }
    check(decoded(widths_answer(0, 0), {}, out, {}) && out.widths.empty(), "an answer of no widths to no texts is taken");
    refused<remote::WidthsAnswer>("an answer of one width to no texts", widths_answer(1, 1), {}, {});

    remote::Caps caps;
    caps.widths = 3;
    check(decoded(widths_answer(3, 3), caps, out, three), "an answer of 3 widths is taken under a widths cap of 3");
    caps.widths = 2;
    refused<remote::WidthsAnswer>("an answer of 3 widths under a widths cap of 2", widths_answer(3, 3), caps, three);
    std::string kept = EARLIER;
    check(!remote::encode(out, caps, kept) && kept == EARLIER, "an answer past the widths cap is not encoded, the earlier value kept");
    caps = {};
    caps.message = 2 + 4 + 3 * 4;
    check(decoded(widths_answer(3, 3), caps, out, three) && remote::encode(out, caps, kept), "an answer of 3 widths takes 18 bytes");
    caps.message = 17;
    refused<remote::WidthsAnswer>("an answer of 3 widths under a message cap of 17", widths_answer(3, 3), caps, three);
    kept = EARLIER;
    check(!remote::encode(out, caps, kept) && kept == EARLIER, "an answer past the message cap is not encoded, the earlier value kept");
    caps = {};
    caps.text = 0;
    refused<remote::WidthsAnswer>("an answer to a request whose texts pass the text cap", widths_answer(3, 3), caps, three);
    caps = {};
    caps.widths_text = 2;
    refused<remote::WidthsAnswer>("an answer to a request whose texts together pass their cap", widths_answer(3, 3), caps, three);
    caps.widths_text = 3;
    check(decoded(widths_answer(3, 3), caps, out, three), "an answer to a request whose texts together are at their cap is taken");
    caps = {};
    caps.widths = 2;
    refused<remote::WidthsAnswer>("an answer to a request past the widths cap", widths_answer(2, 2), caps, three);

    const remote::WidthsRequest pair{{{1, "abc"}, {2, "de"}}}, over{{{1, "abc"}, {2, "def"}}};
    remote::Caps total;
    total.widths_text = 5;
    remote::WidthsRequest request;
    check(decoded(encoded(pair, total), total, request) && request.texts.size() == 2 && request.texts[1].text == "de", "two texts of 5 bytes together are taken under a cap of 5");
    remote::Caps open = total;
    open.widths_text = 6;
    refused<remote::WidthsRequest>("two texts of 6 bytes together under a cap of 5", encoded(over, open), total);
    kept = EARLIER;
    check(!remote::encode(over, total, kept) && kept == EARLIER, "two texts of 6 bytes together are not encoded under a cap of 5, the earlier value kept");
    remote::Caps none;
    none.widths = 0;
    check(decoded(encoded(remote::WidthsRequest{}, none), none, request) && request.texts.empty(), "an empty request is taken under a widths cap of 0");
    refused<remote::WidthsRequest>("a request of one text under a widths cap of 0", encoded(texts_of(1)), none);
    kept = EARLIER;
    check(!remote::encode(texts_of(1), none, kept) && kept == EARLIER, "a request of one text is not encoded under a widths cap of 0, the earlier value kept");
    none = {};
    none.widths_text = 0;
    check(decoded(encoded(texts_of(3, 0), none), none, request) && request.texts.size() == 3, "empty texts are taken under a texts cap of 0");
    refused<remote::WidthsRequest>("a request of a 1-byte text under a texts cap of 0", encoded(texts_of(1)), none);
    caps = {};
    caps.widths = 1024;
    caps.widths_text = 64 << 10;
    check(decoded(encoded(texts_of(1024, 64), caps), caps, request) && request.texts.size() == 1024, "1,024 texts of 64 KiB together are taken under the embedding's caps");
    remote::Caps wider = caps;
    wider.widths = 1025;
    refused<remote::WidthsRequest>("1,025 texts under a widths cap of 1,024", encoded(texts_of(1025, 1), wider), caps);
    wider = caps;
    wider.widths_text = (64 << 10) + 1;
    remote::WidthsRequest heavy = texts_of(1024, 64);
    heavy.texts.back().text += 'x';
    refused<remote::WidthsRequest>("1,024 texts of 64 KiB and a byte under a texts cap of 64 KiB", encoded(heavy, wider), caps);
}

/// The texts one request takes are exactly those encode() takes from there, in order, under every cap, and the answer to them encodes as well.
void widths_in_requests() {
    context = "widths in a request";
    remote::WidthsRequest texts;
    for (const char* t : {"a", "bb", "", "dddd", "eeeeeeee", "f", "gg", "hhhhhhhhhhhhhhhh", "i"}) texts.texts.push_back({int(texts.texts.size() % 3) + 1, t});
    std::vector<std::pair<std::string, remote::Caps>> cases{{"the default caps", {}}, {"tight caps", tight()}};
    const auto with = [&](const std::string& name, auto set) {
        remote::Caps caps;
        set(caps);
        cases.push_back({name, caps});
    };
    with("a widths cap of 0", [](remote::Caps& c) { c.widths = 0; });
    with("a widths cap of 4", [](remote::Caps& c) { c.widths = 4; });
    with("a text cap of 4", [](remote::Caps& c) { c.text = 4; });
    with("texts of 7 bytes together", [](remote::Caps& c) { c.widths_text = 7; });
    with("texts of 15 bytes together", [](remote::Caps& c) { c.widths_text = 15; });
    for (size_t message = 0; message <= 80; message++)
        with("a message cap of " + std::to_string(message), [&](remote::Caps& c) { c.message = message; });
    with("caps at SIZE_MAX", [](remote::Caps& c) { c.widths = c.text = c.widths_text = c.message = SIZE_MAX; });
    size_t prefixes = 0;
    for (const auto& [name, caps] : cases) {
        for (size_t from = 0; from <= texts.texts.size(); from++) {
            const size_t n = remote::widths_in_request(texts.texts, from, caps);
            for (size_t k = 0; from + k <= texts.texts.size(); k++) {
                const remote::WidthsRequest request{std::vector<TextInFont>(texts.texts.begin() + long(from), texts.texts.begin() + long(from + k))};
                std::string bytes;
                const bool taken = remote::encode(request, caps, bytes);
                std::string answer;
                const bool answered = remote::encode(remote::WidthsAnswer{std::vector<float>(k, 1.5f)}, caps, answer);
                prefixes++;
                verify(k == 0 || taken == (k <= n), [&] {
                    return name + ": from text " + std::to_string(from) + ", " + std::to_string(n) + " in a request, but encode " + (taken ? "takes " : "refuses ") + std::to_string(k);
                });
                verify(!taken || answered, [&] { return name + ": the answer to " + std::to_string(k) + " texts from " + std::to_string(from) + " encodes too"; });
            }
        }
    }
    check(prefixes > 1000, "every prefix of nine texts is checked under " + std::to_string(cases.size()) + " caps (" + std::to_string(prefixes) + ")");
    check(remote::widths_in_request(texts.texts, texts.texts.size() + 1, {}) == 0, "nothing past the last text");
}

/// An encoder checks a counted field's room before it appends: past the message cap, it asks the allocator for nothing.
void encoder_room() {
    context = "encoder room";
    remote::Caps caps;
    caps.message = 1000;
    const auto before_appending = [&](const std::string& what, const auto& value) {
        std::string out = EARLIER;
        out.reserve(EARLIER.size() + 64);
        const Measured m = measure(0, [&] { return remote::encode(value, caps, out); });
        check(!m.ok && !m.threw && m.scope.asked == 0 && out == EARLIER,
            what + ": refused past the message cap before it is appended, asking for " + std::to_string(m.scope.asked) + " bytes");
    };
    const std::string big(size_t(1) << 20, 'x');
    before_appending("a selection of 1 MiB", remote::Selection{big});
    before_appending("an image URL of 1 MiB", remote::ImageRequest{1, big});
    before_appending("a width request's text of 1 MiB", remote::WidthRequest{1, big});
    before_appending("a report of a 1 MiB text", remote::Messages{{"a", big}});
    before_appending("pixels of 1 MiB", remote::PixelsAnswer{true, std::vector<uint8_t>(big.size(), 1)});
    before_appending("a widths answer of 1 MiB", remote::WidthsAnswer{std::vector<float>(big.size() / 4, 1.f)});
    caps.widths = SIZE_MAX;
    caps.widths_text = SIZE_MAX;
    before_appending("a widths answer of 1 MiB under widths caps of SIZE_MAX", remote::WidthsAnswer{std::vector<float>(big.size() / 4, 1.f)});
    before_appending("a widths request's text of 1 MiB under widths caps of SIZE_MAX", remote::WidthsRequest{{{1, big}}});
}

// ───────────────────────── A platform end's session ─────────────────────────

struct Capture : remote::Channel {
    std::vector<std::string> sent;

    void send(std::string message) override { sent.push_back(std::move(message)); }
};

/// The host's callbacks in order, each with the number of Backend calls before it and the end's state in it.
struct Recorder : remote::PlatformEnd::Host {
    struct Event {
        std::string what;
        size_t calls = 0;
        remote::State state;
    };

    explicit Recorder(LogBackend& backend) : backend(backend) {}

    void note(const char* what);
    void frame(remote::Frame&& frame) override;
    void state(const remote::State& state) override;
    void messages(std::vector<twidget::Json>&& messages) override;
    void pointer(const remote::Pointer& pointer) override;
    void selection(const remote::Selection& selection) override;
    void claim(const remote::ClaimAnswer& answer) override;
    void refused(const std::string& reason) override;

    LogBackend& backend;
    remote::PlatformEnd* end = nullptr;
    std::vector<Event> events;
    std::vector<remote::Frame> frames;
    std::vector<remote::State> states;
    std::vector<std::vector<twidget::Json>> batches;
    std::vector<remote::Pointer> pointers;
    std::vector<remote::Selection> selections;
    std::vector<remote::ClaimAnswer> claims;
    std::vector<std::string> refusals;
    bool ask_on_frame = false;
    remote::Paint ask_paint = remote::Paint::IfDirty;
    std::vector<uint64_t> asked;
};

struct Session {
    explicit Session(const remote::Caps& caps = {}) : end(backend, host, channel, caps) { host.end = &end; }

    LogBackend backend;
    Recorder host{backend};
    Capture channel;
    remote::PlatformEnd end;
};

void Recorder::note(const char* what) {
    events.push_back({what, backend.calls.size(), end->state()});
}

void Recorder::frame(remote::Frame&& frame) {
    note("frame");
    backend.frame(frame.painted);
    frames.push_back(std::move(frame));
    if (ask_on_frame) {
        ask_on_frame = false;
        asked.push_back(end->request_frame(false, ask_paint));
    }
}

void Recorder::state(const remote::State& state) {
    note("state");
    states.push_back(state);
}

void Recorder::messages(std::vector<twidget::Json>&& messages) {
    note("messages");
    batches.push_back(std::move(messages));
}

void Recorder::pointer(const remote::Pointer& pointer) {
    note("pointer");
    pointers.push_back(pointer);
}

void Recorder::selection(const remote::Selection& selection) {
    note("selection");
    selections.push_back(selection);
}

void Recorder::claim(const remote::ClaimAnswer& answer) {
    note("claim");
    claims.push_back(answer);
}

void Recorder::refused(const std::string& reason) {
    refusals.push_back(reason);
}

std::string callbacks(const Session& s) {
    std::string out;
    for (const Recorder::Event& e : s.host.events) out += (out.empty() ? "" : " ") + e.what;
    return out;
}

/// Whether the end's state was `expected` in the host's last callback.
bool heard_in(const Session& s, const remote::State& expected, std::string& why) {
    why = "with no callback";
    return !s.host.events.empty() && equal(expected, s.host.events.back().state, why);
}

template <class T> bool give(Session& s, const T& message) {
    return s.end.receive(encoded(message));
}

/// The end answers `request`, replacing what the reply held with exactly one answer, which `out` gets decoded against `pixels`.
template <class A, class R> bool answered(Session& s, const R& request, A& out, const AskedFor<A>& pixels = {}) {
    std::string reply = "a reply the answer replaces";
    if (!s.end.answer(encoded(request), reply)) return false;
    std::string again;
    return decode_as(reply, {}, pixels, out) && remote::encode(out, {}, again) && again == reply;
}

template <class T> void was_sent(const Session& s, size_t i, const T& expected, const std::string& what) {
    T out;
    std::string why;
    check(i < s.channel.sent.size() && remote::decode(s.channel.sent[i], {}, out) && equal(expected, out, why), what + ": goes out as it was given " + why);
}

/// Every input goes out as one message in order, frame requests and claim queries each numbered from 1, and reaches neither the Backend nor the host.
void session_inputs() {
    context = "a platform end's inputs";
    Session s;
    check(s.end.request_frame(true, remote::Paint::IfDirty) == 1 && s.end.request_frame(false, remote::Paint::Force) == 2, "frame requests are numbered from 1");
    s.end.set_width(320.5);
    s.end.set_max_height(1e6);
    s.end.set_theme(true);
    s.end.touch(2, 10.25, -3.5, 7, true, true, 0x0f, 2);
    s.end.touch(3, 1.5, 2.5, 8, false);
    s.end.hover(1, 5.5, 6.5, 3);
    s.end.hover(4, -1, -2);
    s.end.clear_selection();
    check(s.end.ask_claim(100.5, 200.25) == 1 && s.end.ask_claim(-1, 0.5) == 2, "claim queries are numbered from 1");
    check(s.channel.sent.size() == 12, "one message for each input");
    was_sent(s, 0, remote::FrameRequest{1, true, remote::Paint::IfDirty}, "the first frame request");
    was_sent(s, 1, remote::FrameRequest{2, false, remote::Paint::Force}, "the second frame request");
    was_sent(s, 2, remote::SetWidth{320.5}, "a width");
    was_sent(s, 3, remote::SetMaxHeight{1e6}, "a maximum height");
    was_sent(s, 4, remote::SetTheme{true}, "a theme");
    was_sent(s, 5, remote::Touch{2, 10.25, -3.5, 7, true, true, 0x0f, 2}, "a mouse press");
    was_sent(s, 6, remote::Touch{3, 1.5, 2.5, 8, false, false, 0, 0}, "a touch with the defaults");
    was_sent(s, 7, remote::Hover{1, 5.5, 6.5, 3}, "a hover");
    was_sent(s, 8, remote::Hover{4, -1, -2, 0}, "a hover with the default modifiers");
    was_sent(s, 9, remote::ClearSelection{}, "clearing the selection");
    was_sent(s, 10, remote::ClaimQuery{1, 100.5, 200.25}, "the first claim query");
    was_sent(s, 11, remote::ClaimQuery{2, -1, 0.5}, "the second claim query");
    check(s.backend.calls.empty() && s.host.events.empty() && s.host.refusals.empty() && !s.end.refusing(), "inputs reach neither the Backend nor the host");
}

/// Each request reaches the Backend with its arguments and outputs as a renderer passes them in-process, and its answer carries what the Backend gave.
void session_requests() {
    context = "a platform end's requests";
    Session s;
    std::string why;
    remote::FontAnswer font;
    check(answered(s, remote::FontRequest{false, "\"Inter\", sans-serif", 13.5f, 600, true, "tnum=1,zero=1"}, font), "a document font is answered");
    remote::FontAnswer canvas;
    check(answered(s, remote::FontRequest{true, "serif", 10, 400, false, ""}, canvas), "a canvas font is answered");
    remote::WidthAnswer width;
    check(answered(s, remote::WidthRequest{font.font, "hello"}, width), "a width is answered");
    const std::string url = "data:image/png;base64,iVBORw0KGgo=";
    remote::ImageAnswer image;
    check(answered(s, remote::ImageRequest{5, url}, image), "an image is answered");
    s.backend.images_load = false;
    remote::ImageAnswer broken;
    check(answered(s, remote::ImageRequest{6, "data:image/png;base64,AAAA"}, broken), "an image the Backend cannot load is answered");
    const remote::PixelsRequest region{3, -1, 2, 2, 3};
    remote::PixelsAnswer pixels;
    check(answered(s, region, pixels, region), "pixels are answered");
    s.backend.reads = LogBackend::Read::Unsupported;
    remote::PixelsAnswer unsupported = sentinel<remote::PixelsAnswer>();
    check(answered(s, region, unsupported, region), "pixels the Backend cannot read are answered");
    s.backend.reads = LogBackend::Read::WrongSize;
    remote::PixelsAnswer wrong = sentinel<remote::PixelsAnswer>();
    check(answered(s, region, wrong, region), "pixels the Backend reads at the wrong size are answered");
    s.backend.reads = LogBackend::Read::Pixels;
    const remote::PixelsRequest empty{4, 0, 0, 0, 5};
    remote::PixelsAnswer none = sentinel<remote::PixelsAnswer>();
    check(answered(s, empty, none, empty), "no pixels are answered");
    const std::vector<Call>& calls = s.backend.calls;
    if (calls.size() != 9) {
        check(false, "each request makes one Backend call, made " + std::to_string(calls.size()));
        return;
    }
    const Call& document = calls[0];
    check(document.op == Call::Font && document.families == "\"Inter\", sans-serif" && document.features == "tnum=1,zero=1" && bits(document.size) == bits(13.5f)
            && document.weight == 600 && document.italic,
        "a document font is made with the request's arguments");
    check(same(document.metrics_in, FontMetrics(), why), "a document font's metrics start at their defaults " + why);
    check(font.font == document.font && document.font > 0 && same(font.metrics, document.metrics, why) && !same(document.metrics, FontMetrics(), why),
        "a document font's answer is the Backend's handle and metrics");
    const Call& marked = calls[1];
    check(marked.op == Call::CanvasFont && marked.families == "serif" && marked.features.empty() && bits(marked.size) == bits(10.f) && marked.weight == 400 && !marked.italic,
        "a canvas font is made as a canvas font, with the request's arguments");
    check(same(marked.metrics_in, FontMetrics(), why) && canvas.font == marked.font && marked.font != document.font && same(canvas.metrics, marked.metrics, why),
        "a canvas font's answer is the Backend's " + why);
    const Call& measured = calls[2];
    check(measured.op == Call::TextWidth && measured.font == font.font && measured.text == "hello" && bits(width.width) == bits(measured.width) && measured.width > 0,
        "a width is measured in the font asked for, and answered");
    const Call& loaded = calls[3];
    check(loaded.op == Call::Image && loaded.id == 5 && loaded.url == url && bits(loaded.w_in) == 0 && bits(loaded.h_in) == 0, "an image is loaded with its id and URL, its size starting at 0");
    check(image.ok && bits(image.w) == bits(loaded.image_w) && bits(image.h) == bits(loaded.image_h) && image.w > 0, "an image's answer is the Backend's size");
    const Call& failed = calls[4];
    check(failed.op == Call::Image && !failed.ok && bits(failed.w_in) == 0 && bits(failed.h_in) == 0, "an image the Backend cannot load is asked for with its size at 0");
    check(!broken.ok && bits(broken.w) == bits(failed.image_w) && bits(broken.h) == bits(failed.image_h) && broken.w > 0, "a failed load's answer still carries the size the Backend gave");
    const Call& read = calls[5];
    check(read.op == Call::CanvasRead && read.id == 3 && read.x == -1 && read.y == 2 && read.w == 2 && read.h == 3 && read.rgba_in == 0,
        "pixels are read with the request's region into an empty buffer");
    check(pixels.ok && pixels.rgba.size() == 24 && pixels.rgba == read.rgba, "the pixels' answer is what the Backend read");
    check(calls[6].op == Call::CanvasRead && !calls[6].ok && !unsupported.ok && unsupported.rgba.empty(), "pixels the Backend cannot read are answered as unsupported");
    check(calls[7].op == Call::CanvasRead && calls[7].rgba.size() == 28 && !wrong.ok && wrong.rgba.empty(), "pixels read at the wrong size are answered as unsupported");
    check(calls[8].op == Call::CanvasRead && calls[8].w == 0 && calls[8].h == 5 && none.ok && none.rgba.empty(), "a read of no pixels asks the Backend and is answered with none");
    check(s.host.events.empty() && !s.end.refusing(), "requests reach no host callback");
}

/// Measures in one call what text_width measures one by one, and counts its calls.
struct BulkBackend : LogBackend {
    std::vector<float> text_widths(const std::vector<TextInFont>& texts) override {
        bulk.push_back(texts.size());
        return LogBackend::text_widths(texts);
    }

    std::vector<size_t> bulk;
};

/// Gives one width too few or too many.
struct MiscountingBackend : LogBackend {
    std::vector<float> text_widths(const std::vector<TextInFont>& texts) override {
        std::vector<float> widths = LogBackend::text_widths(texts);
        if (fewer && !widths.empty()) widths.pop_back();
        if (!fewer) widths.push_back(1);
        return widths;
    }

    bool fewer = true;
};

/// A WidthsRequest reaches the Backend's text_widths once with every text in order, which measures them as text_width does unless the Backend measures them itself, and its answer carries the widths in that order; an empty one asks nothing; a Backend that gives another number of widths stops the end, the reply untouched.
void session_widths() {
    context = "a platform end's widths";
    {
        Session s;
        remote::FontAnswer font, bold;
        check(answered(s, remote::FontRequest{false, "serif", 12, 400, false, ""}, font) && answered(s, remote::FontRequest{false, "serif", 12, 700, false, ""}, bold),
            "two fonts are answered");
        const remote::WidthsRequest request{{{font.font, "hello"}, {bold.font, "world"}, {font.font, ""}, {font.font, "hello"}, {99, "no such font"}}};
        remote::WidthsAnswer widths = sentinel<remote::WidthsAnswer>();
        check(answered(s, request, widths, request), "a widths request is answered");
        const std::vector<Call>& calls = s.backend.calls;
        bool measured = calls.size() == 2 + request.texts.size() && widths.widths.size() == request.texts.size();
        for (size_t i = 0; measured && i < request.texts.size(); i++) {
            const Call& c = calls[2 + i];
            measured = c.op == Call::TextWidth && c.font == request.texts[i].font && c.text == request.texts[i].text && bits(c.width) == bits(widths.widths[i]);
        }
        check(measured, "each text is measured in its font, in order, and the answer holds the Backend's widths in that order");
        check(widths.widths[0] > 0 && widths.widths[2] == 0 && widths.widths[4] == 0, "the Backend's widths, an empty text and an unknown font giving 0 as text_width does");
        remote::WidthsAnswer none = sentinel<remote::WidthsAnswer>();
        check(answered(s, remote::WidthsRequest{}, none, {}) && none.widths.empty() && calls.size() == 2 + request.texts.size(), "an empty widths request is answered with none and no Backend call");
        check(s.host.events.empty() && !s.end.refusing(), "widths reach no host callback");
    }
    {
        BulkBackend backend;
        Recorder host(backend);
        Capture channel;
        remote::PlatformEnd end(backend, host, channel);
        host.end = &end;
        const remote::WidthsRequest request = texts_of(5);
        std::string reply;
        remote::WidthsAnswer widths;
        check(end.answer(encoded(request), reply) && remote::decode(reply, {}, request, widths) && widths.widths.size() == 5, "a Backend that measures many at once answers a widths request");
        check(backend.bulk == std::vector<size_t>{5} && backend.count(Call::TextWidth) == 5, "the platform end hands it the whole request in one call");
    }
    for (bool fewer : {true, false}) {
        const std::string what = std::string("a Backend that gives one width too ") + (fewer ? "few" : "many");
        MiscountingBackend backend;
        backend.fewer = fewer;
        Recorder host(backend);
        Capture channel;
        remote::PlatformEnd end(backend, host, channel);
        host.end = &end;
        std::string reply = "kept";
        check(!end.answer(encoded(texts_of(3)), reply) && reply == "kept" && end.refusing() && host.refusals.size() == 1 && !host.refusals[0].empty(),
            what + ": the end stops with a reason, the reply untouched");
        check(channel.sent.empty() && host.events.empty(), what + ": nothing is sent and the host hears nothing else");
    }
    {
        remote::Caps caps;
        caps.widths = 2;
        caps.widths_text = 5;
        Session s(caps);
        remote::WidthsAnswer widths;
        const remote::WidthsRequest fits{{{1, "abc"}, {1, "de"}}};
        check(answered(s, fits, widths, fits) && widths.widths.size() == 2 && s.backend.calls.size() == 2, "two texts of 5 bytes are answered under caps of 2 texts and 5 bytes");
        remote::Caps wide = caps;
        wide.widths = 3;
        wide.widths_text = 6;
        std::string reply = "kept";
        std::string three;
        check(remote::encode(remote::WidthsRequest{{{1, "a"}, {1, "b"}, {1, "c"}}}, wide, three) && !s.end.answer(three, reply) && reply == "kept" && s.end.refusing()
                && s.backend.calls.size() == 2,
            "three texts under a cap of two stop the end before the Backend measures any");
    }
}

/// Under `caps`, a request for `region`'s pixels is answered, read from the Backend when `read`, else unsupported without asking it.
void pixel_cap(const std::string& what, const remote::Caps& caps, const remote::PixelsRequest& region, bool read) {
    Session s(caps);
    remote::PixelsAnswer out;
    check(answered(s, region, out, region) && !s.end.refusing(), what + ": answered");
    check(out.ok == read && s.backend.calls.size() == (read ? 1u : 0u), what + (read ? ": read" : ": unsupported, the Backend not asked"));
}

/// A pixels answer fits the pixel cap and the message cap with its 7 bytes, or is unsupported before the Backend reads; a cap too small for a request or an answer stops the end, the reply untouched.
void session_caps() {
    context = "a platform end's caps";
    const remote::PixelsRequest six{1, 0, 0, 2, 3};
    remote::Caps caps;
    caps.values.pixels = 24;
    pixel_cap("24 bytes of pixels under a pixel cap of 24", caps, six, true);
    caps.values.pixels = 23;
    pixel_cap("24 bytes of pixels under a pixel cap of 23", caps, six, false);
    caps = {};
    caps.message = 31;
    pixel_cap("24 bytes of pixels under a message cap of 31", caps, six, true);
    caps.message = 30;
    pixel_cap("24 bytes of pixels under a message cap of 30", caps, six, false);
    caps.message = 22;
    pixel_cap("4 bytes of pixels under a message cap the request just fits", caps, {1, 0, 0, 1, 1}, true);
    pixel_cap("16 bytes of pixels under a message cap the request just fits", caps, {1, 0, 0, 2, 2}, false);
    pixel_cap("no pixels under a message cap the request just fits", caps, {1, 0, 0, 0, 0}, true);
    caps.message = 21;
    {
        Session s(caps);
        std::string reply = "kept";
        check(!s.end.answer(encoded(six), reply) && reply == "kept" && s.end.refusing() && s.host.refusals.size() == 1 && s.backend.calls.empty(),
            "a pixels request past the message cap stops the end before the Backend reads");
    }
    caps.message = 30;
    {
        Session s(caps);
        std::string reply = "kept";
        check(!s.end.answer(encoded(remote::FontRequest{true, "a", 1, 2, false, ""}), reply) && reply == "kept" && s.end.refusing() && s.host.refusals.size() == 1,
            "a font answer past the message cap stops the end, the reply untouched");
    }
    caps.message = 11;
    {
        Session s(caps);
        check(s.end.request_frame(true, remote::Paint::IfDirty) == 0 && s.end.refusing() && s.host.refusals.size() == 1 && s.channel.sent.empty(),
            "a frame request past the message cap stops the end, nothing sent");
        s.end.set_width(1);
        check(s.channel.sent.empty() && s.host.refusals.size() == 1, "and the end stays stopped");
    }
    caps.message = 0;
    {
        Session s(caps);
        s.end.clear_selection();
        check(s.end.refusing() && s.host.refusals.size() == 1 && s.channel.sent.empty(), "under a message cap of 0 the first input stops the end");
    }
}

/// The one-way Backend calls reach it at once and in order; reports reach the host, the state updated before it hears of it.
void session_reports() {
    context = "a platform end's reports";
    Session s;
    std::string why;
    const Batch ops = compact_batch();
    check(give(s, remote::CanvasRun{3, 300, 150, ops}) && give(s, remote::FontRelease{7}) && give(s, remote::CanvasRelease{3}), "the three one-way Backend calls are taken");
    std::vector<Call> expected(3);
    expected[0].op = Call::CanvasRun;
    expected[0].id = 3;
    expected[0].w = 300;
    expected[0].h = 150;
    expected[0].ops = ops;
    expected[1].op = Call::DeleteFont;
    expected[1].font = 7;
    expected[2].op = Call::CanvasRelease;
    expected[2].id = 3;
    check(same_calls(expected, s.backend.calls, why), "they reach the Backend in order, the batch member by member and shared alike " + why);
    check(s.host.events.empty(), "they reach no host callback");
    const std::vector<std::string> texts{R"({"type":"size","width":320,"height":96,"innerWidth":320,"innerHeight":48,"dpr":2})",
        R"({"type":"gesture","wants":false,"x":true,"y":false,"hold":true})", R"({"type":"limit","what":"ops"})", R"({"type":"violation","reason":"image refused: x"})",
        R"({"type":"bubble","takes":["menu",{"x":1}]})"};
    check(give(s, remote::Messages{texts}), "a report of the five platform messages is taken");
    bool same_texts = s.host.batches.size() == 1 && s.host.batches[0].size() == texts.size();
    for (size_t i = 0; same_texts && i < texts.size(); i++) same_texts = s.host.batches[0][i].dump() == texts[i];
    check(same_texts, "the host gets the five platform messages as they were written");
    check(give(s, remote::Messages{}) && s.host.batches.size() == 2 && s.host.batches[1].empty(), "an empty report reaches the host empty");
    check(give(s, remote::Selection{"selected\ntext"}) && give(s, remote::Selection{}) && s.host.selections.size() == 2 && s.host.selections[0].text == "selected\ntext"
            && s.host.selections[1].text.empty(),
        "selections reach the host, a cleared one too");
    remote::State state{320.5, 480, true, true, 0.016, false};
    put_bits(state.next_tick_in, SPECIAL_DOUBLES[3]);
    check(give(s, state) && s.host.states.size() == 1 && equal(state, s.host.states[0], why), "a state report reaches the host " + why);
    check(heard_in(s, state, why) && equal(state, s.end.state(), why), "the end's state is the report's when the host hears it " + why);
    check(callbacks(s) == "messages messages selection selection state" && s.backend.calls.size() == 3 && !s.end.refusing(), "the host hears each report once, in order: " + callbacks(s));
}

/// Pointer reports, claim answers and frames reach the host as the renderer end sent them, a frame's state the end's before the host hears of it.
void session_replies() {
    context = "a platform end's replies";
    Session s;
    std::string why;
    s.end.touch(2, 10, 20, 1, true, true);
    const remote::Pointer pressed{"pointer", twidget::WidgetGestureClaim{true, false, true}};
    check(give(s, pressed), "a press's pointer report is taken");
    s.end.hover(1, 30, 40);
    const remote::Pointer moved{"text", std::nullopt};
    check(give(s, moved), "a hover's pointer report is taken");
    check(s.host.pointers.size() == 2 && equal(pressed, s.host.pointers[0], why) && equal(moved, s.host.pointers[1], why), "pointer reports reach the host " + why);
    const uint64_t query = s.end.ask_claim(5, 6);
    const remote::ClaimAnswer hold{query, {false, false, true}};
    check(give(s, hold) && s.host.claims.size() == 1 && equal(hold, s.host.claims[0], why), "a claim answer reaches the host " + why);
    const uint64_t painted = s.end.request_frame(true, remote::Paint::IfDirty);
    const remote::Frame frame{painted, ordinary_state(), true, compact_list(), std::vector<Rect>{{1, 2, 3, 4}}};
    check(give(s, frame) && s.host.frames.size() == 1 && equal(frame, s.host.frames[0], why), "a painted frame reaches the host member by member, its payloads shared alike " + why);
    check(heard_in(s, frame.state, why) && equal(frame.state, s.end.state(), why), "the end's state is the frame's when the host hears of it " + why);
    const uint64_t unchanged = s.end.request_frame(false, remote::Paint::Skip);
    const remote::Frame still{unchanged, {100, 50, false, false, std::numeric_limits<double>::infinity(), true}, false, {}, std::nullopt};
    check(give(s, still) && s.host.frames.size() == 2 && equal(still, s.host.frames[1], why), "an unchanged frame reaches the host " + why);
    check(heard_in(s, still.state, why) && equal(still.state, s.end.state(), why), "and its state is the end's " + why);
    check(s.host.states.empty(), "a frame carries its state without a state callback of its own");
    check(callbacks(s) == "pointer pointer claim frame frame", "the host hears each reply once, in order: " + callbacks(s));
    check(s.backend.count(Call::Frame) == 2 && s.backend.calls.size() == 2 && !s.end.refusing(), "replies make no Backend call");
}

/// One-way Backend calls and frames keep the order they arrived in.
void session_order() {
    context = "a platform end's order";
    Session s;
    const uint64_t seq = s.end.request_frame(true, remote::Paint::IfDirty);
    check(give(s, remote::CanvasRun{1, 10, 10, compact_batch()}) && give(s, remote::Frame{seq, {}, true, compact_list(), std::nullopt}) && give(s, remote::FontRelease{4})
            && give(s, remote::State{}) && give(s, remote::CanvasRelease{1}),
        "a batch, a frame, a font release, a state and a canvas release are taken");
    std::string ops;
    for (const Call& c : s.backend.calls) ops += std::to_string(int(c.op)) + " ";
    check(s.backend.calls.size() == 4 && s.backend.calls[0].op == Call::CanvasRun && s.backend.calls[1].op == Call::Frame && s.backend.calls[2].op == Call::DeleteFont
            && s.backend.calls[3].op == Call::CanvasRelease,
        "the Backend sees the batch, the frame, the release and the canvas release in that order: " + ops);
    check(callbacks(s) == "frame state" && s.host.events[0].calls == 1 && s.host.events[1].calls == 3, "the host hears of the frame after the batch and of the state after the release");
}

using Setup = void (*)(remote::PlatformEnd&);

void no_setup(remote::PlatformEnd&) {}

std::string frame_bytes(uint64_t seq) {
    return encoded(remote::Frame{seq, {}, false, {}, std::nullopt});
}

std::string claim_bytes(uint64_t seq) {
    return encoded(remote::ClaimAnswer{seq, {}});
}

std::string pointer_bytes() {
    return encoded(remote::Pointer{"auto", std::nullopt});
}

std::string report_bytes(std::vector<std::string> texts) {
    return encoded(remote::Messages{std::move(texts)});
}

size_t refusal_cases = 0;

/// Every entry point of a refusing end does nothing: no message, no Backend call, no callback, false or 0 from each, its state kept; the host heard of the stop `heard` times.
void inert(Session& s, const std::string& what, size_t heard = 1) {
    const size_t sent = s.channel.sent.size(), calls = s.backend.calls.size(), events = s.host.events.size();
    const remote::State state = s.end.state();
    bool none = s.end.request_frame(true, remote::Paint::Force) == 0 && s.end.ask_claim(1, 2) == 0;
    s.end.set_width(1);
    s.end.set_max_height(2);
    s.end.set_theme(true);
    s.end.touch(1, 1, 1, 1, true);
    s.end.hover(1, 1, 1);
    s.end.clear_selection();
    none = none && !s.end.receive(encoded(remote::State{})) && !s.end.receive(encoded(remote::Selection{"x"})) && !s.end.receive(encoded(remote::FontRelease{1}));
    std::string reply = "kept";
    none = none && !s.end.answer(encoded(remote::FontRequest{}), reply) && reply == "kept";
    std::string why;
    check(none && s.channel.sent.size() == sent && s.backend.calls.size() == calls && s.host.events.size() == events && equal(state, s.end.state(), why),
        what + ": every later call does nothing " + why);
    check(s.end.refusing() && s.host.refusals.size() == heard, what + ": the host hears of it " + (heard ? "once" : "never"));
}

/// After `setup` on a fresh end, `bad` is refused with a reason, `reason` when one is given, reaching neither the Backend nor the host, and leaves the end inert; after the same setup on another, `good` is taken.
void refusal_case(const std::string& what, Setup setup, const std::string& bad, const std::string& good = {}, bool request = false, const std::string& reason = {}) {
    refusal_cases++;
    if (!good.empty()) {
        Session control;
        setup(control.end);
        std::string reply;
        const bool taken = request ? control.end.answer(good, reply) : control.end.receive(good);
        check(taken && !control.end.refusing() && control.host.refusals.empty(), what + ": the control is taken");
    }
    Session s;
    setup(s.end);
    check(!s.end.refusing(), what + ": the setup is taken");
    const size_t calls = s.backend.calls.size(), events = s.host.events.size();
    std::string reply = "kept";
    const bool taken = request ? s.end.answer(bad, reply) : s.end.receive(bad);
    const std::string heard = s.host.refusals.empty() ? std::string() : s.host.refusals[0];
    check(!taken && s.end.refusing() && s.host.refusals.size() == 1 && !heard.empty() && (reason.empty() || heard == reason),
        what + ": refused with " + (reason.empty() ? std::string("a reason") : "\"" + reason + "\", not \"" + heard + "\""));
    check(reply == "kept" && s.backend.calls.size() == calls && s.host.events.size() == events, what + ": nothing reaches the Backend or the host, the reply untouched");
    inert(s, what);
}

std::vector<std::pair<Kind, std::string>> every_kind() {
    return {{Kind::FrameRequest, encoded(remote::FrameRequest{})}, {Kind::SetWidth, encoded(remote::SetWidth{})}, {Kind::SetMaxHeight, encoded(remote::SetMaxHeight{})},
        {Kind::SetTheme, encoded(remote::SetTheme{})}, {Kind::Touch, encoded(remote::Touch{})}, {Kind::Hover, encoded(remote::Hover{})},
        {Kind::ClearSelection, encoded(remote::ClearSelection{})}, {Kind::ClaimQuery, encoded(remote::ClaimQuery{})}, {Kind::FontAnswer, encoded(remote::FontAnswer{})},
        {Kind::WidthAnswer, encoded(remote::WidthAnswer{})}, {Kind::ImageAnswer, encoded(remote::ImageAnswer{})}, {Kind::PixelsAnswer, encoded(remote::PixelsAnswer{})},
        {Kind::Frame, encoded(remote::Frame{})}, {Kind::State, encoded(remote::State{})}, {Kind::Messages, encoded(remote::Messages{})},
        {Kind::Pointer, encoded(remote::Pointer{})}, {Kind::Selection, encoded(remote::Selection{})}, {Kind::ClaimAnswer, encoded(remote::ClaimAnswer{})},
        {Kind::FontRelease, encoded(remote::FontRelease{})}, {Kind::CanvasRun, encoded(remote::CanvasRun{})}, {Kind::CanvasRelease, encoded(remote::CanvasRelease{})},
        {Kind::FrameDelta, encoded(remote::FrameDelta{0, {}, 0, {{0, 0, "x"}}, std::nullopt})},
        {Kind::FontRequest, encoded(remote::FontRequest{})}, {Kind::WidthRequest, encoded(remote::WidthRequest{})}, {Kind::ImageRequest, encoded(remote::ImageRequest{})},
        {Kind::PixelsRequest, encoded(remote::PixelsRequest{})}, {Kind::WidthsAnswer, encoded(remote::WidthsAnswer{})},
        {Kind::WidthsRequest, encoded(remote::WidthsRequest{})}};
}

/// Fresh ends refuse replies nobody asked for or out of turn, traffic of the wrong direction, bytes that do not decode and reports that are not the five platform messages.
void session_refusals() {
    context = "a platform end's refusals";
    refusal_case("an unsolicited frame", no_setup, frame_bytes(1));
    refusal_case("a frame for another request", [](remote::PlatformEnd& e) { e.request_frame(true, remote::Paint::IfDirty); }, frame_bytes(2), frame_bytes(1));
    refusal_case("a frame twice", [](remote::PlatformEnd& e) {
        e.request_frame(true, remote::Paint::IfDirty);
        e.request_frame(true, remote::Paint::IfDirty);
        e.receive(frame_bytes(1));
    }, frame_bytes(1), frame_bytes(2));
    refusal_case("a frame before the pointer report due", [](remote::PlatformEnd& e) {
        e.touch(1, 1, 1, 1, true);
        e.request_frame(false, remote::Paint::Skip);
    }, frame_bytes(1), pointer_bytes());
    refusal_case("a frame before the claim answer due", [](remote::PlatformEnd& e) {
        e.ask_claim(1, 1);
        e.request_frame(false, remote::Paint::Skip);
    }, frame_bytes(1), claim_bytes(1));
    refusal_case("an unsolicited pointer report", no_setup, pointer_bytes());
    refusal_case("a pointer report before the frame due", [](remote::PlatformEnd& e) {
        e.request_frame(false, remote::Paint::Force);
        e.hover(1, 1, 1);
    }, pointer_bytes(), frame_bytes(1));
    refusal_case("a pointer report twice", [](remote::PlatformEnd& e) {
        e.touch(1, 1, 1, 1, true);
        e.receive(pointer_bytes());
        e.ask_claim(1, 1);
    }, pointer_bytes(), claim_bytes(1));
    refusal_case("an unsolicited claim answer", no_setup, claim_bytes(1));
    refusal_case("a claim answer for another query", [](remote::PlatformEnd& e) { e.ask_claim(1, 1); }, claim_bytes(2), claim_bytes(1));
    refusal_case("claim answers out of order", [](remote::PlatformEnd& e) {
        e.ask_claim(1, 1);
        e.ask_claim(2, 2);
    }, claim_bytes(2), claim_bytes(1));
    refusal_case("a claim answer twice", [](remote::PlatformEnd& e) {
        e.ask_claim(1, 1);
        e.ask_claim(2, 2);
        e.receive(claim_bytes(1));
    }, claim_bytes(1), claim_bytes(2));
    refusal_case("a claim answer before the pointer report due", [](remote::PlatformEnd& e) {
        e.hover(1, 1, 1);
        e.ask_claim(1, 1);
    }, claim_bytes(1), pointer_bytes());

    for (const auto& [kind, bytes] : every_kind()) {
        const uint32_t k = uint32_t(kind);
        const bool one_way = k >= 64 && k <= 73, request = k >= 96;
        if (!one_way) refusal_case("kind " + std::to_string(k) + " as a one-way message", no_setup, bytes);
        if (!request) refusal_case("kind " + std::to_string(k) + " as a request", no_setup, bytes, {}, true);
    }

    const std::string state = encoded(remote::State{});
    const auto versioned = [](std::string bytes, uint8_t version) {
        bytes[1] = char(version);
        return bytes;
    };
    refusal_case("no bytes", no_setup, "", state);
    refusal_case("a kind alone", no_setup, state.substr(0, 1), state);
    refusal_case("kind 0", no_setup, Bytes().u8(0).u8(1).raw(state.substr(2)).s, state);
    refusal_case("kind 74", no_setup, Bytes().u8(74).u8(1).raw(state.substr(2)).s, state);
    refusal_case("kind 255", no_setup, Bytes().u8(255).u8(1).raw(state.substr(2)).s, state);
    refusal_case("a state of version 0", no_setup, versioned(state, 0), state);
    refusal_case("a state of version 2", no_setup, versioned(state, 2), state);
    refusal_case("a state cut short", no_setup, state.substr(0, state.size() - 1), state);
    refusal_case("a state with a byte after it", no_setup, state + '\0', state);
    std::string flag = state;
    flag[14] = 2;
    refusal_case("a state with a flag of 2", no_setup, flag, state);
    refusal_case("a frame that does not decode", [](remote::PlatformEnd& e) { e.request_frame(true, remote::Paint::IfDirty); },
        Bytes().head(Kind::Frame).raw("not a frame").s, frame_bytes(1));
    refusal_case("a canvas run whose batch does not decode", no_setup, Bytes().head(Kind::CanvasRun).i32(1).i32(2).i32(3).value("xx").s,
        encoded(remote::CanvasRun{1, 2, 3, {}}));
    const std::string font = encoded(remote::FontRequest{false, "serif", 12, 400, false, ""});
    refusal_case("no bytes as a request", no_setup, "", font, true);
    refusal_case("a font request cut short", no_setup, font.substr(0, font.size() - 1), font, true);
    refusal_case("a font request with a byte after it", no_setup, font + 'x', font, true);
    refusal_case("a font request of version 0", no_setup, versioned(font, 0), font, true);
    refusal_case("a pixels request wider than a canvas", no_setup, Bytes().head(Kind::PixelsRequest).i32(1).i32(0).i32(0).i32(8193).i32(1).s,
        encoded(remote::PixelsRequest{1, 0, 0, 8192, 1}), true);
    refusal_case("kind 37", no_setup, Bytes().u8(37).u8(1).u32(0).s, encoded(remote::WidthsRequest{}), true);
    refusal_case("kind 101", no_setup, Bytes().u8(101).u8(1).u32(0).s, encoded(remote::WidthsRequest{}), true);
    const remote::Caps defaults;
    remote::Caps wide = defaults;
    wide.widths = defaults.widths + 1;
    wide.widths_text = defaults.widths_text + 1;
    refusal_case("a widths request one text past the widths cap", no_setup, encoded(texts_of(defaults.widths + 1), wide), encoded(texts_of(defaults.widths)), true);
    refusal_case("a widths request one byte past the texts cap", no_setup, encoded(texts_of(1, defaults.widths_text + 1), wide), encoded(texts_of(1, defaults.widths_text)), true);
    remote::WidthsRequest split = texts_of(2, defaults.widths_text / 2);
    refusal_case("a widths request whose last text takes its texts one byte past their cap", no_setup, [&] {
        remote::WidthsRequest over = split;
        over.texts.back().text += 'x';
        return encoded(over, wide);
    }(), encoded(split), true);
    const std::string widths = encoded(texts_of(3));
    refusal_case("a widths request cut short", no_setup, widths.substr(0, widths.size() - 1), widths, true);
    refusal_case("a widths request with a byte after it", no_setup, widths + 'x', widths, true);
    refusal_case("a widths request of version 2", no_setup, versioned(widths, 2), widths, true);
    refusal_case("a widths request declaring a text more than it holds", no_setup, Bytes().head(Kind::WidthsRequest).u32(2).i32(1).str("a").s, widths, true);

    const std::string size = R"({"type":"size","width":1})";
    const char* const refused_texts[] = {"not json", "", "[1]", "\"size\"", "null", "{}", R"({"type":5})", R"({"type":null})", R"({"type":"event"})", R"({"type":"result"})",
        R"({"type":"resize"})", R"({"type":"paint"})", R"({"type":"Size"})", R"({"type":"size "})", R"({"kind":"size"})", R"({"type":"size"} x)", R"({"type":"size",})",
        "{\"type\":\"size\",\"t\":\"\x01\"}"};
    for (const char* text : refused_texts) refusal_case("a report of " + std::string(text), no_setup, report_bytes({text}), report_bytes({size}));
    refusal_case("a report nested five deep", no_setup, report_bytes({R"({"type":"size","a":{"b":{"c":{"d":{"e":1}}}}})"}),
        report_bytes({R"({"type":"size","a":{"b":{"c":{"d":1}}}})"}));
    refusal_case("a report of arrays nested five deep", no_setup, report_bytes({R"({"type":"bubble","a":[[[[1]]]]})"}), report_bytes({R"({"type":"bubble","a":[[[1]]]})"}));
    refusal_case("a report whose last text is refused", no_setup, report_bytes({size, R"({"type":"limit"})", R"({"type":"event"})"}),
        report_bytes({size, R"({"type":"limit"})"}));
}

/// Replies queued for a touch, a frame and a hold query come in that order; a frame asked for from the host's frame callback waits behind what was queued before it.
void session_queue() {
    context = "a platform end's queue";
    Session s;
    s.end.touch(1, 10, 10, 1, true);
    const uint64_t frame = s.end.request_frame(true, remote::Paint::IfDirty);
    const uint64_t query = s.end.ask_claim(10, 10);
    check(frame == 1 && query == 1 && s.channel.sent.size() == 3, "a touch, a frame request and a claim query go out");
    check(give(s, remote::Pointer{"auto", twidget::WidgetGestureClaim{}}) && give(s, remote::Frame{frame, {}, false, {}, std::nullopt}) && give(s, remote::ClaimAnswer{query, {}}),
        "their replies are taken in the order the inputs went");
    const uint64_t second = s.end.request_frame(false, remote::Paint::Skip);
    const uint64_t again = s.end.ask_claim(20, 20);
    s.host.ask_on_frame = true;
    check(give(s, remote::Frame{second, {}, false, {}, std::nullopt}), "the second frame is taken");
    check(s.host.asked.size() == 1 && s.host.asked[0] == 3, "the host asks for a third frame from its frame callback");
    was_sent(s, s.channel.sent.size() - 1, remote::FrameRequest{3, false, remote::Paint::IfDirty}, "the frame request made from the callback");
    check(give(s, remote::ClaimAnswer{again, {}}) && give(s, remote::Frame{3, {}, false, {}, std::nullopt}), "the claim answer queued before it, then the third frame, are taken");
    check(callbacks(s) == "pointer frame claim frame claim frame" && !s.end.refusing(), "the host hears them in order: " + callbacks(s));

    Session t;
    t.end.request_frame(false, remote::Paint::Skip);
    t.end.ask_claim(20, 20);
    t.host.ask_on_frame = true;
    check(give(t, remote::Frame{1, {}, false, {}, std::nullopt}) && t.host.asked.size() == 1 && t.host.asked[0] == 2, "a frame callback asks for the next frame");
    check(!give(t, remote::Frame{2, {}, false, {}, std::nullopt}) && t.end.refusing() && t.host.refusals.size() == 1, "which may not come before the claim answer queued before it");

    Session u;
    u.end.request_frame(false, remote::Paint::Skip);
    u.end.ask_claim(1, 1);
    u.host.ask_on_frame = true;
    u.host.ask_paint = remote::Paint(3);
    check(!give(u, remote::Frame{1, {}, false, {}, std::nullopt}) && u.host.frames.size() == 1 && u.host.asked.size() == 1 && u.host.asked[0] == 0,
        "a frame request the host makes from its callback that does not encode stops the end, and the frame's receive answers false");
    check(u.end.refusing() && u.host.refusals.size() == 1 && u.channel.sent.size() == 2, "the host hears of it once, nothing more sent");
    inert(u, "a stop from a callback");
}

// ───────────────────────── Frame deltas rebuilt ─────────────────────────

std::string list_bytes(const DisplayList& list) {
    std::string out;
    if (!wire::encode(list, {}, out)) check(false, "a list the checks build encodes");
    return out;
}

/// Runs that rebuild `list` from `base` without looking at commands: a copy of the bytes they begin with alike, the rest of `list` as it is, then a copy of the bytes they end with alike.
std::vector<remote::DeltaRun> ends_alike(std::string_view base, std::string_view list) {
    size_t head = 0, tail = 0;
    while (head < base.size() && head < list.size() && base[head] == list[head]) head++;
    while (tail < base.size() - head && tail < list.size() - head && base[base.size() - 1 - tail] == list[list.size() - 1 - tail]) tail++;
    std::vector<remote::DeltaRun> runs;
    if (head) runs.push_back({0, uint32_t(head), {}});
    if (list.size() > head + tail) runs.push_back({0, 0, std::string(list.substr(head, list.size() - head - tail))});
    if (tail) runs.push_back({uint32_t(base.size() - tail), uint32_t(tail), {}});
    return runs;
}

/// A rebuild in the allocation scope, held to what its message and its base allow together.
bool rebuilt(std::string_view message, const remote::Caps& caps, uint64_t base_seq, std::string_view base, remote::Frame& out, std::string& list, Measured* measured = nullptr) {
    const Measured m = measure(message.size() + base.size(), [&] { return remote::rebuild(message, caps, base_seq, base, out, list); });
    last_decode = m;
    measured_decodes++;
    if (measured) *measured = m;
    verify(!m.threw && !m.scope.over, [&] {
        return context + ": a rebuild of " + std::to_string(message.size()) + " bytes on " + std::to_string(base.size()) + " " + (m.threw ? "threw" : "returned") + " after asking for "
            + std::to_string(m.scope.asked) + " bytes against a budget of " + std::to_string(budget(message.size() + base.size()));
    });
    return m.ok && !m.threw;
}

/// `message` is not rebuilt on `base`, the Frame and the list given keep what they held, and the refusal asks for no room the delta declares; `nothing` when it must ask for nothing at all.
void not_rebuilt(const std::string& what, std::string_view message, const remote::Caps& caps, uint64_t base_seq, std::string_view base, bool nothing = false) {
    const remote::Frame expected = sentinel<remote::Frame>();
    remote::Frame out = expected;
    const std::vector<const void*> before = held(out);
    const std::string kept = "a rebuilt list the refusal leaves alone, longer than a small string's room";
    std::string list = kept;
    const char* const data = list.data();
    Measured m;
    const bool taken = rebuilt(message, caps, base_seq, base, out, list, &m);
    std::string why;
    const bool alone = equal(expected, out, why) && held(out) == before && list == kept && list.data() == data;
    verify(!taken && alone, [&] { return what + (taken ? ": rebuilt" : ": the outputs changed " + why); });
    verify(!nothing || m.scope.asked == 0, [&] { return what + ": the refusal asked for " + std::to_string(m.scope.asked) + " bytes"; });
    verify(m.scope.asked <= message.size() * PER_BYTE + FIXED, [&] { return what + ": the refusal asked for " + std::to_string(m.scope.asked) + " bytes, past what its message allows"; });
}

/// Two lists a FrameDelta goes between, the later with its damage.
struct DeltaCase {
    std::string name;
    DisplayList base, list;
    wire::Damage damage;
};

DisplayList moved_list() {
    DisplayList list = compact_list();
    list[1].x = 9.5f;
    return list;
}

std::vector<DeltaCase> delta_cases() {
    DisplayList longer = compact_list();
    longer.push_back(longer[3]);
    DisplayList shorter = compact_list();
    shorter.erase(shorter.begin() + 2);
    return {
        {"one member changed", compact_list(), moved_list(), std::vector<Rect>{{1, 2, 3, 4}}},
        {"the same list", compact_list(), compact_list(), std::vector<Rect>()},
        {"a command added that refers back to a shared path", compact_list(), longer, std::nullopt},
        {"a shared path's first user removed", compact_list(), shorter, std::nullopt},
        {"from an empty list", DisplayList(), compact_list(), std::nullopt},
        {"to an empty list", compact_list(), DisplayList(), std::vector<Rect>()},
    };
}

/// Each case's delta rebuilds its Frame member by member and its list's own bytes, wherever the base lies; every cut, a byte after it and another base are refused.
void deltas_rebuilt() {
    context = "frame deltas rebuilt";
    for (const DeltaCase& c : delta_cases()) {
        const std::string what = "a delta with " + c.name;
        const std::string base = list_bytes(c.base), list = list_bytes(c.list);
        const remote::FrameDelta delta{77, ordinary_state(), 76, ends_alike(base, list), c.damage};
        const std::string bytes = encoded(delta);
        const remote::Frame expected{77, ordinary_state(), true, c.list, c.damage};
        remote::Frame out = sentinel<remote::Frame>();
        std::string again = "a list the rebuild replaces";
        std::string why;
        check(rebuilt(bytes, {}, 76, base, out, again) && equal(expected, out, why), what + ": rebuilds the Frame member by member, its payloads shared alike " + why);
        check(again == list, what + ": the rebuilt bytes are the list's own");
        std::string whole;
        remote::WrittenList where;
        check(remote::encode(expected, {}, whole, SIZE_MAX, where) && where.size == again.size() && whole.compare(where.at, where.size, again) == 0,
            what + ": the rebuilt list is the one the Frame it stands for holds");
        const std::string padded = EARLIER + base + EARLIER;
        remote::Frame inside;
        check(rebuilt(bytes, {}, 76, std::string_view(padded).substr(EARLIER.size(), base.size()), inside, again) && equal(expected, inside, why),
            what + ": rebuilds on a base inside a larger buffer " + why);
        remote::FrameDelta decoded_delta;
        check(decoded(bytes, {}, decoded_delta) && equal(delta, decoded_delta, why), what + ": decodes as a FrameDelta member by member " + why);
        for (size_t n = 0; n < bytes.size(); n++) not_rebuilt(what + " cut to " + std::to_string(n) + " bytes", std::string_view(bytes).substr(0, n), {}, 76, base);
        not_rebuilt(what + " with a byte after it", bytes + '\0', {}, 76, base);
        not_rebuilt(what + " twice over", bytes + bytes, {}, 76, base);
        not_rebuilt(what + " on frame 75", bytes, {}, 75, base, true);
        not_rebuilt(what + " on frame 77", bytes, {}, 77, base, true);
        if (!base.empty() && delta.runs.back().literal.empty()) not_rebuilt(what + " on a base a byte short", bytes, {}, 76, std::string_view(base).substr(0, base.size() - 1), true);
    }
}

/// A FrameDelta's bytes up to its runs, laid out by hand: seq 2, a default state, base 1, the declared length and run count.
Bytes delta_head(uint32_t length, uint32_t runs, uint64_t base = 1) {
    Bytes b;
    b.head(Kind::FrameDelta).u64(2).state({}).u64(base).u32(length).u32(runs);
    return b;
}

std::string no_damage() {
    std::string out;
    wire::encode(wire::Damage(), {}, out);
    return out;
}

std::string with_damage(Bytes b, const std::string& damage) {
    return b.value(damage).s;
}

/// Delta bytes taken and refused against compact_list()'s, every refusal of a run before the delta's list is made.
void delta_bytes() {
    context = "frame delta bytes";
    const std::string base = list_bytes(compact_list()), list = list_bytes(moved_list()), damage = no_damage();
    const auto with = [&](Bytes b) { return with_damage(std::move(b), damage); };
    remote::Frame out;
    std::string again;
    check(rebuilt(with(delta_head(uint32_t(list.size()), 1).u8(1).str(list)), {}, 1, base, out, again) && again == list, "one literal of the whole list is taken");
    const size_t keep = list.size() - 4;
    check(rebuilt(with(delta_head(uint32_t(list.size()), 2).u8(1).str(list.substr(0, keep)).u8(1).str(list.substr(keep))), {}, 1, base, out, again) && again == list,
        "two literals that follow each other are taken");
    for (uint32_t tag : {2u, 3u, 0x80u, 0xffu})
        not_rebuilt("a run of tag " + std::to_string(tag), with(delta_head(uint32_t(list.size()), 1).u8(tag).str(list)), {}, 1, base, true);
    not_rebuilt("a copy of no bytes", with(delta_head(uint32_t(list.size()), 2).u8(0).u32(0).u32(0).u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("a literal of no bytes", with(delta_head(uint32_t(list.size()), 2).u8(1).str("").u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("runs a byte short of the declared length", with(delta_head(uint32_t(list.size() + 1), 1).u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("runs a byte past the declared length", with(delta_head(uint32_t(list.size() - 1), 1).u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("a second run past the declared length", with(delta_head(uint32_t(list.size()), 2).u8(1).str(list).u8(0).u32(0).u32(1)), {}, 1, base, true);
    not_rebuilt("a run count past the runs given", with(delta_head(uint32_t(list.size()), 2).u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("a run count past what the bytes hold", with(delta_head(uint32_t(list.size()), 0xffffffff).u8(1).str(list)), {}, 1, base, true);
    not_rebuilt("no runs", with(delta_head(uint32_t(list.size()), 0)), {}, 1, base, true);
    not_rebuilt("a declared length of 0", with(delta_head(0, 0)), {}, 1, base, true);
    not_rebuilt("a literal longer than the bytes left", delta_head(uint32_t(list.size()), 1).u8(1).u32(uint32_t(list.size() + 100)).raw(list).value(damage).s, {}, 1, base, true);
    const uint32_t size = uint32_t(base.size());
    const struct {
        const char* what;
        uint32_t offset, length;
    } copies[] = {
        {"a copy one byte past the base", size - 3, 4},
        {"a copy from the base's end", size, 1},
        {"a copy whose end wraps 32 bits", 0xffffffff, 2},
        {"a copy of 2^32 - 1 bytes", 0, 0xffffffff},
        {"a copy from 2^31", 0x80000000, 1},
    };
    for (const auto& c : copies) {
        const uint32_t length = c.length > 0x10000 ? uint32_t(list.size()) : uint32_t(list.size()) + c.length;
        not_rebuilt(c.what, with(delta_head(length, 2).u8(0).u32(c.offset).u32(c.length).u8(1).str(list)), {}, 1, base, true);
    }
    not_rebuilt("the base's last 4 bytes after the list, which make no list", with(delta_head(uint32_t(list.size() + 4), 2).u8(1).str(list).u8(0).u32(size - 4).u32(4)), {}, 1, base);
    not_rebuilt("a list that does not decode", with(delta_head(3, 1).u8(1).str("xyz")), {}, 1, base);
    std::vector<size_t> starts;
    std::string scratch;
    check(wire::encode(compact_list(), {}, scratch, starts), "the compact list encodes with its starts");
    const std::string one = list_bytes(DisplayList(1)).substr(0, 6);
    not_rebuilt("a copied reference to a payload the list has not made",
        with(delta_head(uint32_t(6 + starts[4] - starts[3]), 2).u8(1).str(one).u8(0).u32(uint32_t(starts[3])).u32(uint32_t(starts[4] - starts[3]))), {}, 1, base);
    not_rebuilt("a damage that does not decode", delta_head(uint32_t(list.size()), 1).u8(1).str(list).value("xx").s, {}, 1, base);
    not_rebuilt("a damage longer than the bytes left", delta_head(uint32_t(list.size()), 1).u8(1).str(list).u32(uint32_t(damage.size() + 1)).raw(damage).s, {}, 1, base);
    not_rebuilt("a byte after the damage", with(delta_head(uint32_t(list.size()), 1).u8(1).str(list)) + '\0', {}, 1, base);
    not_rebuilt("another kind", Bytes().head(Kind::Frame).raw(with(delta_head(uint32_t(list.size()), 1).u8(1).str(list)).substr(2)).s, {}, 1, base, true);
    std::string version = with(delta_head(uint32_t(list.size()), 1).u8(1).str(list));
    version[1] = 2;
    not_rebuilt("version 2", version, {}, 1, base, true);
}

/// The declared length, the run count, the copy bound, the frame and message caps and the base cap, each taken at its bound and refused one past it before the list is made.
void delta_bounds() {
    context = "frame delta bounds";
    const std::string base = list_bytes(compact_list()), list = list_bytes(moved_list()), damage = no_damage();
    const remote::FrameDelta delta{2, {}, 1, ends_alike(base, list), std::nullopt};
    const std::string bytes = encoded(delta);
    const std::string whole = encoded(remote::Frame{2, {}, true, moved_list(), std::nullopt});
    remote::Frame out;
    std::string again;
    remote::Caps at, below;
    at.delta_base = std::max(base.size(), list.size());
    below.delta_base = at.delta_base - 1;
    check(rebuilt(bytes, at, 1, base, out, again), "a list and a base as long as delta_base are taken");
    not_rebuilt("a list or a base a byte past delta_base", bytes, below, 1, base, true);
    remote::Caps none;
    none.delta_base = 0;
    not_rebuilt("a delta under a delta_base of 0", bytes, none, 1, base, true);
    const std::string tiny = list_bytes(DisplayList(1));
    const std::string to_tiny = encoded(remote::FrameDelta{2, {}, 1, {{0, 0, tiny}}, std::nullopt});
    check(rebuilt(to_tiny, remote::Caps{.delta_base = base.size()}, 1, base, out, again) && again == tiny, "a short list on a base as long as delta_base is taken");
    not_rebuilt("a short list on a base a byte past delta_base", to_tiny, remote::Caps{.delta_base = base.size() - 1}, 1, base, true);
    remote::FrameDelta struct_out;
    check(decoded(bytes, at, struct_out), "the FrameDelta decodes under the same delta_base");
    refused<remote::FrameDelta>("the FrameDelta under a delta_base a byte shorter", bytes, below);
    std::string kept = EARLIER;
    check(!remote::encode(delta, remote::Caps{.delta_base = list.size() - 1}, kept) && kept == EARLIER, "encoding a delta whose list passes delta_base is refused, the earlier value kept");
    not_rebuilt("a declared length past delta_base", with_damage(delta_head(uint32_t(remote::Caps().delta_base + 1), 1).u8(1).str(std::string(remote::Caps().delta_base + 1, 'x')), damage),
        {}, 1, base, true);
    not_rebuilt("a declared length of 2^32 - 1", with_damage(delta_head(0xffffffff, 1).u8(1).str(list), damage), {}, 1, base, true);

    remote::Caps frame_at, frame_below, message_at, message_below;
    frame_at.frame = whole.size();
    frame_below.frame = whole.size() - 1;
    message_at.message = std::max(whole.size(), bytes.size());
    message_below.message = message_at.message - 1;
    check(whole.size() > bytes.size(), "the delta is shorter than the Frame it stands for");
    check(rebuilt(bytes, frame_at, 1, base, out, again), "a delta whose Frame is as long as the frame cap is taken");
    not_rebuilt("a delta whose Frame is a byte past the frame cap", bytes, frame_below, 1, base, true);
    check(rebuilt(bytes, message_at, 1, base, out, again), "a delta whose Frame is as long as the message cap is taken");
    not_rebuilt("a delta whose Frame is a byte past the message cap", bytes, message_below, 1, base, true);
    check(decoded(bytes, frame_at, struct_out) && decoded(bytes, message_at, struct_out), "the FrameDelta decodes under the same caps");
    refused<remote::FrameDelta>("the FrameDelta whose Frame is a byte past the frame cap", bytes, frame_below);
    refused<remote::FrameDelta>("the FrameDelta whose Frame is a byte past the message cap", bytes, message_below);
    kept = EARLIER;
    check(!remote::encode(delta, frame_below, kept) && kept == EARLIER, "encoding a delta whose Frame passes the frame cap is refused, the earlier value kept");
    remote::Caps own;
    own.frame = bytes.size() - 1;
    not_rebuilt("a delta itself a byte past the frame cap", bytes, own, 1, base, true);
    const wire::Damage rects = std::vector<Rect>(64, Rect{1, 2, 3, 4});
    const remote::FrameDelta damaged{2, {}, 1, ends_alike(base, list), rects};
    const std::string damaged_bytes = encoded(damaged), damaged_whole = encoded(remote::Frame{2, {}, true, moved_list(), rects});
    remote::Caps damaged_at, damaged_below;
    damaged_at.frame = damaged_whole.size();
    damaged_below.frame = damaged_whole.size() - 1;
    check(damaged_bytes.size() <= damaged_below.frame && std::any_of(damaged.runs.begin(), damaged.runs.end(), [](const remote::DeltaRun& r) { return !r.literal.empty(); }), "a delta with a literal and 64 damage rects is shorter than its Frame");
    check(rebuilt(damaged_bytes, damaged_at, 1, base, out, again) && again == list && out.damage && out.damage->size() == 64, "a delta with damage whose Frame is as long as the frame cap is taken");
    not_rebuilt("a delta with damage whose Frame is a byte past the frame cap", damaged_bytes, damaged_below, 1, base, true);
    check(decoded(damaged_bytes, damaged_at, struct_out) && struct_out.damage && struct_out.damage->size() == 64, "the FrameDelta with damage decodes under the same cap");
    refused<remote::FrameDelta>("the FrameDelta with damage whose Frame is a byte past the frame cap", damaged_bytes, damaged_below);
    check(last_decode.scope.asked == 0, "its refusal asks for nothing: neither the runs, the literal nor the damage are made first");

    const DisplayList all = compact_list();
    const DisplayList three(all.begin(), all.begin() + 3);
    const std::string short_list = list_bytes(three);
    remote::Caps commands;
    commands.values.commands = 3;
    const auto pieces = [&](size_t n) {
        remote::FrameDelta d{2, {}, 1, {}, std::nullopt};
        for (size_t i = 0; i < n; i++) {
            const size_t from = short_list.size() * i / n, to = short_list.size() * (i + 1) / n;
            d.runs.push_back({0, 0, short_list.substr(from, to - from)});
        }
        return d;
    };
    check(rebuilt(encoded(pieces(4)), commands, 1, base, out, again) && again == short_list, "a command cap of 3 takes 4 runs");
    not_rebuilt("5 runs under a command cap of 3", encoded(pieces(5)), commands, 1, base, true);
    refused<remote::FrameDelta>("the FrameDelta of 5 runs under a command cap of 3", encoded(pieces(5)), commands);
    kept = EARLIER;
    check(!remote::encode(pieces(5), commands, kept) && kept == EARLIER, "encoding 5 runs under a command cap of 3 is refused, the earlier value kept");

    DisplayList four(4), more(5);
    for (DisplayList* l : {&four, &more})
        for (Command& c : *l) {
            c.rect = {1, 2, 3, 4};
            c.color = {9, 8, 7, 255};
        }
    const std::string repeated_base = list_bytes(four), longer = list_bytes(more);
    const uint32_t command = uint32_t((repeated_base.size() - 6) / 4);
    const auto copied = [&](size_t n) {
        Bytes b = delta_head(uint32_t(6 + n * command), uint32_t(1 + n));
        b.u8(1).str(list_bytes(n == 4 ? four : more).substr(0, 6));
        for (size_t i = 0; i < n; i++) b.u8(0).u32(6).u32(command);
        return b.value(damage).s;
    };
    check(rebuilt(copied(4), {}, 1, repeated_base, out, again) && again == repeated_base, "one command copied 4 times, no more than the base holds, is taken");
    not_rebuilt("one command copied 5 times, past what the base holds", copied(5), {}, 1, repeated_base, true);
    check(rebuilt(with_damage(delta_head(uint32_t(longer.size()), 1).u8(1).str(longer), damage), {}, 1, repeated_base, out, again) && again == longer,
        "the same list as one literal is taken");
}

// ───────────────────────── A platform end's frame deltas ─────────────────────────

std::string whole_frame(uint64_t seq, const DisplayList& list) {
    return encoded(remote::Frame{seq, {}, true, list, std::nullopt});
}

/// A FrameDelta from `from` to `to` that copies from painted frame `base`.
std::string delta_frame(uint64_t seq, uint64_t base, const DisplayList& from, const DisplayList& to) {
    return encoded(remote::FrameDelta{seq, ordinary_state(), base, ends_alike(list_bytes(from), list_bytes(to)), std::vector<Rect>{{1, 1, 2, 2}}});
}

DisplayList third_list() {
    DisplayList list = moved_list();
    list.push_back(list[3]);
    return list;
}

/// Frame 1 painted whole with compact_list(), then frame 2 asked for.
void painted_once(remote::PlatformEnd& e) {
    e.request_frame(true, remote::Paint::IfDirty);
    e.receive(whole_frame(1, compact_list()));
    e.request_frame(true, remote::Paint::IfDirty);
}

/// Painted frames whole and as deltas reach the host alike, each delta copying from the last painted list, which an unpainted frame keeps, a list past delta_base drops and a refusal clears.
void session_deltas() {
    context = "a platform end's frame deltas";
    std::string why;
    const DisplayList a = compact_list(), b = moved_list(), c = third_list();
    {
        Session s;
        const uint64_t first = s.end.request_frame(true, remote::Paint::IfDirty);
        check(s.end.receive(whole_frame(first, a)), "a whole painted frame is taken");
        const uint64_t second = s.end.request_frame(true, remote::Paint::IfDirty);
        check(s.end.receive(delta_frame(second, first, a, b)), "a delta that copies from it is taken");
        const remote::Frame expected{second, ordinary_state(), true, b, std::vector<Rect>{{1, 1, 2, 2}}};
        check(s.host.frames.size() == 2 && equal(expected, s.host.frames[1], why), "the host gets the Frame the delta stands for, member by member " + why);
        check(heard_in(s, expected.state, why) && equal(expected.state, s.end.state(), why), "the end's state is the delta's when the host hears of it " + why);
        const uint64_t third = s.end.request_frame(false, remote::Paint::IfDirty);
        check(give(s, remote::Frame{third, {}, false, {}, std::nullopt}), "an unpainted frame is taken");
        const uint64_t fourth = s.end.request_frame(true, remote::Paint::Force);
        check(s.end.receive(delta_frame(fourth, second, b, c)), "the next delta copies from the last painted frame, across the unpainted one");
        check(s.host.frames.size() == 4 && s.host.frames[3].seq == fourth && same(c, s.host.frames[3].list, why), "its list is the delta's " + why);
        const uint64_t fifth = s.end.request_frame(true, remote::Paint::Force);
        check(give(s, remote::Frame{fifth, {}, true, a, std::nullopt}), "a whole frame after deltas is taken");
        const uint64_t sixth = s.end.request_frame(true, remote::Paint::Force);
        check(s.end.receive(delta_frame(sixth, fifth, a, a)) && same(a, s.host.frames.back().list, why), "a delta that copies all of the whole frame is taken " + why);
        check(callbacks(s) == "frame frame frame frame frame frame" && s.backend.calls.size() == 6 && s.backend.count(Call::Frame) == 6 && !s.end.refusing(),
            "the host hears each frame once and the Backend nothing else: " + callbacks(s));
    }
    {
        Session s;
        const uint64_t first = s.end.request_frame(true, remote::Paint::IfDirty);
        check(s.end.receive(whole_frame(first, a)), "a whole painted frame is taken");
        const uint64_t second = s.end.request_frame(true, remote::Paint::IfDirty);
        check(give(s, remote::CanvasRun{1, 10, 10, compact_batch()}) && s.end.receive(delta_frame(second, first, a, b)) && give(s, remote::FontRelease{4}) && give(s, remote::CanvasRelease{1}),
            "a batch, a delta, a font release and a canvas release are taken");
        std::string ops;
        for (const Call& call : s.backend.calls) ops += std::to_string(int(call.op)) + " ";
        check(s.backend.calls.size() == 5 && s.backend.calls[1].op == Call::CanvasRun && s.backend.calls[2].op == Call::Frame && s.backend.calls[3].op == Call::DeleteFont
                && s.backend.calls[4].op == Call::CanvasRelease,
            "the Backend sees the batch, the delta's frame and the releases in the order they came: " + ops);
    }
    remote::Caps small;
    small.delta_base = list_bytes(a).size() - 1;
    {
        Session s(small);
        const uint64_t first = s.end.request_frame(true, remote::Paint::IfDirty);
        check(s.end.receive(whole_frame(first, DisplayList(1))), "a short whole frame is taken");
        const uint64_t second = s.end.request_frame(true, remote::Paint::IfDirty);
        check(s.end.receive(whole_frame(second, a)), "a whole frame past delta_base is taken");
        const uint64_t third = s.end.request_frame(true, remote::Paint::IfDirty);
        check(!s.end.receive(delta_frame(third, first, DisplayList(1), DisplayList(2))) && s.host.refusals.size() == 1
                && s.host.refusals[0] == "a message of kind 73 with no painted list kept to copy from" && s.host.frames.size() == 2,
            "a frame past delta_base drops the list kept before it, and a delta on either is refused");
    }
    const std::string good = delta_frame(2, 1, compact_list(), moved_list());
    const std::string base = list_bytes(compact_list());
    const std::string no_list = "a message of kind 73 with no painted list kept to copy from", undecoded = "a message of kind 73 did not decode",
                      unbuilt = "a message of kind 73 did not rebuild a frame";
    refusal_case("a delta with no frame painted before", [](remote::PlatformEnd& e) { e.request_frame(true, remote::Paint::IfDirty); },
        delta_frame(1, 0, DisplayList(), compact_list()), whole_frame(1, compact_list()), false, no_list);
    refusal_case("a delta after only an unpainted frame", [](remote::PlatformEnd& e) {
        e.request_frame(true, remote::Paint::IfDirty);
        e.receive(frame_bytes(1));
        e.request_frame(true, remote::Paint::IfDirty);
    }, delta_frame(2, 1, compact_list(), moved_list()), whole_frame(2, moved_list()), false, no_list);
    refusal_case("a delta copying from an older painted frame", [](remote::PlatformEnd& e) {
        painted_once(e);
        e.receive(whole_frame(2, moved_list()));
        e.request_frame(true, remote::Paint::IfDirty);
    }, delta_frame(3, 1, compact_list(), third_list()), delta_frame(3, 2, moved_list(), third_list()), false, "a message of kind 73 copying from frame 1 where 2 was painted last");
    refusal_case("a delta for another request", painted_once, delta_frame(3, 1, compact_list(), moved_list()), good, false, "a message of kind 73 for request 3 where 2 was due");
    refusal_case("a delta copying from frame 0", painted_once, delta_frame(2, 0, compact_list(), moved_list()), good, false,
        "a message of kind 73 copying from frame 0 where 1 was painted last");
    refusal_case("a delta where a pointer report was due", [](remote::PlatformEnd& e) {
        e.request_frame(true, remote::Paint::IfDirty);
        e.receive(whole_frame(1, compact_list()));
        e.touch(1, 1, 1, 1, true);
        e.request_frame(true, remote::Paint::IfDirty);
    }, delta_frame(2, 1, compact_list(), moved_list()), pointer_bytes(), false, "a message of kind 73 where kind 67 was due");
    refusal_case("an unsolicited delta", [](remote::PlatformEnd& e) {
        e.request_frame(true, remote::Paint::IfDirty);
        e.receive(whole_frame(1, compact_list()));
    }, delta_frame(2, 1, compact_list(), moved_list()), {}, false, "an unsolicited message of kind 73");
    refusal_case("a delta copying past its base", painted_once,
        with_damage(delta_head(uint32_t(5 + base.size()), 2).u8(0).u32(0).u32(uint32_t(base.size() + 1)).u8(1).str("abcd"), no_damage()), good, false, unbuilt);
    refusal_case("a delta whose list does not decode", painted_once, with_damage(delta_head(3, 1).u8(1).str("xyz"), no_damage()), good, false, unbuilt);
    refusal_case("a delta of an unknown run", painted_once, with_damage(delta_head(3, 1).u8(2).str("xyz"), no_damage()), good, false, unbuilt);
    refusal_case("a delta whose runs fall short of its length", painted_once, with_damage(delta_head(4, 1).u8(1).str("xyz"), no_damage()), good, false, unbuilt);
    for (size_t n : {size_t(2), size_t(40), size_t(41), size_t(60), good.size() - 1})
        refusal_case("a delta cut to " + std::to_string(n) + " bytes", painted_once, good.substr(0, n), good, false, n < 41 ? undecoded : unbuilt);
    refusal_case("a delta with a byte after it", painted_once, good + '\0', good, false, unbuilt);
}

/// An end its embedder closes stops for good without a word to the host: the frame it asked for and a delta on the list it kept are refused quietly, every later call does nothing, and closing again changes nothing.
void session_close() {
    context = "a closed platform end";
    Session s;
    const uint64_t first = s.end.request_frame(true, remote::Paint::IfDirty);
    check(s.end.receive(whole_frame(first, compact_list())), "a whole painted frame is taken");
    const uint64_t second = s.end.request_frame(true, remote::Paint::IfDirty);
    const size_t sent = s.channel.sent.size(), events = s.host.events.size();
    s.end.close();
    check(s.end.refusing() && s.host.refusals.empty() && s.host.events.size() == events && s.channel.sent.size() == sent, "closing stops the end without a word to the host, sending nothing");
    check(!s.end.receive(delta_frame(second, first, compact_list(), moved_list())) && !s.end.receive(whole_frame(second, moved_list())) && s.host.frames.size() == 1
            && s.host.refusals.empty(),
        "neither a delta on the kept list nor the frame it asked for is taken, and the host hears of neither");
    s.end.close();
    inert(s, "a closed end", 0);
}

/// A whole frame whose kept copy cannot be made is not taken: its request stays due, and the same frame given again reaches the host once and becomes the list a delta copies from.
void session_base_copy() {
    context = "a whole frame's kept copy";
    const std::string whole = whole_frame(1, compact_list());
    remote::Frame alone;
    std::string_view list;
    const Measured decode = measure(whole.size(), [&] { return remote::decode(whole, {}, alone, list); });
    Session s;
    s.end.request_frame(true, remote::Paint::IfDirty);
    bool threw = false;
    {
        Armed armed(whole.size());
        scope.budget = decode.scope.granted;
        try {
            s.end.receive(whole);
        } catch (const std::bad_alloc&) {
            threw = true;
        }
    }
    check(decode.ok && threw && scope.over, "the copy of the list the end keeps fails, the one allocation past what the frame's decode makes");
    check(s.host.frames.empty() && s.host.refusals.empty() && !s.end.refusing(), "the host hears nothing and the end still takes traffic");
    check(s.end.receive(whole) && s.host.frames.size() == 1 && s.host.frames[0].seq == 1 && s.host.refusals.empty(), "the same frame given again is taken and reaches the host once");
    const uint64_t second = s.end.request_frame(true, remote::Paint::IfDirty);
    check(s.end.receive(delta_frame(second, 1, compact_list(), moved_list())) && s.host.frames.size() == 2 && s.host.refusals.empty(), "a delta copies from it");
}

void print_reports() {
    size_t samples = 0, decodes = 0, cut = 0, head = 0, taken = 0, refusals = 0;
    for (const Report& r : reports) {
        std::printf("decoder %s (kind %u): %zu samples, %zu decodes: %zu cuts and %zu headers refused, %zu changes decoded and %zu refused\n", r.name.c_str(), unsigned(r.kind),
            r.samples, r.decodes, r.cuts, r.heads, r.accepted, r.refused);
        samples += r.samples;
        decodes += r.decodes;
        cut += r.cuts;
        head += r.heads;
        taken += r.accepted;
        refusals += r.refused;
    }
    std::printf("decoders: %zu kinds, %zu samples, %zu decodes: %zu cuts and %zu headers refused, %zu changes decoded and %zu refused\n", reports.size(), samples, decodes,
        cut, head, taken, refusals);
    std::set<uint32_t> covered;
    for (const Report& r : reports) covered.insert(uint32_t(r.kind));
    check(reports.size() == std::size(KINDS) && covered.size() == std::size(KINDS), "every kind's decoder is checked once");
}

} // namespace

int main() {
    kinds();
    golden_bytes();
    flags();
    inactive_fields();
    count_sites();
    value_lengths();
    nested_caps();
    pixels();
    widths();
    encoder_room();

    suite("FrameRequest", Kind::FrameRequest, frame_requests(), 0x01000000);
    suite("SetWidth", Kind::SetWidth, set_widths(), 0x02000000);
    suite("SetMaxHeight", Kind::SetMaxHeight, set_max_heights(), 0x03000000);
    suite("SetTheme", Kind::SetTheme, set_themes(), 0x04000000);
    suite("Touch", Kind::Touch, touches(), 0x05000000);
    suite("Hover", Kind::Hover, hovers(), 0x06000000);
    suite("ClearSelection", Kind::ClearSelection, clear_selections(), 0x07000000);
    suite("ClaimQuery", Kind::ClaimQuery, claim_queries(), 0x08000000);
    suite("FontAnswer", Kind::FontAnswer, font_answers(), 0x20000000);
    suite("WidthAnswer", Kind::WidthAnswer, width_answers(), 0x21000000);
    suite("ImageAnswer", Kind::ImageAnswer, image_answers(), 0x22000000);
    suite("PixelsAnswer", Kind::PixelsAnswer, pixels_answers(), 0x23000000);
    suite("WidthsAnswer", Kind::WidthsAnswer, widths_answers(), 0x24000000);
    suite("Frame", Kind::Frame, frames(), 0x40000000);
    suite("State", Kind::State, states(), 0x41000000);
    suite("Messages", Kind::Messages, messages(), 0x42000000);
    suite("Pointer", Kind::Pointer, pointers(), 0x43000000);
    suite("Selection", Kind::Selection, selections(), 0x44000000);
    suite("ClaimAnswer", Kind::ClaimAnswer, claim_answers(), 0x45000000);
    suite("FontRelease", Kind::FontRelease, font_releases(), 0x46000000);
    suite("CanvasRun", Kind::CanvasRun, canvas_runs(), 0x47000000);
    suite("CanvasRelease", Kind::CanvasRelease, canvas_releases(), 0x48000000);
    suite("FrameDelta", Kind::FrameDelta, frame_deltas(), 0x49000000);
    suite("FontRequest", Kind::FontRequest, font_requests(), 0x60000000);
    suite("WidthRequest", Kind::WidthRequest, width_requests(), 0x61000000);
    suite("ImageRequest", Kind::ImageRequest, image_requests(), 0x62000000);
    suite("PixelsRequest", Kind::PixelsRequest, pixels_requests(), 0x63000000);
    suite("WidthsRequest", Kind::WidthsRequest, widths_requests(), 0x64000000);

    session_inputs();
    session_requests();
    session_caps();
    session_widths();
    session_reports();
    session_replies();
    session_order();
    session_refusals();
    session_queue();
    deltas_rebuilt();
    delta_bytes();
    delta_bounds();
    session_deltas();
    session_close();
    session_base_copy();
    widths_in_requests();

    print_reports();
    std::printf("platform end: a session of every input, request, widths batch, one-way Backend call and report, and %zu refusal cases\n", refusal_cases);
    std::printf("allocation: %zu decodes measured against a budget of %zu bytes for each input byte and %zu more\n", measured_decodes, PER_BYTE, FIXED);
    print_extreme("the nearest its budget", nearest_budget);
    print_extreme("the most per input byte of 256 or more", most_per_byte);
    std::printf("%d checks, %d failure(s)\n", checks, failures);
    return failures ? 1 : 0;
}
