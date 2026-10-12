#pragma once

#include <litehtml.h>
#include "lhx_sanitizer.h"
#include "lhx_painter.h"
#include "include/LHRenderList.h"
#include <CoreGraphics/CoreGraphics.h>
#include <CoreText/CoreText.h>

#include <array>
#include <functional>
#include <map>
#include <string>
#include <unordered_map>
#include <vector>

#ifdef __OBJC__
@class NSImage;
void LHRegisterAppImage(NSString* name, NSImage* image);
void LHSetImageProvider(NSImage* (^provider)(NSString* url));
NSImage* (^LHImageProvider(void))(NSString* url);
void LHForgetImage(NSString* url);
#endif

struct LHTextRun
{
    std::string text;
    CTFontRef   font;
    CGRect      frame;
};

struct LHDrawOp
{
    CGRect   bounds;
    uint64_t hash;
    uint64_t shape;
};

using LHGPUOp       = lhx::RenderOp;
using LHGlyphSlot   = lhx::AtlasSlot;
using LHGPURecorder = lhx::RenderRecorder;

class LHContainer : public litehtml::document_container
{
  public:
    static size_t cached_font_count();
    static size_t raster_cache_bytes();
    static void   purge_caches();
    LHContainer();
    ~LHContainer() override;

    void set_viewport(const litehtml::position& vp) { m_viewport = vp; }
    void set_default_font_size(float px) { m_default_font_size = px; }
    void set_text_runs(std::vector<LHTextRun>* runs) { m_runs = runs; }
    void set_draw_ops(std::vector<LHDrawOp>* ops) { m_ops = ops; }
    void set_clip_culling(bool enabled) { m_clip_culling = enabled; }
    void set_painter(lhx::Painter* painter) { m_painter = painter; }
    void set_gpu_recorder(LHGPURecorder* recorder)
    {
        flush_capture();
        m_gpu = recorder;
    }
    bool clip_culling() const { return m_clip_culling; }
    bool rasterizes() const { return !m_ops && !m_runs && !m_painter; }

    std::function<void(const std::string& url)> on_anchor;
    std::function<void(const std::string& cursor)> on_cursor;
    std::function<void(const std::string& action, const std::string& value)> on_action;

    litehtml::uint_ptr create_font(const litehtml::font_description& descr, const litehtml::document* doc,
                                   litehtml::font_metrics* fm) override;
    void               delete_font(litehtml::uint_ptr hFont) override;
    litehtml::pixel_t  text_width(const char* text, litehtml::uint_ptr hFont) override;
    void draw_text(litehtml::uint_ptr hdc, const char* text, litehtml::uint_ptr hFont, litehtml::web_color color,
                   const litehtml::position& pos) override;
    litehtml::pixel_t pt_to_px(float pt) const override;
    litehtml::pixel_t get_default_font_size() const override;
    const char*       get_default_font_name() const override;
    void              draw_list_marker(litehtml::uint_ptr hdc, const litehtml::list_marker& marker) override;
    void              load_image(const char* src, const char* baseurl, bool redraw_on_ready) override;
    void              get_image_size(const char* src, const char* baseurl, litehtml::size& sz) override;
    void draw_image(litehtml::uint_ptr hdc, const litehtml::background_layer& layer, const std::string& url,
                    const std::string& base_url) override;
    void draw_solid_fill(litehtml::uint_ptr hdc, const litehtml::background_layer& layer,
                         const litehtml::web_color& color) override;
    void draw_linear_gradient(litehtml::uint_ptr hdc, const litehtml::background_layer& layer,
                              const litehtml::background_layer::linear_gradient& gradient) override;
    void draw_radial_gradient(litehtml::uint_ptr hdc, const litehtml::background_layer& layer,
                              const litehtml::background_layer::radial_gradient& gradient) override;
    void draw_conic_gradient(litehtml::uint_ptr hdc, const litehtml::background_layer& layer,
                             const litehtml::background_layer::conic_gradient& gradient) override;
    void draw_borders(litehtml::uint_ptr hdc, const litehtml::borders& borders, const litehtml::position& draw_pos,
                      bool root) override;
    void draw_box_shadow(litehtml::uint_ptr hdc, const litehtml::position& box, const litehtml::border_radiuses& radius,
                         const litehtml::shadow_paint& shadow) override;
    void begin_group(litehtml::uint_ptr hdc, const litehtml::paint_group& group) override;
    void draw_path(litehtml::uint_ptr hdc, const litehtml::vector_path& path) override;
    void end_group(litehtml::uint_ptr hdc) override;
    bool draw_cached(litehtml::uint_ptr hdc, const litehtml::position& box, uint64_t key, const std::function<void(litehtml::uint_ptr)>& paint) override;
    void draw_isolated(litehtml::uint_ptr hdc, const litehtml::position& box, const std::function<void(litehtml::uint_ptr)>& paint) override;
    bool sanitize_element(std::string& tag, bool svg, litehtml::string_map& attrs) override;
    bool sanitize_declaration(const std::string& name, const litehtml::css_token_vector& value) override;
    bool sanitize_at_rule(const std::string& name) override;
    void sanitizer_limit(const char* what) override;
    void trusted_styles(bool trusted) override { m_trusted_styles = trusted; }

    using SanitizerReport = std::map<std::string, std::pair<int, std::vector<std::string>>>;
    const SanitizerReport& sanitizer_report() const { return m_sanitizer_report; }
    void                   clear_sanitizer_report() { m_sanitizer_report.clear(); }

    void set_caption(const char* caption) override {}
    void set_base_url(const char* base_url) override {}
    void link(const std::shared_ptr<litehtml::document>& doc, const litehtml::element::ptr& el) override {}
    void on_anchor_click(const char* url, const litehtml::element::ptr& el) override;
    bool on_element_click(const litehtml::element::ptr& el) override;
    void on_mouse_event(const litehtml::element::ptr& el, litehtml::mouse_event event) override {}
    void set_cursor(const char* cursor) override;
    void transform_text(std::string& text, litehtml::text_transform tt) override;
    void import_css(std::string& text, const std::string& url, std::string& baseurl) override {}
    void set_clip(const litehtml::position& pos, const litehtml::border_radiuses& bdr_radius) override;
    void del_clip() override;
    void get_viewport(litehtml::position& viewport) const override;
    litehtml::element::ptr create_element(const char* tag_name, const litehtml::string_map& attributes,
                                          const std::shared_ptr<litehtml::document>& doc) override
    {
        return nullptr;
    }
    void get_media_features(litehtml::media_features& media) const override;
    void get_language(std::string& language, std::string& culture) const override;

  private:
    struct Font;
    struct Clip
    {
        litehtml::position        box;
        litehtml::border_radiuses radius;
        CGAffineTransform         matrix;
    };

    struct Group
    {
        CGAffineTransform matrix;
        float             opacity;
        bool              hidden;
        bool              raster;
        bool              layer;
    };

    struct OpHash;
    void sanitizer_report(const char* kind, const std::string& detail);
    SanitizerReport m_sanitizer_report;
    bool            m_trusted_styles = false;
    lhx::Sanitizer  m_sanitizer{nullptr};
    lhx::Painter*   m_painter = nullptr;
    void apply_clips(CGContextRef ctx, CGRect bounds) const;
    void record_op(OpHash& hash, CGRect ink, bool capture = true);
    void flush_capture();
    bool record_solid(const litehtml::background_layer& layer, const litehtml::web_color& color);
    bool record_rounded(CGRect rect, const litehtml::border_radiuses& radius, const litehtml::web_color& color);
    void push_mask(const LHGlyphSlot& slot, CGRect device, const uint8_t* rgba);
    bool record_glyphs(const char* text, Font* f, const litehtml::web_color& color, const litehtml::position& pos, CGRect ink);
    const uint8_t* device_color(const litehtml::web_color& color);
    void clip_background_layer(CGContextRef ctx, const litehtml::background_layer& layer) const;
    void for_each_tile(const litehtml::background_layer& layer, CGSize tile,
                       const std::function<void(CGRect tile)>& fn) const;

    std::vector<Clip>        m_clips;
    std::vector<Group>       m_groups;
    int                      m_hidden_depth = 0;

    CGAffineTransform group_matrix() const { return m_groups.empty() ? CGAffineTransformIdentity : m_groups.back().matrix; }
    std::vector<LHTextRun>*  m_runs = nullptr;
    std::vector<LHDrawOp>*   m_ops  = nullptr;
    LHGPURecorder*           m_gpu  = nullptr;
    CGContextRef             m_capture = nullptr;
    uint64_t                 m_capture_key = 0;
    CGRect                   m_capture_device = CGRectNull;
    std::unordered_map<uint32_t, std::array<uint8_t, 4>> m_device_colors;
    CGColorSpaceRef          m_device_colors_space = nullptr;
    bool                     m_clip_culling = true;
    litehtml::position m_viewport;
    float              m_default_font_size = 15.0f;
};
