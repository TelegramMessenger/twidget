//! `strtod.cpp`: litehtml's own `t_strtod`, Tcl's strtod of 1988. It is not C's: it reads no hexadecimal,
//! `inf` or `nan`, keeps at most 18 mantissa digits, scales by a product of the powers 10^(2^i) (so it is not
//! correctly rounded), caps the decimal exponent at 511, and its end is past an `e` or an exponent sign with
//! no digit after them. Where it reads no digit at all the end is the start and the value a zero with the
//! text's sign.

use twidget_cstd::is_space;

const MAX_EXPONENT: i32 = 511;
const POWERS_OF_10: [f64; 9] = [10., 100., 1.0e4, 1.0e8, 1.0e16, 1.0e32, 1.0e64, 1.0e128, 1.0e256];

/// `t_strtod(string, &end)` on a C string: the value and the offset of `end`. The text ends at its first
/// NUL, as `c_str()` does.
pub fn t_strtod(string: &[u8]) -> (f64, usize) {
    let at = |i: usize| string.get(i).copied().unwrap_or(0);

    // Strip off leading blanks and check for a sign. The C locale's isspace: bytes past 0x7f are none
    // (the C++ passes them as negative chars, which the C libraries read as no class).
    let mut p = 0;
    while is_space(at(p)) {
        p += 1;
    }
    let sign;
    if at(p) == b'-' {
        sign = true;
        p += 1;
    } else {
        if at(p) == b'+' {
            p += 1;
        }
        sign = false;
    }

    // Count the number of digits in the mantissa (including the decimal point), and also locate the
    // decimal point.
    let mut dec_pt: i32 = -1;
    let mut mant_size: i32 = 0;
    loop {
        let c = at(p);
        if !c.is_ascii_digit() {
            if c != b'.' || dec_pt >= 0 {
                break;
            }
            dec_pt = mant_size;
        }
        p += 1;
        mant_size = mant_size.wrapping_add(1);
    }

    // Now suck up the digits in the mantissa: two integers of 9 digits each, the digits past 18 ignored.
    let p_exp = p;
    p -= mant_size as usize;
    if dec_pt < 0 {
        dec_pt = mant_size;
    } else {
        mant_size -= 1; // One of the digits was the point.
    }
    let frac_exp;
    if mant_size > 18 {
        frac_exp = dec_pt.wrapping_sub(18);
        mant_size = 18;
    } else {
        frac_exp = dec_pt.wrapping_sub(mant_size);
    }
    let mut fraction;
    if mant_size == 0 {
        // `goto done` with p at the start of the string.
        return (if sign { -0.0 } else { 0.0 }, 0);
    } else {
        let mut frac1: i32 = 0;
        while mant_size > 9 {
            let mut c = at(p);
            p += 1;
            if c == b'.' {
                c = at(p);
                p += 1;
            }
            frac1 = 10 * frac1 + (i32::from(c) - i32::from(b'0'));
            mant_size -= 1;
        }
        let mut frac2: i32 = 0;
        while mant_size > 0 {
            let mut c = at(p);
            p += 1;
            if c == b'.' {
                c = at(p);
                p += 1;
            }
            frac2 = 10 * frac2 + (i32::from(c) - i32::from(b'0'));
            mant_size -= 1;
        }
        fraction = (1.0e9 * f64::from(frac1)) + f64::from(frac2);
    }

    // Skim off the exponent.
    p = p_exp;
    let mut exp: i32 = 0;
    let mut exp_sign = false;
    if at(p) == b'E' || at(p) == b'e' {
        p += 1;
        if at(p) == b'-' {
            exp_sign = true;
            p += 1;
        } else {
            if at(p) == b'+' {
                p += 1;
            }
            exp_sign = false;
        }
        while at(p).is_ascii_digit() {
            if exp <= MAX_EXPONENT {
                exp = exp * 10 + i32::from(at(p) - b'0');
            }
            p += 1;
        }
    }
    exp = if exp_sign { frac_exp.wrapping_sub(exp) } else { frac_exp.wrapping_add(exp) };

    // The exponent as a product of powers of 10, bit by bit, and the fraction scaled by it.
    if exp < 0 {
        exp_sign = true;
        exp = exp.wrapping_neg();
    } else {
        exp_sign = false;
    }
    if exp > MAX_EXPONENT {
        exp = MAX_EXPONENT; // and errno = ERANGE, which nothing reads
    }
    let mut dbl_exp = 1.0;
    let mut d = 0;
    while exp != 0 {
        if exp & 1 != 0 {
            dbl_exp *= POWERS_OF_10[d];
        }
        exp >>= 1;
        d += 1;
    }
    if exp_sign {
        fraction /= dbl_exp;
    } else {
        fraction *= dbl_exp;
    }

    (if sign { -fraction } else { fraction }, p)
}
