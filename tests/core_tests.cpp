// Drives TwidgetCore without a platform: ops in, events, display lists and layers out, against a
// stub backend that measures text as 8 px per byte.
#include "css.h"
#include "filters.h"
#include "png.h"
#include "renderer.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <set>
#include <string>
#include <thread>
#include <vector>

using twidget::Json;
using namespace twlite;

namespace {

int failures = 0;

void check(bool ok, const std::string& what) {
    std::printf("%s %s\n", ok ? "PASS" : "FAIL", what.c_str());
    if (!ok) failures++;
}

struct StubBackend : Backend {
    int next = 0;
    std::vector<std::vector<CanvasOp>> runs;
    int create_font(const std::string&, float size, int, bool, const std::string&, FontMetrics& out) override {
        out.ascent = size * 0.8f;
        out.descent = size * 0.2f;
        out.height = size * 1.2f;
        out.x_height = size * 0.5f;
        out.zero_width = 8;
        return ++next;
    }
    void delete_font(int) override {}
    float text_width(int, const std::string& text) override { return 8.0f * float(text.size()); }
    bool load_image(int, const std::string&, float&, float&) override { return false; }
    void canvas_run(int, int, int, const std::vector<CanvasOp>& ops) override { runs.push_back(ops); }
    bool canvas_read(int, int, int, int w, int h, std::vector<uint8_t>& rgba) override {
        rgba.assign(size_t(w) * h * 4, 7);
        return true;
    }
};

Json parse(const std::string& s) {
    auto j = Json::parse(s);
    if (!j) {
        std::printf("bad json: %s\n", s.c_str());
        std::exit(2);
    }
    return *j;
}

std::vector<Json> events(const std::vector<Json>& out) {
    std::vector<Json> evs;
    for (const Json& m : out)
        if (m["type"].asString() == "event") evs.push_back(m["ev"]);
    return evs;
}

std::string types(const std::vector<Json>& out) {
    std::string s;
    for (const Json& ev : events(out)) s += ev["type"].asString() + ":" + std::to_string(int(ev["targetId"].asNumber())) + " ";
    return s;
}

/// html(2) head(3) body(4); A(10) > A1(11); B(12) at the right; a style element (20).
const char* PAGE = R"({"type":"ops","ops":[
 {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
 {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
 {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
 {"op":"el","id":20,"tag":"style","attrs":[]},
 {"op":"css","id":20,"text":"#a{width:100px;height:100px;float:left;color:light-dark(#010203,#040506)} #a1{width:50px;height:50px} #b{width:100px;height:100px;float:left} @keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .spin{animation:spin 1s linear infinite} .t{transition:opacity 200ms linear}"},
 {"op":"ins","parent":3,"id":20,"before":null},
 {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},
 {"op":"el","id":11,"tag":"div","attrs":[["id","a1"]]},
 {"op":"el","id":12,"tag":"div","attrs":[["id","b"]]},
 {"op":"ins","parent":4,"id":10,"before":null},
 {"op":"ins","parent":10,"id":11,"before":null},
 {"op":"ins","parent":4,"id":12,"before":null}
]})";

} // namespace

int main() {
    // Unbuffered: a crash still shows the last check that ran.
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    StubBackend backend;
    Renderer r(300, 1, false, backend);
    r.receive(parse(PAGE));
    r.receive(parse(R"({"type":"listen","types":["mouseover","mouseout","mouseenter","mouseleave","pointerenter","pointermove","dblclick","wheel","keydown","animationstart","animationiteration","transitionend"]})"));

    // ── light-dark() ──
    check(r.last_html().find("#010203") != std::string::npos && r.last_html().find("light-dark") == std::string::npos, "light-dark() resolves to the light side in the HTML");
    {
        std::vector<Json> out;
        r.set_theme(true, out);
        check(r.last_html().find("#040506") != std::string::npos, "light-dark() follows the theme");
        r.set_theme(false, out);
    }

    // ── hover ──
    auto out = r.hover(0, 10, 10);
    check(types(out) == "mouseover:11 pointerenter:2 mouseenter:2 pointerenter:4 mouseenter:4 pointerenter:10 mouseenter:10 pointerenter:11 mouseenter:11 ", "hover into A1: over on A1, enter on html, body, A, A1, outermost first (got " + types(out) + ")");
    check(r.wants_tick(), "the hover's mousemove waits for the frame");
    out = r.tick();
    check(types(out) == "pointermove:11 ", "the frame sends the listened move, not the unlistened mousemove (got " + types(out) + ")");
    out = r.hover(0, 150, 10);
    auto evs = events(out);
    check(types(out) == "mouseout:11 mouseleave:11 mouseleave:10 mouseover:12 pointerenter:12 mouseenter:12 ", "A1 to B: out, leave A1 then A, over, enter B only (got " + types(out) + ")");
    check(!evs.empty() && evs[0]["relatedId"].asNumber() == 12, "mouseout names the element entered");
    out = r.hover(1, 0, 0);
    check(types(out) == "pointermove:12 mouseout:12 mouseleave:12 mouseleave:4 mouseleave:2 ", "leaving the view: the pending move first, then B, body, html (got " + types(out) + ")");

    // ── clicks ──
    r.touch(0, 150, 10, 0, true, true, 0);
    out = r.touch(2, 150, 10, 0, true, true, 0);
    r.touch(0, 151, 10, 0, true, true, 0);
    auto second = r.touch(2, 151, 10, 0, true, true, 0);
    auto ev2 = events(second);
    bool dbl = false;
    int detail = 0;
    for (const Json& e : ev2) {
        if (e["type"].asString() == "dblclick") dbl = true;
        if (e["type"].asString() == "click") detail = e["detail"].asInt();
    }
    check(dbl && detail == 2 && types(second).find("click:12 dblclick:12") != std::string::npos, "two quick clicks: click detail 2, then dblclick (got " + types(second) + ")");
    check(types(second).find("touch") == std::string::npos, "a mouse sends no touch events");
    r.touch(0, 10, 10, 0, true, true, 0);
    out = r.touch(2, 150, 10, 0, true, true, 0);
    check(types(out).find("click") == std::string::npos, "press on A1, release on B: no click");
    r.touch(0, 10, 10, 0, true, true, 0);
    out = r.touch(2, 80, 80, 0, true, true, 0);
    bool outer = false;
    for (const Json& e : events(out))
        if (e["type"].asString() == "click") outer = e["targetId"].asNumber() == 10;
    check(outer, "press on A1, release on A: the click goes to A");

    // ── wheel and keys: profile 1 has neither, listened to or not ──
    r.wheel(150, 10, 0, 50, 0);
    out = r.tick();
    check(events(out).empty(), "no wheel reaches the widget");
    out = r.key("keydown", "a", "KeyA", false, 8);
    check(events(out).empty(), "no key reaches the widget");

    // ── CSS animation ──
    out = r.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":12,"name":"class","value":"spin"}]})"));
    check(types(out).find("animationstart:12") != std::string::npos, "adding the class starts the animation (got " + types(out) + ")");
    check(r.last_html().find("-lh-anim:1;transform:rotate(") != std::string::npos, "the sampled transform is written after the marker");
    check(r.wants_tick(), "an infinite animation keeps the clock ticking");
    std::string before = r.last_html();
    std::this_thread::sleep_for(std::chrono::milliseconds(120));
    int patched = r.stats().patched;
    r.tick();
    check(r.stats().patched == patched + 1, "an animation frame patches style holes instead of rebuilding");
    check(r.dirty(), "an animation frame asks for a paint");

    // ── transition ──
    r.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":11,"name":"class","value":"t"},{"op":"style","id":11,"name":"opacity","value":"1"}]})"));
    r.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":11,"name":"opacity","value":"0"}]})"));
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    r.tick();
    auto html = r.last_html();
    check(html.find("data-twid=\"11\"") != std::string::npos, "A1 still there");
    std::this_thread::sleep_for(std::chrono::milliseconds(150));
    bool ended = false;
    for (int i = 0; i < 5 && !ended; i++) {
        out = r.tick();
        ended = types(out).find("transitionend:11") != std::string::npos;
        std::this_thread::sleep_for(std::chrono::milliseconds(30));
    }
    check(ended, "the opacity transition ends with transitionend");

    // ── Web Animations ──
    out = r.receive(parse(R"({"type":"anim","now":0,"anims":[{"aid":1,"target":10,"keyframes":[{"offset":0,"values":{"opacity":"0"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"both"},"startTime":-500}]})"));
    html = r.last_html();
    size_t at = html.find("data-twid=\"10\"");
    std::string tag = at == std::string::npos ? "" : html.substr(at, html.find('>', at) - at);
    check(tag.find("opacity:0.5") != std::string::npos || tag.find("opacity:0.49") != std::string::npos || tag.find("opacity:0.50") != std::string::npos, "a Web Animation half way through samples opacity 0.5 (" + tag + ")");
    r.receive(parse(R"({"type":"anim","now":0,"anims":[{"aid":1,"remove":true}]})"));
    html = r.last_html();
    at = html.find("data-twid=\"10\"");
    tag = html.substr(at, html.find('>', at) - at);
    check(tag.find("opacity") == std::string::npos, "removing the record drops its value");
    Json q = r.receive(parse(R"({"type":"query","qid":1,"query":{"q":"anims","id":12}})"))[0];
    check(q["result"]["list"].arr().size() == 1 && q["result"]["list"].arr()[0]["name"].asString() == "spin", "the anims query reports the running CSS animation");

    // ── SVG geometry ──
    r.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":40,"tag":"svg","attrs":[["width","40"],["height","40"]]},{"op":"el","id":41,"tag":"line","attrs":[["x1","0"],["y1","0"],["x2","30"],["y2","40"]]},
        {"op":"ins","parent":4,"id":40,"before":null},{"op":"ins","parent":40,"id":41,"before":null}]})"));
    q = r.receive(parse(R"({"type":"query","qid":2,"query":{"q":"svg","id":41,"method":"getTotalLength","args":[]}})"))[0];
    check(std::abs(q["result"].asNumber() - 50) < 0.01, "getTotalLength of a 30×40 line is 50 (got " + std::to_string(q["result"].asNumber()) + ")");
    q = r.receive(parse(R"({"type":"query","qid":3,"query":{"q":"svg","id":41,"method":"getPointAtLength","args":[25]}})"))[0];
    check(std::abs(q["result"]["x"].asNumber() - 15) < 0.01 && std::abs(q["result"]["y"].asNumber() - 20) < 0.01, "getPointAtLength half way is the midpoint");
    q = r.receive(parse(R"({"type":"query","qid":4,"query":{"q":"svg","id":41,"method":"getBBox","args":[]}})"))[0];
    check(std::abs(q["result"]["width"].asNumber() - 30) < 0.01 && std::abs(q["result"]["height"].asNumber() - 40) < 0.01, "getBBox spans the line");
    r.receive(parse(R"({"type":"ops","ops":[{"op":"rm","id":40}]})"));

    // ── canvas ──
    r.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"canvas","attrs":[["width","100"],["height","50"]]},{"op":"ins","parent":4,"id":30,"before":null},
        {"op":"canvas","id":30,"cmds":[["s","fillStyle","#ff0000"],["c","translate",10,5],["c","fillRect",0,0,20,10],["c","beginPath"],["c","arc",50,20,10,0,6.2832],["s","lineWidth",4],["c","stroke"],
        ["pn",1,"M0 0 L40 0 L40 40 Z"],["c","fillText","hi",0,0],["g",1,"linear",0,0,10,0],["gs",1,0,"red"],["gs",1,1,"blue"],["s","fillStyle",{"g":1}],["c","fill",{"p":1}]]}]})"));
    size_t runs = backend.runs.size();
    check(runs >= 2, "the canvas resets, then runs its batch");
    if (runs >= 2) {
        const auto& ops = backend.runs.back();
        check(ops.size() == 4, "fillRect, stroke, fillText and fill(Path2D): 4 ops (got " + std::to_string(ops.size()) + ")");
        if (ops.size() == 4) {
            check(ops[0].kind == CanvasOp::Fill && ops[0].paint.color.r == 255 && ops[0].path.points.size() == 8 && ops[0].path.points[0] == 10 && ops[0].path.points[1] == 5, "fillRect: red, translated into canvas px");
            check(ops[1].kind == CanvasOp::Stroke && ops[1].line_width == 4 && ops[1].path.ops.size() >= 5, "arc stroke: béziers, width 4");
            check(ops[2].kind == CanvasOp::Text && ops[2].text == "hi" && ops[2].matrix[4] == 10, "fillText under the transform");
            check(ops[3].kind == CanvasOp::Fill && ops[3].paint.kind == CanvasPaint::Linear && ops[3].paint.stops.size() == 2 && ops[3].path.points[0] == 10, "Path2D filled with a gradient, transformed at fill time");
        }
    }
    check(r.last_html().find("src=\"tw-canvas:30/100x50\"") != std::string::npos, "the canvas is laid out as an image of its bitmap");
    // On the fork's image they would be its style: with a CSS width alone the height would stay at the attribute.
    check(r.last_html().find("width=\"100\"") == std::string::npos && r.last_html().find("height=\"50\"") == std::string::npos,
          "a drawn canvas's width and height are its natural size, not attributes of the image");
    q = r.receive(parse(R"({"type":"query","qid":2,"query":{"q":"canvasCall","id":30,"method":"isPointInPath","args":[{"p":1},30,15]}})"))[0];
    check(q["result"].asBool(), "isPointInPath on a Path2D, under the transform");
    q = r.receive(parse(R"({"type":"query","qid":3,"query":{"q":"canvasCall","id":30,"method":"getTransform","args":[]}})"))[0];
    check(q["result"].arr().size() == 6 && q["result"].arr()[4].asNumber() == 10, "getTransform");
    q = r.receive(parse(R"({"type":"query","qid":4,"query":{"q":"imageData","id":30,"x":0,"y":0,"w":2,"h":1}})"))[0];
    check(q["binaryBase64"].asString() == "BwcHBwcHBwc=" && q["result"]["width"].asNumber() == 2, "getImageData returns the backend's pixels as base64");
    {
        int builds = r.stats().builds;
        r.paint();
        r.receive(parse(R"({"type":"ops","ops":[{"op":"canvas","id":30,"cmds":[["c","fillRect",0,0,5,5]]},{"op":"prop","id":30,"name":"scrollTop","value":4}]})"));
        check(r.stats().builds == builds && r.dirty(), "a canvas-only batch repaints without building the document");
    }
    {
        // A widget that makes gradients every frame keeps drawing them once there are more than the cap.
        std::string cmds;
        for (int g = 2; g <= 5001; g++) cmds += "[\"g\"," + std::to_string(g) + ",\"linear\",0,0,10,0],[\"gs\"," + std::to_string(g) + ",0,\"red\"],";
        r.receive(parse(R"({"type":"ops","ops":[{"op":"canvas","id":30,"cmds":[)" + cmds + R"(["s","fillStyle",{"g":5001}],["c","fillRect",0,0,5,5]]}]})"));
        const auto& ops = backend.runs.back();
        check(!ops.empty() && ops.back().kind == CanvasOp::Fill && ops.back().paint.kind == CanvasPaint::Linear, "the newest gradient still paints past the object cap");
    }
    r.receive(parse(R"({"type":"ops","ops":[{"op":"rm","id":30}]})"));
    check(r.last_html().find("tw-canvas") == std::string::npos, "a removed canvas is gone");

    // ── names ──
    out = r.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":12,"name":"color:red;x","value":"blue"}]})"));
    bool violation = false;
    for (const Json& m : out) violation = violation || m["type"].asString() == "violation";
    check(violation && r.last_html().find("color:red;x") == std::string::npos, "a property name that would break out of its declaration is refused");

    // ── inline style: the attribute and the style ops are one list ──
    r.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":60,"tag":"div","attrs":[["style","position:absolute;left:3px;top:-83px;width:56px"]]},{"op":"ins","parent":4,"id":60,"before":null},
        {"op":"style","id":60,"name":"position","value":""},{"op":"style","id":60,"name":"top","value":""},{"op":"style","id":60,"name":"margin","value":"3px"}]})"));
    {
        std::string page = r.last_html();
        size_t start = page.find("data-twid=\"60\"");
        std::string tag = page.substr(start, page.find('>', start) - start);
        check(tag.find("position") == std::string::npos && tag.find("top:") == std::string::npos && tag.find("left:3px") != std::string::npos && tag.find("margin:3px") != std::string::npos,
              "a style op removes what the style attribute set (" + tag + ")");
    }
    r.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":60,"name":"style","value":"color:red"}]})"));
    {
        std::string page = r.last_html();
        size_t start = page.find("data-twid=\"60\"");
        std::string tag = page.substr(start, page.find('>', start) - start);
        check(tag.find("color:red") != std::string::npos && tag.find("margin") == std::string::npos, "setting the style attribute replaces every inline declaration (" + tag + ")");
    }

    // ── compositor layers ──
    {
        StubBackend layered_backend;
        Renderer c(300, 2, false, layered_backend);
        c.set_compositing(true);
        // #s spins inside its 20 px margin, so nothing painted later meets it; #w animates width.
        c.receive(parse(R"({"type":"ops","ops":[
         {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
         {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
         {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
         {"op":"el","id":20,"tag":"style","attrs":[]},
         {"op":"css","id":20,"text":"#s{width:40px;height:40px;margin:20px;background:#ff0000;animation:spin 1s linear infinite} #t{height:30px;background:#00ff00;transition:opacity 400ms linear} #w{height:10px;background:#0000ff;animation:grow 1s infinite} @keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} @keyframes grow{from{width:10px}to{width:90px}}"},
         {"op":"ins","parent":3,"id":20,"before":null},
         {"op":"el","id":40,"tag":"div","attrs":[["id","s"]]},
         {"op":"el","id":41,"tag":"div","attrs":[["id","t"]]},
         {"op":"ins","parent":4,"id":40,"before":null},
         {"op":"ins","parent":4,"id":41,"before":null}
        ]})"));
        DisplayList main = c.paint();
        const Composition& comp = c.composition();
        const Layer* spin = comp.layers.size() == 1 && comp.layers[0].id == 40 ? &comp.layers[0] : nullptr;
        check(spin != nullptr, "the endless rotation becomes the one layer (" + std::to_string(comp.layers.size()) + " layers)");
        auto red = [](const DisplayList& list) {
            for (const Command& cmd : list)
                if (cmd.kind == Cmd::FillRect && cmd.color.r == 255 && cmd.color.g == 0) return true;
            return false;
        };
        check(!red(main), "the document's paint leaves the layer's element out");
        if (spin) {
            check(red(spin->content), "the layer's content paints the element");
            check(spin->track.repeat && std::fabs(spin->track.duration - 1) < 1e-6 && spin->track.keys.size() >= 3, "one second, looping, " + std::to_string(spin->track.keys.size()) + " keys");
            // A quarter turn about the centre (40, 40) takes the top left corner (20, 20) to the top right.
            auto at = [&](float t, float& x, float& y) {
                const auto& keys = spin->track.keys;
                for (size_t i = 0; i + 1 < keys.size(); i++) {
                    if (t < keys[i].t || t > keys[i + 1].t) continue;
                    float u = (t - keys[i].t) / (keys[i + 1].t - keys[i].t);
                    const Affine &a = keys[i].matrix, &b = keys[i + 1].matrix;
                    x = (1 - u) * a.apply_x(20, 20) + u * b.apply_x(20, 20);
                    y = (1 - u) * a.apply_y(20, 20) + u * b.apply_y(20, 20);
                    return;
                }
            };
            float x = 0, y = 0;
            at(0.25f, x, y);
            check(std::fabs(x - 60) < 0.2f && std::fabs(y - 20) < 0.2f, "a quarter of the track is a quarter turn about the centre (" + std::to_string(x) + ", " + std::to_string(y) + ")");
            check(spin->bounds.x <= 20 && spin->bounds.y <= 20 && spin->bounds.x + spin->bounds.w >= 60, "the layer's bounds hold its box");
            check(!spin->has_clip && spin->raster_scale == 1, "no clip, no extra raster scale");
            double span = 0;
            auto resumed = resume_track(spin->track, 0.25 + 3, span);
            check(span == 1 && resumed.front().t == 0 && resumed.back().t == 1 && std::fabs(resumed.front().matrix.apply_x(20, 20) - 60) < 0.2f &&
                      resumed.back().matrix == resumed.front().matrix,
                  "resumed 3.25 s in, the loop starts and ends at the quarter turn");
            LayerTrack once = spin->track;
            once.repeat = false;
            auto rest = resume_track(once, 0.5, span);
            check(std::fabs(span - 0.5) < 1e-9 && rest.front().t == 0 && std::fabs(rest.front().matrix.apply_x(20, 20) - 60) < 0.2f, "a one-shot resumed half way keeps its second half");
        }
        auto element = [&](int id) {
            std::string page = c.last_html();
            size_t start = page.find("data-twid=\"" + std::to_string(id) + "\"");
            return start == std::string::npos ? std::string() : page.substr(start, page.find('>', start) - start);
        };
        check(element(40).find("transform") == std::string::npos, "the document gets no per-frame transform for a layer (" + element(40) + ")");
        check(!c.wants_tick(), "a layer alone needs no frames");
        check(std::isinf(c.next_tick_in()), "nor a wake-up, with no animationiteration listener");
        uint64_t version = comp.version;
        std::this_thread::sleep_for(std::chrono::milliseconds(30));
        c.tick();
        c.paint();
        check(c.composition().version == version, "a frame later the composition is the same");

        c.receive(parse(R"({"type":"listen","types":["animationiteration"]})"));
        c.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":41,"name":"opacity","value":"1"}]})"));
        c.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":41,"name":"opacity","value":"0"}]})"));
        c.paint();
        const Layer* fade = nullptr;
        for (const Layer& l : c.composition().layers)
            if (l.id == 41) fade = &l;
        check(fade != nullptr, "the opacity transition becomes a layer too");
        if (fade) {
            const auto& keys = fade->track.keys;
            check(!fade->track.repeat && std::fabs(fade->track.duration - 0.4) < 0.05 && keys.front().opacity > 0.9f && keys.back().opacity < 0.1f,
                  "it plays once, 1 to 0, over what is left of 400 ms");
        }
        double wake = c.next_tick_in();
        check(wake > 0.2 && wake < 1.01, "the core wakes for the next animationiteration or the transition's end (" + std::to_string(wake) + " s)");

        // More painted over the spinner's sweep than can be made still layers: back to per-frame paint.
        std::string cover;
        for (int k = 0; k < 4; k++)
            cover += std::string(k ? "," : "") + R"({"op":"el","id":)" + std::to_string(42 + k * 10) + R"(,"tag":"div","attrs":[["style","position:absolute;left:)" + std::to_string(k * 8) +
                     R"(px;top:0;width:30px;height:30px;background:#000"]]},{"op":"ins","parent":4,"id":)" + std::to_string(42 + k * 10) + R"(,"before":null})";
        c.receive(parse(R"({"type":"ops","ops":[)" + cover + "]}"));
        c.paint();
        bool spinning = false;
        for (const Layer& l : c.composition().layers) spinning = spinning || l.id == 40;
        check(!spinning, "content painted over the sweep keeps the element in the document");
        check(c.wants_tick(), "which then needs frames again");

        // Width cannot be composited.
        c.receive(parse(R"({"type":"ops","ops":[{"op":"rm","id":42},{"op":"rm","id":52},{"op":"rm","id":62},{"op":"rm","id":72},{"op":"el","id":43,"tag":"div","attrs":[["id","w"]]},{"op":"ins","parent":4,"id":43,"before":null}]})"));
        c.paint();
        bool grows = false;
        for (const Layer& l : c.composition().layers) grows = grows || l.id == 43;
        check(!grows && c.last_html().find("width:") != std::string::npos, "a width animation stays in the document");

        c.set_compositing(false);
        c.paint();
        check(c.composition().layers.empty() && element(40).find("transform:rotate(") != std::string::npos, "compositing off: no layers, the transform is sampled again");
    }

    // ── a finished animation inside an element does not keep it out of layers ──
    {
        StubBackend lb;
        Renderer c(360, 1, false, lb);
        c.set_compositing(true);
        c.receive(parse(R"({"type":"ops","ops":[
         {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
         {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
         {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
         {"op":"el","id":20,"tag":"style","attrs":[]},
         {"op":"css","id":20,"text":".card{margin:30px;height:60px} .like{width:20px;height:20px} .pop{animation:pop 100ms} .slide{animation:slide 1s} @keyframes pop{from{transform:scale(1.7)}} @keyframes slide{from{opacity:0;transform:translateX(28px)}}"},
         {"op":"ins","parent":3,"id":20,"before":null},
         {"op":"el","id":80,"tag":"div","attrs":[["class","card"]]},
         {"op":"el","id":81,"tag":"div","attrs":[["class","like pop"]]},
         {"op":"ins","parent":4,"id":80,"before":null},
         {"op":"ins","parent":80,"id":81,"before":null}
        ]})"));
        std::this_thread::sleep_for(std::chrono::milliseconds(150));
        c.tick();
        c.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":80,"name":"class","value":"card slide"}]})"));
        c.paint();
        bool card = c.composition().layers.size() == 1 && c.composition().layers[0].id == 80;
        check(card, "a pop that has ended inside the card does not keep the card's slide out of layers");
    }

    // ── a layer paints positioned descendants where the document does ──
    {
        const char* page = R"({"type":"ops","ops":[
         {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
         {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
         {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
         {"op":"el","id":20,"tag":"style","attrs":[]},
         {"op":"css","id":20,"text":".card{padding:12px 14px;border:1px solid #ccc;animation:slide 1s linear infinite} .art{height:64px} .seek{position:relative;height:14px} .prog{position:absolute;left:0;top:5px;width:300px;height:4px;background-color:#7f7f7f} .times{margin-top:2px} @keyframes slide{from{opacity:0;transform:translateX(28px)}}"},
         {"op":"ins","parent":3,"id":20,"before":null},
         {"op":"el","id":70,"tag":"div","attrs":[["class","card"]]},
         {"op":"el","id":71,"tag":"div","attrs":[["class","art"]]},
         {"op":"el","id":72,"tag":"div","attrs":[["class","seek"]]},
         {"op":"el","id":73,"tag":"div","attrs":[["class","prog"]]},
         {"op":"el","id":74,"tag":"div","attrs":[["class","times"]]},
         {"op":"text","id":75,"data":"0:00"},
         {"op":"ins","parent":4,"id":70,"before":null},
         {"op":"ins","parent":70,"id":71,"before":null},
         {"op":"ins","parent":70,"id":72,"before":null},
         {"op":"ins","parent":72,"id":73,"before":null},
         {"op":"ins","parent":70,"id":74,"before":null},
         {"op":"ins","parent":74,"id":75,"before":null}
        ]})";
        auto bar = [](const DisplayList& list, Rect& out) {
            for (const Command& cmd : list)
                if (cmd.kind == Cmd::FillRect && cmd.color.r == 0x7f && cmd.color.g == 0x7f) { out = cmd.rect; return true; }
            return false;
        };
        StubBackend b1, b2;
        Renderer flat(360, 1, false, b1), layered(360, 1, false, b2);
        layered.set_compositing(true);
        flat.receive(parse(page));
        layered.receive(parse(page));
        Rect expected, got;
        bool painted = bar(flat.paint(), expected);
        layered.paint();
        bool lifted = layered.composition().layers.size() == 1 && bar(layered.composition().layers[0].content, got);
        check(painted && lifted, "the card is a layer and its seek bar is in the layer's content");
        if (painted && lifted)
            check(got.x == expected.x && got.y == 82 && expected.y == 82, "the layer and the document both draw the absolutely positioned bar at 82 (layer " + std::to_string(got.y) + ", document " + std::to_string(expected.y) + ")");

        // Groups without an animation: a static opacity, then a static transform on the card.
        for (const char* group : {"opacity:0.5", "transform:translateX(28px)", "opacity:0.5;transform:translateX(28px)"}) {
            StubBackend b3;
            Renderer still(360, 1, false, b3);
            std::string fixed = page;
            fixed.replace(fixed.find("animation:slide 1s linear infinite"), std::strlen("animation:slide 1s linear infinite"), group);
            still.receive(parse(fixed));
            Rect grouped;
            DisplayList list = still.paint();
            bool found = bar(list, grouped);
            int depth = 0, inside = -1;
            for (const Command& cmd : list) {
                if (cmd.kind == Cmd::GroupPush) depth++;
                else if (cmd.kind == Cmd::GroupPop) depth--;
                else if (cmd.kind == Cmd::FillRect && cmd.color.r == 0x7f && cmd.color.g == 0x7f) inside = depth;
            }
            check(inside == 1, std::string("a card with a static ") + group + " paints the bar inside its group");
            check(found && grouped.y == 82, std::string("a card with a static ") + group + " draws the bar at 82 (" + std::to_string(grouped.y) + ")");
        }
    }

    // ── clip-path and fixed elements under an overflow clip ──
    {
        const char* page = R"({"type":"ops","ops":[
         {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},
         {"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
         {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
         {"op":"el","id":20,"tag":"style","attrs":[]},
         {"op":"css","id":20,"text":".box{width:200px;height:200px;overflow:hidden} .round{width:100px;height:100px;clip-path:circle(50%);background-color:#ff0000} .wide{width:300px;height:20px;background-color:#00ff00} .fixed{position:fixed;left:0;top:0;width:300px;height:10px;background-color:#0000ff}"},
         {"op":"ins","parent":3,"id":20,"before":null},
         {"op":"el","id":80,"tag":"div","attrs":[["class","box"]]},
         {"op":"el","id":81,"tag":"div","attrs":[["class","round"]]},
         {"op":"el","id":82,"tag":"div","attrs":[["class","wide"]]},
         {"op":"el","id":83,"tag":"div","attrs":[["class","fixed"]]},
         {"op":"ins","parent":4,"id":80,"before":null},
         {"op":"ins","parent":80,"id":81,"before":null},
         {"op":"ins","parent":80,"id":82,"before":null},
         {"op":"ins","parent":80,"id":83,"before":null}
        ]})";
        StubBackend b;
        Renderer clipped(360, 1, false, b);
        clipped.receive(parse(page));
        int depth = 0, low = 0, wide = -1, fixed = -1, paths = 0;
        for (const Command& cmd : clipped.paint()) {
            if (cmd.kind == Cmd::ClipPush) { depth++; if (cmd.path) paths++; }
            else if (cmd.kind == Cmd::ClipPop) low = std::min(low, --depth);
            else if (cmd.kind == Cmd::FillRect && cmd.color.g == 255) wide = depth;
            else if (cmd.kind == Cmd::FillRect && cmd.color.b == 255 && cmd.color.r == 0) fixed = depth;
        }
        check(paths == 1, "clip-path pushes a path clip");
        check(depth == 0 && low == 0, "clip pushes and pops balance, so clip-path does not pop its parent's clip");
        check(wide >= 1, "the element after the clip-path one is still clipped by the overflow box (depth " + std::to_string(wide) + ")");
        check(fixed >= 0 && fixed < wide, "a fixed element escapes its ancestor's overflow clip (depth " + std::to_string(fixed) + ")");
        auto at = [&](int x, int y) {
            return clipped.receive(parse("{\"type\":\"query\",\"qid\":9,\"query\":{\"q\":\"elementFromPoint\",\"x\":" + std::to_string(x) + ",\"y\":" + std::to_string(y) + "}}"))[0]["result"].asNumber();
        };
        check(at(50, 50) == 81 && at(4, 14) != 81, "hits inside the circle reach it, the corner outside does not (" + std::to_string(at(50, 50)) + ", " + std::to_string(at(4, 14)) + ")");
    }

    // ── JSON for the Android and web painters ──
    {
        DisplayList list;
        Command group;
        group.kind = Cmd::GroupPush;
        group.matrix[0] = 0.70710678f;
        group.opacity = 0.5f;
        list.push_back(group);
        Command shape;
        shape.kind = Cmd::Path;
        auto p = std::make_shared<Path>();
        p->ops = {Path::Move, Path::Line, Path::Close};
        p->points = {0, 0, 10, 0};
        p->fill = true;
        p->fill_color = Color{255, 0, 0, 255};
        p->stroke = true;
        p->stroke_width = 2;
        p->dashes = {4, 2};
        shape.path = p;
        list.push_back(shape);
        Command clip;
        clip.kind = Cmd::ClipPush;
        clip.path = p;
        list.push_back(clip);
        Command shadow;
        shadow.kind = Cmd::Shadow;
        shadow.rect = Rect{1, 2, 3, 4};
        shadow.inset = true;
        shadow.bx = 6;
        list.push_back(shadow);
        list.push_back(Command{Cmd::ClipPop});
        list.push_back(Command{Cmd::GroupPop});
        auto json = Json::parse(to_json(list));
        bool parsed = json && json->arr().size() == 6;
        check(parsed, "the display list JSON parses");
        if (parsed) {
            const Json& j = *json;
            check(j[size_t(0)]["k"].asInt() == int(Cmd::GroupPush) && std::abs(j[size_t(0)]["m"][size_t(0)].asNumber() - 0.70710678) < 1e-6 && j[size_t(0)]["o"].asNumber() == 0.5,
                  "a group keeps its matrix exact and its opacity");
            const Json& path = j[size_t(1)]["p"];
            check(path["o"].arr().size() == 3 && path["v"].arr().size() == 4 && path["f"].asNumber() == double(0xffff0000u) && path["sw"].asNumber() == 2 && path["d"].arr().size() == 2,
                  "an SVG path carries its geometry, fill and dashed stroke");
            check(j[size_t(2)]["p"]["o"].arr().size() == 3 && !j[size_t(2)]["p"].find("f"), "a path clip carries the geometry only");
            check(j[size_t(3)]["n"].asInt() == 1 && j[size_t(3)]["a"][size_t(2)].asNumber() == 6 && j[size_t(3)]["r"][size_t(3)].asNumber() == 4, "a shadow carries inset, blur and box");
        }

        std::vector<CanvasOp> ops(2);
        ops[0].kind = CanvasOp::Fill;
        ops[0].path.ops = {Path::Move, Path::Line};
        ops[0].path.points = {0, 0, 5, 5};
        ops[0].paint.kind = CanvasPaint::Linear;
        ops[0].paint.stops = {Stop{0, Color{}}, Stop{1, Color{255, 255, 255, 255}}};
        ops[0].composite = Composite::Multiply;
        ops[0].clips = std::make_shared<std::vector<CanvasClip>>(1);
        ops[1].kind = CanvasOp::PutImage;
        ops[1].w = ops[1].h = 1;
        ops[1].rgba = std::make_shared<std::vector<uint8_t>>(std::vector<uint8_t>{1, 2, 3, 4});
        auto canvas = Json::parse(to_json(ops));
        bool read = canvas && canvas->arr().size() == 2;
        check(read, "the canvas JSON parses");
        if (read) {
            const Json& c = *canvas;
            check(c[size_t(0)]["pk"].asInt() == 1 && c[size_t(0)]["s"].arr().size() == 4 && c[size_t(0)]["cm"].asInt() == int(Composite::Multiply) && c[size_t(0)]["cl"].arr().size() == 1 && !c[size_t(0)].find("a"),
                  "a fill carries its paint, blend mode and clip, and no alpha when opaque");
            check(c[size_t(1)]["px"].asString() == "AQIDBA==", "putImageData pixels are base64");
        }
    }

    // ── coalescing ──
    {
        StubBackend cb2;
        Renderer c(300, 1, false, cb2);
        c.receive(parse(PAGE));
        c.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":12,"name":"class","value":"spin"}]})"));
        int builds = c.stats().builds;
        for (int i = 0; i < 3; i++) c.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":4,"name":"padding-top","value":")" + std::to_string(1 + i) + R"(px"}]})"));
        check(c.stats().builds == builds && c.wants_tick(), "while something animates, batches wait for the frame");
        int patched = c.stats().in_place, serialised = c.stats().builds - c.stats().patched;
        c.tick();
        // One build of the three batches (in place), and the animation's own style patches.
        check(c.stats().in_place == patched + 1 && c.stats().builds - c.stats().patched == serialised, "and the frame builds them once, in place");
        c.touch(0, 250, 250, 0, true, true, 0);
        int before = c.stats().builds;
        c.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":10,"name":"width","value":"90px"}]})"));
        check(c.stats().builds > before, "a batch right after input builds at once");
        double width = c.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"))[0]["result"]["width"].asNumber();
        check(width == 90, "and a query sees it (" + std::to_string(width) + ")");
    }

    // ── render throttle ──
    {
        StubBackend tb;
        Renderer t(300, 1, false, tb);
        t.receive(parse(PAGE));
        std::this_thread::sleep_for(std::chrono::milliseconds(1100));  // past the load grace
        t.charge(0.05);  // a costly paint: three times as long held back
        int builds = t.stats().builds;
        t.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":4,"name":"padding-top","value":"3px"}]})"));
        t.tick();
        check(t.stats().builds == builds && t.wants_tick(), "after a costly pass, a batch waits out the throttle");
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        t.tick();
        check(t.stats().builds > builds, "and builds once it is over");
    }

    // ── damage ──
    {
        StubBackend db;
        Renderer d(300, 1, false, db);
        d.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"div","attrs":[["style","position:absolute;left:10px;top:10px;width:20px;height:20px;background:red"]]},{"op":"ins","parent":4,"id":5,"before":null},
            {"op":"el","id":6,"tag":"div","attrs":[["style","position:absolute;left:100px;top:10px;width:20px;height:20px;background:blue"]]},{"op":"ins","parent":4,"id":6,"before":null},
            {"op":"el","id":7,"tag":"div","attrs":[["style","position:absolute;left:105px;top:15px;width:20px;height:20px;background:green"]]},{"op":"ins","parent":4,"id":7,"before":null},
            {"op":"el","id":8,"tag":"canvas","attrs":[["width","10"],["height","10"],["style","position:absolute;left:200px;top:10px"]]},{"op":"ins","parent":4,"id":8,"before":null}]})"));
        std::vector<Rect> rects;
        d.paint();
        check(!d.damage(rects), "the first paint is all damage");
        d.paint();
        check(d.damage(rects) && rects.empty(), "an unchanged paint damages nothing");
        auto covers = [&](float x, float y, float w, float h) {
            for (const Rect& r : rects)
                if (r.x <= x && r.y <= y && r.x + r.w >= x + w && r.y + r.h >= y + h) return true;
            return false;
        };
        auto within = [&](float x, float y, float w, float h) {
            for (const Rect& r : rects)
                if (r.x < x || r.y < y || r.x + r.w > x + w || r.y + r.h > y + h) return false;
            return true;
        };
        d.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":5,"name":"background-color","value":"yellow"}]})"));
        d.paint();
        check(d.damage(rects) && covers(10, 10, 20, 20) && within(5, 5, 30, 30), "a new colour damages its box and nothing else");
        d.receive(parse(R"({"type":"ops","ops":[{"op":"canvas","id":8,"cmds":[["c","fillRect",0,0,5,5]]}]})"));
        d.paint();
        check(d.damage(rects) && covers(200, 10, 10, 10) && within(195, 5, 20, 20), "a canvas that drew damages itself");
        d.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":6,"name":"z-index","value":"2"},{"op":"style","id":7,"name":"z-index","value":"1"}]})"));
        d.paint();
        check(d.damage(rects) && covers(105, 15, 15, 15), "boxes that swap paint order damage where they overlap");
        std::string before = to_json(d.paint());
        d.trim();
        std::string after = to_json(d.paint());
        check(!d.damage(rects) && after == before, "after a trim the next paint is whole, and the same");
    }

    // ── height bound ──
    {
        StubBackend hb2;
        Renderer t(300, 1, false, hb2);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"div","attrs":[["style","height:2000px"]]},{"op":"ins","parent":4,"id":5,"before":null}]})"));
        check(t.viewport_height() == 600, "a widget stops at 600 px unless the host says otherwise");
        std::vector<Json> out;
        t.set_max_height(3000, out);
        check(t.viewport_height() == 2000 && !out.empty(), "a host's own bound lets it grow to its content");
    }

    // ── font cache ──
    {
        StubBackend fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","width:50vw"]]},{"op":"text","id":5,"data":"hello"},{"op":"ins","parent":4,"id":5,"before":null}]})"));
        int fonts = fb.next;
        for (int q = 0; q < 2; q++) f.receive(parse(R"({"type":"query","qid":1,"query":{"q":"measureText","font":"12px serif","text":"abc"}})"));
        check(fb.next == fonts + 1, "measureText keeps its font for the next query (" + std::to_string(fb.next - fonts) + " made)");
        std::vector<Json> out;
        f.set_width(200, out);  // viewport lengths: a fresh parse
        check(fb.next == fonts + 1, "a reparsed document finds its fonts made (" + std::to_string(fb.next - fonts - 1) + " more)");
    }

    // ── viewport height ──
    {
        StubBackend gb;
        Renderer g(300, 1, false, gb);
        g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;line-height:20px"]]}]})"));
        uint64_t before = g.layouts();
        for (int i = 0; i < 5; i++)
            g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":)" + std::to_string(10 + i) + R"(,"tag":"div"},{"op":"ins","parent":4,"id":)" + std::to_string(10 + i) +
                            R"(,"before":null},{"op":"text","id":)" + std::to_string(20 + i) + R"(,"data":"row"},{"op":"ins","parent":)" + std::to_string(10 + i) + R"(,"id":)" +
                            std::to_string(20 + i) + R"(,"before":null}]})"));
        check(g.layouts() - before == 5, "a growing document lays out once per batch (" + std::to_string(g.layouts() - before) + " for 5)");
        // A box sized by the viewport's height follows it.
        g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":30,"tag":"div","attrs":[["style","position:absolute;top:0;height:100%;width:5px"]]},{"op":"ins","parent":4,"id":30,"before":null}]})"));
        double tall = g.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":30}})"))[0]["result"]["height"].asNumber();
        check(tall == g.viewport_height(), "a box the viewport's height sizes matches it (" + std::to_string(tall) + " and " + std::to_string(g.viewport_height()) + ")");
    }

    // ── in-place patches ──
    {
        // Random batches into a patching renderer and one that builds every time: the same paint.
        StubBackend pa, pb;
        Renderer patching(300, 1, false, pa), building(300, 1, false, pb);
        building.set_patching(false);
        std::string setup = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style"},
            {"op":"css","id":5,"text":".a{color:red;padding:2px}.b{border:1px solid blue;width:40px}"},{"op":"ins","parent":3,"id":5,"before":null})";
        for (int i = 0; i < 20; i++) {
            std::string e = std::to_string(100 + i), t = std::to_string(200 + i);
            setup += R"(,{"op":"el","id":)" + e + R"(,"tag":"div","attrs":[["class","a"],["style","height:10px"]]},{"op":"ins","parent":4,"id":)" + e +
                     R"(,"before":null},{"op":"text","id":)" + t + R"(,"data":"item"},{"op":"ins","parent":)" + e + R"(,"id":)" + t + R"(,"before":null})";
        }
        setup += "]}";
        patching.receive(parse(setup));
        building.receive(parse(setup));
        uint32_t seed = 7;
        auto next = [&] { return seed = seed * 1664525u + 1013904223u, seed >> 8; };
        int same = 0, rounds = 200;
        int patched_before = patching.stats().in_place;
        for (int round = 0; round < rounds; round++) {
            std::string ops;
            // Mostly what a busy widget sends; now and then what has no hole (a new height, markup in text) builds.
            for (int k = 0; k < 3; k++) {
                std::string e = std::to_string(100 + next() % 20), t = std::to_string(200 + next() % 20);
                if (!ops.empty()) ops += ",";
                unsigned pick = next() % 20;
                if (pick < 6) ops += "{\"op\":\"style\",\"id\":" + e + ",\"name\":\"transform\",\"value\":\"translateX(" + std::to_string(next() % 30) + "px)\"}";
                else if (pick < 11) ops += "{\"op\":\"style\",\"id\":" + e + ",\"name\":\"background-color\",\"value\":\"rgb(" + std::to_string(next() % 255) + ",0,0)\"}";
                else if (pick < 13) ops += "{\"op\":\"attr\",\"id\":" + e + ",\"name\":\"class\",\"value\":\"" + (next() % 2 ? "a" : "a b") + "\"}";
                else if (pick < 18) ops += "{\"op\":\"data\",\"id\":" + t + ",\"data\":\"text " + std::to_string(next() % 1000) + "\"}";
                else if (pick < 19) ops += "{\"op\":\"data\",\"id\":" + t + ",\"data\":\"text &<> " + std::to_string(next() % 1000) + "\"}";
                else ops += "{\"op\":\"style\",\"id\":" + e + ",\"name\":\"height\",\"value\":\"" + std::to_string(5 + next() % 20) + "px\"}";
            }
            std::string batch = R"({"type":"ops","ops":[)" + ops + "]}";
            patching.receive(parse(batch));
            building.receive(parse(batch));
            if (to_json(patching.paint()) == to_json(building.paint()) && patching.last_html() == building.last_html()) same++;
        }
        int patched = patching.stats().in_place - patched_before;
        check(same == rounds, "patched and rebuilt documents paint the same (" + std::to_string(same) + " of " + std::to_string(rounds) + ")");
        check(patched > rounds / 2, "most batches go into the document in place (" + std::to_string(patched) + " of " + std::to_string(rounds) + ")");
    }

    // ── pointer capture ──
    {
        StubBackend cb;
        Renderer c(300, 1, false, cb);
        c.receive(parse(PAGE));
        c.receive(parse(R"({"type":"listen","types":["pointermove","mousemove","pointerup","mouseup","gotpointercapture","lostpointercapture"]})"));
        c.touch(0, 10, 10, 0, true, true, 0);
        auto got = c.receive(parse(R"({"type":"ops","ops":[{"op":"call","id":11,"method":"setPointerCapture","args":[1]}]})"));
        check(types(got).empty(), "setPointerCapture on the pressed mouse: no gotpointercapture, outside profile 1 (got " + types(got) + ")");
        c.touch(1, 150, 10, 0, true, true, 0);
        auto moved = c.tick();
        check(types(moved) == "pointermove:11 mousemove:12 ", "captured: pointermove to the capture, mousemove to the element under the mouse (got " + types(moved) + ")");
        auto up = c.touch(2, 150, 10, 0, true, true, 0);
        check(types(up) == "pointerup:11 mouseup:12 ", "release: pointerup to the capture, mouseup under the mouse (got " + types(up) + ")");
        c.touch(0, 10, 10, 0, true, true, 0);
        auto click = events(c.touch(2, 10, 10, 0, true, true, 0));
        bool actions = false;
        for (const Json& e : click) if (e["type"].asString() == "click") actions = e["defaultActions"].asBool();
        check(actions, "a click asks the sandbox for its default actions");
    }

    // ── compositor coverage ──
    {
        auto layers_of = [](const char* body) {
            StubBackend kb;
            Renderer k(300, 1, false, kb);
            k.set_compositing(true);
            k.receive(parse(std::string(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style"},
                {"op":"css","id":20,"text":"@keyframes spin{from{transform:rotate(0deg)}to{transform:rotate(360deg)}} .s{width:40px;height:40px;background:red;animation:spin 1s linear infinite}"},
                {"op":"ins","parent":3,"id":20,"before":null},)J") + body + "]}"));
            k.paint();
            return k.composition().layers;
        };
        // The spinner sits in two clips; the inner one holds it and the outer one holds the inner.
        auto nested = layers_of(R"J({"op":"el","id":30,"tag":"div","attrs":[["style","overflow:hidden;width:300px;height:300px"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"div","attrs":[["style","overflow:hidden;margin:50px;width:100px;height:100px"]]},{"op":"ins","parent":30,"id":31,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:0"]]},{"op":"ins","parent":31,"id":40,"before":null})J");
        check(nested.size() == 1 && nested[0].has_clip && nested[0].clip.x == 50 && nested[0].clip.w == 100,
              "a layer inside two clips takes the inner one (" + std::to_string(nested.size()) + " layers)");
        // A later box would cover the spinner, but its own overflow:hidden parent cuts it off far below.
        auto clipped = layers_of(R"J({"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","height:150px"]]},{"op":"ins","parent":4,"id":41,"before":null},
            {"op":"el","id":42,"tag":"div","attrs":[["style","overflow:hidden;height:10px"]]},{"op":"ins","parent":4,"id":42,"before":null},
            {"op":"el","id":43,"tag":"div","attrs":[["style","position:relative;top:-200px;height:60px;background:blue"]]},{"op":"ins","parent":42,"id":43,"before":null})J");
        check(clipped.size() == 1 && clipped[0].id == 40, "a box clipped away by its own parent does not cover a layer (" + std::to_string(clipped.size()) + " layers)");
        // An ancestor animating only its own background still lets its child be a layer.
        auto glowing = layers_of(R"J({"op":"el","id":21,"tag":"style"},{"op":"css","id":21,"text":"@keyframes glow{from{background-color:#000}to{background-color:#fff}} .g{padding:20px;animation:glow 1s infinite}"},
            {"op":"ins","parent":3,"id":21,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["class","g"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["class","s"]]},{"op":"ins","parent":30,"id":40,"before":null})J");
        check(glowing.size() == 1 && glowing[0].id == 40, "a parent's background animation leaves its child a layer (" + std::to_string(glowing.size()) + " layers)");
        // A label over a spinner becomes a still layer, so the spinner can be one under it.
        auto labelled = layers_of(R"J({"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","position:absolute;left:25px;top:30px;width:30px;height:12px;background:white"]]},{"op":"ins","parent":4,"id":41,"before":null})J");
        bool spinner = false, label = false;
        for (const Layer& layer : labelled) {
            spinner = spinner || (layer.id == 40 && layer.track.keys.size() > 2);
            label = label || (layer.id == 41 && layer.track.keys.size() == 1);
        }
        check(labelled.size() == 2 && spinner && label && labelled.back().id == 41, "a label over a spinner becomes a still layer above it (" + std::to_string(labelled.size()) + " layers)");
        // Its content leaves out its own transform and opacity: its one key has them.
        auto turned = layers_of(R"J({"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:20px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"div","attrs":[["style","position:absolute;left:25px;top:30px;width:30px;height:12px;background:white;opacity:.5;transform:translateX(4px)"]]},{"op":"ins","parent":4,"id":41,"before":null})J");
        bool kept = turned.size() == 2 && turned.back().id == 41 && turned.back().track.keys.size() == 1 && std::fabs(turned.back().track.keys[0].opacity - 0.5f) < 1e-3f &&
                    std::fabs(turned.back().track.keys[0].matrix.e - 4) < 1e-3f;
        check(kept, "a still layer keeps its element's own transform and opacity");
        {
            // A layer keeps its content while what changes is elsewhere, and records it again when it is inside.
            StubBackend kb;
            Renderer k(300, 1, false, kb);
            k.set_compositing(true);
            k.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style"},
                {"op":"css","id":20,"text":"@keyframes spin{to{transform:rotate(360deg)}} .s{width:40px;height:40px;animation:spin 1s linear infinite}"},{"op":"ins","parent":3,"id":20,"before":null},
                {"op":"el","id":40,"tag":"div","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":40,"before":null},
                {"op":"el","id":41,"tag":"span","attrs":[["style","color:red"]]},{"op":"ins","parent":40,"id":41,"before":null},{"op":"text","id":42,"data":"x"},{"op":"ins","parent":41,"id":42,"before":null},
                {"op":"el","id":50,"tag":"div","attrs":[["style","height:10px;background:blue"]]},{"op":"ins","parent":4,"id":50,"before":null}]})J"));
            k.paint();
            auto version = [&] { return k.composition().layers.empty() ? 0 : k.composition().layers[0].content_version; };
            uint64_t first = version();
            k.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":50,"name":"background-color","value":"green"}]})"));
            k.paint();
            check(first != 0 && version() == first, "a layer keeps its content when something outside it changes");
            k.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":41,"name":"color","value":"blue"}]})"));
            k.paint();
            check(version() != first, "and records it again when something inside it changes");
        }
        // Past the layer cap the topmost spinners are composited and the rest painted.
        std::string many;
        for (int i = 0; i < 300; i++)
            many += R"J({"op":"el","id":)J" + std::to_string(100 + i) + R"J(,"tag":"div","attrs":[["class","s"],["style","display:inline-block;width:10px;height:10px;margin:10px"]]},{"op":"ins","parent":4,"id":)J" +
                    std::to_string(100 + i) + R"J(,"before":null},)J";
        many.pop_back();
        auto capped = layers_of(many.c_str());
        bool top = false, bottom = false;
        for (const Layer& layer : capped) {
            top = top || layer.id == 399;
            bottom = bottom || layer.id == 100;
        }
        check(capped.size() == 256 && top && !bottom, "past the cap, the topmost layers win (" + std::to_string(capped.size()) + " layers)");
        // A layer is hit where its track shows it: half way through a 200 px slide, 100 px along.
        StubBackend hb;
        Renderer h(300, 1, false, hb);
        h.set_compositing(true);
        h.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":20,"tag":"style"},
            {"op":"css","id":20,"text":"@keyframes slide{from{transform:translateX(0)}to{transform:translateX(200px)}} .m{width:40px;height:40px;background:red;animation:slide 1s linear infinite}"},
            {"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["class","m"]]},{"op":"ins","parent":4,"id":40,"before":null}]})J"));
        h.paint();
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
        auto at = [&](int x) { return h.receive(parse(R"({"type":"query","qid":1,"query":{"q":"elementFromPoint","x":)" + std::to_string(x) + R"(,"y":20}})"))[0]["result"].asNumber(); };
        check(h.composition().layers.size() == 1 && at(120) == 40, "a moving layer is hit where it shows");
    }

    // ── layout queries ──
    {
        StubBackend lb;
        Renderer l(300, 1, false, lb);
        l.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font-size:10px;line-height:10px"]]},
            {"op":"el","id":5,"tag":"div","attrs":[["style","position:relative;margin-left:20px;padding:5px;border:1px solid red"]]},{"op":"ins","parent":4,"id":5,"before":null},
            {"op":"el","id":6,"tag":"div","attrs":[["style","margin-left:7px;width:50px;height:10px"]]},{"op":"ins","parent":5,"id":6,"before":null},
            {"op":"el","id":7,"tag":"div","attrs":[["style","width:50px;height:10px;transform:scale(2)"]]},{"op":"ins","parent":4,"id":7,"before":null},
            {"op":"el","id":8,"tag":"textarea","attrs":[["style","height:20px;padding:0;border:0;min-height:0;font-size:10px;line-height:10px"]]},{"op":"ins","parent":4,"id":8,"before":null},
            {"op":"prop","id":8,"name":"value","value":"\na\nb\nc\nd"}]})J"));
        auto query = [&](const char* q, int id) {
            return l.receive(parse(std::string(R"({"type":"query","qid":1,"query":{"q":")") + q + R"(","id":)" + std::to_string(id) + "}}"))[0]["result"];
        };
        Json box = query("box", 6);
        check(box["offsetLeft"].asNumber() == 12 && box["offsetTop"].asNumber() == 5 && box["offsetParent"].asNumber() == 5,
              "offsets are from the positioned parent's padding edge (got " + std::to_string(box["offsetLeft"].asNumber()) + ", " + std::to_string(box["offsetTop"].asNumber()) + ", parent " + std::to_string(box["offsetParent"].asNumber()) + ")");
        check(query("rect", 7)["width"].asNumber() == 100 && query("box", 7)["offsetWidth"].asNumber() == 50, "a transform scales the rect but not the layout size");
        Json area = query("box", 8);
        check(area["clientHeight"].asNumber() == 20 && area["scrollHeight"].asNumber() == 50, "a textarea's scrollHeight is its text's (got " + std::to_string(area["scrollHeight"].asNumber()) + ")");
        check(l.last_html().find("\n\na\nb") != std::string::npos, "a textarea value starting with a newline keeps it through the parser");
    }

    // ── animation-play-state ──
    {
        StubBackend pb;
        Renderer p(300, 1, false, pb);
        p.receive(parse(PAGE));
        auto current = [&] {
            Json q = p.receive(parse(R"({"type":"query","qid":1,"query":{"q":"anims","id":12}})"))[0];
            return q["result"]["list"].arr().empty() ? -1.0 : q["result"]["list"].arr()[0]["currentTime"].asNumber();
        };
        p.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":12,"name":"class","value":"spin"}]})"));
        std::this_thread::sleep_for(std::chrono::milliseconds(60));
        p.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":12,"name":"animation-play-state","value":"paused"}]})"));
        double paused = current();
        std::this_thread::sleep_for(std::chrono::milliseconds(80));
        check(paused >= 50 && current() == paused, "a paused animation's time stands still (" + std::to_string(paused) + " then " + std::to_string(current()) + ")");
        p.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":12,"name":"animation-play-state","value":"running"}]})"));
        double resumed = current();
        check(resumed >= paused && resumed < paused + 40, "resumed, it carries on from where it paused (" + std::to_string(resumed) + ")");
    }

    // ── overflow-wrap ──
    {
        StubBackend wb;
        Renderer w(300, 1, false, wb);
        w.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0;font-size:10px;line-height:10px"]]},
            {"op":"el","id":5,"tag":"div","attrs":[["style","width:40px;overflow-wrap:anywhere"]]},{"op":"ins","parent":4,"id":5,"before":null},
            {"op":"text","id":6,"data":"aaaaaaaaaaaa"},{"op":"ins","parent":5,"id":6,"before":null},
            {"op":"el","id":7,"tag":"div","attrs":[["style","width:40px;overflow-wrap:anywhere"]]},{"op":"ins","parent":4,"id":7,"before":null},
            {"op":"text","id":8,"data":"ááááá"},{"op":"ins","parent":7,"id":8,"before":null}]})"));
        auto height = [&](int id) {
            return w.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"]["height"].asNumber();
        };
        check(height(5) == 30, "a word wider than its box breaks into lines of what fits (got " + std::to_string(height(5)) + ")");
        // Five a + combining acute: 3 bytes, 24 px each in the stub, so one per line, the accent kept with its letter.
        check(height(7) == 50, "a break never splits a letter from its combining mark (got " + std::to_string(height(7)) + ")");
    }

    // ── viewport units ──
    {
        StubBackend vb;
        Renderer v(400, 1, false, vb);
        v.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"div","attrs":[["style","width:50vw;height:10px"]]},{"op":"ins","parent":4,"id":5,"before":null},
            {"op":"el","id":6,"tag":"style"},{"op":"css","id":6,"text":".q{width:25vw;height:10px}"},{"op":"ins","parent":3,"id":6,"before":null},
            {"op":"el","id":7,"tag":"div","attrs":[["class","q"]]},{"op":"ins","parent":4,"id":7,"before":null}]})"));
        auto width = [&](int id) {
            return v.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"]["width"].asNumber();
        };
        check(width(5) == 200 && width(7) == 100, "50vw and 25vw of 400 are 200 and 100");
        std::vector<Json> out;
        v.set_width(200, out);
        check(width(5) == 100, "a resize refolds viewport units in a style attribute (got " + std::to_string(width(5)) + ")");
        check(width(7) == 50, "and in a style sheet (got " + std::to_string(width(7)) + ")");
    }

    // ── scripted layers ──
    {
        StubBackend sb;
        Renderer s(400, 1, false, sb);
        s.set_compositing(true);
        s.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:40px;height:40px;background-color:#f00"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","margin-top:200px;width:300px;height:100px;background-color:#00f"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
        s.paint();
        auto move = [&](int x) {
            s.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":10,"name":"transform","value":"translateX()" + std::to_string(x) + R"J(px)"}]})J"));
        };
        auto layer_x = [&]() -> float {
            const auto& layers = s.composition().layers;
            return layers.size() == 1 && layers[0].id == 10 && layers[0].track.keys.size() == 1 ? layers[0].track.keys[0].matrix.e : -1;
        };
        move(1);
        s.paint();
        move(2);
        s.paint();
        check(s.composition().layers.empty(), "two moves are not a streak yet");
        move(3);
        s.paint();
        check(layer_x() == 3, "a third makes the element a layer, where the script put it");
        int builds = s.stats().builds, reused = s.stats().reused;
        move(4);
        check(s.stats().scripted == 1 && s.stats().builds == builds, "the next move goes to the layer without a build");
        check(s.dirty() && layer_x() == 4, "its track holds the new transform");
        s.paint();
        std::vector<Rect> damage;
        check(s.stats().reused == reused + 1 && s.damage(damage) && damage.empty(), "the paint hands out the last list: the document did not change");
        move(200);
        check(s.stats().builds > builds, "a move past the room the layer has builds");
        s.paint();
        check(layer_x() == 200, "and plans the layer again where it went");
        move(201);
        int settled = s.stats().builds;
        s.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"));
        check(s.stats().builds > settled, "a query first builds what only the layer showed");
        check(s.last_html().find("translateX(201px)") != std::string::npos, "the document has it");
    }

    // ── a crowd moved by script stays in the document ──
    {
        StubBackend cb;
        Renderer c(400, 1, false, cb);
        c.set_compositing(true);
        std::string ops = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]})";
        for (int i = 0; i < 260; i++) {
            std::string id = std::to_string(100 + i);
            ops += R"(,{"op":"el","id":)" + id + R"(,"tag":"div","attrs":[["style","display:inline-block;width:4px;height:4px"]]},{"op":"ins","parent":4,"id":)" + id + R"(,"before":null})";
        }
        c.receive(parse(ops + "]}"));
        for (int f = 0; f < 4; f++) {
            std::string frame = R"({"type":"ops","ops":[)";
            for (int i = 0; i < 260; i++)
                frame += std::string(i ? "," : "") + R"({"op":"style","id":)" + std::to_string(100 + i) + R"(,"name":"transform","value":"translateY()" + std::to_string(f) + R"J(px)"})J";
            c.receive(parse(frame + "]}"));
            c.paint();
        }
        check(c.composition().layers.empty() && c.stats().scripted == 0, "more than the layer budget moving together: no scripted layers");
    }

    // ── shimmer layers ──
    {
        StubBackend hb;
        Renderer h(400, 1, false, hb);
        h.set_compositing(true);
        h.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":".sk{margin:10px;width:200px;height:20px;border-radius:6px;background-color:#ddd;background-image:linear-gradient(90deg,#ddd,#fff,#ddd);background-size:50% 100%;background-repeat:no-repeat;animation:shimmer 1s linear infinite} @keyframes shimmer{from{background-position:-100% 0}to{background-position:200% 0}}"},
            {"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["class","sk"]]},{"op":"ins","parent":4,"id":30,"before":null}]})"));
        DisplayList list = h.paint();
        const auto& layers = h.composition().layers;
        bool layered = layers.size() == 1 && layers[0].id == 30;
        check(layered, "a background-position animation of a lone gradient tile is a layer");
        if (layered) {
            const Layer& l = layers[0];
            auto gradient = [](const DisplayList& d) { return std::count_if(d.begin(), d.end(), [](const Command& c) { return c.kind == Cmd::Linear; }); };
            check(gradient(l.content) == 1 && gradient(list) == 0, "the layer holds the tile, the document the rest of the element");
            check(l.bounds.w == 100 && l.bounds.h == 20 && l.has_clip && l.clip.x == 10 && l.clip.w == 200 && l.clip_radii.tlx == 6, "the tile, under the element's rounded box");
            check(l.track.repeat && l.track.keys.size() >= 2, "sliding forever");
            // -100% of (200 - 100) is -100 px from the box; 200% is 200 px.
            float first = l.track.keys.front().matrix.e, last = l.track.keys.back().matrix.e;
            check(std::fabs(l.bounds.x + first - (10 - 100)) < 0.5f && std::fabs(l.bounds.x + last - (10 + 200)) < 0.5f,
                  "from -100% to 200% of the room (got " + std::to_string(l.bounds.x + first) + " to " + std::to_string(l.bounds.x + last) + ")");
            check(!h.wants_tick(), "and the document needs no frames for it");
        }
    }

    // ── SVG layers ──
    {
        StubBackend vb;
        Renderer v(400, 1, false, vb);
        v.set_compositing(true);
        v.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":40,"tag":"svg","attrs":[["width","60"],["height","60"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"circle","attrs":[["cx","30"],["cy","30"],["r","10"],["fill","#f00"]]},{"op":"ins","parent":40,"id":41,"before":null},
            {"op":"el","id":50,"tag":"div","attrs":[["style","margin-top:100px;height:20px;background-color:#00f"]]},{"op":"ins","parent":4,"id":50,"before":null}]})"));
        v.paint();
        auto grow = [&](int r) {
            v.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":41,"name":"r","value":")" + std::to_string(r) + R"("}]})"));
            return v.paint();
        };
        // Building it counts as the first change.
        grow(11);
        check(v.composition().layers.empty(), "two changes are not a streak yet");
        grow(12);
        bool layered = v.composition().layers.size() == 1 && v.composition().layers[0].id == 40;
        check(layered, "a third makes the SVG a layer");
        uint64_t content = layered ? v.composition().layers[0].content_version : 0;
        grow(14);
        std::vector<Rect> damage;
        check(v.damage(damage) && damage.empty(), "its next change leaves the document as it was");
        check(layered && v.composition().layers[0].content_version != content, "and repaints the layer");
    }

    // ── canvas layers ──
    {
        StubBackend nb;
        Renderer n(400, 1, false, nb);
        n.set_compositing(true);
        n.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":60,"tag":"canvas","attrs":[["width","100"],["height","50"]]},{"op":"ins","parent":4,"id":60,"before":null},
            {"op":"el","id":61,"tag":"div","attrs":[["style","height:20px;background-color:#00f"]]},{"op":"ins","parent":4,"id":61,"before":null}]})"));
        n.paint();
        auto draw = [&](int x) {
            n.receive(parse(R"({"type":"ops","ops":[{"op":"canvas","id":60,"cmds":[["s","fillStyle","#f00"],["c","fillRect",)" + std::to_string(x) + R"(,0,10,10]]}]})"));
            return n.paint();
        };
        draw(0);
        draw(10);
        check(n.composition().layers.empty(), "a canvas drawn twice is in the document");
        draw(20);
        bool layered = n.composition().layers.size() == 1 && n.composition().layers[0].id == 60;
        check(layered, "drawn a third time, it is a layer");
        uint64_t content = layered ? n.composition().layers[0].content_version : 0;
        int builds = n.stats().builds, reused = n.stats().reused;
        DisplayList list = draw(30);
        std::vector<Rect> damage;
        bool in_document = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.canvas != 0; });
        check(!in_document && n.stats().reused == reused + 1 && n.stats().builds == builds && n.damage(damage) && damage.empty(),
              "its next drawing leaves the document's list as it was");
        check(layered && n.composition().layers[0].content_version != content, "and gives the layer a new content version");
    }

    // ── Web Animations on the compositor ──
    {
        StubBackend wb;
        Renderer w(400, 1, false, wb);
        w.set_compositing(true);
        w.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":70,"tag":"div","attrs":[["style","width:40px;height:40px;background-color:#f00"]]},{"op":"ins","parent":4,"id":70,"before":null}]})"));
        w.receive(parse(R"J({"type":"anim","now":0,"anims":[{"aid":1,"target":70,"keyframes":[{"offset":0,"values":{"transform":"translateX(0px)"}},{"offset":1,"values":{"transform":"translateX(100px)"}}],"timing":{"duration":1000,"iterations":null,"easing":"linear"},"startTime":0}]})J"));
        w.paint();
        const auto& layers = w.composition().layers;
        bool layered = layers.size() == 1 && layers[0].id == 70;
        check(layered, "a Web Animation of transform is a layer");
        if (layered) {
            const LayerTrack& t = layers[0].track;
            // Baked from the loop's start, the track has no wrap from 100 back to 0 inside it.
            bool rising = std::is_sorted(t.keys.begin(), t.keys.end(), [](const LayerKey& a, const LayerKey& b) { return a.matrix.e < b.matrix.e; });
            check(t.repeat && std::fabs(t.duration - 1) < 1e-6 && rising && std::fabs(t.keys.front().matrix.e) < 0.01f && std::fabs(t.keys.back().matrix.e - 100) < 0.05f,
                  "its loop is baked from 0 to 100 px (" + std::to_string(t.keys.size()) + " keys)");
        }
        check(!w.wants_tick(), "and the document needs no frames for it");
    }

    // ── nested layers ──
    {
        StubBackend ob;
        Renderer o(400, 1, false, ob);
        o.set_compositing(true);
        o.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":"@keyframes spin{to{transform:rotate(360deg)}} .orbit{margin:40px;width:100px;height:100px;animation:spin 4s linear infinite} .moon{width:20px;height:20px;background:red;animation:spin 1s linear infinite}"},
            {"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["class","orbit"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"div","attrs":[["class","moon"]]},{"op":"ins","parent":30,"id":31,"before":null}]})"));
        o.paint();
        const auto& layers = o.composition().layers;
        bool nested = layers.size() == 2 && layers[0].id == 30 && layers[0].parent == 0 && layers[1].id == 31 && layers[1].parent == 30;
        check(nested, "a spinner inside a spinner is a layer inside its layer (" + std::to_string(layers.size()) + " layers)");
        if (nested) {
            auto has = [](const DisplayList& list, uint8_t red) { return std::any_of(list.begin(), list.end(), [&](const Command& c) { return c.kind == Cmd::FillRect && c.color.r == red && c.color.g == 0; }); };
            check(!has(layers[0].content, 255) && has(layers[1].content, 255), "the parent's content leaves the child out");
            check(to_json(o.composition()).find("\"p\":30") != std::string::npos, "the JSON names the parent");
        }
    }

    // ── focus and the platform's editor ──
    {
        StubBackend fb;
        Renderer f(400, 1, false, fb);
        f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"input","attrs":[["style","width:200px;height:20px;padding:4px"],["placeholder","Name"],["value","ab"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
        f.paint();
        auto tap = [&](double x, double y) {
            f.touch(0, x, y, 0, true, true, 0);
            return f.touch(2, x, y, 0, true, true, 0);
        };
        // Profile 1 drops form controls in the validator and forwards no focus, input, change or blur;
        // the editor still works for a host that hands the renderer a control.
        auto out = tap(50, 10);
        check(types(out).find("focus") == std::string::npos && f.editor(), "a tap on a text input edits it, with no focus event (got " + types(out) + ")");
        const Renderer::EditorSpec* spec = f.editor();
        check(spec && spec->id == 10 && spec->text == "ab" && spec->placeholder == "Name" && spec->input_type == "text" && std::fabs(spec->frame.w - 190) < 0.5f && std::fabs(spec->frame.x - 5) < 0.5f && spec->color.a == 255 && spec->font != 0,
              "the editor spec has the control's content box (inside its padding and border), font, colour, text and placeholder");
        check(f.last_html().find("color:transparent") != std::string::npos && f.last_html().find("placeholder") == std::string::npos,
              "the document stops drawing the edited text and placeholder");
        uint64_t generation = spec ? spec->generation : 0;
        out = f.edit(generation, "abc");
        check(events(out).empty(), "an edit is no input event");
        check(f.last_html().find("value=\"abc\"") != std::string::npos, "and the value is the document's");
        check(f.edit(generation + 100, "x").empty(), "a stale generation is ignored");
        out = tap(50, 50);
        check(types(out).find("change") == std::string::npos && types(out).find("blur") == std::string::npos && !f.editor(), "a tap elsewhere ends it, with no change or blur (got " + types(out) + ")");
        check(f.last_html().find("color:transparent") == std::string::npos, "and the text shows again");
        // A script's focus() right after the user's input takes the keyboard; later it does not.
        out = f.receive(parse(R"({"type":"ops","ops":[{"op":"call","id":10,"method":"focus"}]})"));
        check(f.editor() && f.editor()->id == 10, "focus() just after a tap focuses");
        f.receive(parse(R"({"type":"ops","ops":[{"op":"call","id":10,"method":"setSelectionRange","args":[1,2]}]})"));
        check(f.editor() && f.editor()->selection_start == 1 && f.editor()->selection_end == 2, "setSelectionRange reaches the editor");
        f.end_editing();
        std::this_thread::sleep_for(std::chrono::milliseconds(1100));
        f.receive(parse(R"({"type":"ops","ops":[{"op":"call","id":10,"method":"focus"}]})"));
        check(!f.editor(), "focus() long after the user's input does not");
    }

    // ── widget-sandbox v0.1.0: trusted input, the gesture claim, the theme ──
    {
        StubBackend vb;
        Renderer v(300, 1, false, vb);
        v.receive(parse(PAGE));
        v.touch(0, 10, 10, 0, true, true, 0);
        bool trusted = false;
        for (const Json& m : v.touch(2, 10, 10, 0, true, true, 0))
            if (m["type"].asString() == "event" && m["ev"]["type"].asString() == "click") trusted = m["trusted"].asBool();
        check(trusted, "a click is trusted, as the hosts require before they count a gesture");
    }

    // ── widget-sandbox v0.3.0: a finger's press counts from its release; a widget's canvas pixels together ──
    {
        StubBackend fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(PAGE));
        f.receive(parse(R"({"type":"listen","types":["pointerdown","mousedown","pointerup","click"]})"));
        auto trust = [](const std::vector<Json>& out) {
            std::string s;
            for (const Json& m : out)
                if (m["type"].asString() == "event") s += m["ev"]["type"].asString() + (m["trusted"].asBool() ? "+ " : "- ");
            return s;
        };
        std::string finger = trust(f.touch(0, 10, 10, 0, true, false, 0));
        finger += trust(f.touch(2, 10, 10, 0, true, false, 0));
        std::string mouse = trust(f.touch(0, 10, 10, 0, true, true, 0));
        mouse += trust(f.touch(2, 10, 10, 0, true, true, 0));
        check(finger == "pointerdown- mousedown- pointerup+ click+ ", "a finger's press is no gesture, its release and click are (got " + finger + ")");
        check(mouse == "pointerdown+ mousedown+ pointerup+ click+ ", "a mouse's press is a gesture (got " + mouse + ")");
    }
    {
        struct Pixels : CanvasHost {
            int64_t others = 0;
            int canvas_font(const std::string&, FontMetrics&) override { return 0; }
            float canvas_text_width(int, const std::string&) override { return 0; }
            bool canvas_source(int64_t, int&, int&, float&, float&) override { return false; }
            int64_t canvas_allowance(int) override { return Canvas::MAX_PIXELS - others; }
        } host;
        host.others = Canvas::MAX_PIXELS - 2000 * 2000;
        Canvas c(1, 2000, 2000, host);
        std::vector<CanvasOp> ops;
        c.resize(2000, 2400, ops);
        check(c.width() == 2000 && c.height() == 2000 && !ops.empty() && ops.back().kind == CanvasOp::Reset,
              "a size past the widget's canvas pixels keeps the canvas's own, cleared");
        host.others = Canvas::MAX_PIXELS;
        Canvas fresh(2, 300, 150, host);
        check(fresh.width() == 0 && fresh.height() == 0, "a new canvas past them is 0 x 0");
    }
    {
        StubBackend gb;
        Renderer g(300, 1, false, gb);
        g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","height:40px;touch-action:pan-y"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","height:40px;touch-action:none"]]},{"op":"ins","parent":4,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"div","attrs":[["style","height:40px"]]},{"op":"ins","parent":4,"id":13,"before":null}]})"));
        g.receive(parse(R"({"type":"preventDefault","keys":["13:pointerdown"]})"));
        // "xy" for the axes claimed; "!" when `wants` is not `y`
        auto claim = [&](double y) {
            auto out = g.touch(0, 10, y, 0, true, true, 0);
            g.touch(3, 10, y, 0, true, true, 0);
            for (const Json& m : out)
                if (m["type"].asString() == "gesture")
                    return std::string(m["x"].asBool() ? "x" : "-") + (m["y"].asBool() ? "y" : "-") + (m["wants"].asBool() == m["y"].asBool() ? "" : "!");
            return std::string("none");
        };
        std::string pan_y = claim(20), none = claim(60), plain = claim(100), prevented = claim(140);
        // Vertical drags are always the chat's (v0.1.3): nothing claims `y`.
        check(pan_y == "x-", "touch-action: pan-y claims horizontal drags (got " + pan_y + ")");
        check(none == "x-", "touch-action: none claims horizontal drags, not vertical ones (got " + none + ")");
        check(plain == "--", "no touch-action leaves the touch to the chat (got " + plain + ")");
        check(prevented == "x-", "a prevented pointerdown claims horizontal drags (got " + prevented + ")");
    }
    {
        StubBackend tb;
        Renderer t(300, 1, false, tb);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"button","attrs":[["style","width:100px;height:30px"]]},{"op":"ins","parent":4,"id":10,"before":null}]})"));
        t.receive(parse(R"({"type":"theme","theme":{"colorScheme":"light","params":{"bg_color":"#12AB34","text_color":"red"},
            "button":{"bg":"#ff0000","radius":"12px","font":"14px Arial","padding":"8px  14px","pressedOpacity":0.6,"disabledOpacity":"2"}}})"));
        std::string html = t.last_html();
        auto has = [&](const char* s) { return html.find(s) != std::string::npos; };
        check(has("--tg-theme-bg-color:#12ab34 !important") && has("--tg-theme-text-color:#000000 !important") && has("--tg-color-scheme:light !important"),
              "the palette goes on the root, lowercased, a bad value replaced by the scheme's default");
        check(has("--tg-button-bg:#ff0000") && has("--tg-button-radius:12px") && has("--tg-button-pressed-opacity:0.6") && !has("--tg-button-font") &&
                  !has("--tg-button-padding") && !has("--tg-button-disabled-opacity"),
              "button tokens pass only with their grammar");
        t.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":2,"name":"--tg-theme-bg-color","value":"#000000"},{"op":"style","id":10,"name":"--tg-theme-bg-color","value":"#000001"}]})"));
        html = t.last_html();
        check(!has("--tg-theme-bg-color:#000000") && has("--tg-theme-bg-color:#000001"), "the widget cannot set a theme variable on the root, only shadow it below");
        DisplayList list = t.paint();
        bool button = std::any_of(list.begin(), list.end(), [](const Command& c) {
            return c.kind == Cmd::FillRect && c.color.r == 255 && c.color.g == 0 && c.color.b == 0 && c.radii.tlx == 12;
        });
        check(button, "an unstyled <button> draws in the theme's button colour and radius");

        auto vars = css::theme_variables(parse(R"({"colorScheme":"dark","button":{"font":"italic 600 15px/1.25 system-ui, sans-serif","padding":"6px 1.5em 0 2rem",
            "pressedOpacity":".5","disabledOpacity":"1.0","height":"0","text":"#ABCDEF"}})"));
        auto var = [&](const char* name) {
            for (const auto& [n, v] : vars) if (n == name) return v;
            return std::string("-");
        };
        check(var("--tg-button-font") == "italic 600 15px/1.25 system-ui, sans-serif" && var("--tg-button-padding") == "6px 1.5em 0 2rem" &&
                  var("--tg-button-pressed-opacity") == ".5" && var("--tg-button-disabled-opacity") == "1.0" && var("--tg-button-height") == "0" &&
                  var("--tg-button-text") == "#abcdef",
              "button tokens within their grammar pass as given, colours lowercased");
        check(var("--tg-color-scheme") == "dark" && var("--tg-theme-bg-color") == "#212121" && var("--tg-button-bg") == "-",
              "a dark theme without a palette takes the dark defaults; a missing token sets no variable");
        check(var("--tg-text-font-size") == "15px" && var("--tg-text-line-height") == "1.35" && var("--tg-text-padding") == "8px 12px",
              "without text tokens the reference set stands in");

        vars = css::theme_variables(parse(R"({"text":{"fontSize":17.256,"lineHeight":"1.5","padding":"4px 6px"}})"));
        check(var("--tg-text-font-size") == "17.26px" && var("--tg-text-line-height") == "1.5" && var("--tg-text-padding") == "4px 6px",
              "text tokens within their grammar pass, a font size rounded to two decimals (got " + var("--tg-text-font-size") + ")");
        vars = css::theme_variables(parse(R"({"text":{"fontSize":"100px","lineHeight":0.5,"padding":"4px auto"}})"));
        check(var("--tg-text-font-size") == "15px" && var("--tg-text-line-height") == "1.35" && var("--tg-text-padding") == "8px 12px",
              "text tokens outside their grammar fall back to the reference set");
        check(css::is_theme_variable("--tg-text-padding"), "the widget cannot set a text token on the root");

        check(css::substitute_variables("a{margin:var(--x, 1px) var(--y,rgba(0,0,0,.5)) var(--z)}", {{"--x", "2px"}}) ==
                  "a{margin:2px rgba(0,0,0,.5) var(--z)}",
              "var() takes the theme's value, else its fallback, else stays");
        check(css::unwrap_where(":where(h1,h2){a:1}:where(a[href]){b:2}:where(li ul,li ol) b{c:3}:where(button:not([disabled])):active{d:4}") ==
                  "h1,h2{a:1}a[href]{b:2}:where(li ul,li ol) b{c:3}button:not([disabled]):active{d:4}",
              ":where() unwraps, except a list that more of its selector follows");
        check(css::resolve_light_dark(R"c(a{content:"light-dark(1,2)";color:LIGHT-DARK(red, blue);background:url(x/light-dark(1,2)) /* light-dark(3,4) */})c", true) ==
                  R"c(a{content:"light-dark(1,2)";color:blue;background:url(x/light-dark(1,2)) /* light-dark(3,4) */})c",
              "light-dark() resolves as a function only: strings, URLs and comments keep their text");
        check(rewrite_color_scheme(R"c(@media (prefers-color-scheme: dark){a{content:"(prefers-color-scheme: dark)";--prefers-color-scheme-x:1}})c", false) ==
                  R"c(@media (min-width:999999px){a{content:"(prefers-color-scheme: dark)";--prefers-color-scheme-x:1}})c",
              "prefers-color-scheme resolves as a media feature only, not in a string or a longer name");
    }

    // ── v0.1.1: the profile's UA layer on the text tokens, user-select, relative offsets inside a line ──
    {
        StubBackend ub;
        Renderer u(300, 1, false, ub);
        u.receive(parse(R"({"type":"theme","theme":{"colorScheme":"light","text":{"fontSize":20,"padding":"4px 6px"}}})"));
        u.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body"},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":"#sel{user-select:text} #flat{margin:0}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"h1"},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"Title"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"p","attrs":[["id","sel"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"b"},{"op":"ins","parent":12,"id":13,"before":null},{"op":"text","id":14,"data":"bold"},{"op":"ins","parent":13,"id":14,"before":null},
            {"op":"el","id":15,"tag":"span"},{"op":"ins","parent":12,"id":15,"before":null},{"op":"text","id":16,"data":"base"},{"op":"ins","parent":15,"id":16,"before":null},
            {"op":"el","id":17,"tag":"sup"},{"op":"ins","parent":12,"id":17,"before":null},{"op":"text","id":18,"data":"2"},{"op":"ins","parent":17,"id":18,"before":null},
            {"op":"el","id":19,"tag":"p","attrs":[["id","flat"]]},{"op":"ins","parent":4,"id":19,"before":null}]})"));
        auto ask = [&](Json query) {
            for (const Json& m : u.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        auto cs = [&](int id, const char* prop) { return ask(Json::object().set("q", "cs").set("id", id))[prop].asString(); };
        check(cs(2, "font-size") == "20px" && cs(10, "font-size") == "32px" && cs(10, "font-weight") == "600",
              "the root takes the text token's size and h1 the layer's 1.6em at 600 (got " + cs(2, "font-size") + ", " + cs(10, "font-size") + ", " + cs(10, "font-weight") + ")");
        check(cs(4, "margin-top") == "4px" && cs(4, "margin-left") == "6px" && cs(4, "padding-top") == "0px",
              "the body's margin is the padding token (got " + cs(4, "margin-top") + " " + cs(4, "margin-left") + ")");
        check(cs(19, "margin-top") == "0px", "the widget's own CSS beats the layer");
        bool token = false;
        for (const auto& [name, value] : u.theme_variables()) token = token || (name == "--tg-text-font-size" && value == "20px");
        const std::string& master = u.master_css();
        check(token && master.find("var(--tg-") == std::string::npos && master.find("20px") != std::string::npos,
              "theme_variables() and master_css() give the root's tokens and the UA layer with their values in place");
        std::string padding = ask(Json::object().set("q", "cs").set("id", 2).set("prop", "--tg-text-padding")).asString();
        check(padding == "4px 6px", "a custom property reads back with its spaces (got " + padding + ")");
        check(cs(2, "user-select") == "none" && cs(12, "user-select") == "text" && cs(13, "user-select") == "auto",
              "text is not selectable by default; user-select: text opts in and a child follows with auto");
        double base = ask(Json::object().set("q", "rect").set("id", 15))["y"].asNumber(), sup = ask(Json::object().set("q", "rect").set("id", 17))["y"].asNumber();
        check(sup < base, "a sup is raised by its relative offset, with its text (sup " + std::to_string(sup) + ", base " + std::to_string(base) + ")");
    }

    // ── v0.4.0: the client's time over the widget's corner (docs/sdk-v1.md §2.17) ──
    {
        StubBackend sb;
        Renderer s(300, 1, false, sb);
        s.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body"},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":"#own{padding-bottom:var(--tg-content-safe-area-inset-bottom,9px)}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["id","own"],["style","height:20px"]]},{"op":"ins","parent":4,"id":10,"before":null}]})"));
        auto ask = [&](Json query) {
            for (const Json& m : s.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        auto cs = [&](int id, const char* prop) {
            Json q = Json::object().set("q", "cs").set("id", id);
            return prop[0] == '-' ? ask(q.set("prop", prop)).asString() : ask(q)[prop].asString();
        };
        check(cs(2, "--tg-content-safe-area-inset-bottom") == "0px" && cs(4, "padding-bottom") == "0px" && cs(10, "padding-bottom") == "0px",
              "before the host says otherwise the safe area is 0px, set on the root (got " + cs(2, "--tg-content-safe-area-inset-bottom") + ")");
        std::vector<Json> out = s.receive(parse(R"({"type":"safeArea","inset":{"top":0,"right":0,"bottom":24.567,"left":0,"cornerLeft":0,"cornerRight":300}})"));
        bool sized = false;
        for (const Json& m : out) sized = sized || m["type"].asString() == "size";
        check(cs(2, "--tg-content-safe-area-inset-bottom") == "24.57px" && cs(2, "--tg-content-safe-area-corner-right") == "0px",
              "an inset is rounded to two decimals, a corner past 200 is 0 (got " + cs(2, "--tg-content-safe-area-inset-bottom") + ", " + cs(2, "--tg-content-safe-area-corner-right") + ")");
        check(cs(4, "padding-bottom") == "24.57px" && cs(10, "padding-bottom") == "24.57px" && sized,
              "the UA layer pads the body by the band, a widget's var() reads it, and the height is reported again (got " + cs(4, "padding-bottom") + ")");
        s.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":2,"name":"--tg-content-safe-area-inset-bottom","value":"1px"}]})"));
        check(cs(2, "--tg-content-safe-area-inset-bottom") == "24.57px", "the widget cannot set the safe area on the root");
    }

    // ── text selection: a mouse drag over user-select: text, its text, its highlight ──
    {
        StubBackend tb;
        Renderer t(300, 1, false, tb);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".s{user-select:text} p{margin:0;font:10px/20px sans-serif}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"Hello world"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":4,"id":12,"before":null},{"op":"text","id":13,"data":"Second"},{"op":"ins","parent":12,"id":13,"before":null},
            {"op":"el","id":14,"tag":"p"},{"op":"ins","parent":4,"id":14,"before":null},{"op":"text","id":15,"data":"Plain"},{"op":"ins","parent":14,"id":15,"before":null}]})"));
        auto wants = [](const std::vector<Json>& out) {
            for (const Json& m : out)
                if (m["type"].asString() == "gesture") return m["x"].asBool();
            return false;
        };
        bool claimed = wants(t.touch(0, 1, 10, 1, true, true));
        t.touch(1, 200, 10, 1, true, true);
        t.touch(2, 200, 10, 1, true, true);
        check(t.selected_text() == "Hello world" && claimed, "a mouse drag over selectable text selects it and claims the drag (got '" + t.selected_text() + "')");

        // The same text for a host's own (touch) selection: the selectable runs only, with their fonts and baselines.
        std::string runs = t.selectable_text_json();
        auto parsed = Json::parse(runs);
        bool shaped = parsed && !parsed->arr().empty();
        std::string joined;
        if (shaped) {
            for (const Json& r : parsed->arr()) {
                joined += r[size_t(9)].asString();
                shaped = shaped && r.arr().size() == 11 && r[size_t(6)].asInt() > 0 && r[size_t(5)].asNumber() > r[size_t(2)].asNumber();
            }
        }
        check(shaped && joined.find("Hello") != std::string::npos && joined.find("Second") != std::string::npos && joined.find("Plain") == std::string::npos,
              "selectable_text_json lists the selectable runs with font and baseline, not the rest (got " + runs + ")");

        t.touch(0, 17, 10, 1, true, true);
        t.touch(1, 33, 10, 1, true, true);
        t.touch(2, 33, 10, 1, true, true);
        check(t.selected_text() == "ll", "the ends snap to the nearest character boundaries (got '" + t.selected_text() + "')");
        DisplayList list = t.paint();
        bool tint = std::any_of(list.begin(), list.end(), [](const Command& c) {
            return c.kind == Cmd::FillRect && c.color.r == 51 && c.color.g == 144 && c.color.b == 236 && c.color.a == 77 && c.rect.x == 16 && c.rect.w == 16;
        });
        check(tint, "the selection paints in the link colour over its characters");

        t.touch(0, 1, 10, 1, true, true);
        t.touch(1, 200, 30, 1, true, true);
        t.touch(2, 200, 30, 1, true, true);
        check(t.selected_text() == "Hello world\nSecond", "a selection across blocks keeps a line break between them (got '" + t.selected_text() + "')");
        t.touch(0, 2, 50, 1, true, true);
        t.touch(2, 2, 50, 1, true, true);
        check(!t.has_selection(), "a press elsewhere drops it");

        t.touch(0, 1, 50, 1, true, true);
        t.touch(1, 200, 50, 1, true, true);
        t.touch(2, 200, 50, 1, true, true);
        check(!t.has_selection(), "text without user-select: text does not select");
        t.touch(0, 1, 10, 0, true, false);
        t.touch(1, 200, 10, 0, true, false);
        t.touch(2, 200, 10, 0, true, false);
        check(!t.has_selection(), "a finger's drag does not select: touch selection starts from the client's menu");

        t.receive(parse(R"({"type":"preventDefault","keys":["10:pointerdown"]})"));
        t.touch(0, 1, 10, 1, true, true);
        t.touch(1, 200, 10, 1, true, true);
        t.touch(2, 200, 10, 1, true, true);
        check(!t.has_selection(), "a widget that prevents the press keeps the drag");
        t.receive(parse(R"({"type":"preventDefault","keys":[]})"));

        t.touch(0, 1, 30, 1, true, true);
        t.touch(1, 200, 30, 1, true, true);
        t.touch(2, 200, 30, 1, true, true);
        check(t.clear_selection() && !t.has_selection() && !t.clear_selection(), "clear_selection drops it once");
    }

    // ── text selection: where the text shows, inside its transforms and clips ──
    {
        StubBackend tb;
        Renderer t(300, 1, false, tb);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".s{user-select:text} p{margin:0;font:10px/20px sans-serif;white-space:nowrap} .m{transform:translateX(50px)} .c{overflow:hidden;width:20px}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"p","attrs":[["class","s m"]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"Hello"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["class","c"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"p","attrs":[["class","s"]]},{"op":"ins","parent":12,"id":13,"before":null},{"op":"text","id":14,"data":"Clipped text"},{"op":"ins","parent":13,"id":14,"before":null}]})"));
        t.paint();
        t.touch(0, 51, 10, 1, true, true);
        t.touch(1, 67, 10, 1, true, true);
        t.touch(2, 67, 10, 1, true, true);
        check(t.selected_text() == "He", "translated text selects where it shows (got '" + t.selected_text() + "')");
        std::string runs = t.selectable_text_json();
        auto listed = Json::parse(runs);
        bool moved = listed && !listed->arr().empty();
        for (size_t i = 0; moved && i < listed->arr().size(); i++) {
            const Json& run = (*listed)[i];
            double e = run[size_t(10)][size_t(4)].asNumber();
            moved = run[size_t(9)].asString() == "Hello" ? e == 50 : e == 0;
        }
        check(moved, "selectable_text_json gives each run the transform it shows under (got " + runs + ")");
        DisplayList list = t.paint();
        int depth = 0;
        bool grouped = false;
        for (const Command& c : list) {
            if (c.kind == Cmd::GroupPush) depth++;
            else if (c.kind == Cmd::GroupPop) depth--;
            else if (c.kind == Cmd::FillRect && c.color.a == 77) grouped = depth > 0 && c.rect.x == 0 && c.rect.w == 16;
        }
        check(grouped, "the tint is painted inside the text's transform, in its coordinates");

        t.touch(0, 1, 30, 1, true, true);
        t.touch(1, 200, 30, 1, true, true);
        t.touch(2, 200, 30, 1, true, true);
        check(t.selected_text() == "Clipped text", "a drag past the clip selects the text under it (got '" + t.selected_text() + "')");
        list = t.paint();
        depth = 0;
        bool clipped = false;
        for (const Command& c : list) {
            if (c.kind == Cmd::ClipPush) depth++;
            else if (c.kind == Cmd::ClipPop) depth--;
            else if (c.kind == Cmd::FillRect && c.color.a == 77) clipped = depth > 0;
        }
        check(clipped, "the tint stays inside the clip of its text");
    }

    // ── text selection: a long unbroken run costs a bounded number of measurements ──
    {
        struct CountingBackend : StubBackend {
            size_t measured = 0;
            float text_width(int f, const std::string& text) override {
                measured++;
                return StubBackend::text_width(f, text);
            }
        } tb;
        auto drag = [&](size_t length) {
            Renderer t(300, 1, false, tb);
            t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":"p{user-select:text;margin:0;font:10px/20px sans-serif;white-space:nowrap}"},{"op":"ins","parent":3,"id":5,"before":null},
                {"op":"el","id":10,"tag":"p"},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":")" +
                            std::string(length, 'a') + R"("},{"op":"ins","parent":10,"id":11,"before":null}]})"));
            t.paint();
            tb.measured = 0;
            t.touch(0, 1, 10, 1, true, true);
            for (int x = 100; x <= 800; x += 100) t.touch(1, x, 10, 1, true, true);
            t.touch(2, 800, 10, 1, true, true);
            return t.selected_text();
        };
        std::string got = drag(3000);
        check(got == std::string(100, 'a') && tb.measured <= 64, "a press and eight moves over a 3,000-character run search its carets, reusing them (" + std::to_string(tb.measured) + " measured)");
        got = drag(65536);
        check(got == std::string(100, 'a') && tb.measured == 0, "past the bound a run's carets are spread over its width, unmeasured (" + std::to_string(tb.measured) + " measured)");
    }

    // ── inert clips: many siblings go in one pass, the rest in order ──
    {
        DisplayList list;
        for (int i = 0; i < 1000; i++) {
            Command push, fill, pop;
            push.kind = Cmd::ClipPush;
            push.rect = Rect{float(i), 0, 10, 10};
            fill.rect = Rect{float(i) + 2, 2, 2, 2};
            fill.width = float(i);
            pop.kind = Cmd::ClipPop;
            list.push_back(push);
            list.push_back(fill);
            list.push_back(pop);
        }
        size_t gone = drop_inert_clips(list);
        bool ordered = list.size() == 1000;
        for (size_t i = 0; ordered && i < list.size(); i++) ordered = list[i].kind == Cmd::FillRect && list[i].width == float(i);
        check(gone == 1000 && ordered, "1,000 sibling clips around their content all go, the content kept in order");
    }

    // ── measured widths: long text is not kept ──
    {
        struct CountingBackend : StubBackend {
            size_t shorter = 0, longer = 0;
            float text_width(int f, const std::string& text) override {
                (text.size() > 1000 ? longer : shorter)++;
                return StubBackend::text_width(f, text);
            }
        } mb;
        Renderer m(300, 1, false, mb);
        m.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","white-space:nowrap"]]},
            {"op":"el","id":10,"tag":"p"},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"abc )" + std::string(100000, 'x') + R"("},{"op":"ins","parent":10,"id":11,"before":null}]})"));
        m.paint();
        mb.shorter = mb.longer = 0;
        // The same words again, in a second paragraph.
        m.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":12,"tag":"p"},{"op":"ins","parent":4,"id":12,"before":null},{"op":"text","id":13,"data":"abc )" +
                        std::string(100000, 'x') + R"("},{"op":"ins","parent":12,"id":13,"before":null}]})"));
        m.paint();
        check(mb.shorter == 0 && mb.longer > 0, "a laid-out word's width is kept, a 100,000-character one's measured again (" + std::to_string(mb.shorter) + " short, " +
                                                    std::to_string(mb.longer) + " long measured)");
    }

    // ── measureText measures in the canvas's font, the one fillText draws with ──
    {
        struct FontBackend : StubBackend {
            std::vector<int> measured_in;
            float text_width(int f, const std::string& text) override {
                measured_in.push_back(f);
                return StubBackend::text_width(f, text);
            }
        } fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body"},{"op":"el","id":30,"tag":"canvas","attrs":[["width","50"],["height","20"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"canvas","id":30,"cmds":[["s","font","bold 12px serif"],["c","fillText","hi",0,10]]}]})"));
        int drawn = 0;
        for (const auto& run : fb.runs)
            for (const CanvasOp& op : run)
                if (op.kind == CanvasOp::Text) drawn = op.font;
        fb.measured_in.clear();
        f.receive(parse(R"({"type":"query","qid":1,"query":{"q":"measureText","font":"bold 12px serif","text":"hi"}})"));
        check(drawn != 0 && fb.measured_in.size() == 1 && fb.measured_in[0] == drawn, "measureText measures in the font fillText drew with");
    }

    // ── what a build in paint() says waits for the next tick ──
    {
        StubBackend pb;
        Renderer p(300, 1, false, pb);
        p.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":"@keyframes f{from{opacity:0}to{opacity:1}} .a{animation:f 0.05s}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["class","a"],["style","height:20px"]]},{"op":"ins","parent":4,"id":10,"before":null}]})"));
        p.tick();
        // While the animation runs, a batch waits for the frame: its build is the next paint's.
        auto batch = p.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":11,"tag":"div","attrs":[["style","height:100px"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
        bool sized_at_once = false;
        for (const Json& m : batch) sized_at_once = sized_at_once || m["type"].asString() == "size";
        std::this_thread::sleep_for(std::chrono::milliseconds(120));
        p.paint();
        bool waits = p.wants_tick() && p.next_tick_in() == 0;
        bool sized = false;
        for (const Json& m : p.tick()) sized = sized || m["type"].asString() == "size";
        check(!sized_at_once && waits && sized, "a size from a build in paint() keeps wants_tick() until the next tick hands it out");
    }

    // ── :hover and :active start their transitions at once ──
    {
        StubBackend hb;
        Renderer h(300, 1, false, hb);
        h.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".b{height:20px;opacity:1;transition:opacity 1s} .b:hover{opacity:0}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["class","b"]]},{"op":"ins","parent":4,"id":10,"before":null}]})"));
        h.paint();
        bool idle = !h.wants_tick();
        h.hover(0, 5, 10);
        bool hovered = h.wants_tick();
        StubBackend pb;
        Renderer p(300, 1, false, pb);
        p.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":".c{height:20px;opacity:1;transition:opacity 1s} .c:active{opacity:0}"},{"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["class","c"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
        p.paint();
        p.touch(0, 5, 10, 0, true, false);
        bool pressed = p.wants_tick();
        check(idle && hovered && pressed, "a :hover or :active restyle starts its transition without waiting for a message");
    }

    // ── the cursor under the mouse ──
    {
        StubBackend cb;
        Renderer c(300, 1, false, cb);
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","height:20px;cursor:pointer"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"span","attrs":[["style","display:block;height:20px"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["style","height:20px"]]},{"op":"ins","parent":4,"id":12,"before":null}]})"));
        c.hover(0, 5, 10);
        std::string over = c.cursor();
        c.hover(0, 5, 30);
        std::string plain = c.cursor();
        c.hover(0, 5, 10);
        c.hover(1, 0, 0);
        check(over == "pointer" && plain == "auto" && c.cursor() == "auto",
              "cursor() is the document's cursor under the mouse, inherited, and auto elsewhere or once it left (got " + over + ", " + plain + ", " + c.cursor() + ")");
    }

    // ── image sources: what reaches the backend ──
    {
        struct LoadingBackend : StubBackend {
            std::vector<std::string> loaded;
            bool load_image(int, const std::string& url, float& w, float& h) override {
                loaded.push_back(url);
                w = h = 1;
                return true;
            }
        } lb;
        auto loads = [&](const std::string& ops) {
            lb.loaded.clear();
            Renderer l(300, 1, false, lb);
            l.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body"},)" + ops + "]}"));
            l.paint();
            return lb.loaded.size();
        };
        auto generated = [&](const std::string& content) {
            Json css = Json::object().set("op", "css").set("id", 5).set("text", "p::before{content:" + content + "}");
            return loads(R"({"op":"el","id":5,"tag":"style"},)" + css.dump() + R"(,{"op":"ins","parent":3,"id":5,"before":null},
                {"op":"el","id":10,"tag":"p"},{"op":"ins","parent":4,"id":10,"before":null},{"op":"text","id":11,"data":"x"},{"op":"ins","parent":10,"id":11,"before":null})");
        };
        check(generated(R"c("a\"url(data:image/png;base64,AA==)")c") == 0, "an escaped quote keeps a url( inside its string: generated content loads no image");
        check(generated(R"c("a" url(data:image/png;base64,AA==))c") == 1, "generated content's own url() loads its image");
        check(generated(R"c(url("data:image/x-icon;base64,AA=="))c") == 0, "generated content's image of a type outside the profile is refused");
        auto img = [&](const std::string& src) {
            Json attrs = Json::array();
            attrs.push(Json::array().push("src").push(src));
            return loads(Json::object().set("op", "el").set("id", 10).set("tag", "img").set("attrs", attrs).dump() + R"(,{"op":"ins","parent":4,"id":10,"before":null})");
        };
        check(img("DATA:IMAGE/SVG+XML,%3Csvg%3E%3C/svg%3E") == 0 && img(" data:image/Svg+Xml ;utf8,<svg/>") == 0, "an SVG in any spelling is the placeholder, never the backend's");
        check(img("data:image/PNG;base64,AA==") == 1, "a profile type in capitals still loads");
        check(img("data:image/bmp;base64,AA==") == 0 && img("data:text/html,<b>") == 0, "a type outside the profile is refused");
    }

    // ── getComputedStyle ──
    {
        StubBackend sb;
        Renderer s(300, 1, false, sb);
        s.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":"#a{width:100px;padding:8px;margin:4px 0;--accent:rgb(0,120,0);overflow:hidden;touch-action:pan-y;font-style:italic} #a::before{content:'x'} b{font-weight:bold}"},
            {"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"b"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"text","id":12,"data":"bold"},{"op":"ins","parent":11,"id":12,"before":null}]})"));
        auto cs = [&](int id, const char* pseudo = nullptr) {
            Json q = Json::object().set("type", "query").set("qid", 1).set("query", Json::object().set("q", "cs").set("id", id));
            if (pseudo) q.set("query", Json::object().set("q", "cs").set("id", id).set("pseudo", pseudo));
            for (const Json& m : s.receive(q))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        Json a = cs(10);
        check(a["width"].asString() == "100px" && a["padding-left"].asString() == "8px" && a["margin"].asString() == "4px 0px",
              "width is the content box, padding and margin as used, margin shortened (got " + a["width"].asString() + ", " + a["margin"].asString() + ")");
        check(a["overflow"].asString() == "hidden" && a["font-style"].asString() == "italic" && a["touch-action"].asString() == "pan-y",
              "overflow, font-style and a stylesheet's touch-action");
        check(cs(11)["font-weight"].asString() == "700", "b is bold: font-weight 700 (got " + cs(11)["font-weight"].asString() + ")");
        Json custom = Json::object().set("type", "query").set("qid", 2).set("query", Json::object().set("q", "cs").set("id", 11).set("prop", "--accent"));
        std::string accent;
        for (const Json& m : s.receive(custom))
            if (m["type"].asString() == "result") accent = m["result"].asString();
        check(accent == "rgb(0,120,0)", "a custom property inherits and reads as written (got " + accent + ")");
        check(cs(10, "::before")["content"].asString() == "\"x\"", "::before has its content, quoted (got " + cs(10, "::before")["content"].asString() + ")");
        bool claim_x = false, claim_y = true;
        for (const Json& m : s.touch(0, 20, 20, 0, true, false, 0)) {
            if (m["type"].asString() != "gesture") continue;
            claim_x = m["x"].asBool();
            claim_y = m["y"].asBool();
        }
        s.touch(3, 20, 20, 0, true, false, 0);
        check(claim_x && !claim_y, "a stylesheet's touch-action: pan-y claims horizontal drags");

        const auto checkAlignment = [&](const std::string& property, const std::string& value, const std::string& expected) {
            const auto display = property == "align-items" ? "display:flex;" : "";
            auto ops = Json::array().push(Json::object()
                .set("op", "attr").set("id", 10).set("name", "style")
                .set("value", display + property + ":" + value + ";color:rgb(1,2,3)"));
            s.receive(Json::object().set("type", "ops").set("ops", ops));
            const auto computed = cs(10);
            check(computed["color"].asString() == "rgb(1, 2, 3)"
                && computed[property].asString() == expected,
                property + ": " + value + " survives a full computed-style query (got "
                    + computed[property].asString() + ")");
        };
        for (const auto property : {"align-self", "align-items"}) {
            for (const auto position : {"center", "start", "end", "self-start", "self-end", "flex-start", "flex-end"}) {
                for (const auto prefix : {"", "safe ", "unsafe "}) {
                    const auto value = std::string(prefix) + position;
                    checkAlignment(property, value, value);
                }
            }
            for (const auto value : {"normal", "stretch", "baseline"}) {
                checkAlignment(property, value, value);
            }
            checkAlignment(property, "first baseline", "baseline");
            checkAlignment(property, "baseline first", "baseline");
            checkAlignment(property, "last baseline", "last baseline");
            checkAlignment(property, "baseline last", "last baseline");
        }
        checkAlignment("align-self", "auto", "auto");
        checkAlignment("align-self", "SaFe /**/ CeNtEr", "safe center");
        checkAlignment("align-items", "LAST\nBASELINE", "last baseline");

        s.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":10,"name":"style","value":""}]})"));
        for (const auto property : {"align-self", "align-items"}) {
            for (const auto display : {"flex", "grid"}) {
                for (const auto value : {"safe center", "unsafe flex-end", "first baseline", "last baseline"}) {
                    const auto expected = std::string(value) == "first baseline" ? "baseline" : value;
                    const auto rule = std::string("#a{display:") + display + ";--alignment:" + value + ";"
                        + property + ":var(--alignment)}#a b,#a::before{display:" + display + ";"
                        + property + ":inherit}#a::before{content:'x'}";
                    auto ops = Json::array().push(Json::object().set("op", "css").set("id", 20).set("text", rule));
                    s.receive(Json::object().set("type", "ops").set("ops", ops));
                    const auto label = std::string(display) + " " + property + ": " + value;
                    check(cs(10)[property].asString() == expected, label + " from a stylesheet variable");
                    check(cs(11)[property].asString() == expected, label + " inherited by a child");
                    check(cs(10, "::before")[property].asString() == expected, label + " inherited by a pseudo-element");
                }
            }
        }
    }

    // ── safe and unsafe alignment: the position decides the place ──
    {
        StubBackend fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div"},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div"},{"op":"ins","parent":10,"id":11,"before":null}]})"));
        auto placed = [&](const std::string& container, const std::string& item, const char* axis) {
            auto ops = Json::array()
                .push(Json::object().set("op", "attr").set("id", 10).set("name", "style").set("value", container))
                .push(Json::object().set("op", "attr").set("id", 11).set("name", "style").set("value", "width:20px;height:20px;" + item));
            f.receive(Json::object().set("type", "ops").set("ops", ops));
            for (const Json& m : f.receive(Json::object().set("type", "query").set("qid", 1).set("query", Json::object().set("q", "rect").set("id", 11))))
                if (m["type"].asString() == "result") return m["result"][axis].asNumber();
            return -1.0;
        };
        auto positions = [&](const char* display, const std::string& box, const char* property, bool on_item, const char* axis, double center, double end) {
            auto at = [&](const std::string& value) {
                std::string declaration = std::string(property) + ":" + value;
                return on_item ? placed(box, declaration, axis) : placed(box + declaration, "", axis);
            };
            for (const char* position : {"center", "start", "end", "self-start", "self-end", "flex-start", "flex-end"}) {
                double alone = at(position), safe = at(std::string("safe ") + position), unsafe = at(std::string("unsafe ") + position);
                double want = !std::strcmp(position, "center") ? center : !std::strcmp(position, "end") ? end : alone;
                check(std::fabs(alone - want) < 0.01 && std::fabs(safe - alone) < 0.01 && std::fabs(unsafe - alone) < 0.01,
                      std::string(display) + " " + property + ": safe and unsafe " + position + " are placed as " + position + " (got " +
                          std::to_string(safe) + " and " + std::to_string(unsafe) + ", " + position + " alone " + std::to_string(alone) + ")");
            }
        };
        const std::string flex = "display:flex;width:200px;height:100px;";
        const std::string grid = "display:grid;grid-template-columns:200px;grid-template-rows:100px;width:200px;height:100px;";
        positions("flex", flex, "align-self", true, "y", 40, 80);
        positions("flex", flex, "align-items", false, "y", 40, 80);
        positions("grid", grid, "align-self", true, "y", 40, 80);
        positions("grid", grid, "align-items", false, "y", 40, 80);
        positions("grid", grid, "justify-self", true, "x", 90, 180);
        positions("grid", grid, "justify-items", false, "x", 90, 180);
    }

    // ── the boxes of inline elements: their line boxes ──
    {
        StubBackend ib;
        Renderer in(300, 1, false, ib);
        in.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:100px;text-align:center"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"span"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"text","id":12,"data":"ab"},{"op":"ins","parent":11,"id":12,"before":null},
            {"op":"el","id":20,"tag":"div","attrs":[["style","width:50px"]]},{"op":"ins","parent":4,"id":20,"before":null},
            {"op":"el","id":21,"tag":"span"},{"op":"ins","parent":20,"id":21,"before":null},
            {"op":"text","id":22,"data":"aaaa bbbb cccc"},{"op":"ins","parent":21,"id":22,"before":null}]})"));
        auto ask = [&](const char* kind, int id) {
            for (const Json& m : in.receive(Json::object().set("type", "query").set("qid", 1).set("query", Json::object().set("q", kind).set("id", id))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        Json centred = ask("rect", 11);
        check(std::fabs(centred["x"].asNumber() - 42) < 0.5 && std::fabs(centred["width"].asNumber() - 16) < 0.5,
              "a centred span's rect is where its text is (x " + std::to_string(centred["x"].asNumber()) + ")");
        Json lines = ask("rects", 21);
        check(lines.arr().size() == 3, "a wrapped span has a rect per line (" + std::to_string(lines.arr().size()) + ")");
    }

    // ── <img> load and error ──
    {
        struct Decoding : StubBackend {
            bool load_image(int, const std::string&, float& w, float& h) override {
                w = 2;
                h = 3;
                return true;
            }
        };
        const char* page = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body"},
            {"op":"el","id":10,"tag":"img","attrs":[["src","data:image/png;base64,iVBORw0KGgo="]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"img","attrs":[["src","data:image/png;base64,AAAA"]]}]})";
        Decoding db;
        Renderer d(300, 1, false, db);
        std::string loads;
        for (const Json& ev : events(d.receive(parse(page))))
            if (ev["type"].asString() == "load") loads += std::to_string(int(ev["targetId"].asNumber())) + ":" + std::to_string(int(ev["naturalWidth"].asNumber())) + "x" +
                                                          std::to_string(int(ev["naturalHeight"].asNumber())) + " ";
        check(loads == "10:2x3 11:2x3 ", "an <img>, attached or not, loads with its natural size (got " + loads + ")");
        auto again = events(d.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":10,"name":"src","value":"data:image/png;base64,BBBB"}]})")));
        check(again.size() == 1 && again[0]["type"].asString() == "load", "a new src loads again");
        StubBackend fb2;
        Renderer f2(300, 1, false, fb2);
        check(types(f2.receive(parse(page))) == "error:10 error:11 ", "an image the backend cannot decode is an error");
    }

    // ── scroll sizes, pointer-events, CSS-wide keywords, the placeholder image, movementX, empty gradients ──
    {
        StubBackend pb;
        Renderer p(300, 1, false, pb);
        auto loaded = p.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style"},
            {"op":"css","id":5,"text":"#kw1{display:initial} #kwp{color:rgb(0,0,140)} #kw2{color:rgb(200,0,0);color:unset} #kw3{display:inline;display:revert} p{margin-top:0} #kw4{margin:revert}"},
            {"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["style","overflow:hidden;height:30px"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","height:70px"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":21,"tag":"div","attrs":[["style","position:absolute;left:0;top:100px;width:50px;height:50px"]]},{"op":"ins","parent":4,"id":21,"before":null},
            {"op":"el","id":20,"tag":"div","attrs":[["style","position:absolute;left:0;top:100px;width:50px;height:50px;pointer-events:none"]]},{"op":"ins","parent":4,"id":20,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["id","kw1"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"div","attrs":[["id","kwp"]]},{"op":"ins","parent":4,"id":31,"before":null},
            {"op":"el","id":32,"tag":"span","attrs":[["id","kw2"]]},{"op":"ins","parent":31,"id":32,"before":null},
            {"op":"el","id":33,"tag":"div","attrs":[["id","kw3"]]},{"op":"ins","parent":4,"id":33,"before":null},
            {"op":"el","id":34,"tag":"p","attrs":[["id","kw4"]]},{"op":"ins","parent":4,"id":34,"before":null},
            {"op":"el","id":35,"tag":"div","attrs":[["style","color:rebeccapurple"]]},{"op":"ins","parent":4,"id":35,"before":null},
            {"op":"el","id":40,"tag":"img","attrs":[["src","data:image/svg+xml,%3Csvg%3E%3C/svg%3E"]]},{"op":"ins","parent":4,"id":40,"before":null}]})"));
        auto ask = [&](Json query) {
            for (const Json& m : p.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        auto cs = [&](int id, const char* prop) { return ask(Json::object().set("q", "cs").set("id", id))[prop].asString(); };
        Json box = ask(Json::object().set("q", "box").set("id", 10));
        check(box["scrollHeight"].asNumber() == 70 && box["clientHeight"].asNumber() == 30,
              "scrollHeight is the overflowing content's (got " + std::to_string(box["scrollHeight"].asNumber()) + ")");
        double hit = ask(Json::object().set("q", "elementFromPoint").set("x", 25).set("y", 125)).asNumber();
        check(hit == 21, "pointer-events: none passes the hit to what is below (got " + std::to_string(hit) + ")");
        check(cs(30, "display") == "inline", "display: initial is inline (got " + cs(30, "display") + ")");
        check(cs(32, "color") == "rgb(0, 0, 140)", "color: unset inherits (got " + cs(32, "color") + ")");
        check(cs(33, "display") == "block", "display: revert is the UA's block (got " + cs(33, "display") + ")");
        check(cs(34, "margin-top") == "15px", "margin: revert brings back litehtml's 1em, past the profile's layer (got " + cs(34, "margin-top") + ")");
        check(cs(35, "color") == "rgb(102, 51, 153)", "rebeccapurple is a colour (got " + cs(35, "color") + ")");
        double natural = 0;
        for (const Json& ev : events(loaded))
            if (ev["type"].asString() == "load" && ev["targetId"].asNumber() == 40) natural = ev["naturalWidth"].asNumber();
        check(natural == 48, "an SVG image is the profile's 48 px placeholder (got " + std::to_string(natural) + ")");

        p.receive(parse(R"({"type":"listen","types":["pointermove"]})"));
        p.touch(0, 100, 10, 0, true, true, 0);
        p.touch(1, 120, 10, 0, true, true, 0);
        p.touch(1, 150, 14, 0, true, true, 0);
        auto moves = events(p.tick());
        check(moves.size() == 1 && moves[0]["movementX"].asNumber() == 50 && moves[0]["movementY"].asNumber() == 4,
              "coalesced moves add their movementX/Y up");
        p.touch(1, 160, 14, 0, true, true, 0);
        moves = events(p.tick());
        check(moves.size() == 1 && moves[0]["movementX"].asNumber() == 10, "movementX counts from the last move the widget saw");
        p.touch(2, 160, 14, 0, true, true, 0);

        p.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":50,"tag":"canvas","attrs":[["width","10"],["height","10"]]},{"op":"ins","parent":4,"id":50,"before":null},
            {"op":"canvas","id":50,"cmds":[["g",1,"linear",5,5,5,5],["gs",1,0,"red"],["s","fillStyle",{"g":1}],["c","fillRect",0,0,10,10]]}]})"));
        const auto& last = pb.runs.back();
        check(!last.empty() && last.back().paint.kind == CanvasPaint::Solid && last.back().paint.color.a == 0, "a gradient with no length paints nothing");
    }

    // ── animated computed style, ResizeObserver, <details>, animationstart before any listen ──
    {
        StubBackend db;
        Renderer d(300, 1, false, db);
        auto loaded = d.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style"},
            {"op":"css","id":5,"text":"@keyframes slide{from{transform:translateX(0)}to{transform:translateX(40px)}} #pk{width:20px;height:20px;animation:slide 10s linear -2.5s paused}"},
            {"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["id","pk"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","width:100px;height:20px"]]},{"op":"ins","parent":4,"id":11,"before":null},
            {"op":"el","id":20,"tag":"details"},{"op":"ins","parent":4,"id":20,"before":null},
            {"op":"el","id":21,"tag":"summary"},{"op":"ins","parent":20,"id":21,"before":null},
            {"op":"text","id":22,"data":"s"},{"op":"ins","parent":21,"id":22,"before":null},
            {"op":"el","id":23,"tag":"div","attrs":[["style","height:20px"]]},{"op":"ins","parent":20,"id":23,"before":null}]})"));
        check(types(loaded).find("animationstart:10") != std::string::npos, "animationstart goes out before the widget says it listens (got " + types(loaded) + ")");
        auto ask = [&](Json query) {
            for (const Json& m : d.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        std::string transform = ask(Json::object().set("q", "cs").set("id", 10))["transform"].asString();
        check(transform == "matrix(1, 0, 0, 1, 10, 0)", "a paused animation's frame is the computed transform (got " + transform + ")");
        d.receive(parse(R"J({"type":"ops","ops":[{"op":"style","id":11,"name":"transform","value":"translateX(calc(50% - 1 * (100% + 10px) + 50px)) translateY(calc(100% + 1px))"}]})J"));
        transform = ask(Json::object().set("q", "cs").set("id", 11))["transform"].asString();
        check(transform == "matrix(1, 0, 0, 1, -10, 21)", "calc() of % and px inside translate functions resolves against the box (got " + transform + ")");
        double left = ask(Json::object().set("q", "rect").set("id", 11))["x"].asNumber();
        check(std::fabs(left + 10) < 0.01, "and the document moves the box by it (got " + std::to_string(left) + ")");
        // Math on one side only still interpolates (Chromium does), as a sum of the two translations by weight.
        auto tx = [](const std::string& value) {
            auto m = css::parse_transform(value, 100, 20);
            return m ? double(m->e) : -1e9;
        };
        std::string quarter = interpolate("translateX(0px)", "translateX(calc(100% + 10px))", 0.25);
        std::string from_none = interpolate("none", "translateX(calc(100% + 10px))", 0.25);
        std::string back = interpolate("translateX(calc(50% - 1 * (100% + 10px) + 50px))", "translateX(20px)", 0.5);
        check(std::fabs(tx(quarter) - 27.5) < 0.01 && std::fabs(tx(from_none) - 27.5) < 0.01 && std::fabs(tx(back) - 5) < 0.01,
              "a transition into or out of calc() moves, not flips (got " + quarter + " | " + from_none + " | " + back + ")");
        d.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":11,"name":"transform","value":")" + quarter + R"("}]})"));
        left = ask(Json::object().set("q", "rect").set("id", 11))["x"].asNumber();
        check(std::fabs(left - 27.5) < 0.01, "and the document draws the weighted sum (got " + std::to_string(left) + ")");

        auto resized = [](const std::vector<Json>& out) {
            for (const Json& m : out)
                if (m["type"].asString() == "resize" && !m["entries"].arr().empty()) return m["entries"][size_t(0)]["width"].asNumber();
            return -1.0;
        };
        double first = resized(d.receive(parse(R"({"type":"ops","ops":[{"op":"call","id":11,"method":"observeResize","args":[]}]})")));
        check(first == 100, "observeResize reports the content box at once (got " + std::to_string(first) + ")");
        double next = resized(d.receive(parse(R"({"type":"ops","ops":[{"op":"attr","id":11,"name":"style","value":"width:90px;height:20px"}]})")));
        check(next == 90, "and again when it changes (got " + std::to_string(next) + ")");

        Json details = ask(Json::object().set("q", "rect").set("id", 20)), summary = ask(Json::object().set("q", "rect").set("id", 21));
        check(details["height"].asNumber() > 0 && details["height"].asNumber() == summary["height"].asNumber(), "a closed <details> is as tall as its summary");
        check(ask(Json::object().set("q", "cs").set("id", 21))["display"].asString() == "list-item", "a summary is a list item");
        double x = summary["x"].asNumber() + 4, y = summary["y"].asNumber() + summary["height"].asNumber() / 2;
        d.touch(0, x, y, 0, true, false, 0);
        bool opened = false;
        for (const Json& ev : events(d.touch(2, x, y, 0, true, false, 0)))
            if (ev["type"].asString() == "toggle") opened = ev["targetId"].asNumber() == 20 && ev["open"].asBool();
        check(opened, "tapping the summary opens the <details> and sends toggle");
        check(ask(Json::object().set("q", "rect").set("id", 23))["height"].asNumber() == 20, "and its content shows");
    }

    // ── the fork: :lang, :empty, spacing, the translate property, word-break, fit-content ──
    {
        StubBackend fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style"},
            {"op":"css","id":5,"text":":lang(ru) .lg{color:rgb(0,100,0)} .m:empty{color:rgb(28,0,0)} .m:not(:empty){color:rgb(29,0,0)} span{white-space:nowrap}"},
            {"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["lang","ru-RU"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"span","attrs":[["class","lg"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"span","attrs":[["class","m"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"span","attrs":[["class","m"]]},{"op":"ins","parent":4,"id":13,"before":null},
            {"op":"text","id":14,"data":" "},{"op":"ins","parent":13,"id":14,"before":null},
            {"op":"el","id":20,"tag":"div"},{"op":"ins","parent":4,"id":20,"before":null},
            {"op":"el","id":21,"tag":"span","attrs":[["style","letter-spacing:5px"]]},{"op":"ins","parent":20,"id":21,"before":null},
            {"op":"text","id":22,"data":"abcd"},{"op":"ins","parent":21,"id":22,"before":null},
            {"op":"el","id":23,"tag":"div"},{"op":"ins","parent":4,"id":23,"before":null},
            {"op":"el","id":24,"tag":"span","attrs":[["style","word-spacing:10px"]]},{"op":"ins","parent":23,"id":24,"before":null},
            {"op":"text","id":25,"data":"a b c"},{"op":"ins","parent":24,"id":25,"before":null},
            {"op":"el","id":30,"tag":"div","attrs":[["style","width:20px;height:20px;translate:10px 5px"]]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"div","attrs":[["style","width:40px;word-break:break-all;line-height:16px"]]},{"op":"ins","parent":4,"id":31,"before":null},
            {"op":"text","id":32,"data":"aaaaaaaaaaaa"},{"op":"ins","parent":31,"id":32,"before":null},
            {"op":"el","id":33,"tag":"div","attrs":[["style","width:fit-content"]]},{"op":"ins","parent":4,"id":33,"before":null},
            {"op":"text","id":34,"data":"fit"},{"op":"ins","parent":33,"id":34,"before":null}]})"));
        auto ask = [&](const char* q, int id) {
            for (const Json& m : f.receive(Json::object().set("type", "query").set("qid", 1).set("query", Json::object().set("q", q).set("id", id))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        check(ask("cs", 11)["color"].asString() == "rgb(0, 100, 0)", ":lang(ru) matches under lang=ru-RU");
        check(ask("cs", 12)["color"].asString() == "rgb(28, 0, 0)" && ask("cs", 13)["color"].asString() == "rgb(29, 0, 0)",
              ":empty matches no children, and whitespace is content");
        check(ask("rect", 21)["width"].asNumber() == 4 * 8 + 4 * 5, "letter-spacing goes after each letter (got " + std::to_string(ask("rect", 21)["width"].asNumber()) + ")");
        double spaced = ask("rect", 24)["width"].asNumber();
        check(spaced == 5 * 8 + 2 * 10, "word-spacing widens each space (got " + std::to_string(spaced) + ")");
        Json moved = ask("rect", 30);
        // offsetTop is a whole number, as in browsers.
        check(moved["x"].asNumber() == 10 && std::fabs(moved["y"].asNumber() - ask("box", 30)["offsetTop"].asNumber() - 5) < 1, "the translate property moves the box");
        check(ask("rect", 31)["height"].asNumber() >= 32, "word-break: break-all breaks a long word");
        check(ask("rect", 33)["width"].asNumber() == 24, "width: fit-content shrinks to the text (got " + std::to_string(ask("rect", 33)["width"].asNumber()) + ")");
    }

    // ── the fork: math with percentages, empty table cells, quotes, right to left ──
    {
        StubBackend lb;
        Renderer l(300, 1, false, lb);
        l.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:200px"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","width:calc(50% - 10px);height:2px"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["style","width:min(50px, 30%);height:2px"]]},{"op":"ins","parent":10,"id":12,"before":null},
            {"op":"el","id":13,"tag":"div","attrs":[["style","width:clamp(20px, 50%, 60px);height:2px"]]},{"op":"ins","parent":10,"id":13,"before":null},
            {"op":"el","id":20,"tag":"div","attrs":[["style","display:table;width:200px"]]},{"op":"ins","parent":4,"id":20,"before":null},
            {"op":"el","id":21,"tag":"div","attrs":[["style","display:table-cell;width:50px;height:10px"]]},{"op":"ins","parent":20,"id":21,"before":null},
            {"op":"el","id":22,"tag":"div","attrs":[["style","display:table-cell;height:10px"]]},{"op":"ins","parent":20,"id":22,"before":null},
            {"op":"el","id":30,"tag":"div"},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"span"},{"op":"ins","parent":30,"id":31,"before":null},
            {"op":"text","id":32,"data":"qq"},{"op":"ins","parent":31,"id":32,"before":null},
            {"op":"el","id":33,"tag":"q"},{"op":"ins","parent":30,"id":33,"before":null},
            {"op":"text","id":34,"data":"qq"},{"op":"ins","parent":33,"id":34,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["dir","rtl"],["style","width:200px"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"span","attrs":[["style","display:inline-block;width:10px;height:8px"]]},{"op":"ins","parent":40,"id":41,"before":null},
            {"op":"el","id":42,"tag":"span","attrs":[["style","display:inline-block;width:10px;height:8px"]]},{"op":"ins","parent":40,"id":42,"before":null},
            {"op":"el","id":43,"tag":"div","attrs":[["dir","rtl"],["style","width:200px"]]},{"op":"ins","parent":4,"id":43,"before":null},
            {"op":"el","id":44,"tag":"div","attrs":[["style","width:20px;height:4px;margin-left:10px"]]},{"op":"ins","parent":43,"id":44,"before":null}]})"));
        auto rect = [&](int id) {
            for (const Json& m : l.receive(Json::object().set("type", "query").set("qid", 1).set("query", Json::object().set("q", "rect").set("id", id))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        check(rect(11)["width"].asNumber() == 90 && rect(12)["width"].asNumber() == 50 && rect(13)["width"].asNumber() == 60,
              "calc(), min() and clamp() resolve their percentages against the containing block");
        check(rect(21)["width"].asNumber() == 50 && rect(22)["width"].asNumber() == 150, "an empty auto cell takes the table's spare width");
        check(rect(33)["width"].asNumber() > rect(31)["width"].asNumber() + 2, "a <q> has quotation marks");
        check(rect(41)["x"].asNumber() > rect(42)["x"].asNumber() && rect(41)["x"].asNumber() + 10 == 200, "dir=rtl lays out inline boxes from the right");
        check(rect(44)["x"].asNumber() + 20 == 200, "right to left, a narrow block sits on the right");
    }

    // ── SVG: boxes through the view box, text length ──
    {
        StubBackend sb;
        Renderer s(300, 1, false, sb);
        s.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"svg","svg":true,"attrs":[["width","120"],["height","40"],["viewBox","0 0 60 20"],["style","display:block"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"rect","svg":true,"attrs":[["x","5"],["y","2"],["width","10"],["height","8"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"text","svg":true,"attrs":[["x","20"],["y","10"],["font-size","8"]]},{"op":"ins","parent":10,"id":12,"before":null},
            {"op":"text","id":13,"data":"Hello"},{"op":"ins","parent":12,"id":13,"before":null}]})"));
        auto ask = [&](Json query) {
            for (const Json& m : s.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        Json rect = ask(Json::object().set("q", "rect").set("id", 11));
        check(std::fabs(rect["x"].asNumber() - 10) < 0.5 && std::fabs(rect["width"].asNumber() - 20) < 0.5,
              "an SVG shape's rect goes through the view box (got x " + std::to_string(rect["x"].asNumber()) + " w " + std::to_string(rect["width"].asNumber()) + ")");
        double length = ask(Json::object().set("q", "svg").set("id", 12).set("method", "getComputedTextLength")).asNumber();
        check(length > 10, "getComputedTextLength measures an SVG text (got " + std::to_string(length) + ")");

        // Computed paint: inherited from a group, currentColor resolved; a link in the theme's link colour, from the UA layer.
        s.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":20,"tag":"g","svg":true,"attrs":[["fill","#0a0"],["stroke","currentColor"],["style","color:rgb(1, 2, 3)"]]},{"op":"ins","parent":10,"id":20,"before":null},
            {"op":"el","id":21,"tag":"circle","svg":true,"attrs":[["r","4"]]},{"op":"ins","parent":20,"id":21,"before":null},
            {"op":"el","id":22,"tag":"a","attrs":[["href","https://t.me"]]},{"op":"ins","parent":4,"id":22,"before":null},
            {"op":"text","id":23,"data":"link"},{"op":"ins","parent":22,"id":23,"before":null}]})J"));
        Json cs = ask(Json::object().set("q", "cs").set("id", 21));
        check(cs["fill"].asString() == "rgb(0, 170, 0)" && cs["stroke"].asString() == "rgb(1, 2, 3)",
              "fill and stroke inherit, currentColor resolved (got " + cs["fill"].asString() + ", " + cs["stroke"].asString() + ")");
        check(ask(Json::object().set("q", "cs").set("id", 11))["stroke"].asString() == "none", "no stroke is none");
        Json link = ask(Json::object().set("q", "cs").set("id", 22));
        check(link["color"].asString() == "rgb(51, 144, 236)" && link["text-decoration-line"].asString() == "none",
              "a link is --tg-theme-link-color, not underlined (got " + link["color"].asString() + ", " + link["text-decoration-line"].asString() + ")");
    }

    // ── SVG <use>: paint inherits through nested instances, and mutual references end ──
    {
        auto fills = [](const DisplayList& d) {
            std::vector<Color> out;
            for (const Command& c : d)
                if (c.kind == Cmd::Path && c.path && c.path->fill) out.push_back(c.path->fill_color);
            return out;
        };
        auto same = [](const std::vector<Color>& f, uint8_t r, uint8_t g, uint8_t b) {
            return std::all_of(f.begin(), f.end(), [&](const Color& c) { return c.r == r && c.g == g && c.b == b; });
        };
        StubBackend nb;
        Renderer nested(300, 1, false, nb);
        nested.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"svg","svg":true,"attrs":[["width","100"],["height","100"],["fill","#00a"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"defs","svg":true},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"g","svg":true,"attrs":[["id","a"]]},{"op":"ins","parent":11,"id":12,"before":null},
            {"op":"el","id":13,"tag":"use","svg":true,"attrs":[["href","#b"]]},{"op":"ins","parent":12,"id":13,"before":null},
            {"op":"el","id":14,"tag":"g","svg":true,"attrs":[["id","b"]]},{"op":"ins","parent":11,"id":14,"before":null},
            {"op":"el","id":15,"tag":"rect","svg":true,"attrs":[["width","10"],["height","10"]]},{"op":"ins","parent":14,"id":15,"before":null},
            {"op":"el","id":16,"tag":"use","svg":true,"attrs":[["href","#a"],["fill","#0a0"]]},{"op":"ins","parent":10,"id":16,"before":null}]})"));
        auto f = fills(nested.paint());
        check(f.size() == 1 && same(f, 0, 170, 0), "a rect used through two <use> levels takes the outer <use>'s fill (" + std::to_string(f.size()) + " paths)");

        StubBackend cb;
        Renderer cyclic(300, 1, false, cb);
        cyclic.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"svg","svg":true,"attrs":[["width","100"],["height","100"],["fill","#00a"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"g","svg":true,"attrs":[["id","a"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"rect","svg":true,"attrs":[["width","10"],["height","10"]]},{"op":"ins","parent":11,"id":12,"before":null},
            {"op":"el","id":13,"tag":"use","svg":true,"attrs":[["href","#b"]]},{"op":"ins","parent":11,"id":13,"before":null},
            {"op":"el","id":14,"tag":"g","svg":true,"attrs":[["id","b"]]},{"op":"ins","parent":10,"id":14,"before":null},
            {"op":"el","id":15,"tag":"use","svg":true,"attrs":[["href","#a"]]},{"op":"ins","parent":14,"id":15,"before":null}]})"));
        f = fills(cyclic.paint());
        check(f.size() > 1 && f.size() < 16 && same(f, 0, 0, 170),
              "two groups using each other paint a bounded number of instances, each with the root's fill (" + std::to_string(f.size()) + " paths)");
    }

    // ── resize diffs: which side of a widget changing height a host keeps still ──
    {
        auto run = [&](const char* grow, double press_y) {
            StubBackend rb;
            Renderer r(300, 1, false, rb);
            r.set_resize_tracking(true);
            r.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"div","attrs":[["style","height:50px"]]},{"op":"ins","parent":4,"id":10,"before":null},
                {"op":"el","id":11,"tag":"div","attrs":[["style","height:50px"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
            r.paint();
            if (press_y >= 0) {
                r.touch(0, 10, press_y, 1, true);
                r.touch(2, 10, press_y, 1, true);
            }
            Json size;
            for (const Json& m : r.receive(parse(std::string(R"({"type":"ops","ops":[{"op":"style","id":)") + grow + R"(,"name":"height","value":"80px"}]})"))) {
                if (m["type"].asString() == "size") size = m;
            }
            r.paint();
            for (const Json& m : r.tick())
                if (m["type"].asString() == "size") size = m;
            const Renderer::ResizeDiff* d = r.resize_diff();
            return std::make_pair(d ? *d : Renderer::ResizeDiff{}, d != nullptr && size["resize"].isObject());
        };
        auto [below, sent] = run("11", -1);
        check(sent && below.old_height == 100 && below.new_height == 130 && below.unchanged_top == 100 && !below.keeps_bottom,
              "growing at the bottom keeps the top (top " + std::to_string(below.unchanged_top) + ")");
        auto [above, sent2] = run("10", -1);
        check(sent2 && above.unchanged_top == 50 && above.unchanged_bottom == 100 && above.keeps_bottom,
              "growing at the top keeps the bottom (top " + std::to_string(above.unchanged_top) + ", bottom " + std::to_string(above.unchanged_bottom) + ")");
        auto [pressed_below, s3] = run("10", 75);
        check(pressed_below.has_pointer && pressed_below.pointer_to - pressed_below.pointer_from == 30 && pressed_below.keeps_bottom,
              "a press below what grew keeps the pressed element still");
        auto [pressed_on, s4] = run("10", 25);
        check(pressed_on.has_pointer && pressed_on.pointer_to == pressed_on.pointer_from && !pressed_on.keeps_bottom,
              "a press on what grew keeps its top still");
    }

    // ── painting: borders, tiles, object-fit, gradient stops, ellipsis, filters, canvas order ──
    {
        struct Images : StubBackend {
            bool load_image(int, const std::string&, float& w, float& h) override {
                w = 20;
                h = 10;
                return true;
            }
        };
        Images ib;
        Renderer p(300, 1, false, ib);
        p.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:60px;height:40px;border:4px dashed #333"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["style","width:60px;height:40px;background:url(data:image/png;base64,AAAA) 0 0/20px 10px repeat"]]},{"op":"ins","parent":4,"id":11,"before":null},
            {"op":"el","id":12,"tag":"img","attrs":[["src","data:image/png;base64,BBBB"],["style","display:block;width:40px;height:40px;object-fit:contain"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"div","attrs":[["style","width:40px;height:40px;background:conic-gradient(red 0 50%, blue 0)"]]},{"op":"ins","parent":4,"id":13,"before":null},
            {"op":"el","id":14,"tag":"div","attrs":[["style","width:40px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis"]]},{"op":"ins","parent":4,"id":14,"before":null},
            {"op":"text","id":15,"data":"abcdefghij"},{"op":"ins","parent":14,"id":15,"before":null},
            {"op":"el","id":16,"tag":"div","attrs":[["style","width:20px;height:20px;background:red;filter:blur(2px) brightness(1.5)"]]},{"op":"ins","parent":4,"id":16,"before":null},
            {"op":"el","id":17,"tag":"div","attrs":[["style","width:20px;height:20px;backdrop-filter:blur(4px)"]]},{"op":"ins","parent":4,"id":17,"before":null}]})J"));
        DisplayList list = p.paint();
        auto count = [&](Cmd kind) { return std::count_if(list.begin(), list.end(), [&](const Command& c) { return c.kind == kind; }); };
        bool dashes = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::Path && c.path && c.path->stroke && !c.path->dashes.empty(); });
        check(dashes, "a dashed border is stroked with dashes");

        // The conformance spinner: a circle whose top side has its own colour.
        Renderer ring(300, 1, false, ib);
        ring.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:46px;height:46px;border-radius:50%;border:6px solid #ddd;border-top-color:#36c;box-sizing:border-box"]]},{"op":"ins","parent":4,"id":10,"before":null}]})J"));
        DisplayList rl = ring.paint();
        int rings = 0, clipped = 0;
        for (size_t k = 0; k < rl.size(); k++) {
            const Command& c = rl[k];
            if (c.kind != Cmd::Path || !c.path || !c.path->fill || !c.path->evenodd) continue;
            bool curved = std::count(c.path->ops.begin(), c.path->ops.end(), Path::Cubic) == 8;
            if (curved) rings++;
            if (curved && k > 0 && rl[k - 1].kind == Cmd::ClipPush && rl[k - 1].path) clipped++;
        }
        bool strips = std::any_of(rl.begin(), rl.end(), [](const Command& c) { return c.kind == Cmd::FillRect; });
        check(rings == 4 && clipped == 4 && !strips, "rounded borders of different colours are rings clipped per side, not rectangles");

        // Chromium's dashes: by width on square sides, and round the centre line of rounded ones.
        auto strokes = [&](const char* style) {
            Renderer br(300, 1, false, ib);
            br.receive(parse(std::string(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"div","attrs":[["style","width:100px;height:40px;)J") + style + R"J("]]},{"op":"ins","parent":4,"id":10,"before":null}]})J"));
            std::vector<std::pair<const Command*, const Command*>> out;  // each stroke and the clip it is under
            static DisplayList keep;
            keep = br.paint();
            const Command* clip = nullptr;
            for (const Command& c : keep) {
                if (c.kind == Cmd::ClipPush) clip = &c;
                else if (c.kind == Cmd::Path && c.path && c.path->stroke) out.emplace_back(&c, clip);
            }
            return out;
        };
        auto top = strokes("border:1px dashed #000");
        check(!top.empty() && top[0].first->path->dashes.size() == 2 && top[0].first->path->dashes[0] == 3 && top[0].first->path->line_cap == 0,
              "a 1px dash is three widths long");
        auto wide = strokes("border:4px dashed #000");
        // Left, then top: 108 px along the top, 8 px dashes; 9 of them leave 36 px for 8 gaps, the nearest to 4 px each.
        check(wide.size() == 4 && wide[1].first->path->dashes[0] == 8 && std::fabs(wide[1].first->path->dashes[1] - 4.5f) < 1e-3f,
              "a 4px dash is two widths, its gaps fitted to end in a dash");
        auto small = strokes("border:2px dotted #000");
        check(!small.empty() && small[0].first->path->line_cap == 0 && small[0].first->path->dashes[0] == 2 && small[0].first->path->dashes[1] == 2,
              "2px dots are square");
        auto big = strokes("border:6px dotted #000");
        check(!big.empty() && big[0].first->path->line_cap == 1 && big[0].first->path->dashes[0] == 0, "6px dots are round");
        auto round = strokes("border:3px dashed #000;border-radius:12px");
        check(round.size() == 1 && round[0].first->path->dashes.size() == 2 && round[0].second && round[0].second->path && round[0].second->path->evenodd &&
                  std::count(round[0].first->path->ops.begin(), round[0].first->path->ops.end(), Path::Cubic) == 4,
              "a rounded dashed border is one dashed centre line under a clip to its ring (" + std::to_string(round.size()) + " strokes)");
        auto mixed = strokes("border:3px dashed #000;border-top:3px dashed #f00;border-radius:12px");
        check(mixed.size() == 4, "rounded dashed sides of different colours stroke the centre line under each side's share (" + std::to_string(mixed.size()) + ")");
        check(count(Cmd::Image) >= 13, "a repeated background is drawn once per tile, and the image once (got " + std::to_string(count(Cmd::Image)) + ")");
        bool contained = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::Image && c.rect.w == 40 && c.rect.h == 20; });
        check(contained, "object-fit: contain fits the image inside its box");
        bool stops = std::any_of(list.begin(), list.end(), [](const Command& c) {
            return c.kind == Cmd::Conic && c.stops.size() >= 4 && c.stops[1].offset == 0.5f && c.stops[2].offset == 0.5f;
        });
        check(stops, "a stop at 0 after one at 50% moves up to 50%");

        // Repeating gradients: a period of zero never filled the line and grew the stops until memory ran out, a tiny
        // one made millions; both are the average colour now, and a usual one still repeats.
        auto repeating = [&](const char* background) {
            Renderer g(300, 1, false, ib);
            g.receive(parse(std::string(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"div","attrs":[["style","width:300px;height:100px;background:)J") + background + R"J("]]},{"op":"ins","parent":4,"id":10,"before":null}]})J"));
            static DisplayList keep;
            keep = g.paint();
            for (const Command& c : keep)
                if (c.kind == Cmd::Linear || c.kind == Cmd::Radial || c.kind == Cmd::Conic) return c.stops;
            return std::vector<Stop>{};
        };
        auto solid = [](const std::vector<Stop>& st, Color want) {
            if (st.empty() || st.size() > 4) return false;
            for (const Stop& s : st)
                if (std::abs(s.color.r - want.r) > 1 || std::abs(s.color.g - want.g) > 1 || std::abs(s.color.b - want.b) > 1 || std::abs(s.color.a - want.a) > 1) return false;
            return true;
        };
        // The degenerate-period / non-finite fallback: exactly two stops at 0 and 1 of one average colour. Fails
        // for an empty or truncated list, so a dropped or partly-unrolled gradient cannot pass.
        auto averaged = [&](const std::vector<Stop>& st, Color want) {
            return st.size() == 2 && st.front().offset == 0.0f && st.back().offset == 1.0f && solid(st, want);
        };
        // Every stop offset non-decreasing: a tile seam emitted twice an ulp apart would be re-paired by a painter
        // that sorts stops by offset, turning the ramp between the seams solid.
        auto ordered = [&](const std::vector<Stop>& st) {
            for (size_t i = 1; i < st.size(); i++) if (st[i].offset < st[i - 1].offset) return false;
            return true;
        };
        auto stopAt = [&](const std::vector<Stop>& st, float offset, bool blue) {
            for (const Stop& s : st)
                if (std::abs(s.offset - offset) < 1e-6f && (blue ? (s.color.b > 200 && s.color.r < 55) : (s.color.r > 200 && s.color.b < 55))) return true;
            return false;
        };
        for (const char* zero : {"repeating-linear-gradient(to right,red 50%,blue 50%)", "repeating-radial-gradient(red 10px,blue 10px)",
                                 "repeating-conic-gradient(red 90deg,blue 90deg)", "repeating-linear-gradient(red 0,blue 0)",
                                 "repeating-linear-gradient(red 120%,blue 120%)"}) {
            auto st = repeating(zero);
            check(solid(st, Color{128, 0, 128, 255}), std::string("a repeating gradient with a period of zero is its average colour: ") + zero + " (" + std::to_string(st.size()) + " stops)");
        }
        auto tiny = repeating("repeating-linear-gradient(to right,red 0,red 0.001px,blue 0.001px,blue 0.002px)");
        check(solid(tiny, Color{128, 0, 128, 255}), "a repeating gradient of a period too small to draw is its average colour (" + std::to_string(tiny.size()) + " stops)");
        auto fade = repeating("repeating-linear-gradient(to right,rgba(255,0,0,0) 0,rgba(255,0,0,0) 0.001px,rgb(0,0,255) 0.001px,rgb(0,0,255) 0.002px)");
        check(solid(fade, Color{0, 0, 255, 128}), "the average is premultiplied: transparent red adds no red");
        auto stripes = repeating("repeating-linear-gradient(to right,red 0 10px,blue 10px 20px)");
        check(stripes.size() >= 60 && stripes.size() <= 4096 && stripes.front().offset <= 0 && stripes.back().offset >= 1, "a usual repeating gradient still repeats (" + std::to_string(stripes.size()) + " stops)");
        auto offset = repeating("repeating-linear-gradient(to right,red 50px,blue 60px,red 70px)");
        check(offset.size() >= 40 && offset.front().offset <= 0 && offset.back().offset >= 1, "a period that starts inside the line repeats both ways (" + std::to_string(offset.size()) + " stops)");
        // An interior transparent stop is doubled to fade correctly; a zero-period average must still weigh it
        // once, so red|transparent|blue at one offset averages to half-alpha purple, not a third.
        auto midfade = repeating("repeating-linear-gradient(to right,red 50%,transparent 50%,blue 50%)");
        check(averaged(midfade, Color{128, 0, 128, 128}), "a zero-period average counts an interior transparent stop once (" + std::to_string(midfade.size()) + " stops)");
        // Stops authored far outside the box still describe a fine period that must repeat, not flatten to its
        // average: red and blue 10px apart at 30000px are the same stripes as at 0px.
        auto farout = repeating("repeating-linear-gradient(to right,red 30000px,blue 30010px)");
        check(farout.size() >= 20 && farout.size() <= 4096 && farout.front().offset <= 0 && farout.back().offset >= 1, "a repeating gradient placed far outside the box still repeats (" + std::to_string(farout.size()) + " stops)");
        auto faroutx = repeating("repeating-linear-gradient(to right,red 30000px,red 30005px,blue 30005px,blue 30010px)");
        check(faroutx.size() >= 20 && faroutx.size() <= 4096 && faroutx.front().offset <= 0 && faroutx.back().offset >= 1, "far-outside hard stripes repeat across the box (" + std::to_string(faroutx.size()) + " stops)");
        // Stops wholly to the LEFT of the box must slide to bracket it, not overshoot past it, and still repeat.
        auto farleft = repeating("repeating-linear-gradient(to right,blue -30010px,red -30000px)");
        check(farleft.size() >= 20 && farleft.size() <= 4096 && farleft.front().offset <= 0 && farleft.back().offset >= 1, "a repeating gradient wholly left of the box still repeats (" + std::to_string(farleft.size()) + " stops)");
        // The slide must land the period bracketing the box, not a whole period past it: one red stop at -250% and
        // 2048 blue at -50% (period 200%) slid to [-50%, 150%] is 2049 stops that already cover both edges, whereas
        // overshooting to [150%, 350%] would need a second copy, exceed the budget and flatten to purple.
        std::string farleftBig = "repeating-linear-gradient(to right,red -250%";
        for (int i = 0; i < 2048; i++) farleftBig += ",blue -50%";
        farleftBig += ")";
        auto farleftFit = repeating(farleftBig.c_str());
        check(farleftFit.size() == 2049 && farleftFit.front().offset <= 0 && farleftFit.back().offset >= 1, "a wide period wholly left of the box slides to bracket it within the budget (" + std::to_string(farleftFit.size()) + " stops)");
        // A gradient whose outer stop sits far away but whose period already spans the box keeps its in-box
        // ramp: it must not be slid out of view or flattened to a solid colour.
        auto cover = repeating("repeating-linear-gradient(to right,red -1000000px,red 0px,blue 150px,blue 1000000px)");
        bool cover_red0 = false, cover_blue_half = false;
        for (const Stop& s : cover) {
            if (std::abs(s.offset) < 0.01f && s.color.r > 200 && s.color.b < 55) cover_red0 = true;
            if (std::abs(s.offset - 0.5f) < 0.01f && s.color.b > 200 && s.color.r < 55) cover_blue_half = true;
        }
        check(cover.size() == 4 && cover.front().offset <= 0 && cover.back().offset >= 1 && cover_red0 && cover_blue_half, "a period that already spans the box keeps its exact in-box ramp (" + std::to_string(cover.size()) + " stops)");
        // A fine period placed far outside the box must stay within the painter's stop budget once unrolled (the
        // non-zero phase here unrolls to 4098 stops without the cap): it is drawn within the budget, not beyond.
        auto budget = repeating("repeating-linear-gradient(to right,red 30000.002px,transparent 30000.1485px,blue 30000.295px)");
        check(averaged(budget, Color{128, 0, 128, 128}), "a far fine-period gradient is averaged within the stop budget (" + std::to_string(budget.size()) + " stops)");
        // An infinite stop offset (1e999%) must not corrupt the average to a transparent colour; an all-red
        // gradient stays opaque red.
        auto infoff = repeating("repeating-linear-gradient(to right,red 0%,red 50%,red 1e999%)");
        check(averaged(infoff, Color{255, 0, 0, 255}), "an infinite stop offset averages to opaque red, not transparent (" + std::to_string(infoff.size()) + " stops)");
        // Authored transparent stops that expand past the budget are averaged, never forwarded as an oversized
        // list the painter would refuse.
        std::string manyStops = "repeating-linear-gradient(to right,red 0px";
        for (int i = 0; i < 2100; i++) manyStops += ",transparent 150px";
        manyStops += ",blue 300px)";
        auto oversized = repeating(manyStops.c_str());
        check(averaged(oversized, Color{128, 0, 128, 128}), "a transparent-stop expansion past the budget is averaged (" + std::to_string(oversized.size()) + " stops)");
        // A fine in-box period that fits in 4096 stops must tile and cover both edges, not be flattened by
        // rounding as the copies are placed (the offsets are accumulated in double for this reason).
        auto fits = repeating("repeating-linear-gradient(to right,red 0.006%,blue 0.05485%)");
        check(fits.size() > 2 && fits.size() <= 4096 && fits.front().offset <= 0 && fits.back().offset >= 1, "a fine period that fits the budget tiles and covers the box (" + std::to_string(fits.size()) + " stops)");
        // Exactly at the budget: the 4096th stop lands on 1, so the gradient is forwarded whole, not averaged.
        auto exact = repeating("repeating-linear-gradient(to right,red -2.9118052e-9%,blue 0.048828122%)");
        check(exact.size() == 4096 && exact.front().offset <= 0 && exact.back().offset >= 1, "a period that needs exactly the budget is forwarded and covers the box (" + std::to_string(exact.size()) + " stops)");
        // A 4096th stop placed a hair short of 1 in double still narrows to the 1.0f the painter sees: coverage
        // is judged on that float, so the gradient tiles rather than flattening.
        auto edgeRound = repeating("repeating-linear-gradient(to right,red -2.8e-9%,blue 0.048828122%)");
        check(edgeRound.size() == 4096 && edgeRound.front().offset <= 0 && edgeRound.back().offset >= 1, "a last stop that rounds onto the edge counts as covering it (" + std::to_string(edgeRound.size()) + " stops)");
        // A copy that reaches an edge may stop at its first stop there: with the end stop duplicated 2048 times,
        // one more copy costs two stops, not 2049, so the early-out must not charge it whole and flatten the
        // gradient. Both edges.
        std::string dupEndCss = "repeating-linear-gradient(to right,red 0%";
        for (int i = 0; i < 2048; i++) dupEndCss += ",blue 50%";
        dupEndCss += ")";
        auto dupEnd = repeating(dupEndCss.c_str());
        check(dupEnd.size() == 2051 && dupEnd.front().offset <= 0 && dupEnd.back().offset >= 1 && ordered(dupEnd), "a duplicated end stop lets the last copy stop early and the gradient fit (" + std::to_string(dupEnd.size()) + " stops)");
        std::string dupStartCss = "repeating-linear-gradient(to right,red 50%";
        for (int i = 0; i < 2047; i++) dupStartCss += ",red 50%";
        dupStartCss += ",blue 100%)";
        auto dupStart = repeating(dupStartCss.c_str());
        check(dupStart.size() == 2051 && dupStart.front().offset <= 0 && dupStart.back().offset >= 1, "a duplicated start stop lets the first copy stop early and the gradient fit (" + std::to_string(dupStart.size()) + " stops)");
        // A seed that already reaches into the box must not be slid: slid a period left, this one would need a
        // whole 2049-stop copy to come back and exceed the budget, where in place it covers with two more stops.
        std::string inBoxCss = "repeating-linear-gradient(to right,red 25%";
        for (int i = 0; i < 2047; i++) inBoxCss += ",red 25%";
        inBoxCss += ",blue 125%)";
        auto inBox = repeating(inBoxCss.c_str());
        check(inBox.size() == 2051 && inBox.front().offset <= 0 && inBox.back().offset >= 1, "a seed reaching into the box is tiled in place, not slid (" + std::to_string(inBox.size()) + " stops)");
        // A slid seed whose copies are placed as seed + copy * period can put the two stops of a tile seam an ulp
        // apart in the wrong order; the seam stop must reuse the offset already emitted so the list stays ordered.
        auto seam = repeating("repeating-linear-gradient(to right,red -48%,blue -5.5%)");
        check(seam.size() >= 4 && seam.front().offset <= 0 && seam.back().offset >= 1 && ordered(seam), "tile seams are emitted at one offset, never an ulp apart in reverse (" + std::to_string(seam.size()) + " stops)");
        // A far end stop must not cancel the near ones: with blue at 1e20%, last - first loses the 25%, so a seam
        // placed as offset - period would put the previous tile's blue at 0 instead of 25% (a blue-to-red ramp where
        // the box is solid blue). The seam reuses the emitted offset, and the slide is placed from the end nearest
        // the box.
        auto farEnd = repeating("repeating-linear-gradient(to right,red 25%,blue 75%,blue 1e20%)");
        check(farEnd.size() == 5 && farEnd.front().offset <= 0 && farEnd.back().offset >= 1 && stopAt(farEnd, 0.25f, true) && stopAt(farEnd, 0.25f, false) && ordered(farEnd), "a far end stop does not cancel the previous tile's seam at 25% (" + std::to_string(farEnd.size()) + " stops)");
        auto farSlide = repeating("repeating-linear-gradient(to right,red 125%,blue 175%,blue 1e20%)");
        check(farSlide.size() == 3 && farSlide.front().offset <= 0 && farSlide.back().offset >= 1 && stopAt(farSlide, 1.25f, true) && ordered(farSlide), "a far end stop does not cancel the slid seam at 125% (" + std::to_string(farSlide.size()) + " stops)");
        auto farStart = repeating("repeating-linear-gradient(to right,red -1e20%,red 25%,blue 75%)");
        check(farStart.size() == 5 && farStart.front().offset <= 0 && farStart.back().offset >= 1 && stopAt(farStart, 0.75f, true) && stopAt(farStart, 0.75f, false) && ordered(farStart), "a far start stop does not cancel the next tile's seam at 75% (" + std::to_string(farStart.size()) + " stops)");
        // More authored stops than the budget, with no transparency to expand, are averaged rather than
        // forwarded whole.
        std::string manyOpaque = "repeating-linear-gradient(to right,red 0px";
        for (int i = 1; i <= 4100; i++) manyOpaque += ",red " + std::to_string(i) + "px";
        manyOpaque += ")";
        auto opaqueBig = repeating(manyOpaque.c_str());
        check(averaged(opaqueBig, Color{255, 0, 0, 255}), "an authored list past the budget is averaged (" + std::to_string(opaqueBig.size()) + " stops)");
        // A stop whose offset tokenises to a non-finite value (0e999 -> 0 * inf = NaN) must never reach the
        // painter as a NaN stop; the gradient draws its average instead.
        auto nanoff = repeating("repeating-linear-gradient(to right,red 30000px,lime 0e999%,blue 30010px)");
        bool nan_finite = true;
        for (const Stop& s : nanoff) if (!std::isfinite(s.offset)) nan_finite = false;
        check(nan_finite && averaged(nanoff, Color{64, 128, 64, 255}), "a non-finite stop offset is averaged, never a NaN stop (" + std::to_string(nanoff.size()) + " stops)");
        // A tiny-period repeating gradient with an interior transparent stop draws its premultiplied average,
        // instead of unrolling and then doubling every transparent copy past the stop budget.
        auto tinytrans = repeating("repeating-linear-gradient(to right,red 0,transparent 0.04%,blue 0.08%)");
        check(averaged(tinytrans, Color{128, 0, 128, 128}), "a tiny-period transparent gradient averages without exploding the stop list (" + std::to_string(tinytrans.size()) + " stops)");
        bool mark = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::Text && c.text == "\xE2\x80\xA6"; });
        bool cut = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::Text && !c.text.empty() && c.text.size() < 10 && c.text[0] == 'a'; });
        check(mark && cut, "text-overflow: ellipsis cuts the line and draws the mark");
        auto push = std::find_if(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::FilterPush; });
        check(push != list.end() && push->filter && push->filter->size() == 2 && (*push->filter)[0].kind == Filter::Blur && (*push->filter)[0].amount == 2 &&
                  count(Cmd::FilterPop) == 1,
              "filter wraps the element in a FilterPush with its functions");
        check(count(Cmd::Backdrop) == 1, "backdrop-filter draws a Backdrop");

        // Word spacing: each glyph where layout put it, in one run per word.
        Renderer ws(300, 1, false, ib);
        ws.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","word-spacing:10px;white-space:pre"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"text","id":11,"data":"ab cd e"},{"op":"ins","parent":10,"id":11,"before":null}]})J"));
        DisplayList wl = ws.paint();
        std::string runs;
        for (const Command& c : wl)
            if (c.kind == Cmd::Text && c.text != " ") runs += c.text + "@" + std::to_string(int(c.x)) + " ";
        // The no-break space stays inside its word, so it reaches the painter in the middle of a run (the
        // stub measures it as its two bytes).
        check(runs == "ab@0 cd@34 e@76 ", "word-spacing widens each space as it paints (got " + runs + ")");

        // font-variant-numeric and font-feature-settings reach the backend as one feature list.
        struct Features : StubBackend {
            std::set<std::string> asked;
            int create_font(const std::string& families, float size, int weight, bool italic, const std::string& features, FontMetrics& out) override {
                asked.insert(features);
                return StubBackend::create_font(families, size, weight, italic, features, out);
            }
        };
        Features feb;
        Renderer fe(300, 1, false, feb);
        fe.receive(parse(R"J({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","font-variant-numeric:tabular-nums slashed-zero"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"text","id":11,"data":"10"},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"el","id":12,"tag":"span","attrs":[["style","font-feature-settings:'zero' off, 'ss01'"]]},{"op":"ins","parent":10,"id":12,"before":null},
            {"op":"text","id":13,"data":"20"},{"op":"ins","parent":12,"id":13,"before":null}]})J"));
        fe.paint();
        check(feb.asked.count("tnum=1,zero=1") && feb.asked.count("ss01=1,tnum=1,zero=0"),
              "font features from the variant and the settings, the settings winning");

        // Canvas baselines: top, middle and bottom on the em box, hanging and ideographic on the font.
        struct EmBox : StubBackend {
            int create_font(const std::string&, float size, int, bool, const std::string&, FontMetrics& out) override {
                out.ascent = size * 0.9f;
                out.descent = size * 0.3f;
                out.height = size * 1.2f;
                out.em_ascent = size * 0.7f;
                out.em_descent = size * 0.3f;
                return ++next;
            }
        };
        EmBox eb;
        Renderer em(300, 1, false, eb);
        em.receive(parse(PAGE));
        std::string baselines;
        for (const char* baseline : {"top", "hanging", "middle", "alphabetic", "bottom", "ideographic"})
            baselines += std::string(R"(["s","textBaseline",")") + baseline + R"("],["c","fillText","x",0,0],)";
        baselines.pop_back();
        em.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":40,"tag":"canvas","attrs":[["width","40"],["height","40"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"canvas","id":40,"cmds":[["s","font","10px sans-serif"],)" + baselines + "]}]}"));
        std::string ys;
        for (const CanvasOp& op : eb.runs.back())
            if (op.kind == CanvasOp::Text) ys += css::format_number(op.y, 3) + " ";
        check(ys == "7 7.2 2 0 -3 -3 ", "canvas text sits on the em box (got " + ys + ")");

        // A broken PNG keeps the rows before the damage. These are 2 × 3 RGBA, their data one stored block.
        auto png = [](std::vector<uint8_t> rows, bool end) {
            auto crc = [](const std::vector<uint8_t>& v, size_t from) {
                uint32_t c = 0xFFFFFFFFu;
                for (size_t i = from; i < v.size(); i++) {
                    c ^= v[i];
                    for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
                }
                return c ^ 0xFFFFFFFFu;
            };
            std::vector<uint8_t> out{0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
            auto chunk = [&](const char* type, const std::vector<uint8_t>& data) {
                for (int s = 24; s >= 0; s -= 8) out.push_back(uint8_t(data.size() >> s));
                size_t start = out.size();
                out.insert(out.end(), type, type + 4);
                out.insert(out.end(), data.begin(), data.end());
                uint32_t c = crc(out, start);
                for (int s = 24; s >= 0; s -= 8) out.push_back(uint8_t(c >> s));
            };
            chunk("IHDR", {0, 0, 0, 2, 0, 0, 0, 3, 8, 6, 0, 0, 0});
            std::vector<uint8_t> z{0x78, 0x01, 0x01, uint8_t(rows.size()), uint8_t(rows.size() >> 8), uint8_t(~rows.size()), uint8_t(~rows.size() >> 8)};
            z.insert(z.end(), rows.begin(), rows.end());
            chunk("IDAT", z);
            if (end) chunk("IEND", {});
            return out;
        };
        std::vector<uint8_t> good(27, 0x7f), bad = good, truncated(good.begin(), good.begin() + 20);
        good[0] = good[9] = good[18] = 0;
        bad[0] = bad[9] = 0;
        bad[18] = 7;
        truncated[0] = truncated[9] = 0;
        auto rows_of = [](const std::vector<uint8_t>& p) { return png_intact_rows(p.data(), p.size()); };
        check(rows_of(png(good, true)) == SIZE_MAX, "an intact PNG keeps every row");
        check(rows_of(png(bad, false)) == 2, "rows stop at an unknown filter type");
        check(rows_of(png(truncated, false)) == 2, "and where the data runs out");

        // What an image's header declares, before a decoder allocates it.
        auto size_of = [](std::vector<uint8_t> bytes) {
            uint32_t w = 0, h = 0;
            return image_dimensions(bytes.data(), bytes.size(), w, h) ? std::to_string(w) + "x" + std::to_string(h) : std::string("none");
        };
        std::vector<uint8_t> intact = png(good, true);
        std::vector<uint8_t> gif = {'G', 'I', 'F', '8', '9', 'a', 3, 0, 2, 0};
        std::vector<uint8_t> jpeg = {0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC2, 0, 17, 8, 0, 5, 0, 7, 3};
        std::vector<uint8_t> webp = {'R', 'I', 'F', 'F', 0, 0, 0, 0, 'W', 'E', 'B', 'P', 'V', 'P', '8', 'X', 10, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 19, 0, 0};
        std::vector<uint8_t> short_jpeg = {0xFF, 0xD8, 0xFF, 0xE0, 0, 40};
        check(size_of(intact) == "2x3" && size_of(gif) == "3x2" && size_of(jpeg) == "7x5" && size_of(webp) == "10x20" && size_of(short_jpeg) == "none",
              "image_dimensions reads PNG, GIF, JPEG and WebP headers (got " + size_of(intact) + ", " + size_of(gif) + ", " + size_of(jpeg) + ", " + size_of(webp) + ", " +
                  size_of(short_jpeg) + ")");

        // The CPU filters of the painters that draw into memory.
        auto filtered = [](std::vector<uint8_t> px, int w, int h, Filters fs, bool bgra = false) {
            filter_pixels(px.data(), w, h, size_t(w) * 4, bgra, fs, 1);
            return px;
        };
        auto f = [](Filter::Kind kind, float amount) { Filter x; x.kind = kind; x.amount = amount; return x; };
        std::vector<uint8_t> red{255, 0, 0, 255};
        auto grey = filtered(red, 1, 1, {f(Filter::Grayscale, 1)});
        check(grey[0] == 54 && grey[1] == 54 && grey[2] == 54 && grey[3] == 255, "grayscale(1) of red is its luminance (got " + std::to_string(grey[0]) + ")");
        auto bgr = filtered({0, 0, 255, 255}, 1, 1, {f(Filter::Grayscale, 1)}, true);
        check(bgr[2] == 54 && bgr[0] == 54, "B, G, R, A bytes are read as such");
        auto faded = filtered(red, 1, 1, {f(Filter::Opacity, 0.5f)});
        check(faded[3] == 128 && faded[0] == 128, "opacity keeps the colour premultiplied (got " + std::to_string(faded[0]) + ", " + std::to_string(faded[3]) + ")");
        auto inverted = filtered({0, 0, 0, 255}, 1, 1, {f(Filter::Invert, 1)});
        check(inverted[0] == 255 && inverted[1] == 255, "invert(1) of black is white");
        std::vector<uint8_t> dot(21 * 21 * 4, 0);
        dot[(10 * 21 + 10) * 4 + 3] = 255;
        auto blurred = filtered(dot, 21, 21, {f(Filter::Blur, 2)});
        int spread = 0, total = 0;
        for (int i = 0; i < 21 * 21; i++) {
            spread += blurred[size_t(i) * 4 + 3] > 0;
            total += blurred[size_t(i) * 4 + 3];
        }
        check(spread > 9 && blurred[(10 * 21 + 10) * 4 + 3] < 255 && total > 180 && total < 330, "blur spreads a dot and keeps its alpha (" + std::to_string(spread) + " px, " + std::to_string(total) + ")");
        // An infinite scale (or amount) was a blur radius of 2^30 and a 2^31-step loop per line; NaN, an int from NaN.
        {
            auto started = std::chrono::steady_clock::now();
            std::vector<uint8_t> px = dot;
            const float inf = std::numeric_limits<float>::infinity();
            filter_pixels(px.data(), 21, 21, 21 * 4, false, {f(Filter::Blur, 2)}, inf);
            filter_pixels(px.data(), 21, 21, 21 * 4, false, {f(Filter::Blur, inf), f(Filter::Blur, std::nanf(""))}, 1);
            Filter far = f(Filter::DropShadow, inf);
            far.x = inf;
            far.y = std::nanf("");
            far.color = Color{0, 0, 0, 255};
            filter_pixels(px.data(), 21, 21, 21 * 4, false, {far}, 1);
            double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count();
            check(ms < 1000, "infinite and NaN blur radii and shadow offsets finish (" + std::to_string(int(ms)) + " ms)");
        }
        Filter shadow = f(Filter::DropShadow, 0);
        shadow.x = 3;
        shadow.color = Color{0, 0, 255, 255};
        std::vector<uint8_t> left(8 * 1 * 4, 0);
        left[0] = 255; left[3] = 255;
        auto shadowed = filtered(left, 8, 1, {shadow});
        check(shadowed[3 * 4 + 2] == 255 && shadowed[3 * 4 + 3] == 255 && shadowed[0] == 255, "a drop shadow is the alpha in its colour, moved, under the image");

        // A canvas drawn from a canvas made in the same batch, whose drawing comes after.
        Renderer c(300, 1, false, ib);
        c.receive(parse(PAGE));
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":40,"tag":"canvas","attrs":[["width","40"],["height","40"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"canvas","id":40,"cmds":[["c","drawImage",{"n":41},0,0]]},
            {"op":"el","id":41,"tag":"canvas","attrs":[["width","10"],["height","10"]]},
            {"op":"canvas","id":41,"cmds":[["s","fillStyle","#ff0000"],["c","fillRect",0,0,10,10]]}]})"));
        bool sourced = false;
        for (const auto& run : ib.runs)
            for (const CanvasOp& op : run)
                if (op.kind == CanvasOp::Image && op.canvas == 41) sourced = true;
        check(sourced, "drawImage of a canvas drawn later in the batch draws it");

        // A refused image is the placeholder in a canvas too, drawn as the document draws it.
        ib.runs.clear();
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":50,"tag":"img","attrs":[["src","data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>"]]},
            {"op":"el","id":42,"tag":"canvas","attrs":[["width","48"],["height","48"]]},{"op":"ins","parent":4,"id":42,"before":null},
            {"op":"canvas","id":42,"cmds":[["c","drawImage",{"n":50},0,0,24,24]]}]})"));
        int images = 0, fills = 0;
        bool inside = true;
        for (const auto& run : ib.runs)
            for (const CanvasOp& op : run) {
                if (op.kind == CanvasOp::Image) images++;
                if (op.kind != CanvasOp::Fill) continue;
                fills++;
                inside = inside && op.paint.color.r == 136 && op.clips && !op.clips->empty();
                for (size_t i = 0; i < op.path.points.size(); i++) inside = inside && op.path.points[i] >= -0.01f && op.path.points[i] <= 24.01f;
            }
        check(images == 0 && fills == 3 && inside, "drawImage of the placeholder draws its drawing into the destination (" + std::to_string(images) + " images, " + std::to_string(fills) + " fills)");

        // At the cap a new Path2D is refused; one the widget holds keeps drawing.
        ib.runs.clear();
        std::string cmds = R"(["s","fillStyle","#ff0000"],["pn",1,"M1 1 L3 1 L3 3 Z"])";
        for (int i = 2; i <= 4097; i++) cmds += R"(,["pn",)" + std::to_string(i) + R"(,"M0 0 L4 0 L4 4 Z"])";
        cmds += R"(,["c","fill",{"p":1}],["c","fill",{"p":4097}])";
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":43,"tag":"canvas","attrs":[["width","8"],["height","8"]]},{"op":"ins","parent":4,"id":43,"before":null},
            {"op":"canvas","id":43,"cmds":[)" + cmds + R"(]}]})"));
        fills = 0;
        bool first = false;
        for (const auto& run : ib.runs)
            for (const CanvasOp& op : run)
                if (op.kind == CanvasOp::Fill) {
                    fills++;
                    first = !op.path.points.empty() && op.path.points[0] == 1;
                }
        check(fills == 1 && first, "past 4,096 paths the first still fills and the refused one does not (" + std::to_string(fills) + " fills)");
    }

    // ── grid shorthands ──
    {
        StubBackend gb;
        auto grid = [&](const std::string& style) {
            Renderer g(300, 1, false, gb);
            g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"div","attrs":[["style",")" + style + R"("]]},{"op":"ins","parent":4,"id":10,"before":null},
                {"op":"el","id":11,"tag":"i","attrs":[["style","display:block"]]},{"op":"ins","parent":10,"id":11,"before":null},
                {"op":"el","id":12,"tag":"i","attrs":[["style","display:block"]]},{"op":"ins","parent":10,"id":12,"before":null},
                {"op":"el","id":13,"tag":"i","attrs":[["style","display:block"]]},{"op":"ins","parent":10,"id":13,"before":null}]})"));
            std::string out;
            for (int id : {11, 12, 13}) {
                Json r = g.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"];
                out += css::format_number(r["x"].asNumber(), 1) + "," + css::format_number(r["y"].asNumber(), 1) + " " + css::format_number(r["width"].asNumber(), 1) + "x" +
                       css::format_number(r["height"].asNumber(), 1) + ";";
            }
            return out;
        };
        std::string t = grid("display:grid;grid-template:10px 20px / 30px 40px");
        check(t == "0,0 30x10;30,0 40x10;0,10 30x20;", "grid-template: rows / columns places a 2×2 grid (got " + t + ")");
        t = grid("display:grid;grid:15px / 50px 50px");
        check(t == "0,0 50x15;50,0 50x15;0,15 50x0;", "grid: rows / columns, the third item in an auto row (got " + t + ")");
        t = grid("display:grid;grid:auto-flow 12px / 40px 40px");
        check(t == "0,0 40x12;40,0 40x12;0,12 40x12;", "grid: auto-flow sizes the implicit rows (got " + t + ")");
        t = grid("display:grid;grid-template-columns:50px 50px;grid-template:none");
        check(t == "0,0 300x0;0,0 300x0;0,0 300x0;", "grid-template: none resets the columns (got " + t + ")");
        t = grid("display:grid;grid-template:var(--t);--t:10px / 30px 30px");
        check(t == "0,0 30x10;30,0 30x10;0,10 30x0;", "grid-template from var() (got " + t + ")");
    }

    // ── hostile markup: var() expansion and nested layout are bounded ──
    {
        auto widget = [&](Renderer& g, const std::string& css, const std::string& ops) {
            std::string all = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style"},{"op":"css","id":5,"text":")" +
                              css + R"("},{"op":"ins","parent":3,"id":5,"before":null})" + ops + "]}";
            return g.receive(parse(all));
        };
        auto div = [](int id, int parent, const std::string& style) {
            return R"(,{"op":"el","id":)" + std::to_string(id) + R"(,"tag":"div","attrs":[["style",")" + style + R"("]]},{"op":"ins","parent":)" +
                   std::to_string(parent) + R"(,"id":)" + std::to_string(id) + R"(,"before":null})";
        };
        auto rect = [](Renderer& g, int id) {
            Json r = g.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"];
            return css::format_number(r["x"].asNumber(), 1) + " " + css::format_number(r["width"].asNumber(), 1);
        };
        auto violation = [](const std::vector<Json>& out) {
            std::string found;
            for (const Json& m : out)
                if (m["type"].asString() == "violation") found += m["detail"].asString() + ";";
            return found;
        };

        // Each custom property refers to the one before ten times: --a6 would be a million tokens, --a9 a billion.
        std::string vars = ":root{--a0:1px;";
        for (int k = 1; k <= 9; k++) {
            vars += "--a" + std::to_string(k) + ":";
            for (int f = 0; f < 10; f++) vars += "var(--a" + std::to_string(k - 1) + ") ";
            vars += ";";
        }
        vars += "}";
        Renderer v(300, 1, false, backend);
        widget(v, vars, div(10, 4, "width:var(--a6,30px)") + div(11, 4, "width:var(--a9)") + div(12, 4, "width:50px;margin-left:var(--a2,7px)"));
        check(rect(v, 10) == "0 30", "a var() past the substitution limit takes its fallback (got " + rect(v, 10) + ")");
        check(rect(v, 11) == "0 300", "a declaration past it is unset (got " + rect(v, 11) + ")");
        check(rect(v, 12) == "0 50", "one of a hundred tokens is still substituted, an invalid margin (got " + rect(v, 12) + ")");

        // Flex boxes measure their children several times each: forty nested are 2^40 layouts without the budget.
        std::string nested;
        for (int k = 0; k < 40; k++) nested += div(10 + k, k == 0 ? 4 : 9 + k, "display:flex");
        Renderer n(300, 1, false, backend);
        auto t0 = std::chrono::steady_clock::now();
        std::string seen = violation(widget(n, "", nested));
        double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
        check(seen.find("layout limit") != std::string::npos && ms < 2000,
              "forty nested flex boxes stop at the layout budget (" + seen + " " + std::to_string(int(ms)) + " ms)");
        DisplayList badge = n.paint();
        bool text = std::any_of(badge.begin(), badge.end(), [](const Command& c) { return c.kind == Cmd::Text; });
        check(text && badge.size() < 16, "the stopped widget is the badge alone (" + std::to_string(badge.size()) + " commands)");
        widget(n, "", div(60, 4, "display:flex"));
        check(n.paint().size() == badge.size(), "and lays nothing of the widget's out again");

        std::string usual;
        for (int k = 0; k < 6; k++) usual += div(10 + k, k == 0 ? 4 : 9 + k, "display:flex");
        Renderer u(300, 1, false, backend);
        check(violation(widget(u, "", usual)).empty(), "six nested flex boxes are well within it");

        // List markers out of a style's range are decimal: roman was one more 'M' per thousand, two million of them.
        auto marker = [&](const std::string& type, int start) {
            Renderer m(300, 1, false, backend);
            widget(m, "", R"(,{"op":"el","id":10,"tag":"ol","attrs":[["start",")" + std::to_string(start) + R"("],["style","list-style-type:)" + type +
                              R"("]]},{"op":"ins","parent":4,"id":10,"before":null},{"op":"el","id":11,"tag":"li"},{"op":"ins","parent":10,"id":11,"before":null},)"
                              R"({"op":"text","id":12,"data":"x"},{"op":"ins","parent":11,"id":12,"before":null})");
            DisplayList list = m.paint();
            std::string texts;
            for (const Command& c : list)
                if (c.kind == Cmd::Text && c.text != "x") texts += c.text;
            return texts;
        };
        std::string got = marker("upper-roman", 2000000000);
        check(got == "2000000000.", "an upper-roman marker past 3999 is decimal (got " + got.substr(0, 40) + ", " + std::to_string(got.size()) + " bytes)");
        got = marker("lower-roman", 3999);
        check(got == "mmmcmxcix.", "3999 is still roman (got " + got + ")");
        got = marker("lower-roman", 0);
        check(got == "0.", "and 0 decimal (got " + got + ")");
        got = marker("lower-alpha", -3);
        check(got == "-3.", "an alphabetic marker below 1 is decimal (got " + got + ")");
        got = marker("upper-alpha", 28);
        check(got == "AB.", "28 is AB (got " + got + ")");

        // Path data that goes on after a closepath without a command: `z` took nothing and was repeated forever, a
        // close per turn, until the host ran out of memory. The path ends at the error (SVG 2, "Error handling in path
        // data") and keeps what came before it.
        auto path = [&](const std::string& d) {
            Renderer p(300, 1, false, backend);
            widget(p, "", R"(,{"op":"el","id":10,"tag":"svg","svg":true,"attrs":[["width","40"],["height","40"],["style","display:block"]]},)"
                          R"({"op":"ins","parent":4,"id":10,"before":null},{"op":"el","id":11,"tag":"path","svg":true,"attrs":[["d",")" + d +
                              R"("],["stroke","red"]]},{"op":"ins","parent":10,"id":11,"before":null})");
            size_t ops = 0;
            for (const Command& c : p.paint())
                if (c.path) ops += c.path->ops.size();
            return ops;
        };
        check(path("M0 0z4") <= 2, "a number after z ends the path (" + std::to_string(path("M0 0z4")) + " ops)");
        check(path("M0 0 L10 10 z 4,4 L 20 20") <= 3, "and keeps the subpath before it");
        check(path("M0 0 L10 10 z M20 20 L30 30 z") == 6, "a z followed by a command still starts the next subpath");
        check(path("Z0") == 0, "Z0 draws nothing and ends");

        // font-weight: bolder and lighter on the root are relative to a parent it does not have: the initial 400.
        for (const char* weight : {"bolder", "lighter"}) {
            Renderer f(300, 1, false, backend);
            f.receive(parse(std::string(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[["style","font-weight:)") +
                            weight + R"("]]},{"op":"el","id":3,"tag":"head","adopt":"head"},{"op":"el","id":4,"tag":"body","adopt":"body"},)"
                                     R"({"op":"text","id":5,"data":"x"},{"op":"ins","parent":4,"id":5,"before":null}]})"));
            check(!f.paint().empty(), std::string("a root with font-weight: ") + weight + " lays out");
        }

        // <use> fan-out: ten uses of the level below on each of eight levels were 10^8 drawn elements.
        {
            Renderer u(300, 1, false, backend);
            std::string ops = R"(,{"op":"el","id":10,"tag":"svg","svg":true,"attrs":[["width","40"],["height","40"]]},{"op":"ins","parent":4,"id":10,"before":null},)"
                              R"({"op":"el","id":11,"tag":"defs","svg":true},{"op":"ins","parent":10,"id":11,"before":null},)"
                              R"({"op":"el","id":12,"tag":"rect","svg":true,"attrs":[["id","l0"],["width","1"],["height","1"]]},{"op":"ins","parent":11,"id":12,"before":null})";
            int id = 20;
            for (int level = 1; level <= 8; level++) {
                int group = id++;
                ops += R"(,{"op":"el","id":)" + std::to_string(group) + R"(,"tag":"g","svg":true,"attrs":[["id","l)" + std::to_string(level) +
                       R"("]]},{"op":"ins","parent":11,"id":)" + std::to_string(group) + R"(,"before":null})";
                for (int k = 0; k < 10; k++) {
                    int use = id++;
                    ops += R"(,{"op":"el","id":)" + std::to_string(use) + R"(,"tag":"use","svg":true,"attrs":[["href","#l)" + std::to_string(level - 1) +
                           R"("]]},{"op":"ins","parent":)" + std::to_string(group) + R"(,"id":)" + std::to_string(use) + R"(,"before":null})";
                }
            }
            ops += R"(,{"op":"el","id":)" + std::to_string(id) + R"(,"tag":"use","svg":true,"attrs":[["href","#l8"]]},{"op":"ins","parent":10,"id":)" +
                   std::to_string(id) + R"(,"before":null})";
            widget(u, "", ops);
            auto started = std::chrono::steady_clock::now();
            size_t drawn = u.paint().size();
            double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count();
            check(ms < 2000, "a <use> fan-out of 10^8 is cut short (" + std::to_string(int(ms)) + " ms, " + std::to_string(drawn) + " commands)");
        }

        // Fifteen nested tables: the layout budget stops them as it does nested flex boxes.
        {
            Renderer t(300, 1, false, backend);
            std::string ops;
            int id = 10, parent = 4;
            for (int k = 0; k < 15; k++) {
                int table = id++, row = id++, cell = id++;
                ops += R"(,{"op":"el","id":)" + std::to_string(table) + R"(,"tag":"table"},{"op":"ins","parent":)" + std::to_string(parent) +
                       R"(,"id":)" + std::to_string(table) + R"(,"before":null},{"op":"el","id":)" + std::to_string(row) +
                       R"(,"tag":"tr"},{"op":"ins","parent":)" + std::to_string(table) + R"(,"id":)" + std::to_string(row) +
                       R"(,"before":null},{"op":"el","id":)" + std::to_string(cell) + R"(,"tag":"td"},{"op":"ins","parent":)" +
                       std::to_string(row) + R"(,"id":)" + std::to_string(cell) + R"(,"before":null})";
                parent = cell;
            }
            ops += R"(,{"op":"text","id":)" + std::to_string(id) + R"(,"data":"deep"},{"op":"ins","parent":)" + std::to_string(parent) + R"(,"id":)" +
                   std::to_string(id) + R"(,"before":null})";
            auto started = std::chrono::steady_clock::now();
            widget(t, "", ops);
            t.paint();
            double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count();
            check(ms < 2000, "fifteen nested tables finish (" + std::to_string(int(ms)) + " ms)");
        }

        // A compound that names ::before or ::after matched the generated box too, which got one of its own, without end.
        for (const char* selector : {":last-child::after", ":not(.a)::before", ":nth-child(1n)::after", ":first-child::before",
                                     "div::after{content:'x'} -tag-after::after"}) {
            Renderer g(300, 1, false, backend);
            widget(g, std::string(selector) + "{content:'x'}", div(10, 4, "width:10px"));
            int generated = 0;
            for (const Command& c : g.paint())
                if (c.kind == Cmd::Text && c.text == "x") generated++;
            check(generated >= 1 && generated <= 3, std::string(selector) + " generates a box for an element, none for a generated box (" + std::to_string(generated) + ")");
        }

        // Nothing gets below a float whose height is not finite: the float after it looked for room without end.
        for (const char* height : {"1e39px", "0e999px"}) {
            for (const char* side : {"left", "right"}) {
                Renderer f(300, 1, false, backend);
                auto started = std::chrono::steady_clock::now();
                widget(f, "", div(10, 4, std::string("float:") + side + ";width:80%;height:" + height) + div(11, 4, std::string("float:") + side + ";width:50%;height:10px"));
                f.paint();
                double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count();
                check(ms < 2000, std::string("a float: ") + side + " after one of height " + height + " is placed (" + std::to_string(int(ms)) + " ms)");
            }
        }

        {
            Renderer f(300, 1, false, backend);
            auto out = widget(f, "", div(9, 4, "height:0e999px;padding-top:3px") + div(10, 4, "width:1e39px;height:3e38rem") + div(12, 4, "width:1e30%;height:1px") +
                                         div(13, 12, "width:1e30%;line-height:1e39") + R"(,{"op":"text","id":15,"data":"x"},{"op":"ins","parent":13,"id":15,"before":null})" +
                                         div(14, 4, "width:calc(1e38px * 3);height:1px"));
            auto box = [&](int id) {
                Json r = f.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"];
                return css::format_number(r["width"].asNumber(), 1) + " " + css::format_number(r["height"].asNumber(), 1) + " at " + css::format_number(r["y"].asNumber(), 1);
            };
            check(box(9) == "300 3 at 0" && box(10) == "33554432 33554432 at 3",
                  "0e999px is 0 and a length past 2^25 px is 2^25 px, as written or once its unit is resolved (got " + box(9) + ", " + box(10) + ")");
            check(box(13).rfind("33554432 33554432 at", 0) == 0, "so are a percentage of a percentage and a line height (got " + box(13) + ")");
            check(box(14).rfind("33554432 1 at", 0) == 0, "and a calc() product (got " + box(14) + ")");
            double height = 0;
            for (const Json& m : out)
                if (m["type"].asString() == "size") height = m["height"].asNumber();
            check(height >= 33554432 && height < 1e9, "the host is told a height (got " + css::format_number(height, 1) + ")");
        }

        // An integer past the range of int was cast as it was: undefined, and on x86 the far end of the range.
        {
            struct Features : StubBackend {
                std::string seen;
                int create_font(const std::string& family, float size, int weight, bool italic, const std::string& features, FontMetrics& out) override {
                    seen += features + ";";
                    return StubBackend::create_font(family, size, weight, italic, features, out);
                }
            } fonts;
            Renderer z(300, 1, false, fonts);
            widget(z, "", div(10, 4, "position:relative;height:10px") + div(11, 10, "position:absolute;z-index:99999999999;width:10px;height:10px;background:#f00") +
                              div(12, 10, "position:absolute;z-index:5;width:10px;height:10px;background:#00f") + div(13, 4, "display:flex") +
                              div(14, 13, "order:99999999999;width:10px;height:5px") + div(15, 13, "order:1;width:10px;height:5px") +
                              div(16, 4, "display:grid;grid-template-columns:repeat(99999999999,10px)") + div(17, 16, "height:5px") + div(18, 16, "height:5px") +
                              div(19, 4, "display:grid;grid-template-columns:10px 10px") + div(20, 19, "grid-column:99999999999;height:5px") +
                              div(21, 4, "font-feature-settings:'tnum' 99999999999;color:hsl(0 0e999% 50%);background:rgb(0e999 0 0)") +
                              R"(,{"op":"text","id":22,"data":"x"},{"op":"ins","parent":21,"id":22,"before":null})");
            int red = -1, blue = -1, at = 0;
            Color text{1, 1, 1, 1}, fill{1, 1, 1, 1};
            for (const Command& c : z.paint()) {
                if (c.kind == Cmd::FillRect && c.color.r == 255 && c.color.b == 0) red = at;
                if (c.kind == Cmd::FillRect && c.color.b == 255) blue = at;
                if (c.kind == Cmd::FillRect && c.rect.w == 300 && c.rect.h > 10) fill = c.color;
                if (c.kind == Cmd::Text) text = c.color;
                at++;
            }
            check(blue >= 0 && red > blue, "a z-index past INT_MAX is the highest (" + std::to_string(blue) + ", " + std::to_string(red) + ")");
            check(rect(z, 14) == "10 10", "an order past INT_MAX is the last (got " + rect(z, 14) + ")");
            check(rect(z, 18) == "10 10", "repeat() past its limit is the limit, not nothing (got " + rect(z, 18) + ")");
            check(rect(z, 20) == "299.7 0.3", "a grid line past the limit is the limit (got " + rect(z, 20) + ")");
            check(fonts.seen.find("tnum=65535") != std::string::npos, "so is a font feature's value (got " + fonts.seen + ")");
            check(text.r == 0 && text.g == 0 && text.b == 0 && text.a == 255 && fill.r == 0 && fill.g == 0 && fill.b == 0 && fill.a == 255,
                  "a colour from a number that is none is black (" + std::to_string(text.r) + " " + std::to_string(text.g) + " " + std::to_string(text.b) + " " + std::to_string(text.a) + ", " +
                      std::to_string(fill.r) + " " + std::to_string(fill.g) + " " + std::to_string(fill.b) + " " + std::to_string(fill.a) + ")");
        }

        // (index - B) * A was worked out in int: for 65536n+65537 and the first child it wrapped to 0, a match.
        {
            Renderer n(300, 1, false, backend);
            widget(n, "div:nth-child(65536n+65537){width:50px} div:nth-last-child(65536n+65537){width:60px} div:nth-child(-2147483648n+2147483647){width:70px} div:nth-child(2n+1){margin-left:5px}",
                   div(10, 4, "height:1px") + div(11, 4, "height:1px") + div(12, 4, "height:1px"));
            check(rect(n, 10) == "5 295" && rect(n, 11) == "0 300" && rect(n, 12) == "5 295",
                  ":nth-child(An+B) with a product past int matches no more than it should (got " + rect(n, 10) + ", " + rect(n, 11) + ", " + rect(n, 12) + ")");
        }

        // A flex factor is kept in thousandths in an int: one past that was cast as it was, and two of them overflowed their sum.
        {
            Renderer x(300, 1, false, backend);
            widget(x, "", div(10, 4, "display:flex") + div(11, 10, "flex-grow:99999999999;height:5px") + div(12, 10, "flex-grow:99999999999;height:5px") + div(13, 10, "width:100px;height:5px") +
                              div(14, 4, "display:flex") + div(15, 14, "flex-shrink:99999999999;width:300px;height:5px") + div(16, 14, "flex-shrink:99999999999;width:300px;height:5px"));
            check(rect(x, 11) == "0 100" && rect(x, 12) == "100 100" && rect(x, 13) == "200 100", "two items with a huge flex-grow share the free space (got " + rect(x, 11) + ", " + rect(x, 12) + ", " + rect(x, 13) + ")");
            check(rect(x, 15) == "0 150" && rect(x, 16) == "150 150", "and two with a huge flex-shrink the overflow (got " + rect(x, 15) + ", " + rect(x, 16) + ")");
        }

        // A height worked out from an aspect ratio is a length like any other: 300 px by 1/1e30 was 3e32 px.
        {
            Renderer a(300, 1, false, backend);
            widget(a, "", div(10, 4, "aspect-ratio:1/1e30") + div(11, 4, "height:1px"));
            Json r = a.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"))[0]["result"];
            check(r["height"].asNumber() == 33554432, "a height from an aspect ratio stops at 2^25 px (got " + css::format_number(r["height"].asNumber(), 1) + ")");
        }

        // Each float after an inline box that wrapped doubled the markers that carry the box onto the next line: 2^24 of them here.
        for (int floats : {8, 24}) {
            Renderer w(300, 1, false, backend);
            std::string ops = R"(,{"op":"el","id":10,"tag":"p"},{"op":"ins","parent":4,"id":10,"before":null},{"op":"el","id":11,"tag":"span","attrs":[["style","background:#f00"]]},)"
                              R"({"op":"ins","parent":10,"id":11,"before":null},{"op":"text","id":12,"data":"word word word word word word word word word word word word"},{"op":"ins","parent":11,"id":12,"before":null})";
            for (int i = 0; i < floats; i++)
                ops += R"(,{"op":"el","id":)" + std::to_string(20 + i) + R"(,"tag":"i","attrs":[["style","float:left"]]},{"op":"ins","parent":10,"id":)" + std::to_string(20 + i) + R"(,"before":null})";
            auto started = std::chrono::steady_clock::now();
            widget(w, "", ops);
            int red = 0;
            for (const Command& c : w.paint())
                if (c.kind == Cmd::FillRect && c.color.r == 255 && c.color.g == 0) red++;
            double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - started).count();
            check(red == 2 && ms < 2000, std::to_string(floats) + " floats after a wrapped span leave it one box a line (" + std::to_string(red) + " boxes, " + std::to_string(int(ms)) + " ms)");
        }
    }

    // ── who gets a press: widget-sandbox's gesture policy ──
    {
        Renderer g(300, 1, false, backend);
        // none (30) over auto (31), 40 px in; a full-width none strip (32) for the leading edge.
        g.receive(parse(R"({"type":"ops","ops":[
            {"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":30,"tag":"div","attrs":[["style","margin-left:40px;width:100px;height:40px;touch-action:none"]]},
            {"op":"el","id":31,"tag":"div","attrs":[["style","margin-left:40px;width:100px;height:40px"]]},
            {"op":"el","id":32,"tag":"div","attrs":[["style","height:40px;touch-action:none"]]},
            {"op":"ins","parent":4,"id":30,"before":null},{"op":"ins","parent":4,"id":31,"before":null},{"op":"ins","parent":4,"id":32,"before":null}]})"));
        g.receive(parse(R"({"type":"listen","types":["contextmenu","dblclick","pointerup","pointercancel"]})"));
        auto claim_of = [](const std::vector<Json>& out) {
            for (const Json& m : out)
                if (m["type"].asString() == "gesture") return std::string(m["x"].asBool() ? "x" : "") + (m["y"].asBool() ? "y" : "") + (m["hold"].asBool() ? "h" : "");
            return std::string("none");
        };
        auto held = [&] {
            std::this_thread::sleep_for(std::chrono::milliseconds(520));
            bool due = g.wants_tick();
            auto ticked = g.tick();
            return std::make_pair(due, types(ticked));
        };

        auto down = g.touch(0, 60, 20, 0, true);
        check(claim_of(down) == "xh", "a press on touch-action: none claims horizontal drags and holds (got " + claim_of(down) + ")");
        check(g.next_tick_in() > 0.4 && g.next_tick_in() <= 0.5, "the hold deadline is the next tick");
        auto [due, ticked] = held();
        check(due && ticked == "contextmenu:30 ", "a finger held on none gets the widget's contextmenu (got " + ticked + ")");
        auto up = g.touch(2, 60, 20, 0, true);
        check(types(up) == "pointerup:30 ", "and its release is no click (got " + types(up) + ")");

        down = g.touch(0, 60, 60, 0, true);
        check(claim_of(down) == "", "a press on auto claims nothing");
        std::tie(due, ticked) = held();
        check(ticked.empty(), "a hold on auto is the bubble's: no contextmenu (got " + ticked + ")");
        up = g.touch(3, 60, 60, 0, true);
        check(types(up) == "pointercancel:31 ", "the client's menu cancels it");

        down = g.touch(0, 5, 100, 0, true);
        std::tie(due, ticked) = held();
        g.touch(3, 5, 100, 0, true);
        check(ticked.empty(), "the leading 20 px take no claim: no contextmenu on none (got " + ticked + ")");

        down = g.touch(0, 60, 60, 0, true, true, 0, 2);
        up = g.touch(2, 60, 60, 0, true, true, 0);
        check(types(down) + types(up) == "pointerup:31 ", "a right click on auto: no contextmenu, no click (got " + types(down) + types(up) + ")");
        down = g.touch(0, 60, 20, 0, true, true, 0, 2);
        up = g.touch(2, 60, 20, 0, true, true, 0);
        auto ctx = events(down);
        check(types(down) == "contextmenu:30 " && ctx[0]["button"].asInt() == 2 && types(up) == "pointerup:30 ", "a right click on none: the widget's contextmenu, no click (got " + types(down) + types(up) + ")");

        g.touch(0, 60, 60, 0, true);
        g.touch(2, 60, 60, 0, true);
        g.touch(0, 62, 61, 0, true);
        up = g.touch(2, 62, 61, 0, true);
        check(types(up) == "pointerup:31 click:31 dblclick:31 ", "a double tap on auto is the widget's: click, then dblclick (got " + types(up) + ")");

        // Playing the bubble, the renderer takes the presses the policy gives it.
        g.receive(parse(R"({"type":"arbitrate","bubble":true})"));
        g.touch(0, 60, 60, 0, true);
        std::this_thread::sleep_for(std::chrono::milliseconds(520));
        auto taken = g.tick();
        std::string action;
        for (const Json& m : taken)
            if (m["type"].asString() == "bubble") action = m["action"].asString() + "/" + m["gesture"].asString();
        up = g.touch(2, 60, 60, 0, true);
        check(types(taken) == "pointercancel:31 " && action == "menu/hold" && types(up).empty(), "as the bubble, a hold on auto ends with pointercancel and the menu, then nothing (got " + types(taken) + action + " " + types(up) + ")");
        g.touch(0, 5, 100, 0, true);
        auto moved = g.touch(1, 40, 100, 0, true);
        action.clear();
        for (const Json& m : moved)
            if (m["type"].asString() == "bubble") action = m["action"].asString() + (m["edge"].asBool() ? "/edge" : "");
        g.touch(2, 40, 100, 0, true);
        check(types(moved) == "pointercancel:32 " && action == "back/edge", "as the bubble, a drag from the leading edge is back (got " + types(moved) + action + ")");
        // A vertical drag is always the chat's, even on none and after the widget took the hold.
        g.touch(0, 60, 20, 0, true);
        std::this_thread::sleep_for(std::chrono::milliseconds(520));
        ticked = types(g.tick());
        moved = g.touch(1, 62, 60, 0, true);
        action.clear();
        for (const Json& m : moved)
            if (m["type"].asString() == "bubble") action = m["action"].asString();
        g.touch(2, 62, 60, 0, true);
        check(ticked == "contextmenu:30 " && types(moved) == "pointercancel:30 " && action == "scroll", "as the bubble, a vertical drag after a hold on none scrolls the chat (got " + ticked + types(moved) + action + ")");
    }

    // ── heavy boxes ──
    {
        // A body of boxes, one per style, and an optional style sheet.
        auto page = [](const std::vector<std::string>& styles, const std::string& css = "", const std::string& attrs = "", const char* tag = "div") {
            std::string ops = R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},)"
                              R"({"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]})";
            if (!css.empty())
                ops += R"(,{"op":"el","id":20,"tag":"style"},{"op":"css","id":20,"text":")" + css + R"("},{"op":"ins","parent":3,"id":20,"before":null})";
            for (size_t i = 0; i < styles.size(); i++) {
                std::string id = std::to_string(30 + i);
                ops += R"(,{"op":"el","id":)" + id + R"(,"tag":")" + tag + R"(","attrs":[["style","width:10px;height:10px;)" + styles[i] + "\"]" + attrs +
                       R"(]},{"op":"ins","parent":4,"id":)" + id + R"(,"before":null})";
            }
            return parse(ops + "]}");
        };
        auto run = [](Renderer& r, const Json& msg) {
            auto out = r.receive(msg);
            auto ticked = r.tick();
            out.insert(out.end(), ticked.begin(), ticked.end());
            std::string limit;
            for (const Json& m : out)
                if (m["type"].asString() == "limit") limit = m["code"].asString() + " " + std::to_string(m["count"].asInt());
            return limit;
        };
        std::vector<std::string> shadows(16, "box-shadow:0 2px 4px red");

        StubBackend hb;
        Renderer h(300, 1, false, hb);
        check(run(h, page(shadows)).empty(), "16 boxes with a blurred shadow run");
        StubBackend hb17;
        Renderer h17(300, 1, false, hb17);
        auto many = shadows;
        many.push_back("filter:brightness(1)");
        std::string stopped = run(h17, page(many));
        check(stopped == "paint-limit 17", "a 17th heavy box reports the limit (got '" + stopped + "')");

        StubBackend cb3;
        Renderer c(300, 1, false, cb3);
        c.receive(parse(R"({"type":"policy","policy":"compat"})"));
        check(run(c, page(many)).empty(), "under compat nothing is counted");

        StubBackend bb;
        Renderer b(300, 1, false, bb);
        std::vector<std::string> dimmed(20, "opacity:0.5");
        check(run(b, page(dimmed, "", R"(,["disabled",""])", "button")).empty(), "a disabled button's own opacity is not heavy");
        std::string more = R"({"type":"ops","ops":[)";
        for (int i = 0; i < 17; i++)
            more += std::string(i ? "," : "") + R"({"op":"el","id":)" + std::to_string(60 + i) + R"(,"tag":"div","attrs":[["style","opacity:0.5"]]},{"op":"ins","parent":4,"id":)" +
                    std::to_string(60 + i) + R"(,"before":null})";
        check(run(b, parse(more + "]}")) == "paint-limit 17", "while a box's opacity is");

        StubBackend db2;
        Renderer d(300, 1, false, db2);
        stopped = run(d, page({"backdrop-filter:blur(4px)", "backdrop-filter:blur(4px)", "-webkit-backdrop-filter:blur(4px)"}));
        check(stopped == "backdrop-limit 3", "a third backdrop-filter reports its own limit (got '" + stopped + "')");

        // Rule 5: an animation of opacity makes its box heavy while it runs, not during its delay.
        const char* fade = "@keyframes fade{from{opacity:1}to{opacity:1}} .late{animation:fade 1s linear 10s} .now{animation:fade 1s linear infinite}";
        StubBackend ab;
        Renderer a(300, 1, false, ab);
        auto waiting = shadows;
        waiting.push_back("");
        check(run(a, page(waiting, fade)).empty(), "a plain 17th box is not heavy");
        check(run(a, parse(R"({"type":"ops","ops":[{"op":"attr","id":46,"name":"class","value":"late"}]})")).empty(), "nor while its animation waits out the delay");
        stopped = run(a, parse(R"({"type":"ops","ops":[{"op":"attr","id":46,"name":"class","value":"now"}]})"));
        check(stopped == "paint-limit 17", "but it is once the animation runs (got '" + stopped + "')");

        StubBackend wb;
        Renderer w(300, 1, false, wb);
        check(run(w, page(waiting)).empty(), "a page of 16 heavy boxes runs");
        const char* held = R"({"type":"anim","now":0,"anims":[{"aid":1,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"fill":"forwards"},"startTime":-5000}]})";
        check(run(w, parse(held)).empty(), "a finished Web Animation that holds its end is not counted");
        const char* running = R"({"type":"anim","now":0,"anims":[{"aid":2,"target":46,"keyframes":[{"offset":0,"values":{"opacity":"1"}},{"offset":1,"values":{"opacity":"1"}}],"timing":{"duration":1000,"iterations":null},"startTime":0}]})";
        stopped = run(w, parse(running));
        check(stopped == "paint-limit 17", "a running one is (got '" + stopped + "')");

        // As the conformance set writes them: a class on the 17th box, after the page ran.
        auto restyle = [&](const char* css, const char* cls, const char* what) {
            StubBackend xb;
            Renderer x(300, 1, false, xb);
            std::string all = std::string(".h{box-shadow:0 2px 6px rgba(0,0,0,.4)} ") + css;
            check(run(x, page(shadows, all, R"(,["class","h"])")).empty() && run(x, page(waiting, all)).empty(), std::string(what) + ": runs before");
            stopped = run(x, parse(std::string(R"({"type":"ops","ops":[{"op":"attr","id":46,"name":"class","value":")") + cls + R"("}]})"));
            check(!stopped.empty(), std::string(what) + ": stops (got '" + stopped + "')");
        };
        restyle("@keyframes p{to{opacity:.55}} .a{animation:p 1.2s ease-in-out infinite alternate}", "a", "an animation with a to frame only");
        restyle(".p::before{content:\\\"\\\";display:block;width:6px;height:6px;filter:blur(1px)}", "p", "a ::before with a filter");
        restyle(".g{-webkit-backdrop-filter:blur(3px);backdrop-filter:blur(3px)}", "g", "a backdrop-filter");

        // The stopped widget: no blur, filters, backdrop or transparency, and a transparent box hidden.
        StubBackend sb;
        Renderer s(300, 1, false, sb);
        auto heavy = shadows;
        heavy[0] = "box-shadow:0 2px 4px red, 1px 1px 0 blue";
        heavy[1] = "filter:blur(2px)";
        heavy[2] = "backdrop-filter:blur(4px)";
        heavy[3] = "opacity:0.5";
        heavy[4] = "opacity:0";
        heavy.push_back("opacity:0.25");
        check(run(s, page(heavy)) == "paint-limit 17", "a mixed page stops");
        int blurred = 0, effects = 0, translucent = 0, hidden = 0, sharp = 0;
        for (const Command& cmd : s.paint()) {
            if (cmd.kind == Cmd::Shadow) (cmd.bx > 0 ? blurred : sharp)++;
            if (cmd.kind == Cmd::FilterPush || cmd.kind == Cmd::Backdrop) effects++;
            if (cmd.kind == Cmd::GroupPush && cmd.hidden) hidden++;
            else if (cmd.kind == Cmd::GroupPush && cmd.opacity < 1) translucent++;
        }
        check(!blurred && sharp == 1 && !effects && !translucent && hidden == 1,
              "the stopped paint keeps sharp shadows only, opaque, the transparent box hidden (blurred " + std::to_string(blurred) + ", sharp " + std::to_string(sharp) +
                  ", effects " + std::to_string(effects) + ", translucent " + std::to_string(translucent) + ", hidden " + std::to_string(hidden) + ")");
        check(!s.wants_tick() && run(s, page(heavy)).empty(), "and takes no more ops or frames");
    }

    // ── a long flush in parts ──
    {
        StubBackend pb;
        Renderer p(300, 1, false, pb);
        p.receive(parse(PAGE));
        std::string part = R"({"type":"ops","ops":[)";
        for (int i = 0; i < 1000; i++) part += std::string(i ? "," : "") + R"({"op":"style","id":10,"name":"width","value":")" + std::to_string(i % 90 + 1) + R"(px"})";
        int builds = p.stats().builds;
        p.receive(parse(part + "]}"));
        check(p.stats().builds == builds, "a part of 1000 ops waits for the rest of its task");
        p.receive(parse(R"({"type":"ops","ops":[{"op":"style","id":10,"name":"width","value":"70px"}]})"));
        check(p.stats().builds == builds && p.wants_tick(), "and so does the last, to the frame");
        p.tick();
        int built = p.stats().builds - builds;
        double width = p.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"))[0]["result"]["width"].asNumber();
        check(built == 1 && width == 70, "which builds the task once (" + std::to_string(built) + " builds)");
    }

    // ── a pointer move whose restyle runs the layout past its limit ──
    {
        StubBackend hb;
        Renderer h(300, 1, false, hb);
        std::string ops = R"j({"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]},{"op":"el","id":5,"tag":"style","attrs":[]},
            {"op":"css","id":5,"text":"body{margin:0}#t{width:100px;min-height:40px;transition:width 1s linear}#t:hover{width:200px}#b{display:none}#t:hover #b{display:block}.g{display:grid}"},
            {"op":"ins","parent":3,"id":5,"before":null},{"op":"el","id":10,"tag":"div","attrs":[["id","t"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":10,"id":11,"before":null})j";
        // Twenty nested grids, not displayed until #t is hovered.
        int parent = 11;
        for (int i = 0, id = 100; i < 20; i++, id += 3) {
            std::string g = std::to_string(id), c = std::to_string(id + 1), t = std::to_string(id + 2);
            ops += R"(,{"op":"el","id":)" + g + R"(,"tag":"div","attrs":[["class","g"]]},{"op":"el","id":)" + c + R"(,"tag":"div","attrs":[]},{"op":"text","id":)" + t +
                   R"(,"data":"x"},{"op":"ins","parent":)" + c + R"(,"id":)" + t + R"(,"before":null},{"op":"ins","parent":)" + g + R"(,"id":)" + c +
                   R"(,"before":null},{"op":"ins","parent":)" + std::to_string(parent) + R"(,"id":)" + g + R"(,"before":null})";
            parent = id;
        }
        h.receive(parse(R"({"type":"ops","ops":[)" + ops + "]}"));
        h.touch(0, 250, 300, 0, true, true);
        bool stopped = false;
        for (const Json& m : h.touch(1, 50, 20, 0, true, true))
            stopped = stopped || (m["type"].asString() == "violation" && m["detail"].asString().find("layout limit") != std::string::npos);
        check(stopped && events(h.touch(2, 50, 20, 0, true, true)).empty(), "a pointer move that runs the layout past its limit ends with the freeze: its touch is gone");
    }

    // ── selectable text, and text placed at 1e30 px ──
    {
        StubBackend sb;
        Renderer s(300, 1, false, sb);
        s.receive(parse(PAGE));
        s.receive(parse(R"j({"type":"ops","ops":[{"op":"el","id":64,"tag":"svg","ns":"svg","attrs":[["width","40"],["height","40"]]},{"op":"el","id":65,"tag":"text","ns":"svg","attrs":[["y","1e30"]]},
            {"op":"text","id":66,"data":"far"},{"op":"ins","parent":65,"id":66,"before":null},{"op":"ins","parent":64,"id":65,"before":null},{"op":"ins","parent":4,"id":64,"before":null},
            {"op":"el","id":60,"tag":"div","attrs":[["style","user-select:text"]]},{"op":"text","id":61,"data":"selectable"},{"op":"ins","parent":60,"id":61,"before":null},
            {"op":"el","id":62,"tag":"div","attrs":[["style","aspect-ratio:1/1e30"]]},{"op":"ins","parent":60,"id":62,"before":null},
            {"op":"text","id":63,"data":"below"},{"op":"ins","parent":60,"id":63,"before":null},{"op":"ins","parent":4,"id":60,"before":null}]})j"));
        DisplayList list = s.paint();
        bool far = std::any_of(list.begin(), list.end(), [](const Command& c) { return c.kind == Cmd::Text && c.text == "far" && c.rect.y > 1e29f; });
        check(far, "text drawn at 1e30 px does not hang the paint of a document with selectable text");
    }

    // ── canvas geometry a few commands multiply: Path2D.addPath, clip lists, a long path filled again and again ──
    {
        StubBackend cb;
        Renderer c(300, 1, false, cb);
        c.receive(parse(PAGE));
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":40,"tag":"canvas","attrs":[["width","100"],["height","100"]]},{"op":"ins","parent":4,"id":40,"before":null}]})"));
        auto draw = [&](const std::string& cmds) {
            cb.runs.clear();
            c.receive(parse(R"({"type":"ops","ops":[{"op":"canvas","id":40,"cmds":[["r",100,100],)" + cmds + "]}]}"));
        };
        std::string cmds = R"(["pn",1,"M0 0L10 10L20 0Z"],["pn",2,"M0 0L10 10L20 0Z"])";
        for (int i = 0; i < 14; i++) cmds += R"(,["pc",1,"addPath",{"p":2}],["pc",2,"addPath",{"p":1}])";
        draw(cmds + R"(,["c","fill",{"p":1}])");
        size_t longest = 0;
        for (const auto& ops : cb.runs)
            for (const CanvasOp& op : ops) longest = std::max(longest, op.path.points.size());
        check(longest > 0 && longest <= (size_t(1) << 20), "two Path2D objects added to each other stop growing (" + std::to_string(longest) + " floats)");

        cmds = R"(["c","beginPath"])";
        for (int i = 0; i < 500; i++) cmds += R"(,["c","lineTo",)" + std::to_string(i % 100) + "," + std::to_string(i * 7 % 100) + "]";
        for (int i = 0; i < 300; i++) cmds += R"(,["c","clip"],["c","fillRect",0,0,1,1])";
        draw(cmds);
        size_t clipped = 0, fills = 0;
        for (const auto& ops : cb.runs)
            for (const CanvasOp& op : ops) {
                size_t held = 0;
                if (op.clips)
                    for (const CanvasClip& clip : *op.clips) held += clip.path.points.size();
                clipped = std::max(clipped, held);
                fills += op.kind == CanvasOp::Fill;
            }
        check(fills == 300 && clipped <= (size_t(1) << 14), "a clip list of long paths stops growing (" + std::to_string(clipped) + " floats in one)");

        std::string d = "M0 0";
        for (int i = 0; i < 50000; i++) d += " " + std::to_string(i % 100) + " " + std::to_string(i * 7 % 100);
        cmds = R"(["pn",3,")" + d + R"("])";
        for (int i = 0; i < 400; i++) cmds += R"(,["c","fill",{"p":3}])";
        draw(cmds);
        size_t drawn = 0;
        for (const auto& ops : cb.runs)
            for (const CanvasOp& op : ops) drawn += op.path.points.size();
        check(drawn >= 100000 && drawn <= (size_t(1) << 24), "one canvas op hands the backend a bounded amount of geometry (" + std::to_string(drawn) + " floats)");
    }

    // ── trees deeper than a document may be ──
    {
        auto chain = [](int levels) {
            std::string ops = R"({"op":"el","id":100000,"tag":"div","attrs":[]})";
            for (int i = 1; i < levels; i++) {
                std::string id = std::to_string(100000 + i);
                ops += R"(,{"op":"el","id":)" + id + R"(,"tag":"div","attrs":[]},{"op":"ins","parent":)" + id + R"(,"id":)" + std::to_string(100000 + i - 1) + R"(,"before":null})";
            }
            return ops;
        };
        const int levels = 100000;
        const std::string top = std::to_string(100000 + levels - 1);
        StubBackend db;
        Renderer d(300, 1, false, db);
        d.receive(parse(PAGE));
        d.receive(parse(R"({"type":"ops","ops":[)" + chain(levels) + R"(,{"op":"ins","parent":4,"id":)" + top + R"(,"before":null}]})"));
        size_t depth = 0, deepest = 0;
        const std::string& html = d.last_html();
        for (size_t at = html.find('<'); at != std::string::npos; at = html.find('<', at + 1)) {
            if (html.compare(at, 4, "<div") == 0) deepest = std::max(deepest, ++depth);
            else if (html.compare(at, 5, "</div") == 0) depth--;
        }
        check(deepest > 0 && deepest <= 1024, "a chain of 100000 nested elements is not built past 1024 levels (" + std::to_string(deepest) + ")");
        StubBackend fb;
        Renderer f(300, 1, false, fb);
        f.receive(parse(PAGE));
        f.receive(parse(R"({"type":"ops","ops":[)" + chain(levels) + R"(,{"op":"rm","id":)" + top + "}]}"));
        check(f.last_html().find("id=\"a\"") != std::string::npos, "and a detached one is removed without recursing through it");
    }

    // ── canvases drawn from one another down a long chain ──
    {
        StubBackend cb;
        Renderer c(300, 1, false, cb);
        c.receive(parse(PAGE));
        std::string ops;
        for (int i = 0; i < 200000; i++)
            ops += std::string(i ? "," : "") + R"({"op":"canvas","id":)" + std::to_string(1000 + i) + R"(,"cmds":[["c","drawImage",{"n":)" + std::to_string(1001 + i) + "},0,0]]}";
        c.receive(parse(R"({"type":"ops","ops":[)" + ops + "]}"));
        check(c.last_html().find("id=\"a\"") != std::string::npos, "a batch of 200000 canvas ops, each drawing the next one's canvas, does not recurse through them all");
    }

    // ── an adopt op that names a live node's id ──
    {
        StubBackend ab;
        Renderer a(300, 1, false, ab);
        a.receive(parse(PAGE));
        a.receive(parse(R"({"type":"ops","ops":[{"op":"text","id":30,"data":"kept"},{"op":"ins","parent":11,"id":30,"before":null}]})"));
        a.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":11,"tag":"head","adopt":"head","attrs":[]}]})"));
        check(a.last_html().find("kept") != std::string::npos, "an adopt op naming a live node's id is ignored: the node stays in the tree");
        a.receive(parse(R"({"type":"ops","ops":[{"op":"rm","id":10}]})"));
        check(a.last_html().find("kept") == std::string::npos && a.last_html().find("id=\"b\"") != std::string::npos, "and its parent is removed with it afterwards");
    }

    // ── a textarea whose value replaced its children, then removed ──
    {
        StubBackend tb;
        Renderer t(300, 1, false, tb);
        t.receive(parse(PAGE));
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":50,"tag":"textarea","attrs":[]},{"op":"text","id":51,"data":"old"},{"op":"ins","parent":50,"id":51,"before":null},
            {"op":"ins","parent":4,"id":50,"before":null},{"op":"prop","id":50,"name":"value","value":"new"},{"op":"rm","id":50}]})"));
        t.receive(parse(R"({"type":"ops","ops":[{"op":"ins","parent":4,"id":51,"before":null}]})"));
        check(t.last_html().find("old") != std::string::npos && t.last_html().find("<textarea") == std::string::npos,
              "the text a textarea's value replaced does not point at the textarea once that is removed");
    }

    // ── nesting: a document is built 128 levels deep, which is what its walks cost in stack ──
    {
        StubBackend nb;
        Renderer n(300, 1, false, nb);
        std::string ops = R"({"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},{"op":"el","id":4,"tag":"body","adopt":"body","attrs":[]})";
        for (int i = 0; i < 300; i++) {
            std::string id = std::to_string(1000 + i), parent = std::to_string(i ? 999 + i : 4);
            ops += R"(,{"op":"el","id":)" + id + R"(,"tag":"div","attrs":[]},{"op":"ins","parent":)" + parent + R"(,"id":)" + id + R"(,"before":null})";
            if (i % 100 == 99) ops += R"(,{"op":"text","id":)" + std::to_string(2000 + i) + R"(,"data":"at)" + std::to_string(i + 1) + R"("},{"op":"ins","parent":)" + id + R"(,"id":)" + std::to_string(2000 + i) + R"(,"before":null})";
        }
        n.receive(parse(R"({"type":"ops","ops":[)" + ops + "]}"));
        std::string texts;
        for (const Command& c : n.paint())
            if (c.kind == Cmd::Text) texts += c.text + " ";
        size_t depth = 0, deepest = 0;
        const std::string& html = n.last_html();
        for (size_t at = html.find('<'); at != std::string::npos; at = html.find('<', at + 1)) {
            if (html.compare(at, 4, "<div") == 0) deepest = std::max(deepest, ++depth);
            else if (html.compare(at, 5, "</div") == 0) depth--;
        }
        check(texts == "at100 " && deepest == 126, "300 nested elements are written and laid out to the 128th level, <html> the first (" + texts + ", " + std::to_string(deepest) + " divs deep)");
    }

    // ── a float beside a line that a clearing box begins ──
    {
        Renderer c(300, 1, false, backend);
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:100px;font:10px/20px sans-serif"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"span","attrs":[["style","display:inline-block;clear:both"]]},{"op":"ins","parent":10,"id":11,"before":null},
            {"op":"text","id":12,"data":"x"},{"op":"ins","parent":11,"id":12,"before":null},
            {"op":"text","id":13,"data":" aaaaaaaaaa"},{"op":"ins","parent":10,"id":13,"before":null},
            {"op":"el","id":14,"tag":"span","attrs":[["style","float:left;width:60px;height:10px"]]},{"op":"ins","parent":10,"id":14,"before":null}]})"));
        c.paint();
        Json box = c.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"))[0]["result"];
        check(box["height"].asNumber() == 40, "the word a float leaves no room for goes to the next line (height " + css::format_number(box["height"].asNumber(), 1) + ")");
    }

    // ── text-overflow: ellipsis: a long line is cut with a bounded number of measurements ──
    {
        struct CountingBackend : StubBackend {
            size_t measured = 0, bytes = 0;
            float text_width(int f, const std::string& text) override {
                measured++;
                bytes += text.size();
                return StubBackend::text_width(f, text);
            }
        } eb;
        Renderer e(300, 1, false, eb);
        e.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"div","attrs":[["style","width:100000px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"text","id":11,"data":")" + std::string(20000, 'a') + R"("},{"op":"ins","parent":10,"id":11,"before":null}]})"));
        DisplayList list = e.paint();
        size_t shown = 0;
        float mark = 0;
        for (const Command& c : list) {
            if (c.kind == Cmd::Text && c.text == "\xE2\x80\xA6") mark = c.x;
            else if (c.kind == Cmd::Text && !c.text.empty() && c.text[0] == 'a') shown = c.text.size();
        }
        check(shown == 12497 && mark == 99976 && eb.measured <= 64 && eb.bytes <= 1000000,
              "a 20,000-character line is cut where the mark fits, by search (" + std::to_string(shown) + " shown, mark at " + std::to_string(int(mark)) + ", " +
                  std::to_string(eb.measured) + " measured, " + std::to_string(eb.bytes) + " bytes)");
    }

    // ── auto margins on the main axis take a flex line's free space in one step ──
    {
        StubBackend fb;
        auto flex = [&](const std::string& style) {
            Renderer f(300, 1, false, fb);
            f.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"div","attrs":[["style",")" + style + R"("]]},{"op":"ins","parent":4,"id":10,"before":null},
                {"op":"el","id":11,"tag":"div","attrs":[["style","width:50px;height:10px"]]},{"op":"ins","parent":10,"id":11,"before":null},
                {"op":"el","id":12,"tag":"div","attrs":[["style","width:50px;height:10px;margin-left:auto"]]},{"op":"ins","parent":10,"id":12,"before":null}]})"));
            std::string out;
            for (int id : {11, 12})
                out += css::format_number(f.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"]["x"].asNumber(), 1) + " ";
            return out;
        };
        std::string t = flex("display:flex");
        check(t == "0 250 ", "margin-left: auto pushes a flex item to the end of its line (got " + t + ")");
        t = flex("display:flex;justify-content:center");
        check(t == "0 250 ", "and leaves justify-content no free space (got " + t + ")");
        t = flex("display:flex;width:30000000px");
        check(t == "0 29999950 ", "in a line 30000000px wide too, where a pixel is less than a float's step (got " + t + ")");
    }

    // ── SVG <use>: its href names an id, never a selector ──
    {
        auto paths = [](const DisplayList& d) {
            return std::count_if(d.begin(), d.end(), [](const Command& c) { return c.kind == Cmd::Path; });
        };
        auto page = [](const std::string& href) {
            return parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html"},{"op":"el","id":3,"tag":"head","adopt":"head"},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":10,"tag":"svg","ns":"svg","attrs":[["width","100"],["height","100"]]},{"op":"ins","parent":4,"id":10,"before":null},
                {"op":"el","id":11,"tag":"defs","ns":"svg"},{"op":"ins","parent":10,"id":11,"before":null},
                {"op":"el","id":12,"tag":"rect","ns":"svg","attrs":[["id","a"],["width","10"],["height","10"]]},{"op":"ins","parent":11,"id":12,"before":null},
                {"op":"el","id":14,"tag":"rect","ns":"svg","attrs":[["width","20"],["height","20"]]},{"op":"ins","parent":11,"id":14,"before":null},
                {"op":"el","id":13,"tag":"use","ns":"svg","attrs":[["href",")" + href + R"("]]},{"op":"ins","parent":10,"id":13,"before":null}]})");
        };
        StubBackend ub;
        Renderer named(300, 1, false, ub);
        named.receive(page("#a"));
        check(paths(named.paint()) == 1, "a <use> draws the element its href names");
        Renderer listed(300, 1, false, ub);
        listed.receive(page("#a+rect"));
        check(paths(listed.paint()) == 0, "a <use> whose href is a selector draws nothing");
        Renderer nested(300, 1, false, ub);
        nested.receive(page("#a" + std::string(8189, '[')));
        check(paths(nested.paint()) == 0, "a <use> whose href nests 8189 blocks draws nothing");
    }

    // ── elements that are not tags ──
    {
        StubBackend b;
        Renderer t(300, 1, false, b);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":20,"tag":"style","attrs":[["id","s"]]},{"op":"css","id":20,"text":".x{color:red}"},{"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["class","x"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"text","id":11,"data":"anchor"},{"op":"ins","parent":10,"id":11,"before":null}]})"));
        t.tick();
        auto ask = [&](Json query) {
            for (const Json& m : t.receive(Json::object().set("type", "query").set("qid", 1).set("query", std::move(query))))
                if (m["type"].asString() == "result") return m["result"];
            return Json();
        };
        auto cs = [&](int id, const char* prop) { return ask(Json::object().set("q", "cs").set("id", id))[prop].asString(); };
        auto style = [&](const char* op, const char* name, const std::string& value) {
            auto ops = Json::array().push(Json::object().set("op", op).set("id", 20).set("name", name).set("value", value));
            t.receive(Json::object().set("type", "ops").set("ops", ops));
            t.tick();
        };
        // Each first write puts the attribute there, each later one goes through the property patch.
        style("attr", "style", "transform:translateX(1px)");
        for (const auto value : {"translateX(2px)", "translateX(3px)", "none"}) style("style", "transform", value);
        for (const auto value : {"0.5", "0.25", "1"}) style("style", "opacity", value);
        for (const auto value : {"red", "blue"}) style("style", "background-color", value);
        for (const auto value : {"10px", "20px"}) style("style", "width", value);
        for (const auto value : {"a", "b", ""}) style("attr", "class", value);
        check(cs(10, "color") == "rgb(255, 0, 0)" && t.last_html().find("width:20px") != std::string::npos,
              "inline styles and classes on a <style> element change nothing but its attributes (got " + cs(10, "color") + ")");

        t.receive(parse(R"({"type":"ops","ops":[{"op":"css","id":20,"text":":root{--d:inline-text} .t{display:var(--d)} .u{display:inline-text} .w{padding-left:300px}"},
            {"op":"el","id":30,"tag":"div","attrs":[]},{"op":"ins","parent":4,"id":30,"before":null},
            {"op":"el","id":31,"tag":"span","attrs":[["class","t"]]},{"op":"ins","parent":30,"id":31,"before":null},
            {"op":"text","id":32,"data":"words"},{"op":"ins","parent":31,"id":32,"before":null},
            {"op":"el","id":33,"tag":"span","attrs":[["class","u"]]},{"op":"ins","parent":30,"id":33,"before":null},
            {"op":"text","id":34,"data":"more"},{"op":"ins","parent":33,"id":34,"before":null},
            {"op":"el","id":40,"tag":"div","attrs":[["style","width:100px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"]]},{"op":"ins","parent":4,"id":40,"before":null},
            {"op":"el","id":41,"tag":"span","attrs":[["class","t w x"]]},{"op":"ins","parent":40,"id":41,"before":null},
            {"op":"el","id":42,"tag":"img","attrs":[["class","u w"],["width","300"],["height","10"]]},{"op":"ins","parent":40,"id":42,"before":null}]})"));
        t.tick();
        auto width = [&](int id) { return ask(Json::object().set("q", "rect").set("id", id))["width"].asNumber(); };
        check(width(31) == 40 && width(33) == 32 && width(41) == 300,
              "a tag that declares the display of a text node, by a variable or by name, is laid out as an inline box (got " +
                  std::to_string(width(31)) + ", " + std::to_string(width(33)) + ", " + std::to_string(width(41)) + ")");

        t.receive(parse(R"({"type":"ops","ops":[{"op":"css","id":20,"text":":root{--w:0-lh-calc} .c{width:calc(50% - 7px)} .v{width:var(--w)} .n{width:1-lh-calc}"},
            {"op":"el","id":50,"tag":"div","attrs":[["class","c"]]},{"op":"ins","parent":4,"id":50,"before":null},
            {"op":"el","id":51,"tag":"div","attrs":[["class","v"]]},{"op":"ins","parent":4,"id":51,"before":null},
            {"op":"el","id":52,"tag":"div","attrs":[["class","n"]]},{"op":"ins","parent":4,"id":52,"before":null}]})"));
        t.tick();
        check(width(50) == 143 && width(51) == 300 && width(52) == 300,
              "a length cannot name the unit that marks a calc expression and take another's value (got " +
                  std::to_string(width(50)) + ", " + std::to_string(width(51)) + ", " + std::to_string(width(52)) + ")");
    }

    // ── calc(): a product of many factors is one scaled node, not a chain as deep as the product ──
    {
        double chain = 0, mixed = 0;
        std::thread worker([&] {
            StubBackend cb;
            Renderer c(300, 1, false, cb);
            std::string factors;
            for (int i = 0; i < 15000; i++) factors += "*1";
            c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
                {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
                {"op":"el","id":20,"tag":"style","attrs":[]},{"op":"css","id":20,"text":"#a{width:calc((50% + 10px))" + factors + R"()}#b{width:calc(2*(25% + 5px)*3/4*2)}"},
                {"op":"ins","parent":3,"id":20,"before":null},
                {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"ins","parent":4,"id":10,"before":null},
                {"op":"el","id":11,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":4,"id":11,"before":null}]})"));
            c.tick();
            chain = c.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})"))[0]["result"]["width"].asNumber();
            mixed = c.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":11}})"))[0]["result"]["width"].asNumber();
        });
        worker.join();
        check(chain == 160 && mixed == 240, "a calc() product of 15000 factors is laid out on a thread's stack (" + std::to_string(chain) + ", " + std::to_string(mixed) + ")");
    }


    // ── a text that splits into no words leaves its hole without elements ──
    {
        StubBackend b;
        Renderer t(300, 1, false, b);
        t.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":10,"tag":"span","attrs":[]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"text","id":11,"data":"abc"},{"op":"ins","parent":10,"id":11,"before":null}]})"));
        t.tick();
        auto data = [&](const std::string& text) {
            auto ops = Json::array().push(Json::object().set("op", "data").set("id", 11).set("data", text));
            t.receive(Json::object().set("type", "ops").set("ops", ops));
            t.tick();
        };
        auto width = [&] {
            for (const Json& m : t.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":10}})")))
                if (m["type"].asString() == "result") return m["result"]["width"].asNumber();
            return -1.0;
        };
        // An overlong NUL ends the text where it stands: no word, no space, a hole with no element.
        data("\xE0\x80\x80");
        data("defg");
        check(width() == 32, "a text set after one that split into no words is laid out (got " + std::to_string(width()) + ")");
        data("hi");
        check(width() == 16, "and patched in place again (got " + std::to_string(width()) + ")");
    }

    // ── grid: a track list holds at most the 1000 tracks that one repeat() may count to ──
    {
        StubBackend gb;
        Renderer g(300, 1, false, gb);
        std::string sizes;
        for (int i = 0; i < 7900; i++) sizes += " 1px";
        g.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},
            {"op":"el","id":20,"tag":"style","attrs":[]},{"op":"css","id":20,"text":"div{display:grid}#a{grid-template-rows:repeat(1000,1px 1px 1px)}#b{grid-template-rows:repeat(1000,)" + sizes + R"()}#c{grid-template-rows:repeat(600,1px) repeat(600,2px) 5px 5px}#d{grid-template-columns:repeat(auto-fill,)" + sizes + R"()}"},
            {"op":"ins","parent":3,"id":20,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":4,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["id","c"]]},{"op":"ins","parent":4,"id":12,"before":null},
            {"op":"el","id":13,"tag":"div","attrs":[["id","d"]]},{"op":"ins","parent":4,"id":13,"before":null}]})"));
        g.tick();
        auto height = [&](int id) {
            return g.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"]["height"].asNumber();
        };
        check(height(10) == 999 && height(11) == 0 && height(12) == 1400 && height(13) == 0,
              "a grid's track list stops at 1000 tracks (" + std::to_string(height(10)) + ", " + std::to_string(height(11)) + ", " + std::to_string(height(12)) + ", " + std::to_string(height(13)) + ")");
    }

    // ── blocks and functions nested deeper than the CSS parser keeps ──
    {
        StubBackend nb;
        Renderer n(300, 1, false, nb);
        std::string nested;
        for (int i = 0; i < 150; i++) nested += ":is(";
        nested += "#a" + std::string(150, ')');
        const std::string deeper = std::string(12000, '(') + std::string(12000, ')');
        n.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":2,"tag":"html","adopt":"html","attrs":[]},{"op":"el","id":3,"tag":"head","adopt":"head","attrs":[]},
            {"op":"el","id":4,"tag":"body","adopt":"body","attrs":[["style","margin:0"]]},{"op":"el","id":5,"tag":"style","attrs":[]},
            {"op":"css","id":5,"text":"div{width:10px})" + nested + R"({width:20px}#b{width:calc)" + deeper + R"(}#c:not)" + deeper + R"({width:40px}#c{width:30px}"},
            {"op":"ins","parent":3,"id":5,"before":null},
            {"op":"el","id":10,"tag":"div","attrs":[["id","a"]]},{"op":"ins","parent":4,"id":10,"before":null},
            {"op":"el","id":11,"tag":"div","attrs":[["id","b"]]},{"op":"ins","parent":4,"id":11,"before":null},
            {"op":"el","id":12,"tag":"div","attrs":[["id","c"]]},{"op":"ins","parent":4,"id":12,"before":null}]})"));
        n.tick();
        auto width = [&](int id) {
            return n.receive(parse(R"({"type":"query","qid":1,"query":{"q":"rect","id":)" + std::to_string(id) + "}}"))[0]["result"]["width"].asNumber();
        };
        check(width(10) == 10 && width(11) == 10 && width(12) == 30,
              "a selector or a value nested deeper than the CSS parser keeps is left out, and the rules after it still apply (got " +
                  std::to_string(width(10)) + ", " + std::to_string(width(11)) + ", " + std::to_string(width(12)) + ")");
    }

    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
