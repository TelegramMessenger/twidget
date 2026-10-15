// Drives TwidgetCore without a platform: ops in, events, display lists and layers out, against a
// stub backend that measures text as 8 px per byte.
#include "renderer.h"

#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <thread>

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
    int create_font(const std::string&, float size, int, bool, FontMetrics& out) override {
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
    check(dbl && detail == 2, "two quick clicks: click detail 2 and a dblclick");
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

    // ── wheel and keys ──
    r.wheel(150, 10, 0, 50, 0);
    r.wheel(150, 10, 0, 30, 0);
    out = r.tick();
    evs = events(out);
    check(evs.size() == 1 && evs[0]["deltaY"].asNumber() == 80 && evs[0]["targetId"].asNumber() == 12, "two wheels in a frame arrive as one with the sum");
    out = r.key("keydown", "a", "KeyA", false, 8);
    evs = events(out);
    check(evs.size() == 1 && evs[0]["targetId"].asNumber() == 4 && evs[0]["key"].asString() == "a" && evs[0]["modifiers"].asNumber() == 8, "keydown goes to the body with key, code and modifiers");

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

        // Something painted over the spinner's sweep: it goes back to per-frame paint.
        c.receive(parse(R"({"type":"ops","ops":[{"op":"el","id":42,"tag":"div","attrs":[["style","position:absolute;left:0;top:0;width:30px;height:30px;background:#000"]]},{"op":"ins","parent":4,"id":42,"before":null}]})"));
        c.paint();
        bool spinning = false;
        for (const Layer& l : c.composition().layers) spinning = spinning || l.id == 40;
        check(!spinning, "content painted over the sweep keeps the element in the document");
        check(c.wants_tick(), "which then needs frames again");

        // Width cannot be composited.
        c.receive(parse(R"({"type":"ops","ops":[{"op":"rm","id":42},{"op":"el","id":43,"tag":"div","attrs":[["id","w"]]},{"op":"ins","parent":4,"id":43,"before":null}]})"));
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
        check(types(got) == "gotpointercapture:11 ", "setPointerCapture on the pressed mouse: gotpointercapture (got " + types(got) + ")");
        c.touch(1, 150, 10, 0, true, true, 0);
        auto moved = c.tick();
        check(types(moved) == "pointermove:11 mousemove:12 ", "captured: pointermove to the capture, mousemove to the element under the mouse (got " + types(moved) + ")");
        auto up = c.touch(2, 150, 10, 0, true, true, 0);
        check(types(up) == "pointerup:11 mouseup:12 lostpointercapture:11 ", "release: pointerup to the capture, mouseup under the mouse, then the capture is lost (got " + types(up) + ")");
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
            {"op":"el","id":40,"tag":"div","attrs":[["class","s"],["style","margin:30px"]]},{"op":"ins","parent":31,"id":40,"before":null})J");
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
        // Past the layer cap the topmost spinners are composited and the rest painted.
        std::string many;
        for (int i = 0; i < 70; i++)
            many += R"J({"op":"el","id":)J" + std::to_string(100 + i) + R"J(,"tag":"div","attrs":[["class","s"],["style","display:inline-block;width:10px;height:10px;margin:10px"]]},{"op":"ins","parent":4,"id":)J" +
                    std::to_string(100 + i) + R"J(,"before":null},)J";
        many.pop_back();
        auto capped = layers_of(many.c_str());
        bool top = false, bottom = false;
        for (const Layer& layer : capped) {
            top = top || layer.id == 169;
            bottom = bottom || layer.id == 100;
        }
        check(capped.size() == 64 && top && !bottom, "past the cap, the topmost layers win (" + std::to_string(capped.size()) + " layers)");
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

    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
