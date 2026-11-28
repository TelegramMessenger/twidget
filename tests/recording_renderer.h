// A twlite::Renderer that records each non-const call into the process's one recording, with the reads RendererEnd makes after it, for twidget_replay.
#pragma once

#include "log_backend.h"
#include "recording.h"
#include "remote.h"
#include "renderer.h"
#include "string_id.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <mutex>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

namespace {
namespace record {

/// The process's recording: every renderer's steps in the order they ended, and the first thing that went wrong.
struct Sink {
    /// Recursive, so a call made inside a step on the same thread is refused rather than deadlocked.
    std::recursive_mutex mutex;
    Writer writer;
    bool opened = false;
    uint32_t sessions = 0, live = 0;
    uint64_t steps = 0;
    bool in_step = false;
    bool named = false;
    uint64_t names = 0, name_bytes = 0;
    std::string error, label;

    void fail(const std::string& why) {
        if (error.empty()) error = why;
    }
    bool names_moved() const {
        const litehtml::string_id_usage usage = litehtml::_id_usage();
        return named && (usage.names != names || usage.bytes != name_bytes);
    }
};

Sink& sink() {
    static Sink s;
    return s;
}

bool open(const std::string& path, const std::string& origin) {
    Sink& s = sink();
    std::lock_guard lock(s.mutex);
    if (s.opened || !s.writer.open(path, origin)) {
        std::fprintf(stderr, "recording: cannot open %s\n", path.c_str());
        return false;
    }
    s.opened = true;
    return true;
}

/// Where the renderers made from now on come from, written with their construction.
[[maybe_unused]] void label(std::string name) {
    Sink& s = sink();
    std::lock_guard lock(s.mutex);
    s.label = std::move(name);
}

/// Ends the recording with its counts and its first error (the recorder's, else `error`, else renderers still alive); true when there was none, else it is printed.
bool close(const std::string& error = {}) {
    Sink& s = sink();
    std::lock_guard lock(s.mutex);
    if (!s.opened) {
        std::fprintf(stderr, "recording: nothing open to close\n");
        return false;
    }
    if (s.in_step) s.fail("the recording closed inside a step");
    if (s.names_moved()) s.fail("the name table grew after the last step");
    if (!error.empty()) s.fail(error);
    if (s.live) s.fail(std::to_string(s.live) + " renderers still alive");
    std::string why = s.error;
    s.opened = false;
    if (!s.writer.close(End{s.sessions, s.steps, why}) && why.empty()) why = "the recording could not be written";
    std::printf("recorded %u sessions, %llu steps\n", unsigned(s.sessions), static_cast<unsigned long long>(s.steps));
    if (!why.empty()) std::fprintf(stderr, "recording failed: %s\n", why.c_str());
    return why.empty();
}

} // namespace record
} // namespace

namespace twlite {
namespace {

/// Real steady time in whole microseconds; the reads made while a step is open go into it.
class RecordingClock : public Clock {
public:
    double now_ms() override {
        const auto since = std::chrono::steady_clock::now().time_since_epoch();
        const uint64_t us = uint64_t(std::chrono::duration_cast<std::chrono::microseconds>(since).count());
        if (reads) reads->push_back(us);
        return double(us) / 1000;
    }

    std::vector<uint64_t>* reads = nullptr;
};

/// What a RecordingRenderer records with, made before its Renderer and gone after it; each name carries `record_`, as the Renderer's own members are in scope beside them.
struct RecordingParts {
    RecordingParts(double width, double dpr, bool dark, Backend& backend);
    ~RecordingParts();

    bool record_begin(record::Kind kind);
    void record_end();

    std::unique_lock<std::recursive_mutex> record_lock_;
    LogBackend record_tap_;
    RecordingClock record_clock_;
    record::Step record_step_;
    uint32_t record_session_ = 0;
    bool record_asked_ = false;
    bool record_in_ = false;
};

RecordingParts::RecordingParts(double width, double dpr, bool dark, Backend& backend) : record_lock_(record::sink().mutex) {
    record::Sink& s = record::sink();
    if (!s.opened) {
        std::fprintf(stderr, "recording: a renderer was made with no recording open\n");
        std::abort();
    }
    record_tap_.answers = &backend;
    record_session_ = ++s.sessions;
    s.live++;
    if (!record_begin(record::Kind::Construct)) return;
    record_step_.label = s.label;
    record_step_.numbers = {width, dpr, dark ? 1.0 : 0.0};
}

RecordingParts::~RecordingParts() {
    if (record_in_) record_end();
    record::sink().live--;
}

bool RecordingParts::record_begin(record::Kind kind) {
    record::Sink& s = record::sink();
    const std::string what = std::string(record::kind_name(kind)) + " of session " + std::to_string(record_session_);
    if (s.in_step) {
        s.fail("a " + what + " inside another step");
        return false;
    }
    if (!record_tap_.calls.empty()) {
        s.fail(std::to_string(record_tap_.calls.size()) + " Backend calls outside any step, before a " + what);
        record_tap_.calls.clear();
    }
    if (s.names_moved()) s.fail("the name table grew outside any step, before a " + what);
    const litehtml::string_id_usage usage = litehtml::_id_usage();
    record_step_ = record::Step();
    record_step_.kind = kind;
    record_step_.session = record_session_;
    record_step_.names = usage.names;
    record_step_.name_bytes = usage.bytes;
    record_clock_.reads = &record_step_.clock;
    s.in_step = record_in_ = true;
    return true;
}

void RecordingParts::record_end() {
    record::Sink& s = record::sink();
    record_clock_.reads = nullptr;
    record_step_.calls = std::move(record_tap_.calls);
    record_tap_.calls.clear();
    if (s.opened) {
        if (s.writer.write(record_step_)) {
            s.steps++;
        } else {
            s.fail("step " + std::to_string(s.steps + 1) + ", a " + record::kind_name(record_step_.kind) + " of session " + std::to_string(record_session_)
                + ", could not be written");
        }
    }
    const litehtml::string_id_usage usage = litehtml::_id_usage();
    s.names = usage.names;
    s.name_bytes = usage.bytes;
    s.named = true;
    s.in_step = record_in_ = false;
    record_step_ = record::Step();
}

/// A Renderer whose every non-const call is one step of the recording: what it was given, the Backend calls and clock reads it made, what it gave and showed after.
class RecordingRenderer : private RecordingParts, public Renderer {
public:
    /// What one frame gave: the tick's messages, whether the engine's frame message is due, the list when it painted.
    struct Framed {
        std::vector<twidget::Json> messages;
        bool engine = false;
        DisplayList list;
    };

    RecordingRenderer(double width, double dpr, bool dark, Backend& backend)
        : RecordingParts(width, dpr, dark, backend), Renderer(width, dpr, dark, record_tap_, &record_clock_) {
        if (record_in_) record_finish();
        record_lock_.unlock();
    }
    ~RecordingRenderer() {
        record_lock_.lock();
        record_begin(record::Kind::Destroy);
    }

    std::vector<twidget::Json> receive(const twidget::Json& msg) {
        return record_call(record::Kind::Receive, {}, {}, [&] {
            record_step_.message = msg;
            return record_messages(Renderer::receive(msg));
        });
    }
    void set_width(double width, std::vector<twidget::Json>& out) {
        record_append(out, record_call(record::Kind::SetWidth, {width}, {}, [&] {
            std::vector<twidget::Json> got;
            Renderer::set_width(width, got);
            return record_messages(std::move(got));
        }));
    }
    void set_max_height(double height, std::vector<twidget::Json>& out) {
        record_append(out, record_call(record::Kind::SetMaxHeight, {height}, {}, [&] {
            std::vector<twidget::Json> got;
            Renderer::set_max_height(height, got);
            return record_messages(std::move(got));
        }));
    }
    void set_theme(bool dark, std::vector<twidget::Json>& out) {
        record_append(out, record_call(record::Kind::SetTheme, {double(dark)}, {}, [&] {
            std::vector<twidget::Json> got;
            Renderer::set_theme(dark, got);
            return record_messages(std::move(got));
        }));
    }
    std::vector<twidget::Json> touch(int action, double x, double y, int pointer, bool primary, bool mouse = false, int modifiers = 0, int button = 0) {
        const std::vector<double> numbers{double(action), x, y, double(pointer), double(primary), double(mouse), double(modifiers), double(button)};
        return record_call(record::Kind::Touch, numbers, {}, [&] {
            std::vector<twidget::Json> out = record_messages(Renderer::touch(action, x, y, pointer, primary, mouse, modifiers, button));
            record_step_.cursor = cursor();
            return out;
        });
    }
    std::vector<twidget::Json> hover(int action, double x, double y, int modifiers = 0) {
        return record_call(record::Kind::Hover, {double(action), x, y, double(modifiers)}, {}, [&] {
            std::vector<twidget::Json> out = record_messages(Renderer::hover(action, x, y, modifiers));
            record_step_.cursor = cursor();
            return out;
        });
    }
    twidget::WidgetGestureClaim claim_at(double x, double y) {
        return record_call(record::Kind::ClaimAt, {x, y}, {}, [&] { return record_step_.claim = Renderer::claim_at(x, y); });
    }
    std::vector<twidget::Json> wheel(double x, double y, double dx, double dy, int mode, int modifiers = 0) {
        return record_call(record::Kind::Wheel, {x, y, dx, dy, double(mode), double(modifiers)}, {}, [&] { return record_messages(Renderer::wheel(x, y, dx, dy, mode, modifiers)); });
    }
    std::vector<twidget::Json> key(const std::string& type, const std::string& key, const std::string& code, bool repeat, int modifiers) {
        return record_call(record::Kind::Key, {double(repeat), double(modifiers)}, {type, key, code}, [&] { return record_messages(Renderer::key(type, key, code, repeat, modifiers)); });
    }
    std::vector<twidget::Json> edit(uint64_t generation, const std::string& text, const std::string& input_type = "insertText") {
        return record_call(record::Kind::Edit, {double(generation)}, {text, input_type}, [&] {
            if (generation > (uint64_t(1) << 53)) record::sink().fail("an edit generation past what a double holds exactly");
            return record_messages(Renderer::edit(generation, text, input_type));
        });
    }
    std::vector<twidget::Json> end_editing() {
        return record_call(record::Kind::EndEditing, {}, {}, [&] { return record_messages(Renderer::end_editing()); });
    }
    bool clear_selection() {
        return record_call(record::Kind::ClearSelection, {}, {}, [&] { return Renderer::clear_selection(); });
    }
    std::vector<twidget::Json> tick() { return frame(true, remote::Paint::Skip).messages; }
    void set_compositing(bool enabled) {
        record_call(record::Kind::SetCompositing, {double(enabled)}, {}, [&] { Renderer::set_compositing(enabled); });
    }
    std::vector<twidget::Json> image_ready(int id, float w, float h) {
        return record_call(record::Kind::ImageReady, {double(id), double(w), double(h)}, {}, [&] { return record_messages(Renderer::image_ready(id, w, h)); });
    }
    void set_resize_tracking(bool enabled, bool prefers_bottom = false) {
        record_call(record::Kind::SetResizeTracking, {double(enabled), double(prefers_bottom)}, {}, [&] { Renderer::set_resize_tracking(enabled, prefers_bottom); });
    }
    DisplayList paint() { return frame(false, remote::Paint::Force).list; }
    void trim() {
        record_call(record::Kind::Trim, {}, {}, [&] { Renderer::trim(); });
    }
    void charge(double seconds) {
        record_call(record::Kind::Charge, {seconds}, {}, [&] { Renderer::charge(seconds); });
    }
    void set_patching(bool enabled) {
        record_call(record::Kind::SetPatching, {double(enabled)}, {}, [&] { Renderer::set_patching(enabled); });
    }
    const std::string& last_html() {
        return record_call(record::Kind::LastHtml, {}, {}, [&]() -> const std::string& { return Renderer::last_html(); });
    }

    /// One frame as RendererEnd runs a frame request: the tick when asked, then a paint when forced, or when asked and dirty.
    Framed frame(bool tick, remote::Paint paint) {
        return record_call(record::Kind::Frame, {double(tick), double(uint8_t(paint))}, {}, [&] {
            Framed f;
            if (tick) {
                f.messages = record_messages(Renderer::tick());
                f.engine = record_step_.engine = std::exchange(record_asked_, false);
            }
            if (paint == remote::Paint::Force || (paint == remote::Paint::IfDirty && dirty())) {
                f.list = Renderer::paint();
                std::vector<Rect> rects;
                if (Renderer::damage(rects)) record_step_.damage = std::move(rects);
                record_step_.list = f.list;
                record_step_.painted = true;
            }
            return f;
        });
    }
    /// The engine asked for its frame message: the next frame that ticks says it is due.
    void want_frame() {
        record_call(record::Kind::WantFrame, {}, {}, [&] { record_asked_ = true; });
    }

private:
    template <class Run> std::invoke_result_t<Run&> record_call(record::Kind kind, std::vector<double> numbers, std::vector<std::string> texts, Run&& run) {
        std::lock_guard lock(record::sink().mutex);
        const bool recorded = record_begin(kind);
        if (recorded) {
            record_step_.numbers = std::move(numbers);
            record_step_.texts = std::move(texts);
        }
        if constexpr (std::is_void_v<std::invoke_result_t<Run&>>) {
            run();
            if (recorded) record_finish();
        } else {
            decltype(auto) result = run();
            if (recorded) record_finish();
            return result;
        }
    }
    std::vector<twidget::Json> record_messages(std::vector<twidget::Json> out) {
        for (const twidget::Json& m : out) record_step_.messages.push_back(m.dump());
        return out;
    }
    /// The reads RendererEnd's finish() makes, in its order, then the Frame marker as the platform end logs it, then the write.
    void record_finish() {
        record_step_.selection = may_have_selection() ? selected_text() : std::string();
        record_step_.state = remote::State{width(), viewport_height(), dirty(), wants_tick(), next_tick_in(), record_asked_};
        if (record_step_.kind == record::Kind::Frame) record_tap_.frame(record_step_.painted);
        record_end();
    }
    static void record_append(std::vector<twidget::Json>& out, std::vector<twidget::Json> got) {
        for (twidget::Json& m : got) out.push_back(std::move(m));
    }
};

} // namespace
} // namespace twlite
