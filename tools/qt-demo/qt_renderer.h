// WidgetHost's Renderer on TwidgetCore, painted by QtBackend into a QImage per frame. The Qt twin of the
// Windows spike's SurfaceRenderer, without compositor layers: every frame is painted whole.
//
// Threading: the core is not locked on its side, so every call into it goes through one mutex here. The
// host thread (messages), the clock thread (frames and paints) and the UI thread (input) take turns;
// messages to the host and frames to the view go out after the lock is released.
#pragma once

#include "qt_backend.h"
#include "renderer.h"
#include "twidget/renderer.h"

#include <QImage>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <functional>
#include <mutex>
#include <optional>
#include <thread>

class QtRenderer final : public twidget::Renderer {
public:
    /// `onFrame` runs on the clock thread with each painted frame, its device pixel ratio set.
    QtRenderer(const std::string& theme, double width, double dpr, std::function<void(QImage)> onFrame);
    ~QtRenderer() override;

    // Renderer (host thread)
    void load() override;
    void send(const std::string& json) override;
    void setTheme(const std::string& theme) override;
    void setWidth(double width) override;
    void destroy() override;

    // Input in CSS px, any thread. touch action: 0 down, 1 move, 2 up, 3 cancel; hover action: 0 move, 1 leave.
    void touch(int action, double x, double y, int pointer, bool primary, bool mouse, int modifiers);
    void hover(int action, double x, double y, int modifiers);
    void wheel(double x, double y, double dx, double dy, int mode, int modifiers);
    void key(const std::string& type, const std::string& key, const std::string& code, bool repeat, int modifiers);
    // Off screen nothing animates: the clock stops ticking until the widget is back.
    void setVisible(bool visible);

    int paints() const { return paints_; }
    double lastPaintMs() const { return lastPaintMs_; }
    double lastBuildMs() const { return lastBuildMs_; }

private:
    void dispatch(std::vector<twidget::Json> messages);
    // Runs `call` on the core under the lock, then sends what it returned, repaints and keeps ticking as it asks.
    template <class Call> void run(Call call);
    void setWake(double seconds);
    void setTicking(bool ticking);
    void schedulePaint();
    void requestFrame();
    void clockLoop();
    void paint();
    static double nowMs();

    double dpr_;
    std::function<void(QImage)> onFrame_;
    twlite::QtBackend backend_;
    std::unique_ptr<twlite::Renderer> core_;
    std::mutex mutex_;  // every call into core_ and backend_
    QImage last_;       // the last frame painted, which the next one starts from

    // One tick per display refresh at most.
    std::mutex clockMutex_;
    std::condition_variable clockCv_;
    std::thread clock_;
    bool frameRequested_ = false;
    bool ticking_ = false;  // the core animates or holds coalesced moves
    // Not ticking, the core still needs a tick here: an animation event is due.
    std::optional<std::chrono::steady_clock::time_point> wakeAt_;
    bool visible_ = true;
    bool paintScheduled_ = false;
    bool stopClock_ = false;
    bool frozen_ = false;
    bool destroyed_ = false;

    std::atomic<int> paints_{0};
    std::atomic<double> lastPaintMs_{0};
    std::atomic<double> lastBuildMs_{0};
};
