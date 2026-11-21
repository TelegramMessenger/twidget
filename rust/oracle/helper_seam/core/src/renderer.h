// What tgw_renderer_rules.h reads of TwidgetCore's renderer, for renderer_rules_oracle: the build counts and the clock, set by the cases.
#pragma once

namespace twlite {

struct BuildStats {
    int builds = 0;
    int patched = 0;
};

class Renderer {
public:
    const BuildStats& stats() const { return stats_; }
    static double clock();

    BuildStats stats_;
};

} // namespace twlite
