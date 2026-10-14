// Pixel checks of the QPainter backend (platform/qt): display list commands, canvas operations and a
// canvas through the whole renderer, read back from the images it draws. Runs headless on Qt's
// offscreen platform.
#include "qt_backend.h"
#include "renderer.h"

#include <QGuiApplication>
#include <QPainter>

#include <cmath>
#include <cstdio>
#include <string>

using twidget::Json;
using namespace twlite;

static int failures = 0;

static void check(bool ok, const char* what) {
    if (!ok) {
        std::printf("FAIL: %s\n", what);
        failures++;
    }
}

static Json parse(const std::string& s) {
    auto v = Json::parse(s);
    if (!v) {
        std::printf("bad test JSON: %s\n", s.c_str());
        failures++;
        return Json();
    }
    return *v;
}

static bool near(QRgb a, QColor b, int tolerance = 6) {
    return std::abs(qRed(a) - b.red()) <= tolerance && std::abs(qGreen(a) - b.green()) <= tolerance && std::abs(qBlue(a) - b.blue()) <= tolerance &&
           std::abs(qAlpha(a) - b.alpha()) <= tolerance;
}

static QRgb at(const QImage& image, int x, int y) { return image.pixel(x, y); }

static Command fill_rect(Rect r, Color c, Radii q = {}) {
    Command cmd;
    cmd.kind = Cmd::FillRect;
    cmd.rect = r;
    cmd.color = c;
    cmd.radii = q;
    return cmd;
}

static Geometry rect_path(float x, float y, float w, float h) {
    Geometry g;
    g.ops = {Path::Move, Path::Line, Path::Line, Path::Line, Path::Close};
    g.points = {x, y, x + w, y, x + w, y + h, x, y + h};
    return g;
}

static void test_display_list() {
    QtBackend backend;
    QImage image = QtBackend::make_image(100, 100);

    DisplayList list;
    list.push_back(fill_rect({0, 0, 100, 100}, {255, 255, 255, 255}));
    // A rounded clip around a full-size red fill: the corners stay white.
    Command clip;
    clip.kind = Cmd::ClipPush;
    clip.rect = {10, 10, 80, 80};
    clip.radii = {20, 20, 20, 20, 20, 20, 20, 20};
    list.push_back(clip);
    list.push_back(fill_rect({0, 0, 100, 100}, {255, 0, 0, 255}));
    Command pop;
    pop.kind = Cmd::ClipPop;
    list.push_back(pop);
    // A half-transparent group over two overlapping blue squares: one layer, so the overlap is no darker.
    Command group;
    group.kind = Cmd::GroupPush;
    group.opacity = 0.5f;
    list.push_back(group);
    list.push_back(fill_rect({0, 90, 6, 6}, {0, 0, 255, 255}));
    list.push_back(fill_rect({3, 90, 6, 6}, {0, 0, 255, 255}));
    Command group_pop;
    group_pop.kind = Cmd::GroupPop;
    list.push_back(group_pop);
    backend.paint(list, image, QTransform());

    check(near(at(image, 50, 50), QColor(255, 0, 0)), "inside the rounded clip is red");
    check(near(at(image, 12, 12), QColor(255, 255, 255)), "the rounded clip's corner stays white");
    check(near(at(image, 5, 5), QColor(255, 255, 255)), "outside the clip stays white");
    check(near(at(image, 1, 92), at(image, 4, 92)), "an opacity group composites once: the overlap is not darker");
    check(near(at(image, 4, 92), QColor(128, 128, 255), 8), "half-transparent blue over white");
}

static void test_shadow_and_gradients() {
    QtBackend backend;
    QImage image = QtBackend::make_image(200, 100);
    DisplayList list;
    list.push_back(fill_rect({0, 0, 200, 100}, {255, 255, 255, 255}));
    Command shadow;
    shadow.kind = Cmd::Shadow;
    shadow.rect = {20, 20, 40, 40};
    shadow.color = {0, 0, 0, 255};
    shadow.ax = 0;
    shadow.ay = 0;
    shadow.bx = 10;  // blur
    shadow.by = 0;   // spread
    list.push_back(shadow);

    Command conic;
    conic.kind = Cmd::Conic;
    conic.rect = {100, 0, 100, 100};
    conic.ax = 150;
    conic.ay = 50;
    conic.bx = 0;
    conic.stops = {{0, {255, 0, 0, 255}}, {0.5f, {0, 0, 255, 255}}, {1, {255, 0, 0, 255}}};
    list.push_back(conic);
    backend.paint(list, image, QTransform());

    QRgb inside = at(image, 40, 40), edge = at(image, 61, 40), far = at(image, 95, 40);
    check(near(inside, QColor(255, 255, 255)), "an outer shadow does not draw inside its box");
    check(qRed(edge) < 200, "the shadow is dark next to the box");
    check(qRed(edge) > 40, "the shadow is blurred, not solid, next to the box");
    check(near(far, QColor(255, 255, 255)), "the blur fades out");
    // The conic starts at the top and turns clockwise: blue half way, at the bottom.
    check(qRed(at(image, 150, 5)) > 200 && qBlue(at(image, 150, 5)) < 60, "the conic is red at the top");
    check(qBlue(at(image, 150, 95)) > 200 && qRed(at(image, 150, 95)) < 60, "the conic is blue at the bottom");
    check(qRed(at(image, 195, 50)) > qBlue(at(image, 195, 50)) - 40 && qBlue(at(image, 195, 50)) > 60, "a quarter turn in, on the right, it is between");
}

static void test_canvas_ops() {
    QtBackend backend;
    std::vector<CanvasOp> ops(1);
    ops[0].kind = CanvasOp::Reset;

    CanvasOp fill;
    fill.kind = CanvasOp::Fill;
    fill.path = rect_path(0, 0, 10, 20);
    fill.paint.kind = CanvasPaint::Linear;
    fill.paint.args[0] = 0;
    fill.paint.args[1] = 20;
    fill.paint.args[2] = 0;
    fill.paint.args[3] = 0;
    fill.paint.stops = {{0, {255, 0, 0, 255}}, {1, {0, 0, 255, 255}}};
    ops.push_back(fill);

    CanvasOp solid;
    solid.kind = CanvasOp::Fill;
    solid.path = rect_path(20, 0, 10, 10);
    solid.paint.color = {0, 255, 0, 255};
    ops.push_back(solid);

    // Multiply over the green square: green × yellow stays green.
    CanvasOp multiply = solid;
    multiply.path = rect_path(20, 0, 10, 10);
    multiply.paint.color = {255, 255, 0, 255};
    multiply.composite = Composite::Multiply;
    ops.push_back(multiply);

    // Luminosity, which QPainter lacks: a white source over green takes the source's lightness.
    CanvasOp lum = solid;
    lum.path = rect_path(20, 10, 10, 10);
    ops.push_back(lum);
    CanvasOp lum2 = lum;
    lum2.paint.color = {255, 255, 255, 255};
    lum2.composite = Composite::Luminosity;
    ops.push_back(lum2);

    // Copy under a clip: replaces inside the clip only.
    CanvasOp copy = solid;
    copy.path = rect_path(40, 0, 10, 10);
    copy.paint.color = {0, 0, 255, 255};
    ops.push_back(copy);
    CanvasOp replace = copy;
    replace.path = rect_path(45, 5, 1, 1);
    replace.paint.color = {255, 0, 0, 255};
    replace.composite = Composite::Copy;
    auto clips = std::make_shared<std::vector<CanvasClip>>(1);
    (*clips)[0].path = rect_path(40, 0, 5, 10);
    replace.clips = clips;
    ops.push_back(replace);

    backend.canvas_run(7, 60, 20, ops);
    std::vector<uint8_t> px;
    backend.canvas_read(7, 0, 0, 60, 20, px);
    auto rgba = [&](int x, int y) { const uint8_t* p = px.data() + (size_t(y) * 60 + x) * 4; return QColor(p[0], p[1], p[2], p[3]); };

    check(rgba(5, 19).red() > 200 && rgba(5, 19).blue() < 40, "the gradient is red at its start");
    check(rgba(5, 0).blue() > 200 && rgba(5, 0).red() < 40, "and blue at its end");
    check(rgba(25, 5) == QColor(0, 255, 0, 255), "multiply: green × yellow is green");
    check(rgba(25, 15).red() > 200 && rgba(25, 15).green() > 200, "luminosity: white lightness over green is near white");
    check(rgba(42, 5) == QColor(0, 0, 0, 0), "copy clears inside the clip, outside its shape");
    check(rgba(47, 5) == QColor(0, 0, 255, 255), "copy leaves what is outside the clip");
}

/// A <canvas> through the renderer: the ops, the canvas run and the display list image.
static void test_canvas_through_renderer(double dpr) {
    QtBackend backend;
    Renderer r(100, dpr, false, backend);
    r.receive(parse(R"J({"type":"ops","ops":[
      {"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},{"op":"el","id":4,"tag":"body","adopt":"body"},
      {"op":"style","id":4,"name":"margin","value":"0"},
      {"op":"el","id":30,"tag":"canvas","attrs":[["width","100"],["height","50"]]},{"op":"ins","parent":4,"id":30,"before":null},
      {"op":"canvas","id":30,"cmds":[["c","clearRect",0,0,100,50],["g",1,"linear",0,50,0,0],["gs",1,0,"#ff0000"],["gs",1,1,"rgb(255,0,0)"],
        ["s","fillStyle",{"g":1}],["c","fillRect",0,0,100,50]]}]})J"));
    QImage image = QtBackend::make_image(int(r.px_width()), int(r.px_height()));
    backend.paint(r.paint(), image, QTransform::fromScale(dpr, dpr));
    check(r.px_height() >= 50 * dpr, "the canvas takes its height");
    check(near(at(image, int(50 * dpr), int(25 * dpr)), QColor(255, 0, 0)), "the canvas bitmap is painted where the element is");
}

int main(int argc, char** argv) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    test_display_list();
    test_shadow_and_gradients();
    test_canvas_ops();
    test_canvas_through_renderer(1);
    test_canvas_through_renderer(1.5);
    if (failures) {
        std::printf("%d failure(s)\n", failures);
        return 1;
    }
    std::printf("qt backend: all passed\n");
    return 0;
}
