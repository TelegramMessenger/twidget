// A renderer used directly, recorded call by call in its process's order for a replay without the source: what each non-const call was given, the Backend calls and clock reads it made and what it gave back, in a strict binary file that ends with its own counts.
#pragma once

#include "log_backend.h"
#include "remote.h"
#include "twidget/json.h"
#include "wire.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace {
namespace record {

static_assert(std::is_same_v<int, int32_t> && sizeof(float) == 4 && sizeof(double) == 8);

constexpr char MAGIC[] = "TWREC";
constexpr size_t MAGIC_BYTES = sizeof MAGIC - 1;
constexpr uint8_t VERSION = 1;
constexpr int MAX_DEPTH = 1024;

enum class Kind : uint8_t {
    End = 0, Construct, Destroy, Receive, Frame, Touch, Hover, SetWidth, SetMaxHeight, SetTheme, ClearSelection, ClaimAt, WantFrame,
    Wheel, Key, Edit, EndEditing, SetCompositing, ImageReady, SetResizeTracking, Trim, Charge, SetPatching, LastHtml,
};

[[maybe_unused]] const char* kind_name(Kind kind) {
    switch (kind) {
    case Kind::End: return "End";
    case Kind::Construct: return "Construct";
    case Kind::Destroy: return "Destroy";
    case Kind::Receive: return "Receive";
    case Kind::Frame: return "Frame";
    case Kind::Touch: return "Touch";
    case Kind::Hover: return "Hover";
    case Kind::SetWidth: return "SetWidth";
    case Kind::SetMaxHeight: return "SetMaxHeight";
    case Kind::SetTheme: return "SetTheme";
    case Kind::ClearSelection: return "ClearSelection";
    case Kind::ClaimAt: return "ClaimAt";
    case Kind::WantFrame: return "WantFrame";
    case Kind::Wheel: return "Wheel";
    case Kind::Key: return "Key";
    case Kind::Edit: return "Edit";
    case Kind::EndEditing: return "EndEditing";
    case Kind::SetCompositing: return "SetCompositing";
    case Kind::ImageReady: return "ImageReady";
    case Kind::SetResizeTracking: return "SetResizeTracking";
    case Kind::Trim: return "Trim";
    case Kind::Charge: return "Charge";
    case Kind::SetPatching: return "SetPatching";
    case Kind::LastHtml: return "LastHtml";
    }
    return "unknown";
}

/// One non-const call into one renderer: its arguments, the Backend calls and clock reads (µs) it made, and what the renderer gave and showed after it.
struct Step {
    Kind kind = Kind::End;
    uint32_t session = 0;
    std::string label;
    uint64_t names = 0, name_bytes = 0;
    std::vector<double> numbers;
    std::vector<std::string> texts;
    std::optional<twidget::Json> message;
    std::vector<uint64_t> clock;
    std::vector<Call> calls;
    std::vector<std::string> messages;
    bool engine = false;
    std::string cursor;
    bool painted = false;
    DisplayList list;
    wire::Damage damage;
    twidget::WidgetGestureClaim claim;
    std::string selection;
    remote::State state;
};

struct End {
    uint32_t sessions = 0;
    uint64_t steps = 0;
    std::string error;
};

void put_varint(std::string& out, uint64_t v) {
    while (v >= 0x80) {
        out.push_back(char(uint8_t(v) | 0x80));
        v >>= 7;
    }
    out.push_back(char(uint8_t(v)));
}

template <class U> void put_little(std::string& out, U v) {
    for (size_t i = 0; i < sizeof(U); i++) out.push_back(char(uint8_t(v >> (8 * i))));
}

void put_f64(std::string& out, const double& v) {
    uint64_t b = 0;
    std::memcpy(&b, &v, sizeof b);
    put_little(out, b);
}

void put_f32(std::string& out, const float& v) {
    uint32_t b = 0;
    std::memcpy(&b, &v, sizeof b);
    put_little(out, b);
}

void put_i32(std::string& out, int v) {
    uint32_t b = 0;
    std::memcpy(&b, &v, sizeof b);
    put_little(out, b);
}

void put_flag(std::string& out, bool v) {
    out.push_back(char(v ? 1 : 0));
}

void put_bytes(std::string& out, std::string_view bytes) {
    put_varint(out, bytes.size());
    out.append(bytes);
}

bool put_json(std::string& out, const twidget::Json& value, int depth) {
    if (depth > MAX_DEPTH) return false;
    switch (value.type()) {
    case twidget::Json::Type::Null: out.push_back(0); return true;
    case twidget::Json::Type::Bool: out.push_back(char(value.asBool() ? 2 : 1)); return true;
    case twidget::Json::Type::Number:
        out.push_back(3);
        put_f64(out, value.asNumber());
        return true;
    case twidget::Json::Type::String:
        out.push_back(4);
        put_bytes(out, value.asString());
        return true;
    case twidget::Json::Type::Array:
        out.push_back(5);
        put_varint(out, value.arr().size());
        for (const twidget::Json& item : value.arr())
            if (!put_json(out, item, depth + 1)) return false;
        return true;
    case twidget::Json::Type::Object:
        out.push_back(6);
        put_varint(out, value.obj().size());
        for (const auto& [key, item] : value.obj()) {
            put_bytes(out, key);
            if (!put_json(out, item, depth + 1)) return false;
        }
        return true;
    }
    return false;
}

void put_metrics(std::string& out, const FontMetrics& metrics) {
    std::string value;
    wire::encode(metrics, value);
    put_bytes(out, value);
}

bool put_call(std::string& out, const Call& c) {
    out.push_back(char(uint8_t(c.op)));
    switch (c.op) {
    case Call::Font:
    case Call::CanvasFont:
        put_bytes(out, c.families);
        if (c.op == Call::Font) put_bytes(out, c.features);
        put_f32(out, c.size);
        put_i32(out, c.weight);
        put_flag(out, c.italic);
        put_metrics(out, c.metrics_in);
        put_metrics(out, c.metrics);
        put_i32(out, c.font);
        return true;
    case Call::DeleteFont:
        put_i32(out, c.font);
        return true;
    case Call::TextWidth:
        put_i32(out, c.font);
        put_bytes(out, c.text);
        put_f32(out, c.width);
        return true;
    case Call::Image:
        put_i32(out, c.id);
        put_bytes(out, c.url);
        put_f32(out, c.w_in);
        put_f32(out, c.h_in);
        put_flag(out, c.ok);
        put_f32(out, c.image_w);
        put_f32(out, c.image_h);
        return true;
    case Call::CanvasRun: {
        std::string value;
        if (!wire::encode(c.ops, wire::Caps(), value)) return false;
        put_i32(out, c.id);
        put_i32(out, c.w);
        put_i32(out, c.h);
        put_bytes(out, value);
        return true;
    }
    case Call::CanvasRead:
        put_i32(out, c.id);
        put_i32(out, c.x);
        put_i32(out, c.y);
        put_i32(out, c.w);
        put_i32(out, c.h);
        put_varint(out, c.rgba_in);
        put_flag(out, c.ok);
        put_bytes(out, view(c.rgba));
        return true;
    case Call::CanvasRelease:
        put_i32(out, c.id);
        return true;
    case Call::Frame:
        put_flag(out, c.painted);
        return true;
    }
    return false;
}

enum class Got { Value, Short, Bad };

/// LEB128 of at most ten bytes in its shortest form; Short when `next` runs out first.
template <class Next> Got get_varint(Next&& next, uint64_t& v) {
    v = 0;
    for (int shift = 0; shift < 64; shift += 7) {
        uint8_t b = 0;
        if (!next(b)) return Got::Short;
        if ((shift == 63 && b > 1) || (shift > 0 && b == 0)) return Got::Bad;
        v |= uint64_t(b & 0x7f) << shift;
        if (!(b & 0x80)) return Got::Value;
    }
    return Got::Bad;
}

/// The fields of one record, in order; false with `bad` empty when the record runs past its frame, else with `bad` naming the value.
struct Take {
    std::string_view rest;
    std::string bad;

    bool refuse(std::string why) {
        bad = std::move(why);
        return false;
    }
    bool byte(uint8_t& v) {
        if (rest.empty()) return false;
        v = uint8_t(rest[0]);
        rest.remove_prefix(1);
        return true;
    }
    bool varint(uint64_t& v) {
        const Got got = get_varint([&](uint8_t& b) { return byte(b); }, v);
        return got == Got::Value || (got == Got::Bad && refuse("a malformed number"));
    }
    bool count(size_t& n, size_t item_bytes) {
        uint64_t c = 0;
        if (!varint(c) || c > rest.size() / item_bytes) return false;
        n = size_t(c);
        return true;
    }
    template <class U> bool little(U& v) {
        if (rest.size() < sizeof(U)) return false;
        U x = 0;
        for (size_t i = 0; i < sizeof(U); i++) x = U(x | (U(uint8_t(rest[i])) << (8 * i)));
        v = x;
        rest.remove_prefix(sizeof(U));
        return true;
    }
    bool f64(double& v) {
        uint64_t b = 0;
        if (!little(b)) return false;
        std::memcpy(&v, &b, sizeof v);
        return true;
    }
    bool f32(float& v) {
        uint32_t b = 0;
        if (!little(b)) return false;
        std::memcpy(&v, &b, sizeof v);
        return true;
    }
    bool i32(int& v) {
        uint32_t b = 0;
        if (!little(b)) return false;
        std::memcpy(&v, &b, sizeof v);
        return true;
    }
    bool flag(bool& v) {
        uint8_t b = 0;
        if (!byte(b)) return false;
        if (b > 1) return refuse("a flag of " + std::to_string(b));
        v = b == 1;
        return true;
    }
    bool bytes(std::string_view& v) {
        size_t n = 0;
        if (!count(n, 1)) return false;
        v = rest.substr(0, n);
        rest.remove_prefix(n);
        return true;
    }
    bool bytes(std::string& v) {
        std::string_view b;
        if (!bytes(b)) return false;
        v.assign(b);
        return true;
    }
    bool metrics(FontMetrics& m) {
        std::string_view b;
        return bytes(b) && (wire::decode(b, m) || refuse("font metrics that do not decode"));
    }
    bool json(twidget::Json& out, int depth);
    bool call(Call& c);
};

bool Take::json(twidget::Json& out, int depth) {
    if (depth > MAX_DEPTH) return refuse("a message nested past " + std::to_string(MAX_DEPTH));
    uint8_t tag = 0;
    if (!byte(tag)) return false;
    switch (tag) {
    case 0: out = twidget::Json(); return true;
    case 1: out = twidget::Json(false); return true;
    case 2: out = twidget::Json(true); return true;
    case 3: {
        double d = 0;
        if (!f64(d)) return false;
        out = twidget::Json(d);
        return true;
    }
    case 4: {
        std::string s;
        if (!bytes(s)) return false;
        out = twidget::Json(std::move(s));
        return true;
    }
    case 5: {
        size_t n = 0;
        if (!count(n, 1)) return false;
        twidget::Json::Array items;
        for (size_t i = 0; i < n; i++) {
            twidget::Json item;
            if (!json(item, depth + 1)) return false;
            items.push_back(std::move(item));
        }
        out = twidget::Json(std::move(items));
        return true;
    }
    case 6: {
        size_t n = 0;
        if (!count(n, 2)) return false;
        twidget::Json::Object pairs;
        for (size_t i = 0; i < n; i++) {
            std::string key;
            twidget::Json item;
            if (!bytes(key) || !json(item, depth + 1)) return false;
            pairs.emplace_back(std::move(key), std::move(item));
        }
        out = twidget::Json(std::move(pairs));
        return true;
    }
    default: return refuse("a message value of tag " + std::to_string(tag));
    }
}

bool Take::call(Call& c) {
    uint8_t op = 0;
    if (!byte(op)) return false;
    if (op > Call::Frame) return refuse("a call of unknown op " + std::to_string(op));
    c = Call();
    c.op = Call::Op(op);
    std::string_view value;
    uint64_t in = 0;
    switch (c.op) {
    case Call::Font:
    case Call::CanvasFont:
        return bytes(c.families) && (c.op == Call::CanvasFont || bytes(c.features)) && f32(c.size) && i32(c.weight) && flag(c.italic)
            && metrics(c.metrics_in) && metrics(c.metrics) && i32(c.font);
    case Call::DeleteFont: return i32(c.font);
    case Call::TextWidth: return i32(c.font) && bytes(c.text) && f32(c.width);
    case Call::Image: return i32(c.id) && bytes(c.url) && f32(c.w_in) && f32(c.h_in) && flag(c.ok) && f32(c.image_w) && f32(c.image_h);
    case Call::CanvasRun:
        return i32(c.id) && i32(c.w) && i32(c.h) && bytes(value) && (wire::decode(value, wire::Caps(), c.ops) || refuse("a canvas batch that does not decode"));
    case Call::CanvasRead:
        if (!i32(c.id) || !i32(c.x) || !i32(c.y) || !i32(c.w) || !i32(c.h) || !varint(in) || !flag(c.ok) || !bytes(value)) return false;
        c.rgba_in = size_t(in);
        if (c.rgba_in != in) return refuse("a pixel count of " + std::to_string(in));
        c.rgba.assign(reinterpret_cast<const uint8_t*>(value.data()), reinterpret_cast<const uint8_t*>(value.data()) + value.size());
        return true;
    case Call::CanvasRelease: return i32(c.id);
    case Call::Frame: return flag(c.painted);
    }
    return false;
}

bool take_step(Take& t, Step& s, std::map<uint32_t, uint64_t>& clocks) {
    uint64_t session = 0;
    if (!t.varint(session)) return false;
    if (session == 0 || session > UINT32_MAX) return t.refuse("session " + std::to_string(session));
    s.session = uint32_t(session);
    size_t n = 0;
    bool message = false;
    if (!t.bytes(s.label) || !t.varint(s.names) || !t.varint(s.name_bytes) || !t.count(n, 8)) return false;
    s.numbers.resize(n);
    for (double& number : s.numbers)
        if (!t.f64(number)) return false;
    if (!t.count(n, 1)) return false;
    for (size_t i = 0; i < n; i++)
        if (!t.bytes(s.texts.emplace_back())) return false;
    if (!t.flag(message)) return false;
    if (message && !t.json(s.message.emplace(), 0)) return false;
    if (!t.count(n, 1)) return false;
    uint64_t& last = clocks[s.session];
    for (size_t i = 0; i < n; i++) {
        uint64_t delta = 0;
        if (!t.varint(delta)) return false;
        if (delta > UINT64_MAX - last) return t.refuse("a clock read past 64 bits");
        last += delta;
        s.clock.push_back(last);
    }
    if (!t.count(n, 2)) return false;
    for (size_t i = 0; i < n; i++)
        if (!t.call(s.calls.emplace_back())) return false;
    if (!t.count(n, 1)) return false;
    for (size_t i = 0; i < n; i++)
        if (!t.bytes(s.messages.emplace_back())) return false;
    uint8_t flags = 0;
    if (!t.byte(flags) || !t.bytes(s.cursor)) return false;
    s.engine = flags & 1;
    s.painted = flags & 2;
    s.claim = twidget::WidgetGestureClaim{bool(flags & 4), bool(flags & 8), bool(flags & 16)};
    s.state.dirty = flags & 32;
    s.state.wants_tick = flags & 64;
    s.state.frame_asked = flags & 128;
    std::string_view value;
    if (s.painted) {
        if (!t.bytes(value)) return false;
        if (!wire::decode(value, wire::Caps(), s.list)) return t.refuse("a list that does not decode");
        if (!t.bytes(value)) return false;
        if (!wire::decode(value, wire::Caps(), s.damage)) return t.refuse("damage that does not decode");
    }
    return t.bytes(s.selection) && t.f64(s.state.width) && t.i32(s.state.height) && t.f64(s.state.next_tick_in);
}

class Writer {
public:
    bool open(const std::string& path, const std::string& origin);
    /// False, nothing written, for a step no Reader would give back: a list, damage or batch that does not encode, a clock read before its session's last, a message nested too deep.
    bool write(const Step& step);
    bool close(const End& end);

private:
    bool frame(const std::string& record);

    std::ofstream out_;
    std::map<uint32_t, uint64_t> clocks_;
};

bool Writer::open(const std::string& path, const std::string& origin) {
    if (out_.is_open() || !twidget::Json::parse(origin)) return false;
    out_.open(path, std::ios::binary | std::ios::trunc);
    std::string head(MAGIC, MAGIC_BYTES);
    head.push_back(char(VERSION));
    put_bytes(head, origin);
    out_.write(head.data(), std::streamsize(head.size()));
    return out_.is_open() && bool(out_);
}

bool Writer::frame(const std::string& record) {
    std::string head;
    put_varint(head, record.size());
    out_.write(head.data(), std::streamsize(head.size()));
    out_.write(record.data(), std::streamsize(record.size()));
    return bool(out_);
}

bool Writer::write(const Step& s) {
    if (!out_.is_open() || s.kind == Kind::End || uint8_t(s.kind) > uint8_t(Kind::LastHtml) || s.session == 0) return false;
    if (!s.painted && (!s.list.empty() || s.damage)) return false;
    std::string r;
    r.push_back(char(uint8_t(s.kind)));
    put_varint(r, s.session);
    put_bytes(r, s.label);
    put_varint(r, s.names);
    put_varint(r, s.name_bytes);
    put_varint(r, s.numbers.size());
    for (const double& number : s.numbers) put_f64(r, number);
    put_varint(r, s.texts.size());
    for (const std::string& text : s.texts) put_bytes(r, text);
    put_flag(r, s.message.has_value());
    if (s.message && !put_json(r, *s.message, 0)) return false;
    uint64_t last = clocks_[s.session];
    put_varint(r, s.clock.size());
    for (const uint64_t read : s.clock) {
        if (read < last) return false;
        put_varint(r, read - last);
        last = read;
    }
    put_varint(r, s.calls.size());
    for (const Call& c : s.calls)
        if (!put_call(r, c)) return false;
    put_varint(r, s.messages.size());
    for (const std::string& m : s.messages) put_bytes(r, m);
    r.push_back(char(uint8_t((s.engine ? 1 : 0) | (s.painted ? 2 : 0) | (s.claim.x ? 4 : 0) | (s.claim.y ? 8 : 0) | (s.claim.hold ? 16 : 0)
        | (s.state.dirty ? 32 : 0) | (s.state.wants_tick ? 64 : 0) | (s.state.frame_asked ? 128 : 0))));
    put_bytes(r, s.cursor);
    if (s.painted) {
        std::string value;
        if (!wire::encode(s.list, wire::Caps(), value)) return false;
        put_bytes(r, value);
        value.clear();
        if (!wire::encode(s.damage, wire::Caps(), value)) return false;
        put_bytes(r, value);
    }
    put_bytes(r, s.selection);
    put_f64(r, s.state.width);
    put_i32(r, s.state.height);
    put_f64(r, s.state.next_tick_in);
    if (!frame(r)) return false;
    clocks_[s.session] = last;
    return true;
}

bool Writer::close(const End& end) {
    if (!out_.is_open()) return false;
    std::string r;
    r.push_back(char(uint8_t(Kind::End)));
    put_varint(r, end.sessions);
    put_varint(r, end.steps);
    put_bytes(r, end.error);
    const bool written = frame(r);
    out_.close();
    return written && !out_.fail();
}

class Reader {
public:
    bool open(const std::string& path);
    const std::string& origin() const { return origin_; }
    /// The next step and the bytes its frame took; false at a complete end record, with ended(), or on a refusal, with error().
    bool next(Step& step, size_t& bytes);
    bool ended() const { return ended_; }
    const End& end() const { return end_; }
    const std::string& error() const { return error_; }

private:
    bool refuse(std::string why);
    bool read(char* to, size_t n);
    bool get(uint8_t& b);
    Got length(uint64_t& n);
    bool finish(Take& t);

    std::ifstream in_;
    uint64_t left_ = 0;
    uint64_t steps_ = 0;
    uint32_t sessions_ = 0;
    std::map<uint32_t, uint64_t> clocks_;
    std::string origin_, frame_, error_;
    End end_;
    bool ended_ = false, done_ = false;
};

bool Reader::refuse(std::string why) {
    error_ = std::move(why);
    done_ = true;
    return false;
}

bool Reader::read(char* to, size_t n) {
    if (n > left_) return false;
    if (n && !in_.read(to, std::streamsize(n))) return false;
    left_ -= n;
    return true;
}

bool Reader::get(uint8_t& b) {
    char c = 0;
    if (!read(&c, 1)) return false;
    b = uint8_t(c);
    return true;
}

Got Reader::length(uint64_t& n) {
    return get_varint([&](uint8_t& b) { return get(b); }, n);
}

bool Reader::open(const std::string& path) {
    in_.open(path, std::ios::binary);
    if (!in_.is_open()) return refuse("cannot open " + path);
    in_.seekg(0, std::ios::end);
    const std::streamoff size = in_.tellg();
    in_.seekg(0, std::ios::beg);
    if (!in_ || size < 0) return refuse("cannot read " + path);
    left_ = uint64_t(size);
    char magic[MAGIC_BYTES] = {};
    uint8_t version = 0;
    if (!read(magic, MAGIC_BYTES) || std::memcmp(magic, MAGIC, MAGIC_BYTES) != 0 || !get(version)) return refuse("not a recording");
    if (version != VERSION) return refuse("version " + std::to_string(version));
    uint64_t n = 0;
    const Got got = length(n);
    if (got == Got::Bad) return refuse("a malformed origin length");
    if (got == Got::Short || n > left_) return refuse("truncated in the origin");
    origin_.resize(size_t(n));
    if (!read(origin_.data(), origin_.size())) return refuse("truncated in the origin");
    if (!twidget::Json::parse(origin_)) return refuse("the origin is not JSON");
    return true;
}

bool Reader::next(Step& step, size_t& bytes) {
    bytes = 0;
    if (done_) return false;
    const std::string at = "step " + std::to_string(steps_ + 1);
    if (left_ == 0) return refuse("no end record after " + std::to_string(steps_) + " steps");
    const uint64_t before = left_;
    uint64_t n = 0;
    const Got got = length(n);
    if (got == Got::Short) return refuse("truncated in " + at);
    if (got == Got::Bad) return refuse(at + " has a malformed length");
    if (n > left_) return refuse(in_.peek() == int(Kind::End) ? "truncated in the end record" : "truncated in " + at);
    frame_.resize(size_t(n));
    if (!read(frame_.data(), frame_.size())) return refuse("truncated in " + at);
    bytes = size_t(before - left_);
    if (frame_.empty()) return refuse(at + " does not fill its frame exactly");
    const uint8_t kind = uint8_t(frame_[0]);
    Take t{std::string_view(frame_).substr(1), {}};
    if (kind == uint8_t(Kind::End)) return finish(t);
    if (kind > uint8_t(Kind::LastHtml)) return refuse(at + " has unknown kind " + std::to_string(kind));
    step = Step();
    step.kind = Kind(kind);
    if (!take_step(t, step, clocks_) || !t.rest.empty()) return refuse(at + (t.bad.empty() ? " does not fill its frame exactly" : " holds " + t.bad));
    steps_++;
    if (step.kind == Kind::Construct) sessions_++;
    return true;
}

bool Reader::finish(Take& t) {
    uint64_t sessions = 0;
    if (!t.varint(sessions) || !t.varint(end_.steps) || !t.bytes(end_.error) || !t.rest.empty())
        return refuse(t.bad.empty() ? "the end record does not fill its frame exactly" : "the end record holds " + t.bad);
    if (sessions > UINT32_MAX) return refuse("the end record counts " + std::to_string(sessions) + " sessions");
    end_.sessions = uint32_t(sessions);
    if (left_ != 0) return refuse("bytes after the end record");
    if (!end_.error.empty()) return refuse("the recording ends with an error: " + end_.error);
    if (end_.sessions != sessions_ || end_.steps != steps_) {
        return refuse("the end record counts " + std::to_string(end_.sessions) + " sessions and " + std::to_string(end_.steps) + " steps where the file has "
            + std::to_string(sessions_) + " and " + std::to_string(steps_));
    }
    ended_ = true;
    done_ = true;
    return false;
}

} // namespace record
} // namespace
