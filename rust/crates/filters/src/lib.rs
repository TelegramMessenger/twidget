//! CSS filter functions (`filter`, `backdrop-filter`) for painters (`core/src/filters.h`, `filters.cpp`):
//! the colour functions as matrices, how far a chain reaches past what it filters, and a CPU pass over a
//! painter's own pixels for the ones that paint into memory (Qt, CoreGraphics, Android). The Direct2D
//! painter takes the matrices into its effects.
//!
//! Every float is an `f32` where the C++ has a `float`, in the C++'s order of operations; `std::min`,
//! `std::max` and `std::clamp` are written out as the standard defines them, so a NaN goes where the C++
//! sends it (the first argument of `min` and `max` when nothing compares, the value itself through `clamp`).

#![forbid(unsafe_code)]

use twidget_wire::{Filter, FilterKind};

/// The C++'s `3.14159265f`: the same float.
const PI: f32 = std::f32::consts::PI;

/// A colour function as Filter Effects 1 writes its shorthand equivalent: on straight-alpha colour,
/// out = rgb · in + offset, alpha times `alpha`. `rgb` is row-major, a row per output channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorMatrix {
    pub rgb: [f32; 9],
    pub offset: f32,
    pub alpha: f32,
}

impl Default for ColorMatrix {
    fn default() -> ColorMatrix {
        ColorMatrix { rgb: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], offset: 0.0, alpha: 1.0 }
    }
}

/// How far past its box a chain paints, in CSS px: blurs spread three standard deviations, a drop shadow
/// that and its offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FilterReach {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// `std::min(a, b)`: `b < a ? b : a`.
fn min(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `a < b ? b : a`.
fn max(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// `std::clamp(v, lo, hi)`: `v < lo ? lo : hi < v ? hi : v`.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// `std::lround` of a float, as an `i64` (`long` is 32 bits on Windows, but every caller here clamps first
/// or keeps only the low byte): half away from zero; NaN, which `lroundf` leaves to the C library, gives 0 in
/// the low byte on MSVC's, glibc's and Apple's alike.
fn lround(v: f32) -> i64 {
    if v.is_nan() { 0 } else { v.round() as i64 }
}

/// Sets `out` to the matrix of a colour function; false for blur and drop-shadow, which are none.
pub fn color_matrix(f: &Filter, out: &mut ColorMatrix) -> bool {
    *out = ColorMatrix::default();
    let a = f.amount;
    match f.kind {
        FilterKind::Brightness => {
            out.rgb[0] = a;
            out.rgb[4] = a;
            out.rgb[8] = a;
            true
        }
        FilterKind::Contrast => {
            out.rgb[0] = a;
            out.rgb[4] = a;
            out.rgb[8] = a;
            out.offset = (1.0 - a) / 2.0;
            true
        }
        FilterKind::Invert => {
            let k = min(a, 1.0);
            let v = 1.0 - 2.0 * k;
            out.rgb[0] = v;
            out.rgb[4] = v;
            out.rgb[8] = v;
            out.offset = k;
            true
        }
        FilterKind::Opacity => {
            out.alpha = clamp(a, 0.0, 1.0);
            true
        }
        FilterKind::Grayscale | FilterKind::Sepia => {
            let s = 1.0 - min(a, 1.0);
            let grey = [
                0.2126 + 0.7874 * s,
                0.7152 - 0.7152 * s,
                0.0722 - 0.0722 * s,
                0.2126 - 0.2126 * s,
                0.7152 + 0.2848 * s,
                0.0722 - 0.0722 * s,
                0.2126 - 0.2126 * s,
                0.7152 - 0.7152 * s,
                0.0722 + 0.9278 * s,
            ];
            let sepia = [
                0.393 + 0.607 * s,
                0.769 - 0.769 * s,
                0.189 - 0.189 * s,
                0.349 - 0.349 * s,
                0.686 + 0.314 * s,
                0.168 - 0.168 * s,
                0.272 - 0.272 * s,
                0.534 - 0.534 * s,
                0.131 + 0.869 * s,
            ];
            out.rgb = if f.kind == FilterKind::Grayscale { grey } else { sepia };
            true
        }
        FilterKind::Saturate => {
            let s = a;
            out.rgb = [
                0.213 + 0.787 * s,
                0.715 - 0.715 * s,
                0.072 - 0.072 * s,
                0.213 - 0.213 * s,
                0.715 + 0.285 * s,
                0.072 - 0.072 * s,
                0.213 - 0.213 * s,
                0.715 - 0.715 * s,
                0.072 + 0.928 * s,
            ];
            true
        }
        FilterKind::HueRotate => {
            let r = a * PI / 180.0;
            let c = r.cos();
            let s = r.sin();
            out.rgb = [
                0.213 + c * 0.787 - s * 0.213,
                0.715 - c * 0.715 - s * 0.715,
                0.072 - c * 0.072 + s * 0.928,
                0.213 - c * 0.213 + s * 0.143,
                0.715 + c * 0.285 + s * 0.140,
                0.072 - c * 0.072 - s * 0.283,
                0.213 - c * 0.213 - s * 0.787,
                0.715 - c * 0.715 + s * 0.715,
                0.072 + c * 0.928 + s * 0.072,
            ];
            true
        }
        FilterKind::Blur | FilterKind::DropShadow => false,
    }
}

/// How far past its box `filters` paint.
pub fn filter_reach(filters: &[Filter]) -> FilterReach {
    let mut r = FilterReach::default();
    for f in filters {
        if f.kind == FilterKind::Blur {
            let d = 3.0 * max(f.amount, 0.0);
            r.left += d;
            r.top += d;
            r.right += d;
            r.bottom += d;
        } else if f.kind == FilterKind::DropShadow {
            // The shadow's blur is half the radius as a standard deviation, as the Direct2D painter has it.
            let d = 1.5 * max(f.amount, 0.0);
            r.left = max(r.left, d - f.x);
            r.right = max(r.right, d + f.x);
            r.top = max(r.top, d - f.y);
            r.bottom = max(r.bottom, d + f.y);
        }
    }
    r
}

/// One box blur pass of radius `r` along rows (`horizontal`) or columns, over 4 interleaved channels.
fn box_pass(src: &[f32], dst: &mut [f32], w: i32, h: i32, r: i32, horizontal: bool) {
    let (lines, length) = if horizontal { (h, w) } else { (w, h) };
    let scale = 1.0f32 / (2 * r + 1) as f32;
    for line in 0..lines {
        let at = |i: i32| -> Option<usize> {
            let i = i.clamp(-1, length);
            if i < 0 || i >= length {
                return None;
            }
            Some((if horizontal { line as usize * w as usize + i as usize } else { i as usize * w as usize + line as usize }) * 4)
        };
        let mut sum = [0.0f32; 4];
        // Outside the image is transparent: the sum starts with the pixels that reach position 0, which are the
        // ones in the image only (a radius far past it costs no more than the line).
        let end = i64::from(r).min(i64::from(length) - 1) as i32;
        for i in 0..=end {
            let k = at(i).expect("in the line");
            for c in 0..4 {
                sum[c] += src[k + c];
            }
        }
        for i in 0..length {
            let out = at(i).expect("in the line");
            for c in 0..4 {
                dst[out + c] = sum[c] * scale;
            }
            if let Some(k) = at(i - r) {
                for c in 0..4 {
                    sum[c] -= src[k + c];
                }
            }
            if let Some(k) = at(i + r + 1) {
                for c in 0..4 {
                    sum[c] += src[k + c];
                }
            }
        }
    }
}

const MAX_SIGMA: f32 = 100000.0;

/// A Gaussian of standard deviation `sigma` px as three box blurs (the box sizes SVG's feGaussianBlur gives).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn gaussian(px: &mut [f32], w: i32, h: i32, sigma: f32) {
    if !(sigma >= 0.5) {
        return; // NaN too
    }
    // An infinite or huge deviation (a scale or amount from a painter) is the same blur as this one at any size an
    // image has, and keeps the radius an int.
    let sigma = min(sigma, MAX_SIGMA);
    let d = (sigma * 3.0 * (2.0 * PI).sqrt() / 4.0 + 0.5).floor() as i32;
    let r = 1.max(d / 2);
    let mut tmp = vec![0.0f32; px.len()];
    for _ in 0..3 {
        box_pass(px, &mut tmp, w, h, r, true);
        box_pass(&tmp, px, w, h, r, false);
    }
}

/// Runs `filters` in order over premultiplied 8-bit pixels, in place: `height` rows of `width` pixels,
/// `stride` bytes apart. `bgra`: the bytes are B, G, R, A (QImage's premultiplied ARGB32 on a little-endian
/// machine) rather than R, G, B, A. `scale`: device px per CSS px, for blur radii and shadow offsets.
///
/// The C++ reads and writes past a buffer too short for its rows (undefined behaviour); this leaves such a
/// buffer as it is.
pub fn filter_pixels(pixels: &mut [u8], w: i32, h: i32, stride: usize, bgra: bool, filters: &[Filter], scale: f32) {
    if w <= 0 || h <= 0 || filters.is_empty() {
        return;
    }
    let (wu, hu) = (w as usize, h as usize);
    let needed = (hu - 1).checked_mul(stride).and_then(|rows| rows.checked_add(wu.checked_mul(4)?));
    if needed.is_none_or(|n| n > pixels.len()) {
        return;
    }
    // Premultiplied R, G, B, A in floats, whatever the bytes' order.
    let (red, blue) = if bgra { (2, 0) } else { (0, 2) };
    let mut px = vec![0.0f32; wu * hu * 4];
    for y in 0..hu {
        for x in 0..wu {
            let s = &pixels[y * stride + x * 4..][..4];
            let d = &mut px[(y * wu + x) * 4..][..4];
            d[0] = f32::from(s[red]) / 255.0;
            d[1] = f32::from(s[1]) / 255.0;
            d[2] = f32::from(s[blue]) / 255.0;
            d[3] = f32::from(s[3]) / 255.0;
        }
    }
    for f in filters {
        let mut m = ColorMatrix::default();
        if color_matrix(f, &mut m) {
            for p in px.chunks_exact_mut(4) {
                let a = p[3];
                if a <= 0.0 {
                    continue;
                }
                let c = [p[0] / a, p[1] / a, p[2] / a];
                let na = clamp(a * m.alpha, 0.0, 1.0);
                for o in 0..3 {
                    let v = m.rgb[o * 3] * c[0] + m.rgb[o * 3 + 1] * c[1] + m.rgb[o * 3 + 2] * c[2] + m.offset;
                    p[o] = clamp(v, 0.0, 1.0) * na;
                }
                p[3] = na;
            }
        } else if f.kind == FilterKind::Blur {
            gaussian(&mut px, w, h, f.amount * scale);
        } else if f.kind == FilterKind::DropShadow {
            // The alpha in the shadow's colour, blurred and moved, under the image.
            let mut shadow = vec![0.0f32; px.len()];
            // Past the image either way the shadow misses it; NaN is no offset.
            let reach = w.wrapping_add(h) as f32;
            let offset = |v: f32| -> i32 {
                let moved = v * scale;
                if moved.is_nan() { 0 } else { lround(clamp(moved, -reach, reach)) as i32 }
            };
            let dx = offset(f.x);
            let dy = offset(f.y);
            let color =
                [f32::from(f.color.r) / 255.0, f32::from(f.color.g) / 255.0, f32::from(f.color.b) / 255.0, f32::from(f.color.a) / 255.0];
            for y in 0..h {
                for x in 0..w {
                    let sx = x - dx;
                    let sy = y - dy;
                    if sx < 0 || sy < 0 || sx >= w || sy >= h {
                        continue;
                    }
                    let a = px[(sy as usize * wu + sx as usize) * 4 + 3] * color[3];
                    let d = &mut shadow[(y as usize * wu + x as usize) * 4..][..4];
                    d[0] = color[0] * a;
                    d[1] = color[1] * a;
                    d[2] = color[2] * a;
                    d[3] = a;
                }
            }
            gaussian(&mut shadow, w, h, f.amount * scale / 2.0);
            for (p, s) in px.chunks_exact_mut(4).zip(shadow.chunks_exact(4)) {
                let under = 1.0 - p[3];
                for c in 0..4 {
                    p[c] += s[c] * under;
                }
            }
        }
    }
    let byte = |v: f32| lround(clamp(v, 0.0, 1.0) * 255.0) as u8;
    for y in 0..hu {
        for x in 0..wu {
            let s = &px[(y * wu + x) * 4..][..4];
            let d = &mut pixels[y * stride + x * 4..][..4];
            d[red] = byte(s[0]);
            d[1] = byte(s[1]);
            d[blue] = byte(s[2]);
            d[3] = byte(s[3]);
        }
    }
}
