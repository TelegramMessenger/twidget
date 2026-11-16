// wire.h's forms: a Writer appends fields to the caller's string, a Reader takes them from what is left of the bytes, checking every count against its cap and against what is left before making room for it.
#include "wire.h"

#include <algorithm>
#include <climits>
#include <cstring>
#include <limits>
#include <type_traits>
#include <unordered_map>
#include <utility>

namespace twlite::wire {
namespace {

static_assert(CHAR_BIT == 8 && sizeof(int) == 4 && sizeof(float) == 4 && std::numeric_limits<float>::is_iec559);
// The last value of each enum version 1 carries: one added after it is refused until a form takes it.
static_assert(int(Cmd::Backdrop) == 17 && Path::Close == 4 && Filter::DropShadow == 9);
static_assert(CanvasOp::PutImage == 6 && CanvasPaint::Conic == 3 && uint8_t(Composite::Luminosity) == 25);

enum CommandBit {
    CMD_RECT, CMD_RADII, CMD_CLIP, CMD_HAS_CLIP, CMD_COLOR, CMD_WIDTH, CMD_FONT, CMD_TEXT, CMD_X, CMD_Y, CMD_IMAGE, CMD_CANVAS,
    CMD_AX, CMD_AY, CMD_BX, CMD_BY, CMD_STOPS, CMD_PATH, CMD_FILTER, CMD_OPACITY, CMD_MATRIX, CMD_HIDDEN, CMD_INSET, CMD_KEY,
    COMMAND_BITS
};
enum PathBit {
    PATH_OPS, PATH_POINTS, PATH_FILL, PATH_EVENODD, PATH_FILL_COLOR, PATH_GRADIENT, PATH_X1, PATH_Y1, PATH_X2, PATH_Y2, PATH_R,
    PATH_FX, PATH_FY, PATH_STOPS, PATH_STROKE, PATH_STROKE_COLOR, PATH_STROKE_WIDTH, PATH_LINE_CAP, PATH_LINE_JOIN,
    PATH_MITER_LIMIT, PATH_DASHES, PATH_DASH_OFFSET, PATH_STROKE_GRADIENT, PATH_SG, PATH_STROKE_STOPS, PATH_BITS
};
static_assert(COMMAND_BITS == 24 && PATH_BITS == 25);
enum CanvasOpBit {
    OP_PATH, OP_EVENODD, OP_PAINT, OP_MATRIX, OP_LINE_WIDTH, OP_LINE_CAP, OP_LINE_JOIN, OP_MITER_LIMIT, OP_DASHES, OP_DASH_OFFSET,
    OP_ALPHA, OP_COMPOSITE, OP_SHADOW_COLOR, OP_SHADOW_BLUR, OP_SHADOW_X, OP_SHADOW_Y, OP_CLIPS, OP_FONT, OP_TEXT, OP_X, OP_Y,
    OP_SCALE_X, OP_TEXT_DX, OP_STROKE_TEXT, OP_IMAGE, OP_CANVAS, OP_SX, OP_SY, OP_SW, OP_SH, OP_DX, OP_DY, OP_DW, OP_DH,
    OP_SMOOTHING, OP_RGBA, OP_W, OP_H, CANVAS_OP_BITS
};
enum PaintBit { PAINT_COLOR, PAINT_ARGS, PAINT_STOPS, PAINT_BITS };
static_assert(CANVAS_OP_BITS == 38 && PAINT_BITS == 3);

/// The fewest bytes one item of a counted list takes.
constexpr size_t STOP_BYTES = 8, FLOAT_BYTES = 4, OP_BYTES = 1, FILTER_BYTES = 17, COMMAND_BYTES = 5, CANVAS_OP_BYTES = 9, CLIP_BYTES = 9,
    RECT_BYTES = 16;

using Clips = std::vector<CanvasClip>;
using Pixels = std::vector<uint8_t>;

uint32_t bits(const float& f) {
    uint32_t b = 0;
    std::memcpy(&b, &f, sizeof b);
    return b;
}

/// The int whose two's-complement bits `u` holds, by no conversion that overflows.
int to_int(uint32_t u) {
    return u <= uint32_t(INT_MAX) ? int(u) : int(u - uint32_t(INT_MAX) - 1) - INT_MAX - 1;
}

template <class U> bool has(U mask, int bit) {
    return ((mask >> bit) & 1) != 0;
}

bool same(const float& a, const float& b) {
    return bits(a) == bits(b);
}
template <size_t N> bool same(const float (&a)[N], const float (&b)[N]) {
    for (size_t i = 0; i < N; i++)
        if (!same(a[i], b[i])) return false;
    return true;
}
bool same(const Color& a, const Color& b) {
    return a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a;
}
bool same(const Rect& a, const Rect& b) {
    return same(a.x, b.x) && same(a.y, b.y) && same(a.w, b.w) && same(a.h, b.h);
}
bool same(const Radii& a, const Radii& b) {
    return same(a.tlx, b.tlx) && same(a.tly, b.tly) && same(a.trx, b.trx) && same(a.try_, b.try_) && same(a.brx, b.brx)
        && same(a.bry, b.bry) && same(a.blx, b.blx) && same(a.bly, b.bly);
}

/// Version 1's defaults, written out rather than taken from the structs, so that old bytes keep their meaning.
struct Defaults {
    Command command;
    Path path;
    CanvasOp op;
    CanvasPaint paint;
};

void identity(float (&m)[6]) {
    m[0] = 1;
    m[1] = 0;
    m[2] = 0;
    m[3] = 1;
    m[4] = 0;
    m[5] = 0;
}

Defaults version_1() {
    Defaults d;
    const Color black{0, 0, 0, 255};
    Command& c = d.command;
    c.kind = Cmd::FillRect;
    c.rect = c.clip = Rect{0, 0, 0, 0};
    c.radii = Radii{0, 0, 0, 0, 0, 0, 0, 0};
    c.has_clip = c.hidden = c.inset = false;
    c.color = black;
    c.width = c.x = c.y = c.ax = c.ay = c.bx = c.by = 0;
    c.font = c.image = c.canvas = 0;
    c.text = {};
    c.stops = {};
    c.path = {};
    c.filter = {};
    c.opacity = 1;
    identity(c.matrix);
    c.key = 0;
    Path& p = d.path;
    p.ops = {};
    p.points = p.dashes = {};
    p.fill = p.evenodd = p.stroke = false;
    p.fill_color = p.stroke_color = black;
    p.gradient = p.line_cap = p.line_join = p.stroke_gradient = 0;
    p.x1 = p.y1 = p.x2 = p.y2 = p.r = p.fx = p.fy = p.dash_offset = 0;
    p.stops = p.stroke_stops = {};
    p.stroke_width = 1;
    p.miter_limit = 4;
    for (float& v : p.sg) v = 0;
    CanvasPaint& paint = d.paint;
    paint.kind = CanvasPaint::Solid;
    paint.color = black;
    for (float& v : paint.args) v = 0;
    paint.stops = {};
    CanvasOp& o = d.op;
    o.kind = CanvasOp::Fill;
    o.path = Geometry{};
    o.evenodd = o.stroke_text = false;
    o.paint = paint;
    identity(o.matrix);
    o.line_width = o.alpha = o.scale_x = 1;
    o.line_cap = o.line_join = o.font = o.image = o.canvas = o.w = o.h = 0;
    o.miter_limit = 10;
    o.dashes = {};
    o.dash_offset = o.shadow_blur = o.shadow_x = o.shadow_y = o.x = o.y = o.text_dx = 0;
    o.sx = o.sy = o.dx = o.dy = o.dw = o.dh = 0;
    o.sw = o.sh = -1;
    o.composite = Composite::SourceOver;
    o.shadow_color = Color{0, 0, 0, 0};
    o.clips = {};
    o.text = {};
    o.smoothing = true;
    o.rgba = {};
    return d;
}

const Defaults& v1() {
    static const Defaults d = version_1();
    return d;
}

class Writer {
public:
    explicit Writer(std::string& out) : out_(out) {}

    size_t size() const { return out_.size(); }
    template <class U> void put(U v) {
        for (size_t i = 0; i < sizeof(U); i++) out_.push_back(char(uint8_t(v >> (8 * i))));
    }
    /// Overwrites the `U` put at `at`.
    template <class U> void patch(size_t at, U v) {
        for (size_t i = 0; i < sizeof(U); i++) out_[at + i] = char(uint8_t(v >> (8 * i)));
    }
    void u8(uint8_t v) { put(v); }
    void u32(uint32_t v) { put(v); }
    void u64(uint64_t v) { put(v); }
    void i32(int v) { put(uint32_t(v)); }
    void f32(const float& v) { put(bits(v)); }
    template <size_t N> void f32s(const float (&v)[N]) {
        for (const float& f : v) f32(f);
    }
    void color(const Color& c) {
        u8(c.r);
        u8(c.g);
        u8(c.b);
        u8(c.a);
    }
    void rect(const Rect& r) {
        f32(r.x);
        f32(r.y);
        f32(r.w);
        f32(r.h);
    }
    void radii(const Radii& r) {
        f32(r.tlx);
        f32(r.tly);
        f32(r.trx);
        f32(r.try_);
        f32(r.brx);
        f32(r.bry);
        f32(r.blx);
        f32(r.bly);
    }
    void flag(bool v) { u8(v ? 1 : 0); }
    void head(Form form) {
        u8(uint8_t(form));
        u8(VERSION);
    }
    bool count(size_t n, size_t cap) {
        if (n > std::min<size_t>(cap, UINT32_MAX)) return false;
        u32(uint32_t(n));
        return true;
    }
    bool str(const std::string& s, size_t cap) {
        if (!count(s.size(), cap)) return false;
        out_.append(s);
        return true;
    }
    bool bytes(const Pixels& v, size_t cap) {
        if (!count(v.size(), cap)) return false;
        if (!v.empty()) out_.append(v.begin(), v.end());
        return true;
    }
    bool stops(const std::vector<Stop>& v, size_t cap) {
        if (!count(v.size(), cap)) return false;
        for (const Stop& s : v) {
            f32(s.offset);
            color(s.color);
        }
        return true;
    }
    bool floats(const std::vector<float>& v, size_t cap) {
        if (!count(v.size(), cap)) return false;
        for (const float& f : v) f32(f);
        return true;
    }
    bool path_ops(const std::vector<Path::Op>& v, size_t cap) {
        if (!count(v.size(), cap)) return false;
        for (Path::Op op : v) {
            if (op > Path::Close) return false;
            u8(op);
        }
        return true;
    }
    bool geometry(const Geometry& g, const Caps& caps) { return path_ops(g.ops, caps.path_ops) && floats(g.points, caps.floats); }

private:
    std::string& out_;
};

/// A presence mask put as 0 before the members it marks and patched after them, so `if (m.set(BIT, ...)) w.field(...)` writes each in its place.
template <class U> class Mask {
public:
    explicit Mask(Writer& w) : w_(w), at_(w.size()) { w.put(U(0)); }

    bool set(int bit, bool present) {
        if (present) bits_ |= U(1) << bit;
        return present;
    }
    /// A bool of no bytes: its bit says it is true.
    void flag(int bit, bool value) {
        if (value) bits_ |= U(1) << bit;
    }
    void finish() { w_.patch(at_, bits_); }

private:
    Writer& w_;
    size_t at_ = 0;
    U bits_ = 0;
};

/// Reads into members fresh from their defaults, each field only when all of it is left.
class Reader {
public:
    explicit Reader(std::string_view bytes) : rest_(bytes) {}

    bool done() const { return rest_.empty(); }
    bool u8(uint8_t& v) { return little(v); }
    bool u32(uint32_t& v) { return little(v); }
    bool u64(uint64_t& v) { return little(v); }
    bool i32(int& v) {
        uint32_t u = 0;
        if (!u32(u)) return false;
        v = to_int(u);
        return true;
    }
    bool f32(float& v) {
        uint32_t b = 0;
        if (!u32(b)) return false;
        std::memcpy(&v, &b, sizeof v);
        return true;
    }
    template <size_t N> bool f32s(float (&v)[N]) {
        for (float& f : v)
            if (!f32(f)) return false;
        return true;
    }
    bool color(Color& c) { return u8(c.r) && u8(c.g) && u8(c.b) && u8(c.a); }
    bool rect(Rect& r) { return f32(r.x) && f32(r.y) && f32(r.w) && f32(r.h); }
    bool radii(Radii& r) {
        return f32(r.tlx) && f32(r.tly) && f32(r.trx) && f32(r.try_) && f32(r.brx) && f32(r.bry) && f32(r.blx) && f32(r.bly);
    }
    bool flag(bool& v) {
        uint8_t b = 0;
        if (!u8(b) || b > 1) return false;
        v = b == 1;
        return true;
    }
    bool head(Form form) {
        uint8_t f = 0, v = 0;
        return u8(f) && f == uint8_t(form) && u8(v) && v == VERSION;
    }
    /// A count of items of at least `item_bytes` each, checked by division so that nothing wraps.
    bool count(size_t& n, size_t cap, size_t item_bytes) {
        uint32_t c = 0;
        if (!u32(c) || c > cap || c > rest_.size() / item_bytes) return false;
        n = c;
        return true;
    }
    bool str(std::string& s, size_t cap) {
        size_t n = 0;
        if (!count(n, cap, 1)) return false;
        if (n) {
            s.assign(rest_.data(), n);
            rest_.remove_prefix(n);
        }
        return true;
    }
    bool bytes(Pixels& v, size_t cap) {
        size_t n = 0;
        if (!count(n, cap, 1)) return false;
        if (n) {
            v.assign(rest_.data(), rest_.data() + n);
            rest_.remove_prefix(n);
        }
        return true;
    }
    bool stops(std::vector<Stop>& v, size_t cap) {
        size_t n = 0;
        if (!count(n, cap, STOP_BYTES)) return false;
        v.reserve(n);
        for (size_t i = 0; i < n; i++) {
            Stop& s = v.emplace_back();
            if (!f32(s.offset) || !color(s.color)) return false;
        }
        return true;
    }
    bool floats(std::vector<float>& v, size_t cap) {
        size_t n = 0;
        if (!count(n, cap, FLOAT_BYTES)) return false;
        v.resize(n);
        for (float& f : v)
            if (!f32(f)) return false;
        return true;
    }
    bool path_ops(std::vector<Path::Op>& v, size_t cap) {
        size_t n = 0;
        if (!count(n, cap, OP_BYTES)) return false;
        v.reserve(n);
        for (size_t i = 0; i < n; i++) {
            uint8_t op = 0;
            if (!u8(op) || op > Path::Close) return false;
            v.push_back(Path::Op(op));
        }
        return true;
    }
    bool geometry(Geometry& g, const Caps& caps) { return path_ops(g.ops, caps.path_ops) && floats(g.points, caps.floats); }

private:
    template <class U> bool little(U& v) {
        if (rest_.size() < sizeof(U)) return false;
        U x = 0;
        for (size_t i = 0; i < sizeof(U); i++) x = U(x | (U(uint8_t(rest_[i])) << (8 * i)));
        v = x;
        rest_.remove_prefix(sizeof(U));
        return true;
    }

    std::string_view rest_;
};

/// The payloads of one kind a value has written, by address: the first user puts 0 and the payload, a later one its place from 1.
template <class T> class WrittenPayloads {
public:
    template <class Write> bool put(Writer& w, const std::shared_ptr<const T>& p, Write write) {
        const auto [at, fresh] = places_.try_emplace(p.get(), uint32_t(places_.size() + 1));
        if (!fresh) {
            w.u32(at->second);
            return true;
        }
        w.u32(0);
        return write(*p);
    }

private:
    std::unordered_map<const T*, uint32_t> places_;
};

/// The payloads of one kind a value has read, in order; a new one joins them only once it is whole.
template <class T> class ReadPayloads {
public:
    template <class Read> bool get(Reader& r, const T& initial, Read read, std::shared_ptr<const T>& out) {
        uint32_t place = 0;
        if (!r.u32(place)) return false;
        if (place) {
            if (place > payloads_.size()) return false;
            out = payloads_[place - 1];
            return true;
        }
        std::shared_ptr<T> fresh = std::make_shared<T>(initial);
        if (!read(*fresh)) return false;
        out = payloads_.emplace_back(std::move(fresh));
        return true;
    }

private:
    std::vector<std::shared_ptr<const T>> payloads_;
};

/// Runs `write` over `out`, cutting `out` back to its size on entry when it refuses.
template <class Write> bool appended(std::string& out, Write write) {
    const size_t start = out.size();
    Writer w(out);
    if (write(w)) return true;
    out.resize(start);
    return false;
}

bool write_path(Writer& w, const Caps& caps, const Path& p) {
    const Path& d = v1().path;
    Mask<uint32_t> m(w);
    if (m.set(PATH_OPS, !p.ops.empty()) && !w.path_ops(p.ops, caps.path_ops)) return false;
    if (m.set(PATH_POINTS, !p.points.empty()) && !w.floats(p.points, caps.floats)) return false;
    m.flag(PATH_FILL, p.fill);
    m.flag(PATH_EVENODD, p.evenodd);
    if (m.set(PATH_FILL_COLOR, !same(p.fill_color, d.fill_color))) w.color(p.fill_color);
    if (m.set(PATH_GRADIENT, p.gradient != d.gradient)) w.i32(p.gradient);
    if (m.set(PATH_X1, !same(p.x1, d.x1))) w.f32(p.x1);
    if (m.set(PATH_Y1, !same(p.y1, d.y1))) w.f32(p.y1);
    if (m.set(PATH_X2, !same(p.x2, d.x2))) w.f32(p.x2);
    if (m.set(PATH_Y2, !same(p.y2, d.y2))) w.f32(p.y2);
    if (m.set(PATH_R, !same(p.r, d.r))) w.f32(p.r);
    if (m.set(PATH_FX, !same(p.fx, d.fx))) w.f32(p.fx);
    if (m.set(PATH_FY, !same(p.fy, d.fy))) w.f32(p.fy);
    if (m.set(PATH_STOPS, !p.stops.empty()) && !w.stops(p.stops, caps.stops)) return false;
    m.flag(PATH_STROKE, p.stroke);
    if (m.set(PATH_STROKE_COLOR, !same(p.stroke_color, d.stroke_color))) w.color(p.stroke_color);
    if (m.set(PATH_STROKE_WIDTH, !same(p.stroke_width, d.stroke_width))) w.f32(p.stroke_width);
    if (m.set(PATH_LINE_CAP, p.line_cap != d.line_cap)) w.i32(p.line_cap);
    if (m.set(PATH_LINE_JOIN, p.line_join != d.line_join)) w.i32(p.line_join);
    if (m.set(PATH_MITER_LIMIT, !same(p.miter_limit, d.miter_limit))) w.f32(p.miter_limit);
    if (m.set(PATH_DASHES, !p.dashes.empty()) && !w.floats(p.dashes, caps.floats)) return false;
    if (m.set(PATH_DASH_OFFSET, !same(p.dash_offset, d.dash_offset))) w.f32(p.dash_offset);
    if (m.set(PATH_STROKE_GRADIENT, p.stroke_gradient != d.stroke_gradient)) w.i32(p.stroke_gradient);
    if (m.set(PATH_SG, !same(p.sg, d.sg))) w.f32s(p.sg);
    if (m.set(PATH_STROKE_STOPS, !p.stroke_stops.empty()) && !w.stops(p.stroke_stops, caps.stops)) return false;
    m.finish();
    return true;
}

bool read_path(Reader& r, const Caps& caps, Path& p) {
    uint32_t mask = 0;
    if (!r.u32(mask) || mask >> PATH_BITS) return false;
    if (has(mask, PATH_OPS) && !r.path_ops(p.ops, caps.path_ops)) return false;
    if (has(mask, PATH_POINTS) && !r.floats(p.points, caps.floats)) return false;
    if (has(mask, PATH_FILL)) p.fill = true;
    if (has(mask, PATH_EVENODD)) p.evenodd = true;
    if (has(mask, PATH_FILL_COLOR) && !r.color(p.fill_color)) return false;
    if (has(mask, PATH_GRADIENT) && !r.i32(p.gradient)) return false;
    if (has(mask, PATH_X1) && !r.f32(p.x1)) return false;
    if (has(mask, PATH_Y1) && !r.f32(p.y1)) return false;
    if (has(mask, PATH_X2) && !r.f32(p.x2)) return false;
    if (has(mask, PATH_Y2) && !r.f32(p.y2)) return false;
    if (has(mask, PATH_R) && !r.f32(p.r)) return false;
    if (has(mask, PATH_FX) && !r.f32(p.fx)) return false;
    if (has(mask, PATH_FY) && !r.f32(p.fy)) return false;
    if (has(mask, PATH_STOPS) && !r.stops(p.stops, caps.stops)) return false;
    if (has(mask, PATH_STROKE)) p.stroke = true;
    if (has(mask, PATH_STROKE_COLOR) && !r.color(p.stroke_color)) return false;
    if (has(mask, PATH_STROKE_WIDTH) && !r.f32(p.stroke_width)) return false;
    if (has(mask, PATH_LINE_CAP) && !r.i32(p.line_cap)) return false;
    if (has(mask, PATH_LINE_JOIN) && !r.i32(p.line_join)) return false;
    if (has(mask, PATH_MITER_LIMIT) && !r.f32(p.miter_limit)) return false;
    if (has(mask, PATH_DASHES) && !r.floats(p.dashes, caps.floats)) return false;
    if (has(mask, PATH_DASH_OFFSET) && !r.f32(p.dash_offset)) return false;
    if (has(mask, PATH_STROKE_GRADIENT) && !r.i32(p.stroke_gradient)) return false;
    if (has(mask, PATH_SG) && !r.f32s(p.sg)) return false;
    if (has(mask, PATH_STROKE_STOPS) && !r.stops(p.stroke_stops, caps.stops)) return false;
    return true;
}

bool write_filters(Writer& w, const Caps& caps, const Filters& list) {
    if (!w.count(list.size(), caps.filters)) return false;
    for (const Filter& f : list) {
        if (f.kind > Filter::DropShadow) return false;
        w.u8(f.kind);
        w.f32(f.amount);
        w.f32(f.x);
        w.f32(f.y);
        w.color(f.color);
    }
    return true;
}

bool read_filters(Reader& r, const Caps& caps, Filters& list) {
    size_t n = 0;
    if (!r.count(n, caps.filters, FILTER_BYTES)) return false;
    list.reserve(n);
    for (size_t i = 0; i < n; i++) {
        Filter& f = list.emplace_back();
        uint8_t kind = 0;
        if (!r.u8(kind) || kind > Filter::DropShadow) return false;
        f.kind = Filter::Kind(kind);
        if (!r.f32(f.amount) || !r.f32(f.x) || !r.f32(f.y) || !r.color(f.color)) return false;
    }
    return true;
}

bool write_command(Writer& w, const Caps& caps, WrittenPayloads<Path>& paths, WrittenPayloads<Filters>& filters, const Command& c) {
    const int kind = int(c.kind);
    if (kind < 0 || kind > int(Cmd::Backdrop)) return false;
    const Command& d = v1().command;
    w.u8(uint8_t(kind));
    Mask<uint32_t> m(w);
    if (m.set(CMD_RECT, !same(c.rect, d.rect))) w.rect(c.rect);
    if (m.set(CMD_RADII, !same(c.radii, d.radii))) w.radii(c.radii);
    if (m.set(CMD_CLIP, !same(c.clip, d.clip))) w.rect(c.clip);
    m.flag(CMD_HAS_CLIP, c.has_clip);
    if (m.set(CMD_COLOR, !same(c.color, d.color))) w.color(c.color);
    if (m.set(CMD_WIDTH, !same(c.width, d.width))) w.f32(c.width);
    if (m.set(CMD_FONT, c.font != d.font)) w.i32(c.font);
    if (m.set(CMD_TEXT, !c.text.empty()) && !w.str(c.text, caps.text)) return false;
    if (m.set(CMD_X, !same(c.x, d.x))) w.f32(c.x);
    if (m.set(CMD_Y, !same(c.y, d.y))) w.f32(c.y);
    if (m.set(CMD_IMAGE, c.image != d.image)) w.i32(c.image);
    if (m.set(CMD_CANVAS, c.canvas != d.canvas)) w.i32(c.canvas);
    if (m.set(CMD_AX, !same(c.ax, d.ax))) w.f32(c.ax);
    if (m.set(CMD_AY, !same(c.ay, d.ay))) w.f32(c.ay);
    if (m.set(CMD_BX, !same(c.bx, d.bx))) w.f32(c.bx);
    if (m.set(CMD_BY, !same(c.by, d.by))) w.f32(c.by);
    if (m.set(CMD_STOPS, !c.stops.empty()) && !w.stops(c.stops, caps.stops)) return false;
    if (m.set(CMD_PATH, c.path != nullptr)
        && !paths.put(w, c.path, [&](const Path& p) { return write_path(w, caps, p); }))
        return false;
    if (m.set(CMD_FILTER, c.filter != nullptr)
        && !filters.put(w, c.filter, [&](const Filters& f) { return write_filters(w, caps, f); }))
        return false;
    if (m.set(CMD_OPACITY, !same(c.opacity, d.opacity))) w.f32(c.opacity);
    if (m.set(CMD_MATRIX, !same(c.matrix, d.matrix))) w.f32s(c.matrix);
    m.flag(CMD_HIDDEN, c.hidden);
    m.flag(CMD_INSET, c.inset);
    if (m.set(CMD_KEY, c.key != d.key)) w.u64(c.key);
    m.finish();
    return true;
}

bool read_command(Reader& r, const Caps& caps, ReadPayloads<Path>& paths, ReadPayloads<Filters>& filters, Command& c) {
    uint8_t kind = 0;
    uint32_t mask = 0;
    if (!r.u8(kind) || kind > int(Cmd::Backdrop) || !r.u32(mask) || mask >> COMMAND_BITS) return false;
    c.kind = Cmd(kind);
    if (has(mask, CMD_RECT) && !r.rect(c.rect)) return false;
    if (has(mask, CMD_RADII) && !r.radii(c.radii)) return false;
    if (has(mask, CMD_CLIP) && !r.rect(c.clip)) return false;
    if (has(mask, CMD_HAS_CLIP)) c.has_clip = true;
    if (has(mask, CMD_COLOR) && !r.color(c.color)) return false;
    if (has(mask, CMD_WIDTH) && !r.f32(c.width)) return false;
    if (has(mask, CMD_FONT) && !r.i32(c.font)) return false;
    if (has(mask, CMD_TEXT) && !r.str(c.text, caps.text)) return false;
    if (has(mask, CMD_X) && !r.f32(c.x)) return false;
    if (has(mask, CMD_Y) && !r.f32(c.y)) return false;
    if (has(mask, CMD_IMAGE) && !r.i32(c.image)) return false;
    if (has(mask, CMD_CANVAS) && !r.i32(c.canvas)) return false;
    if (has(mask, CMD_AX) && !r.f32(c.ax)) return false;
    if (has(mask, CMD_AY) && !r.f32(c.ay)) return false;
    if (has(mask, CMD_BX) && !r.f32(c.bx)) return false;
    if (has(mask, CMD_BY) && !r.f32(c.by)) return false;
    if (has(mask, CMD_STOPS) && !r.stops(c.stops, caps.stops)) return false;
    if (has(mask, CMD_PATH) && !paths.get(r, v1().path, [&](Path& p) { return read_path(r, caps, p); }, c.path)) return false;
    if (has(mask, CMD_FILTER) && !filters.get(r, Filters(), [&](Filters& f) { return read_filters(r, caps, f); }, c.filter))
        return false;
    if (has(mask, CMD_OPACITY) && !r.f32(c.opacity)) return false;
    if (has(mask, CMD_MATRIX) && !r.f32s(c.matrix)) return false;
    if (has(mask, CMD_HIDDEN)) c.hidden = true;
    if (has(mask, CMD_INSET)) c.inset = true;
    if (has(mask, CMD_KEY) && !r.u64(c.key)) return false;
    return true;
}

/// Whether `p` differs from version 1's paint, so that its op carries it.
bool painted(const CanvasPaint& p) {
    const CanvasPaint& d = v1().paint;
    return p.kind != d.kind || !same(p.color, d.color) || !same(p.args, d.args) || !p.stops.empty();
}

bool write_paint(Writer& w, const Caps& caps, const CanvasPaint& p) {
    if (p.kind > CanvasPaint::Conic) return false;
    const CanvasPaint& d = v1().paint;
    w.u8(p.kind);
    Mask<uint8_t> m(w);
    if (m.set(PAINT_COLOR, !same(p.color, d.color))) w.color(p.color);
    if (m.set(PAINT_ARGS, !same(p.args, d.args))) w.f32s(p.args);
    if (m.set(PAINT_STOPS, !p.stops.empty()) && !w.stops(p.stops, caps.stops)) return false;
    m.finish();
    return true;
}

bool read_paint(Reader& r, const Caps& caps, CanvasPaint& p) {
    uint8_t kind = 0, mask = 0;
    if (!r.u8(kind) || kind > CanvasPaint::Conic || !r.u8(mask) || mask >> PAINT_BITS) return false;
    p.kind = CanvasPaint::Kind(kind);
    if (has(mask, PAINT_COLOR) && !r.color(p.color)) return false;
    if (has(mask, PAINT_ARGS) && !r.f32s(p.args)) return false;
    if (has(mask, PAINT_STOPS) && !r.stops(p.stops, caps.stops)) return false;
    return true;
}

bool write_clips(Writer& w, const Caps& caps, const Clips& list) {
    if (!w.count(list.size(), caps.clips)) return false;
    for (const CanvasClip& c : list) {
        if (!w.geometry(c.path, caps)) return false;
        w.flag(c.evenodd);
    }
    return true;
}

bool read_clips(Reader& r, const Caps& caps, Clips& list) {
    size_t n = 0;
    if (!r.count(n, caps.clips, CLIP_BYTES)) return false;
    list.reserve(n);
    for (size_t i = 0; i < n; i++) {
        CanvasClip& c = list.emplace_back();
        if (!r.geometry(c.path, caps) || !r.flag(c.evenodd)) return false;
    }
    return true;
}

bool write_op(Writer& w, const Caps& caps, WrittenPayloads<Clips>& clips, WrittenPayloads<Pixels>& pixels, const CanvasOp& o) {
    if (o.kind > CanvasOp::PutImage) return false;
    const CanvasOp& d = v1().op;
    w.u8(o.kind);
    Mask<uint64_t> m(w);
    if (m.set(OP_PATH, !o.path.ops.empty() || !o.path.points.empty()) && !w.geometry(o.path, caps)) return false;
    m.flag(OP_EVENODD, o.evenodd);
    if (m.set(OP_PAINT, painted(o.paint)) && !write_paint(w, caps, o.paint)) return false;
    if (m.set(OP_MATRIX, !same(o.matrix, d.matrix))) w.f32s(o.matrix);
    if (m.set(OP_LINE_WIDTH, !same(o.line_width, d.line_width))) w.f32(o.line_width);
    if (m.set(OP_LINE_CAP, o.line_cap != d.line_cap)) w.i32(o.line_cap);
    if (m.set(OP_LINE_JOIN, o.line_join != d.line_join)) w.i32(o.line_join);
    if (m.set(OP_MITER_LIMIT, !same(o.miter_limit, d.miter_limit))) w.f32(o.miter_limit);
    if (m.set(OP_DASHES, !o.dashes.empty()) && !w.floats(o.dashes, caps.floats)) return false;
    if (m.set(OP_DASH_OFFSET, !same(o.dash_offset, d.dash_offset))) w.f32(o.dash_offset);
    if (m.set(OP_ALPHA, !same(o.alpha, d.alpha))) w.f32(o.alpha);
    if (m.set(OP_COMPOSITE, o.composite != d.composite)) {
        if (o.composite > Composite::Luminosity) return false;
        w.u8(uint8_t(o.composite));
    }
    if (m.set(OP_SHADOW_COLOR, !same(o.shadow_color, d.shadow_color))) w.color(o.shadow_color);
    if (m.set(OP_SHADOW_BLUR, !same(o.shadow_blur, d.shadow_blur))) w.f32(o.shadow_blur);
    if (m.set(OP_SHADOW_X, !same(o.shadow_x, d.shadow_x))) w.f32(o.shadow_x);
    if (m.set(OP_SHADOW_Y, !same(o.shadow_y, d.shadow_y))) w.f32(o.shadow_y);
    if (m.set(OP_CLIPS, o.clips != nullptr)
        && !clips.put(w, o.clips, [&](const Clips& list) { return write_clips(w, caps, list); }))
        return false;
    if (m.set(OP_FONT, o.font != d.font)) w.i32(o.font);
    if (m.set(OP_TEXT, !o.text.empty()) && !w.str(o.text, caps.text)) return false;
    if (m.set(OP_X, !same(o.x, d.x))) w.f32(o.x);
    if (m.set(OP_Y, !same(o.y, d.y))) w.f32(o.y);
    if (m.set(OP_SCALE_X, !same(o.scale_x, d.scale_x))) w.f32(o.scale_x);
    if (m.set(OP_TEXT_DX, !same(o.text_dx, d.text_dx))) w.f32(o.text_dx);
    m.flag(OP_STROKE_TEXT, o.stroke_text);
    if (m.set(OP_IMAGE, o.image != d.image)) w.i32(o.image);
    if (m.set(OP_CANVAS, o.canvas != d.canvas)) w.i32(o.canvas);
    if (m.set(OP_SX, !same(o.sx, d.sx))) w.f32(o.sx);
    if (m.set(OP_SY, !same(o.sy, d.sy))) w.f32(o.sy);
    if (m.set(OP_SW, !same(o.sw, d.sw))) w.f32(o.sw);
    if (m.set(OP_SH, !same(o.sh, d.sh))) w.f32(o.sh);
    if (m.set(OP_DX, !same(o.dx, d.dx))) w.f32(o.dx);
    if (m.set(OP_DY, !same(o.dy, d.dy))) w.f32(o.dy);
    if (m.set(OP_DW, !same(o.dw, d.dw))) w.f32(o.dw);
    if (m.set(OP_DH, !same(o.dh, d.dh))) w.f32(o.dh);
    m.flag(OP_SMOOTHING, !o.smoothing);
    if (m.set(OP_RGBA, o.rgba != nullptr)
        && !pixels.put(w, o.rgba, [&](const Pixels& px) { return w.bytes(px, caps.pixels); }))
        return false;
    if (m.set(OP_W, o.w != d.w)) w.i32(o.w);
    if (m.set(OP_H, o.h != d.h)) w.i32(o.h);
    m.finish();
    return true;
}

bool read_op(Reader& r, const Caps& caps, ReadPayloads<Clips>& clips, ReadPayloads<Pixels>& pixels, CanvasOp& o) {
    uint8_t kind = 0;
    uint64_t mask = 0;
    if (!r.u8(kind) || kind > CanvasOp::PutImage || !r.u64(mask) || mask >> CANVAS_OP_BITS) return false;
    o.kind = CanvasOp::Kind(kind);
    if (has(mask, OP_PATH) && !r.geometry(o.path, caps)) return false;
    if (has(mask, OP_EVENODD)) o.evenodd = true;
    if (has(mask, OP_PAINT) && !read_paint(r, caps, o.paint)) return false;
    if (has(mask, OP_MATRIX) && !r.f32s(o.matrix)) return false;
    if (has(mask, OP_LINE_WIDTH) && !r.f32(o.line_width)) return false;
    if (has(mask, OP_LINE_CAP) && !r.i32(o.line_cap)) return false;
    if (has(mask, OP_LINE_JOIN) && !r.i32(o.line_join)) return false;
    if (has(mask, OP_MITER_LIMIT) && !r.f32(o.miter_limit)) return false;
    if (has(mask, OP_DASHES) && !r.floats(o.dashes, caps.floats)) return false;
    if (has(mask, OP_DASH_OFFSET) && !r.f32(o.dash_offset)) return false;
    if (has(mask, OP_ALPHA) && !r.f32(o.alpha)) return false;
    if (has(mask, OP_COMPOSITE)) {
        uint8_t composite = 0;
        if (!r.u8(composite) || composite > uint8_t(Composite::Luminosity)) return false;
        o.composite = Composite(composite);
    }
    if (has(mask, OP_SHADOW_COLOR) && !r.color(o.shadow_color)) return false;
    if (has(mask, OP_SHADOW_BLUR) && !r.f32(o.shadow_blur)) return false;
    if (has(mask, OP_SHADOW_X) && !r.f32(o.shadow_x)) return false;
    if (has(mask, OP_SHADOW_Y) && !r.f32(o.shadow_y)) return false;
    if (has(mask, OP_CLIPS) && !clips.get(r, Clips(), [&](Clips& list) { return read_clips(r, caps, list); }, o.clips)) return false;
    if (has(mask, OP_FONT) && !r.i32(o.font)) return false;
    if (has(mask, OP_TEXT) && !r.str(o.text, caps.text)) return false;
    if (has(mask, OP_X) && !r.f32(o.x)) return false;
    if (has(mask, OP_Y) && !r.f32(o.y)) return false;
    if (has(mask, OP_SCALE_X) && !r.f32(o.scale_x)) return false;
    if (has(mask, OP_TEXT_DX) && !r.f32(o.text_dx)) return false;
    if (has(mask, OP_STROKE_TEXT)) o.stroke_text = true;
    if (has(mask, OP_IMAGE) && !r.i32(o.image)) return false;
    if (has(mask, OP_CANVAS) && !r.i32(o.canvas)) return false;
    if (has(mask, OP_SX) && !r.f32(o.sx)) return false;
    if (has(mask, OP_SY) && !r.f32(o.sy)) return false;
    if (has(mask, OP_SW) && !r.f32(o.sw)) return false;
    if (has(mask, OP_SH) && !r.f32(o.sh)) return false;
    if (has(mask, OP_DX) && !r.f32(o.dx)) return false;
    if (has(mask, OP_DY) && !r.f32(o.dy)) return false;
    if (has(mask, OP_DW) && !r.f32(o.dw)) return false;
    if (has(mask, OP_DH) && !r.f32(o.dh)) return false;
    if (has(mask, OP_SMOOTHING)) o.smoothing = false;
    if (has(mask, OP_RGBA) && !pixels.get(r, Pixels(), [&](Pixels& px) { return r.bytes(px, caps.pixels); }, o.rgba)) return false;
    if (has(mask, OP_W) && !r.i32(o.w)) return false;
    if (has(mask, OP_H) && !r.i32(o.h)) return false;
    return true;
}

} // namespace

bool encode(const DisplayList& list, const Caps& caps, std::string& out) {
    return appended(out, [&](Writer& w) {
        w.head(Form::List);
        if (!w.count(list.size(), caps.commands)) return false;
        WrittenPayloads<Path> paths;
        WrittenPayloads<Filters> filters;
        for (const Command& c : list)
            if (!write_command(w, caps, paths, filters, c)) return false;
        return true;
    });
}

bool decode(std::string_view bytes, const Caps& caps, DisplayList& out) {
    Reader r(bytes);
    size_t n = 0;
    if (!r.head(Form::List) || !r.count(n, caps.commands, COMMAND_BYTES)) return false;
    DisplayList list;
    list.reserve(n);
    ReadPayloads<Path> paths;
    ReadPayloads<Filters> filters;
    for (size_t i = 0; i < n; i++)
        if (!read_command(r, caps, paths, filters, list.emplace_back(v1().command))) return false;
    if (!r.done()) return false;
    out = std::move(list);
    return true;
}

bool encode(const Damage& damage, const Caps& caps, std::string& out) {
    return appended(out, [&](Writer& w) {
        w.head(Form::Damage);
        w.flag(damage.has_value());
        if (!damage) return true;
        if (!w.count(damage->size(), caps.rects)) return false;
        for (const Rect& rect : *damage) w.rect(rect);
        return true;
    });
}

bool decode(std::string_view bytes, const Caps& caps, Damage& out) {
    Reader r(bytes);
    bool rects = false;
    if (!r.head(Form::Damage) || !r.flag(rects)) return false;
    Damage damage;
    if (rects) {
        size_t n = 0;
        if (!r.count(n, caps.rects, RECT_BYTES)) return false;
        std::vector<Rect>& list = damage.emplace();
        list.reserve(n);
        for (size_t i = 0; i < n; i++)
            if (!r.rect(list.emplace_back())) return false;
    }
    if (!r.done()) return false;
    out = std::move(damage);
    return true;
}

bool encode(const std::vector<CanvasOp>& ops, const Caps& caps, std::string& out) {
    return appended(out, [&](Writer& w) {
        w.head(Form::Batch);
        if (!w.count(ops.size(), caps.canvas_ops)) return false;
        WrittenPayloads<Clips> clips;
        WrittenPayloads<Pixels> pixels;
        for (const CanvasOp& o : ops)
            if (!write_op(w, caps, clips, pixels, o)) return false;
        return true;
    });
}

bool decode(std::string_view bytes, const Caps& caps, std::vector<CanvasOp>& out) {
    Reader r(bytes);
    size_t n = 0;
    if (!r.head(Form::Batch) || !r.count(n, caps.canvas_ops, CANVAS_OP_BYTES)) return false;
    std::vector<CanvasOp> ops;
    ops.reserve(n);
    ReadPayloads<Clips> clips;
    ReadPayloads<Pixels> pixels;
    for (size_t i = 0; i < n; i++)
        if (!read_op(r, caps, clips, pixels, ops.emplace_back(v1().op))) return false;
    if (!r.done()) return false;
    out = std::move(ops);
    return true;
}

void encode(const FontMetrics& metrics, std::string& out) {
    Writer w(out);
    w.head(Form::Metrics);
    w.f32(metrics.ascent);
    w.f32(metrics.descent);
    w.f32(metrics.height);
    w.f32(metrics.x_height);
    w.f32(metrics.zero_width);
    w.f32(metrics.em_ascent);
    w.f32(metrics.em_descent);
}

bool decode(std::string_view bytes, FontMetrics& out) {
    // Committed as bytes rather than as floats, whose copy could quiet a signalling NaN.
    static_assert(std::is_trivially_copyable_v<FontMetrics> && sizeof(FontMetrics) == 7 * sizeof(float));
    Reader r(bytes);
    FontMetrics m;
    if (!r.head(Form::Metrics) || !r.f32(m.ascent) || !r.f32(m.descent) || !r.f32(m.height) || !r.f32(m.x_height)
        || !r.f32(m.zero_width) || !r.f32(m.em_ascent) || !r.f32(m.em_descent) || !r.done())
        return false;
    std::memcpy(&out, &m, sizeof out);
    return true;
}

} // namespace twlite::wire
