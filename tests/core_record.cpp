// twidget_core_record: tests/core_tests.cpp unchanged, every renderer it makes recorded call by call into the file it is given, for twidget_replay.
#include "recording_renderer.h"

// What core_tests.cpp includes comes first, so the two macros below rename its own text only.
#include "css.h"
#include "filters.h"
#include "png.h"
#include "renderer.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <set>
#include <string>
#include <thread>
#include <vector>

#if defined(__APPLE__)
#include <malloc/malloc.h>
#endif

#define Renderer RecordingRenderer
#define main core_main
#include "core_tests.cpp"
#undef main
#undef Renderer

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: twidget_core_record <recording>\n");
        return 2;
    }
    if (!record::open(argv[1], R"({"source":"tests/core_tests.cpp","recorded by":"twidget_core_record"})")) return 2;
    const int result = core_main();
    const bool closed = record::close();
    std::printf("core_tests returned %d\n", result);
    return closed ? (result ? 1 : 0) : 2;
}
