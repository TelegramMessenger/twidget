//! What the PNG tests share: chunks with their CRCs, a zlib stream from stored, fixed and dynamic Huffman
//! blocks (a small deflater of its own, so that the inflater meets every block kind and back reference),
//! PNG files of generated rows, and the image files and `data:` images of twidget and widget-sandbox.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub use twidget_testkit::Lcg;

pub fn crc(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFFFFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xFFFFFFFF
}

/// A chunk: its length, type, data and CRC.
pub fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let c = crc(&out[start..]);
    out.extend_from_slice(&c.to_be_bytes());
}

pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub fn ihdr(width: u32, height: u32, depth: u8, color: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&width.to_be_bytes());
    d.extend_from_slice(&height.to_be_bytes());
    d.extend_from_slice(&[depth, color, 0, 0, interlace]);
    d
}

/// LSB-first bits, as deflate packs them.
#[derive(Default)]
pub struct BitWriter {
    pub out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    pub fn bits(&mut self, v: u32, k: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += k;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code, most significant bit first.
    pub fn code(&mut self, code: u32, len: u32) {
        for i in (0..len).rev() {
            self.bits((code >> i) & 1, 1);
        }
    }

    pub fn align(&mut self) {
        if self.n > 0 {
            self.bits(0, 8 - self.n);
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// Canonical codes for `lengths`.
fn canonical(lengths: &[u8]) -> Vec<u32> {
    let mut count = [0u32; 16];
    for &l in lengths {
        count[usize::from(l)] += 1;
    }
    count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0;
    for bits in 1..16 {
        code = (code + count[bits - 1]) << 1;
        next[bits] = code;
    }
    lengths
        .iter()
        .map(|&l| {
            if l == 0 {
                0
            } else {
                let c = next[usize::from(l)];
                next[usize::from(l)] += 1;
                c
            }
        })
        .collect()
}

/// Huffman code lengths for `freq`, none longer than `limit` (the longest ones flattened when needed).
fn code_lengths(freq: &[u32], limit: u8) -> Vec<u8> {
    let mut lengths = vec![0u8; freq.len()];
    let used: Vec<usize> = (0..freq.len()).filter(|&i| freq[i] > 0).collect();
    if used.is_empty() {
        return lengths;
    }
    if used.len() == 1 {
        lengths[used[0]] = 1;
        return lengths;
    }
    // Nodes: (weight, leaves under it).
    let mut nodes: Vec<(u64, Vec<usize>)> = used.iter().map(|&i| (u64::from(freq[i]), vec![i])).collect();
    while nodes.len() > 1 {
        nodes.sort_by_key(|n| std::cmp::Reverse(n.0));
        let a = nodes.pop().expect("two nodes");
        let b = nodes.pop().expect("two nodes");
        for &leaf in a.1.iter().chain(&b.1) {
            lengths[leaf] += 1;
        }
        let mut leaves = a.1;
        leaves.extend(b.1);
        nodes.push((a.0 + b.0, leaves));
    }
    if lengths.iter().any(|&l| l > limit) {
        // Flat codes long enough for every symbol: always valid, never optimal.
        let bits = (usize::BITS - (used.len() - 1).leading_zeros()) as u8;
        for &i in &used {
            lengths[i] = bits.max(1);
        }
    }
    lengths
}

/// One deflate symbol: a literal or a match.
#[derive(Clone, Copy)]
enum Sym {
    Literal(u8),
    Match(u32, u32),
}

const LENGTH_BASE: [u32; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385,
    24577,
];
const DIST_EXTRA: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn length_code(len: u32) -> usize {
    (0..29).rev().find(|&i| LENGTH_BASE[i] <= len).expect("a length of 3 or more")
}

fn dist_code(d: u32) -> usize {
    (0..30).rev().find(|&i| DIST_BASE[i] <= d).expect("a distance of 1 or more")
}

/// Greedy LZ77 over `data`, with matches up to `window` back.
fn lz77(data: &[u8], window: usize) -> Vec<Sym> {
    let mut out = Vec::new();
    let mut heads: std::collections::HashMap<[u8; 3], Vec<usize>> = std::collections::HashMap::new();
    let mut i = 0;
    while i < data.len() {
        let mut best = (0usize, 0usize);
        if i + 3 <= data.len() {
            let key = [data[i], data[i + 1], data[i + 2]];
            if let Some(list) = heads.get(&key) {
                for &j in list.iter().rev().take(16) {
                    if i - j > window {
                        break;
                    }
                    let mut l = 0;
                    while i + l < data.len() && l < 258 && data[j + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best.0 {
                        best = (l, i - j);
                    }
                }
            }
        }
        let step = if best.0 >= 3 { best.0 } else { 1 };
        for k in i..i + step {
            if k + 3 <= data.len() {
                heads.entry([data[k], data[k + 1], data[k + 2]]).or_default().push(k);
            }
        }
        out.push(if best.0 >= 3 { Sym::Match(best.0 as u32, best.1 as u32) } else { Sym::Literal(data[i]) });
        i += step;
    }
    out
}

fn put_symbols(w: &mut BitWriter, syms: &[Sym], lit: &[u8], dist: &[u8]) {
    let lit_codes = canonical(lit);
    let dist_codes = canonical(dist);
    for s in syms {
        match *s {
            Sym::Literal(b) => w.code(lit_codes[usize::from(b)], u32::from(lit[usize::from(b)])),
            Sym::Match(len, d) => {
                let lc = length_code(len);
                w.code(lit_codes[257 + lc], u32::from(lit[257 + lc]));
                w.bits(len - LENGTH_BASE[lc], LENGTH_EXTRA[lc]);
                let dc = dist_code(d);
                w.code(dist_codes[dc], u32::from(dist[dc]));
                w.bits(d - DIST_BASE[dc], DIST_EXTRA[dc]);
            }
        }
    }
    w.code(lit_codes[256], u32::from(lit[256]));
}

/// How a block of the stream is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Block {
    Stored,
    Fixed,
    Dynamic,
}

fn fixed_lengths() -> (Vec<u8>, Vec<u8>) {
    let lit = (0..288)
        .map(|i| {
            if i < 144 {
                8
            } else if i < 256 {
                9
            } else if i < 280 {
                7
            } else {
                8
            }
        })
        .collect();
    (lit, vec![5; 30])
}

fn dynamic_block(w: &mut BitWriter, syms: &[Sym]) {
    let mut lf = vec![0u32; 286];
    let mut df = vec![0u32; 30];
    for s in syms {
        match *s {
            Sym::Literal(b) => lf[usize::from(b)] += 1,
            Sym::Match(len, d) => {
                lf[257 + length_code(len)] += 1;
                df[dist_code(d)] += 1;
            }
        }
    }
    lf[256] += 1;
    let lit = code_lengths(&lf, 15);
    let mut dist = code_lengths(&df, 15);
    if dist.iter().all(|&l| l == 0) {
        dist[0] = 1;
    }
    let hlit = lit.iter().rposition(|&l| l != 0).map_or(257, |p| (p + 1).max(257));
    let hdist = dist.iter().rposition(|&l| l != 0).map_or(1, |p| p + 1);
    // The code lengths, run-length coded with 16, 17 and 18.
    let all: Vec<u8> = lit[..hlit].iter().chain(&dist[..hdist]).copied().collect();
    let mut rle: Vec<(u8, u32, u32)> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let mut run = 1;
        while i + run < all.len() && all[i + run] == all[i] {
            run += 1;
        }
        if all[i] == 0 && run >= 11 {
            let n = run.min(138);
            rle.push((18, (n - 11) as u32, 7));
            i += n;
        } else if all[i] == 0 && run >= 3 {
            let n = run.min(10);
            rle.push((17, (n - 3) as u32, 3));
            i += n;
        } else if i > 0 && all[i] == all[i - 1] && run >= 3 {
            let n = run.min(6);
            rle.push((16, (n - 3) as u32, 2));
            i += n;
        } else {
            rle.push((all[i], 0, 0));
            i += 1;
        }
    }
    let mut cf = vec![0u32; 19];
    for &(s, _, _) in &rle {
        cf[usize::from(s)] += 1;
    }
    let clen = code_lengths(&cf, 7);
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let hclen = ORDER.iter().rposition(|&o| clen[o] != 0).map_or(4, |p| (p + 1).max(4));
    w.bits(2, 2);
    w.bits((hlit - 257) as u32, 5);
    w.bits((hdist - 1) as u32, 5);
    w.bits((hclen - 4) as u32, 4);
    for &o in &ORDER[..hclen] {
        w.bits(u32::from(clen[o]), 3);
    }
    let ccodes = canonical(&clen);
    for &(s, extra, k) in &rle {
        w.code(ccodes[usize::from(s)], u32::from(clen[usize::from(s)]));
        w.bits(extra, k);
    }
    let mut lit_full = lit.clone();
    lit_full.resize(286, 0);
    put_symbols(w, syms, &lit_full, &dist);
}

/// A zlib stream of `data` in blocks of the given kinds, each about as many symbols, matches reaching
/// `window` back across them.
pub fn zlib(data: &[u8], blocks: &[Block], window: usize) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.out.extend_from_slice(&[0x78, 0x9c]);
    let syms = lz77(data, window);
    let per = syms.len().div_ceil(blocks.len().max(1)).max(1);
    let groups: Vec<&[Sym]> = if syms.is_empty() { vec![&syms[..]] } else { syms.chunks(per).collect() };
    let mut from = 0;
    for (k, group) in groups.iter().enumerate() {
        let last = u32::from(k + 1 == groups.len());
        let span: usize = group
            .iter()
            .map(|s| match *s {
                Sym::Literal(_) => 1,
                Sym::Match(l, _) => l as usize,
            })
            .sum();
        match blocks.get(k).copied().unwrap_or(Block::Fixed) {
            Block::Stored => {
                let pieces: Vec<&[u8]> = if span == 0 { vec![&data[from..from]] } else { data[from..from + span].chunks(65535).collect() };
                for (i, piece) in pieces.iter().enumerate() {
                    w.bits(u32::from(last == 1 && i + 1 == pieces.len()), 1);
                    w.bits(0, 2);
                    w.align();
                    let n = piece.len() as u16;
                    w.out.extend_from_slice(&n.to_le_bytes());
                    w.out.extend_from_slice(&(!n).to_le_bytes());
                    w.out.extend_from_slice(piece);
                }
            }
            Block::Fixed => {
                w.bits(last, 1);
                w.bits(1, 2);
                let (lit, dist) = fixed_lengths();
                put_symbols(&mut w, group, &lit, &dist);
            }
            Block::Dynamic => {
                w.bits(last, 1);
                dynamic_block(&mut w, group);
            }
        }
        from += span;
    }
    let mut out = w.finish();
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// Image rows as a PNG holds them, each a filter byte (0 to 4) and `stride - 1` bytes of content.
pub fn rows(random: &mut Lcg, stride: usize, height: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(stride * height);
    let flat = random.chance(2);
    for _ in 0..height {
        out.push(random.below(5) as u8);
        for x in 1..stride {
            let range = if random.chance(4) { 256 } else { 4 };
            out.push(if flat { (x / 4 * 3) as u8 } else { random.below(range) as u8 });
        }
    }
    out
}

/// A PNG of `idat` in pieces of `split` bytes, with or without its IEND.
pub fn png_file(header: &[u8], idat: &[u8], split: usize, end: bool) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    chunk(&mut out, b"IHDR", header);
    if idat.is_empty() {
        chunk(&mut out, b"IDAT", idat);
    }
    for piece in idat.chunks(split.max(1)) {
        chunk(&mut out, b"IDAT", piece);
    }
    if end {
        chunk(&mut out, b"IEND", &[]);
    }
    out
}

pub fn widget_sandbox() -> PathBuf {
    std::env::var_os("TWIDGET_WIDGET_SANDBOX")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../widget-sandbox"))
}

fn files(dir: &Path, out: &mut Vec<PathBuf>, keep: &dyn Fn(&Path) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if path.is_dir() {
            if name != "node_modules" && name != "target" && name != "build" && !name.starts_with('.') {
                files(&path, out, keep);
            }
        } else if keep(&path) {
            out.push(path);
        }
    }
}

fn decode_base64(text: &[u8]) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut n = 0;
    for &c in text {
        let Some(v) = value(c) else { break };
        acc = (acc << 6) | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    out
}

/// Every image file of twidget and widget-sandbox (the conformance references are Chromium's PNGs), and every
/// `data:` image of widget-sandbox's corpus, conformance and stress widgets.
pub fn corpus_images() -> Vec<(String, Vec<u8>)> {
    let image = |p: &Path| p.extension().is_some_and(|e| ["png", "gif", "jpg", "jpeg", "webp"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    let mut paths = Vec::new();
    files(&widget_sandbox(), &mut paths, &image);
    files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."), &mut paths, &|p: &Path| {
        image(p) && !p.to_string_lossy().contains("rust")
    });
    let mut out: Vec<(String, Vec<u8>)> = paths.iter().filter_map(|p| Some((p.display().to_string(), std::fs::read(p).ok()?))).collect();
    let mut pages = Vec::new();
    for dir in ["corpus", "packages/conformance", "packages/stress"] {
        files(&widget_sandbox().join(dir), &mut pages, &|p: &Path| p.extension().is_some_and(|e| e == "html"));
    }
    for page in pages {
        let Ok(text) = std::fs::read(&page) else { continue };
        let needle = b";base64,";
        for at in (0..text.len().saturating_sub(needle.len())).filter(|&i| &text[i..i + needle.len()] == needle) {
            if !text[at.saturating_sub(24)..at].windows(11).any(|w| w == b"data:image/") {
                continue;
            }
            let bytes = decode_base64(&text[at + needle.len()..]);
            if bytes.len() >= 8 && !out.iter().any(|(_, b)| *b == bytes) {
                out.push((format!("{} at {at}", page.display()), bytes));
            }
        }
    }
    out
}
