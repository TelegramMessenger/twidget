// What tgw_renderer_rules.h and tgw_cursor.cpp read of TwidgetCore's renderer, for renderer_rules_oracle and cursor_oracle: the build counts, the clock, the HTML and the viewport, set by the cases.
#pragma once

#include <string>

namespace twlite {

struct BuildStats {
    int builds = 0;
    int patched = 0;
};

class Renderer {
public:
    const BuildStats& stats() const { return stats_; }
    static double clock();
    const std::string& last_html() { return html_; }
    const std::string& master_css() const { return master_; }
    double width() const { return width_; }
    int viewport_height() const { return height_; }

    BuildStats stats_;
    std::string html_;
    std::string master_;
    double width_ = 0.;
    int height_ = 0;
};

} // namespace twlite
