#pragma once

#include "twidget/js_thread.h"

#include <functional>
#include <memory>
#include <utility>

namespace oracle {

class StatsTimer {
public:
    auto after(double now, double delay, std::function<void()> callback) {
        _due = now + delay;
        _callback = std::move(callback);
        _handle = std::make_shared<twidget::JSThread::Scheduled>();
        return _handle;
    }

    void advance(double now) {
        if (_handle && now >= _due) {
            const auto handle = std::move(_handle);
            auto callback = std::move(_callback);
            if (!handle->cancelled()) callback();
        }
    }

private:
    double _due = 0.;
    std::function<void()> _callback;
    std::shared_ptr<twidget::JSThread::Scheduled> _handle;
};

inline thread_local StatsTimer statsTimer;

} // namespace oracle
