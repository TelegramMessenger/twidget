#ifndef LITEHTML_EL_SVG_H
#define LITEHTML_EL_SVG_H

#include "html_tag.h"
#include "document.h"

namespace litehtml
{
    class el_svg : public html_tag
    {
        bool view_box(float box[4]) const;
        uint64_t m_last_paint_key = 0;
        int      m_stable_draws   = 0;
        bool     cacheable_paint_key(uint64_t& key) const;

      public:
        explicit el_svg(const document::ptr& doc);

        bool is_replaced() const override;
        void parse_attributes() override;
        void get_content_size(size& sz, pixel_t max_width) override;
        void draw(uint_ptr hdc, pixel_t x, pixel_t y, const position* clip,
                  const std::shared_ptr<render_item>& ri) override;

        std::shared_ptr<render_item> create_render_item(const std::shared_ptr<render_item>& parent_ri) override;
    };
} // namespace litehtml

#endif // LITEHTML_EL_SVG_H
