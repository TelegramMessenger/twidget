// One widget in the feed: a WidgetHost (QuickJS, the validator) over a QtRenderer, shown at its reported
// height, with mouse, wheel and keys forwarded to the renderer in CSS px.
#pragma once

#include "twidget/widget_host.h"

#include <QImage>
#include <QWidget>

#include <functional>
#include <memory>
#include <mutex>

#include "qt_renderer.h"

#include <QTimer>

class WidgetView : public QWidget {
public:
    WidgetView(const std::string& html, bool dark, double width, std::shared_ptr<twidget::SandboxResources> resources, QWidget* parent = nullptr);
    ~WidgetView() override;

    void start();
    void setDark(bool dark);
    void setContentWidth(double width);
    void setOnScreen(bool visible);

    /// One line of diagnostics, on the UI thread whenever the host's change.
    std::function<void(const QString&)> onStatus;

protected:
    void paintEvent(QPaintEvent*) override;
    void mousePressEvent(QMouseEvent*) override;
    void mouseMoveEvent(QMouseEvent*) override;
    void mouseReleaseEvent(QMouseEvent*) override;
    void leaveEvent(QEvent*) override;
    void wheelEvent(QWheelEvent*) override;
    void keyPressEvent(QKeyEvent*) override;
    void keyReleaseEvent(QKeyEvent*) override;

private:
    void changed(const twidget::WidgetDiagnostics& d);
    void forwardKey(QKeyEvent* e, const char* type);

    std::unique_ptr<twidget::WidgetHost> host_;
    QtRenderer* renderer_ = nullptr;  // owned by host_
    double width_;
    bool onScreen_ = true;

    std::mutex frameMutex_;  // frame_ and layers_: written on the clock thread, drawn on the UI thread
    QImage frame_;
    std::vector<QtLayer> layers_;
    QTimer* animation_ = nullptr;  // repaints while a layer's track runs
};
