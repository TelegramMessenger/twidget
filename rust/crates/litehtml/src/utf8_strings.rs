//! `utf8_strings.cpp`: UTF-8 one code point at a time, as litehtml reads and writes it. The reader checks
//! nothing but the lead byte: continuation bytes are taken as they come (their top bits dropped), a lead
//! byte past 0xf7 or a stray continuation byte is U+FFFD, and a NUL, or the end, stops it without moving,
//! also in the middle of a sequence. A sequence can therefore decode to 0 (`C0 80`), which its callers take
//! for the end.

/// `read_utf8_char(str, index)`: the code point at `index` and `index` past it.
pub fn read_utf8_char(str: &[u8], index: &mut usize) -> u32 {
    let mut getb = || -> u32 {
        // Past the end is the terminator `std::string` keeps there.
        match str.get(*index) {
            None | Some(0) => 0,
            Some(&b) => {
                *index += 1;
                u32::from(b)
            }
        }
    };

    let b1 = getb();

    // Determine whether we are dealing with a one-, two-, three-, or four-byte sequence.
    if (b1 & 0x80) == 0 {
        // 1-byte sequence: 000000000xxxxxxx = 0xxxxxxx
        return b1;
    }
    if (b1 & 0xe0) == 0xc0 {
        // 2-byte sequence: 00000yyyyyxxxxxx = 110yyyyy 10xxxxxx
        let mut r = (b1 & 0x1f) << 6;
        r |= getb() & 0x3f;
        return r;
    }
    if (b1 & 0xf0) == 0xe0 {
        // 3-byte sequence: zzzzyyyyyyxxxxxx = 1110zzzz 10yyyyyy 10xxxxxx
        let mut r = (b1 & 0x0f) << 12;
        r |= (getb() & 0x3f) << 6;
        r |= getb() & 0x3f;
        return r;
    }
    if (b1 & 0xf8) == 0xf0 {
        // 4-byte sequence: uuuzzzzzzyyyyyyxxxxxx = 11110uuu 10zzzzzz 10yyyyyy 10xxxxxx
        let b2 = getb() & 0x3f;
        let b3 = getb() & 0x3f;
        let b4 = getb() & 0x3f;
        return ((b1 & 7) << 18) | (b2 << 12) | (b3 << 6) | b4;
    }

    0xFFFD
}

/// `prev_utf8_char(str, index)`: `index` back over continuation bytes to the byte before them.
pub fn prev_utf8_char(str: &[u8], index: &mut usize) {
    while *index != 0 {
        *index -= 1;
        if str.get(*index).copied().unwrap_or(0) >> 6 != 0b10 {
            break;
        }
    }
}

/// `is_valid_utf8`: well-formed UTF-8 (no overlong form, no surrogate, nothing past U+10FFFF).
pub fn is_valid_utf8(str: &[u8]) -> bool {
    let mut i = 0;
    while i < str.len() {
        let b = str[i];
        if b < 0x80 {
            i += 1;
            continue;
        }
        let n = if (0xC2..=0xDF).contains(&b) {
            2
        } else if (0xE0..=0xEF).contains(&b) {
            3
        } else if (0xF0..=0xF4).contains(&b) {
            4
        } else {
            0
        };
        if n == 0 || str.len() - i < n {
            return false;
        }
        let second = str[i + 1];
        let low = match b {
            0xE0 => 0xA0,
            0xF0 => 0x90,
            _ => 0x80,
        };
        let high = match b {
            0xED => 0x9F,
            0xF4 => 0x8F,
            _ => 0xBF,
        };
        if second < low || second > high {
            return false;
        }
        for k in 2..n {
            if str[i + k] & 0xC0 != 0x80 {
                return false;
            }
        }
        i += n;
    }
    true
}

/// `append_char(str, code)`: the code point in UTF-8; a surrogate or a value past U+10FFFF adds nothing.
pub fn append_char(str: &mut Vec<u8>, code: u32) {
    if code <= 0x7F {
        str.push(code as u8);
    } else if code <= 0x7FF {
        str.push(((code >> 6) + 192) as u8);
        str.push(((code & 63) + 128) as u8);
    } else if (0xd800..=0xdfff).contains(&code) {
        // error: unexpected surrogate (code is UTF-32, not UTF-16)
    } else if code <= 0xFFFF {
        str.push(((code >> 12) + 224) as u8);
        str.push((((code >> 6) & 63) + 128) as u8);
        str.push(((code & 63) + 128) as u8);
    } else if code <= 0x10FFFF {
        str.push(((code >> 18) + 240) as u8);
        str.push((((code >> 12) & 63) + 128) as u8);
        str.push((((code >> 6) & 63) + 128) as u8);
        str.push(((code & 63) + 128) as u8);
    }
}

/// `utf8_to_utf32`: the code points up to the first that reads as 0.
pub fn utf8_to_utf32(val: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        let ch = read_utf8_char(val, &mut index);
        if ch == 0 {
            return out;
        }
        out.push(ch);
    }
}

/// `utf32_to_utf8`.
pub fn utf32_to_utf8(val: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &ch in val {
        append_char(&mut out, ch);
    }
    out
}
