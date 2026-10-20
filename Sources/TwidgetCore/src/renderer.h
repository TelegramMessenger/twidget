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
#include <deque>
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
    /// Passes after which the render throttle held the widget back.
    int throttled = 0;
    /// Of those, builds that went into the document without serialising it (ops patched in place).
    int in_place = 0;
    /// Batches that only moved scripted layers: no build.
    int scripted = 0;
    /// Paints that handed out the last list again, only layers having moved.
    int reused = 0;
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
    /// The tallest the widget may grow, in CSS px (600, widget-sandbox's shared bound, unless the host
    /// says otherwise); past it the document is cut off.
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
    /// `type` keydown or keyup, with DOM `key` and `code`, aimed at the control being edited, or else
    /// at the body.
    std::vector<twidget::Json> key(const std::string& type, const std::string& key, const std::string& code, bool repeat, int modifiers);

    /// The text control being edited (the Swift renderer's WidgetEditorSpec): a platform covers it
    /// with its own editor, which takes the keyboard and the input method. A tap on a text input or
    /// a textarea starts editing, and a script's focus() too within a second of the user's input;
    /// the document stops drawing the control's text meanwhile.
    struct EditorSpec {
        int64_t id = 0;
        /// Changes whenever anything below does; edit() names the one it answers.
        uint64_t generation = 0;
        bool multiline = false;  // a textarea
        Rect frame;              // the content box, document coordinates
        bool has_clip = false;   // a clipping ancestor's padding box
        Rect clip;
        int font = 0;            // the backend's font, and what it was made from
        std::string font_family;
        float font_size = 16;
        int font_weight = 400;
        bool italic = false;
        Color color;
        int align = 0;           // 0 start, 1 center, 2 end
        std::string text, placeholder;
        std::string input_type;  // text, search, number, email, url, tel; empty for a textarea
        int max_length = -1;
        bool read_only = false;
        /// What a script's select() or setSelectionRange() asked for, in UTF-16 code units as the
        /// script counts them; -1 when it asked for nothing.
        int selection_start = -1, selection_end = -1;
    };
    /// Null when nothing is being edited.
    const EditorSpec* editor() const { return editing_.id ? &editing_ : nullptr; }
    /// The JSON of editor(): null, or the fields above by name.
    std::string editor_json() const;
    /// The platform's editor changed the text of `generation`'s control: the value goes into the
    /// document and an input event to the widget. A stale generation is ignored.
    std::vector<twidget::Json> edit(uint64_t generation, const std::string& text, const std::string& input_type = "insertText");
    /// The platform's editor let go (the user dismissed it, or it lost the keyboard): change if the
    /// value differs from when editing began, then blur and focusout.
    std::vector<twidget::Json> end_editing();
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

    /// Where the content went when the document changed height, for a host that keeps part of the
    /// widget still on screen while it resizes (the Swift renderer's WidgetResizeDiff). Edges are in
    /// CSS px of the document before the change.
    struct ResizeDiff {
        double old_height = 0, new_height = 0;
        /// How far down from the top, and up from the bottom, nothing moved.
        double unchanged_top = 0, unchanged_bottom = 0;
        /// A point of the element pressed last, before and after, when it was pressed recently.
        bool has_pointer = false;
        double pointer_from = 0, pointer_to = 0;
        /// The side to keep still: the bottom (the widget grows or shrinks upward) or the top.
        bool keeps_bottom = false;
    };
    /// Diffs each layout that changes the document's height. Off by default: it measures every element
    /// after each layout. `prefers_bottom` is the side to keep when the content does not decide.
    void set_resize_tracking(bool enabled, bool prefers_bottom = false);
    /// The last height change, null before one; the size message carries it too, as `resize`.
    const ResizeDiff* resize_diff() const { return has_resize_ ? &resize_ : nullptr; }

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
    /// Seconds of work done for this widget, toward the render throttle. The renderer charges its own
    /// builds, frames and paints; a host charges its rasterising of them too.
    void charge(double seconds);
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
        double sent_x = x0, sent_y = y0;  // where the widget last heard the pointer was, for movementX/Y
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
        double movement_x = 0, movement_y = 0;
    };

    // ops → shadow tree
    void apply_op(const twidget::Json& op, std::vector<twidget::Json>& out);
    /// A batch's ops in order, but for a canvas drawn from another one later in the batch: that one's
    /// node and drawing come first, as the script's calls ran.
    void apply_ops(const std::vector<twidget::Json>& ops, std::vector<twidget::Json>& out);
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
    bool throttled() const;
    /// Charges the outermost pass it spans, whatever nests inside.
    struct Meter {
        Renderer& r;
        double start;
        explicit Meter(Renderer& renderer) : r(renderer), start(r.meter_depth_++ ? 0 : now_s()) {}
        ~Meter() {
            if (--r.meter_depth_ == 0) r.charge(now_s() - start);
        }
    };
    int meter_depth_ = 0;
    std::deque<std::pair<double, double>> charges_;  // when, and how long, the passes of the last window took
    double first_charge_ = -1;
    double throttled_until_ = 0;
    void report_size(std::vector<twidget::Json>& out);
    /// After a layout, with tracking on: the element edges, and the diff when the height changed.
    void note_resize();
    // queries & events
    twidget::Json answer(const twidget::Json& q);
    bool match_media(const std::string& query) const;
    int64_t widget_id_at(double x, double y, Rect* placement) const;
    /// Whether the widget takes horizontal and vertical drags of a press on `target`: its `touch-action`
    /// and its preventDefault on pointerdown or touchstart.
    std::pair<bool, bool> gesture_claim(int64_t target) const;
    void set_capture(Touch& t, int64_t id, std::vector<twidget::Json>& out);
    /// The border box of element `id`, empty when it is not laid out.
    Rect box_of(int64_t id) const;
    void emit_event(const char* type, int64_t target, const Rect& target_rect, const Touch& t, int action, std::vector<twidget::Json>& out);
    twidget::Json event_json(const char* type, int64_t target, const Rect& target_rect, const Input& in) const;
    /// Sends `ev` if the sandbox listens for its type; move-type events wait for the next tick, the
    /// latest one per type and pointer.
    /// `load` (with naturalWidth/Height) or `error` for each <img> whose src changed.
    void load_images(std::vector<twidget::Json>& out);
    void image_event(const char* type, int64_t id, float w, float h, std::vector<twidget::Json>& out);
    static twidget::Json event_message(twidget::Json ev);
    void dispatch(twidget::Json ev, std::vector<twidget::Json>& out);
    void flush_moves(std::vector<twidget::Json>& out);
    void flush_pending(std::vector<twidget::Json>& out);
    /// `resize` for the ResizeObserver targets whose content box changed since the last one.
    void report_resizes(std::vector<twidget::Json>& out);
    /// A click's default action on a <details>' summary: open or close it, and send `toggle`.
    void toggle_details(int64_t clicked, std::vector<twidget::Json>& out);
    /// The sandbox wants events of `type` (it listens, or every widget gets them).
    bool listened(const char* type) const;
    void send_event(const char* type, int64_t target, const Input& in, std::vector<twidget::Json>& out);
    // focus
    /// The text control `id` is, or holds within a few levels (a tap on its inner text), or 0.
    int64_t text_control(int64_t id) const;
    std::string control_value(const Node* n) const;
    void focus(int64_t id, std::vector<twidget::Json>& out);
    void blur(std::vector<twidget::Json>& out);
    void form_event(const char* type, int64_t id, std::vector<twidget::Json>& out, const char* input_type = nullptr);
    /// Brings the spec up to the document after a build: geometry and font; drops it when its
    /// control left the document.
    void refresh_editor();
    EditorSpec editing_;
    std::string editing_initial_;
    uint64_t editor_generation_ = 0;
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
        /// A still layer a script moves (apply_scripted), within `reach` and up to `raster_scale`.
        bool scripted = false;
        /// What the document declared for the layer properties at the last build.
        Declared base;
        /// A shimmer: the element's background tile alone, `box`, sliding under `clip`; the document
        /// paints the rest of the element.
        bool shimmer = false;
        DisplayList tile;
        uint64_t tile_version = 0;
        /// An SVG a script keeps changing: a still layer whose content repaints without the document.
        bool svg = false;
        /// A canvas drawn again and again: a still layer whose drawing repaints only it.
        bool canvas = false;
        /// The layer this one moves with (Layer::parent), and the clip clip_of found before it was
        /// dropped or kept, to tell a clip of its own from the parent's.
        int64_t parent = 0;
        bool found_clip = false;
        Rect found_clip_rect;
    };
    void update_composition(double now);
    // Scripted layers (the Swift renderer's): an element a script moves batch after batch through the
    // layer properties alone becomes a layer, and a batch that only moves such layers moves them
    // without a build or a paint. The document takes the batch with its next build.
    bool scripted_only(const twidget::Json::Array& ops, std::vector<int64_t>& touched) const;
    /// Counts the batches running that change each SVG; three make it an SVG layer.
    void note_svg(const twidget::Json::Array& ops);
    std::unordered_map<int64_t, int> svg_streaks_;
    std::set<int64_t> svg_layers_;
    /// How often each canvas drew, and what it drew last; a canvas id is its node's.
    std::unordered_map<int, uint64_t> canvas_draws_;
    std::set<int64_t> canvas_layers_;
    /// The content version of a layer `content`: its commands, and what the canvases in it drew.
    uint64_t content_version(const DisplayList& content) const;
    void note_scripted(const twidget::Json::Array& ops);
    bool apply_scripted(const std::vector<int64_t>& ids);
    /// The layer's matrix and opacity as the element's style has them now.
    bool scripted_key(int64_t id, const Promotion& p, LayerKey& out) const;
    /// A still layer's track and reach: one key holding the element's own transform and opacity,
    /// which its content leaves out. The running track stays when the key is the same.
    bool still_track(int64_t id, Promotion& p, double now);
    /// Nothing but layer tracks changed since the last paint: its list stands.
    bool reuse_paint() const;
    struct ScriptedHit {
        int count = 0;
        uint64_t batch = 0;
        double at = 0;
    };
    std::unordered_map<int64_t, ScriptedHit> scripted_hits_;
    uint64_t ops_batch_ = 0;
    std::set<int64_t> scripted_candidates_;
    bool scripted_pending_ = false;  // DOM changes only the layers show; the document takes them when it builds
    bool composition_only_ = false;  // since the last paint, only scripted layers moved
    DisplayList painted_list_;       // kept while there are scripted layers, for reuse_paint
    bool painted_list_valid_ = false;
    std::tuple<const void*, uint32_t, uint64_t> painted_key_{};
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
    double max_height_ = 600;
    struct Edges {
        float top, bottom;
    };
    bool track_resize_ = false, prefers_bottom_ = false;
    std::unordered_map<int64_t, Edges> edges_;  // the elements' vertical edges at the last layout
    double edges_height_ = 0;
    uint64_t edges_layouts_ = 0;  // the layout they were taken after; 0 for none
    bool has_resize_ = false;
    ResizeDiff resize_;
    double resize_at_ = -1e9;
    uint64_t resize_serial_ = 0, resize_reported_ = 0;
    int64_t press_id_ = 0;  // the element pressed last, kept past the release for the layout it causes
    double press_y_ = 0, press_at_ = -1e9;
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
    /// <img> elements whose src changed in this batch; and those whose image the backend is still decoding, by image id.
    std::vector<int64_t> pending_images_;
    std::unordered_map<int, std::vector<int64_t>> waiting_images_;
    std::unordered_set<std::string> interest_;
    /// From the host's `theme` message, set on the root after the widget's own style.
    std::vector<std::pair<std::string, std::string>> theme_vars_;
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
    std::map<int64_t, std::pair<float, float>> resize_watch_;  // observeResize: id → content box last reported

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
