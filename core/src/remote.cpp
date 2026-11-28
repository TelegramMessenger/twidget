// remote.h's messages: a Writer appends fields within the message cap, a Reader takes them from what is left, checking every length against its cap and against what is left before making room for it.
#include "remote.h"

#include "canvas.h"

#include <algorithm>
#include <climits>
#include <cstring>
#include <type_traits>
#include <utility>

namespace twlite::remote {
namespace {

static_assert(CHAR_BIT == 8 && sizeof(float) == 4 && sizeof(double) == 8);
static_assert(std::numeric_limits<float>::is_iec559 && std::numeric_limits<double>::is_iec559);

/// The fewest bytes a str takes: its length.
constexpr size_t STR_BYTES = 4;
/// A WidthsRequest's header and count, and the fewest bytes one of its texts takes: its font and its length.
constexpr size_t WIDTHS_HEAD = 2 + 4, WIDTHS_ITEM_BYTES = 4 + STR_BYTES;

/// A painted Frame's bytes before its list: header, seq, state and the flag.
constexpr size_t STATE_BYTES = 8 + 4 + 1 + 1 + 8 + 1, FRAME_HEAD = 2 + 8 + STATE_BYTES + 1;
/// A FrameDelta run's tag, and the fewest bytes a run takes: a literal of one byte.
constexpr uint8_t COPY_RUN = 0, LITERAL_RUN = 1;
constexpr size_t RUN_BYTES = 1 + 4 + 1;

constexpr std::string_view PLATFORM_TYPES[] = {"size", "gesture", "limit", "violation", "bubble"};

template <class F, class U> U bits_of(const F& f) {
    static_assert(sizeof(F) == sizeof(U));
    U b = 0;
    std::memcpy(&b, &f, sizeof b);
    return b;
}

int32_t to_int(uint32_t u) {
    return u <= uint32_t(INT32_MAX) ? int32_t(u) : int32_t(u - uint32_t(INT32_MAX) - 1) - INT32_MAX - 1;
}

bool known(uint8_t kind) {
    return (kind >= 1 && kind <= 8) || (kind >= 32 && kind <= 36) || (kind >= 64 && kind <= 73) || (kind >= 96 && kind <= 100);
}

std::optional<uint32_t> pixel_bytes(int32_t w, int32_t h) {
    if (w < 0 || h < 0 || w > Canvas::MAX_SIDE || h > Canvas::MAX_SIDE || int64_t(w) * h > Canvas::MAX_PIXELS) return std::nullopt;
    return uint32_t(uint64_t(w) * uint64_t(h) * 4);
}

/// How many of `texts`, from `from` on, keep within the widths caps in order: their count, each text and all of them, and, when `sized`, the message cap as a WidthsRequest's, which bounds its WidthsAnswer too.
size_t widths_within(const std::vector<TextInFont>& texts, size_t from, const Caps& caps, bool sized) {
    const size_t most = std::min<size_t>(caps.widths, UINT32_MAX), longest = std::min<size_t>(caps.text, UINT32_MAX);
    size_t bytes = WIDTHS_HEAD, text = 0, n = 0;
    for (size_t i = from; i < texts.size() && n < most; i++, n++) {
        const size_t size = texts[i].text.size();
        if (size > longest || size > caps.widths_text - text) break;
        if (sized && (caps.message < bytes || caps.message - bytes < WIDTHS_ITEM_BYTES || size > caps.message - bytes - WIDTHS_ITEM_BYTES)) break;
        bytes += WIDTHS_ITEM_BYTES + size;
        text += size;
    }
    return n;
}

bool widths_fit(const WidthsRequest& request, const Caps& caps) {
    return widths_within(request.texts, 0, caps, false) == request.texts.size();
}

class Writer {
public:
    Writer(std::string& out, size_t limit) : out_(out), start_(out.size()), limit_(limit) {}

    size_t written() const { return out_.size() - start_; }
    template <class U> void put(U v) {
        for (size_t i = 0; i < sizeof(U); i++) out_.push_back(char(uint8_t(v >> (8 * i))));
    }
    void u8(uint8_t v) { put(v); }
    void u32(uint32_t v) { put(v); }
    void u64(uint64_t v) { put(v); }
    void i32(int32_t v) { put(uint32_t(v)); }
    void f32(const float& v) { put(bits_of<float, uint32_t>(v)); }
    void f64(const double& v) { put(bits_of<double, uint64_t>(v)); }
    void flag(bool v) { u8(v ? 1 : 0); }
    void head(Kind kind) {
        u8(uint8_t(kind));
        u8(VERSION);
    }
    bool count(size_t n, size_t cap) {
        if (n > std::min<size_t>(cap, UINT32_MAX)) return false;
        u32(uint32_t(n));
        return true;
    }
    bool str(const std::string& s, size_t cap) {
        if (!room(s.size()) || !count(s.size(), cap)) return false;
        out_.append(s);
        return true;
    }
    bool bytes(const std::vector<uint8_t>& v, size_t cap) {
        if (!room(v.size()) || !count(v.size(), cap)) return false;
        if (!v.empty()) out_.append(reinterpret_cast<const char*>(v.data()), v.size());
        return true;
    }
    bool floats(const std::vector<float>& v, size_t cap) {
        if (v.size() > SIZE_MAX / sizeof(float) || !room(v.size() * sizeof(float)) || !count(v.size(), cap)) return false;
        for (const float& f : v) f32(f);
        return true;
    }
    template <class Write> bool value(Write write) {
        const size_t at = out_.size();
        u32(0);
        if (!write(out_)) return false;
        const size_t n = out_.size() - at - 4;
        if (n > UINT32_MAX) return false;
        for (size_t i = 0; i < 4; i++) out_[at + i] = char(uint8_t(n >> (8 * i)));
        return true;
    }

private:
    /// Whether a u32 length and `n` bytes after it keep the message within its cap.
    bool room(size_t n) const {
        const size_t used = written();
        return used <= limit_ && STR_BYTES <= limit_ - used && n <= limit_ - used - STR_BYTES;
    }

    std::string& out_;
    size_t start_ = 0;
    size_t limit_ = 0;
};

class Reader {
public:
    explicit Reader(std::string_view bytes) : rest_(bytes) {}

    bool done() const { return rest_.empty(); }
    bool u8(uint8_t& v) { return little(v); }
    bool u32(uint32_t& v) { return little(v); }
    bool u64(uint64_t& v) { return little(v); }
    bool i32(int32_t& v) {
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
    bool f64(double& v) {
        uint64_t b = 0;
        if (!u64(b)) return false;
        std::memcpy(&v, &b, sizeof v);
        return true;
    }
    bool flag(bool& v) {
        uint8_t b = 0;
        if (!u8(b) || b > 1) return false;
        v = b == 1;
        return true;
    }
    bool head(Kind kind) {
        uint8_t k = 0, v = 0;
        return u8(k) && k == uint8_t(kind) && u8(v) && v == VERSION;
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
    bool bytes(std::vector<uint8_t>& v, size_t n) {
        if (n > rest_.size()) return false;
        if (n) {
            v.assign(rest_.data(), rest_.data() + n);
            rest_.remove_prefix(n);
        }
        return true;
    }
    bool view(std::string_view& v, size_t n) {
        if (n > rest_.size()) return false;
        v = rest_.substr(0, n);
        rest_.remove_prefix(n);
        return true;
    }
    template <class Read> bool value(Read read) {
        uint32_t n = 0;
        if (!u32(n) || n > rest_.size() || !read(rest_.substr(0, n))) return false;
        rest_.remove_prefix(n);
        return true;
    }

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

/// Runs `write` after the header, cutting `out` back to its size on entry when it refuses or the message passes its cap.
template <class Write> bool appended(std::string& out, const Caps& caps, Kind kind, Write write) {
    const size_t start = out.size();
    Writer w(out, caps.message);
    w.head(kind);
    if (write(w) && w.written() <= caps.message) return true;
    out.resize(start);
    return false;
}

/// Copies a value without passing its floats through a float, whose copy could quiet a signalling NaN.
template <class T> void commit(T& out, T& value) {
    if constexpr (std::is_trivially_copyable_v<T>) {
        std::memcpy(&out, &value, sizeof out);
    } else {
        out = std::move(value);
    }
}

void commit(Frame& out, Frame& value) {
    out.seq = value.seq;
    commit(out.state, value.state);
    out.painted = value.painted;
    out.list = std::move(value.list);
    out.damage = std::move(value.damage);
}

void commit(FrameDelta& out, FrameDelta& value) {
    out.seq = value.seq;
    commit(out.state, value.state);
    out.base = value.base;
    out.runs = std::move(value.runs);
    out.damage = std::move(value.damage);
}

/// Replaces `out` with what `read` takes from all of `message` after its header, once the last byte has passed.
template <class T, class Read> bool taken(std::string_view message, const Caps& caps, Kind kind, T& out, Read read) {
    if (message.size() > caps.message) return false;
    Reader r(message);
    T value;
    if (!r.head(kind) || !read(r, value) || !r.done()) return false;
    commit(out, value);
    return true;
}

void put_state(Writer& w, const State& s) {
    w.f64(s.width);
    w.i32(s.height);
    w.flag(s.dirty);
    w.flag(s.wants_tick);
    w.f64(s.next_tick_in);
    w.flag(s.frame_asked);
}

bool read_state(Reader& r, State& s) {
    return r.f64(s.width) && r.i32(s.height) && r.flag(s.dirty) && r.flag(s.wants_tick) && r.f64(s.next_tick_in) && r.flag(s.frame_asked);
}

void put_claim(Writer& w, const twidget::WidgetGestureClaim& c) {
    w.flag(c.x);
    w.flag(c.y);
    w.flag(c.hold);
}

bool read_claim(Reader& r, twidget::WidgetGestureClaim& c) {
    return r.flag(c.x) && r.flag(c.y) && r.flag(c.hold);
}

bool put_id(std::string& out, const Caps& caps, Kind kind, int32_t id) {
    return appended(out, caps, kind, [&](Writer& w) {
        w.i32(id);
        return true;
    });
}

template <class T> bool take_id(std::string_view message, const Caps& caps, Kind kind, T& out, int32_t T::*id) {
    return taken(message, caps, kind, out, [&](Reader& r, T& m) { return r.i32(m.*id); });
}

/// Whether the painted Frame of a list and a damage of these lengths keeps within the frame and the message caps.
bool fits_frame(const Caps& caps, uint64_t list, uint64_t damage) {
    const uint64_t bytes = FRAME_HEAD + 4 + list + 4 + damage;
    return bytes <= caps.frame && bytes <= caps.message;
}

/// A Frame after its header; for a painted one, `list` where its list went, starts noted within `most` bytes, when given.
bool put_frame(Writer& w, const Frame& message, const Caps& caps, size_t most, WrittenList* list) {
    w.u64(message.seq);
    put_state(w, message.state);
    w.flag(message.painted);
    if (!message.painted) return message.list.empty() && !message.damage && w.written() <= caps.frame;
    WrittenList written;
    size_t damage = 0;
    const bool put = w.value([&](std::string& s) {
        written.at = s.size();
        if (!(list ? wire::encode(message.list, caps.values, s, written.starts, most) : wire::encode(message.list, caps.values, s))) return false;
        written.size = s.size() - written.at;
        return true;
    }) && w.value([&](std::string& s) {
        const size_t at = s.size();
        if (!wire::encode(message.damage, caps.values, s)) return false;
        damage = s.size() - at;
        return true;
    }) && fits_frame(caps, written.size, damage);
    if (put && list) *list = std::move(written);
    return put;
}

/// One run for the list's head and one for each command: what an encoder that copies whole commands may need.
size_t most_runs(const Caps& caps) {
    return std::min<size_t>(caps.values.commands, UINT32_MAX - 1) + 1;
}

/// A FrameDelta's fields before its runs.
struct DeltaHead {
    uint64_t seq = 0;
    State state;
    uint64_t base = 0;
    uint32_t length = 0;
    size_t runs = 0;
};

bool read_delta_head(Reader& r, const Caps& caps, DeltaHead& h) {
    return r.u64(h.seq) && read_state(r, h.state) && r.u64(h.base) && r.u32(h.length) && h.length > 0 && h.length <= caps.delta_base
        && r.count(h.runs, std::min<size_t>(most_runs(caps), h.length), RUN_BYTES);
}

/// Takes runs that rebuild exactly `length` bytes, each copy within a base of `base` bytes and all of them copying no more than it holds, and gives each to `run(offset, length, literal)`, the literal empty for a copy.
template <class Run> bool read_runs(Reader& r, size_t count, uint32_t length, size_t base, Run run) {
    size_t built = 0, copied = 0;
    for (size_t i = 0; i < count; i++) {
        uint8_t tag = 0;
        uint32_t offset = 0, n = 0;
        std::string_view literal;
        if (!r.u8(tag)) return false;
        if (tag == COPY_RUN) {
            if (!r.u32(offset) || !r.u32(n) || offset > base || n > base - offset || n > base - copied) return false;
            copied += n;
        } else if (tag != LITERAL_RUN || !r.u32(n) || !r.view(literal, n)) {
            return false;
        }
        if (n == 0 || n > length - built) return false;
        built += n;
        run(offset, n, literal);
    }
    return built == length;
}

/// A FrameDelta's runs on a base of `base` bytes and its damage, read as views up to its last byte, and the Frame it stands for within the caps; nothing made yet, `damage` viewing the damage's bytes.
bool scan_delta(Reader& r, const Caps& caps, const DeltaHead& h, size_t base, std::string_view& damage) {
    const auto seen = [&](std::string_view bytes) {
        damage = bytes;
        return true;
    };
    return read_runs(r, h.runs, h.length, base, [](uint32_t, uint32_t, std::string_view) {}) && r.value(seen) && r.done() && fits_frame(caps, h.length, damage.size());
}

} // namespace

std::optional<Kind> kind_of(std::string_view message) {
    if (message.size() < 2 || uint8_t(message[1]) != VERSION || !known(uint8_t(message[0]))) return std::nullopt;
    return Kind(uint8_t(message[0]));
}

bool is_platform_message(std::string_view type) {
    return std::find(std::begin(PLATFORM_TYPES), std::end(PLATFORM_TYPES), type) != std::end(PLATFORM_TYPES);
}

bool encode(const FrameRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::FrameRequest, [&](Writer& w) {
        if (uint8_t(message.paint) > uint8_t(Paint::Force)) return false;
        w.u64(message.seq);
        w.flag(message.tick);
        w.u8(uint8_t(message.paint));
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, FrameRequest& out) {
    return taken(message, caps, Kind::FrameRequest, out, [](Reader& r, FrameRequest& m) {
        uint8_t paint = 0;
        if (!r.u64(m.seq) || !r.flag(m.tick) || !r.u8(paint) || paint > uint8_t(Paint::Force)) return false;
        m.paint = Paint(paint);
        return true;
    });
}

bool encode(const SetWidth& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::SetWidth, [&](Writer& w) {
        w.f64(message.width);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, SetWidth& out) {
    return taken(message, caps, Kind::SetWidth, out, [](Reader& r, SetWidth& m) { return r.f64(m.width); });
}

bool encode(const SetMaxHeight& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::SetMaxHeight, [&](Writer& w) {
        w.f64(message.height);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, SetMaxHeight& out) {
    return taken(message, caps, Kind::SetMaxHeight, out, [](Reader& r, SetMaxHeight& m) { return r.f64(m.height); });
}

bool encode(const SetTheme& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::SetTheme, [&](Writer& w) {
        w.flag(message.dark);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, SetTheme& out) {
    return taken(message, caps, Kind::SetTheme, out, [](Reader& r, SetTheme& m) { return r.flag(m.dark); });
}

bool encode(const Touch& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Touch, [&](Writer& w) {
        w.i32(message.action);
        w.f64(message.x);
        w.f64(message.y);
        w.i32(message.pointer);
        w.flag(message.primary);
        w.flag(message.mouse);
        w.i32(message.modifiers);
        w.i32(message.button);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, Touch& out) {
    return taken(message, caps, Kind::Touch, out, [](Reader& r, Touch& m) {
        return r.i32(m.action) && r.f64(m.x) && r.f64(m.y) && r.i32(m.pointer) && r.flag(m.primary) && r.flag(m.mouse) && r.i32(m.modifiers)
            && r.i32(m.button);
    });
}

bool encode(const Hover& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Hover, [&](Writer& w) {
        w.i32(message.action);
        w.f64(message.x);
        w.f64(message.y);
        w.i32(message.modifiers);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, Hover& out) {
    return taken(message, caps, Kind::Hover, out, [](Reader& r, Hover& m) { return r.i32(m.action) && r.f64(m.x) && r.f64(m.y) && r.i32(m.modifiers); });
}

bool encode(const ClearSelection&, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::ClearSelection, [](Writer&) { return true; });
}

bool decode(std::string_view message, const Caps& caps, ClearSelection& out) {
    return taken(message, caps, Kind::ClearSelection, out, [](Reader&, ClearSelection&) { return true; });
}

bool encode(const ClaimQuery& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::ClaimQuery, [&](Writer& w) {
        w.u64(message.seq);
        w.f64(message.x);
        w.f64(message.y);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, ClaimQuery& out) {
    return taken(message, caps, Kind::ClaimQuery, out, [](Reader& r, ClaimQuery& m) { return r.u64(m.seq) && r.f64(m.x) && r.f64(m.y); });
}

bool encode(const FontAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::FontAnswer, [&](Writer& w) {
        w.i32(message.font);
        return w.value([&](std::string& s) {
            wire::encode(message.metrics, s);
            return true;
        });
    });
}

bool decode(std::string_view message, const Caps& caps, FontAnswer& out) {
    return taken(message, caps, Kind::FontAnswer, out, [](Reader& r, FontAnswer& m) {
        return r.i32(m.font) && r.value([&](std::string_view bytes) { return wire::decode(bytes, m.metrics); });
    });
}

bool encode(const WidthAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::WidthAnswer, [&](Writer& w) {
        w.f32(message.width);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, WidthAnswer& out) {
    return taken(message, caps, Kind::WidthAnswer, out, [](Reader& r, WidthAnswer& m) { return r.f32(m.width); });
}

bool encode(const ImageAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::ImageAnswer, [&](Writer& w) {
        w.flag(message.ok);
        w.f32(message.w);
        w.f32(message.h);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, ImageAnswer& out) {
    return taken(message, caps, Kind::ImageAnswer, out, [](Reader& r, ImageAnswer& m) { return r.flag(m.ok) && r.f32(m.w) && r.f32(m.h); });
}

bool encode(const PixelsAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::PixelsAnswer, [&](Writer& w) {
        w.flag(message.ok);
        if (!message.ok) return message.rgba.empty();
        return w.bytes(message.rgba, caps.values.pixels);
    });
}

bool decode(std::string_view message, const Caps& caps, const PixelsRequest& asked, PixelsAnswer& out) {
    const std::optional<uint32_t> expected = pixel_bytes(asked.w, asked.h);
    return taken(message, caps, Kind::PixelsAnswer, out, [&](Reader& r, PixelsAnswer& m) {
        if (!expected || !r.flag(m.ok)) return false;
        if (!m.ok) return true;
        uint32_t n = 0;
        return r.u32(n) && n == *expected && n <= caps.values.pixels && r.bytes(m.rgba, n);
    });
}

bool encode(const WidthsAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::WidthsAnswer, [&](Writer& w) { return w.floats(message.widths, caps.widths); });
}

bool decode(std::string_view message, const Caps& caps, const WidthsRequest& asked, WidthsAnswer& out) {
    const bool fits = widths_fit(asked, caps);
    return taken(message, caps, Kind::WidthsAnswer, out, [&](Reader& r, WidthsAnswer& m) {
        size_t n = 0;
        if (!fits || !r.count(n, caps.widths, sizeof(float)) || n != asked.texts.size()) return false;
        m.widths.resize(n);
        for (float& width : m.widths)
            if (!r.f32(width)) return false;
        return true;
    });
}

bool encode(const Frame& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Frame, [&](Writer& w) { return put_frame(w, message, caps, 0, nullptr); });
}

bool encode(const Frame& message, const Caps& caps, std::string& out, size_t most, WrittenList& list) {
    WrittenList written;
    if (!appended(out, caps, Kind::Frame, [&](Writer& w) { return put_frame(w, message, caps, most, &written); })) return false;
    list = std::move(written);
    return true;
}

bool decode(std::string_view message, const Caps& caps, Frame& out) {
    std::string_view list;
    return decode(message, caps, out, list);
}

bool decode(std::string_view message, const Caps& caps, Frame& out, std::string_view& list) {
    std::string_view painted;
    const bool taken_whole = message.size() <= caps.frame && taken(message, caps, Kind::Frame, out, [&](Reader& r, Frame& m) {
        if (!r.u64(m.seq) || !read_state(r, m.state) || !r.flag(m.painted)) return false;
        if (!m.painted) return true;
        return r.value([&](std::string_view bytes) {
            painted = bytes;
            return wire::decode(bytes, caps.values, m.list);
        }) && r.value([&](std::string_view bytes) { return wire::decode(bytes, caps.values, m.damage); });
    });
    if (taken_whole) list = painted;
    return taken_whole;
}

bool encode(const FrameDelta& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::FrameDelta, [&](Writer& w) {
        uint64_t length = 0, copied = 0;
        for (const DeltaRun& run : message.runs) {
            if (!run.literal.empty()) {
                if (run.offset || run.length) return false;
                length += run.literal.size();
            } else {
                if (run.length == 0 || run.offset > caps.delta_base || run.length > caps.delta_base - run.offset) return false;
                copied += run.length;
                length += run.length;
            }
        }
        if (message.runs.empty() || message.runs.size() > most_runs(caps) || length > caps.delta_base || length > UINT32_MAX || copied > caps.delta_base) return false;
        w.u64(message.seq);
        put_state(w, message.state);
        w.u64(message.base);
        w.u32(uint32_t(length));
        w.u32(uint32_t(message.runs.size()));
        for (const DeltaRun& run : message.runs) {
            if (!run.literal.empty()) {
                w.u8(LITERAL_RUN);
                if (!w.str(run.literal, caps.delta_base)) return false;
                continue;
            }
            w.u8(COPY_RUN);
            w.u32(run.offset);
            w.u32(run.length);
        }
        const size_t before = w.written();
        return w.value([&](std::string& s) { return wire::encode(message.damage, caps.values, s); }) && w.written() <= caps.frame
            && fits_frame(caps, length, w.written() - before - 4);
    });
}

bool decode(std::string_view message, const Caps& caps, FrameDelta& out) {
    return message.size() <= caps.frame && taken(message, caps, Kind::FrameDelta, out, [&](Reader& r, FrameDelta& m) {
        DeltaHead h;
        std::string_view damage;
        if (!read_delta_head(r, caps, h)) return false;
        Reader runs = r;
        if (!scan_delta(r, caps, h, caps.delta_base, damage)) return false;
        m.seq = h.seq;
        commit(m.state, h.state);
        m.base = h.base;
        m.runs.reserve(h.runs);
        read_runs(runs, h.runs, h.length, caps.delta_base, [&](uint32_t offset, uint32_t n, std::string_view literal) {
            if (literal.empty()) {
                m.runs.push_back({offset, n, {}});
            } else {
                m.runs.push_back({0, 0, std::string(literal)});
            }
        });
        return wire::decode(damage, caps.values, m.damage);
    });
}

bool delta_ids(std::string_view message, uint64_t& seq, uint64_t& base) {
    Reader r(message);
    uint64_t s = 0, b = 0;
    State state;
    if (!r.head(Kind::FrameDelta) || !r.u64(s) || !read_state(r, state) || !r.u64(b)) return false;
    seq = s;
    base = b;
    return true;
}

bool delta_scan(std::string_view message, const Caps& caps, uint64_t& seq, uint32_t& length, std::string_view& damage) {
    if (message.size() > caps.message || message.size() > caps.frame) return false;
    Reader r(message);
    DeltaHead h;
    std::string_view value;
    if (!r.head(Kind::FrameDelta) || !read_delta_head(r, caps, h) || !scan_delta(r, caps, h, caps.delta_base, value)) return false;
    seq = h.seq;
    length = h.length;
    damage = value;
    return true;
}

bool rebuild(std::string_view message, const Caps& caps, uint64_t base_seq, std::string_view base, Frame& out, std::string& list) {
    if (message.size() > caps.message || message.size() > caps.frame || base.size() > caps.delta_base) return false;
    Reader r(message);
    DeltaHead h;
    std::string_view damage;
    if (!r.head(Kind::FrameDelta) || !read_delta_head(r, caps, h) || h.base != base_seq) return false;
    Reader runs = r;
    if (!scan_delta(r, caps, h, base.size(), damage)) return false;
    Frame frame;
    if (!wire::decode(damage, caps.values, frame.damage)) return false;
    std::string bytes;
    bytes.reserve(h.length);
    read_runs(runs, h.runs, h.length, base.size(), [&](uint32_t offset, uint32_t n, std::string_view literal) {
        bytes.append(literal.empty() ? base.substr(offset, n) : literal);
    });
    if (!wire::decode(bytes, caps.values, frame.list)) return false;
    frame.seq = h.seq;
    commit(frame.state, h.state);
    frame.painted = true;
    commit(out, frame);
    list = std::move(bytes);
    return true;
}

bool encode(const State& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::State, [&](Writer& w) {
        put_state(w, message);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, State& out) {
    return taken(message, caps, Kind::State, out, read_state);
}

bool encode(const Messages& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Messages, [&](Writer& w) {
        if (!w.count(message.texts.size(), caps.messages)) return false;
        for (const std::string& text : message.texts)
            if (!w.str(text, caps.text)) return false;
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, Messages& out) {
    return taken(message, caps, Kind::Messages, out, [&](Reader& r, Messages& m) {
        size_t n = 0;
        if (!r.count(n, caps.messages, STR_BYTES)) return false;
        m.texts.reserve(n);
        for (size_t i = 0; i < n; i++)
            if (!r.str(m.texts.emplace_back(), caps.text)) return false;
        return true;
    });
}

bool encode(const Pointer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Pointer, [&](Writer& w) {
        if (!w.str(message.cursor, caps.text)) return false;
        w.flag(message.claim.has_value());
        if (message.claim) put_claim(w, *message.claim);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, Pointer& out) {
    return taken(message, caps, Kind::Pointer, out, [&](Reader& r, Pointer& m) {
        bool claimed = false;
        if (!r.str(m.cursor, caps.text) || !r.flag(claimed)) return false;
        return !claimed || read_claim(r, m.claim.emplace());
    });
}

bool encode(const Selection& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::Selection, [&](Writer& w) { return w.str(message.text, caps.text); });
}

bool decode(std::string_view message, const Caps& caps, Selection& out) {
    return taken(message, caps, Kind::Selection, out, [&](Reader& r, Selection& m) { return r.str(m.text, caps.text); });
}

bool encode(const ClaimAnswer& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::ClaimAnswer, [&](Writer& w) {
        w.u64(message.seq);
        put_claim(w, message.claim);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, ClaimAnswer& out) {
    return taken(message, caps, Kind::ClaimAnswer, out, [](Reader& r, ClaimAnswer& m) { return r.u64(m.seq) && read_claim(r, m.claim); });
}

bool encode(const FontRelease& message, const Caps& caps, std::string& out) {
    return put_id(out, caps, Kind::FontRelease, message.font);
}

bool decode(std::string_view message, const Caps& caps, FontRelease& out) {
    return take_id(message, caps, Kind::FontRelease, out, &FontRelease::font);
}

bool encode_canvas_run(int32_t id, int32_t w, int32_t h, const std::vector<CanvasOp>& ops, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::CanvasRun, [&](Writer& writer) {
        writer.i32(id);
        writer.i32(w);
        writer.i32(h);
        return writer.value([&](std::string& s) { return wire::encode(ops, caps.values, s); });
    });
}

bool encode(const CanvasRun& message, const Caps& caps, std::string& out) {
    return encode_canvas_run(message.id, message.w, message.h, message.ops, caps, out);
}

bool decode(std::string_view message, const Caps& caps, CanvasRun& out) {
    return taken(message, caps, Kind::CanvasRun, out, [&](Reader& r, CanvasRun& m) {
        return r.i32(m.id) && r.i32(m.w) && r.i32(m.h) && r.value([&](std::string_view bytes) { return wire::decode(bytes, caps.values, m.ops); });
    });
}

bool encode(const CanvasRelease& message, const Caps& caps, std::string& out) {
    return put_id(out, caps, Kind::CanvasRelease, message.id);
}

bool decode(std::string_view message, const Caps& caps, CanvasRelease& out) {
    return take_id(message, caps, Kind::CanvasRelease, out, &CanvasRelease::id);
}

bool encode(const FontRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::FontRequest, [&](Writer& w) {
        w.flag(message.canvas);
        if (!w.str(message.families, caps.text)) return false;
        w.f32(message.size);
        w.i32(message.weight);
        w.flag(message.italic);
        return message.canvas ? message.features.empty() : w.str(message.features, caps.text);
    });
}

bool decode(std::string_view message, const Caps& caps, FontRequest& out) {
    return taken(message, caps, Kind::FontRequest, out, [&](Reader& r, FontRequest& m) {
        if (!r.flag(m.canvas) || !r.str(m.families, caps.text) || !r.f32(m.size) || !r.i32(m.weight) || !r.flag(m.italic)) return false;
        return m.canvas || r.str(m.features, caps.text);
    });
}

bool encode(const WidthRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::WidthRequest, [&](Writer& w) {
        w.i32(message.font);
        return w.str(message.text, caps.text);
    });
}

bool decode(std::string_view message, const Caps& caps, WidthRequest& out) {
    return taken(message, caps, Kind::WidthRequest, out, [&](Reader& r, WidthRequest& m) { return r.i32(m.font) && r.str(m.text, caps.text); });
}

bool encode(const ImageRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::ImageRequest, [&](Writer& w) {
        w.i32(message.id);
        return w.str(message.url, caps.url);
    });
}

bool decode(std::string_view message, const Caps& caps, ImageRequest& out) {
    return taken(message, caps, Kind::ImageRequest, out, [&](Reader& r, ImageRequest& m) { return r.i32(m.id) && r.str(m.url, caps.url); });
}

bool encode(const PixelsRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::PixelsRequest, [&](Writer& w) {
        if (!pixel_bytes(message.w, message.h)) return false;
        w.i32(message.id);
        w.i32(message.x);
        w.i32(message.y);
        w.i32(message.w);
        w.i32(message.h);
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, PixelsRequest& out) {
    return taken(message, caps, Kind::PixelsRequest, out, [](Reader& r, PixelsRequest& m) {
        return r.i32(m.id) && r.i32(m.x) && r.i32(m.y) && r.i32(m.w) && r.i32(m.h) && pixel_bytes(m.w, m.h).has_value();
    });
}

size_t widths_in_request(const std::vector<TextInFont>& texts, size_t from, const Caps& caps) {
    return widths_within(texts, from, caps, true);
}

bool encode(const WidthsRequest& message, const Caps& caps, std::string& out) {
    return appended(out, caps, Kind::WidthsRequest, [&](Writer& w) {
        if (widths_in_request(message.texts, 0, caps) != message.texts.size() || !w.count(message.texts.size(), caps.widths)) return false;
        for (const TextInFont& t : message.texts) {
            w.i32(t.font);
            if (!w.str(t.text, caps.text)) return false;
        }
        return true;
    });
}

bool decode(std::string_view message, const Caps& caps, WidthsRequest& out) {
    return taken(message, caps, Kind::WidthsRequest, out, [&](Reader& r, WidthsRequest& m) {
        size_t n = 0;
        if (!r.count(n, caps.widths, WIDTHS_ITEM_BYTES)) return false;
        m.texts.reserve(n);
        size_t left = caps.widths_text;
        for (size_t i = 0; i < n; i++) {
            TextInFont& t = m.texts.emplace_back();
            if (!r.i32(t.font) || !r.str(t.text, std::min(caps.text, left))) return false;
            left -= t.text.size();
        }
        return true;
    });
}

} // namespace twlite::remote
