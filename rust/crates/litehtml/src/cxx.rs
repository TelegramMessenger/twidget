//! What C++ and its standard library do in the expressions litehtml writes, where Rust's own operations
//! would answer otherwise: `std::min` and `std::max` (the first argument unless the other compares smaller or
//! larger, so NaN and ties keep their place), float to `int` casts (undefined outside `int`'s range and for
//! NaN), and `std::to_string`.

/// `std::min(a, b)`: `b < a ? b : a`.
pub fn min_f32(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `a < b ? b : a`.
pub fn max_f32(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// `std::min(a, b)` on doubles.
pub fn min_f64(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)` on doubles.
pub fn max_f64(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// Whether the C++ standard library is MSVC's on x86, whose `std::min` and `std::max` of an initializer
/// list of floats are vectorized.
const MSVC_STL_X86: bool = cfg!(all(target_env = "msvc", any(target_arch = "x86", target_arch = "x86_64")));

/// `std::min({…})` on floats: the first of the smallest by `<`. A NaN in the list makes it undefined (NaN is
/// no strict weak order), and the standard libraries part there: libc++ and libstdc++ compare from the
/// first element, so a NaN that comes first stays; MSVC's on x86 skips NaNs in a list under 16 bytes (the
/// first of the smallest that is a number, the first element when none is), measured on every order of three.
/// Each is kept where it is the C++'s; MSVC's longer lists, which go its vector path, are not reproduced.
pub fn min_list_f32(values: &[f32]) -> f32 {
    list_extreme(values, |a, b| a < b)
}

/// `std::max({…})` on floats: the first of the largest, as [`min_list_f32`] with `>`.
pub fn max_list_f32(values: &[f32]) -> f32 {
    list_extreme(values, |a, b| a > b)
}

fn list_extreme(values: &[f32], better: impl Fn(f32, f32) -> bool) -> f32 {
    if MSVC_STL_X86 && values.len() * 4 < 16 {
        let mut numbers = values.iter().copied().filter(|v| !v.is_nan());
        let Some(mut best) = numbers.next() else { return values[0] };
        for v in numbers {
            if better(v, best) {
                best = v;
            }
        }
        return best;
    }
    let mut best = values[0];
    for &v in &values[1..] {
        if better(v, best) {
            best = v;
        }
    }
    best
}

/// `static_cast<int>(float)`: truncation toward zero. Outside `int`'s range and for NaN C++ leaves it
/// undefined; x86's `cvttss2si`, which MSVC and clang emit there, gives `INT_MIN`, arm64's `fcvtzs`
/// saturates and gives 0 for NaN, as Rust's `as` does.
pub fn f32_to_int(v: f32) -> i32 {
    if cfg!(any(target_arch = "x86", target_arch = "x86_64")) && (v.is_nan() || v >= 2147483648.0) {
        return i32::MIN;
    }
    v as i32
}

/// `static_cast<int>(double)`, as [`f32_to_int`].
pub fn f64_to_int(v: f64) -> i32 {
    if cfg!(any(target_arch = "x86", target_arch = "x86_64")) && (v.is_nan() || v >= 2147483648.0) {
        return i32::MIN;
    }
    v as i32
}

/// `static_cast<unsigned char>(float)`: undefined outside 0..255; x86 converts to a 32-bit `int` as
/// [`f32_to_int`] and keeps its low byte, arm64 converts to a saturated 32-bit unsigned and keeps its low byte.
pub fn f32_to_byte(v: f32) -> u8 {
    if cfg!(any(target_arch = "x86", target_arch = "x86_64")) { f32_to_int(v) as u8 } else { v as u32 as u8 }
}

/// `std::to_string(int)`.
pub fn to_string_int(v: i32) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// `std::to_string(float)` and `std::to_string(double)`: `%f` of the value as a double.
pub fn to_string_f64(v: f64) -> Vec<u8> {
    if v.is_nan() {
        return nan_text(v).into_bytes();
    }
    twidget_cstd::format_fixed(v, 6).into_bytes()
}

/// How the C library prints a NaN with `%f`. The Windows C runtime names the indefinite NaN x86 computes
/// (sign set, only the quiet bit) `-nan(ind)` and any other quiet one `nan` or `-nan`; the other C libraries
/// print `nan` or `-nan`.
fn nan_text(v: f64) -> String {
    let negative = v.is_sign_negative();
    if cfg!(target_env = "msvc") && v.to_bits() == 0xfff8_0000_0000_0000 {
        return "-nan(ind)".into();
    }
    if negative { "-nan".into() } else { "nan".into() }
}
