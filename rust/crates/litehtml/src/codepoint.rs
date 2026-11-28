//! `codepoint.cpp`: the URL code point classes of RFC 3986, as bit tables over ASCII.

fn lookup(table: &[u32; 4], c: u8) -> bool {
    table[usize::from(c >> 5)] & (1 << (c & 0x1f)) != 0
}

/// `is_ascii_codepoint`.
pub fn is_ascii_codepoint(c: u8) -> bool {
    c < 128
}

/// `is_url_reserved_codepoint`: https://datatracker.ietf.org/doc/html/rfc3986#section-2.2
pub fn is_url_reserved_codepoint(c: u8) -> bool {
    const RESERVED_LOOKUP: [u32; 4] = [0x00000000, 0xac009fda, 0x28000001, 0x00000000];
    if !is_ascii_codepoint(c) {
        return false;
    }
    lookup(&RESERVED_LOOKUP, c)
}

/// `is_url_scheme_codepoint`: https://datatracker.ietf.org/doc/html/rfc3986#section-3.1
pub fn is_url_scheme_codepoint(c: u8) -> bool {
    const SCHEME_LOOKUP: [u32; 4] = [0x00000000, 0x03ff6800, 0x07fffffe, 0x07fffffe];
    if !is_ascii_codepoint(c) {
        return false;
    }
    lookup(&SCHEME_LOOKUP, c)
}
