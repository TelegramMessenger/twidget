// tdesktop's tgw/tgw_hosted_wire.h and the helper work record of test/test_widget_renderer_forward.h on stdin, compiled from a tdesktop checkout (helper_oracles.cmake), for rust/crates/helper to compare with: 'd' a message, answered with what every decoder makes of it; 'e' an encoder (u8 which, then its arguments), answered with its bytes, status 0 when it refuses; 'k' the constants and the embedding caps.

#include "oracle_io.h"
#include "test/test_widget_renderer_forward.h"
#include "tgw/tgw_hosted_wire.h"

#include <string>
#include <string_view>

namespace {

namespace HostedWire = Tgw::HostedWire;
using HostedWire::Kind;
using oracle::In;
using oracle::Out;

constexpr Kind kKinds[] = {
    Kind::Open,
    Kind::MenuDeliver,
    Kind::FingerHold,
    Kind::FingerRelease,
    Kind::Mirror,
    Kind::MirrorWork,
    Kind::CanvasRefill,
    Kind::Content,
    Kind::Loaded,
    Kind::Stopped,
    Kind::Mirrored,
    Kind::Cursor,
    Kind::CanvasRefused,
    Kind::Clicked,
};

void PutRecord(Out &out, const Test::HelperWorkRecord &record) {
    out.u8(uint8_t(record.kind));
    out.i64(record.started).i64(record.finished).i64(record.waited);
    out.u64(record.seq).u32(record.waits).u32(record.frameBytes);
}

void Decode(std::string_view message) {
    auto out = Out();
    const auto kind = HostedWire::KindOf(message);
    out.u8(kind ? uint8_t(*kind) : 0);
    out.flag(HostedWire::IsControl(message));
    for (const auto each : kKinds) {
        out.flag(HostedWire::IsExactly(message, each));
    }
    if (message.size() >= 2) {
        const auto named = Kind(uint8_t(message[1]));
        out.flag(HostedWire::ToHelper(named));
        out.flag(HostedWire::FromHelper(named));
        out.flag(HostedWire::IsTapControl(named));
    }
    if (message.size() >= 3) {
        out.flag(HostedWire::IsMirroredWay(message[2]));
    }
    const auto open = HostedWire::DecodeOpen(message);
    out.flag(open.has_value()).f64(open.value_or(0.));
    const auto press = HostedWire::DecodePress(message);
    out.flag(press.has_value()).u64(press.value_or(0));
    const auto clicked = HostedWire::DecodeClicked(message);
    out.flag(clicked.has_value()).u64(clicked.value_or(0));
    const auto stopped = HostedWire::DecodeStopped(message);
    out.flag(stopped.has_value()).str(stopped.value_or(std::string_view()));
    const auto cursor = HostedWire::DecodeCursor(message);
    out.flag(cursor.has_value());
    out.u64(cursor ? cursor->pointers : 0).str(cursor ? cursor->cursor : std::string_view());
    const auto mirrored = HostedWire::DecodeMirrored(message);
    out.flag(mirrored.has_value());
    out.u8(mirrored ? uint8_t(mirrored->way) : 0).str(mirrored ? mirrored->text : std::string_view());
    const auto work = Test::DecodeHelperWork(message);
    out.flag(work.has_value());
    PutRecord(out, work.value_or(Test::HelperWorkRecord()));
    oracle::answer(1, out.s);
}

void Encode(In &in) {
    const auto which = in.u8();
    switch (which) {
    case 0: return oracle::answer(1, HostedWire::EncodeOpen(in.f64()));
    case 1: return oracle::answer(1, HostedWire::EncodeMenuDeliver(in.u64()));
    case 2: return oracle::answer(1, HostedWire::EncodeFingerHold(in.u64()));
    case 3: return oracle::answer(1, HostedWire::EncodeFingerRelease(in.u64()));
    case 4: return oracle::answer(1, HostedWire::EncodeClicked(in.u64()));
    case 5: return oracle::answer(1, HostedWire::EncodeMirror());
    case 6: return oracle::answer(1, HostedWire::EncodeMirrorWork());
    case 7: return oracle::answer(1, HostedWire::EncodeCanvasRefill());
    case 8: return oracle::answer(1, HostedWire::EncodeCanvasRefused());
    case 9: return oracle::answer(1, HostedWire::EncodeContent());
    case 10: return oracle::answer(1, HostedWire::EncodeLoaded());
    case 11: return oracle::answer(1, HostedWire::EncodeStopped(in.str()));
    case 12: {
        const auto pointers = in.u64();
        return oracle::answer(1, HostedWire::EncodeCursor(pointers, in.str()));
    }
    case 13: {
        const auto way = char(in.u8());
        const auto bytes = HostedWire::EncodeMirrored(way, in.str());
        return oracle::answer(bytes ? 1 : 0, bytes.value_or(std::string()));
    }
    case 14: {
        auto record = Test::HelperWorkRecord();
        record.kind = Test::HelperWorkKind(in.u8());
        record.started = in.i64();
        record.finished = in.i64();
        record.waited = in.i64();
        record.seq = in.u64();
        record.waits = in.u32();
        record.frameBytes = in.u32();
        return oracle::answer(1, Test::EncodeHelperWork(record));
    }
    }
    oracle::answer(2, {});
}

void Constants() {
    auto out = Out();
    const auto caps = HostedWire::EmbeddingCaps();
    const auto &v = caps.values;
    for (const auto value : {
            v.commands, v.canvas_ops, v.path_ops, v.floats, v.stops, v.filters, v.clips, v.text, v.pixels, v.rects,
            caps.message, caps.text, caps.url, caps.messages, caps.widths, caps.widths_text, caps.frame, caps.delta_base }) {
        out.u64(uint64_t(value));
    }
    for (const auto value : {
            HostedWire::kHeaderBytes, HostedWire::kNumberBytes, HostedWire::kLengthBytes, HostedWire::kMaxMessageBytes,
            HostedWire::kMaxListCommands, HostedWire::kMaxFrameBytes, HostedWire::kMaxCanvasRunOps, HostedWire::kMaxWidthsTexts,
            HostedWire::kMaxWidthsTextBytes, HostedWire::kMaxDeltaBaseBytes, HostedWire::kMaxStopReasonBytes, HostedWire::kMaxCursorBytes,
            HostedWire::kMirroredOverheadBytes, HostedWire::kMaxMirroredTextBytes, Test::kHelperWorkRecordBytes }) {
        out.u64(uint64_t(value));
    }
    out.u8(HostedWire::kTag);
    out.u8(uint8_t(HostedWire::kMirroredBuilt)).u8(uint8_t(HostedWire::kMirroredHost)).u8(uint8_t(HostedWire::kMirroredRenderer));
    out.u8(uint8_t(HostedWire::kMirroredTheme)).u8(uint8_t(HostedWire::kMirroredWork));
    out.i64(Test::kMaxHelperInterval);
    oracle::answer(1, out.s);
}

} // namespace

int main() {
    return oracle::serve([](int op, In &in) {
        if (op == 'd') {
            Decode(in.s);
        } else if (op == 'e') {
            Encode(in);
        } else if (op == 'k') {
            Constants();
        } else {
            return false;
        }
        return true;
    });
}
