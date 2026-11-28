// tdesktop's tgw/tgw_renderer_rules.h on stdin, over helper_seam's renderer.h, for rust/crates/helper to compare with; each record is one rule's inputs (oracle_io.h), answered with what the rule makes of them.

#include "core/src/renderer.h"
#include "oracle_io.h"
#include "tgw/tgw_renderer_rules.h"

#include <string>
#include <vector>

namespace {

namespace Rules = Tgw::RendererRules;
using oracle::In;
using oracle::Out;
using twidget::Json;

double Clock = 0.;

constexpr const char *kTypes[] = { "ops", "query", "event", "size", "result", "frame", "" };

void PutList(Out &out, const std::vector<Json> &list) {
    out.u32(uint32_t(list.size()));
    for (const auto &message : list) {
        out.str(message.dump());
    }
}

std::vector<Json> TakeList(In &in) {
    auto result = std::vector<Json>();
    const auto count = in.u32();
    for (auto i = uint32_t(); i != count; ++i) {
        result.push_back(Json::parse(in.str()).value_or(Json()));
    }
    return result;
}

struct Receiver {
    std::vector<Json> answer;
    std::string asked;

    std::vector<Json> receive(const Json &query) {
        asked = query.dump();
        return answer;
    }
};

struct Spelling {
    uint32_t id = 0;
};

bool Run(int op, In &in) {
    auto out = Out();
    switch (op) {
    case 'r':
        out.f64(Rules::DeviceRatio(in.f64()));
        break;
    case 's': {
        const auto dx = in.f64();
        out.flag(Rules::OutsideSlop(dx, in.f64()));
    } break;
    case 'p': {
        const auto hit = in.i64();
        const auto held = in.flag();
        const auto synced = in.flag();
        const auto pressHit = in.i64();
        const auto targets = Rules::PointerTargets(hit, held, synced, pressHit, in.flag());
        out.i64(targets.hover).i64(targets.active);
    } break;
    case 'q': {
        auto renderer = twlite::Renderer();
        renderer.stats_.builds = in.i32();
        renderer.stats_.patched = in.i32();
        out.i32(Rules::ParsedDocuments(renderer));
    } break;
    case 'f':
        Clock = in.f64();
        out.str(Rules::FrameMessage().dump());
        break;
    case 'j': {
        const auto parsed = Json::parse(in.str());
        if (!parsed) {
            oracle::answer(0, {});
            return true;
        }
        const auto &message = *parsed;
        out.flag(Rules::KeptBeforeReady(message));
        out.flag(Rules::DrawsContent(message));
        out.flag(Rules::IsLoadAck(message));
        out.flag(Rules::IsContextMenu(message, "mouse"));
        out.flag(Rules::IsContextMenu(message, "touch"));
        out.flag(Rules::IsContextMenu(message, ""));
        out.flag(Rules::IsClick(message));
        for (const auto type : kTypes) {
            out.flag(Rules::OfType(type)(message));
        }
        out.str(Rules::LocalQuery(message).dump());
    } break;
    case 'a': {
        auto hasContent = in.flag();
        auto acked = in.flag();
        auto loaded = in.flag();
        for (const auto &message : TakeList(in)) {
            const auto change = Rules::AdvanceLoad(message, hasContent, acked, loaded);
            out.flag(change.content).flag(change.loaded);
            out.flag(hasContent).flag(acked).flag(loaded);
        }
    } break;
    case 'm': {
        const auto pointerType = in.str();
        auto list = TakeList(in);
        const auto menus = Rules::TakeContextMenus(list, pointerType);
        PutList(out, menus);
        PutList(out, list);
    } break;
    case 't': {
        auto receiver = Receiver{ .answer = TakeList(in) };
        const auto settled = Rules::TakeSettled(receiver);
        out.str(receiver.asked);
        PutList(out, settled);
    } break;
    case 'c': {
        auto spellings = Rules::CanvasSpellings<Spelling>();
        auto parses = uint32_t();
        const auto count = in.u32();
        for (auto i = uint32_t(); i != count; ++i) {
            const auto refill = in.flag();
            const auto css = in.str();
            if (refill) {
                spellings.refill();
                continue;
            }
            auto parsed = false;
            const auto entry = spellings.find(css, [&](const std::string &text) {
                parsed = true;
                return Spelling{ .id = ++parses + uint32_t(text.size()) * 1000 };
            });
            out.flag(entry != nullptr).flag(parsed).u32(entry ? entry->id : 0);
        }
    } break;
    case 'x': {
        out.str(Rules::kReadyMessage).str(Rules::CanvasFontLimitText()).str(Rules::kCanvasFontLimitKind);
        out.f64(Rules::kMinRatio).f64(Rules::kMaxRatio);
        for (const auto value : {
                Rules::kTouchDown, Rules::kTouchMove, Rules::kTouchUp, Rules::kTouchCancel, Rules::kHoverMove,
                Rules::kHoverLeave, Rules::kMousePointer, Rules::kFingerPointer, Rules::kPrimaryButton,
                Rules::kSecondaryButton, Rules::kLocalQueryId, Rules::kLoadAckQueryId, Rules::kMaxNewCanvasFonts,
                Rules::kMaxCanvasFonts, Rules::kMaxCanvasFontParses, Rules::kMaxCanvasFontRequests }) {
            out.i32(value);
        }
        out.u64(Rules::kMaxCanvasFontSpellings).u64(Rules::kMaxCanvasFontSpellingSize);
        out.str(Rules::ViewportQuery().dump());
    } break;
    default:
        return false;
    }
    oracle::answer(1, out.s);
    return true;
}

} // namespace

double twlite::Renderer::clock() {
    return Clock;
}

int main() {
    return oracle::serve(Run);
}
