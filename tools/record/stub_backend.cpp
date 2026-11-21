// The recorder's Backend where it does not measure and draw with CoreGraphics: every answer is a fixed function of the calls, so a recording made with it is the same on every system.
#include "backend.h"
#include "png.h"
#include "wire.h"

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace {

using namespace twlite;

constexpr uint64_t FNV_OFFSET = 0xcbf29ce484222325ull;
constexpr uint64_t FNV_PRIME = 0x100000001b3ull;

uint64_t fnv1a(uint64_t hash, const std::string& bytes) {
    for (const char c : bytes) {
        hash ^= uint8_t(c);
        hash *= FNV_PRIME;
    }
    return hash;
}

// SplitMix64's finaliser: the pixel a canvas shows at one place.
uint64_t mixed(uint64_t z) {
    z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ull;
    z = (z ^ (z >> 27)) * 0x94d049bb133111ebull;
    return z ^ (z >> 31);
}

// The CoreGraphics backend's lenient base64: other characters are skipped, '-' and '_' read as '+' and '/'.
std::string base64_decoded(const std::string& in) {
    int table[256];
    for (int& t : table) t = -1;
    const char* alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (int i = 0; i < 64; i++) table[uint8_t(alphabet[i])] = i;
    table[uint8_t('-')] = 62;
    table[uint8_t('_')] = 63;
    std::string out;
    out.reserve(in.size() * 3 / 4);
    int value = 0, bits = -8;
    for (const char c : in) {
        const int t = table[uint8_t(c)];
        if (t == -1) continue;
        value = (value << 6) + t;
        bits += 6;
        if (bits >= 0) {
            out.push_back(char((value >> bits) & 0xFF));
            bits -= 8;
        }
    }
    return out;
}

// The CoreGraphics backend's percent-decoding of a data: URL that is not base64.
std::string percent_decoded(const std::string& in) {
    std::string out;
    for (size_t i = 0; i < in.size(); i++) {
        if (in[i] == '%' && i + 2 < in.size()) {
            out.push_back(char(std::strtol(in.substr(i + 1, 2).c_str(), nullptr, 16)));
            i += 2;
        } else {
            out.push_back(in[i]);
        }
    }
    return out;
}

// Fonts measured as renderer_oracle's stub measures (8 px a byte, its metrics), images sized by their headers, canvas pixels a hash of what was drawn since the bitmap was made.
class StubBackend final : public Backend {
public:
    int create_font(const std::string&, float size_px, int, bool, const std::string&, FontMetrics& out) override {
        out = metrics(size_px);
        return ++next_font_;
    }
    int create_canvas_font(const std::string&, float size_px, int, bool, FontMetrics& out) override {
        out = metrics(size_px);
        return ++next_font_;
    }
    void delete_font(int) override {}
    float text_width(int, const std::string& text) override { return 8.0f * float(text.size()); }

    // A data: URL as the CoreGraphics backend reads it, an SVG refused; the size its PNG, GIF, JPEG or WebP header declares, else refused.
    bool load_image(int, const std::string& url, float& w, float& h) override {
        const size_t comma = url.find(',');
        if (url.rfind("data:", 0) != 0 || comma == std::string::npos) return false;
        const std::string meta = url.substr(5, comma - 5);
        const std::string payload = url.substr(comma + 1);
        if (meta.find("svg") != std::string::npos) return false;
        const std::string bytes = meta.find(";base64") != std::string::npos ? base64_decoded(payload) : percent_decoded(payload);
        uint32_t iw = 0, ih = 0;
        if (bytes.empty() || !image_dimensions(reinterpret_cast<const uint8_t*>(bytes.data()), bytes.size(), iw, ih) || iw == 0 || ih == 0) return false;
        w = float(iw);
        h = float(ih);
        return true;
    }

    // The bitmap is made on the first run and remade transparent at another size or a Reset; what is drawn after folds into its hash.
    void canvas_run(int id, int w, int h, const std::vector<CanvasOp>& ops) override {
        if (w <= 0 || h <= 0) {
            canvases_.erase(id);
            return;
        }
        Canvas& canvas = canvases_[id];
        if (canvas.w != w || canvas.h != h) canvas = Canvas{w, h, 0};
        size_t from = 0;
        for (size_t i = 0; i < ops.size(); i++) {
            if (ops[i].kind != CanvasOp::Reset) continue;
            from = i + 1;
            canvas.drawn = 0;
        }
        if (from == ops.size()) return;
        std::string bytes;
        if (!wire::encode(std::vector<CanvasOp>(ops.begin() + long(from), ops.end()), wire::Caps(), bytes)) bytes = "a batch past the wire's caps";
        canvas.drawn = fnv1a(canvas.drawn ? canvas.drawn : FNV_OFFSET, bytes);
    }

    // As CoreGraphics: zeros past the bitmap, and true; inside it, nothing drawn reads transparent, else each pixel is a hash of the drawing and its place.
    bool canvas_read(int id, int x, int y, int w, int h, std::vector<uint8_t>& rgba) override {
        rgba.assign(size_t(w > 0 ? w : 0) * size_t(h > 0 ? h : 0) * 4, 0);
        const auto it = canvases_.find(id);
        if (it == canvases_.end() || it->second.drawn == 0 || w <= 0 || h <= 0) return true;
        const Canvas& canvas = it->second;
        const int64_t x0 = x > 0 ? x : 0, y0 = y > 0 ? y : 0;
        const int64_t x1 = int64_t(x) + w < canvas.w ? int64_t(x) + w : canvas.w;
        const int64_t y1 = int64_t(y) + h < canvas.h ? int64_t(y) + h : canvas.h;
        for (int64_t row = y0; row < y1; row++) {
            for (int64_t col = x0; col < x1; col++) {
                const uint64_t v = mixed(canvas.drawn ^ (uint64_t(row) << 32 | uint64_t(col)));
                uint8_t* dst = rgba.data() + (size_t(row - y) * size_t(w) + size_t(col - x)) * 4;
                for (int i = 0; i < 4; i++) dst[i] = uint8_t(v >> (8 * i));
            }
        }
        return true;
    }

    void canvas_release(int id) override { canvases_.erase(id); }

private:
    struct Canvas {
        int w = 0, h = 0;
        uint64_t drawn = 0;
    };

    static FontMetrics metrics(float size) {
        FontMetrics m;
        m.ascent = size * 0.8f;
        m.descent = size * 0.2f;
        m.height = size * 1.2f;
        m.x_height = size * 0.5f;
        m.zero_width = 8;
        return m;
    }

    int next_font_ = 0;
    std::map<int, Canvas> canvases_;
};

} // namespace

std::unique_ptr<twlite::Backend> platform_backend() {
    return std::make_unique<StubBackend>();
}

const char* platform_backend_name() {
    return "stub (tools/record/stub_backend.cpp): renderer_oracle's stub fonts, 8 px a byte; images at the size their header declares; canvas pixels a hash of what was drawn";
}
