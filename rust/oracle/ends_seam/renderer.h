// The seam for ends_oracle: renderer_end.cpp built against this renderer.h, a scripted Renderer that does what crates/ends/tests/common/scripted.rs does.
#pragma once

#include "backend.h"
#include "display_list.h"
#include "twidget/gesture_policy.h"
#include "twidget/json.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <deque>
#include <limits>
#include <map>
#include <memory>
#include <string>
#include <utility>
#include <vector>

namespace twlite {

// The renderer's log and the script the next renderer is made with, set by the oracle.
inline std::vector<std::string> g_renderer_log;
inline const twidget::Json* g_renderer_script = nullptr;

class Clock {
public:
    virtual ~Clock() = default;
    virtual double now_ms() = 0;
};

namespace scripted {

inline std::string hex(const std::string& bytes) {
    static const char digits[] = "0123456789abcdef";
    std::string out;
    for (unsigned char c : bytes) {
        out.push_back(digits[c >> 4]);
        out.push_back(digits[c & 15]);
    }
    return out;
}

inline std::string f32s(float v) {
    uint32_t b;
    std::memcpy(&b, &v, 4);
    char s[16];
    std::snprintf(s, sizeof s, "%08x", b);
    return s;
}

inline std::string f64s(double v) {
    uint64_t b;
    std::memcpy(&b, &v, 8);
    char s[24];
    std::snprintf(s, sizeof s, "%016llx", static_cast<unsigned long long>(b));
    return s;
}

inline std::string metrics_line(const FontMetrics& m) {
    return f32s(m.ascent) + "," + f32s(m.descent) + "," + f32s(m.height) + "," + f32s(m.x_height) + "," + f32s(m.zero_width) + "," + f32s(m.em_ascent) + "," + f32s(m.em_descent);
}

inline float number_f32(const twidget::Json& j) {
    return float(j.asNumber(0));
}

inline Rect rect_of(const twidget::Json& j) {
    return Rect{number_f32(j[size_t(0)]), number_f32(j[size_t(1)]), number_f32(j[size_t(2)]), number_f32(j[size_t(3)])};
}

inline Color argb(const twidget::Json& j) {
    const uint32_t v = uint32_t(j.asNumber(0));
    return Color{uint8_t(v >> 16), uint8_t(v >> 8), uint8_t(v), uint8_t(v >> 24)};
}

inline int wrapped(int v, int n) {
    return ((v % n) + n) % n;
}

inline Geometry rect_geometry(const Rect& r) {
    Geometry g;
    g.ops = {Path::Move, Path::Line, Path::Line, Path::Close};
    g.points = {r.x, r.y, r.x + r.w, r.y, r.x + r.w, r.y + r.h};
    return g;
}

inline DisplayList build_list(const twidget::Json& spec) {
    std::map<int, std::shared_ptr<const Path>> paths;
    std::map<int, std::shared_ptr<const Filters>> filters;
    DisplayList list;
    for (const twidget::Json& item : spec.arr()) {
        Command c;
        c.kind = Cmd(wrapped(item["k"].asInt(0), 18));
        c.rect = rect_of(item["r"]);
        c.color = argb(item["c"]);
        c.text = item["t"].asString();
        c.font = item["f"].asInt(0);
        c.image = item["i"].asInt(0);
        c.canvas = item["cv"].asInt(0);
        const int p = item["p"].asInt(-1);
        if (p >= 0) {
            auto& path = paths[p];
            if (!path) {
                auto made = std::make_shared<Path>();
                made->ops = {Path::Move, Path::Line, Path::Close};
                made->points = {float(p), 0, float(p) + 10, 5};
                made->fill = true;
                made->fill_color = c.color;
                path = made;
            }
            c.path = path;
        }
        const int f = item["fl"].asInt(-1);
        if (f >= 0) {
            auto& list_of = filters[f];
            if (!list_of) {
                Filter blur;
                blur.kind = Filter::Blur;
                blur.amount = float(f);
                list_of = std::make_shared<Filters>(Filters{blur});
            }
            c.filter = list_of;
        }
        list.push_back(std::move(c));
    }
    return list;
}

inline std::vector<CanvasOp> build_ops(const twidget::Json& spec) {
    std::map<int, std::shared_ptr<const std::vector<CanvasClip>>> clips;
    std::map<int, std::shared_ptr<const std::vector<uint8_t>>> pixels;
    std::vector<CanvasOp> ops;
    for (const twidget::Json& item : spec.arr()) {
        CanvasOp op;
        op.kind = CanvasOp::Kind(wrapped(item["k"].asInt(1), 7));
        const Rect r = rect_of(item["r"]);
        if (op.kind == CanvasOp::Fill || op.kind == CanvasOp::Stroke || op.kind == CanvasOp::Clear) op.path = rect_geometry(r);
        op.paint.color = argb(item["c"]);
        op.text = item["t"].asString();
        op.font = item["f"].asInt(0);
        op.x = r.x;
        op.y = r.y;
        const int cl = item["cl"].asInt(-1);
        if (cl >= 0) {
            auto& list_of = clips[cl];
            if (!list_of) {
                CanvasClip clip;
                clip.path = rect_geometry(Rect{float(cl), float(cl), 10, 10});
                clip.evenodd = cl % 2 == 1;
                list_of = std::make_shared<std::vector<CanvasClip>>(std::vector<CanvasClip>{clip});
            }
            op.clips = list_of;
        }
        const twidget::Json& px = item["px"];
        if (px.isArray()) {
            const int w = px[size_t(0)].asInt(0), h = px[size_t(1)].asInt(0), n = px[size_t(2)].asInt(0);
            auto& rgba = pixels[n];
            if (!rgba) {
                std::vector<uint8_t> bytes(size_t(std::max(w, 0) * std::max(h, 0) * 4));
                for (size_t i = 0; i < bytes.size(); i++) bytes[i] = uint8_t(size_t(n) * 31 + i);
                rgba = std::make_shared<std::vector<uint8_t>>(std::move(bytes));
            }
            op.rgba = rgba;
            op.w = w;
            op.h = h;
            op.dx = r.x;
            op.dy = r.y;
        }
        ops.push_back(std::move(op));
    }
    return ops;
}

inline std::string dumps(const std::vector<twidget::Json>& messages) {
    std::string out;
    for (size_t i = 0; i < messages.size(); i++) out += (i ? " " : "") + messages[i].dump();
    return out;
}

} // namespace scripted

class Renderer {
public:
    Renderer(double width, double dpr, bool dark, Backend& backend, Clock* = nullptr) : backend_(backend), width_(width) {
        const twidget::Json& script = *g_renderer_script;
        for (const auto& [method, list] : script["on"].obj())
            for (const twidget::Json& r : list.arr()) queues_[method].push_back(r);
        if (const twidget::Json* d = script.find("drop")) {
            drop_ = *d;
            has_drop_ = true;
        }
        line("new " + scripted::f64s(width) + " " + scripted::f64s(dpr) + " " + (dark ? "1" : "0"));
        if (const twidget::Json* ctor = script.find("ctor")) react(*ctor);
    }

    ~Renderer() {
        line("drop");
        if (has_drop_) {
            react(drop_);
            return;
        }
        std::vector<int> canvases = std::move(canvases_), fonts = std::move(fonts_);
        for (int id : canvases) {
            backend_.canvas_release(id);
            line("  release " + std::to_string(id));
        }
        for (int font : fonts)
            if (font != 0) {
                backend_.delete_font(font);
                line("  del " + std::to_string(font));
            }
    }

    Renderer(const Renderer&) = delete;
    Renderer& operator=(const Renderer&) = delete;

    void push(const std::string& method, const twidget::Json& reaction) { queues_[method].push_back(reaction); }

    std::vector<twidget::Json> receive(const twidget::Json& message) {
        line("receive " + message.dump());
        const twidget::Json* r = message.find("react");
        std::vector<twidget::Json> out = r && r->isObject() ? react(twidget::Json(*r)) : queued("receive");
        line("  -> " + scripted::dumps(out));
        return out;
    }
    void set_width(double width, std::vector<twidget::Json>& out) {
        line("set_width " + scripted::f64s(width));
        width_ = width;
        for (twidget::Json& m : queued("set_width")) out.push_back(std::move(m));
    }
    void set_max_height(double height, std::vector<twidget::Json>& out) {
        line("set_max_height " + scripted::f64s(height));
        for (twidget::Json& m : queued("set_max_height")) out.push_back(std::move(m));
    }
    void set_theme(bool dark, std::vector<twidget::Json>& out) {
        line(std::string("set_theme ") + (dark ? "1" : "0"));
        for (twidget::Json& m : queued("set_theme")) out.push_back(std::move(m));
    }
    std::vector<twidget::Json> touch(int action, double x, double y, int pointer, bool primary, bool mouse = false, int modifiers = 0, int button = 0) {
        line("touch " + std::to_string(action) + " " + scripted::f64s(x) + " " + scripted::f64s(y) + " " + std::to_string(pointer) + " " + (primary ? "1" : "0") + " " + (mouse ? "1" : "0") + " " +
             std::to_string(modifiers) + " " + std::to_string(button));
        return queued("touch");
    }
    std::vector<twidget::Json> hover(int action, double x, double y, int modifiers = 0) {
        line("hover " + std::to_string(action) + " " + scripted::f64s(x) + " " + scripted::f64s(y) + " " + std::to_string(modifiers));
        return queued("hover");
    }
    std::string cursor() const {
        line("cursor");
        queued("cursor");
        return cursor_;
    }
    twidget::WidgetGestureClaim claim_at(double x, double y) {
        line("claim_at " + scripted::f64s(x) + " " + scripted::f64s(y));
        queued("claim_at");
        return claim_;
    }
    bool clear_selection() {
        line("clear_selection");
        const bool had = !selection_.empty();
        selection_.clear();
        queued("clear_selection");
        return had;
    }
    bool may_have_selection() const {
        line("may_have_selection");
        queued("may_have_selection");
        return !selection_.empty();
    }
    std::string selected_text() const {
        line("selected_text");
        queued("selected_text");
        return selection_;
    }
    std::vector<twidget::Json> tick() {
        line("tick");
        return queued("tick");
    }
    bool dirty() const {
        line("dirty");
        queued("dirty");
        return dirty_;
    }
    DisplayList paint() {
        line("paint");
        dirty_ = false;
        twidget::Json r;
        if (!pop("paint", r)) return {};
        react(r);
        return scripted::build_list(r["list"]);
    }
    bool damage(std::vector<Rect>& out) const {
        line("damage");
        twidget::Json r;
        if (!pop("damage", r)) return false;
        react(r);
        const twidget::Json& rects = r["damage"];
        if (!rects.isArray()) return false;
        for (const twidget::Json& rect : rects.arr()) out.push_back(scripted::rect_of(rect));
        return true;
    }
    double width() const {
        line("width");
        queued("width");
        return width_;
    }
    int viewport_height() const {
        line("viewport_height");
        queued("viewport_height");
        return height_;
    }
    bool wants_tick() const {
        line("wants_tick");
        queued("wants_tick");
        return wants_tick_;
    }
    double next_tick_in() const {
        line("next_tick_in");
        queued("next_tick_in");
        return next_tick_in_;
    }

private:
    void line(std::string text) const { g_renderer_log.push_back(std::move(text)); }

    bool pop(const std::string& method, twidget::Json& out) const {
        auto it = queues_.find(method);
        if (it == queues_.end() || it->second.empty()) return false;
        out = std::move(it->second.front());
        it->second.pop_front();
        return true;
    }

    std::vector<twidget::Json> queued(const std::string& method) const {
        twidget::Json r;
        return pop(method, r) ? react(r) : std::vector<twidget::Json>();
    }

    int font_at(int k) const { return k >= 0 && size_t(k) < fonts_.size() ? fonts_[size_t(k)] : k; }

    std::vector<twidget::Json> react(const twidget::Json& r) const {
        for (const twidget::Json& c : r["calls"].arr()) call(c);
        const twidget::Json& set = r["set"];
        if (const twidget::Json* v = set.find("width")) width_ = v->asNumber(0);
        if (const twidget::Json* v = set.find("height")) height_ = v->asInt(0);
        if (const twidget::Json* v = set.find("dirty")) dirty_ = v->asBool(false);
        if (const twidget::Json* v = set.find("tick")) wants_tick_ = v->asBool(false);
        if (const twidget::Json* v = set.find("next")) next_tick_in_ = v->isNumber() ? v->asNumber(0) : std::numeric_limits<double>::infinity();
        if (const twidget::Json* v = set.find("cursor")) cursor_ = v->asString();
        if (const twidget::Json* v = set.find("selection")) selection_ = v->asString();
        if (const twidget::Json* v = set.find("claim")) claim_ = twidget::WidgetGestureClaim{(*v)[size_t(0)].asBool(false), (*v)[size_t(1)].asBool(false), (*v)[size_t(2)].asBool(false)};
        return r["out"].arr();
    }

    void call(const twidget::Json& c) const {
        const std::string& name = c[size_t(0)].asString();
        if (name == "font" || name == "cfont") {
            const std::string& families = c[size_t(1)].asString();
            const float size = scripted::number_f32(c[size_t(2)]);
            const int weight = c[size_t(3)].asInt(0);
            const bool italic = c[size_t(4)].asBool(false);
            FontMetrics m;
            const int font = name == "font" ? backend_.create_font(families, size, weight, italic, c[size_t(5)].asString(), m) : backend_.create_canvas_font(families, size, weight, italic, m);
            fonts_.push_back(font);
            line("  " + name + " " + twidget::Json::quote(families) + " -> " + std::to_string(font) + " " + scripted::metrics_line(m));
        } else if (name == "del") {
            const int k = c[size_t(1)].asInt(0);
            const int font = font_at(k);
            if (k >= 0 && size_t(k) < fonts_.size()) fonts_[size_t(k)] = 0;
            backend_.delete_font(font);
            line("  del " + std::to_string(font));
        } else if (name == "width") {
            const int font = font_at(c[size_t(1)].asInt(0));
            const float w = backend_.text_width(font, c[size_t(2)].asString());
            line("  width " + std::to_string(font) + " " + twidget::Json::quote(c[size_t(2)].asString()) + " -> " + scripted::f32s(w));
        } else if (name == "image") {
            float w = 0, h = 0;
            const int id = c[size_t(1)].asInt(0);
            const bool ok = backend_.load_image(id, c[size_t(2)].asString(), w, h);
            line("  image " + std::to_string(id) + " -> " + (ok ? "1" : "0") + " " + scripted::f32s(w) + " " + scripted::f32s(h));
        } else if (name == "run") {
            const int id = c[size_t(1)].asInt(0);
            bool known = false;
            for (int k : canvases_) known = known || k == id;
            if (!known) canvases_.push_back(id);
            const std::vector<CanvasOp> ops = scripted::build_ops(c[size_t(4)]);
            backend_.canvas_run(id, c[size_t(2)].asInt(0), c[size_t(3)].asInt(0), ops);
            line("  run " + std::to_string(id) + " " + std::to_string(ops.size()));
        } else if (name == "read") {
            int a[5];
            for (size_t i = 0; i < 5; i++) a[i] = c[i + 1].asInt(0);
            std::vector<uint8_t> rgba;
            const bool ok = backend_.canvas_read(a[0], a[1], a[2], a[3], a[4], rgba);
            const bool good = ok && int64_t(rgba.size()) == int64_t(std::max(a[3], 0)) * int64_t(std::max(a[4], 0)) * 4;
            line("  read " + std::to_string(a[0]) + " -> " + (good ? "1" : "0") + " " + (good ? scripted::hex(std::string(rgba.begin(), rgba.end())) : std::string()));
        } else if (name == "release") {
            const int id = c[size_t(1)].asInt(0);
            std::vector<int> kept;
            for (int k : canvases_)
                if (k != id) kept.push_back(k);
            canvases_ = std::move(kept);
            backend_.canvas_release(id);
            line("  release " + std::to_string(id));
        } else {
            line("  unknown " + c.dump());
        }
    }

    Backend& backend_;
    mutable std::map<std::string, std::deque<twidget::Json>> queues_;
    twidget::Json drop_;
    bool has_drop_ = false;
    mutable double width_ = 0;
    mutable int height_ = 0;
    mutable bool dirty_ = false, wants_tick_ = false;
    mutable double next_tick_in_ = std::numeric_limits<double>::infinity();
    mutable std::string cursor_ = "auto", selection_;
    mutable twidget::WidgetGestureClaim claim_;
    mutable std::vector<int> fonts_, canvases_;
};

} // namespace twlite
