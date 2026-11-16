// The recorder's CoreGraphics Backend, made in a translation unit of its own: MacTypes.h's global Rect would meet the test helpers' `using namespace twlite`.
#include "coregraphics.h"

#include <memory>

std::unique_ptr<twlite::Backend> platform_backend() {
    return std::make_unique<twlite::CGBackend>();
}
