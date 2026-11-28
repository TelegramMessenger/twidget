// tdesktop's tgw/tgw_cursor.cpp over helper_seam's renderer.h and a copy of tgw_cursor.h with its members public (helper_oracles.cmake), for rust/crates/helper to compare with: 'N' a new CursorStyles at a ratio, 'p' a pointer() call with the renderer's state, answered with its result, the styles' state and every indexed element's computed cursor.

#include "core/src/renderer.h"
#include "oracle_io.h"
#include "tgw/tgw_cursor.h"

#include <litehtml.h>

#include <algorithm>
#include <memory>
#include <utility>
#include <vector>

namespace {

using oracle::In;
using oracle::Out;

std::unique_ptr<Tgw::CursorStyles> Styles;
twlite::Renderer Source;

void Pointer(In &in) {
    Source.stats_.builds = in.i32();
    Source.html_ = in.str();
    Source.master_ = in.str();
    Source.width_ = in.f64();
    Source.height_ = in.i32();
    const auto hit = in.i64();
    const auto hover = in.i64();
    const auto active = in.i64();
    auto out = Out();
    out.flag(Styles->pointer(Source, hit, hover, active));
    out.i64(Styles->_hover).i64(Styles->_active).i32(Styles->_builds);
    out.f64(Styles->_width).i32(Styles->_height).flag(Styles->_document != nullptr);
    out.u64(Styles->_html.size()).u64(Styles->_master.size());
    auto ids = std::vector<std::pair<int64_t, std::shared_ptr<litehtml::element>>>(Styles->_elements.begin(), Styles->_elements.end());
    std::sort(ids.begin(), ids.end(), [](const auto &a, const auto &b) { return a.first < b.first; });
    out.u32(uint32_t(ids.size()));
    for (const auto &[id, element] : ids) {
        out.i64(id).str(element->css().get_cursor());
    }
    oracle::answer(1, out.s);
}

} // namespace

double twlite::Renderer::clock() {
    return 0.;
}

int main() {
    return oracle::serve([](int op, In &in) {
        if (op == 'N') {
            Styles = nullptr;
            Styles = std::make_unique<Tgw::CursorStyles>(in.f64());
            oracle::answer(1, {});
        } else if (op == 'p' && Styles) {
            Pointer(in);
        } else {
            return false;
        }
        return true;
    });
}
