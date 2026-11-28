//! The Rust PNG damage finder and header reader against the C++ (`oracle/png_oracle`, built from
//! core/src/png.cpp): the rows each finds and the sizes each reads, on the image files of twidget and
//! widget-sandbox (Chromium's PNGs among them) and the corpus's `data:` images, whole, cut, with their IEND
//! dropped, their CRCs broken and their compressed data changed; on generated PNGs of every colour type,
//! depth and block kind, cut and mutated; and on generated GIF, JPEG and WebP headers. Skipped without
//! `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

mod common;

use common::*;
use twidget_png::*;
use twidget_testkit::{Comparison, Oracle, framed, mutate};

use std::sync::atomic::{AtomicUsize, Ordering};

/// Answers that found damage, of the rows compared.
static DAMAGED: AtomicUsize = AtomicUsize::new(0);

fn both(c: &mut Comparison, bytes: &[u8], what: &str) {
    let rows = png_intact_rows(bytes).map_or(u64::MAX, |r| r as u64);
    if rows != u64::MAX {
        DAMAGED.fetch_add(1, Ordering::Relaxed);
    }
    c.case(&framed(b'r', bytes), (1, rows.to_le_bytes().to_vec()), format!("{what}: rows"));
    let (ok, (w, h)) = match image_dimensions(bytes) {
        Some(size) => (1, size),
        None => (0, (7, 9)),
    };
    let mut out = w.to_le_bytes().to_vec();
    out.extend_from_slice(&h.to_le_bytes());
    c.case(&framed(b'd', bytes), (ok, out), format!("{what}: dimensions"));
}

/// Where a PNG's chunks start, and the IDAT data they hold together.
fn chunks(bytes: &[u8]) -> (Vec<usize>, Vec<u8>) {
    let mut starts = Vec::new();
    let mut data = Vec::new();
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize;
        if size > bytes.len() - at - 12 {
            break;
        }
        starts.push(at);
        if &bytes[at + 4..at + 8] == b"IDAT" {
            data.extend_from_slice(&bytes[at + 8..at + 8 + size]);
        }
        at += size + 12;
    }
    (starts, data)
}

fn header_of(bytes: &[u8]) -> Vec<u8> {
    bytes[16..29].to_vec()
}

#[test]
fn corpus_images_alike() {
    let Some(oracle) = Oracle::find("png_oracle") else { return };
    let images = corpus_images();
    assert!(images.len() >= 150, "the corpus's images are there ({})", images.len());
    let mut random = Lcg::new(0x9e60_0002);
    let mut c = Comparison::new();
    let mut pngs = 0;
    for (name, bytes) in &images {
        both(&mut c, bytes, name);
        let rounds = if bytes.len() > 30000 { 6 } else { 24 };
        for _ in 0..rounds {
            let cut = random.below(bytes.len());
            both(&mut c, &bytes[..cut], &format!("{name} cut to {cut}"));
        }
        for _ in 0..rounds {
            let mut changed = bytes.clone();
            let edits = mutate(&mut random, &mut changed);
            both(&mut c, &changed, &format!("{name}:{edits}"));
        }
        if bytes.len() < 33 || bytes[..8] != SIGNATURE {
            continue;
        }
        pngs += 1;
        let (starts, data) = chunks(bytes);
        // Without the IEND every row is inflated; then cut at each chunk and with a CRC broken.
        let iend = starts.iter().copied().find(|&at| &bytes[at + 4..at + 8] == b"IEND").unwrap_or(bytes.len());
        both(&mut c, &bytes[..iend], &format!("{name} without IEND"));
        for &at in starts.iter().step_by((starts.len() / 8).max(1)) {
            both(&mut c, &bytes[..at], &format!("{name} cut at the chunk at {at}"));
            if &bytes[at + 4..at + 8] == b"IDAT" {
                let mut broken = bytes.clone();
                let size = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize;
                broken[at + 8 + random.below(size + 4)] ^= 1 << random.below(8);
                both(&mut c, &broken, &format!("{name} with the chunk at {at} broken"));
            }
        }
        // The compressed data changed, its CRCs right and its IEND gone: the inflater meets what the change made.
        for round in 0..rounds {
            let mut z = data.clone();
            let edits = mutate(&mut random, &mut z);
            let file = png_file(&header_of(bytes), &z, 1 + random.below(z.len().max(1)), false);
            both(&mut c, &file, &format!("{name}, data round {round}:{edits}"));
        }
    }
    let tally = c.check(&oracle);
    eprintln!(
        "corpus images: {} files ({pngs} PNGs), {} answers alike ({} refused alike; damage found in {} of all the tests' row answers so far)",
        images.len(),
        tally[1] + tally[0],
        tally[0],
        DAMAGED.load(Ordering::Relaxed)
    );
}

#[test]
fn generated_pngs_alike() {
    let Some(oracle) = Oracle::find("png_oracle") else { return };
    let mut random = Lcg::new(0x9e60_0003);
    let kinds = [Block::Stored, Block::Fixed, Block::Dynamic];
    let mut c = Comparison::new();
    for round in 0..3000 {
        let width = if random.chance(30) { [0, 0x7fffffff, 0xffffffff][random.below(3)] } else { 1 + random.below(60) as u32 };
        let height = if random.chance(30) { [0, 0xffffffff][random.below(2)] } else { 1 + random.below(40) as u32 };
        let color = if random.chance(10) { random.next() as u8 } else { [0u8, 2, 3, 4, 6][random.below(5)] };
        let depth = if random.chance(10) { random.next() as u8 } else { [1u8, 2, 4, 8, 16][random.below(5)] };
        let interlace = u8::from(random.chance(20));
        let channels = match color {
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 1,
        };
        let stride = ((u64::from(width.min(60)) * channels * u64::from(depth)).div_ceil(8) + 1) as usize;
        let data = rows(&mut random, stride, height.min(40) as usize);
        let blocks: Vec<Block> = (0..1 + random.below(4)).map(|_| kinds[random.below(3)]).collect();
        let z = zlib(&data, &blocks, if random.chance(2) { 32768 } else { 1 + random.below(300) });
        let header = ihdr(width, height, depth, color, interlace);
        let what = format!("generated {round}: {width}x{height} colour {color} depth {depth} {blocks:?}");
        both(&mut c, &png_file(&header, &z, 1 + random.below(z.len()), random.chance(4)), &what);
        for k in 0..4 {
            let cut = random.below(z.len() + 1);
            both(&mut c, &png_file(&header, &z[..cut], 1 + random.below(cut.max(1)), false), &format!("{what}, cut {k} at {cut}"));
        }
        for k in 0..6 {
            let mut changed = z.clone();
            let edits = mutate(&mut random, &mut changed);
            both(&mut c, &png_file(&header, &changed, 1 + random.below(changed.len().max(1)), false), &format!("{what}, data {k}:{edits}"));
        }
        let mut file = png_file(&header, &z, 1 + random.below(z.len()), random.chance(2));
        let edits = mutate(&mut random, &mut file);
        both(&mut c, &file, &format!("{what}, file:{edits}"));
    }
    let tally = c.check(&oracle);
    eprintln!("generated PNGs: {} answers alike ({} refused alike)", tally[1] + tally[0], tally[0]);
}

#[test]
fn generated_headers_alike() {
    let Some(oracle) = Oracle::find("png_oracle") else { return };
    let mut random = Lcg::new(0x9e60_0004);
    let mut c = Comparison::new();
    for round in 0..6000 {
        let mut bytes: Vec<u8> = match random.below(4) {
            0 => {
                let mut b = if random.chance(2) { b"GIF89a".to_vec() } else { b"GIF87a".to_vec() };
                b.extend((0..4 + random.below(8)).map(|_| random.next() as u8));
                b
            }
            1 => {
                let mut b = vec![0xFF, 0xD8];
                for _ in 0..random.below(5) {
                    let marker = [0xE0, 0xE1, 0xDB, 0xC4, 0xFF, 0x01, 0xD0, 0xC8, 0xCC, 0xDA, 0xD9][random.below(11)];
                    b.extend_from_slice(&[0xFF, marker]);
                    if marker != 0xFF && marker != 0x01 && marker != 0xD0 {
                        let n = random.below(12);
                        b.extend_from_slice(&((n + 2) as u16).to_be_bytes());
                        b.extend((0..n).map(|_| random.next() as u8));
                    }
                }
                b.extend_from_slice(&[0xFF, 0xC0 + random.below(16) as u8, 0, 17, 8]);
                b.extend((0..4 + random.below(8)).map(|_| random.next() as u8));
                b
            }
            2 => {
                let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
                b.extend_from_slice([b"VP8 ", b"VP8L", b"VP8X", b"ALPH"][random.below(4)]);
                b.extend((0..14 + random.below(10)).map(|_| random.next() as u8));
                if random.chance(2) && b.len() > 20 {
                    b[20] = 0x2F;
                }
                b
            }
            _ => {
                let mut b = SIGNATURE.to_vec();
                chunk(&mut b, b"IHDR", &(0..13).map(|_| random.next() as u8).collect::<Vec<_>>());
                b
            }
        };
        both(&mut c, &bytes, &format!("header {round}"));
        let cut = random.below(bytes.len() + 1);
        both(&mut c, &bytes[..cut], &format!("header {round} cut to {cut}"));
        let edits = mutate(&mut random, &mut bytes);
        both(&mut c, &bytes, &format!("header {round}:{edits}"));
    }
    let tally = c.check(&oracle);
    eprintln!("generated headers: {} answers alike ({} refused alike)", tally[1] + tally[0], tally[0]);
}
