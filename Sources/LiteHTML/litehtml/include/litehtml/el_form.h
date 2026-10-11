#ifndef LITEHTML_EL_FORM_H
#define LITEHTML_EL_FORM_H

#include "html_tag.h"
#include "document.h"

namespace litehtml
{
    class el_input : public html_tag
    {
        std::string m_type;
        bool        m_checked  = false;
        float       m_fraction = 0.5f;
        web_color   m_swatch   = web_color::black;

        bool is_text_like() const;
        bool is_button_like() const;

      public:
        explicit el_input(const document::ptr& doc);

        void parse_attributes() override;
        void compute_styles(bool recursive = true) override;
        void draw(uint_ptr hdc, pixel_t x, pixel_t y, const position* clip,
                  const std::shared_ptr<render_item>& ri) override;
    };

    class el_select : public html_tag
    {
      public:
        explicit el_select(const document::ptr& doc);

        void parse_attributes() override;
        void compute_styles(bool recursive = true) override;
    };

    class el_button : public html_tag
    {
      public:
        explicit el_button(const document::ptr& doc);

        void compute_styles(bool recursive = true) override;
    };
} // namespace litehtml

#endif // LITEHTML_EL_FORM_H
