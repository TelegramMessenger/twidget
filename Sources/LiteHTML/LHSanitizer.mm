#include "LHContainer.h"

#include <cctype>
#include <string_view>
#include <unordered_map>
#include <unordered_set>

namespace lh = litehtml;

namespace
{

using Set = std::unordered_set<std::string_view>;

std::vector<std::string_view> Words(const char* text)
{
    std::vector<std::string_view> out;
    const char*                   p = text;
    while(*p)
    {
        while(*p && std::isspace(static_cast<unsigned char>(*p)))
            p++;
        const char* start = p;
        while(*p && !std::isspace(static_cast<unsigned char>(*p)))
            p++;
        if(p > start)
            out.emplace_back(start, static_cast<size_t>(p - start));
    }
    return out;
}

Set MakeSet(const char* text)
{
    auto words = Words(text);
    return Set(words.begin(), words.end());
}

std::string Lower(std::string_view s)
{
    std::string out(s);
    for(char& c : out)
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return out;
}

const Set& DroppedTags()
{
    static const Set set = MakeSet(R"(
        script iframe frame frameset object embed link meta base portal noscript set title template applet param
        noembed noframes plaintext xmp listing slot track area map math metadata fencedframe audio video source)");
    return set;
}

const Set& QuietDrops()
{
    static const Set set = MakeSet("script title meta");
    return set;
}

const Set& HTMLTags()
{
    static const Set set = MakeSet(R"(
        html head body div span p a b i u s em strong small big sub sup mark code pre kbd samp var abbr cite q blockquote
        del ins dfn time data address br hr wbr
        h1 h2 h3 h4 h5 h6 header footer main nav section article aside hgroup search
        ul ol li dl dt dd menu
        table thead tbody tfoot tr td th caption colgroup col
        form input button select option optgroup textarea label fieldset legend datalist output progress meter
        details summary dialog figure figcaption picture img canvas
        ruby rt rp bdi bdo center font tt strike nobr style)");
    return set;
}

const std::unordered_set<std::string>& SVGTags()
{
    static const std::unordered_set<std::string> set = [] {
        std::unordered_set<std::string> s;
        for(auto w : Words(R"(
            svg g path rect circle ellipse line polyline polygon text tspan textPath defs symbol use image clipPath mask
            pattern marker linearGradient radialGradient stop filter foreignObject title desc a switch view
            feBlend feColorMatrix feComponentTransfer feComposite feConvolveMatrix feDiffuseLighting feDisplacementMap
            feDistantLight feDropShadow feFlood feFuncA feFuncB feFuncG feFuncR feGaussianBlur feImage feMerge feMergeNode
            feMorphology feOffset fePointLight feSpecularLighting feSpotLight feTile feTurbulence
            animate animateTransform animateMotion mpath style)"))
            s.insert(Lower(w));
        return s;
    }();
    return set;
}

const std::unordered_set<std::string>& AllowedAttributes()
{
    static const std::unordered_set<std::string> set = [] {
        std::unordered_set<std::string> s;
        for(auto w : Words(R"(
            class id title lang dir hidden tabindex role draggable contenteditable spellcheck translate inert popover
            enterkeyhint inputmode autocapitalize slot part name
            type value checked disabled readonly required placeholder min max step minlength maxlength pattern multiple
            selected size rows cols wrap for label accept autocomplete list open colspan rowspan headers scope span
            width height alt align valign bgcolor border cellpadding cellspacing color face nowrap start reversed
            low high optimum datetime novalidate method enctype
            dirname popovertarget popovertargetaction command commandfor style
            viewBox preserveAspectRatio x y x1 y1 x2 y2 cx cy r rx ry d points dx dy rotate transform
            fill fill-opacity fill-rule stroke stroke-width stroke-opacity stroke-linecap stroke-linejoin
            stroke-dasharray stroke-dashoffset stroke-miterlimit opacity display visibility overflow
            clip-path clip-rule mask filter marker marker-start marker-mid marker-end markerWidth markerHeight markerUnits
            refX refY orient text-anchor dominant-baseline alignment-baseline baseline-shift font-family font-size
            font-weight font-style font-variant font-stretch letter-spacing word-spacing text-decoration writing-mode
            textLength lengthAdjust startOffset spacing side
            gradientUnits gradientTransform spreadMethod fx fy fr offset stop-color stop-opacity
            patternUnits patternContentUnits patternTransform clipPathUnits maskUnits maskContentUnits
            pointer-events shape-rendering text-rendering image-rendering color-interpolation color-interpolation-filters
            vector-effect paint-order mix-blend-mode isolation cursor pathLength
            in in2 result mode operator k1 k2 k3 k4 stdDeviation edgeMode kernelMatrix kernelUnitLength order divisor bias
            targetX targetY preserveAlpha surfaceScale diffuseConstant specularConstant specularExponent
            lighting-color flood-color flood-opacity azimuth elevation z pointsAtX pointsAtY pointsAtZ limitingConeAngle
            baseFrequency numOctaves seed stitchTiles scale xChannelSelector yChannelSelector radius values tableValues
            slope intercept amplitude exponent filterUnits primitiveUnits
            attributeName attributeType begin dur end repeatCount repeatDur from to by keyTimes keySplines keyPoints
            calcMode additive accumulate restart path
            version xml:space xml:lang systemLanguage requiredExtensions)"))
            s.insert(Lower(w));
        return s;
    }();
    return set;
}

const Set& URLAttributes()
{
    static const Set set = MakeSet(R"(
        href xlink:href src srcset poster action formaction background ping data cite codebase manifest longdesc
        usemap profile icon archive classid lowsrc dynsrc srcdoc imagesrcset imagesizes attributionsrc)");
    return set;
}

const Set& QuietAttributes()
{
    static const Set set = MakeSet(R"(
        xmlns xmlns:xlink target rel download autofocus nonce is loading decoding crossorigin referrerpolicy
        integrity fetchpriority charset http-equiv content async defer)");
    return set;
}

const Set& PaintAttributes()
{
    static const Set set = MakeSet(R"(
        fill stroke filter mask clip-path marker marker-start marker-mid marker-end cursor color flood-color
        lighting-color stop-color)");
    return set;
}

const Set& FragmentHrefTags()
{
    static const Set set = MakeSet("use lineargradient radialgradient pattern filter textpath mpath");
    return set;
}

const Set& AllowedAtRules()
{
    static const Set set = MakeSet(R"(
        media supports keyframes -webkit-keyframes container layer property counter-style starting-style scope page
        font-feature-values font-palette-values position-try charset)");
    return set;
}

const Set& ImageMIMETypes()
{
    static const Set set = MakeSet(R"(
        image/png image/jpeg image/jpg image/gif image/webp image/avif image/bmp image/x-icon
        image/vnd.microsoft.icon image/svg+xml)");
    return set;
}

const Set& LinkSchemes()
{
    static const Set set = MakeSet("http https chat tg mailto");
    return set;
}

const Set& ImageFunctions()
{
    static const Set set = MakeSet("image-set -webkit-image-set image src cross-fade -webkit-cross-fade");
    return set;
}

std::string_view Trimmed(std::string_view s)
{
    while(!s.empty() && static_cast<unsigned char>(s.front()) <= ' ')
        s.remove_prefix(1);
    while(!s.empty() && static_cast<unsigned char>(s.back()) <= ' ')
        s.remove_suffix(1);
    return s;
}

std::string Scheme(std::string_view url)
{
    size_t i = 0;
    while(i < url.size() && (std::isalnum(static_cast<unsigned char>(url[i])) || url[i] == '+' || url[i] == '-' || url[i] == '.'))
        i++;
    if(i == 0 || i >= url.size() || url[i] != ':' || !std::isalpha(static_cast<unsigned char>(url[0])))
        return {};
    return Lower(url.substr(0, i));
}

bool IsFragment(std::string_view url)
{
    if(url.empty() || url[0] != '#')
        return false;
    for(char c : url.substr(1))
    {
        if(std::isspace(static_cast<unsigned char>(c)) || c == '"' || c == '\'' || c == '(' || c == ')' || c == '\\' ||
           c == '<' || c == '>')
            return false;
    }
    return true;
}

bool AllowedImage(std::string_view raw)
{
    std::string_view url    = Trimmed(raw);
    std::string      scheme = Scheme(url);
    if(scheme == "sf" || scheme == "app")
        return true;
    if(scheme != "data")
        return false;
    std::string_view rest = url.substr(5);
    size_t           end  = rest.find_first_of(";,");
    if(end == std::string_view::npos)
        return false;
    return ImageMIMETypes().count(Lower(Trimmed(rest.substr(0, end)))) != 0;
}

bool AllowedLink(std::string_view raw)
{
    std::string_view url = Trimmed(raw);
    if(IsFragment(url))
        return true;
    std::string scheme = Scheme(url);
    return !scheme.empty() && LinkSchemes().count(scheme) != 0;
}

bool ValidAttributeName(const std::string& name)
{
    if(name.empty() || !(std::isalpha(static_cast<unsigned char>(name[0])) || name[0] == '_'))
        return false;
    for(char c : name)
    {
        if(!(std::isalnum(static_cast<unsigned char>(c)) || c == '_' || c == '.' || c == ':' || c == '-'))
            return false;
    }
    return true;
}

bool ValidTagName(const std::string& name)
{
    if(name.empty() || name.size() > 64 || !std::isalpha(static_cast<unsigned char>(name[0])))
        return false;
    for(char c : name)
    {
        if(!(std::isalnum(static_cast<unsigned char>(c)) || c == '_' || c == '.' || c == '-'))
            return false;
    }
    return true;
}

bool PaintValueAllowed(const std::string& value)
{
    const std::string lower = Lower(value);
    size_t            pos   = 0;
    while((pos = lower.find("url(", pos)) != std::string::npos)
    {
        size_t           close = value.find(')', pos);
        std::string_view inner = Trimmed(std::string_view(value).substr(pos + 4, close == std::string::npos ? std::string::npos : close - pos - 4));
        if(!inner.empty() && (inner.front() == '"' || inner.front() == '\''))
            inner = Trimmed(inner.substr(1, inner.size() >= 2 ? inner.size() - 2 : 0));
        if(!IsFragment(inner))
            return false;
        pos += 4;
    }
    return true;
}

}

void LHContainer::sanitizer_report(const char* kind, const std::string& detail)
{
    auto& entry = m_sanitizer_report[kind];
    entry.first++;
    if(entry.second.size() < 8)
        entry.second.push_back(detail.size() > 120 ? detail.substr(0, 120) + "…" : detail);
}

bool LHContainer::sanitize_element(std::string& tag, bool svg, lh::string_map& attrs)
{
    if(!ValidTagName(tag))
    {
        sanitizer_report("tag-dropped", "<" + tag + ">");
        return false;
    }
    std::string name    = Lower(tag);
    bool        renamed = false;
    if(svg)
    {
        if(name != "title" && DroppedTags().count(name))
            return false;
        if(!SVGTags().count(name))
        {
            sanitizer_report("tag-dropped", "<svg:" + tag + ">");
            return false;
        }
    } else if(DroppedTags().count(name))
    {
        if(!QuietDrops().count(name))
            sanitizer_report("tag-dropped", "<" + name + ">");
        return false;
    } else if(!HTMLTags().count(name))
    {
        sanitizer_report("tag-replaced", "<" + name + "> rendered as <span>");
        attrs["data-tg-tag"] = name;
        tag                  = "span";
        name                 = "span";
        renamed              = true;
    }

    for(auto it = attrs.begin(); it != attrs.end();)
    {
        const std::string& attr = it->first;
        if(!ValidAttributeName(attr))
        {
            it = attrs.erase(it);
            continue;
        }
        std::string key = Lower(attr);
        if(key == "data-tg-tag" && renamed)
        {
            ++it;
            continue;
        }
        if(key.compare(0, 2, "on") == 0)
        {
            sanitizer_report("attr-dropped", name + "[" + key + "]");
            it = attrs.erase(it);
            continue;
        }
        if(URLAttributes().count(key))
        {
            const bool href = key == "href" || key == "xlink:href";
            bool       keep = false;
            if(href && (name == "a" || name == "area"))
                keep = AllowedLink(it->second);
            else if(href && svg && name == "image")
                keep = AllowedImage(it->second);
            else if(href && svg && FragmentHrefTags().count(name))
                keep = IsFragment(Trimmed(it->second));
            else if(key == "src" && (name == "img" || name == "input"))
                keep = AllowedImage(it->second);
            if(!keep)
            {
                sanitizer_report("url-blocked", name + "[" + key + "]=" + it->second);
                it = attrs.erase(it);
                continue;
            }
            ++it;
            continue;
        }
        if(key.compare(0, 8, "data-tg-") == 0)
        {
            it = attrs.erase(it);
            continue;
        }
        if(key.compare(0, 5, "data-") == 0 || key.compare(0, 5, "aria-") == 0)
        {
            ++it;
            continue;
        }
        if(QuietAttributes().count(key))
        {
            it = attrs.erase(it);
            continue;
        }
        if(!AllowedAttributes().count(key))
        {
            sanitizer_report("attr-dropped", name + "[" + key + "]");
            it = attrs.erase(it);
            continue;
        }
        if(svg && PaintAttributes().count(key) && !PaintValueAllowed(it->second))
        {
            sanitizer_report("url-blocked", name + "[" + key + "]=" + it->second);
            it = attrs.erase(it);
            continue;
        }
        ++it;
    }
    return true;
}

static bool DeclarationValueAllowed(const lh::css_token_vector& tokens, std::vector<std::string>& functions, std::string& why)
{
    for(const auto& tok : tokens)
    {
        if(tok.type == lh::URL || tok.type == lh::BAD_URL)
        {
            if(tok.type == lh::BAD_URL || !AllowedImage(tok.str()))
            {
                why = "url(" + tok.str() + ")";
                return false;
            }
            continue;
        }
        if(tok.type == lh::STRING && !functions.empty() && ImageFunctions().count(functions.back()))
        {
            if(!AllowedImage(tok.str()))
            {
                why = "\"" + tok.str() + "\" in " + functions.back() + "()";
                return false;
            }
            continue;
        }
        if(tok.type == lh::CV_FUNCTION)
        {
            std::string fn = Lower(tok.name());
            if(fn == "expression")
            {
                why = "expression()";
                return false;
            }
            if(fn == "url")
            {
                if(tok.value.size() != 1 || tok.value[0].type != lh::STRING || !AllowedImage(tok.value[0].str()))
                {
                    why = "url() with a blocked source or modifiers";
                    return false;
                }
                continue;
            }
            if((fn == "var" || fn == "env" || fn == "attr"))
            {
                for(const auto& f : functions)
                {
                    if(ImageFunctions().count(f))
                    {
                        why = fn + "() inside " + f + "()";
                        return false;
                    }
                }
            }
            functions.push_back(fn);
            bool ok = DeclarationValueAllowed(tok.value, functions, why);
            functions.pop_back();
            if(!ok)
                return false;
            continue;
        }
        if(tok.is_component_value() && !DeclarationValueAllowed(tok.value, functions, why))
            return false;
    }
    return true;
}

bool LHContainer::sanitize_declaration(const std::string& name, const lh::css_token_vector& value)
{
    if(m_trusted_styles)
        return true;
    if(name == "behavior" || name == "-moz-binding")
    {
        sanitizer_report("css-dropped", name);
        return false;
    }
    bool simple = true;
    for(const auto& tok : value)
    {
        if(tok.type == lh::URL || tok.type == lh::BAD_URL || tok.is_component_value())
        {
            simple = false;
            break;
        }
    }
    if(simple)
        return true;
    std::vector<std::string> functions;
    std::string              why;
    if(DeclarationValueAllowed(value, functions, why))
        return true;
    sanitizer_report(why.compare(0, 3, "url") == 0 || why.front() == '"' ? "url-blocked" : "css-dropped", name + ": " + why);
    return false;
}

bool LHContainer::sanitize_at_rule(const std::string& name)
{
    if(m_trusted_styles)
        return true;
    if(AllowedAtRules().count(name))
        return true;
    sanitizer_report("css-dropped", "@" + name);
    return false;
}

void LHContainer::sanitizer_limit(const char* what)
{
    sanitizer_report("limit", what);
}
