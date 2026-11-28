#pragma once

#include <cstdint>
#include <unordered_map>

inline uint64_t hash_id(uint64_t hash, std::unordered_map<uint64_t, uint64_t>& seen) {
#ifdef _MSC_VER
    return hash;
#else
    if (!hash) return 0;
    return seen.emplace(hash, seen.size() + 1).first->second;
#endif
}
