// twidget-record: widget-sandbox corpus widgets run for real (WidgetHost, QuickJS, the validator) on a recorded TwidgetCore over CoreGraphics with one fixed input script, into one recording for twidget_replay.
#include "recording_renderer.h"

#include "twidget/intl.h"
#include "twidget/json.h"
#include "twidget/renderer.h"
#include "twidget/resources.h"
#include "twidget/widget_host.h"

#include <sys/sysctl.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <exception>
#include <filesystem>
#include <fstream>
#include <memory>
#include <mutex>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <system_error>
#include <thread>
#include <utility>
#include <vector>

std::unique_ptr<twlite::Backend> platform_backend();

namespace {

namespace fs = std::filesystem;
using twidget::Json;

/// The input every widget gets, in the order it is given; the origin carries every number.
struct Script {
    double width = 320, narrow = 240, dpr = 2;
    double load_s = 30, render_s = 5;
    int frame_ms = 16, settle_ms = 300;
    std::vector<double> claim_x{40, 120, 200, 280};
    double claim_y = 30, claim_dy = 60;
    int moves = 6;
    double move_dx = 8, move_dy = 2;
    int move_ms = 20, after_press_ms = 200;
    int hovers = 6;
    double hover_x = 20, hover_dx = 50;
    int hover_ms = 16, width_ms = 150, theme_ms = 200;
};

const Script SCRIPT;

double now_ms() {
    return std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

void wait_ms(int ms) {
    std::this_thread::sleep_for(std::chrono::milliseconds(ms));
}

uint64_t steps_so_far() {
    record::Sink& s = record::sink();
    std::lock_guard lock(s.mutex);
    return s.steps;
}

twidget::WidgetHostOptions host_options() {
    twidget::WidgetHostOptions o;
    o.devicePixelRatio = SCRIPT.dpr;
    o.policy = "strict";
    o.features = {"webAnimations"};
    o.locale = "en";
    return o;
}

twidget::WidgetTheme theme_of(const char* scheme) {
    twidget::WidgetTheme t;
    t.colorScheme = scheme;
    return t;
}

Json array_of(const std::vector<Json>& items) {
    Json out = Json::array();
    for (const Json& item : items) out.push(item);
    return out;
}

std::string system_version() {
    char version[64] = {};
    size_t size = sizeof version - 1;
    if (sysctlbyname("kern.osproductversion", version, &size, nullptr, 0) != 0) return "macOS, version unknown";
    return "macOS " + std::string(version);
}

Json origin(const std::vector<std::string>& paths) {
    const twidget::WidgetHostOptions options = host_options();
    Json host = Json::object();
    host.set("width", SCRIPT.width);
    host.set("devicePixelRatio", options.devicePixelRatio);
    host.set("policy", options.policy);
    host.set("features", array_of(std::vector<Json>(options.features.begin(), options.features.end())));
    host.set("locale", options.locale);
    host.set("colorScheme", "light");
    Json toolchain = Json::object();
    toolchain.set("compiler", __VERSION__);
#if defined(_LIBCPP_VERSION)
    toolchain.set("standard library", "libc++ " + std::to_string(_LIBCPP_VERSION) + ", ABI " + std::to_string(_LIBCPP_ABI_VERSION));
#else
    toolchain.set("standard library", "not libc++");
#endif
    toolchain.set("system", system_version());
    const Script& s = SCRIPT;
    Json script = Json::object();
    script.set("name", "tools/record corpus script 1");
    script.set("load timeout s", s.load_s);
    script.set("render timeout s", s.render_s);
    script.set("frame loop ms", s.frame_ms);
    script.set("frames before input ms", s.settle_ms);
    script.set("claim_at x", array_of(std::vector<Json>(s.claim_x.begin(), s.claim_x.end())));
    script.set("claim_at y from", s.claim_y);
    script.set("claim_at y step, below the height", s.claim_dy);
    script.set("press", "at the first claiming point, else the centre");
    script.set("moves", s.moves);
    script.set("move px", array_of({s.move_dx, s.move_dy}));
    script.set("move ms", s.move_ms);
    script.set("after the release ms", s.after_press_ms);
    script.set("hovers", s.hovers);
    script.set("hover x from", s.hover_x);
    script.set("hover x step", s.hover_dx);
    script.set("hover ms", s.hover_ms);
    script.set("widths", array_of({s.narrow, s.width}));
    script.set("width ms", s.width_ms);
    script.set("themes", array_of({"dark", "light"}));
    script.set("theme ms", s.theme_ms);
    std::vector<Json> widgets(paths.begin(), paths.end());
    Json o = Json::object();
    o.set("source", "widget-sandbox corpus");
    o.set("recorded by", "tools/record (twidget-record)");
    o.set("twidget", TW_COMMIT);
    o.set("widget-sandbox", TW_FRAMEWORK);
    o.set("backend", "CoreGraphics");
    o.set("host", std::move(host));
    o.set("toolchain", std::move(toolchain));
    o.set("script", std::move(script));
    o.set("widgets", array_of(widgets));
    return o;
}

/// WidgetHost's Renderer on a recorded TwidgetCore, as OffscreenRenderer is: one mutex for the core, the engine's frame request and the stop; messages to the host after it is released; a frame loop.
class Recorded final : public twidget::Renderer {
public:
    Recorded(double width, bool dark) : backend_(platform_backend()), core_(std::make_unique<twlite::RecordingRenderer>(width, SCRIPT.dpr, dark, *backend_)) {}
    ~Recorded() override { destroy(); }

    void load() override {
        {
            std::lock_guard lock(mutex_);
            if (!stop_ && !loop_.joinable()) loop_ = std::thread([this] { frames(); });
        }
        if (onMessage) onMessage("{\"type\":\"ready\"}");
    }

    void send(const std::string& json) override {
        if (json.rfind("{\"type\":\"requestFrame\"", 0) == 0) {
            std::lock_guard lock(mutex_);
            if (!core_) return;
            core_->want_frame();
            asked_ = true;
            return;
        }
        std::optional<Json> msg = Json::parse(json);
        if (!msg) {
            std::lock_guard lock(mutex_);
            if (error_.empty()) error_ = "the host sent a message the recorder cannot parse: " + json.substr(0, 120);
            return;
        }
        run([&] { return core_->receive(*msg); });
    }

    void setTheme(const std::string& theme) override {
        run([&] {
            std::vector<Json> out;
            core_->set_theme(theme == "dark", out);
            return out;
        });
    }

    void setWidth(double width) override {
        run([&] {
            std::vector<Json> out;
            core_->set_width(width, out);
            return out;
        });
    }

    void destroy() override {
        std::thread loop;
        {
            std::lock_guard lock(mutex_);
            if (stop_) return;
            stop_ = true;
            loop = std::move(loop_);
        }
        if (loop.joinable()) loop.join();
        std::lock_guard lock(mutex_);
        core_.reset();
    }

    twidget::WidgetGestureClaim claim_at(double x, double y) {
        std::lock_guard lock(mutex_);
        return core_ ? core_->claim_at(x, y) : twidget::WidgetGestureClaim();
    }

    /// The primary pointer, a touch; on a press, what its `gesture` message claims.
    twidget::WidgetGestureClaim touch(int action, double x, double y) {
        twidget::WidgetGestureClaim claim;
        run([&] {
            std::vector<Json> out = core_->touch(action, x, y, 0, true, false, 0, 0);
            for (const Json& m : out)
                if (m["type"].asString() == "gesture") claim = {m["x"].asBool(), m["y"].asBool(), m["hold"].asBool()};
            return out;
        });
        return claim;
    }

    void hover(int action, double x, double y) {
        run([&] { return core_->hover(action, x, y, 0); });
    }

    double height() {
        std::lock_guard lock(mutex_);
        return core_ ? core_->viewport_height() : 0;
    }

    /// Frames that painted a list with something in it.
    size_t paints() {
        std::lock_guard lock(mutex_);
        return paints_;
    }

    size_t engine_frames() {
        std::lock_guard lock(mutex_);
        return engine_frames_;
    }

    std::string error() {
        std::lock_guard lock(mutex_);
        return error_;
    }

private:
    template <class Call> void run(Call&& call) {
        std::vector<Json> out;
        {
            std::lock_guard lock(mutex_);
            if (!core_) return;
            out = call();
        }
        for (const Json& m : out)
            if (onMessage) onMessage(m.dump());
    }

    /// One frame per display refresh at most: a tick when the engine asked, an animation or a coalesced move waits, or the renderer's wake is due; a paint when dirty.
    void frames() {
        while (true) {
            wait_ms(SCRIPT.frame_ms);
            std::vector<Json> out;
            bool engine = false;
            {
                std::lock_guard lock(mutex_);
                if (stop_ || !core_) return;
                const double now = now_ms();
                const bool tick = asked_ || core_->wants_tick() || (wake_at_ && now >= wake_at_);
                if (!tick && !core_->dirty()) continue;
                twlite::RecordingRenderer::Framed f = core_->frame(tick, twlite::remote::Paint::IfDirty);
                if (tick) {
                    asked_ = false;
                    const double next = core_->next_tick_in();
                    wake_at_ = std::isfinite(next) ? now + next * 1000 : 0;
                }
                paints_ += !f.list.empty();
                engine_frames_ += f.engine;
                out = std::move(f.messages);
                engine = f.engine;
            }
            for (const Json& m : out)
                if (onMessage) onMessage(m.dump());
            if (engine && onMessage) onMessage("{\"type\":\"frame\",\"time\":" + Json::number(now_ms()) + "}");
        }
    }

    std::unique_ptr<twlite::Backend> backend_;
    std::mutex mutex_;
    std::unique_ptr<twlite::RecordingRenderer> core_;
    std::thread loop_;
    bool asked_ = false, stop_ = false;
    double wake_at_ = 0;
    size_t paints_ = 0, engine_frames_ = 0;
    std::string error_;
};

struct Widget {
    std::string path, html;
};

/// Why the widget cannot be recorded as it stands: it died or threw an uncaught error.
std::string trouble(const twidget::WidgetDiagnostics& d) {
    if (d.dead) return "the widget died: " + *d.dead;
    for (const auto& [level, text] : d.logs)
        if (level == "error" && text.rfind("Uncaught ", 0) == 0) return "the widget threw: " + text;
    if (d.lastError) return "the widget threw: " + *d.lastError;
    return {};
}

/// What the host's observers saw of the widget, kept past the host, whose destructor drains its thread and so delivers the last of it.
struct Health {
    std::mutex mutex;
    bool stopping = false;
    std::string error, death;

    void threw(const std::string& text) {
        std::lock_guard lock(mutex);
        if (error.empty()) error = text;
    }
    void changed(const twidget::WidgetDiagnostics& d) {
        std::lock_guard lock(mutex);
        if (d.dead && death.empty() && !(stopping && *d.dead == "terminated")) death = *d.dead;
    }
    /// From here a death as "terminated" is the recorder's own destroy().
    void stop() {
        std::lock_guard lock(mutex);
        stopping = true;
    }
    std::string trouble() {
        std::lock_guard lock(mutex);
        if (!death.empty()) return "the widget died: " + death;
        if (!error.empty()) return "the widget threw: " + error;
        return {};
    }
};

std::string claim_text(const twidget::WidgetGestureClaim& c) {
    return std::string("x ") + (c.x ? "yes" : "no") + ", y " + (c.y ? "yes" : "no") + ", hold " + (c.hold ? "yes" : "no");
}

/// One widget from its construction to its destruction, the script in between; what went wrong, or nothing.
std::string record_widget(const Widget& widget, const std::shared_ptr<twidget::SandboxResources>& resources) {
    record::label(widget.path);
    const uint64_t first = steps_so_far() + 1;
    std::vector<std::pair<std::string, uint64_t>> milestones;
    const auto milestone = [&](const char* name) { milestones.emplace_back(name, steps_so_far()); };
    std::string why;
    size_t queries = 0, claiming = 0, paints_before = 0, paints = 0, engine_frames = 0;
    double height = 0, px = -1, py = -1;
    twidget::WidgetGestureClaim press;
    twidget::WidgetDiagnostics d;
    Health health;
    {
        Recorded* adapter = nullptr;
        twidget::WidgetHost host(
            widget.html, theme_of("light"), SCRIPT.width, host_options(), resources,
            [&](std::shared_ptr<twidget::SandboxResources>, const std::string& scheme, double width) -> std::unique_ptr<twidget::Renderer> {
                auto r = std::make_unique<Recorded>(width, scheme == "dark");
                adapter = r.get();
                return r;
            },
            std::make_shared<twidget::BasicIntl>());
        host.onError = [&health](const std::string& text) { health.threw(text); };
        host.onChange = [&health](const twidget::WidgetDiagnostics& seen) { health.changed(seen); };
        host.start();
        const bool loaded = host.waitLoaded(SCRIPT.load_s);
        const bool rendered = host.waitRendered(SCRIPT.render_s);
        wait_ms(SCRIPT.settle_ms);
        d = host.diagnostics();
        if (!adapter) throw std::runtime_error("WidgetHost made no renderer");
        paints_before = adapter->paints();
        if (!loaded || !d.loaded) {
            why = "the widget did not load in " + Json::number(SCRIPT.load_s) + " s";
        } else if (!rendered || !d.rendered) {
            why = "the widget did not render in " + Json::number(SCRIPT.render_s) + " s";
        } else {
            why = trouble(d);
        }
        if (why.empty() && paints_before == 0) why = "the renderer painted nothing before the input";
        if (why.empty()) {
            milestone("claim_at");
            height = adapter->height();
            for (double y = SCRIPT.claim_y; y < height; y += SCRIPT.claim_dy) {
                for (double x : SCRIPT.claim_x) {
                    const twidget::WidgetGestureClaim c = adapter->claim_at(x, y);
                    queries++;
                    if (!c.x && !c.y && !c.hold) continue;
                    claiming++;
                    if (px >= 0) continue;
                    px = x;
                    py = y;
                }
            }
            if (px < 0) {
                px = SCRIPT.width / 2;
                py = height / 2;
            }
            milestone("press");
            press = adapter->touch(0, px, py);
            for (int i = 1; i <= SCRIPT.moves; i++) {
                wait_ms(SCRIPT.move_ms);
                adapter->touch(1, px + i * SCRIPT.move_dx, py + i * SCRIPT.move_dy);
            }
            adapter->touch(2, px + SCRIPT.moves * SCRIPT.move_dx, py + SCRIPT.moves * SCRIPT.move_dy);
            wait_ms(SCRIPT.after_press_ms);
            milestone("hovers");
            for (int i = 0; i < SCRIPT.hovers; i++) {
                adapter->hover(0, SCRIPT.hover_x + i * SCRIPT.hover_dx, py);
                wait_ms(SCRIPT.hover_ms);
            }
            adapter->hover(1, 0, 0);
            milestone("widths");
            adapter->setWidth(SCRIPT.narrow);
            wait_ms(SCRIPT.width_ms);
            adapter->setWidth(SCRIPT.width);
            wait_ms(SCRIPT.width_ms);
            milestone("themes");
            host.setTheme(theme_of("dark"));
            wait_ms(SCRIPT.theme_ms);
            host.setTheme(theme_of("light"));
            wait_ms(SCRIPT.theme_ms);
            milestone("destroy");
            d = host.diagnostics();
            why = adapter->error();
        }
        paints = adapter->paints();
        engine_frames = adapter->engine_frames();
        health.stop();
        host.destroy();
    }
    if (why.empty()) why = health.trouble();
    const uint64_t last = steps_so_far();
    size_t errors = 0;
    for (const auto& [level, text] : d.logs) errors += level == "error";
    std::printf("%s: steps %llu to %llu; %zu painted frames (%zu before the input), %zu engine frames; height %.0f; %zu claim_at queries, %zu claiming; press at (%.0f, %.0f) claims %s; %zu log lines, %zu errors\n",
                widget.path.c_str(), static_cast<unsigned long long>(first), static_cast<unsigned long long>(last), paints, paints_before, engine_frames, height, queries, claiming, px, py,
                claim_text(press).c_str(), d.logs.size(), errors);
    for (const auto& [name, after] : milestones) std::printf("  %s begins after step %llu\n", name.c_str(), static_cast<unsigned long long>(after));
    for (const auto& [kind, entry] : d.diags) std::printf("  diagnostic %s: %d\n", kind.c_str(), entry.first);
    return why.empty() ? std::string() : widget.path + ": " + why;
}

std::optional<std::string> read_file(const fs::path& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) return std::nullopt;
    std::ostringstream s;
    s << in.rdbuf();
    if (in.bad()) return std::nullopt;
    return s.str();
}

int usage() {
    std::fprintf(stderr, "usage: twidget-record --out <recording> <corpus path>...\n"
                         "corpus paths are read from %s/corpus, the widget-sandbox this build was configured with (TWIDGET_WIDGET_SANDBOX)\n",
                 TW_SANDBOX);
    return 2;
}

/// A corpus path as given: relative, without a ".." part.
bool corpus_path(const std::string& path) {
    return !path.empty() && path[0] != '/' && ("/" + path + "/").find("/../") == std::string::npos;
}

/// Whether a resolved path lies below a resolved directory, compared part by part rather than as text.
bool below(const fs::path& dir, const fs::path& file) {
    const auto [in_dir, in_file] = std::mismatch(dir.begin(), dir.end(), file.begin(), file.end());
    return in_dir == dir.end() && in_file != file.end();
}

} // namespace

int main(int argc, char** argv) {
    const std::string sandbox = TW_SANDBOX;
    std::string out;
    std::vector<std::string> paths;
    for (int i = 1; i < argc; i++) {
        const std::string arg = argv[i];
        if (arg == "--out") {
            if (i + 1 == argc) return usage();
            out = argv[++i];
        } else if (!corpus_path(arg) || arg[0] == '-') {
            return usage();
        } else {
            paths.push_back(arg);
        }
    }
    if (out.empty() || paths.empty()) return usage();
    std::error_code missing;
    const fs::path corpus = fs::canonical(sandbox + "/corpus", missing);
    if (missing) {
        std::fprintf(stderr, "twidget-record: cannot read %s/corpus\n", sandbox.c_str());
        return 2;
    }
    std::vector<Widget> widgets;
    for (const std::string& path : paths) {
        const fs::path file = fs::canonical(corpus / path, missing);
        if (!missing && !below(corpus, file)) {
            std::fprintf(stderr, "twidget-record: %s/corpus/%s resolves to %s, which is not inside the corpus\n", sandbox.c_str(), path.c_str(), file.c_str());
            return 2;
        }
        std::optional<std::string> html;
        if (!missing) html = read_file(file);
        if (!html) {
            std::fprintf(stderr, "twidget-record: cannot read %s/corpus/%s\n", sandbox.c_str(), path.c_str());
            return 2;
        }
        widgets.push_back({path, std::move(*html)});
    }
    std::shared_ptr<twidget::SandboxResources> resources;
    try {
        resources = twidget::SandboxResources::fromDirectory(sandbox + "/packages/desktop/WidgetJS");
    } catch (const std::exception& e) {
        std::fprintf(stderr, "twidget-record: %s\n", e.what());
        return 2;
    }
    const std::string from = origin(paths).dump();
    std::printf("origin %s\n", from.c_str());
    if (!record::open(out, from)) return 2;
    std::string error;
    for (const Widget& widget : widgets) {
        try {
            error = record_widget(widget, resources);
        } catch (const std::exception& e) {
            error = widget.path + ": " + e.what();
        }
        if (!error.empty()) break;
    }
    if (!error.empty()) std::fprintf(stderr, "twidget-record: %s\n", error.c_str());
    const bool closed = record::close(error);
    return error.empty() && closed ? 0 : 1;
}
