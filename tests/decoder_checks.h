// Replaces the global operator new and delete to measure decodes, so only a decoder test program's one source includes it.
#pragma once

#include "wire_compare.h"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <iterator>
#include <new>
#include <string>
#include <string_view>

namespace {

/// What the run being measured has asked the allocator for, the refused request included; armed around one run at a time.
struct Scope {
    bool armed = false;
    bool over = false;
    size_t budget = 0;
    size_t asked = 0;
    size_t granted = 0;
    size_t count = 0;
};

Scope scope;

/// Counts a request while armed, and refuses before malloc the one that would take the run past its budget.
void* counted(size_t n) noexcept {
    if (scope.armed) {
        scope.count++;
        const bool within = !scope.over && n <= scope.budget - scope.granted;
        scope.asked = n > SIZE_MAX - scope.asked ? SIZE_MAX : scope.asked + n;
        if (!within) {
            scope.over = true;
            return nullptr;
        }
        scope.granted += n;
    }
    return std::malloc(n ? n : 1);
}

} // namespace

void* operator new(size_t n) {
    if (void* p = counted(n)) return p;
    throw std::bad_alloc();
}

void* operator new[](size_t n) {
    if (void* p = counted(n)) return p;
    throw std::bad_alloc();
}

void* operator new(size_t n, const std::nothrow_t&) noexcept {
    return counted(n);
}

void* operator new[](size_t n, const std::nothrow_t&) noexcept {
    return counted(n);
}

void operator delete(void* p) noexcept {
    std::free(p);
}

void operator delete[](void* p) noexcept {
    std::free(p);
}

void operator delete(void* p, size_t) noexcept {
    std::free(p);
}

void operator delete[](void* p, size_t) noexcept {
    std::free(p);
}

void operator delete(void* p, const std::nothrow_t&) noexcept {
    std::free(p);
}

void operator delete[](void* p, const std::nothrow_t&) noexcept {
    std::free(p);
}

namespace {

int checks = 0, failures = 0;

/// What the checks are at, for a failure found deep in a run of them.
std::string context;

void check(bool ok, const std::string& what) {
    checks++;
    if (!ok) {
        std::printf("FAIL %s\n", what.c_str());
        failures++;
    }
}

/// check() for the checks made by the thousand: the message is put together only for a failure.
template <class What> void verify(bool ok, What what) {
    if (ok) {
        checks++;
        return;
    }
    check(false, what());
}

/// The most a decode of `input` bytes may ask for in all: 128 bytes for each and 1024 more, which calibrate_allocations() checks against what this build's sizes and its library's bookkeeping need.
constexpr size_t PER_BYTE = 128, FIXED = 1024;

size_t budget(size_t input) {
    return input > (SIZE_MAX - FIXED) / PER_BYTE ? SIZE_MAX : PER_BYTE * input + FIXED;
}

/// One run in the scope: what it returned, whether it threw, and what it asked for.
struct Measured {
    bool ok = false;
    bool threw = false;
    Scope scope;
};

/// Arms the scope for one run and disarms it however the run ends.
class Armed {
public:
    explicit Armed(size_t input) {
        scope = Scope();
        scope.budget = budget(input);
        scope.armed = true;
    }
    ~Armed() { scope.armed = false; }
};

template <class Run> Measured measure(size_t input, Run run) {
    Measured m;
    {
        Armed armed(input);
        try {
            m.ok = run();
        } catch (...) {
            m.threw = true;
        }
    }
    m.scope = scope;
    return m;
}

/// The decode that has come nearest some measure, and where the checks were.
struct Extreme {
    double value = 0;
    size_t asked = 0, input = 0;
    std::string where;

    void note(double v, const Measured& m, size_t n) {
        if (v <= value) return;
        value = v;
        asked = m.scope.asked;
        input = n;
        where = context;
    }
};

size_t measured_decodes = 0;
/// Of all decodes the nearest its budget, and of those of 256 bytes or more the most per input byte.
Extreme nearest_budget, most_per_byte;
Measured last_decode;

/// Runs one decode in the scope: past its budget or throwing, it fails here and never passes as a refusal.
template <class Decode> bool in_scope(std::string_view bytes, Decode decode) {
    const Measured m = measure(bytes.size(), decode);
    last_decode = m;
    measured_decodes++;
    nearest_budget.note(double(m.scope.asked) / double(budget(bytes.size())), m, bytes.size());
    if (bytes.size() >= 256) most_per_byte.note(double(m.scope.asked) / double(bytes.size()), m, bytes.size());
    verify(!m.threw && !m.scope.over, [&] {
        return context + ": a decode of " + std::to_string(bytes.size()) + " bytes " + (m.threw ? "threw" : "returned") + " after asking for "
            + std::to_string(m.scope.asked) + " bytes against a budget of " + std::to_string(budget(bytes.size()));
    });
    return m.ok && !m.threw;
}

/// The precedent's 64-bit LCG: the high 31 bits of each step.
struct Lcg {
    uint64_t state = 0;

    uint32_t next() {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        return uint32_t(state >> 33);
    }
    size_t below(size_t n) { return n ? size_t(next()) % n : 0; }
};

/// u32 values at the edges of what a count, a length, a reference, a mask or float bits can be.
constexpr uint32_t EDGES[] = {0, 1, 2, 3, 4, 5, 0x7f, 0x80, 0xff, 0x100, 0xffff, 0x10000, 0x10000000, 0x20000000, 0x40000000, 0x7fffffff, 0x80000000, 0xfffffffe, 0xffffffff};

/// One to three random changes to `bytes`, and what they were.
std::string mutate(Lcg& random, std::string& bytes) {
    std::string done;
    const size_t edits = 1 + random.below(3);
    for (size_t e = 0; e < edits; e++) {
        const size_t at = random.below(bytes.size());
        switch (random.below(7)) {
        case 0:
            done += " flip@" + std::to_string(at);
            if (!bytes.empty()) bytes[at] = char(uint8_t(bytes[at]) ^ (1u << random.below(8)));
            break;
        case 1:
            done += " byte@" + std::to_string(at);
            if (!bytes.empty()) bytes[at] = char(random.next());
            break;
        case 2:
            if (bytes.size() >= 4) {
                const size_t p = random.below(bytes.size() - 3);
                const uint32_t v = EDGES[random.below(std::size(EDGES))];
                done += " u32@" + std::to_string(p) + "=" + hex(v);
                for (size_t i = 0; i < 4; i++) bytes[p + i] = char((v >> (8 * i)) & 0xff);
            }
            break;
        case 3: {
            const size_t n = 1 + random.below(4);
            done += " delete@" + std::to_string(at) + "+" + std::to_string(n);
            if (!bytes.empty()) bytes.erase(at, n);
            break;
        }
        case 4: {
            const size_t p = random.below(bytes.size() + 1), n = 1 + random.below(4);
            done += " insert@" + std::to_string(p) + "+" + std::to_string(n);
            std::string added;
            for (size_t i = 0; i < n; i++) added.push_back(char(random.next()));
            bytes.insert(p, added);
            break;
        }
        case 5: {
            const size_t n = std::min(1 + random.below(8), bytes.size() - at);
            done += " repeat@" + std::to_string(at) + "+" + std::to_string(n);
            if (!bytes.empty()) bytes.insert(at, bytes.substr(at, n));
            break;
        }
        default:
            done += " cut@" + std::to_string(at);
            bytes.resize(at);
            break;
        }
    }
    return done;
}

constexpr int ROUNDS = 3000;

/// How many changed values the decoder took and how many it refused.
struct Tally {
    size_t accepted = 0, refused = 0;
};

void print_extreme(const char* what, const Extreme& e) {
    std::printf("allocation: %s: %zu bytes asked for %zu input bytes (%.1f per byte, budget %zu), in %s\n", what, e.asked, e.input, e.input ? double(e.asked) / double(e.input) : 0.0,
        budget(e.input), e.where.c_str());
}

} // namespace
