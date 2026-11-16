// twidget_replay: a recording of renderers used directly, replayed through the renderer end and the platform end alone with the recorded Backend answers and clock reads, every step compared member by member with what the recorded renderer gave.
#include "platform_end.h"
#include "remote.h"
#include "renderer_end.h"
#include "string_id.h"

#include "log_backend.h"
#include "loopback.h"
#include "recording.h"
#include "reports.h"
#include "wire_compare.h"

#include <algorithm>
#include <cfloat>
#include <climits>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

using twidget::Json;

namespace {

const char* OP_NAMES[] = {"document fonts", "canvas fonts", "font releases", "widths", "images", "canvas runs", "pixel reads", "canvas releases"};

/// One step's recorded Backend answers, handed out in order past its Frame markers; a call of another kind, or one past them, gets zeros and is named.
struct Answers : Backend {
    void start(const std::vector<Call>& step) {
        calls = &step;
        next = taken = 0;
        mismatch.clear();
    }

    /// The recorded calls no replayed call has taken.
    size_t left() const {
        size_t n = 0;
        for (size_t i = next; calls && i < calls->size(); i++) n += (*calls)[i].op != Call::Frame;
        return n;
    }

    int create_font(const std::string&, float, int, bool, const std::string&, FontMetrics& out) override { return font(Call::Font, out); }
    int create_canvas_font(const std::string&, float, int, bool, FontMetrics& out) override { return font(Call::CanvasFont, out); }
    void delete_font(int) override { take(Call::DeleteFont); }
    float text_width(int, const std::string&) override {
        const Call* c = take(Call::TextWidth);
        return c ? c->width : 0;
    }
    bool load_image(int, const std::string&, float& w, float& h) override {
        const Call* c = take(Call::Image);
        if (!c) return false;
        w = c->image_w;
        h = c->image_h;
        return c->ok;
    }
    void canvas_run(int, int, int, const std::vector<CanvasOp>&) override { take(Call::CanvasRun); }
    bool canvas_read(int, int, int, int, int, std::vector<uint8_t>& rgba) override {
        const Call* c = take(Call::CanvasRead);
        if (!c) return false;
        rgba = c->rgba;
        return c->ok;
    }
    void canvas_release(int) override { take(Call::CanvasRelease); }

    const std::vector<Call>* calls = nullptr;
    size_t next = 0, taken = 0;
    std::string mismatch;

private:
    const Call* take(Call::Op op) {
        while (calls && next < calls->size() && (*calls)[next].op == Call::Frame) next++;
        const std::string at = "replayed Backend call " + std::to_string(taken++) + " (" + OP_NAMES[op] + ")";
        if (!calls || next == calls->size()) return miss(at + " is past the recorded ones");
        const Call& c = (*calls)[next++];
        if (c.op != op) return miss(at + " is where the recording has one of the " + OP_NAMES[c.op]);
        return &c;
    }
    const Call* miss(const std::string& why) {
        if (mismatch.empty()) mismatch = why;
        return nullptr;
    }
    int font(Call::Op op, FontMetrics& out) {
        const Call* c = take(op);
        if (!c) return 0;
        out = c->metrics;
        return c->font;
    }
};

/// One step's recorded clock reads, in µs and in order; a read past them repeats the session's last one and is marked.
struct ReplayClock : Clock {
    void start(const std::vector<uint64_t>& step) {
        reads = &step;
        next = 0;
        over = false;
    }

    double now_ms() override {
        if (reads && next < reads->size()) {
            last = (*reads)[next++];
        } else {
            over = true;
        }
        return double(last) / 1000;
    }

    const std::vector<uint64_t>* reads = nullptr;
    size_t next = 0;
    uint64_t last = 0;
    bool over = false;
};

/// One recorded renderer replayed: its answers and clock, then the two ends over the in-memory channel, and what the host was last told.
struct Session {
    explicit Session(const remote::Caps& caps) : loop(caps) { loop.backend.answers = &answers; }

    Answers answers;
    ReplayClock clock;
    Loopback loop;
    std::string label, selection;
    remote::State sent;
    /// Which recorded and replayed SVG paint keys stand for each other, both ways: litehtml derives those keys from heap addresses.
    std::map<uint64_t, uint64_t> keys_to, keys_back;
    uint64_t first_key = 0;
};

/// What the replay compared, over a whole source or one labelled session.
struct Counts {
    uint64_t sessions = 0, steps = 0, bytes = 0, lists = 0, unchanged = 0, ticks = 0, engine = 0, calls = 0, pointers = 0, claims = 0, claiming = 0, holds = 0,
             holding = 0, selections = 0, states = 0, messages = 0, resizes = 0, themes = 0, violations = 0, clock = 0, keyed = 0, keys = 0;
    uint64_t ops[Call::Frame] = {};

    void add(const record::Step& step, size_t frame_bytes, const Loopback& loop, const std::vector<Json>& returned, size_t keyed_commands, size_t new_keys) {
        steps++;
        bytes += frame_bytes;
        keyed += keyed_commands;
        keys += new_keys;
        sessions += step.kind == record::Kind::Construct;
        ticks += step.kind == record::Kind::Frame && step.numbers[0] != 0;
        engine += step.engine;
        resizes += step.kind == record::Kind::SetWidth || step.kind == record::Kind::SetMaxHeight;
        themes += step.kind == record::Kind::SetTheme;
        clock += step.clock.size();
        for (const Call& c : loop.backend.calls) {
            if (c.op == Call::Frame) continue;
            calls++;
            ops[c.op]++;
        }
        for (const remote::Frame& f : loop.host.frames) (f.painted ? lists : unchanged)++;
        for (const remote::Pointer& p : loop.host.pointers) {
            pointers++;
            claims += p.claim.has_value();
            claiming += p.claim && (p.claim->x || p.claim->y || p.claim->hold);
        }
        for (const remote::ClaimAnswer& c : loop.host.claims) {
            holds++;
            holding += c.claim.x || c.claim.y || c.claim.hold;
        }
        selections += loop.host.selections.size();
        states += loop.host.states.size();
        messages += loop.host.batches.size();
        for (const Json& m : returned) violations += m["type"].asString() == "violation";
    }

    std::string text() const {
        const auto n = [](uint64_t v) { return std::to_string(v); };
        std::string out = n(sessions) + " sessions, " + n(steps) + " steps, " + n(bytes) + " recorded bytes; frames " + n(lists) + " painted, " + n(unchanged) + " unchanged, " + n(ticks)
            + " ticks, " + n(engine) + " engine frames; Backend " + n(calls) + " calls (";
        for (int op = 0; op < Call::Frame; op++) out += (op ? ", " : "") + n(ops[op]) + " " + OP_NAMES[op];
        return out + "); host " + n(pointers) + " Pointer (" + n(claims) + " with a claim, " + n(claiming) + " claiming), " + n(holds) + " ClaimAnswer (" + n(holding) + " claiming), "
            + n(selections) + " Selection, " + n(states) + " State, " + n(messages) + " Messages; " + n(keyed) + " keyed commands with " + n(keys)
            + " distinct keys compared by correspondence; " + n(resizes) + " resizes, " + n(themes) + " theme changes, " + n(violations) + " violations, " + n(clock)
            + " clock reads";
    }

    /// The count `--needs` names, nothing for a name it does not know.
    std::optional<uint64_t> need(const std::string& name) const {
        if (name == "sessions") return sessions;
        if (name == "steps") return steps;
        if (name == "lists") return lists;
        if (name == "batches") return ops[Call::CanvasRun];
        if (name == "reads") return ops[Call::CanvasRead];
        if (name == "calls") return calls;
        if (name == "pointers") return pointers;
        if (name == "claims") return claims;
        if (name == "holds") return holds;
        if (name == "selections") return selections;
        if (name == "ticks") return ticks;
        if (name == "engine") return engine;
        if (name == "resizes") return resizes;
        if (name == "themes") return themes;
        if (name == "images") return ops[Call::Image];
        if (name == "keys") return keyed;
        return std::nullopt;
    }
};

enum class Perturb { None, List, Call, Key };

const char* perturb_name(Perturb perturb) {
    return perturb == Perturb::List ? "list" : perturb == Perturb::Call ? "call" : "key";
}

/// Where `--perturb key` changes a replayed key: the step, from 1, and the command of its painted list; a merge or a repeated key.
struct KeyTarget {
    uint64_t step = 0;
    size_t command = 0;
    bool merge = false;
};

/// The first keyed command whose recorded key its session has painted before, else the first that brings a session's second distinct key.
std::optional<KeyTarget> key_target(const std::string& path) {
    record::Reader reader;
    if (!reader.open(path)) return std::nullopt;
    std::map<uint32_t, std::set<uint64_t>> seen;
    std::optional<KeyTarget> merge;
    record::Step step;
    size_t bytes = 0;
    for (uint64_t index = 1; reader.next(step, bytes); index++) {
        if (step.kind != record::Kind::Frame || !step.painted) continue;
        std::set<uint64_t>& keys = seen[step.session];
        for (size_t i = 0; i < step.list.size(); i++) {
            const uint64_t key = step.list[i].key;
            if (!key) continue;
            if (keys.count(key)) return KeyTarget{index, i, false};
            if (keys.size() == 1 && !merge) merge = KeyTarget{index, i, true};
            keys.insert(key);
        }
    }
    return merge;
}

struct Options {
    std::string recording, source;
    std::vector<std::string> needs;
    std::optional<uint64_t> sessions;
    std::optional<uint64_t> message_bytes;
    Perturb perturb = Perturb::None;
};

bool number(const std::string& text, uint64_t& out) {
    if (text.empty() || text.size() > 19 || text.find_first_not_of("0123456789") != std::string::npos) return false;
    out = std::strtoull(text.c_str(), nullptr, 10);
    return true;
}

bool options_of(int argc, char** argv, Options& o) {
    for (int i = 1; i < argc; i++) {
        const std::string arg = argv[i];
        const bool valued = arg == "--source" || arg == "--needs" || arg == "--sessions" || arg == "--message-bytes" || arg == "--perturb";
        if (valued && i + 1 == argc) return false;
        uint64_t n = 0;
        if (arg == "--source") {
            o.source = argv[++i];
        } else if (arg == "--needs") {
            std::string list = argv[++i];
            for (size_t at = 0; at <= list.size();) {
                const size_t comma = std::min(list.find(',', at), list.size());
                o.needs.push_back(list.substr(at, comma - at));
                if (!Counts().need(o.needs.back())) return false;
                at = comma + 1;
            }
        } else if (arg == "--sessions") {
            if (!number(argv[++i], n)) return false;
            o.sessions = n;
        } else if (arg == "--message-bytes") {
            if (!number(argv[++i], n) || n > std::numeric_limits<size_t>::max()) return false;
            o.message_bytes = n;
        } else if (arg == "--perturb") {
            const std::string what = argv[++i];
            if (what != "list" && what != "call" && what != "key") return false;
            o.perturb = what == "list" ? Perturb::List : what == "call" ? Perturb::Call : Perturb::Key;
        } else if (arg.empty() || arg[0] == '-' || !o.recording.empty()) {
            return false;
        } else {
            o.recording = arg;
        }
    }
    return !o.recording.empty() && !o.source.empty();
}

/// The numbers a step of `kind` carries, a letter each (d a double, f a float, i an int, b a bool, u an edit generation, p a Paint), and how many texts.
std::pair<std::string, size_t> shape_of(record::Kind kind) {
    switch (kind) {
    case record::Kind::Construct: return {"ddb", 0};
    case record::Kind::Frame: return {"bp", 0};
    case record::Kind::Touch: return {"iddibbii", 0};
    case record::Kind::Hover: return {"iddi", 0};
    case record::Kind::SetWidth:
    case record::Kind::SetMaxHeight:
    case record::Kind::Charge: return {"d", 0};
    case record::Kind::SetTheme:
    case record::Kind::SetCompositing:
    case record::Kind::SetPatching: return {"b", 0};
    case record::Kind::ClaimAt: return {"dd", 0};
    case record::Kind::SetResizeTracking: return {"bb", 0};
    case record::Kind::Wheel: return {"ddddii", 0};
    case record::Kind::Key: return {"bi", 3};
    case record::Kind::Edit: return {"u", 2};
    case record::Kind::ImageReady: return {"iff", 0};
    default: return {"", 0};
    }
}

bool fits(char type, double v) {
    switch (type) {
    case 'd': return true;
    case 'f': return std::isnan(v) || std::isinf(v) || (std::fabs(v) <= FLT_MAX && double(float(v)) == v);
    case 'i': return v >= INT_MIN && v <= INT_MAX && v == std::floor(v);
    case 'b': return v == 0 || v == 1;
    case 'u': return v >= 0 && v <= 9007199254740992.0 && v == std::floor(v);
    case 'p': return v == 0 || v == 1 || v == 2;
    default: return false;
    }
}

bool well_formed(const record::Step& step) {
    const auto [numbers, texts] = shape_of(step.kind);
    if (step.numbers.size() != numbers.size() || step.texts.size() != texts || step.message.has_value() != (step.kind == record::Kind::Receive)) return false;
    for (size_t i = 0; i < numbers.size(); i++)
        if (!fits(numbers[i], step.numbers[i])) return false;
    return true;
}

/// A call the channel does not carry, made by the helper on the end's renderer with the recorded arguments.
std::vector<Json> helper(Renderer& r, const record::Step& step) {
    const std::vector<double>& n = step.numbers;
    const std::vector<std::string>& t = step.texts;
    switch (step.kind) {
    case record::Kind::Wheel: return r.wheel(n[0], n[1], n[2], n[3], int(n[4]), int(n[5]));
    case record::Kind::Key: return r.key(t[0], t[1], t[2], n[0] != 0, int(n[1]));
    case record::Kind::Edit: return r.edit(uint64_t(n[0]), t[0], t[1]);
    case record::Kind::EndEditing: return r.end_editing();
    case record::Kind::SetCompositing: r.set_compositing(n[0] != 0); return {};
    case record::Kind::ImageReady: return r.image_ready(int(n[0]), float(n[1]), float(n[2]));
    case record::Kind::SetResizeTracking: r.set_resize_tracking(n[0] != 0, n[1] != 0); return {};
    case record::Kind::Trim: r.trim(); return {};
    case record::Kind::Charge: r.charge(n[0]); return {};
    case record::Kind::SetPatching: r.set_patching(n[0] != 0); return {};
    case record::Kind::LastHtml: r.last_html(); return {};
    default: return {};
    }
}

void forget(Loopback& loop) {
    loop.host.events.clear();
    loop.host.frames.clear();
    loop.host.states.clear();
    loop.host.batches.clear();
    loop.host.pointers.clear();
    loop.host.selections.clear();
    loop.host.claims.clear();
    loop.host.refusals.clear();
    loop.backend.calls.clear();
    loop.outputs.clear();
    loop.delivered.clear();
    loop.wire.kinds.clear();
}

class Replay {
public:
    explicit Replay(Options options) : o_(std::move(options)) {
        if (o_.message_bytes) caps_.message = size_t(*o_.message_bytes);
    }

    int run();

private:
    int refuse(const std::string& why) const;
    std::string at(const record::Step& step) const;
    std::string perturb(record::Step& step);
    bool correspond(Session& session, const DisplayList& recorded, DisplayList& heard, size_t& keyed, size_t& fresh, std::string& why);
    bool expect(const record::Step& step, const Session& session, Expected& s) const;
    int play(record::Step& step, size_t bytes);
    void print_failures(const record::Step& step, const std::vector<std::string>& failures) const;

    Options o_;
    remote::Caps caps_;
    std::map<uint32_t, std::unique_ptr<Session>> live_;
    std::map<uint32_t, std::pair<std::string, Counts>> labelled_;
    Counts total_;
    uint64_t index_ = 0;
    uint32_t made_ = 0;
    size_t most_alive_ = 0;
    bool perturbed_ = false;
    std::optional<KeyTarget> key_target_;
};

int Replay::refuse(const std::string& why) const {
    std::printf("%s: %s\n", o_.source.c_str(), why.c_str());
    return 2;
}

std::string Replay::at(const record::Step& step) const {
    const auto found = live_.find(step.session);
    const std::string label = step.kind == record::Kind::Construct ? step.label : found == live_.end() ? std::string() : found->second->label;
    return "step " + std::to_string(index_) + ", a " + record::kind_name(step.kind) + " of session " + std::to_string(step.session) + (label.empty() ? "" : " (" + label + ")");
}

/// Changes the first eligible recorded value once, and gives the start of the one failure that must name it.
std::string Replay::perturb(record::Step& step) {
    if (perturbed_) return {};
    if (o_.perturb == Perturb::List && step.kind == record::Kind::Frame && step.painted && !step.list.empty()) {
        float& x = step.list[0].rect.x;
        const float moved = std::nextafter(x, std::numeric_limits<float>::infinity());
        if (!std::isfinite(x) || bits(moved) == bits(x)) return {};
        std::printf("%s: --perturb list moves the list's [0].rect.x of %s from %s to %s\n", o_.source.c_str(), at(step).c_str(), hex(bits(x)).c_str(), hex(bits(moved)).c_str());
        x = moved;
        perturbed_ = true;
        return "the frame's list is the direct paint [0].rect.x: ";
    }
    if (o_.perturb == Perturb::Call) {
        for (size_t i = 0; i < step.calls.size(); i++) {
            if (step.calls[i].op != Call::TextWidth) continue;
            std::printf("%s: --perturb call appends a byte to the text of Backend call %zu of %s\n", o_.source.c_str(), i, at(step).c_str());
            step.calls[i].text.push_back('x');
            perturbed_ = true;
            return "the Backend calls are the direct renderer's, in order call[" + std::to_string(i) + "].text: ";
        }
    }
    return {};
}

/// Holds the session's one-to-one correspondence of keys over a heard list (zero only with zero, any other key always with the same partner), then gives the heard list the recorded keys, so same() compares all else; false naming the first command that breaks it.
bool Replay::correspond(Session& session, const DisplayList& recorded, DisplayList& heard, size_t& keyed, size_t& fresh, std::string& why) {
    const size_t n = std::min(recorded.size(), heard.size());
    bool held = true;
    for (size_t i = 0; i < n; i++) {
        const uint64_t r = recorded[i].key;
        uint64_t& k = heard[i].key;
        if (key_target_ && !perturbed_ && index_ == key_target_->step && i == key_target_->command) {
            const uint64_t was = k;
            const auto first = session.keys_to.find(session.first_key);
            k = key_target_->merge && first != session.keys_to.end() ? first->second : k ^ 1;
            perturbed_ = true;
            std::printf("%s: --perturb key changes the replayed [%zu].key of step %llu from %s to %s, %s\n", o_.source.c_str(), i, static_cast<unsigned long long>(index_), hex(was).c_str(),
                        hex(k).c_str(), key_target_->merge ? "the session's first key, merging two" : "its lowest bit flipped where the recorded key repeats");
        }
        if (!r && !k) continue;
        const auto to = r ? session.keys_to.find(r) : session.keys_to.end();
        const auto back = k ? session.keys_back.find(k) : session.keys_back.end();
        std::string broken;
        if (!r || !k) {
            broken = "a key against none";
        } else if (to != session.keys_to.end() && to->second != k) {
            broken = "where the session's earlier commands gave " + hex(r) + " the key " + hex(to->second);
        } else if (to == session.keys_to.end() && back != session.keys_back.end()) {
            broken = "where " + hex(k) + " already stands for " + hex(back->second);
        }
        if (!broken.empty()) {
            why = "[" + std::to_string(i) + "].key: " + hex(r) + " against " + hex(k) + ", " + broken;
            held = false;
            break;
        }
        keyed++;
        if (to != session.keys_to.end()) continue;
        session.keys_to.emplace(r, k);
        session.keys_back.emplace(k, r);
        if (!session.first_key) session.first_key = r;
        fresh++;
    }
    for (size_t i = 0; i < n; i++) heard[i].key = recorded[i].key;
    return held;
}

/// What the recorded renderer gave, with the reports the host must hear of it; false when a recorded message does not read back as it was written.
bool Replay::expect(const record::Step& step, const Session& session, Expected& s) const {
    for (const std::string& text : step.messages) {
        std::optional<Json> m = Json::parse(text, Json::Surrogates::Keep, 4096);
        if (!m || m->dump() != text) return false;
        s.messages.push_back(std::move(*m));
    }
    s.engine = step.engine;
    if (step.kind == record::Kind::Touch || step.kind == record::Kind::Hover) s.pointer = remote::Pointer{step.cursor, gesture_of(s.messages)};
    if (step.kind == record::Kind::Frame) {
        remote::Frame f;
        f.painted = step.painted;
        f.list = step.list;
        f.damage = step.damage;
        s.frame = std::move(f);
    }
    if (step.kind == record::Kind::ClaimAt) s.claim = step.claim;
    s.selection = step.selection;
    s.state = step.state;
    if (step.kind == record::Kind::Construct) {
        if (!s.selection.empty()) s.events.push_back(remote::Kind::Selection);
        s.events.push_back(remote::Kind::State);
    } else if (step.kind != record::Kind::Destroy) {
        predict(s, session.selection, session.sent);
    }
    return true;
}

void Replay::print_failures(const record::Step& step, const std::vector<std::string>& failures) const {
    std::printf("%s: %s differs:\n", o_.source.c_str(), at(step).c_str());
    for (size_t i = 0; i < failures.size() && i < 6; i++) std::printf("  %s\n", failures[i].c_str());
    if (failures.size() > 6) std::printf("  and %zu more\n", failures.size() - 6);
}

/// One step through its session's two ends: -1 when it matched, else the exit code.
int Replay::play(record::Step& step, size_t bytes) {
    const record::Kind kind = step.kind;
    if (!well_formed(step)) return refuse(at(step) + " does not hold the arguments such a step takes");
    auto found = live_.find(step.session);
    if (kind == record::Kind::Construct) {
        if (found != live_.end() || step.session <= made_) return refuse(at(step) + " makes a session that is not new");
        made_ = step.session;
        found = live_.emplace(step.session, std::make_unique<Session>(caps_)).first;
        found->second->label = step.label;
        most_alive_ = std::max(most_alive_, live_.size());
    } else if (found == live_.end()) {
        return refuse(at(step) + " names no live session");
    }
    Session& session = *found->second;
    Loopback& loop = session.loop;
    std::vector<std::string> failures;
    const auto ok = [&](bool cond, const std::string& what) {
        if (!cond) failures.push_back(what);
    };
    const litehtml::string_id_usage usage = litehtml::_id_usage();
    ok(usage.names == step.names && usage.bytes == step.name_bytes, "litehtml's name table holds " + std::to_string(usage.names) + " names of " + std::to_string(usage.bytes)
        + " bytes where the recording had " + std::to_string(step.names) + " of " + std::to_string(step.name_bytes));
    std::string leaf = perturb(step);
    if (key_target_ && !perturbed_ && index_ == key_target_->step) leaf = "the frame's list is the direct paint [" + std::to_string(key_target_->command) + "].key: ";
    Expected s;
    if (!expect(step, session, s)) return refuse(at(step) + " holds a message that does not read back as it was recorded");
    session.answers.start(step.calls);
    session.clock.start(step.clock);
    const std::vector<double>& n = step.numbers;
    std::vector<Json> returned;
    std::optional<remote::RendererEnd::Output> out;
    bool input = false;
    switch (kind) {
    case record::Kind::Construct: loop.open(n[0], n[1], n[2] != 0, &session.clock); break;
    case record::Kind::Destroy: loop.close(); break;
    case record::Kind::Receive: returned = loop.receive(*step.message); break;
    case record::Kind::WantFrame: loop.want_frame(); break;
    case record::Kind::Frame:
        s.seq = loop.platform.request_frame(n[0] != 0, remote::Paint(uint8_t(n[1])));
        input = true;
        break;
    case record::Kind::Touch:
        loop.platform.touch(int(n[0]), n[1], n[2], int(n[3]), n[4] != 0, n[5] != 0, int(n[6]), int(n[7]));
        input = true;
        break;
    case record::Kind::Hover:
        loop.platform.hover(int(n[0]), n[1], n[2], int(n[3]));
        input = true;
        break;
    case record::Kind::SetWidth:
        loop.platform.set_width(n[0]);
        input = true;
        break;
    case record::Kind::SetMaxHeight:
        loop.platform.set_max_height(n[0]);
        input = true;
        break;
    case record::Kind::SetTheme:
        loop.platform.set_theme(n[0] != 0);
        input = true;
        break;
    case record::Kind::ClearSelection:
        loop.platform.clear_selection();
        input = true;
        break;
    case record::Kind::ClaimAt:
        s.seq = loop.platform.ask_claim(n[0], n[1]);
        input = true;
        break;
    default:
        ok(!loop.end->stopped(), "the helper calls the renderer of a healthy end");
        if (!loop.end->stopped()) {
            returned = helper(loop.end->renderer(), step);
            loop.end->report(returned);
            loop.wire.flush();
        }
        break;
    }
    if (input) {
        ok(loop.pump() && loop.delivered.size() == 1 && loop.delivered.back(), "the platform input is delivered and taken");
        out = loop.outputs.empty() ? remote::RendererEnd::Output() : loop.outputs.back();
        returned = out->messages;
    }
    size_t keyed = 0, fresh = 0;
    if (kind == record::Kind::Destroy) {
        ok(loop.host.events.empty() && loop.host.refusals.empty() && loop.wire.refused == 0 && !loop.platform.refusing() && loop.queue.messages.empty(),
           "destruction reports nothing and the platform end takes all of it");
    } else {
        healthy(ok, loop);
        if (s.frame && !loop.host.frames.empty()) {
            std::string broken;
            const bool held = correspond(session, s.frame->list, loop.host.frames.back().list, keyed, fresh, broken);
            ok(held, "the frame's list is the direct paint " + broken);
        }
        compare_reports(ok, s, loop, 0, returned, out ? &*out : nullptr);
    }
    std::string why;
    const bool same_log = same_calls(step.calls, loop.backend.calls, why);
    ok(same_log, "the Backend calls are the direct renderer's, in order " + why);
    ok(session.answers.mismatch.empty(), session.answers.mismatch);
    const size_t left = session.answers.left();
    ok(left == 0, std::to_string(left) + " recorded Backend calls are never made");
    const ReplayClock& clock = session.clock;
    ok(!clock.over && clock.next == step.clock.size(), "the renderer reads the clock " + (clock.over ? std::string("past its ") : std::to_string(clock.next) + " of its ")
        + std::to_string(step.clock.size()) + " recorded times");
    if (!leaf.empty()) {
        const char* name = perturb_name(o_.perturb);
        if (failures.size() == 1 && failures[0].compare(0, leaf.size(), leaf) == 0) {
            std::printf("%s: --perturb %s is caught at %s, on that leaf alone: %s\n", o_.source.c_str(), name, at(step).c_str(), failures[0].c_str());
            return 0;
        }
        if (!failures.empty()) print_failures(step, failures);
        std::printf("%s: --perturb %s is not caught at %s on precisely the changed leaf\n", o_.source.c_str(), name, at(step).c_str());
        return 1;
    }
    if (!failures.empty()) {
        print_failures(step, failures);
        return 1;
    }
    total_.add(step, bytes, loop, returned, keyed, fresh);
    if (!session.label.empty()) {
        std::pair<std::string, Counts>& entry = labelled_[step.session];
        entry.first = session.label;
        entry.second.add(step, bytes, loop, returned, keyed, fresh);
    }
    session.selection = s.selection;
    session.sent = s.state;
    forget(loop);
    if (kind == record::Kind::Destroy) live_.erase(found);
    return -1;
}

int Replay::run() {
    record::Reader reader;
    if (!reader.open(o_.recording)) return refuse("the recording is refused: " + reader.error());
    std::printf("%s: origin %s\n", o_.source.c_str(), reader.origin().c_str());
    if (o_.perturb == Perturb::Key) {
        key_target_ = key_target(o_.recording);
        if (!key_target_) {
            std::printf("%s: --perturb key found no keyed command to change\n", o_.source.c_str());
            return 1;
        }
    }
    record::Step step;
    size_t bytes = 0;
    while (reader.next(step, bytes)) {
        index_++;
        if (const int code = play(step, bytes); code >= 0) return code;
    }
    if (!reader.ended()) return refuse("the recording is refused: " + reader.error());
    if (o_.perturb != Perturb::None) {
        std::printf("%s: --perturb %s found no step to change\n", o_.source.c_str(), perturb_name(o_.perturb));
        return 1;
    }
    if (!live_.empty()) {
        std::printf("%s: %zu sessions are still alive at the end, the first %u\n", o_.source.c_str(), live_.size(), unsigned(live_.begin()->first));
        return 1;
    }
    std::printf("%s: compared %s; at most %zu sessions alive at once; message cap %zu bytes\n", o_.source.c_str(), total_.text().c_str(), most_alive_, caps_.message);
    for (const auto& [id, entry] : labelled_) std::printf("%s session %u (%s): %s\n", o_.source.c_str(), unsigned(id), entry.first.c_str(), entry.second.text().c_str());
    bool met = true;
    if (o_.sessions && total_.sessions != *o_.sessions) {
        std::printf("%s: %llu sessions where %llu are due\n", o_.source.c_str(), static_cast<unsigned long long>(total_.sessions), static_cast<unsigned long long>(*o_.sessions));
        met = false;
    }
    for (const std::string& name : o_.needs) {
        if (*total_.need(name) > 0) continue;
        std::printf("%s: nothing compared of %s\n", o_.source.c_str(), name.c_str());
        met = false;
    }
    if (!met) return 1;
    std::printf("%s: every step of every session matches\n", o_.source.c_str());
    return 0;
}

} // namespace

int main(int argc, char** argv) {
    Options o;
    if (!options_of(argc, argv, o)) {
        std::fprintf(stderr, "usage: twidget_replay <recording> --source NAME [--needs a,b,...] [--sessions N] [--message-bytes N] [--perturb list|call|key]\n"
                             "needs: sessions steps lists batches reads calls pointers claims holds selections ticks engine resizes themes images keys\n");
        return 2;
    }
    return Replay(std::move(o)).run();
}
