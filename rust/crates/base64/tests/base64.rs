//! tests/core_tests.cpp's base64 checks (the canvas JSON's putImageData pixels, getImageData's answer), and
//! the same bytes against the C++ (`oracle/base64_oracle`, built from core/src/encode.cpp) with every length
//! around the padding. The comparison skips without `TWIDGET_ORACLE_DIR`; `tools/gate.py` sets it.

use twidget_base64::encode;
use twidget_testkit::{Comparison, Lcg, Oracle, framed};

#[test]
fn pixels_are_base64() {
    assert_eq!(encode(&[1, 2, 3, 4]), "AQIDBA==", "putImageData pixels are base64");
    assert_eq!(encode(&[7; 8]), "BwcHBwcHBwc=", "getImageData returns the backend's pixels as base64");
    assert_eq!(encode(&[]), "");
    assert_eq!(encode(b"\xfb\xff\xbf"), "+/+/");
}

#[test]
fn bytes_alike() {
    let Some(oracle) = Oracle::find("base64_oracle") else { return };
    let mut random = Lcg::new(0xba5e_0001);
    let mut c = Comparison::new();
    for n in 0..300 {
        for _ in 0..8 {
            let bytes: Vec<u8> = (0..n).map(|_| random.next() as u8).collect();
            c.case(&framed(b'b', &bytes), (1, encode(&bytes).into_bytes()), format!("{n} bytes"));
        }
    }
    for n in [4096 * 4, 65535, 65536, 1 << 20] {
        let bytes: Vec<u8> = (0..n).map(|i| (i * 7 + i / 255) as u8).collect();
        c.case(&framed(b'b', &bytes), (1, encode(&bytes).into_bytes()), format!("{n} bytes"));
    }
    let tally = c.check(&oracle);
    eprintln!("base64: {} answers alike", tally[1]);
}
