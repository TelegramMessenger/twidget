// The whole renderer with the probes of renderer_probe.h at its calls into the document and the container (renderer_oracle.cmake's copy of renderer.cpp, included whole so that its Node is complete here), driven by scripts for rust/crates/renderer: scripts, the tape and the transcript are laid out in that crate's tests/common/mod.rs.
#include "renderer.cpp"
#include "wire.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <map>
#include <memory>
#include <string>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

namespace ro {

using twidget::Json;
using twlite::Backend;
using twlite::CanvasClip;
using twlite::CanvasOp;
using twlite::Clock;
using twlite::FontMetrics;
using twlite::Rect;
using twlite::Renderer;
using twlite::Stop;

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
    int32_t i32() { return raw<int32_t>(); }
    uint32_t u32() { return raw<uint32_t>(); }
    uint64_t u64() { return raw<uint64_t>(); }
    float f32() { return raw<float>(); }
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
    void str(std::string_view v) {
        raw(uint32_t(v.size()));
        s.append(v);
    }
    void rect(const Rect& r) {
        raw(r.x);
        raw(r.y);
        raw(r.w);
        raw(r.h);
    }
};

uint64_t fnv64(std::string_view v, uint64_t h = 1469598103934665603ull) {
    for (unsigned char c : v) h = (h ^ c) * 1099511628211ull;
    return h;
}

void canonical(float& v) {
    if (std::isnan(v)) v = std::numeric_limits<float>::quiet_NaN();
}

void canonical(std::vector<float>& list) {
    for (float& v : list) canonical(v);
}

// As canvas_oracle.cpp writes a batch: every NaN as one, shared clip lists still shared.
std::vector<CanvasOp> canonical(std::vector<CanvasOp> ops) {
    std::map<const void*, std::shared_ptr<const std::vector<CanvasClip>>> clips;
    for (CanvasOp& op : ops) {
        canonical(op.path.points);
        for (float& v : op.paint.args) canonical(v);
        for (Stop& s : op.paint.stops) canonical(s.offset);
        for (float& v : op.matrix) canonical(v);
        canonical(op.dashes);
        for (float* v : {&op.line_width, &op.miter_limit, &op.dash_offset, &op.alpha, &op.shadow_blur, &op.shadow_x, &op.shadow_y, &op.x, &op.y,
                         &op.scale_x, &op.text_dx, &op.sx, &op.sy, &op.sw, &op.sh, &op.dx, &op.dy, &op.dw, &op.dh})
            canonical(*v);
        if (op.clips) {
            auto& made = clips[op.clips.get()];
            if (!made) {
                auto copy = std::make_shared<std::vector<CanvasClip>>(*op.clips);
                for (CanvasClip& c : *copy) canonical(c.path.points);
                made = copy;
            }
            op.clips = made;
        }
    }
    return ops;
}

struct StepClock : Clock {
    double ms = 0;
    double now_ms() override { return ms; }
};

// The handles of canvas fonts start here: the renderer's own font calls are told from the container's by their handle.
constexpr int CANVAS_FONTS = 1 << 20;

// core_tests.cpp's StubBackend, with images that decode as their URL says ("fail" never, "async" later, any other at 24 x 16), canvas fonts numbered on their own, and every call the renderer makes itself written down.
struct Stub : Backend {
    int next = 0, next_canvas_font = CANVAS_FONTS;
    Out* log = nullptr;
    // A script's 'B' step: in mode 1 a document font's widths grow a quarter pixel a call, five calls round, so the container's width cache and every measure show.
    int mode = 0;
    uint64_t measured = 0;
    void metrics(float size, FontMetrics& out) {
        out.ascent = size * 0.8f;
        out.descent = size * 0.2f;
        out.height = size * 1.2f;
        out.x_height = size * 0.5f;
        out.zero_width = 8;
    }
    int create_font(const std::string&, float size, int, bool, const std::string&, FontMetrics& out) override {
        metrics(size, out);
        return ++next;
    }
    int create_canvas_font(const std::string& family, float size, int weight, bool italic, FontMetrics& out) override {
        metrics(size, out);
        int handle = ++next_canvas_font;
        if (log) {
            log->u8('F');
            log->str(family);
            log->raw(size);
            log->raw(int32_t(weight));
            log->u8(italic);
            log->raw(int32_t(handle));
        }
        return handle;
    }
    void delete_font(int font) override {
        if (log && font > CANVAS_FONTS) {
            log->u8('D');
            log->raw(int32_t(font));
        }
    }
    float text_width(int font, const std::string& text) override {
        if (log && font > CANVAS_FONTS) {
            log->u8('W');
            log->raw(int32_t(font));
            log->str(text);
        }
        if (mode == 1 && font <= CANVAS_FONTS) return 8.0f * float(text.size()) + float(measured++ % 5) * 0.25f;
        return 8.0f * float(text.size());
    }
    bool load_image(int, const std::string& url, float& w, float& h) override {
        if (url.find("fail") != std::string::npos) return false;
        if (url.find("async") != std::string::npos) {
            w = h = 0;
            return true;
        }
        w = 24;
        h = 16;
        return true;
    }
    void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override {
        if (!log) return;
        std::string bytes;
        const bool encoded = twlite::wire::encode(canonical(ops), twlite::wire::Caps{}, bytes);
        log->u8('R');
        log->raw(int32_t(id));
        log->raw(int32_t(w));
        log->raw(int32_t(h));
        log->raw(uint32_t(ops.size()));
        log->u8(encoded ? 1 : 0);
        log->raw(fnv64(bytes));
    }
    bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>&) override {
        if (log) {
            log->u8('G');
            for (int v : {id, x, y, w, h}) log->raw(int32_t(v));
        }
        return false;
    }
    void canvas_release(int id) override {
        if (!log) return;
        log->u8('X');
        log->raw(int32_t(id));
    }
};

} // namespace ro

namespace twlite {

// Renderer's friend in the oracle's copy of renderer.h: reads what the renderer holds.
struct RendererOracle {
    static uint64_t tree(const Renderer& r, uint32_t max_id) {
        uint64_t h = 1469598103934665603ull;
        std::string b;
        for (uint32_t id = 1; id <= max_id; id++) {
            const Renderer::Node* n = r.node(id);
            if (!n) continue;
            b.clear();
            b.append(reinterpret_cast<const char*>(&id), sizeof id);
            b.push_back(char(n->kind));
            b += n->tag;
            b.push_back('\0');
            for (const auto& [name, value] : n->attrs) {
                b += name;
                b.push_back('=');
                b += value;
                b.push_back('\0');
            }
            b.push_back('\1');
            b += n->data;
            h = ro::fnv64(b, h);
        }
        return h;
    }

    // The input's state, as events_oracle writes it.
    static void input(Renderer& r, ro::Out& o) {
        o.raw(uint32_t(r.touches_.size()));
        for (const auto& [pointer, t] : r.touches_) {
            o.raw(int32_t(pointer));
            o.raw(t.x0);
            o.raw(t.y0);
            o.raw(t.x);
            o.raw(t.y);
            o.raw(int64_t(t.target));
            o.rect(t.target_rect);
            o.u8(t.moved);
            o.u8(t.primary);
            o.raw(int32_t(t.pointer));
            o.u8(t.mouse);
            o.raw(int32_t(t.modifiers));
            o.raw(int32_t(t.button));
            o.u8(t.held);
            o.raw(int64_t(t.capture));
            o.raw(t.sent_x);
            o.raw(t.sent_y);
            o.u8(t.finger);
        }
        o.raw(int64_t(r.hover_));
        o.raw(r.hover_x_);
        o.raw(r.hover_y_);
        o.raw(int32_t(r.click_count_));
        o.raw(int32_t(r.arbiter_pointer_));
        auto deadline = r.hold_deadline();
        o.u8(deadline.has_value());
        o.raw(deadline.value_or(0.0));
        o.u8(r.bubble_);
        o.u8(r.bubble_touch_);
        o.u8(r.rtl_);
        o.raw(uint32_t(r.pending_moves_.size()));
        for (const auto& [key, ev] : r.pending_moves_) {
            o.str(key);
            o.str(ev.dump());
        }
        for (const auto* set : {&r.interest_, &r.prevent_}) {
            std::vector<std::string> sorted(set->begin(), set->end());
            std::sort(sorted.begin(), sorted.end());
            o.raw(uint32_t(sorted.size()));
            for (const std::string& k : sorted) o.str(k);
        }
        o.str(r.editor_json());
        o.str(r.editing_initial_);
        o.raw(uint64_t(r.editor_generation_));
        o.raw(int64_t(r.selection_.anchor.owner));
        o.raw(uint64_t(r.selection_.anchor.offset));
        o.raw(int64_t(r.selection_.focus.owner));
        o.raw(uint64_t(r.selection_.focus.offset));
        o.raw(int32_t(r.selection_.pointer));
        o.raw(r.last_input_);
        o.raw(uint32_t(r.resize_watch_.size()));
        for (const auto& [id, size] : r.resize_watch_) {
            o.raw(int64_t(id));
            o.raw(size.first);
            o.raw(size.second);
        }
        std::map<int, std::vector<int64_t>> waiting(r.waiting_images_.begin(), r.waiting_images_.end());
        o.raw(uint32_t(waiting.size()));
        for (const auto& [image, ids] : waiting) {
            o.raw(int32_t(image));
            o.raw(uint32_t(ids.size()));
            for (int64_t id : ids) o.raw(id);
        }
        const auto& key = r.runs_key_;
        o.raw(uint64_t(uintptr_t(std::get<0>(key))));
        o.raw(uint64_t(std::get<1>(key)));
        o.raw(uint64_t(std::get<2>(key)));
        o.raw(uint64_t(std::get<3>(key)));
        o.raw(uint32_t(r.runs_.size()));
        for (const auto& run : r.runs_) {
            o.u8(run.drawn);
            o.raw(int64_t(run.layer));
            for (float v : {run.shown.a, run.shown.b, run.shown.c, run.shown.d, run.shown.e, run.shown.f}) o.raw(v);
            o.raw(uint32_t(run.advances.size()));
        }
        o.raw(uint64_t(uintptr_t(r.selectable_key_.first)));
        o.raw(uint64_t(r.selectable_key_.second));
        o.u8(r.selectable_);
    }

    // What the builds, the reports, the animations and the canvas fonts left.
    static void build(Renderer& r, ro::Out& o) {
        o.raw(r.width_);
        o.raw(r.dpr_);
        o.raw(int32_t(r.vp_height_));
        o.raw(r.doc_height_);
        o.raw(r.max_height_);
        o.u8(r.dark_);
        o.u8(r.dirty_);
        o.u8(r.stale_);
        o.u8(r.document_changed_);
        o.u8(r.html_stale_);
        o.raw(ro::fnv64(r.html_));
        o.raw(uint32_t(r.html_.size()));
        for (double v : r.last_size_) o.raw(v);
        o.u8(r.track_resize_);
        o.u8(r.prefers_bottom_);
        std::map<int64_t, Renderer::Edges> edges(r.edges_.begin(), r.edges_.end());
        o.raw(uint32_t(edges.size()));
        for (const auto& [id, e] : edges) {
            o.raw(int64_t(id));
            o.raw(e.top);
            o.raw(e.bottom);
        }
        o.raw(r.edges_height_);
        o.raw(uint64_t(r.edges_layouts_));
        o.u8(r.has_resize_);
        const auto& d = r.resize_;
        o.raw(d.old_height);
        o.raw(d.new_height);
        o.raw(d.unchanged_top);
        o.raw(d.unchanged_bottom);
        o.u8(d.has_pointer);
        o.raw(d.pointer_from);
        o.raw(d.pointer_to);
        o.u8(d.keeps_bottom);
        o.raw(r.resize_at_);
        o.raw(uint64_t(r.resize_serial_));
        o.raw(uint64_t(r.resize_reported_));
        o.raw(int64_t(r.press_id_));
        o.raw(r.press_y_);
        o.raw(r.press_at_);
        o.u8(r.patch_log_.valid);
        // An invalid log can hold nodes removed since: only a valid one's are read.
        const bool log = r.patch_log_.valid;
        o.raw(uint32_t(log ? r.patch_log_.attributes.size() : 0));
        for (const auto& [n, names] : r.patch_log_.attributes) {
            if (!log) break;
            o.raw(int64_t(n->id));
            o.raw(uint32_t(names.size()));
            for (const std::string& name : names) o.str(name);
        }
        o.raw(uint32_t(log ? r.patch_log_.texts.size() : 0));
        for (const auto* n : r.patch_log_.texts) {
            if (!log) break;
            o.raw(int64_t(n->id));
        }
        std::vector<int64_t> touched(r.touched_.begin(), r.touched_.end());
        std::sort(touched.begin(), touched.end());
        o.raw(uint32_t(touched.size()));
        for (int64_t id : touched) o.raw(id);
        o.u8(r.touched_all_);
        o.u8(r.patching_);
        const auto& s = r.stats_;
        o.raw(s.serialize_ms);
        o.raw(s.parse_ms);
        o.raw(int32_t(s.passes));
        o.raw(uint64_t(s.html_bytes));
        for (int v : {s.builds, s.patched, s.throttled, s.in_place, s.scripted, s.reused}) o.raw(int32_t(v));
        o.raw(uint64_t(s.names));
        o.raw(uint64_t(s.name_bytes));
        o.raw(uint32_t(r.pending_violations_.size()));
        for (const std::string& v : r.pending_violations_) o.str(v);
        o.u8(r.frozen_);
        o.str(r.frozen_reason_);
        o.u8(r.frozen_fatal_);
        o.u8(r.layout_stopped_);
        o.u8(r.paint_stopped_);
        o.str(r.theme_variables_json());
        o.raw(ro::fnv64(r.master_css()));
        const Container& c = *r.container_;
        o.u8(c.doc_ != nullptr);
        o.raw(int32_t(c.vp_height_));
        o.raw(c.rendered_width_);
        o.u8(c.viewport_units_);
        o.u8(c.patched_);
        o.raw(uint64_t(c.layouts_));
        o.raw(uint64_t(c.built_fonts_));
        o.raw(uint32_t(r.overrides_.size()));
        for (const auto& [id, values] : r.overrides_) {
            o.raw(int64_t(id));
            o.raw(uint32_t(values.size()));
            for (const auto& [k, v] : values) {
                o.str(k);
                o.str(v);
            }
        }
        o.raw(ro::fnv64(r.sheets_key_));
        o.raw(uint64_t(r.declared_generation_));
        o.u8(r.animating_);
        o.raw(uint32_t(r.pending_events_.size()));
        for (const Json& m : r.pending_events_) o.str(m.dump());
        std::map<std::string, int> fonts;
        for (const auto& [font, entry] : r.canvas_fonts_) fonts.emplace(font, entry.first);
        o.raw(uint32_t(fonts.size()));
        for (const auto& [font, handle] : fonts) {
            o.str(font);
            o.raw(int32_t(handle));
        }
        o.raw(uint32_t(r.canvas_font_order_.size()));
        for (int handle : r.canvas_font_order_) o.raw(int32_t(handle));
        o.raw(r.first_charge_);
        o.raw(r.throttled_until_);
        o.raw(uint32_t(r.charges_.size()));
        o.raw(int32_t(r.meter_depth_));
        o.raw(int32_t(r.name_meter_depth_));
        o.u8(r.scripted_pending_);
        o.raw(uint32_t(r.canvases_.size()));
        for (const auto& [node, canvas] : r.canvases_) {
            o.raw(int64_t(node));
            o.raw(int32_t(canvas->id()));
            o.raw(int32_t(canvas->width()));
            o.raw(int32_t(canvas->height()));
        }
    }

    static uint64_t generation(const Renderer& r) { return r.editor() ? r.editor()->generation : 0; }

    // A display list as the transcript keeps it: whether the wire takes it, the digest and length of its wire form.
    static void list(const twlite::DisplayList& l, ro::Out& o) {
        std::string bytes;
        o.u8(twlite::wire::encode(l, twlite::wire::Caps{}, bytes) ? 1 : 0);
        o.raw(ro::fnv64(bytes));
        o.raw(uint32_t(bytes.size()));
    }
    static void radii(const twlite::Radii& k, ro::Out& o) {
        for (float v : {k.tlx, k.tly, k.trx, k.try_, k.brx, k.bry, k.blx, k.bly}) o.raw(v);
    }
    // A track by the digest of its fields and keys, every float by its bits.
    static void track(const twlite::LayerTrack& t, ro::Out& o) {
        ro::Out k;
        k.raw(t.start);
        k.raw(t.duration);
        k.u8(t.repeat);
        for (const auto& key : t.keys) {
            k.raw(key.t);
            for (float v : {key.matrix.a, key.matrix.b, key.matrix.c, key.matrix.d, key.matrix.e, key.matrix.f}) k.raw(v);
            k.raw(key.opacity);
        }
        o.raw(uint32_t(t.keys.size()));
        o.raw(ro::fnv64(k.s));
    }
    static void composition(const twlite::Composition& c, ro::Out& o) {
        o.raw(uint64_t(c.version));
        o.raw(uint32_t(c.layers.size()));
        for (const auto& l : c.layers) {
            o.raw(int64_t(l.id));
            o.rect(l.bounds);
            o.raw(uint64_t(l.content_version));
            list(l.content, o);
            o.raw(l.raster_scale);
            o.u8(l.has_clip);
            o.rect(l.clip);
            radii(l.clip_radii, o);
            o.raw(uint64_t(l.track_version));
            track(l.track, o);
            o.raw(int64_t(l.parent));
        }
    }
    template <class Set> static void sorted_ids(const Set& set, ro::Out& o) {
        std::vector<int64_t> ids(set.begin(), set.end());
        std::sort(ids.begin(), ids.end());
        o.raw(uint32_t(ids.size()));
        for (int64_t id : ids) o.raw(id);
    }

    // What stage 4.5 left: the layers, the scripted, SVG and canvas streaks, the last paint and its damage, the heavy boxes.
    static void layers(Renderer& r, ro::Out& o) {
        o.u8(r.compositing_);
        o.raw(uint32_t(r.promoted_.size()));
        for (const auto& [id, p] : r.promoted_) {
            o.raw(int64_t(id));
            o.rect(p.box);
            o.rect(p.reach);
            o.raw(p.raster_scale);
            o.u8(p.has_clip);
            o.rect(p.clip);
            radii(p.clip_radii, o);
            o.raw(uint64_t(p.signature));
            o.raw(uint64_t(p.track_version));
            track(p.track, o);
            o.raw(uint64_t(p.order));
            o.u8(p.scripted);
            std::map<std::string, std::string> base(p.base.begin(), p.base.end());
            o.raw(uint32_t(base.size()));
            for (const auto& [k, v] : base) {
                o.str(k);
                o.str(v);
            }
            o.u8(p.shimmer);
            list(p.tile, o);
            o.raw(uint64_t(p.tile_version));
            o.u8(p.svg);
            o.u8(p.canvas);
            o.raw(int64_t(p.parent));
            o.u8(p.found_clip);
            o.rect(p.found_clip_rect);
        }
        sorted_ids(r.layer_ids_, o);
        composition(r.composition_, o);
        o.raw(r.wake_at_);
        o.raw(uint64_t(r.composition_key_));
        o.raw(uint64_t(r.next_version_));
        std::map<int64_t, int> svg_streaks(r.svg_streaks_.begin(), r.svg_streaks_.end());
        o.raw(uint32_t(svg_streaks.size()));
        for (const auto& [id, n] : svg_streaks) {
            o.raw(int64_t(id));
            o.raw(int32_t(n));
        }
        sorted_ids(r.svg_layers_, o);
        std::map<int, uint64_t> draws(r.canvas_draws_.begin(), r.canvas_draws_.end());
        o.raw(uint32_t(draws.size()));
        for (const auto& [canvas, n] : draws) {
            o.raw(int32_t(canvas));
            o.raw(uint64_t(n));
        }
        sorted_ids(r.canvas_layers_, o);
        std::map<int64_t, Renderer::ScriptedHit> hits(r.scripted_hits_.begin(), r.scripted_hits_.end());
        o.raw(uint32_t(hits.size()));
        for (const auto& [id, h] : hits) {
            o.raw(int64_t(id));
            o.raw(int32_t(h.count));
            o.raw(uint64_t(h.batch));
            o.raw(h.at);
        }
        o.raw(uint64_t(r.ops_batch_));
        sorted_ids(r.scripted_candidates_, o);
        o.u8(r.composition_only_);
        list(r.painted_list_, o);
        o.u8(r.painted_list_valid_);
        o.raw(uint64_t(uintptr_t(std::get<0>(r.painted_key_))));
        o.raw(uint32_t(std::get<1>(r.painted_key_)));
        o.raw(uint64_t(std::get<2>(r.painted_key_)));
        o.raw(uint32_t(r.painted_.entries.size()));
        for (const auto& e : r.painted_.entries) {
            o.raw(uint64_t(e.key));
            o.rect(e.ink);
            o.raw(int32_t(e.canvas));
            o.u8(e.backdrop);
        }
        o.u8(r.painted_valid_);
        o.raw(uint32_t(r.painted_size_.first));
        o.raw(uint32_t(r.painted_size_.second));
        o.raw(uint32_t(r.damage_.size()));
        for (const auto& d : r.damage_) o.rect(d);
        o.u8(r.damage_valid_);
        std::vector<int> drawn(r.canvases_drawn_.begin(), r.canvases_drawn_.end());
        std::sort(drawn.begin(), drawn.end());
        o.raw(uint32_t(drawn.size()));
        for (int c : drawn) o.raw(int32_t(c));
        o.raw(uint64_t(r.painted_layouts_));
        o.u8(r.paint_on_);
        o.u8(r.paint_reports_);
        o.raw(int32_t(r.paint_reported_.first));
        o.raw(int32_t(r.paint_reported_.second));
        o.u8(r.paint_scan_);
        for (int v : {r.paint_.heavy(), r.paint_.backdrop(), r.paint_.peak_heavy(), r.paint_.peak_backdrop()}) o.raw(int32_t(v));
        o.raw(uint64_t(r.motion_hash_));
        o.u8(r.motion_stale_);
        o.raw(uint32_t(r.motions_.size()));
        for (const auto& m : r.motions_) {
            o.raw(int64_t(m.id));
            o.u8(uint8_t(m.kind));
            // Its name is a view into the animator, which can have let it go since: only the length is the motion's own.
            o.raw(uint32_t(m.name.size()));
            o.raw(int64_t(m.aid));
            o.u8(m.paint.heavy);
            o.u8(m.paint.backdrop);
        }
        o.raw(uint32_t(r.motion_counted_.size()));
        for (const auto& c : r.motion_counted_) {
            o.raw(int64_t(c.key));
            o.str(c.id);
            o.u8(c.paint.heavy);
            o.u8(c.paint.backdrop);
        }
    }
};

} // namespace twlite

namespace ro {

using twlite::RendererOracle;

void messages(const std::vector<Json>& out, Out& o) {
    o.raw(uint32_t(out.size()));
    for (const Json& m : out) o.str(m.dump());
}

void script(In& in, Out& o) {
    const double width = in.f64(), dpr = in.f64();
    const bool dark = in.u8() != 0;
    const double t0 = in.f64();
    const uint32_t max_id = in.u32();
    Stub backend;
    StepClock clock;
    clock.ms = t0;
    twp::tape = twp::Tape{};
    twp::tape.on = true;
    Out log;
    backend.log = &log;
    auto r = std::make_unique<Renderer>(width, dpr, dark, backend, &clock);
    Out transcript;
    transcript.str(log.s);
    log.s.clear();
    RendererOracle::build(*r, transcript);
    RendererOracle::layers(*r, transcript);
    const uint32_t steps = in.u32();
    for (uint32_t i = 0; i < steps && in.at < in.s.size(); i++) {
        const uint8_t kind = in.u8();
        clock.ms = in.f64();
        twp::step(i);
        transcript.u8(kind);
        std::vector<Json> out;
        Out extra;
        switch (kind) {
        case 'o':
        case 'm': {
            auto parsed = Json::parse(in.str());
            extra.u8(parsed ? 1 : 0);
            if (!parsed) break;
            Json msg = kind == 'o' ? Json::object().set("type", "ops").set("ops", std::move(*parsed)) : std::move(*parsed);
            out = r->receive(msg);
            break;
        }
        case 'd': r->set_theme(in.u8() != 0, out); break;
        case 'w': r->set_width(in.f64(), out); break;
        case 'M': r->set_max_height(in.f64(), out); break;
        case 'C': r->set_compositing(in.u8() != 0); break;
        case 't': {
            const int action = in.i32();
            const double x = in.f64(), y = in.f64();
            const int pointer = in.i32();
            const bool primary = in.u8() != 0, mouse = in.u8() != 0;
            const int modifiers = in.i32(), button = in.i32();
            out = r->touch(action, x, y, pointer, primary, mouse, modifiers, button);
            break;
        }
        case 'h': {
            const int action = in.i32();
            const double x = in.f64(), y = in.f64();
            const int modifiers = in.i32();
            out = r->hover(action, x, y, modifiers);
            break;
        }
        case 'c': {
            const double x = in.f64(), y = in.f64();
            auto claim = r->claim_at(x, y);
            extra.u8(uint8_t((claim.x ? 1 : 0) | (claim.y ? 2 : 0) | (claim.hold ? 4 : 0)));
            break;
        }
        case 'e': {
            const uint8_t mode = in.u8();
            const uint64_t literal = in.u64();
            const std::string text = in.str(), type = in.str();
            const uint64_t generation = mode == 0 ? RendererOracle::generation(*r) : literal;
            out = r->edit(generation, text, type);
            break;
        }
        case 'E': out = r->end_editing(); break;
        case 'x': extra.u8(r->clear_selection() ? 1 : 0); break;
        case 'i': {
            const int id = in.i32();
            const float w = in.f32(), h = in.f32();
            out = r->image_ready(id, w, h);
            break;
        }
        case 'W': {
            const double x = in.f64(), y = in.f64(), dx = in.f64(), dy = in.f64();
            const int mode = in.i32(), modifiers = in.i32();
            out = r->wheel(x, y, dx, dy, mode, modifiers);
            break;
        }
        case 'k': {
            const std::string type = in.str(), key = in.str(), code = in.str();
            const bool repeat = in.u8() != 0;
            const int modifiers = in.i32();
            out = r->key(type, key, code, repeat, modifiers);
            break;
        }
        case 'f': out = r->tick(); break;
        case 'l': extra.str(r->last_html()); break;
        case 'p': {
            RendererOracle::list(r->paint(), extra);
            std::vector<Rect> damage;
            const bool valid = r->damage(damage);
            extra.u8(valid);
            extra.raw(uint32_t(damage.size()));
            for (const Rect& d : damage) extra.rect(d);
            extra.str(r->damage_json());
            RendererOracle::composition(r->composition(), extra);
            break;
        }
        case 'q': {
            extra.str(r->cursor());
            extra.u8(r->has_selection());
            extra.str(r->selected_text());
            extra.str(r->selectable_text_json());
            break;
        }
        case 'T': r->trim(); break;
        case 'r': {
            const bool enabled = in.u8() != 0, prefers_bottom = in.u8() != 0;
            r->set_resize_tracking(enabled, prefers_bottom);
            break;
        }
        case 'P': r->set_patching(in.u8() != 0); break;
        case 'g': r->charge(in.f64()); break;
        case 'B':
            backend.mode = in.u8();
            backend.measured = 0;
            break;
        case 'n': {
            extra.u8(r->wants_tick());
            extra.raw(r->next_tick_in());
            extra.str(r->master_css());
            extra.str(r->theme_variables_json());
            extra.str(r->editor_json());
            extra.raw(r->layouts());
            extra.raw(uint32_t(r->px_width()));
            extra.raw(uint32_t(r->px_height()));
            break;
        }
        default: std::exit(4);
        }
        messages(out, transcript);
        transcript.s += extra.s;
        transcript.str(log.s);
        log.s.clear();
        RendererOracle::input(*r, transcript);
        RendererOracle::build(*r, transcript);
        RendererOracle::layers(*r, transcript);
        transcript.raw(RendererOracle::tree(*r, max_id));
    }
    twp::tape.on = false;
    backend.log = nullptr;
    r.reset();
    o.raw(uint32_t(twp::tape.bytes.size()));
    o.s += twp::tape.bytes;
    o.s += transcript.s;
    twp::tape = twp::Tape{};
}

} // namespace ro

int main() {
    using namespace ro;
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
        if (op != 'S') return 3;
        In in{bytes};
        Out o;
        script(in, o);
        const uint32_t size = uint32_t(o.s.size());
        const unsigned char head[5] = {1, uint8_t(size), uint8_t(size >> 8), uint8_t(size >> 16), uint8_t(size >> 24)};
        std::fwrite(head, 1, 5, stdout);
        if (size) std::fwrite(o.s.data(), 1, size, stdout);
    }
    std::fflush(stdout);
    return 0;
}
