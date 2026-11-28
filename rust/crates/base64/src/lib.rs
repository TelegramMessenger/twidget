//! TwidgetCore's base64 (`core/src/encode.cpp`, `twlite::base64`): the standard alphabet with `=` padding,
//! as the renderer answers a canvas's `getImageData` and the JSON form of a canvas batch carries
//! `putImageData`'s pixels.

#![forbid(unsafe_code)]

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn digit(v: u32) -> char {
    char::from(ALPHABET[(v & 63) as usize])
}

/// `bytes` in base64, padded to a multiple of four characters.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for c in &mut chunks {
        let v = (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
        out.extend([digit(v >> 18), digit(v >> 12), digit(v >> 6), digit(v)]);
    }
    match *chunks.remainder() {
        [a] => {
            let v = u32::from(a) << 16;
            out.extend([digit(v >> 18), digit(v >> 12), '=', '=']);
        }
        [a, b] => {
            let v = (u32::from(a) << 16) | (u32::from(b) << 8);
            out.extend([digit(v >> 18), digit(v >> 12), digit(v >> 6), '=']);
        }
        _ => {}
    }
    out
}
