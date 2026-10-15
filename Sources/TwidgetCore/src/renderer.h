// The renderer trust domain of widget-sandbox's docs/protocol.md on litehtml:
// validated RenderOps in, layout answers and WidgetEvents out, a DisplayList on
// request. litehtml has no DOM mutation API, so the ops maintain a shadow tree
// here; every batch serialises it to HTML, which the fork patches into the live
// document where it can and parses afresh where it cannot. Nothing here parses
// untrusted HTML (the string is generated from validated ops) or runs script.
//
// One Renderer per widget, every call on the platform's main thread (the glue
// guarantees it), so the document needs no lock. Coordinates are CSS px.
#pragma once

#include "animations.h"
#include "backend.h"
#include "canvas.h"
#include "compositor.h"
#include "display_list.h"
#include "twidget/json.h"

#include <array>
#include <cstdint>
#include <limits>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace twlite {

class Container;

/// Timings of the last rebuild (serialise → parse+layout, per pass).
struct BuildStats {
    double serialize_ms = 0, parse_ms = 0;
    int passes = 0;
    size_t html_bytes = 0;
    int builds = 0;
    /// Builds that patched the previous document instead of parsing a new one (TW_LITEHTML_TWIDGET).
    int patched = 0;
    /// Of those, builds that went into the document without serialising it (ops patched in place).
    int in_place = 0;
};

class Renderer : private CanvasHost {
public:
    /// `backend` is owned by the platform layer and must outlive the renderer.
    Renderer(double width, double dpr, bool dark, Backend& backend);
    ~Renderer();
    Renderer(const Renderer&) = delete;
    Renderer& operator=(const Renderer&) = delete;

    /// Host → renderer message (parsed JSON). Returns renderer → host messages.
    std::vector<twidget::Json> receive(const twidget::Json& msg);
    void set_width(double width, std::vector<twidget::Json>& out);
    /// The tallest the widget may grow, in CSS px (900 unless the host says otherwise); past it the
    /// document is cut off. A chat that scrolls tall widgets sets its own bound.
    void set_max_height(double height, std::vector<twidget::Json>& out);
    /// The chat theme changed: `prefers-color-scheme` inside the document follows.
    void set_theme(bool dark, std::vector<twidget::Json>& out);
    /// A pointer from the host view, in CSS px. `action`: 0 down, 1 move, 2 up, 3 cancel. A mouse
    /// sends no touch events; `modifiers` is bit0 alt, bit1 ctrl, bit2 meta, bit3 shift.
    std::vector<twidget::Json> touch(int action, double x, double y, int pointer, bool primary, bool mouse = false, int modifiers = 0);
    /// A mouse over the view with no button down (`action` 0), or leaving it (1): hover state and
    /// the over/out/enter/leave events.
    std::vector<twidget::Json> hover(int action, double x, double y, int modifiers = 0);
    /// A wheel or trackpad scroll over the view. `mode`: 0 pixels, 1 lines, 2 pages.
    std::vector<twidget::Json> wheel(double x, double y, double dx, double dy, int mode, int modifiers = 0);
    /// `type` keydown or keyup, with DOM `key` and `code`; the target is the body (no focus model).
    std::vector<twidget::Json> key(const std::string& type, const std::string& key, const std::string& code, bool repeat, int modifiers);
    /// One display frame: coalesced move events go out and animations advance. Returns renderer → host messages.
    std::vector<twidget::Json> tick();
    /// Something waits for the next frame: a running animation or a coalesced move.
    bool wants_tick() const;
    /// When `wants_tick()` is false: seconds until `tick()` is due anyway (an animation event, the
    /// end of a layer's track); infinity when nothing is scheduled.
    double next_tick_in() const;

    /// Lets the platform's compositor run transform and opacity animations (compositor.h). Off by
    /// default: a platform that never turns it on gets every frame painted, as before.
    void set_compositing(bool enabled);
    /// The layers that go above the display list `paint()` returned last.
    const Composition& composition() const { return composition_; }
    /// The renderer clock, in seconds: what `LayerTrack::start` is on.
    static double clock() { return now_s(); }
    /// A backend that decodes images asynchronously (the web) reports the size of image `id` here;
    /// the document is rebuilt with it. Returns renderer → host messages (a `size`, when it changed).
    std::vector<twidget::Json> image_ready(int id, float w, float h);

    /// Viewport in CSS px (the height the document converged to) and in physical px.
    double width() const { return width_; }
    int viewport_height() const { return vp_height_; }
    double dpr() const { return dpr_; }
    uint32_t px_width() const;
    uint32_t px_height() const;
    bool dirty() const { return dirty_; }
    /// Records the current document into a display list (clears `dirty`).
    DisplayList paint();
    /// Where the last `paint()` differs from the one before, in CSS px; false when everything must be
    /// painted (the first paint, a new size). Empty when nothing moved.
    bool damage(std::vector<Rect>& out) const;
    /// Drops caches that are made again on demand, for memory pressure or a widget off screen; the
    /// next paint is a whole one.
    void trim();
    /// damage() as JSON for the C ABI: `null` (paint everything) or `[[x, y, w, h], ...]`.
    std::string damage_json() const;
    const BuildStats& stats() const { return stats_; }
    /// Off: every batch serialises the document again. For checks that patching changes nothing.
    void set_patching(bool enabled) { patching_ = enabled; }
    /// Layouts of the document so far: full renders after a parse, a patch or a restyle that moved boxes.
    uint64_t layouts() const;
    /// The HTML of the last build (diagnostics).
    /// The document as HTML, built first when a batch is waiting for its frame, and serialised again
    /// when style patches changed it since the last build.
    const std::string& last_html() {
        if (stale_) settle(pending_events_);
        if (html_stale_) {
            html_ = serialize();
            html_stale_ = false;
        }
        return html_;
    }

private:
    friend class Container;
    struct Node;
    struct Touch {
        double x0, y0, x, y;
        int64_t target;
        Rect target_rect;
        bool moved;
        bool primary;
        int pointer;
        bool mouse;
        int modifiers;
        int64_t capture = 0;  // setPointerCapture: pointer events go here instead
    };
    /// What an input event carries besides its type and target.
    struct Input {
        double x = 0, y = 0;
        int pointer = 0;
        bool primary = true;
        bool mouse = false;
        int buttons = 0;
        int modifiers = 0;
        int detail = 0;
        int64_t related = 0;
    };

    // ops → shadow tree
    void apply_op(const twidget::Json& op, std::vector<twidget::Json>& out);
    Node* node(int64_t id) const;
    void adopt(int64_t id, Node* n, const twidget::Json& attrs);
    void detach(Node* n);
    void forget(Node* n);
    // shadow tree → HTML → litehtml
    std::string serialize() const;
    void serialize_node(const Node* n, std::string& out) const;
    /// `patch`: an ops batch, which may go into the document in place (patch_document).
    void rebuild(bool patch = false);
    /// Builds a document a coalesced batch left stale.
    void settle(std::vector<twidget::Json>& out);
    void report_size(std::vector<twidget::Json>& out);
    // queries & events
    twidget::Json answer(const twidget::Json& q);
    bool match_media(const std::string& query) const;
    int64_t widget_id_at(double x, double y, Rect* placement) const;
    bool wants_gesture(int64_t target) const;
    void set_capture(Touch& t, int64_t id, std::vector<twidget::Json>& out);
    /// The border box of element `id`, empty when it is not laid out.
    Rect box_of(int64_t id) const;
    void emit_event(const char* type, int64_t target, const Rect& target_rect, const Touch& t, int action, std::vector<twidget::Json>& out);
    twidget::Json event_json(const char* type, int64_t target, const Rect& target_rect, const Input& in) const;
    /// Sends `ev` if the sandbox listens for its type; move-type events wait for the next tick, the
    /// latest one per type and pointer.
    void dispatch(twidget::Json ev, std::vector<twidget::Json>& out);
    void flush_moves(std::vector<twidget::Json>& out);
    void flush_pending(std::vector<twidget::Json>& out);
    /// The sandbox wants events of `type` (it listens, or every widget gets them).
    bool listened(const char* type) const;
    void send_event(const char* type, int64_t target, const Input& in, std::vector<twidget::Json>& out);
    bool is_ancestor(int64_t ancestor, int64_t id) const;
    std::vector<int64_t> chain(int64_t id) const;
    // shadow tree → style attribute, with animation overrides
    void write_style(const Node* n, std::string& out) const;
    // animations
    static double now_s();
    void build_document(bool patch = false);
    // In-place patching (the Swift renderer's patch log): which attributes and sole texts the ops of
    // a batch changed, for the fork's `update_holes`, so the document is not serialised again.
    struct PatchLog {
        bool valid = true;
        std::map<const Node*, std::set<std::string>> attributes;
        std::set<const Node*> texts;
    };
    bool attached(const Node* n) const;
    void note_attribute(const Node* n, const std::string& name);
    void note_children(const Node* parent, const Node* child);
    /// The log into the document; false, leaving it as it was, when the log cannot say it all.
    bool patch_document();
    void animate();
    void apply_overrides(StyleOverrides sampled);
    void queue_animation_events();
    // compositor layers
    struct Promotion {
        Rect box;
        Rect reach;  // what the layer can cover over its whole track
        float raster_scale = 1;
        bool has_clip = false;
        Rect clip;
        Radii clip_radii;
        uint64_t signature = 0;
        uint64_t track_version = 0;
        LayerTrack track;
        size_t order = 0;
    };
    void update_composition(double now);
    /// Drops what the layers animate from `sampled`: the compositor shows it, the document must not.
    void strip_layers(StyleOverrides& sampled) const;
    // canvas
    Canvas* canvas_for(Node* n);
    void run_canvas(Canvas& canvas, const twidget::Json& cmds);
    void forget_canvas(int64_t id);
    int canvas_font(const std::string& css, FontMetrics& metrics) override;
    float canvas_text_width(int font, const std::string& text) override;
    bool canvas_source(int64_t node, int& image, int& canvas, float& w, float& h) override;
    int64_t canvas_allowance(int id) override;
    void freeze(const std::string& reason, bool fatal);

    std::unique_ptr<Container> container_;
    Backend& backend_;
    double width_;
    double dpr_;
    bool dark_;
    int vp_height_;
    double doc_height_ = 0;
    bool dirty_ = true;
    bool document_changed_ = false;
    bool stale_ = false;  // ops applied, the build waits for the next frame
    double last_input_ = -1e9;
    DisplayList paint_document();
    PaintSummary painted_;
    bool painted_valid_ = false;
    std::pair<uint32_t, uint32_t> painted_size_{0, 0};
    std::vector<Rect> damage_;
    bool damage_valid_ = false;
    std::unordered_set<int> canvases_drawn_;
    double max_height_ = 900;
    PatchLog patch_log_;
    // What changed since the last paint, for layers to keep their content: the elements patched, or
    // everything (a build, a layout, hover state).
    std::unordered_set<int64_t> touched_;
    bool touched_all_ = true;
    uint64_t painted_layouts_ = 0;
    bool patching_ = true;  // an op since the last build changed what serialize() writes
    mutable std::string html_;
    mutable bool html_stale_ = false;
    BuildStats stats_;
    std::vector<std::string> pending_violations_;

    /// Widget nodes by id. html/head/body are fixed nodes owned below and reachable through `aliases_` once adopted.
    std::unordered_map<int64_t, std::unique_ptr<Node>> nodes_;
    std::unordered_map<int64_t, Node*> aliases_;
    std::unique_ptr<Node> html_node_, head_node_, body_node_;
    std::unordered_set<std::string> interest_;
    std::unordered_set<std::string> prevent_;
    bool frozen_ = false;
    std::string frozen_reason_;
    bool frozen_fatal_ = false;
    std::array<double, 4> last_size_{};  // width, document height, viewport height, dpr as last reported
    uint64_t declared_generation_ = 0;
    std::unordered_map<int, Touch> touches_;
    int64_t hover_ = 0;
    double hover_x_ = 0, hover_y_ = 0;
    int click_count_ = 0;
    double last_click_ms_ = -1e9, last_click_x_ = 0, last_click_y_ = 0;
    std::vector<std::pair<std::string, twidget::Json>> pending_moves_;  // key "type:pointer", latest event
    std::vector<twidget::Json> pending_events_;

    Animator animator_;
    StyleOverrides overrides_;
    std::string sheets_key_;
    bool animating_ = false;  // inside animate(): its own rebuild must not re-enter

    bool compositing_ = false;
    std::map<int64_t, Promotion> promoted_;
    std::unordered_set<int64_t> layer_ids_;
    Composition composition_;
    double wake_at_ = std::numeric_limits<double>::infinity();
    uint64_t composition_key_ = 0;
    uint64_t next_version_ = 1;

    std::unordered_map<int64_t, std::unique_ptr<Canvas>> canvases_;
    std::unordered_map<std::string, std::pair<int, FontMetrics>> canvas_fonts_;
    std::string binary_;  // the binary payload of the query being answered (imageData), base64
};

/// Rewrites `prefers-color-scheme` media queries into width queries litehtml can
/// evaluate (`(min-width:0px)` = always, `(min-width:999999px)` = never), since
/// litehtml knows no colour-scheme feature. Exposed for the unit test.
std::string rewrite_color_scheme(const std::string& css, bool dark);

} // namespace twlite
