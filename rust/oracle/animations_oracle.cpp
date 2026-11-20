// core/src/animations.cpp on stdin for rust/crates/animations to compare with: its records and answers are laid out in that crate's tests/common/mod.rs.
#include "animations.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <map>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

using namespace twlite;
using twidget::Json;

namespace {

bool read_exact(void* to, size_t n) {
    return n == 0 || std::fread(to, 1, n, stdin) == n;
}

struct In {
    std::string_view s;
    size_t at = 0;

    template <typename T> T raw() {
        T v{};
        if (at + sizeof v <= s.size()) std::memcpy(&v, s.data() + at, sizeof v);
        at += sizeof v;
        return v;
    }
    uint8_t u8() { return raw<uint8_t>(); }
    uint32_t u32() { return raw<uint32_t>(); }
    int64_t i64() { return raw<int64_t>(); }
    double f64() { return raw<double>(); }
    std::string str() {
        size_t n = u32();
        n = std::min(n, at <= s.size() ? s.size() - at : 0);
        std::string out(s.substr(std::min(at, s.size()), n));
        at += n;
        return out;
    }
};

struct Out {
    std::string s;
    template <typename T> void raw(T v) { s.append(reinterpret_cast<const char*>(&v), sizeof v); }
    void u8(uint8_t v) { raw(v); }
    void u32(uint32_t v) { raw(v); }
    void i32(int32_t v) { raw(v); }
    void i64(int64_t v) { raw(v); }
    void u64(uint64_t v) { raw(v); }
    // Which NaN an operation gives is the compiler's choice, not the source's: every NaN is written as one.
    void f64(double v) { raw(std::isnan(v) ? std::numeric_limits<double>::quiet_NaN() : v); }
    void str(std::string_view v) {
        u32(uint32_t(v.size()));
        s.append(v);
    }
    void list(const std::vector<std::string>& v) {
        u32(uint32_t(v.size()));
        for (const std::string& x : v) str(x);
    }
    void ids(const std::vector<int64_t>& v) {
        u32(uint32_t(v.size()));
        for (int64_t x : v) i64(x);
    }
    void values(const StyleValues& v) {
        u32(uint32_t(v.size()));
        for (const auto& [name, value] : v) {
            str(name);
            str(value);
        }
    }
    void timing(const Timing& t) {
        u8(uint8_t(t.kind));
        u8(uint8_t(t.jump));
        for (double v : {t.x1, t.y1, t.x2, t.y2}) f64(v);
        i32(t.steps);
        u32(uint32_t(t.points.size()));
        for (const auto& [input, output] : t.points) {
            f64(input);
            f64(output);
        }
        str(t.text);
    }
};

Declared declared(In& in) {
    Declared d;
    const uint32_t n = in.u32();
    for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) {
        std::string name = in.str();
        d[name] = in.str();
    }
    return d;
}

std::vector<int64_t> id_list(In& in) {
    std::vector<int64_t> out;
    const uint32_t n = in.u32();
    for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) out.push_back(in.i64());
    return out;
}

// The animator's maps are unordered: its events come per element in no order the source decides, so they go out by element, each element's in its order.
void events(Out& o, std::vector<AnimationEvent> list) {
    std::stable_sort(list.begin(), list.end(), [](const AnimationEvent& a, const AnimationEvent& b) {
        const int pa = a.type == "transitionend", pb = b.type == "transitionend";
        return pa != pb ? pa < pb : a.target < b.target;
    });
    o.u32(uint32_t(list.size()));
    for (const AnimationEvent& e : list) {
        o.str(e.type);
        o.i64(e.target);
        o.str(e.name);
        o.f64(e.elapsed);
    }
}

void motions(Out& o, std::vector<Animator::Motion> list) {
    auto rank = [](char kind) { return kind == 'a' ? 0 : kind == 't' ? 1 : 2; };
    std::stable_sort(list.begin(), list.end(), [&](const Animator::Motion& a, const Animator::Motion& b) {
        if (rank(a.kind) != rank(b.kind)) return rank(a.kind) < rank(b.kind);
        return rank(a.kind) < 2 && a.id < b.id;
    });
    o.u32(uint32_t(list.size()));
    for (const Animator::Motion& m : list) {
        o.i64(m.id);
        o.u8(uint8_t(m.kind));
        o.str(m.name);
        o.i64(m.aid);
        o.u8(m.paint.heavy ? 1 : 0);
        o.u8(m.paint.backdrop ? 1 : 0);
    }
}

void script(In& in, Out& o) {
    Animator a;
    const uint32_t steps = in.u32();
    for (uint32_t i = 0; i < steps && in.at < in.s.size(); i++) {
        const uint8_t kind = in.u8();
        o.u8(kind);
        switch (kind) {
        case 'k': {
            std::vector<std::string> texts;
            const uint32_t n = in.u32();
            for (uint32_t k = 0; k < n && in.at < in.s.size(); k++) texts.push_back(in.str());
            a.set_style_sheets(texts);
            break;
        }
        case 'u': {
            const double now = in.f64();
            std::unordered_map<int64_t, Declared> all;
            const uint32_t n = in.u32();
            for (uint32_t k = 0; k < n && in.at < in.s.size(); k++) {
                const int64_t id = in.i64();
                all[id] = declared(in);
            }
            a.update(all, now);
            events(o, a.take_events());
            break;
        }
        case 'p': {
            auto out = a.sample(in.f64());
            o.u32(uint32_t(out.size()));
            for (const auto& [id, values] : out) {
                o.i64(id);
                o.values(values);
            }
            events(o, a.take_events());
            break;
        }
        case 'w': {
            std::string text = in.str();
            const double offset = in.f64();
            auto message = Json::parse(text);
            o.u8(message ? 1 : 0);
            if (message) a.apply_web(*message, offset);
            break;
        }
        case 'b': {
            a.underlying.clear();
            const uint32_t n = in.u32();
            for (uint32_t k = 0; k < n && in.at < in.s.size(); k++) {
                const int64_t id = in.i64();
                a.underlying[id] = declared(in);
            }
            break;
        }
        case 'a': {
            const double now = in.f64();
            auto skip = id_list(in);
            o.u8(a.active(now) ? 1 : 0);
            o.u8(a.active(now, std::unordered_set<int64_t>(skip.begin(), skip.end())) ? 1 : 0);
            break;
        }
        case 'i': o.ids(a.animated_ids(in.f64())); break;
        case 'r': o.list(a.animated_properties(in.i64())); break;
        case 'e': {
            const int64_t id = in.i64();
            o.values(a.peek(id, in.f64()));
            break;
        }
        case 'v': {
            const int64_t id = in.i64();
            o.str(a.base_value(id, in.str()));
            break;
        }
        case 'd': o.u8(a.declares_motion(in.i64()) ? 1 : 0); break;
        case 'l': {
            const int64_t id = in.i64();
            const double now = in.f64();
            auto plan = a.plan(id, now, in.u8() != 0);
            o.u8(plan ? 1 : 0);
            if (plan) {
                o.u8(plan->ready ? 1 : 0);
                o.f64(plan->start);
                o.f64(plan->duration);
                o.u8(plan->repeat ? 1 : 0);
                o.f64(plan->wake);
            }
            break;
        }
        case 'g': o.u64(a.signature(in.i64())); break;
        case 'q': {
            auto ids = id_list(in);
            o.str(a.css_animations(ids, in.f64()).dump());
            break;
        }
        case 't': {
            o.ids(a.web_targets());
            o.list(a.web_properties(in.i64()));
            break;
        }
        case 'x': {
            auto alive = id_list(in);
            a.prune([&](int64_t id) { return std::find(alive.begin(), alive.end(), id) != alive.end(); });
            break;
        }
        case 'h': {
            const double now = in.f64();
            o.u64(a.heavy_motion(now, nullptr));
            std::vector<Animator::Motion> list;
            o.u64(a.heavy_motion(now, &list));
            motions(o, std::move(list));
            break;
        }
        default: std::exit(4);
        }
    }
}

void timing(In& in, Out& o) {
    std::string text = in.str();
    auto t = Timing::parse(text);
    o.u8(t ? 1 : 0);
    if (t) o.timing(*t);
    const uint32_t n = in.u32();
    for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) {
        const double x = in.f64();
        if (!t) continue;
        o.f64(t->css(x));
        o.f64(t->web(x, false));
        o.f64(t->web(x, true));
    }
}

void keyframes(In& in, Out& o) {
    KeyframeRules rules;
    const uint32_t n = in.u32();
    for (uint32_t i = 0; i < n && in.at < in.s.size(); i++) parse_keyframes(in.str(), rules);
    std::vector<std::string> names;
    for (const auto& [name, frames] : rules) names.push_back(name);
    std::sort(names.begin(), names.end());
    o.u32(uint32_t(names.size()));
    for (const std::string& name : names) {
        o.str(name);
        const auto& frames = rules[name];
        o.u32(uint32_t(frames.size()));
        for (const Keyframe& f : frames) {
            o.f64(f.offset);
            o.values(f.declarations);
            o.u8(f.timing ? 1 : 0);
            if (f.timing) o.timing(*f.timing);
        }
    }
}

void specs(In& in, Out& o) {
    Declared d = declared(in);
    auto animations = AnimationSpec::list(d);
    o.u32(uint32_t(animations.size()));
    for (const AnimationSpec& s : animations) {
        o.str(s.name);
        o.f64(s.duration);
        o.str(s.timing_text);
        o.f64(s.delay);
        o.f64(s.iterations);
        o.str(s.direction);
        o.str(s.fill);
        o.u8(s.paused ? 1 : 0);
        o.timing(s.timing());
    }
    auto transitions = TransitionSpec::list(d);
    o.u32(uint32_t(transitions.size()));
    for (const TransitionSpec& s : transitions) {
        o.str(s.property);
        o.f64(s.duration);
        o.timing(s.timing);
        o.f64(s.delay);
    }
}

void web(In& in, Out& o) {
    std::string text = in.str();
    const double offset = in.f64();
    auto record = Json::parse(text);
    o.u8(record ? 1 : 0);
    if (!record) return;
    auto a = WebAnimation::parse(*record, offset);
    o.u8(a ? 1 : 0);
    if (!a) return;
    o.i64(a->aid);
    o.i64(a->target);
    o.u32(uint32_t(a->frames.size()));
    for (const WebAnimation::Frame& f : a->frames) {
        o.f64(f.offset);
        o.timing(f.easing);
        o.str(f.composite);
        o.values(f.values);
    }
    for (double v : {a->duration, a->delay, a->end_delay, a->iterations, a->iteration_start}) o.f64(v);
    o.str(a->direction);
    o.str(a->fill);
    o.timing(a->easing);
    o.str(a->composite);
    o.str(a->iteration_composite);
    o.u8(a->start_ms ? 1 : 0);
    o.f64(a->start_ms.value_or(0));
    o.u8(a->hold_ms ? 1 : 0);
    o.f64(a->hold_ms.value_or(0));
    o.f64(a->rate);
    o.list(a->properties);
    o.u8(a->running() ? 1 : 0);
}

} // namespace

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::string bytes;
    for (int op; (op = std::fgetc(stdin)) != EOF;) {
        unsigned char b[4];
        if (!read_exact(b, 4)) return 2;
        const uint32_t n = uint32_t(b[0]) | (uint32_t(b[1]) << 8) | (uint32_t(b[2]) << 16) | (uint32_t(b[3]) << 24);
        bytes.resize(n);
        if (!read_exact(bytes.data(), n)) return 2;
        In in{bytes};
        Out o;
        switch (op) {
        case 'S': script(in, o); break;
        case 'T': timing(in, o); break;
        case 'I': {
            std::string a = in.str(), b = in.str();
            o.str(interpolate(a, b, in.f64()));
            break;
        }
        case 'K': keyframes(in, o); break;
        case 'A': specs(in, o); break;
        case 'W': web(in, o); break;
        default: return 3;
        }
        const uint32_t size = uint32_t(o.s.size());
        const unsigned char head[5] = {1, uint8_t(size), uint8_t(size >> 8), uint8_t(size >> 16), uint8_t(size >> 24)};
        std::fwrite(head, 1, 5, stdout);
        if (size) std::fwrite(o.s.data(), 1, size, stdout);
    }
    std::fflush(stdout);
    return 0;
}
