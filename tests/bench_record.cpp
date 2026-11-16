// twidget_bench_record: tests/bench.cpp unchanged, seven of its scenarios run in one process and every renderer recorded call by call into the file it is given, for twidget_replay.
#include "recording_renderer.h"

// What bench.cpp includes comes first, so the two macros below rename its own text only.
#include "renderer.h"

#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

#define Renderer RecordingRenderer
#define main bench_main
#include "bench.cpp"
#undef main
#undef Renderer

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: twidget_bench_record <recording>\n");
        return 2;
    }
    if (!record::open(argv[1], R"({"source":"tests/bench.cpp","recorded by":"twidget_bench_record"})")) return 2;
    const char* const modes[][3] = {
        {"mixed", "60", "10"}, {"transform", "60", "10"}, {"swarm", "60", "10"}, {"css", "60", "10"}, {"drag", "60", "10"}, {"drag-doc", "60", "10"}, {"canvas", "50", "10"}};
    std::string error;
    for (const auto& mode : modes) {
        std::string name = mode[0], count = mode[1], frames = mode[2];
        record::label(name + " " + count + " " + frames);
        char* args[] = {argv[0], name.data(), count.data(), frames.data(), nullptr};
        if (const int result = bench_main(4, args); result != 0) {
            error = "bench " + name + " returned " + std::to_string(result);
            break;
        }
    }
    return record::close(error) ? 0 : 2;
}
