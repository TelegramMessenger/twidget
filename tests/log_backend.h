// A Backend that answers every call the same way each run and logs it in order, with the arguments, what it found in the outputs and what it gave back, and with a marker for each frame answer.
#pragma once

#include "backend.h"
#include "wire_compare.h"

#include <cstddef>
#include <cstdint>
#include <map>
#include <string>
#include <utility>
#include <vector>

namespace {

struct Call {
    enum Op { Font, CanvasFont, DeleteFont, TextWidth, Image, CanvasRun, CanvasRead, CanvasRelease, Frame };

    Op op = Frame;
    std::string families, features, text, url;
    float size = 0;
    int weight = 0;
    bool italic = false;
    int font = 0;
    FontMetrics metrics_in, metrics;
    float width = 0;
    int id = 0, x = 0, y = 0, w = 0, h = 0;
    float w_in = 0, h_in = 0, image_w = 0, image_h = 0;
    bool ok = false;
    bool painted = false;
    size_t rgba_in = 0;
    std::vector<uint8_t> rgba;
    std::vector<CanvasOp> ops;
};

class LogBackend : public Backend {
public:
    enum class Read { Pixels, Unsupported, WrongSize };

    int create_font(const std::string& families, float size_px, int weight, bool italic, const std::string& features, FontMetrics& out) override;
    int create_canvas_font(const std::string& families, float size_px, int weight, bool italic, FontMetrics& out) override;
    void delete_font(int font) override;
    float text_width(int font, const std::string& text) override;
    bool load_image(int id, const std::string& url, float& w, float& h) override;
    void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override;
    bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) override;
    void canvas_release(int id) override;

    void frame(bool painted);
    size_t count(Call::Op op) const;
    size_t live_fonts() const { return sizes_.size(); }

    bool images_load = true;
    Read reads = Read::Pixels;
    Backend* answers = nullptr;
    std::vector<Call> calls;

private:
    int made(Call::Op op, const std::string& families, float size, int weight, bool italic, const std::string& features, FontMetrics& out);

    int next_font_ = 0;
    std::map<int, float> sizes_;
};

FontMetrics metrics_for(float size, int weight, bool italic) {
    FontMetrics m;
    m.ascent = size * 0.75f;
    m.descent = size * 0.25f;
    m.height = size * 1.25f;
    m.x_height = size * 0.5f;
    m.zero_width = size * (weight >= 600 ? 0.625f : 0.5f);
    if (!italic) {
        m.em_ascent = size * 0.8125f;
        m.em_descent = size - m.em_ascent;
    }
    return m;
}

int LogBackend::made(Call::Op op, const std::string& families, float size, int weight, bool italic, const std::string& features, FontMetrics& out) {
    Call c;
    c.op = op;
    c.families = families;
    c.features = features;
    c.size = size;
    c.weight = weight;
    c.italic = italic;
    c.metrics_in = out;
    if (!answers) {
        out = metrics_for(size, weight, italic);
        c.font = ++next_font_;
    } else if (op == Call::CanvasFont) {
        c.font = answers->create_canvas_font(families, size, weight, italic, out);
    } else {
        c.font = answers->create_font(families, size, weight, italic, features, out);
    }
    c.metrics = out;
    if (c.font != 0) sizes_[c.font] = size;
    const int font = c.font;
    calls.push_back(std::move(c));
    return font;
}

int LogBackend::create_font(const std::string& families, float size_px, int weight, bool italic, const std::string& features, FontMetrics& out) {
    return made(Call::Font, families, size_px, weight, italic, features, out);
}

int LogBackend::create_canvas_font(const std::string& families, float size_px, int weight, bool italic, FontMetrics& out) {
    return made(Call::CanvasFont, families, size_px, weight, italic, {}, out);
}

void LogBackend::delete_font(int font) {
    Call c;
    c.op = Call::DeleteFont;
    c.font = font;
    sizes_.erase(font);
    calls.push_back(std::move(c));
    if (answers) answers->delete_font(font);
}

float LogBackend::text_width(int font, const std::string& text) {
    Call c;
    c.op = Call::TextWidth;
    c.font = font;
    c.text = text;
    float width = 0;
    if (answers) {
        width = answers->text_width(font, text);
    } else if (const auto it = sizes_.find(font); it != sizes_.end()) {
        width = it->second * 0.5f * float(text.size());
    }
    c.width = width;
    calls.push_back(std::move(c));
    return width;
}

bool LogBackend::load_image(int id, const std::string& url, float& w, float& h) {
    Call c;
    c.op = Call::Image;
    c.id = id;
    c.url = url;
    c.w_in = w;
    c.h_in = h;
    if (answers) {
        c.ok = answers->load_image(id, url, w, h);
    } else {
        c.ok = images_load;
        w = images_load ? float(8 + url.size() % 32) : 2;
        h = images_load ? float(4 + url.size() % 16) : 3;
    }
    c.image_w = w;
    c.image_h = h;
    const bool ok = c.ok;
    calls.push_back(std::move(c));
    return ok;
}

void LogBackend::canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) {
    Call c;
    c.op = Call::CanvasRun;
    c.id = id;
    c.w = w;
    c.h = h;
    c.ops = ops;
    calls.push_back(std::move(c));
    if (answers) answers->canvas_run(id, w, h, ops);
}

bool LogBackend::canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) {
    Call c;
    c.op = Call::CanvasRead;
    c.id = id;
    c.x = x;
    c.y = y;
    c.w = w;
    c.h = h;
    c.rgba_in = rgba.size();
    if (answers) {
        c.ok = answers->canvas_read(id, x, y, w, h, rgba);
    } else {
        const size_t bytes = size_t(w > 0 ? w : 0) * size_t(h > 0 ? h : 0) * 4;
        c.ok = reads != Read::Unsupported;
        if (reads == Read::Pixels) {
            rgba.resize(bytes);
            for (size_t i = 0; i < bytes; i++) rgba[i] = uint8_t(size_t(id) * 31 + size_t(x) * 7 + size_t(y) * 13 + i);
        } else if (reads == Read::WrongSize) {
            rgba.assign(bytes + 4, 0x7f);
        }
    }
    c.rgba = rgba;
    const bool ok = c.ok;
    calls.push_back(std::move(c));
    return ok;
}

void LogBackend::canvas_release(int id) {
    Call c;
    c.op = Call::CanvasRelease;
    c.id = id;
    calls.push_back(std::move(c));
    if (answers) answers->canvas_release(id);
}

void LogBackend::frame(bool painted) {
    Call c;
    c.op = Call::Frame;
    c.painted = painted;
    calls.push_back(std::move(c));
}

size_t LogBackend::count(Call::Op op) const {
    size_t n = 0;
    for (const Call& c : calls) n += c.op == op;
    return n;
}

template <class V> void walk(V& v, const At& at, const Call& a, const Call& b) {
    v.integer(At{&at, "op"}, a.op, b.op);
    v.raw(At{&at, "families"}, a.families, b.families);
    v.raw(At{&at, "features"}, a.features, b.features);
    v.raw(At{&at, "text"}, a.text, b.text);
    v.raw(At{&at, "url"}, a.url, b.url);
    v.f32(At{&at, "size"}, a.size, b.size);
    v.integer(At{&at, "weight"}, uint32_t(a.weight), uint32_t(b.weight));
    v.integer(At{&at, "italic"}, a.italic, b.italic);
    v.integer(At{&at, "font"}, uint32_t(a.font), uint32_t(b.font));
    walk(v, At{&at, "metrics_in"}, a.metrics_in, b.metrics_in);
    walk(v, At{&at, "metrics"}, a.metrics, b.metrics);
    v.f32(At{&at, "width"}, a.width, b.width);
    v.integer(At{&at, "id"}, uint32_t(a.id), uint32_t(b.id));
    v.integer(At{&at, "x"}, uint32_t(a.x), uint32_t(b.x));
    v.integer(At{&at, "y"}, uint32_t(a.y), uint32_t(b.y));
    v.integer(At{&at, "w"}, uint32_t(a.w), uint32_t(b.w));
    v.integer(At{&at, "h"}, uint32_t(a.h), uint32_t(b.h));
    v.f32(At{&at, "w_in"}, a.w_in, b.w_in);
    v.f32(At{&at, "h_in"}, a.h_in, b.h_in);
    v.f32(At{&at, "image_w"}, a.image_w, b.image_w);
    v.f32(At{&at, "image_h"}, a.image_h, b.image_h);
    v.integer(At{&at, "ok"}, a.ok, b.ok);
    v.integer(At{&at, "painted"}, a.painted, b.painted);
    v.integer(At{&at, "rgba_in"}, a.rgba_in, b.rgba_in);
    walk(v, At{&at, "rgba"}, a.rgba, b.rgba);
    walk(v, At{&at, "ops"}, a.ops, b.ops);
}

/// Whether two logs hold the same calls in the same order member by member, each batch's payloads shared alike within it; `why` names the first difference.
bool same_calls(const std::vector<Call>& a, const std::vector<Call>& b, std::string& why) {
    const At log{nullptr, "call"};
    for (size_t i = 0; i < a.size() || i < b.size(); i++) {
        if (i == a.size() || i == b.size()) {
            why = "call " + std::to_string(i) + " is in one log only, of " + std::to_string(a.size()) + " and " + std::to_string(b.size());
            return false;
        }
        Equal e;
        walk(e, At{&log, "", i, At::Item}, a[i], b[i]);
        if (e.differences) {
            why = e.first;
            return false;
        }
        if (sharing(a[i].ops) != sharing(b[i].ops)) {
            why = "call " + std::to_string(i) + " shares its batch's payloads otherwise";
            return false;
        }
    }
    why.clear();
    return true;
}

} // namespace
