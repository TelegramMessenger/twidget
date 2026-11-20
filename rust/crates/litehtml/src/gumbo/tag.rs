//! tag.c: tag names and the gperf hash that finds a tag from its name, and parser.c's
//! `gumbo_normalize_svg_tagname`.

use super::tables::{TAG_ASSO_VALUES, TAG_MAP, TAG_NAMES, TAG_SIZES, kSvgTagReplacements};
use super::{GUMBO_TAG_LAST, GUMBO_TAG_UNKNOWN, GumboStringPiece, GumboTag};

/// `gumbo_normalized_tagname`: "" for GUMBO_TAG_UNKNOWN and GUMBO_TAG_LAST.
pub fn gumbo_normalized_tagname(tag: GumboTag) -> &'static [u8] {
    TAG_NAMES[tag as usize]
}

/// `gumbo_tag_from_original_text`: the name in a tag's original text `<name ...>` or `</name...>`. The asserts
/// are the release build's (none); a byte past the input reads as the NUL the C string ends with.
pub fn gumbo_tag_from_original_text(input: &[u8], text: &mut GumboStringPiece) {
    let Some(data) = text.data else { return };
    let at = |i: usize| input.get(i).copied().unwrap_or(0);
    if at(data + 1) == b'/' {
        text.data = Some(data + 2);
        text.length = text.length.wrapping_sub(3);
    } else {
        text.data = Some(data + 1);
        text.length = text.length.wrapping_sub(2);
        let start = data + 1;
        let mut c = start;
        while c != start.wrapping_add(text.length) {
            let ch = at(c);
            if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\x0c' || ch == b'/' {
                text.length = c - start;
                break;
            }
            c += 1;
        }
    }
}

/// `tolower` of a `char` in the C locale: ASCII letters only, every other byte (a negative `char` included) as it is.
fn case_memcmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    for i in 0..n {
        let c1 = s1[i].to_ascii_lowercase();
        let c2 = s2.get(i).copied().unwrap_or(0).to_ascii_lowercase();
        if c1 != c2 {
            return i32::from(c1) - i32::from(c2);
        }
    }
    0
}

fn tag_hash(s: &[u8]) -> u32 {
    let len = s.len();
    let mut hval = len as u32;
    if len != 1 {
        hval = hval.wrapping_add(u32::from(TAG_ASSO_VALUES[usize::from(s[1]) + 3]));
    }
    hval = hval.wrapping_add(u32::from(TAG_ASSO_VALUES[usize::from(s[0])]));
    hval.wrapping_add(u32::from(TAG_ASSO_VALUES[usize::from(s[len - 1])]))
}

/// `gumbo_tagn_enum`.
pub fn gumbo_tagn_enum(tagname: &[u8]) -> GumboTag {
    let length = tagname.len();
    if length != 0 {
        let key = tag_hash(tagname);
        if (key as usize) < TAG_MAP.len() {
            let tag = TAG_MAP[key as usize];
            if length == usize::from(TAG_SIZES[tag as usize]) && case_memcmp(tagname, TAG_NAMES[tag as usize], length) == 0 {
                return tag;
            }
        }
    }
    GUMBO_TAG_UNKNOWN
}

/// `gumbo_tag_enum` of a C string.
pub fn gumbo_tag_enum(tagname: &[u8]) -> GumboTag {
    gumbo_tagn_enum(super::c_str(tagname))
}

/// `gumbo_normalize_svg_tagname`: the camel-cased name of an SVG tag, or None.
pub fn gumbo_normalize_svg_tagname(tag: &[u8]) -> Option<&'static [u8]> {
    kSvgTagReplacements.iter().find(|(from, _)| from.len() == tag.len() && from.eq_ignore_ascii_case(tag)).map(|(_, to)| *to)
}

const _: () = assert!(TAG_NAMES.len() == GUMBO_TAG_LAST as usize + 1);
