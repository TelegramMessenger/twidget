#include <litehtml/css_properties.h>
#include <litehtml/style.h>

#include <cstdio>
#include <cstring>
#include <new>
#include <string>
#include <utility>

namespace
{

auto checks = 0;
auto failures = 0;

void Check(bool condition, const std::string& name)
{
    ++checks;
    if(!condition)
    {
        std::printf("FAIL %s\n", name.c_str());
        ++failures;
    }
}

void CheckInteger(
    const litehtml::style& style,
    litehtml::string_id name,
    int expected,
    const std::string& context)
{
    const auto& value = style.get_property(name);
    Check(value.is<int>() && value.get<int>() == expected, context + ": " + litehtml::_s(name));
}

void CheckImage(
    const litehtml::style& style,
    const std::string& expected,
    const std::string& context)
{
    const auto& value = style.get_property(litehtml::_list_style_image_);
    Check(value.is<std::string>() && value.get<std::string>() == expected, context + ": list-style-image");
}

void InvalidDeclarations()
{
    for(const auto invalid : {"123", "1px", "\"none\"", "none auto"})
    {
        auto style = litehtml::style();
        const auto declarations = std::string("list-style-type:square;appearance:none;list-style-type:")
            + invalid + ";appearance:" + invalid;
        style.add(declarations);
        CheckInteger(style, litehtml::_list_style_type_, litehtml::list_style_type_square, declarations);
        CheckInteger(style, litehtml::_appearance_, litehtml::appearance_none, declarations);
    }
    for(const auto invalid : {"123", "1px", "square disc"})
    {
        auto style = litehtml::style();
        const auto declarations = std::string("list-style:square inside url(marker.png);list-style:") + invalid;
        style.add(declarations);
        CheckInteger(style, litehtml::_list_style_type_, litehtml::list_style_type_square, declarations);
        CheckInteger(style, litehtml::_list_style_position_, litehtml::list_style_position_inside, declarations);
        CheckImage(style, "marker.png", declarations);
    }
}

void ImageShorthand()
{
    auto imageOnly = litehtml::style();
    imageOnly.add(std::string("list-style:url(marker.png)"));
    CheckInteger(imageOnly, litehtml::_list_style_type_, litehtml::list_style_type_disc, "URL shorthand");
    CheckInteger(imageOnly, litehtml::_list_style_position_, litehtml::list_style_position_outside, "URL shorthand");
    CheckImage(imageOnly, "marker.png", "URL shorthand");

    for(const auto value : {
        "url(marker.png) inside square", "url(marker.png) square inside",
        "inside url(marker.png) square", "inside square url(marker.png)",
        "square url(marker.png) inside", "square inside url(marker.png)"})
    {
        auto style = litehtml::style();
        const auto declaration = std::string("list-style:") + value;
        style.add(declaration);
        CheckInteger(style, litehtml::_list_style_type_, litehtml::list_style_type_square, declaration);
        CheckInteger(style, litehtml::_list_style_position_, litehtml::list_style_position_inside, declaration);
        CheckImage(style, "marker.png", declaration);
    }
}

void KeywordBounds()
{
    auto style = litehtml::style();
    style.add(std::string("list-style-type:upper-roman;appearance:textarea"));
    CheckInteger(style, litehtml::_list_style_type_, litehtml::list_style_type_upper_roman, "final keywords");
    CheckInteger(style, litehtml::_appearance_, litehtml::appearance_textarea, "final keywords");
    Check(litehtml::list_style_type_strings.back() == "upper-roman", "list keyword table ends at upper-roman");
    Check(litehtml::appearance_strings.back() == "textarea", "appearance keyword table ends at textarea");

    constexpr auto padded = litehtml::split_css_values<2>("valid");
    const auto keywords = litehtml::css_values(padded);
    Check(!keywords.value_index("").has_value(), "empty keyword is rejected even with an unused table entry");
    Check(keywords.value_index("valid") == 0, "nonempty keyword keeps its index");
}

void DeterminateMembers()
{
    // The storage is filled first, as a stack or a reused heap block is, so a member left unset shows.
    alignas(litehtml::css_properties) static unsigned char properties[sizeof(litehtml::css_properties)];
    std::memset(properties, 0x5a, sizeof(properties));
    auto* css = new(properties) litehtml::css_properties;
    Check(css->font_style_value() == litehtml::font_style_normal, "a new computed style has the normal font style");
    css->~css_properties();

    alignas(litehtml::css_token) static unsigned char token[sizeof(litehtml::css_token)];
    const litehtml::css_token ident(litehtml::IDENT, "a");
    std::memset(token, 0x5a, sizeof(token));
    auto* copy = new(token) litehtml::css_token(ident);
    Check(copy->n.number == 0 && copy->n.number_type == litehtml::css_number_integer, "a copied identifier token holds the number zero");
    copy->~css_token();

    litehtml::css_token source(litehtml::IDENT, "a");
    std::memset(token, 0x5a, sizeof(token));
    auto* moved = new(token) litehtml::css_token(std::move(source));
    Check(moved->n.number == 0 && moved->n.number_type == litehtml::css_number_integer, "a moved identifier token holds the number zero");
    moved->~css_token();
}

}

int main()
{
    InvalidDeclarations();
    ImageShorthand();
    KeywordBounds();
    DeterminateMembers();
    std::printf("%d checks, %d failures\n", checks, failures);
    return failures ? 1 : 0;
}
