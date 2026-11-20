//! What a browser shows of a broken PNG (`core/src/png.h`, `png.cpp`). Decoders differ on a cut or corrupt
//! stream: Chromium keeps the rows decoded before the damage and leaves the rest transparent, where others
//! fail the whole image or fill the rest with whatever their inflater made of it. The backends decode as
//! they do and then clear the rows [`png_intact_rows`] finds. Also the size an image header declares,
//! read before a decoder allocates it.
//!
//! The inflater is png.cpp's own: it counts whole rows as it inflates, keeping only the 32 KB its back
//! references reach, and stops at the first thing that goes wrong.

#![forbid(unsafe_code)]

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn crc32(p: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = 0xFFFFFFFFu32;
    for &b in p {
        c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFFFFFF
}

/// Counts whole rows as the inflater makes them, keeping only the 32 KB its back references reach.
struct Rows {
    stride: u64,
    height: u64,
    produced: u64,
    intact: u64,
    window: Box<[u8; 32768]>,
}

impl Rows {
    /// False once the image is complete or a row starts with an unknown filter type.
    fn put(&mut self, b: u8) -> bool {
        if self.produced == self.intact.wrapping_mul(self.stride) && b > 4 {
            return false;
        }
        self.window[(self.produced & 32767) as usize] = b;
        self.produced += 1;
        if self.produced == (self.intact + 1).wrapping_mul(self.stride) {
            self.intact += 1;
        }
        self.intact < self.height
    }
}

struct Bits<'a> {
    p: &'a [u8],
    at: usize,
    buf: u32,
    count: i32,
    over: bool,
}

impl Bits<'_> {
    fn get(&mut self, k: i32) -> u32 {
        while self.count < k {
            if self.at >= self.p.len() {
                self.over = true;
                return 0;
            }
            self.buf |= u32::from(self.p[self.at]) << self.count;
            self.at += 1;
            self.count += 8;
        }
        let v = self.buf & ((1u32 << k) - 1);
        self.buf >>= k;
        self.count -= k;
        v
    }
}

/// A canonical Huffman code: how many codes of each length, and the symbols in code order.
#[derive(Clone)]
struct Tree {
    counts: [u16; 16],
    symbols: [u16; 320],
}

impl Tree {
    fn new() -> Tree {
        Tree { counts: [0; 16], symbols: [0; 320] }
    }

    fn build(&mut self, lengths: &[u8]) {
        let mut offsets = [0u16; 16];
        for &l in lengths {
            self.counts[usize::from(l)] = self.counts[usize::from(l)].wrapping_add(1);
        }
        self.counts[0] = 0;
        for i in 1..16 {
            offsets[i] = offsets[i - 1].wrapping_add(self.counts[i - 1]);
        }
        for (i, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let at = &mut offsets[usize::from(l)];
                self.symbols[usize::from(*at)] = i as u16;
                *at = at.wrapping_add(1);
            }
        }
    }

    fn decode(&self, bits: &mut Bits) -> i32 {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= bits.get(1) as i32;
            if bits.over {
                return -1;
            }
            let count = i32::from(self.counts[len]);
            if code - first < count {
                // code >= first always holds (each length starts its codes where the shorter ones ended), and
                // index + count is at most the symbols built.
                return i32::from(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        -1
    }
}

const LENGTH_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385,
    24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// False when the rows stop: the image is complete, a row is broken, or the data is.
fn inflate_block(bits: &mut Bits, lit: &Tree, dist: &Tree, rows: &mut Rows) -> bool {
    loop {
        let sym = lit.decode(bits);
        if sym < 0 {
            return false;
        }
        if sym < 256 {
            if !rows.put(sym as u8) {
                return false;
            }
            continue;
        }
        if sym == 256 {
            return true;
        }
        let sym = (sym - 257) as usize;
        if sym >= 29 {
            return false;
        }
        let length = u32::from(LENGTH_BASE[sym]) + bits.get(i32::from(LENGTH_EXTRA[sym]));
        let d = dist.decode(bits);
        if !(0..30).contains(&d) {
            return false;
        }
        let back = u64::from(DIST_BASE[d as usize]) + u64::from(bits.get(i32::from(DIST_EXTRA[d as usize])));
        if bits.over || back > rows.produced {
            return false;
        }
        for _ in 0..length {
            if !rows.put(rows.window[((rows.produced - back) & 32767) as usize]) {
                return false;
            }
        }
    }
}

fn fixed_trees() -> &'static (Tree, Tree) {
    static FIXED: std::sync::OnceLock<(Tree, Tree)> = std::sync::OnceLock::new();
    FIXED.get_or_init(|| {
        let mut lengths = [0u8; 320];
        for (i, l) in lengths.iter_mut().enumerate().take(288) {
            *l = if i < 144 {
                8
            } else if i < 256 {
                9
            } else if i < 280 {
                7
            } else {
                8
            };
        }
        let mut lit = Tree::new();
        lit.build(&lengths[..288]);
        for l in lengths.iter_mut().take(30) {
            *l = 5;
        }
        let mut dist = Tree::new();
        dist.build(&lengths[..30]);
        (lit, dist)
    })
}

/// Inflates a zlib stream into `rows`, as far as it goes.
fn inflate(data: &[u8], rows: &mut Rows) {
    if data.len() < 2 || (data[0] & 0x0F) != 8 || ((u32::from(data[0]) << 8) | u32::from(data[1])) % 31 != 0 || (data[1] & 0x20) != 0 {
        return;
    }
    let mut bits = Bits { p: &data[2..], at: 0, buf: 0, count: 0, over: false };
    let mut last = false;
    while !last {
        last = bits.get(1) != 0;
        let kind = bits.get(2);
        if bits.over {
            return;
        }
        if kind == 0 {
            bits.buf = 0;
            bits.count = 0;
            if bits.at + 4 > bits.p.len() {
                return;
            }
            let len = u32::from(bits.p[bits.at]) | (u32::from(bits.p[bits.at + 1]) << 8);
            bits.at += 4;
            for _ in 0..len {
                if bits.at >= bits.p.len() {
                    return;
                }
                let b = bits.p[bits.at];
                bits.at += 1;
                if !rows.put(b) {
                    return;
                }
            }
        } else if kind == 1 {
            let (lit, dist) = fixed_trees();
            if !inflate_block(&mut bits, lit, dist, rows) {
                return;
            }
        } else if kind == 2 {
            let hlit = bits.get(5) + 257;
            let hdist = bits.get(5) + 1;
            let hclen = bits.get(4) + 4;
            const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
            let mut code_lengths = [0u8; 19];
            for &o in ORDER.iter().take(hclen as usize) {
                code_lengths[o] = bits.get(3) as u8;
            }
            if bits.over || hlit > 286 || hdist > 30 {
                return;
            }
            let mut codes = Tree::new();
            codes.build(&code_lengths);
            let mut lengths = [0u8; 320];
            let total = hlit + hdist;
            let mut i = 0u32;
            while i < total {
                let sym = codes.decode(&mut bits);
                if sym < 0 {
                    return;
                }
                if sym < 16 {
                    lengths[i as usize] = sym as u8;
                    i += 1;
                    continue;
                }
                let mut repeat = 0u8;
                let times;
                if sym == 16 {
                    if i == 0 {
                        return;
                    }
                    repeat = lengths[i as usize - 1];
                    times = 3 + bits.get(2);
                } else if sym == 17 {
                    times = 3 + bits.get(3);
                } else {
                    times = 11 + bits.get(7);
                }
                if bits.over || i + times > total {
                    return;
                }
                for _ in 0..times {
                    lengths[i as usize] = repeat;
                    i += 1;
                }
            }
            let mut lit = Tree::new();
            let mut dist = Tree::new();
            lit.build(&lengths[..hlit as usize]);
            dist.build(&lengths[hlit as usize..total as usize]);
            if !inflate_block(&mut bits, &lit, &dist, rows) {
                return;
            }
        } else {
            return;
        }
    }
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// How many rows of the PNG in `bytes` come before the damage, when it is damaged: it lacks its IEND, or an
/// IDAT chunk fails its CRC. The rows then stop at an unknown filter type or where the image data runs out.
/// None (the C++'s `SIZE_MAX`) when nothing is damaged, or `bytes` is no non-interlaced PNG.
pub fn png_intact_rows(bytes: &[u8]) -> Option<usize> {
    let length = bytes.len();
    if length < 33 || bytes[..8] != SIGNATURE {
        return None;
    }
    if &bytes[12..16] != b"IHDR" || be32(bytes, 8) < 13 {
        return None;
    }
    let width = be32(bytes, 16);
    let height = be32(bytes, 20);
    let depth = bytes[24];
    let kind = bytes[25];
    let interlace = bytes[28];
    let channels: u64 = match kind {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => 0,
    };
    if channels == 0 || interlace != 0 || width == 0 || height == 0 {
        return None;
    }

    let mut damaged = true;
    let mut at = 8usize;
    while at + 12 <= length {
        let size = be32(bytes, at) as usize;
        if size > length - at - 12 {
            break;
        }
        if &bytes[at + 4..at + 8] == b"IEND" {
            damaged = false;
            break;
        }
        if &bytes[at + 4..at + 8] == b"IDAT" && crc32(&bytes[at + 4..at + 8 + size]) != be32(bytes, at + 8 + size) {
            break;
        }
        at += size + 12;
    }
    if !damaged {
        return None;
    }

    // Every whole IDAT chunk, the damaged one too: its rows count until they go wrong.
    let mut data = Vec::new();
    let mut at = 8usize;
    while at + 12 <= length {
        let size = be32(bytes, at) as usize;
        if size > length - at - 12 || &bytes[at + 4..at + 8] == b"IEND" {
            break;
        }
        if &bytes[at + 4..at + 8] == b"IDAT" {
            data.extend_from_slice(&bytes[at + 8..at + 8 + size]);
        }
        at += size + 12;
    }
    let mut rows = Rows {
        stride: (u64::from(width) * channels * u64::from(depth)).div_ceil(8) + 1,
        height: u64::from(height),
        produced: 0,
        intact: 0,
        window: Box::new([0; 32768]),
    };
    inflate(&data, &mut rows);
    (rows.intact < u64::from(height)).then_some(rows.intact as usize)
}

/// The size a PNG, GIF, JPEG or WebP header in `b` declares, read before decoding: what a decoder would
/// allocate for a small file. None for anything else, or a header cut short.
pub fn image_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    let n = b.len();
    let be16 = |at: usize| (u32::from(b[at]) << 8) | u32::from(b[at + 1]);
    let le16 = |at: usize| u32::from(b[at]) | (u32::from(b[at + 1]) << 8);
    let be32 = |at: usize| (be16(at) << 16) | be16(at + 2);
    let le24 = |at: usize| le16(at) | (u32::from(b[at + 2]) << 16);
    if n >= 24 && b[..8] == SIGNATURE && &b[12..16] == b"IHDR" {
        return Some((be32(16), be32(20)));
    }
    if n >= 10 && (&b[..6] == b"GIF87a" || &b[..6] == b"GIF89a") {
        return Some((le16(6), le16(8)));
    }
    if n >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        // The first start-of-frame marker, past the segments before it.
        let mut i = 2usize;
        while i + 3 < n {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
                i += 2;
                continue;
            }
            if marker == 0xDA || marker == 0xD9 {
                return None;
            }
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
                if i + 9 > n {
                    return None;
                }
                return Some((be16(i + 7), be16(i + 5)));
            }
            i += 2 + be16(i + 2) as usize;
        }
        return None;
    }
    if n >= 30 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        if &b[12..16] == b"VP8 " {
            return Some((le16(26) & 0x3FFF, le16(28) & 0x3FFF));
        }
        if &b[12..16] == b"VP8L" && b[20] == 0x2F {
            let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
            return Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1));
        }
        if &b[12..16] == b"VP8X" {
            return Some((le24(24) + 1, le24(27) + 1));
        }
    }
    None
}
