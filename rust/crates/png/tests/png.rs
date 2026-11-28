//! tests/core_tests.cpp's checks of png.cpp (a broken PNG keeps the rows before the damage; what an image's
//! header declares), and the inflater on streams of every block kind: whole rows count, a cut stream stops
//! where its data does, a bad row stops the count.

mod common;

use common::*;
use twidget_png::*;

/// core_tests' PNGs: 2 × 3 RGBA, their data one stored block.
fn stored_png(rows: &[u8], end: bool) -> Vec<u8> {
    let n = rows.len() as u16;
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&n.to_le_bytes());
    z.extend_from_slice(&(!n).to_le_bytes());
    z.extend_from_slice(rows);
    png_file(&ihdr(2, 3, 8, 6, 0), &z, usize::MAX, end)
}

#[test]
fn a_broken_png_keeps_the_rows_before_the_damage() {
    let mut good = vec![0x7fu8; 27];
    let mut bad = good.clone();
    let mut truncated = good[..20].to_vec();
    good[0] = 0;
    good[9] = 0;
    good[18] = 0;
    bad[0] = 0;
    bad[9] = 0;
    bad[18] = 7;
    truncated[0] = 0;
    truncated[9] = 0;
    assert_eq!(png_intact_rows(&stored_png(&good, true)), None, "an intact PNG keeps every row");
    assert_eq!(png_intact_rows(&stored_png(&bad, false)), Some(2), "rows stop at an unknown filter type");
    assert_eq!(png_intact_rows(&stored_png(&truncated, false)), Some(2), "and where the data runs out");
}

#[test]
fn image_headers_declare_sizes() {
    let size_of = |bytes: &[u8]| image_dimensions(bytes).map_or("none".to_string(), |(w, h)| format!("{w}x{h}"));
    let mut good = vec![0x7fu8; 27];
    good[0] = 0;
    good[9] = 0;
    good[18] = 0;
    let intact = stored_png(&good, true);
    let gif = [b'G', b'I', b'F', b'8', b'9', b'a', 3, 0, 2, 0];
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC2, 0, 17, 8, 0, 5, 0, 7, 3];
    let webp =
        [b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P', b'8', b'X', 10, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 19, 0, 0];
    let short_jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 40];
    let got = [size_of(&intact), size_of(&gif), size_of(&jpeg), size_of(&webp), size_of(&short_jpeg)];
    assert_eq!(got, ["2x3", "3x2", "7x5", "10x20", "none"], "image_dimensions reads PNG, GIF, JPEG and WebP headers");
}

#[test]
fn every_block_kind_inflates() {
    let mut random = Lcg::new(0x9e60_0001);
    let kinds = [Block::Stored, Block::Fixed, Block::Dynamic];
    for round in 0..120 {
        let width = 1 + random.below(40) as u32;
        let height = 2 + random.below(30);
        let (color, channels) = [(0u8, 1usize), (2, 3), (3, 1), (4, 2), (6, 4)][random.below(5)];
        let depth = if color == 3 || color == 0 { [1u8, 2, 4, 8][random.below(4)] } else { 8 };
        let stride = (width as usize * channels * usize::from(depth)).div_ceil(8) + 1;
        let data = rows(&mut random, stride, height);
        let blocks: Vec<Block> = (0..1 + random.below(4)).map(|_| kinds[random.below(3)]).collect();
        let z = zlib(&data, &blocks, if random.chance(2) { 32768 } else { 64 });
        let header = ihdr(width, height as u32, depth, color, 0);
        let what = format!("round {round}: {width}x{height}, colour {color} depth {depth}, {blocks:?}");
        assert_eq!(png_intact_rows(&png_file(&header, &z, 1 + random.below(z.len()), true)), None, "{what}: intact");
        assert_eq!(png_intact_rows(&png_file(&header, &z, 1 + random.below(z.len()), false)), None, "{what}: every row without IEND");
        // Cut short, a stored stream keeps the whole rows before the cut.
        let stored = zlib(&data, &[Block::Stored], 32768);
        let k = random.below(data.len());
        let rows = png_intact_rows(&png_file(&header, &stored[..7 + k], 1 + random.below(7 + k), false));
        assert_eq!(rows, Some(k / stride), "{what}: cut after {k} bytes of rows");
        // A row with an unknown filter type stops the count right there.
        let mut broken = data.clone();
        let at = random.below(height);
        broken[at * stride] = 5 + random.below(250) as u8;
        let z = zlib(&broken, &blocks, 32768);
        assert_eq!(png_intact_rows(&png_file(&header, &z, z.len(), false)), Some(at), "{what}: row {at} broken");
    }
}

#[test]
fn not_a_png() {
    assert_eq!(png_intact_rows(&[]), None);
    assert_eq!(png_intact_rows(&SIGNATURE), None);
    let mut z = vec![0x78, 0x01, 0x01, 3, 0, !3, !0, 0, 1, 2];
    z.truncate(8);
    assert_eq!(png_intact_rows(&png_file(&ihdr(2, 2, 8, 6, 1), &z, 4, false)), None, "interlaced");
    assert_eq!(png_intact_rows(&png_file(&ihdr(2, 2, 8, 5, 0), &z, 4, false)), None, "no such colour type");
    assert_eq!(png_intact_rows(&png_file(&ihdr(0, 2, 8, 6, 0), &z, 4, false)), None, "no width");
    assert_eq!(png_intact_rows(&png_file(&ihdr(2, 2, 8, 6, 0), &z, 4, false)), Some(0), "a cut first row");
}
