// The widths a face has measured, and those a layout asks for in bulk before it reads them: its texts' words, the units its spaced runs are painted in and, at its first unmeasured break, the characters of the texts it may break; docs/TWIDGET_CORE.md "A layout's words".
#pragma once

#include "backend.h"

#include <cstddef>
#include <functional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#ifdef TW_LITEHTML_TWIDGET
#include <litehtml/types.h>

namespace litehtml {
class el_text;
}
#endif

namespace twlite {

/// Measured widths, keyed by their text: longer text is measured each time, and every face's keys together stay within the byte bound.
constexpr size_t MAX_WIDTH_ENTRIES = 4096;
constexpr size_t MAX_WIDTH_KEY = 256;
constexpr size_t MAX_WIDTH_BYTES = 1 << 20;

/// The widths measured in one backend font.
struct FaceWidths {
    int handle = 0;
    struct Hash {
        using is_transparent = void;
        size_t operator()(std::string_view s) const { return std::hash<std::string_view>{}(s); }
    };
    std::unordered_map<std::string, float, Hash, std::equal_to<>> widths;
    size_t* pool = nullptr;  // the key bytes every face holds
    size_t bytes = 0;        // this face's share of them
    FaceWidths() = default;
    FaceWidths(const FaceWidths&) = delete;
    FaceWidths& operator=(const FaceWidths&) = delete;
    ~FaceWidths() { forget(); }
    void forget() {
        widths.clear();
        if (pool) *pool -= bytes;
        bytes = 0;
    }
    float width(Backend& backend, std::string_view text) {
        if (auto c = widths.find(text); c != widths.end()) return c->second;
        float w = backend.text_width(handle, std::string(text));
        remember(text, w);
        return w;
    }
    void remember(std::string_view text, float w) {
        // Words repeat across documents, but a widget that types a counter would grow this forever.
        if (text.size() > MAX_WIDTH_KEY) return;
        if (widths.size() >= MAX_WIDTH_ENTRIES || (pool && *pool + text.size() > MAX_WIDTH_BYTES)) forget();
        if (pool && *pool + text.size() > MAX_WIDTH_BYTES) return;  // other faces hold the budget
        if (!widths.emplace(std::string(text), w).second) return;
        bytes += text.size();
        if (pool) *pool += text.size();
    }
};

#ifdef TW_LITEHTML_TWIDGET
/// A layout's widths in bulk, each set in one Backend::text_widths call: before it, the words its texts owe and the units its spaced runs are painted in; at its first character no face has measured, the characters of every text it may break.
class LayoutWidths {
public:
    /// What the container knows of a litehtml font: the widths of its face, null for a font it does not know, the units a run in it is painted in, none when it adds no spacing, and where a text may break, from 0 to its end.
    class Fonts {
    public:
        virtual FaceWidths* face(litehtml::uint_ptr font) const = 0;
        virtual std::vector<std::string_view> painted_units(litehtml::uint_ptr font, std::string_view text) const = 0;
        virtual std::vector<size_t> break_points(const std::string& text) const = 0;

    protected:
        ~Fonts() = default;
    };

    /// `pool` counts the key bytes every face holds.
    LayoutWidths(Backend& backend, const Fonts& fonts, const size_t& pool) : backend_(backend), fonts_(fonts), pool_(pool) {}

    /// Measures the words `owed` owe and the units the `shown` texts are painted in, pays every owed width, and keeps the shown texts the layout may break.
    void before(const std::vector<litehtml::el_text*>& owed, const std::vector<litehtml::el_text*>& shown);
    /// One character of a text that breaks is about to be measured in `font`: the first its face lacks asks for the characters of every text kept, at most once a layout.
    void character(litehtml::uint_ptr font, std::string_view text);
    /// The layout is over: no text is kept past it.
    void after() { breakable_.clear(); }

private:
    void measure_breaks();

    Backend& backend_;
    const Fonts& fonts_;
    const size_t& pool_;
    std::vector<litehtml::el_text*> breakable_;
};
#endif

} // namespace twlite
