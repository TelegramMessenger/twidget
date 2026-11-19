// Copyright 2014-2017 The html5ever Project Developers. See the
// COPYRIGHT file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

pub mod atom;
pub mod tendril;
#[macro_use]
#[path = "generated/atoms.rs"]
mod generated_atoms;
#[path = "generated/entities.rs"]
mod generated_entities;

/// Create a [`SmallCharSet`], with each space-separated number stored in the set.
///
/// # Examples
///
/// ```
/// # #[macro_use] extern crate markup5ever;
/// # fn main() {
/// let set = small_char_set!(12 54 42);
/// assert_eq!(set.bits,
///            0b00000000_01000000_00000100_00000000_00000000_00000000_00010000_00000000);
/// # }
/// ```
///
/// [`SmallCharSet`]: struct.SmallCharSet.html
#[macro_export]
macro_rules! small_char_set ( ($($e:expr)+) => (
    $ crate ::SmallCharSet {
        bits: $( (1 << ($e as usize)) )|+
    }
));

pub use generated_atoms::*;

pub mod data {
    pub struct NamedEntities;

    impl NamedEntities {
        #[inline]
        pub fn get(&self, key: &str) -> Option<&'static (u32, u32)> {
            let table = &crate::generated_entities::NAMED_ENTITIES_SORTED;
            table
                .binary_search_by(|(name, _)| name.as_bytes().cmp(key.as_bytes()))
                .ok()
                .and_then(|i| table.get(i))
                .map(|(_, v)| v)
        }
    }

    pub static NAMED_ENTITIES: NamedEntities = NamedEntities;

    pub static C1_REPLACEMENTS: [Option<char>; 32] = [
    Some('\u{20ac}'),
    None,
    Some('\u{201a}'),
    Some('\u{0192}'),
    Some('\u{201e}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{02c6}'),
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{0152}'),
    None,
    Some('\u{017d}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201c}'),
    Some('\u{201d}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{02dc}'),
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203a}'),
    Some('\u{0153}'),
    None,
    Some('\u{017e}'),
    Some('\u{0178}'),
    ];
}
#[macro_use]
pub mod interface;
pub mod serialize;
mod util {
    pub mod buffer_queue;
    pub mod smallcharset;
}

pub use interface::{Attribute, ExpandedName, QualName, TokenizerResult};
pub use util::smallcharset::SmallCharSet;
pub use util::*;
