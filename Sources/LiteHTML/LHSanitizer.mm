#import "LHSanitizer.h"

#include "LHContainer.h"
#include "lhx_sanitizer.h"

namespace lh = litehtml;

@implementation LHSanitizer

+ (void)setLinkSchemes:(NSArray<NSString *> *)linkSchemes imageSchemes:(NSArray<NSString *> *)imageSchemes
{
    auto policy = std::make_shared<lhx::SanitizerPolicy>(lhx::SanitizerPolicy::standard());
    policy->linkSchemes.clear();
    for(NSString *scheme in linkSchemes)
        policy->linkSchemes.insert(scheme.lowercaseString.UTF8String);
    policy->imageSchemes.clear();
    for(NSString *scheme in imageSchemes)
        policy->imageSchemes.insert(scheme.lowercaseString.UTF8String);
    lhx::Sanitizer::setCurrent(policy);
}

@end

void LHContainer::sanitizer_report(const char* kind, const std::string& detail)
{
    auto& entry = m_sanitizer_report[kind];
    entry.first++;
    if(entry.second.size() < 8)
        entry.second.push_back(detail.size() > 120 ? detail.substr(0, 120) + "…" : detail);
}

bool LHContainer::sanitize_element(std::string& tag, bool svg, lh::string_map& attrs)
{
    return m_sanitizer.element(tag, svg, attrs, [this](const char* kind, const std::string& detail) { sanitizer_report(kind, detail); });
}

bool LHContainer::sanitize_declaration(const std::string& name, const lh::css_token_vector& value)
{
    if(m_trusted_styles)
        return true;
    return m_sanitizer.declaration(name, value, [this](const char* kind, const std::string& detail) { sanitizer_report(kind, detail); });
}

bool LHContainer::sanitize_at_rule(const std::string& name)
{
    if(m_trusted_styles)
        return true;
    return m_sanitizer.atRule(name, [this](const char* kind, const std::string& detail) { sanitizer_report(kind, detail); });
}

void LHContainer::sanitizer_limit(const char* what)
{
    sanitizer_report("limit", what);
}
