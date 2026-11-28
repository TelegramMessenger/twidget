#pragma once

#include "loopback.h"
#include "remote.h"
#include "renderer_end.h"
#include "twidget/json.h"
#include "wire_compare.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <string>
#include <vector>

namespace {

std::string dumps(const std::vector<twidget::Json>& messages) {
    std::string out;
    for (const twidget::Json& m : messages) out += m.dump() + "\n";
    return out;
}

bool platform_type(const twidget::Json& message) {
    const std::string& type = message["type"].asString();
    return type == "size" || type == "gesture" || type == "limit" || type == "violation" || type == "bubble";
}

std::vector<twidget::Json> platform_only(const std::vector<twidget::Json>& messages) {
    std::vector<twidget::Json> out;
    for (const twidget::Json& m : messages)
        if (platform_type(m)) out.push_back(m);
    return out;
}

uint64_t bits64(const double& d) {
    uint64_t b = 0;
    std::memcpy(&b, &d, sizeof b);
    return b;
}

bool same_state(const remote::State& a, const remote::State& b) {
    return bits64(a.width) == bits64(b.width) && a.height == b.height && a.dirty == b.dirty && a.wants_tick == b.wants_tick
        && bits64(a.next_tick_in) == bits64(b.next_tick_in) && a.frame_asked == b.frame_asked;
}

std::string report_name(remote::Kind kind) {
    switch (kind) {
    case remote::Kind::Frame: return "Frame";
    case remote::Kind::State: return "State";
    case remote::Kind::Messages: return "Messages";
    case remote::Kind::Pointer: return "Pointer";
    case remote::Kind::Selection: return "Selection";
    case remote::Kind::ClaimAnswer: return "ClaimAnswer";
    default: return "kind " + std::to_string(int(kind));
    }
}

std::string reports_name(const std::vector<remote::Kind>& kinds) {
    std::string out;
    for (remote::Kind kind : kinds) out += (out.empty() ? "" : " ") + report_name(kind);
    return out.empty() ? "nothing" : out;
}

std::optional<twidget::WidgetGestureClaim> gesture_of(const std::vector<twidget::Json>& messages) {
    for (const twidget::Json& m : messages)
        if (m["type"].asString() == "gesture") return twidget::WidgetGestureClaim{m["x"].asBool(), m["y"].asBool(), m["hold"].asBool()};
    return std::nullopt;
}

std::string claim_name(const std::optional<twidget::WidgetGestureClaim>& claim) {
    if (!claim) return "none";
    return std::string(claim->x ? "x" : "-") + (claim->y ? "y" : "-") + (claim->hold ? "h" : "-");
}

/// What the direct renderer gave for a step, and the reports the host must hear of it in order.
struct Expected {
    std::vector<twidget::Json> messages;
    bool engine = false;
    uint64_t seq = 0;
    std::optional<remote::Pointer> pointer;
    std::optional<remote::Frame> frame;
    std::optional<twidget::WidgetGestureClaim> claim;
    std::string selection;
    remote::State state;
    std::vector<remote::Kind> events;
};

void predict(Expected& s, const std::string& selection, const remote::State& sent) {
    if (!platform_only(s.messages).empty()) s.events.push_back(remote::Kind::Messages);
    if (s.pointer) s.events.push_back(remote::Kind::Pointer);
    if (s.selection != selection) s.events.push_back(remote::Kind::Selection);
    if (s.frame) {
        s.frame->state = s.state;
        s.events.push_back(remote::Kind::Frame);
    } else {
        if (s.claim) s.events.push_back(remote::Kind::ClaimAnswer);
        if (!same_state(sent, s.state)) s.events.push_back(remote::Kind::State);
    }
}

template <class Ok> void healthy(Ok&& ok, const Loopback& loop) {
    const bool stopped = loop.end->stopped();
    ok(!stopped && loop.host.refusals.empty() && loop.wire.refused == 0 && !loop.platform.refusing() && loop.queue.messages.empty(),
       "both ends stay healthy" + (stopped ? ": " + loop.end->stop_reason() : std::string()) + (loop.host.refusals.empty() ? std::string() : ": " + loop.host.refusals[0]));
}

template <class Ok>
void compare_reports(Ok&& ok, const Expected& s, const Loopback& loop, size_t heard_before, const std::vector<twidget::Json>& returned, const remote::RendererEnd::Output* out) {
    std::string why;
    ok(dumps(returned) == dumps(s.messages), "the helper gets every message the direct renderer gave");
    if (out) ok(out->frame == s.engine, std::string("the engine's frame is ") + (s.engine ? "due" : "not due"));
    const std::vector<remote::Kind> heard(loop.host.events.begin() + long(heard_before), loop.host.events.end());
    ok(heard == s.events, "the host hears " + reports_name(s.events) + ", not " + reports_name(heard));
    if (heard == s.events) {
        for (remote::Kind kind : heard) {
            if (kind == remote::Kind::Messages) {
                ok(dumps(loop.host.batches.back()) == dumps(platform_only(s.messages)), "the Messages report holds the call's platform messages alone");
            } else if (kind == remote::Kind::Pointer) {
                const remote::Pointer& p = loop.host.pointers.back();
                ok(p.cursor == s.pointer->cursor && p.claim == s.pointer->claim,
                   "the Pointer report is " + s.pointer->cursor + " and " + claim_name(s.pointer->claim) + ", not " + p.cursor + " and " + claim_name(p.claim));
            } else if (kind == remote::Kind::Selection) {
                ok(loop.host.selections.back().text == s.selection, "the Selection report is the direct selected text '" + s.selection + "'");
            } else if (kind == remote::Kind::Frame) {
                const remote::Frame& f = loop.host.frames.back();
                ok(f.seq == s.seq && f.painted == s.frame->painted, "the Frame answers request " + std::to_string(s.seq) + (s.frame->painted ? ", painted" : ", unpainted"));
                const bool same_list = same(s.frame->list, f.list, why);
                ok(same_list, "the frame's list is the direct paint " + why);
                ok(sharing(s.frame->list) == sharing(f.list), "the frame's list shares its payloads as the direct paint does");
                const bool same_damage = same(s.frame->damage, f.damage, why);
                ok(same_damage, "the frame's damage is the direct damage " + why);
                ok(same_state(f.state, s.state), "the Frame carries the direct state");
            } else if (kind == remote::Kind::ClaimAnswer) {
                const remote::ClaimAnswer& c = loop.host.claims.back();
                ok(c.seq == s.seq && c.claim == *s.claim, "the ClaimAnswer is " + claim_name(s.claim) + ", not " + claim_name(c.claim));
            } else if (kind == remote::Kind::State) {
                ok(same_state(loop.host.states.back(), s.state), "the State report is the direct state");
            }
        }
    }
    ok(same_state(loop.platform.state(), s.state), "the platform end's state is the direct renderer's");
}

} // namespace
