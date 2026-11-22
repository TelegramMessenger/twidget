use std::collections::BTreeMap;

// The renderer compares these hashes for equality. C++ standard libraries choose their own hash values.
pub fn hash_id(hash: u64, seen: &mut BTreeMap<u64, u64>) -> u64 {
    if cfg!(target_env = "msvc") || hash == 0 {
        return hash;
    }
    let next = seen.len() as u64 + 1;
    *seen.entry(hash).or_insert(next)
}
