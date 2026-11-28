//! What the helper's tests share: the oracle programs built from a tdesktop checkout, which skip with a word when
//! the oracle was configured without one (`oracle/helper_oracles.cmake`'s marker), and the answers' fields.

#![allow(dead_code, unused_imports)]

use std::path::PathBuf;

pub mod docs;
#[path = "../../../ends/tests/common/mod.rs"]
pub mod ends;
#[path = "../../../events/tests/common/mod.rs"]
pub mod events;
pub mod glue;
pub mod tap;

pub use twidget_testkit::{Comparison, Lcg, Oracle, framed, mutate};

/// The helper oracle `name`, as `Oracle::find` finds one, or None when the oracle was built without tdesktop's
/// sources, whatever `TWIDGET_ORACLE_REQUIRED` says: the gate passes without them.
pub fn helper_oracle(name: &str) -> Option<Oracle> {
    let Some(dir) = std::env::var_os("TWIDGET_ORACLE_DIR") else { return Oracle::find(name) };
    let marker = PathBuf::from(&dir).join("helper_oracles.txt");
    match std::fs::read_to_string(&marker) {
        Ok(text) if text.starts_with("built") => Oracle::find(name),
        Ok(text) => {
            eprintln!(
                "the oracle was configured without tdesktop's sources ({}): the comparison of the helper's glue is skipped",
                text.trim()
            );
            None
        }
        Err(_) => {
            assert!(std::env::var_os("TWIDGET_ORACLE_REQUIRED").is_none(), "no {} beside the oracle programs", marker.display());
            eprintln!("no {}: the comparison of the helper's glue is skipped", marker.display());
            None
        }
    }
}

/// Little-endian answer fields, as `oracle_io.h`'s `Out` writes them.
#[derive(Clone, Debug, Default)]
pub struct Out(pub Vec<u8>);

impl Out {
    pub fn u8(&mut self, v: u8) -> &mut Out {
        self.0.push(v);
        self
    }
    pub fn flag(&mut self, v: bool) -> &mut Out {
        self.u8(u8::from(v))
    }
    pub fn u32(&mut self, v: u32) -> &mut Out {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Out {
        self.u32(v as u32)
    }
    pub fn u64(&mut self, v: u64) -> &mut Out {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i64(&mut self, v: i64) -> &mut Out {
        self.u64(v as u64)
    }
    pub fn f64(&mut self, v: f64) -> &mut Out {
        self.u64(v.to_bits())
    }
    pub fn str(&mut self, v: &[u8]) -> &mut Out {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
}
