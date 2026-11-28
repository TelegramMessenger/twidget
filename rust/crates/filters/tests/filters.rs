//! tests/core_tests.cpp's checks of the CPU filters (the painters that draw into memory), by what they check:
//! grayscale, B, G, R, A bytes, opacity, invert, blur, infinite and NaN radii and offsets, and a drop shadow.

use std::time::Instant;

use twidget_filters::*;
use twidget_wire::{Color, Filter, FilterKind};

fn filtered(mut px: Vec<u8>, w: i32, h: i32, fs: &[Filter], bgra: bool) -> Vec<u8> {
    filter_pixels(&mut px, w, h, w as usize * 4, bgra, fs, 1.0);
    px
}

fn f(kind: FilterKind, amount: f32) -> Filter {
    Filter { kind, amount, ..Filter::default() }
}

#[test]
fn colour_functions() {
    let red = vec![255, 0, 0, 255];
    let grey = filtered(red.clone(), 1, 1, &[f(FilterKind::Grayscale, 1.0)], false);
    assert!(grey[0] == 54 && grey[1] == 54 && grey[2] == 54 && grey[3] == 255, "grayscale(1) of red is its luminance (got {})", grey[0]);
    let bgr = filtered(vec![0, 0, 255, 255], 1, 1, &[f(FilterKind::Grayscale, 1.0)], true);
    assert!(bgr[2] == 54 && bgr[0] == 54, "B, G, R, A bytes are read as such");
    let faded = filtered(red, 1, 1, &[f(FilterKind::Opacity, 0.5)], false);
    assert!(faded[3] == 128 && faded[0] == 128, "opacity keeps the colour premultiplied (got {}, {})", faded[0], faded[3]);
    let inverted = filtered(vec![0, 0, 0, 255], 1, 1, &[f(FilterKind::Invert, 1.0)], false);
    assert!(inverted[0] == 255 && inverted[1] == 255, "invert(1) of black is white");
}

fn dot() -> Vec<u8> {
    let mut dot = vec![0u8; 21 * 21 * 4];
    dot[(10 * 21 + 10) * 4 + 3] = 255;
    dot
}

#[test]
fn blur_spreads_a_dot() {
    let blurred = filtered(dot(), 21, 21, &[f(FilterKind::Blur, 2.0)], false);
    let spread = (0..21 * 21).filter(|i| blurred[i * 4 + 3] > 0).count();
    let total: u32 = (0..21 * 21).map(|i| u32::from(blurred[i * 4 + 3])).sum();
    assert!(
        spread > 9 && blurred[(10 * 21 + 10) * 4 + 3] < 255 && total > 180 && total < 330,
        "blur spreads a dot and keeps its alpha ({spread} px, {total})"
    );
}

#[test]
fn infinite_and_nan_radii_finish() {
    // An infinite scale (or amount) was a blur radius of 2^30 and a 2^31-step loop per line; NaN, an int from NaN.
    let started = Instant::now();
    let mut px = dot();
    filter_pixels(&mut px, 21, 21, 21 * 4, false, &[f(FilterKind::Blur, 2.0)], f32::INFINITY);
    filter_pixels(&mut px, 21, 21, 21 * 4, false, &[f(FilterKind::Blur, f32::INFINITY), f(FilterKind::Blur, f32::NAN)], 1.0);
    let far =
        Filter { kind: FilterKind::DropShadow, amount: f32::INFINITY, x: f32::INFINITY, y: f32::NAN, color: Color::rgba(0, 0, 0, 255) };
    filter_pixels(&mut px, 21, 21, 21 * 4, false, &[far], 1.0);
    let ms = started.elapsed().as_millis();
    // The C++ test allows 1 s optimized; an unoptimized build gets ten.
    assert!(ms < if cfg!(debug_assertions) { 10000 } else { 1000 }, "infinite and NaN blur radii and shadow offsets finish ({ms} ms)");
}

#[test]
fn drop_shadow_goes_under() {
    let shadow = Filter { kind: FilterKind::DropShadow, amount: 0.0, x: 3.0, y: 0.0, color: Color::rgba(0, 0, 255, 255) };
    let mut left = vec![0u8; 8 * 4];
    left[0] = 255;
    left[3] = 255;
    let shadowed = filtered(left, 8, 1, &[shadow], false);
    assert!(
        shadowed[3 * 4 + 2] == 255 && shadowed[3 * 4 + 3] == 255 && shadowed[0] == 255,
        "a drop shadow is the alpha in its colour, moved, under the image"
    );
}

#[test]
fn matrices_and_reach() {
    let mut m = ColorMatrix { offset: 7.0, ..ColorMatrix::default() };
    assert!(!color_matrix(&f(FilterKind::Blur, 2.0), &mut m) && m == ColorMatrix::default(), "blur is no matrix, and the matrix is reset");
    assert!(color_matrix(&f(FilterKind::Contrast, 2.0), &mut m) && m.rgb[0] == 2.0 && m.offset == -0.5);
    let reach = filter_reach(&[
        f(FilterKind::Blur, 2.0),
        Filter { kind: FilterKind::DropShadow, amount: 4.0, x: 10.0, y: -3.0, color: Color::BLACK },
    ]);
    assert_eq!(reach, FilterReach { left: 6.0, top: 9.0, right: 16.0, bottom: 6.0 });
}

#[test]
fn a_short_buffer_is_left_alone() {
    let mut px = vec![9u8; 15];
    filter_pixels(&mut px, 2, 2, 8, false, &[f(FilterKind::Invert, 1.0)], 1.0);
    assert_eq!(px, vec![9u8; 15], "the C++ would write past it");
}
