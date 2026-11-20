//! parser.c (with insertion_mode.h): tree construction, every insertion mode, the adoption agency, foster
//! parenting, foreign content, fragments, and the fork's stop at 400 open elements or a bounded size of clones.
//!
//! The C's `_current_token` always points at the token the main loop handles; here that token is
//! [`Parser::token`], which the handlers read and change as the C's do through their `token` argument.

use super::error::{Errors, GUMBO_ERR_PARSER, GUMBO_ERR_UNACKNOWLEDGED_SELF_CLOSING_TAG, GumboError, GumboErrorValue};
use super::tables::{
    kForeignAttributeReplacements, kLimitedQuirksPublicIdPrefixes, kLimitedQuirksRequiresSystemIdPublicIdPrefixes,
    kQuirksModePublicIdExactMatches, kQuirksModePublicIdPrefixes, kQuirksModeSystemIdExactMatches, kSvgAttributeReplacements,
};
use super::tokenizer::*;
use super::utf8::kUtf8ReplacementChar;
use super::*;

pub type GumboInsertionMode = u32;
pub const GUMBO_INSERTION_MODE_INITIAL: GumboInsertionMode = 0;
pub const GUMBO_INSERTION_MODE_BEFORE_HTML: GumboInsertionMode = 1;
pub const GUMBO_INSERTION_MODE_BEFORE_HEAD: GumboInsertionMode = 2;
pub const GUMBO_INSERTION_MODE_IN_HEAD: GumboInsertionMode = 3;
pub const GUMBO_INSERTION_MODE_IN_HEAD_NOSCRIPT: GumboInsertionMode = 4;
pub const GUMBO_INSERTION_MODE_AFTER_HEAD: GumboInsertionMode = 5;
pub const GUMBO_INSERTION_MODE_IN_BODY: GumboInsertionMode = 6;
pub const GUMBO_INSERTION_MODE_TEXT: GumboInsertionMode = 7;
pub const GUMBO_INSERTION_MODE_IN_TABLE: GumboInsertionMode = 8;
pub const GUMBO_INSERTION_MODE_IN_TABLE_TEXT: GumboInsertionMode = 9;
pub const GUMBO_INSERTION_MODE_IN_CAPTION: GumboInsertionMode = 10;
pub const GUMBO_INSERTION_MODE_IN_COLUMN_GROUP: GumboInsertionMode = 11;
pub const GUMBO_INSERTION_MODE_IN_TABLE_BODY: GumboInsertionMode = 12;
pub const GUMBO_INSERTION_MODE_IN_ROW: GumboInsertionMode = 13;
pub const GUMBO_INSERTION_MODE_IN_CELL: GumboInsertionMode = 14;
pub const GUMBO_INSERTION_MODE_IN_SELECT: GumboInsertionMode = 15;
pub const GUMBO_INSERTION_MODE_IN_SELECT_IN_TABLE: GumboInsertionMode = 16;
pub const GUMBO_INSERTION_MODE_IN_TEMPLATE: GumboInsertionMode = 17;
pub const GUMBO_INSERTION_MODE_AFTER_BODY: GumboInsertionMode = 18;
pub const GUMBO_INSERTION_MODE_IN_FRAMESET: GumboInsertionMode = 19;
pub const GUMBO_INSERTION_MODE_AFTER_FRAMESET: GumboInsertionMode = 20;
pub const GUMBO_INSERTION_MODE_AFTER_AFTER_BODY: GumboInsertionMode = 21;
pub const GUMBO_INSERTION_MODE_AFTER_AFTER_FRAMESET: GumboInsertionMode = 22;
/// Not an insertion mode: where [`Reached::modes`] counts the tokens foreign content handles.
pub const REACHED_FOREIGN_CONTENT: usize = 23;

/// The fork's limits: open elements at once (a tree is at most twice as deep), and bytes cloned on top of the
/// input's length.
const kMaxOpenElements: usize = 400;
const kMaxClonedBytes: usize = 1 << 20;
/// `sizeof(GumboNode)` on the 64-bit targets the helper builds for, which each clone counts.
pub const SIZEOF_GUMBO_NODE: usize = 128;

const kDoctypeHtml: &[u8] = b"html";
const kPublicIdHtml4_0: &[u8] = b"-//W3C//DTD HTML 4.0//EN";
const kPublicIdHtml4_01: &[u8] = b"-//W3C//DTD HTML 4.01//EN";
const kPublicIdXhtml1_0: &[u8] = b"-//W3C//DTD XHTML 1.0 Strict//EN";
const kPublicIdXhtml1_1: &[u8] = b"-//W3C//DTD XHTML 1.1//EN";
const kSystemIdRecHtml4_0: &[u8] = b"http://www.w3.org/TR/REC-html40/strict.dtd";
const kSystemIdHtml4: &[u8] = b"http://www.w3.org/TR/html4/strict.dtd";
const kSystemIdXhtmlStrict1_1: &[u8] = b"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd";
const kSystemIdXhtml1_1: &[u8] = b"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd";
const kSystemIdLegacyCompat: &[u8] = b"about:legacy-compat";

/// `kActiveFormattingScopeMarker` in the list of active formatting elements. The C's marker is a zeroed static
/// node, which [`Parser::count_formatting_elements_of_tag`] reads as an `<html>` without attributes.
const MARKER: NodeId = usize::MAX;

/// `gumbo_tagset`: a byte per tag, the bit of each namespace it is in for that set.
type TagSet = [u8; GUMBO_TAG_LAST as usize];

/// A tagset as the C's designated initializers make it: each entry assigns its namespace's bit.
const fn tagset(entries: &[(GumboTag, GumboNamespaceEnum)]) -> TagSet {
    let mut set = [0u8; GUMBO_TAG_LAST as usize];
    let mut i = 0;
    while i < entries.len() {
        set[entries[i].0 as usize] = 1 << entries[i].1;
        i += 1;
    }
    set
}

/// A tagset of HTML tags (`TAG(...)`).
const fn tags(list: &[GumboTag]) -> TagSet {
    let mut set = [0u8; GUMBO_TAG_LAST as usize];
    let mut i = 0;
    while i < list.len() {
        set[list[i] as usize] = 1 << GUMBO_NAMESPACE_HTML;
        i += 1;
    }
    set
}

const H: GumboNamespaceEnum = GUMBO_NAMESPACE_HTML;
const S: GumboNamespaceEnum = GUMBO_NAMESPACE_SVG;
const M: GumboNamespaceEnum = GUMBO_NAMESPACE_MATHML;

static TABLE_CONTEXT_FOSTER: TagSet = tags(&[GUMBO_TAG_TABLE, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD, GUMBO_TAG_TR]);
static TABLE_ROW_CONTEXT: TagSet = tags(&[GUMBO_TAG_HTML, GUMBO_TAG_TR, GUMBO_TAG_TEMPLATE]);
static TABLE_CONTEXT: TagSet = tags(&[GUMBO_TAG_HTML, GUMBO_TAG_TABLE, GUMBO_TAG_TEMPLATE]);
static TABLE_BODY_CONTEXT: TagSet = tags(&[GUMBO_TAG_HTML, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD, GUMBO_TAG_TEMPLATE]);
static MATHML_INTEGRATION: TagSet =
    tagset(&[(GUMBO_TAG_MI, M), (GUMBO_TAG_MO, M), (GUMBO_TAG_MN, M), (GUMBO_TAG_MS, M), (GUMBO_TAG_MTEXT, M)]);
static HTML_INTEGRATION: TagSet = tagset(&[(GUMBO_TAG_FOREIGNOBJECT, S), (GUMBO_TAG_DESC, S), (GUMBO_TAG_TITLE, S)]);
static HTML_ONLY: TagSet = tags(&[GUMBO_TAG_HTML]);
const SCOPE_ENTRIES: [(GumboTag, GumboNamespaceEnum); 18] = [
    (GUMBO_TAG_APPLET, H),
    (GUMBO_TAG_CAPTION, H),
    (GUMBO_TAG_HTML, H),
    (GUMBO_TAG_TABLE, H),
    (GUMBO_TAG_TD, H),
    (GUMBO_TAG_TH, H),
    (GUMBO_TAG_MARQUEE, H),
    (GUMBO_TAG_OBJECT, H),
    (GUMBO_TAG_TEMPLATE, H),
    (GUMBO_TAG_MI, M),
    (GUMBO_TAG_MO, M),
    (GUMBO_TAG_MN, M),
    (GUMBO_TAG_MS, M),
    (GUMBO_TAG_MTEXT, M),
    (GUMBO_TAG_ANNOTATION_XML, M),
    (GUMBO_TAG_FOREIGNOBJECT, S),
    (GUMBO_TAG_DESC, S),
    (GUMBO_TAG_TITLE, S),
];
static SCOPE: TagSet = tagset(&SCOPE_ENTRIES);
static LIST_SCOPE: TagSet = {
    let mut set = tagset(&SCOPE_ENTRIES);
    set[GUMBO_TAG_OL as usize] = 1 << H;
    set[GUMBO_TAG_UL as usize] = 1 << H;
    set
};
static BUTTON_SCOPE: TagSet = {
    let mut set = tagset(&SCOPE_ENTRIES);
    set[GUMBO_TAG_BUTTON as usize] = 1 << H;
    set
};
static TABLE_SCOPE: TagSet = tags(&[GUMBO_TAG_HTML, GUMBO_TAG_TABLE, GUMBO_TAG_TEMPLATE]);
static SELECT_SCOPE: TagSet = tags(&[GUMBO_TAG_OPTGROUP, GUMBO_TAG_OPTION]);
static IMPLIED_END_TAGS: TagSet = tags(&[
    GUMBO_TAG_DD,
    GUMBO_TAG_DT,
    GUMBO_TAG_LI,
    GUMBO_TAG_OPTION,
    GUMBO_TAG_OPTGROUP,
    GUMBO_TAG_P,
    GUMBO_TAG_RP,
    GUMBO_TAG_RB,
    GUMBO_TAG_RT,
    GUMBO_TAG_RTC,
]);
static IMPLIED_END_TAGS_THOROUGHLY: TagSet = tags(&[
    GUMBO_TAG_CAPTION,
    GUMBO_TAG_COLGROUP,
    GUMBO_TAG_DD,
    GUMBO_TAG_DT,
    GUMBO_TAG_LI,
    GUMBO_TAG_OPTION,
    GUMBO_TAG_OPTGROUP,
    GUMBO_TAG_P,
    GUMBO_TAG_RP,
    GUMBO_TAG_RT,
    GUMBO_TAG_RTC,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_HEAD,
    GUMBO_TAG_TR,
]);
static SPECIAL: TagSet = tagset(&[
    (GUMBO_TAG_ADDRESS, H),
    (GUMBO_TAG_APPLET, H),
    (GUMBO_TAG_AREA, H),
    (GUMBO_TAG_ARTICLE, H),
    (GUMBO_TAG_ASIDE, H),
    (GUMBO_TAG_BASE, H),
    (GUMBO_TAG_BASEFONT, H),
    (GUMBO_TAG_BGSOUND, H),
    (GUMBO_TAG_BLOCKQUOTE, H),
    (GUMBO_TAG_BODY, H),
    (GUMBO_TAG_BR, H),
    (GUMBO_TAG_BUTTON, H),
    (GUMBO_TAG_CAPTION, H),
    (GUMBO_TAG_CENTER, H),
    (GUMBO_TAG_COL, H),
    (GUMBO_TAG_COLGROUP, H),
    (GUMBO_TAG_DD, H),
    (GUMBO_TAG_DETAILS, H),
    (GUMBO_TAG_DIR, H),
    (GUMBO_TAG_DIV, H),
    (GUMBO_TAG_DL, H),
    (GUMBO_TAG_DT, H),
    (GUMBO_TAG_EMBED, H),
    (GUMBO_TAG_FIELDSET, H),
    (GUMBO_TAG_FIGCAPTION, H),
    (GUMBO_TAG_FIGURE, H),
    (GUMBO_TAG_FOOTER, H),
    (GUMBO_TAG_FORM, H),
    (GUMBO_TAG_FRAME, H),
    (GUMBO_TAG_FRAMESET, H),
    (GUMBO_TAG_H1, H),
    (GUMBO_TAG_H2, H),
    (GUMBO_TAG_H3, H),
    (GUMBO_TAG_H4, H),
    (GUMBO_TAG_H5, H),
    (GUMBO_TAG_H6, H),
    (GUMBO_TAG_HEAD, H),
    (GUMBO_TAG_HEADER, H),
    (GUMBO_TAG_HGROUP, H),
    (GUMBO_TAG_HR, H),
    (GUMBO_TAG_HTML, H),
    (GUMBO_TAG_IFRAME, H),
    (GUMBO_TAG_IMG, H),
    (GUMBO_TAG_INPUT, H),
    (GUMBO_TAG_LI, H),
    (GUMBO_TAG_LINK, H),
    (GUMBO_TAG_LISTING, H),
    (GUMBO_TAG_MARQUEE, H),
    (GUMBO_TAG_MENU, H),
    (GUMBO_TAG_META, H),
    (GUMBO_TAG_NAV, H),
    (GUMBO_TAG_NOEMBED, H),
    (GUMBO_TAG_NOFRAMES, H),
    (GUMBO_TAG_NOSCRIPT, H),
    (GUMBO_TAG_OBJECT, H),
    (GUMBO_TAG_OL, H),
    (GUMBO_TAG_P, H),
    (GUMBO_TAG_PARAM, H),
    (GUMBO_TAG_PLAINTEXT, H),
    (GUMBO_TAG_PRE, H),
    (GUMBO_TAG_SCRIPT, H),
    (GUMBO_TAG_SECTION, H),
    (GUMBO_TAG_SELECT, H),
    (GUMBO_TAG_STYLE, H),
    (GUMBO_TAG_SUMMARY, H),
    (GUMBO_TAG_TABLE, H),
    (GUMBO_TAG_TBODY, H),
    (GUMBO_TAG_TD, H),
    (GUMBO_TAG_TEMPLATE, H),
    (GUMBO_TAG_TEXTAREA, H),
    (GUMBO_TAG_TFOOT, H),
    (GUMBO_TAG_TH, H),
    (GUMBO_TAG_THEAD, H),
    (GUMBO_TAG_TITLE, H),
    (GUMBO_TAG_TR, H),
    (GUMBO_TAG_UL, H),
    (GUMBO_TAG_WBR, H),
    (GUMBO_TAG_XMP, H),
    (GUMBO_TAG_MI, M),
    (GUMBO_TAG_MO, M),
    (GUMBO_TAG_MN, M),
    (GUMBO_TAG_MS, M),
    (GUMBO_TAG_MTEXT, M),
    (GUMBO_TAG_ANNOTATION_XML, M),
    (GUMBO_TAG_FOREIGNOBJECT, S),
    (GUMBO_TAG_DESC, S),
]);
static DD_DT: TagSet = tags(&[GUMBO_TAG_DD, GUMBO_TAG_DT]);
static ADDRESS_DIV_P: TagSet = tags(&[GUMBO_TAG_ADDRESS, GUMBO_TAG_DIV, GUMBO_TAG_P]);
static HEAD_BODY_HTML_BR: TagSet = tags(&[GUMBO_TAG_HEAD, GUMBO_TAG_BODY, GUMBO_TAG_HTML, GUMBO_TAG_BR]);
static BODY_HTML_BR: TagSet = tags(&[GUMBO_TAG_BODY, GUMBO_TAG_HTML, GUMBO_TAG_BR]);
static BASE_BASEFONT_BGSOUND_LINK: TagSet = tags(&[GUMBO_TAG_BASE, GUMBO_TAG_BASEFONT, GUMBO_TAG_BGSOUND, GUMBO_TAG_LINK]);
static NOFRAMES_STYLE: TagSet = tags(&[GUMBO_TAG_NOFRAMES, GUMBO_TAG_STYLE]);
static NOSCRIPT_IN_HEAD: TagSet =
    tags(&[GUMBO_TAG_BASEFONT, GUMBO_TAG_BGSOUND, GUMBO_TAG_LINK, GUMBO_TAG_META, GUMBO_TAG_NOFRAMES, GUMBO_TAG_STYLE]);
static HEAD_NOSCRIPT: TagSet = tags(&[GUMBO_TAG_HEAD, GUMBO_TAG_NOSCRIPT]);
static IN_HEAD_START: TagSet = tags(&[
    GUMBO_TAG_BASE,
    GUMBO_TAG_BASEFONT,
    GUMBO_TAG_BGSOUND,
    GUMBO_TAG_LINK,
    GUMBO_TAG_META,
    GUMBO_TAG_NOFRAMES,
    GUMBO_TAG_SCRIPT,
    GUMBO_TAG_STYLE,
    GUMBO_TAG_TEMPLATE,
    GUMBO_TAG_TITLE,
]);
static BODY_EOF_ALLOWED: TagSet = tags(&[
    GUMBO_TAG_DD,
    GUMBO_TAG_DT,
    GUMBO_TAG_LI,
    GUMBO_TAG_P,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
    GUMBO_TAG_BODY,
    GUMBO_TAG_HTML,
]);
static BODY_HTML: TagSet = tags(&[GUMBO_TAG_BODY, GUMBO_TAG_HTML]);
static BODY_END_ALLOWED: TagSet = tags(&[
    GUMBO_TAG_DD,
    GUMBO_TAG_DT,
    GUMBO_TAG_LI,
    GUMBO_TAG_OPTGROUP,
    GUMBO_TAG_OPTION,
    GUMBO_TAG_P,
    GUMBO_TAG_RB,
    GUMBO_TAG_RP,
    GUMBO_TAG_RT,
    GUMBO_TAG_RTC,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
    GUMBO_TAG_BODY,
    GUMBO_TAG_HTML,
]);
static BLOCK_START: TagSet = tags(&[
    GUMBO_TAG_ADDRESS,
    GUMBO_TAG_ARTICLE,
    GUMBO_TAG_ASIDE,
    GUMBO_TAG_BLOCKQUOTE,
    GUMBO_TAG_CENTER,
    GUMBO_TAG_DETAILS,
    GUMBO_TAG_DIALOG,
    GUMBO_TAG_DIR,
    GUMBO_TAG_DIV,
    GUMBO_TAG_DL,
    GUMBO_TAG_FIELDSET,
    GUMBO_TAG_FIGCAPTION,
    GUMBO_TAG_FIGURE,
    GUMBO_TAG_FOOTER,
    GUMBO_TAG_HEADER,
    GUMBO_TAG_HGROUP,
    GUMBO_TAG_MENU,
    GUMBO_TAG_MAIN,
    GUMBO_TAG_NAV,
    GUMBO_TAG_OL,
    GUMBO_TAG_P,
    GUMBO_TAG_SECTION,
    GUMBO_TAG_SUMMARY,
    GUMBO_TAG_UL,
    GUMBO_TAG_SEARCH,
]);
static HEADINGS: TagSet = tags(&[GUMBO_TAG_H1, GUMBO_TAG_H2, GUMBO_TAG_H3, GUMBO_TAG_H4, GUMBO_TAG_H5, GUMBO_TAG_H6]);
static PRE_LISTING: TagSet = tags(&[GUMBO_TAG_PRE, GUMBO_TAG_LISTING]);
static BLOCK_END: TagSet = tags(&[
    GUMBO_TAG_ADDRESS,
    GUMBO_TAG_ARTICLE,
    GUMBO_TAG_ASIDE,
    GUMBO_TAG_BLOCKQUOTE,
    GUMBO_TAG_BUTTON,
    GUMBO_TAG_CENTER,
    GUMBO_TAG_DETAILS,
    GUMBO_TAG_DIALOG,
    GUMBO_TAG_DIR,
    GUMBO_TAG_DIV,
    GUMBO_TAG_DL,
    GUMBO_TAG_FIELDSET,
    GUMBO_TAG_FIGCAPTION,
    GUMBO_TAG_FIGURE,
    GUMBO_TAG_FOOTER,
    GUMBO_TAG_HEADER,
    GUMBO_TAG_HGROUP,
    GUMBO_TAG_LISTING,
    GUMBO_TAG_MAIN,
    GUMBO_TAG_MENU,
    GUMBO_TAG_NAV,
    GUMBO_TAG_OL,
    GUMBO_TAG_PRE,
    GUMBO_TAG_SECTION,
    GUMBO_TAG_SUMMARY,
    GUMBO_TAG_UL,
    GUMBO_TAG_SEARCH,
]);
static FORMATTING: TagSet = tags(&[
    GUMBO_TAG_B,
    GUMBO_TAG_BIG,
    GUMBO_TAG_CODE,
    GUMBO_TAG_EM,
    GUMBO_TAG_FONT,
    GUMBO_TAG_I,
    GUMBO_TAG_S,
    GUMBO_TAG_SMALL,
    GUMBO_TAG_STRIKE,
    GUMBO_TAG_STRONG,
    GUMBO_TAG_TT,
    GUMBO_TAG_U,
]);
static FORMATTING_END: TagSet = tags(&[
    GUMBO_TAG_A,
    GUMBO_TAG_B,
    GUMBO_TAG_BIG,
    GUMBO_TAG_CODE,
    GUMBO_TAG_EM,
    GUMBO_TAG_FONT,
    GUMBO_TAG_I,
    GUMBO_TAG_NOBR,
    GUMBO_TAG_S,
    GUMBO_TAG_SMALL,
    GUMBO_TAG_STRIKE,
    GUMBO_TAG_STRONG,
    GUMBO_TAG_TT,
    GUMBO_TAG_U,
]);
static APPLET_MARQUEE_OBJECT: TagSet = tags(&[GUMBO_TAG_APPLET, GUMBO_TAG_MARQUEE, GUMBO_TAG_OBJECT]);
static VOID_IN_BODY: TagSet =
    tags(&[GUMBO_TAG_AREA, GUMBO_TAG_BR, GUMBO_TAG_EMBED, GUMBO_TAG_IMG, GUMBO_TAG_IMAGE, GUMBO_TAG_KEYGEN, GUMBO_TAG_WBR]);
static PARAM_SOURCE_TRACK: TagSet = tags(&[GUMBO_TAG_PARAM, GUMBO_TAG_SOURCE, GUMBO_TAG_TRACK]);
static OPTION_OPTGROUP: TagSet = tags(&[GUMBO_TAG_OPTION, GUMBO_TAG_OPTGROUP]);
static RUBY_PARTS: TagSet = tags(&[GUMBO_TAG_RB, GUMBO_TAG_RP, GUMBO_TAG_RT, GUMBO_TAG_RTC]);
static RT_RP: TagSet = tags(&[GUMBO_TAG_RT, GUMBO_TAG_RP]);
static TABLE_PARTS_IN_BODY: TagSet = tags(&[
    GUMBO_TAG_CAPTION,
    GUMBO_TAG_COL,
    GUMBO_TAG_COLGROUP,
    GUMBO_TAG_FRAME,
    GUMBO_TAG_HEAD,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
]);
static TABLE_SECTIONS_AND_CELLS: TagSet =
    tags(&[GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD, GUMBO_TAG_TD, GUMBO_TAG_TH, GUMBO_TAG_TR]);
static TD_TH_TR: TagSet = tags(&[GUMBO_TAG_TD, GUMBO_TAG_TH, GUMBO_TAG_TR]);
static IN_TABLE_END_IGNORED: TagSet = tags(&[
    GUMBO_TAG_BODY,
    GUMBO_TAG_CAPTION,
    GUMBO_TAG_COL,
    GUMBO_TAG_COLGROUP,
    GUMBO_TAG_HTML,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
]);
static STYLE_SCRIPT_TEMPLATE: TagSet = tags(&[GUMBO_TAG_STYLE, GUMBO_TAG_SCRIPT, GUMBO_TAG_TEMPLATE]);
static CAPTION_CLOSERS: TagSet = tags(&[
    GUMBO_TAG_CAPTION,
    GUMBO_TAG_COL,
    GUMBO_TAG_COLGROUP,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
]);
static CAPTION_END_IGNORED: TagSet = tags(&[
    GUMBO_TAG_BODY,
    GUMBO_TAG_COL,
    GUMBO_TAG_COLGROUP,
    GUMBO_TAG_HTML,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TD,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_TH,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
]);
static TD_TH: TagSet = tags(&[GUMBO_TAG_TD, GUMBO_TAG_TH]);
static TABLE_SECTIONS: TagSet = tags(&[GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD]);
static TABLE_BODY_CLOSERS: TagSet =
    tags(&[GUMBO_TAG_CAPTION, GUMBO_TAG_COL, GUMBO_TAG_COLGROUP, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD]);
static TABLE_BODY_END_IGNORED: TagSet =
    tags(&[GUMBO_TAG_BODY, GUMBO_TAG_CAPTION, GUMBO_TAG_COL, GUMBO_TAG_TR, GUMBO_TAG_COLGROUP, GUMBO_TAG_HTML, GUMBO_TAG_TD, GUMBO_TAG_TH]);
static ROW_CLOSERS: TagSet =
    tags(&[GUMBO_TAG_CAPTION, GUMBO_TAG_COL, GUMBO_TAG_COLGROUP, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD, GUMBO_TAG_TR]);
static ROW_END_IGNORED: TagSet =
    tags(&[GUMBO_TAG_BODY, GUMBO_TAG_CAPTION, GUMBO_TAG_COL, GUMBO_TAG_COLGROUP, GUMBO_TAG_HTML, GUMBO_TAG_TD, GUMBO_TAG_TH]);
static CELL_END_IGNORED: TagSet = tags(&[GUMBO_TAG_BODY, GUMBO_TAG_CAPTION, GUMBO_TAG_COL, GUMBO_TAG_COLGROUP, GUMBO_TAG_HTML]);
static CELL_END_CLOSERS: TagSet = tags(&[GUMBO_TAG_TABLE, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD, GUMBO_TAG_TR]);
static INPUT_KEYGEN_TEXTAREA: TagSet = tags(&[GUMBO_TAG_INPUT, GUMBO_TAG_KEYGEN, GUMBO_TAG_TEXTAREA]);
static SCRIPT_TEMPLATE: TagSet = tags(&[GUMBO_TAG_SCRIPT, GUMBO_TAG_TEMPLATE]);
static SELECT_IN_TABLE_CLOSERS: TagSet = tags(&[
    GUMBO_TAG_CAPTION,
    GUMBO_TAG_TABLE,
    GUMBO_TAG_TBODY,
    GUMBO_TAG_TFOOT,
    GUMBO_TAG_THEAD,
    GUMBO_TAG_TR,
    GUMBO_TAG_TD,
    GUMBO_TAG_TH,
]);
static TEMPLATE_TABLE_PARTS: TagSet = tags(&[GUMBO_TAG_CAPTION, GUMBO_TAG_COLGROUP, GUMBO_TAG_TBODY, GUMBO_TAG_TFOOT, GUMBO_TAG_THEAD]);
static FOREIGN_BREAKOUT: TagSet = tags(&[
    GUMBO_TAG_B,
    GUMBO_TAG_BIG,
    GUMBO_TAG_BLOCKQUOTE,
    GUMBO_TAG_BODY,
    GUMBO_TAG_BR,
    GUMBO_TAG_CENTER,
    GUMBO_TAG_CODE,
    GUMBO_TAG_DD,
    GUMBO_TAG_DIV,
    GUMBO_TAG_DL,
    GUMBO_TAG_DT,
    GUMBO_TAG_EM,
    GUMBO_TAG_EMBED,
    GUMBO_TAG_H1,
    GUMBO_TAG_H2,
    GUMBO_TAG_H3,
    GUMBO_TAG_H4,
    GUMBO_TAG_H5,
    GUMBO_TAG_H6,
    GUMBO_TAG_HEAD,
    GUMBO_TAG_HR,
    GUMBO_TAG_I,
    GUMBO_TAG_IMG,
    GUMBO_TAG_LI,
    GUMBO_TAG_LISTING,
    GUMBO_TAG_MENU,
    GUMBO_TAG_META,
    GUMBO_TAG_NOBR,
    GUMBO_TAG_OL,
    GUMBO_TAG_P,
    GUMBO_TAG_PRE,
    GUMBO_TAG_RUBY,
    GUMBO_TAG_S,
    GUMBO_TAG_SMALL,
    GUMBO_TAG_SPAN,
    GUMBO_TAG_STRONG,
    GUMBO_TAG_STRIKE,
    GUMBO_TAG_SUB,
    GUMBO_TAG_SUP,
    GUMBO_TAG_TABLE,
    GUMBO_TAG_TT,
    GUMBO_TAG_U,
    GUMBO_TAG_UL,
    GUMBO_TAG_VAR,
]);
static BR_P: TagSet = tags(&[GUMBO_TAG_BR, GUMBO_TAG_P]);
static MGLYPH_MALIGNMARK: TagSet = tags(&[GUMBO_TAG_MGLYPH, GUMBO_TAG_MALIGNMARK]);

fn tagset_includes(set: &TagSet, ns: GumboNamespaceEnum, tag: GumboTag) -> bool {
    tag < GUMBO_TAG_LAST && set[tag as usize] & (1u8 << ns) != 0
}

/// A doctype field the C reads as a C string; NULL only on a second doctype of one `<!DOCTYPE`, which no
/// insertion mode that reads the fields meets.
fn doc_str(s: &Option<Vec<u8>>) -> &[u8] {
    s.as_deref().unwrap_or(b"")
}

fn is_in_static_list(needle: &[u8], haystack: &[&[u8]], exact_match: bool) -> bool {
    haystack.iter().any(|h| if exact_match { strcmp(needle, h) } else { strcasecmp(needle, h) })
}

fn attribute_matches(attributes: &[GumboAttribute], name: &[u8], value: &[u8]) -> bool {
    gumbo_get_attribute(attributes, name).is_some_and(|attr| strcasecmp(value, &attr.value))
}

fn attribute_matches_case_sensitive(attributes: &[GumboAttribute], name: &[u8], value: &[u8]) -> bool {
    gumbo_get_attribute(attributes, name).is_some_and(|attr| strcmp(value, &attr.value))
}

fn all_attributes_match(attr1: &[GumboAttribute], attr2: &[GumboAttribute]) -> bool {
    let mut num_unmatched_attr2_elements = attr2.len() as u32;
    for attr in attr1 {
        if attribute_matches_case_sensitive(attr2, &attr.name, &attr.value) {
            num_unmatched_attr2_elements = num_unmatched_attr2_elements.wrapping_sub(1);
        } else {
            return false;
        }
    }
    num_unmatched_attr2_elements == 0
}

fn compute_quirks_mode(doctype: &GumboTokenDocType) -> GumboQuirksModeEnum {
    let public = doc_str(&doctype.public_identifier);
    let system = doc_str(&doctype.system_identifier);
    if doctype.force_quirks
        || !strcmp(doc_str(&doctype.name), kDoctypeHtml)
        || is_in_static_list(public, &kQuirksModePublicIdPrefixes, false)
        || is_in_static_list(public, &kQuirksModePublicIdExactMatches, true)
        || is_in_static_list(system, &kQuirksModeSystemIdExactMatches, true)
        || (is_in_static_list(public, &kLimitedQuirksRequiresSystemIdPublicIdPrefixes, false) && !doctype.has_system_identifier)
    {
        GUMBO_DOCTYPE_QUIRKS
    } else if is_in_static_list(public, &kLimitedQuirksPublicIdPrefixes, false)
        || (is_in_static_list(public, &kLimitedQuirksRequiresSystemIdPublicIdPrefixes, false) && doctype.has_system_identifier)
    {
        GUMBO_DOCTYPE_LIMITED_QUIRKS
    } else {
        GUMBO_DOCTYPE_NO_QUIRKS
    }
}

fn doctype_matches(doctype: &GumboTokenDocType, public_id: &[u8], system_id: &[u8], allow_missing_system_id: bool) -> bool {
    strcmp(doc_str(&doctype.public_identifier), public_id)
        && (allow_missing_system_id || doctype.has_system_identifier)
        && strcmp(doc_str(&doctype.system_identifier), system_id)
}

/// `gumbo_vector_index_of` as an `int`.
fn index_of(list: &[NodeId], node: NodeId) -> i32 {
    list.iter().position(|n| *n == node).map_or(-1, |i| i as i32)
}

#[derive(Clone, Debug, Default)]
struct TextNodeBufferState {
    buffer: Vec<u8>,
    start_original_text: usize,
    start_position: GumboSourcePosition,
    ty: GumboNodeType,
}

#[derive(Clone, Debug, Default)]
struct GumboParserState {
    insertion_mode: GumboInsertionMode,
    original_insertion_mode: GumboInsertionMode,
    open_elements: Vec<NodeId>,
    active_formatting_elements: Vec<NodeId>,
    template_insertion_modes: Vec<GumboInsertionMode>,
    head_element: Option<NodeId>,
    form_element: Option<NodeId>,
    fragment_ctx: Option<NodeId>,
    reprocess_current_token: bool,
    self_closing_flag_acknowledged: bool,
    frameset_ok: bool,
    ignore_next_linefeed: bool,
    foster_parent_insertions: bool,
    text_node: TextNodeBufferState,
    closed_body_tag: bool,
    closed_html_tag: bool,
    cloned_bytes: usize,
}

/// `InsertionLocation`.
#[derive(Clone, Copy, Debug)]
struct InsertionLocation {
    target: NodeId,
    index: i32,
}

/// `GumboParser`: the options, the output being built, and the tokenizer's and the tree builder's state.
pub struct Parser<'a> {
    pub(crate) text: &'a [u8],
    pub(crate) options: GumboOptions,
    pub(crate) errors: Errors,
    pub(crate) tokenizer: GumboTokenizerState,
    pub(crate) reached: Reached,
    nodes: Vec<GumboNode>,
    root: Option<NodeId>,
    truncated: bool,
    state: GumboParserState,
    /// The token `_current_token` points at once the first is handled.
    pub(crate) token: GumboToken,
    has_current_token: bool,
}

impl std::fmt::Debug for Parser<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parser").field("nodes", &self.nodes.len()).field("insertion_mode", &self.state.insertion_mode).finish()
    }
}

const DOCUMENT: NodeId = 0;

impl Parser<'_> {
    // ---- Nodes ----------------------------------------------------------------------------------------------

    fn node(&self, id: NodeId) -> &GumboNode {
        &self.nodes[id]
    }

    fn elem(&self, id: NodeId) -> &GumboElement {
        self.nodes[id].element()
    }

    fn elem_mut(&mut self, id: NodeId) -> &mut GumboElement {
        self.nodes[id].element_mut()
    }

    fn children_mut(&mut self, id: NodeId) -> &mut Vec<NodeId> {
        self.nodes[id].children_mut().expect("a node with children")
    }

    fn create_node(&mut self, ty: GumboNodeType, v: GumboNodeData) -> NodeId {
        self.nodes.push(GumboNode { ty, parent: None, index_within_parent: NO_INDEX, parse_flags: GUMBO_INSERTION_NORMAL, v });
        self.nodes.len() - 1
    }

    fn count(&mut self, mode: usize) {
        let ty = self.token.ty as usize;
        if let Some(n) = self.reached.modes.get_mut(mode).and_then(|row| row.get_mut(ty)) {
            *n = n.saturating_add(1);
        }
    }

    // ---- Tokens ---------------------------------------------------------------------------------------------

    fn token_has_attribute(&self, name: &[u8]) -> bool {
        gumbo_get_attribute(&self.token.start_tag().attributes, name).is_some()
    }

    fn tag_in(&self, is_start: bool, tags: &TagSet) -> bool {
        let token_tag = if is_start && self.token.ty == GUMBO_TOKEN_START_TAG {
            self.token.start_tag().tag
        } else if !is_start && self.token.ty == GUMBO_TOKEN_END_TAG {
            self.token.end_tag()
        } else {
            return false;
        };
        token_tag < GUMBO_TAG_LAST && tags[token_tag as usize] != 0
    }

    fn tag_is(&self, is_start: bool, tag: GumboTag) -> bool {
        if is_start && self.token.ty == GUMBO_TOKEN_START_TAG {
            self.token.start_tag().tag == tag
        } else if !is_start && self.token.ty == GUMBO_TOKEN_END_TAG {
            self.token.end_tag() == tag
        } else {
            false
        }
    }

    fn set_frameset_not_ok(&mut self) {
        self.state.frameset_ok = false;
    }

    // ---- Node predicates ------------------------------------------------------------------------------------

    fn node_tag_in_set(&self, node: NodeId, tags: &TagSet) -> bool {
        let n = self.node(node);
        if !n.is_element() {
            return false;
        }
        let e = n.element();
        tagset_includes(tags, e.tag_namespace, e.tag)
    }

    fn node_qualified_tag_is(&self, node: NodeId, ns: GumboNamespaceEnum, tag: GumboTag) -> bool {
        let n = self.node(node);
        n.is_element() && n.element().tag == tag && n.element().tag_namespace == ns
    }

    fn node_html_tag_is(&self, node: NodeId, tag: GumboTag) -> bool {
        self.node_qualified_tag_is(node, GUMBO_NAMESPACE_HTML, tag)
    }

    fn current_node_html_tag_is(&self, tag: GumboTag) -> bool {
        self.get_current_node().is_some_and(|n| self.node_html_tag_is(n, tag))
    }

    fn current_node_tag_in_set(&self, tags: &TagSet) -> bool {
        self.get_current_node().is_some_and(|n| self.node_tag_in_set(n, tags))
    }

    fn is_mathml_integration_point(&self, node: NodeId) -> bool {
        self.node_tag_in_set(node, &MATHML_INTEGRATION)
    }

    fn is_html_integration_point(&self, node: NodeId) -> bool {
        self.node_tag_in_set(node, &HTML_INTEGRATION)
            || (self.node_qualified_tag_is(node, GUMBO_NAMESPACE_MATHML, GUMBO_TAG_ANNOTATION_XML)
                && (attribute_matches(&self.elem(node).attributes, b"encoding", b"text/html")
                    || attribute_matches(&self.elem(node).attributes, b"encoding", b"application/xhtml+xml")))
    }

    fn is_special_node(&self, node: NodeId) -> bool {
        self.node_tag_in_set(node, &SPECIAL)
    }

    // ---- State ----------------------------------------------------------------------------------------------

    fn is_fragment_parser(&self) -> bool {
        self.state.fragment_ctx.is_some()
    }

    fn get_current_node(&self) -> Option<NodeId> {
        self.state.open_elements.last().copied()
    }

    /// The current node where the C dereferences it unchecked: every caller runs with the html element open.
    fn current(&self) -> NodeId {
        self.get_current_node().expect("an open element")
    }

    fn get_adjusted_current_node(&self) -> Option<NodeId> {
        if self.state.open_elements.len() == 1 && self.state.fragment_ctx.is_some() {
            return self.state.fragment_ctx;
        }
        self.get_current_node()
    }

    fn set_insertion_mode(&mut self, mode: GumboInsertionMode) {
        self.state.insertion_mode = mode;
    }

    fn push_template_insertion_mode(&mut self, mode: GumboInsertionMode) {
        self.state.template_insertion_modes.push(mode);
    }

    fn pop_template_insertion_mode(&mut self) {
        self.state.template_insertion_modes.pop();
    }

    fn get_current_template_insertion_mode(&self) -> GumboInsertionMode {
        self.state.template_insertion_modes.last().copied().unwrap_or(GUMBO_INSERTION_MODE_INITIAL)
    }

    fn get_appropriate_insertion_mode(&self, index: usize) -> GumboInsertionMode {
        let open_elements = &self.state.open_elements;
        let mut node = open_elements[index];
        let is_last = index == 0;
        if is_last && let Some(ctx) = self.state.fragment_ctx {
            node = ctx;
        }
        let e = self.elem(node);
        if e.tag_namespace != GUMBO_NAMESPACE_HTML {
            return if is_last { GUMBO_INSERTION_MODE_IN_BODY } else { GUMBO_INSERTION_MODE_INITIAL };
        }
        match e.tag {
            GUMBO_TAG_SELECT => {
                if is_last {
                    return GUMBO_INSERTION_MODE_IN_SELECT;
                }
                let mut i = index;
                while i > 0 {
                    let ancestor = open_elements[i];
                    if self.node_html_tag_is(ancestor, GUMBO_TAG_TEMPLATE) {
                        return GUMBO_INSERTION_MODE_IN_SELECT;
                    }
                    if self.node_html_tag_is(ancestor, GUMBO_TAG_TABLE) {
                        return GUMBO_INSERTION_MODE_IN_SELECT_IN_TABLE;
                    }
                    i -= 1;
                }
                return GUMBO_INSERTION_MODE_IN_SELECT;
            }
            GUMBO_TAG_TD | GUMBO_TAG_TH => {
                if !is_last {
                    return GUMBO_INSERTION_MODE_IN_CELL;
                }
            }
            GUMBO_TAG_TR => return GUMBO_INSERTION_MODE_IN_ROW,
            GUMBO_TAG_TBODY | GUMBO_TAG_THEAD | GUMBO_TAG_TFOOT => return GUMBO_INSERTION_MODE_IN_TABLE_BODY,
            GUMBO_TAG_CAPTION => return GUMBO_INSERTION_MODE_IN_CAPTION,
            GUMBO_TAG_COLGROUP => return GUMBO_INSERTION_MODE_IN_COLUMN_GROUP,
            GUMBO_TAG_TABLE => return GUMBO_INSERTION_MODE_IN_TABLE,
            GUMBO_TAG_TEMPLATE => return self.get_current_template_insertion_mode(),
            GUMBO_TAG_HEAD => {
                if !is_last {
                    return GUMBO_INSERTION_MODE_IN_HEAD;
                }
            }
            GUMBO_TAG_BODY => return GUMBO_INSERTION_MODE_IN_BODY,
            GUMBO_TAG_FRAMESET => return GUMBO_INSERTION_MODE_IN_FRAMESET,
            GUMBO_TAG_HTML => {
                return if self.state.head_element.is_some() { GUMBO_INSERTION_MODE_AFTER_HEAD } else { GUMBO_INSERTION_MODE_BEFORE_HEAD };
            }
            _ => {}
        }
        if is_last { GUMBO_INSERTION_MODE_IN_BODY } else { GUMBO_INSERTION_MODE_INITIAL }
    }

    /// `reset_insertion_mode_appropriately`; with no mode found the C's release build changes nothing.
    fn reset_insertion_mode_appropriately(&mut self) {
        for i in (0..self.state.open_elements.len()).rev() {
            let mode = self.get_appropriate_insertion_mode(i);
            if mode != GUMBO_INSERTION_MODE_INITIAL {
                self.set_insertion_mode(mode);
                return;
            }
        }
    }

    /// `parser_add_parse_error`; false where `gumbo_add_error` gave NULL.
    fn parser_add_parse_error(&mut self) -> bool {
        let token = &self.token;
        let input_tag = match token.ty {
            GUMBO_TOKEN_START_TAG => token.start_tag().tag,
            GUMBO_TOKEN_END_TAG => token.end_tag(),
            _ => GUMBO_TAG_UNKNOWN,
        };
        if !self.errors.room() {
            return false;
        }
        let tag_stack = self.state.open_elements.iter().map(|n| self.elem(*n).tag).collect();
        self.errors.add(GumboError {
            ty: GUMBO_ERR_PARSER,
            position: token.position,
            original_text: token.original_text.data,
            v: GumboErrorValue::Parser { input_type: token.ty, input_tag, parser_state: self.state.insertion_mode, tag_stack },
        })
    }

    // ---- Insertion ------------------------------------------------------------------------------------------

    fn get_appropriate_insertion_location(&self, override_target: Option<NodeId>) -> InsertionLocation {
        // With the html element on the stack until finish_parsing, root set means a current node; the C would
        // dereference NULL otherwise, and this takes the document.
        let target =
            override_target.unwrap_or_else(|| if self.root.is_some() { self.get_current_node().unwrap_or(DOCUMENT) } else { DOCUMENT });
        let retval = InsertionLocation { target, index: -1 };
        if !self.state.foster_parent_insertions || !self.node_tag_in_set(target, &TABLE_CONTEXT_FOSTER) {
            return retval;
        }
        let open_elements = &self.state.open_elements;
        let mut last_template_index: i32 = -1;
        let mut last_table_index: i32 = -1;
        for (i, node) in open_elements.iter().enumerate() {
            if self.node_html_tag_is(*node, GUMBO_TAG_TEMPLATE) {
                last_template_index = i as i32;
            }
            if self.node_html_tag_is(*node, GUMBO_TAG_TABLE) {
                last_table_index = i as i32;
            }
        }
        if last_template_index != -1 && (last_table_index == -1 || last_template_index > last_table_index) {
            return InsertionLocation { target: open_elements[last_template_index as usize], index: -1 };
        }
        if last_table_index == -1 {
            return InsertionLocation { target: open_elements[0], index: -1 };
        }
        let last_table = open_elements[last_table_index as usize];
        if let Some(parent) = self.node(last_table).parent {
            return InsertionLocation { target: parent, index: self.node(last_table).index_within_parent as i32 };
        }
        InsertionLocation { target: open_elements[last_table_index as usize - 1], index: -1 }
    }

    fn append_node(&mut self, parent: NodeId, node: NodeId) {
        let children = self.children_mut(parent);
        let index = children.len();
        children.push(node);
        let n = &mut self.nodes[node];
        n.parent = Some(parent);
        n.index_within_parent = index;
    }

    fn insert_node(&mut self, node: NodeId, location: InsertionLocation) {
        let parent = location.target;
        let index = location.index;
        if index != -1 {
            let index = index as usize;
            self.nodes[node].parent = Some(parent);
            self.nodes[node].index_within_parent = index;
            let children = self.children_mut(parent);
            children.insert(index, node);
            let after: Vec<NodeId> = children[index + 1..].to_vec();
            for (k, sibling) in after.into_iter().enumerate() {
                self.nodes[sibling].index_within_parent = index + 1 + k;
            }
        } else {
            self.append_node(parent, node);
        }
    }

    fn maybe_flush_text_node_buffer(&mut self) {
        if self.state.text_node.buffer.is_empty() {
            return;
        }
        let buffer_state = &mut self.state.text_node;
        let start = buffer_state.start_original_text;
        let end = self.token.original_text.data.unwrap_or(0);
        let text = GumboText {
            text: std::mem::take(&mut buffer_state.buffer),
            original_text: GumboStringPiece { data: Some(start), length: end.wrapping_sub(start) },
            start_pos: buffer_state.start_position,
        };
        let ty = buffer_state.ty;
        let location = self.get_appropriate_insertion_location(None);
        if self.node(location.target).ty != GUMBO_NODE_DOCUMENT {
            let text_node = self.create_node(ty, GumboNodeData::Text(text));
            self.insert_node(text_node, location);
        }
        self.state.text_node.buffer.clear();
        self.state.text_node.ty = GUMBO_NODE_WHITESPACE;
    }

    fn record_end_of_element(&mut self, element: NodeId) {
        let position = self.token.position;
        let original_end_tag = if self.token.ty == GUMBO_TOKEN_END_TAG { self.token.original_text } else { kGumboEmptyString };
        let e = self.elem_mut(element);
        e.end_pos = position;
        e.original_end_tag = original_end_tag;
    }

    fn pop_current_node(&mut self) -> Option<NodeId> {
        self.maybe_flush_text_node_buffer();
        let current_node = self.state.open_elements.pop()?;
        let is_closed_body_or_html_tag = (self.node_html_tag_is(current_node, GUMBO_TAG_BODY) && self.state.closed_body_tag)
            || (self.node_html_tag_is(current_node, GUMBO_TAG_HTML) && self.state.closed_html_tag);
        if (self.token.ty != GUMBO_TOKEN_END_TAG || !self.node_html_tag_is(current_node, self.token.end_tag()))
            && !is_closed_body_or_html_tag
        {
            self.nodes[current_node].parse_flags |= GUMBO_INSERTION_IMPLICIT_END_TAG;
        }
        if !is_closed_body_or_html_tag {
            self.record_end_of_element(current_node);
        }
        Some(current_node)
    }

    /// `pop_current_node` where the C dereferences what it returns.
    fn pop(&mut self) -> NodeId {
        self.pop_current_node().expect("an open element")
    }

    fn append_comment_node(&mut self, node: NodeId) {
        self.maybe_flush_text_node_buffer();
        let text = match &mut self.token.v {
            GumboTokenData::Text(t) => std::mem::take(t),
            _ => Vec::new(),
        };
        let comment = self.create_node(
            GUMBO_NODE_COMMENT,
            GumboNodeData::Text(GumboText { text, original_text: self.token.original_text, start_pos: self.token.position }),
        );
        self.append_node(node, comment);
    }

    fn clear_stack_to_context(&mut self, tags: &TagSet) {
        while !self.current_node_tag_in_set(tags) {
            self.pop_current_node();
        }
    }

    fn create_element(&mut self, tag: GumboTag) -> NodeId {
        let start_pos = if self.has_current_token { self.token.position } else { kGumboEmptySourcePosition };
        let element = GumboElement {
            children: Vec::new(),
            tag,
            tag_namespace: GUMBO_NAMESPACE_HTML,
            original_tag: kGumboEmptyString,
            original_end_tag: kGumboEmptyString,
            start_pos,
            end_pos: kGumboEmptySourcePosition,
            attributes: Vec::new(),
        };
        self.create_node(GUMBO_NODE_ELEMENT, GumboNodeData::Element(element))
    }

    fn create_element_from_token(&mut self, tag_namespace: GumboNamespaceEnum) -> NodeId {
        let start_tag = self.token.start_tag_mut();
        let tag = start_tag.tag;
        let attributes = std::mem::take(&mut start_tag.attributes);
        let ty = if tag_namespace == GUMBO_NAMESPACE_HTML && tag == GUMBO_TAG_TEMPLATE { GUMBO_NODE_TEMPLATE } else { GUMBO_NODE_ELEMENT };
        let element = GumboElement {
            children: Vec::new(),
            tag,
            tag_namespace,
            original_tag: self.token.original_text,
            original_end_tag: kGumboEmptyString,
            start_pos: self.token.position,
            end_pos: kGumboEmptySourcePosition,
            attributes,
        };
        self.create_node(ty, GumboNodeData::Element(element))
    }

    fn insert_element(&mut self, node: NodeId, is_reconstructing_formatting_elements: bool) {
        if !is_reconstructing_formatting_elements {
            self.maybe_flush_text_node_buffer();
        }
        let location = self.get_appropriate_insertion_location(None);
        self.insert_node(node, location);
        self.state.open_elements.push(node);
    }

    fn insert_element_from_token(&mut self) -> NodeId {
        let element = self.create_element_from_token(GUMBO_NAMESPACE_HTML);
        self.insert_element(element, false);
        element
    }

    fn insert_element_of_tag_type(&mut self, tag: GumboTag, reason: GumboParseFlags) -> NodeId {
        let element = self.create_element(tag);
        self.nodes[element].parse_flags |= GUMBO_INSERTION_BY_PARSER | reason;
        self.insert_element(element, false);
        element
    }

    /// `insert_foreign_element`. Its checks of `xmlns` and `xmlns:xlink` read the token's attributes after the
    /// element took them, so they never find one.
    fn insert_foreign_element(&mut self, tag_namespace: GumboNamespaceEnum) -> NodeId {
        let element = self.create_element_from_token(tag_namespace);
        self.insert_element(element, false);
        let legal: &[u8] = match tag_namespace {
            GUMBO_NAMESPACE_HTML => b"http://www.w3.org/1999/xhtml",
            GUMBO_NAMESPACE_SVG => b"http://www.w3.org/2000/svg",
            _ => b"http://www.w3.org/1998/Math/MathML",
        };
        if self.token_has_attribute(b"xmlns") && !attribute_matches_case_sensitive(&self.token.start_tag().attributes, b"xmlns", legal) {
            self.parser_add_parse_error();
        }
        if self.token_has_attribute(b"xmlns:xlink")
            && !attribute_matches_case_sensitive(&self.token.start_tag().attributes, b"xmlns:xlink", b"http://www.w3.org/1999/xlink")
        {
            self.parser_add_parse_error();
        }
        element
    }

    fn insert_text_token(&mut self) {
        let buffer_state = &mut self.state.text_node;
        if buffer_state.buffer.is_empty() {
            buffer_state.start_original_text = self.token.original_text.data.unwrap_or(0);
            buffer_state.start_position = self.token.position;
        }
        append_codepoint(self.token.character(), &mut buffer_state.buffer);
        if self.token.ty == GUMBO_TOKEN_CHARACTER {
            buffer_state.ty = GUMBO_NODE_TEXT;
        } else if self.token.ty == GUMBO_TOKEN_CDATA {
            buffer_state.ty = GUMBO_NODE_CDATA;
        }
    }

    fn run_generic_parsing_algorithm(&mut self, lexer_state: GumboTokenizerEnum) {
        self.insert_element_from_token();
        self.gumbo_tokenizer_set_state(lexer_state);
        self.state.original_insertion_mode = self.state.insertion_mode;
        self.state.insertion_mode = GUMBO_INSERTION_MODE_TEXT;
    }

    fn acknowledge_self_closing_tag(&mut self) {
        self.state.self_closing_flag_acknowledged = true;
    }

    // ---- Active formatting elements -------------------------------------------------------------------------

    fn find_last_anchor_index(&self) -> Option<usize> {
        for (i, node) in self.state.active_formatting_elements.iter().enumerate().rev() {
            if *node == MARKER {
                return None;
            }
            if self.node_html_tag_is(*node, GUMBO_TAG_A) {
                return Some(i);
            }
        }
        None
    }

    fn count_formatting_elements_of_tag(&self, desired_node: NodeId, earliest_matching_index: &mut usize) -> i32 {
        let (tag, ns, attributes): (GumboTag, GumboNamespaceEnum, &[GumboAttribute]) = if desired_node == MARKER {
            (GUMBO_TAG_HTML, GUMBO_NAMESPACE_HTML, &[])
        } else {
            let e = self.elem(desired_node);
            (e.tag, e.tag_namespace, &e.attributes)
        };
        let mut num_identical_elements = 0;
        for (i, node) in self.state.active_formatting_elements.iter().enumerate().rev() {
            if *node == MARKER {
                break;
            }
            if self.node_qualified_tag_is(*node, ns, tag) && all_attributes_match(&self.elem(*node).attributes, attributes) {
                num_identical_elements += 1;
                *earliest_matching_index = i;
            }
        }
        num_identical_elements
    }

    fn add_formatting_element(&mut self, node: NodeId) {
        let mut earliest_identical_element = self.state.active_formatting_elements.len();
        let num_identical_elements = self.count_formatting_elements_of_tag(node, &mut earliest_identical_element);
        if num_identical_elements >= 3 {
            self.state.active_formatting_elements.remove(earliest_identical_element);
        }
        self.state.active_formatting_elements.push(node);
    }

    fn is_open_element(&self, node: NodeId) -> bool {
        self.state.open_elements.contains(&node)
    }

    fn clone_node(&mut self, node: NodeId, reason: GumboParseFlags) -> NodeId {
        let tag_length = self.elem(node).original_tag.length;
        self.state.cloned_bytes = self.state.cloned_bytes.wrapping_add(SIZEOF_GUMBO_NODE).wrapping_add(tag_length);
        let mut new_node = self.node(node).clone();
        new_node.parent = None;
        new_node.index_within_parent = NO_INDEX;
        new_node.parse_flags &= !GUMBO_INSERTION_IMPLICIT_END_TAG;
        new_node.parse_flags |= reason | GUMBO_INSERTION_BY_PARSER;
        let element = new_node.element_mut();
        element.children = Vec::new();
        // Each attribute is copied whole, its name and value with gumbo_copy_stringz: up to a NUL.
        for attr in &mut element.attributes {
            attr.name = c_str(&attr.name).to_vec();
            attr.value = c_str(&attr.value).to_vec();
        }
        self.nodes.push(new_node);
        self.nodes.len() - 1
    }

    fn reconstruct_active_formatting_elements(&mut self) {
        let len = self.state.active_formatting_elements.len();
        if len == 0 {
            return;
        }
        let mut i: u32 = len as u32 - 1;
        let mut element = self.state.active_formatting_elements[i as usize];
        if element == MARKER || self.is_open_element(element) {
            return;
        }
        loop {
            if i == 0 {
                i = u32::MAX;
                break;
            }
            i -= 1;
            element = self.state.active_formatting_elements[i as usize];
            if element == MARKER || self.is_open_element(element) {
                break;
            }
        }
        i = i.wrapping_add(1);
        while (i as usize) < self.state.active_formatting_elements.len() {
            let element = self.state.active_formatting_elements[i as usize];
            let clone = self.clone_node(element, GUMBO_INSERTION_RECONSTRUCTED_FORMATTING_ELEMENT);
            let location = self.get_appropriate_insertion_location(None);
            self.insert_node(clone, location);
            self.state.open_elements.push(clone);
            self.state.active_formatting_elements[i as usize] = clone;
            i += 1;
        }
    }

    fn clear_active_formatting_elements(&mut self) {
        loop {
            let node = self.state.active_formatting_elements.pop();
            if node.is_none() || node == Some(MARKER) {
                break;
            }
        }
    }

    // ---- Scopes ---------------------------------------------------------------------------------------------

    fn has_an_element_in_specific_scope(&self, expected: &[GumboTag], negate: bool, tags: &TagSet) -> bool {
        for node in self.state.open_elements.iter().rev() {
            let n = self.node(*node);
            if !n.is_element() {
                continue;
            }
            let e = n.element();
            for tag in expected {
                if e.tag == *tag && e.tag_namespace == GUMBO_NAMESPACE_HTML {
                    return true;
                }
            }
            let found = tagset_includes(tags, e.tag_namespace, e.tag);
            if negate != found {
                return false;
            }
        }
        false
    }

    fn has_open_element(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], false, &HTML_ONLY)
    }

    fn has_an_element_in_scope(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], false, &SCOPE)
    }

    fn has_node_in_scope(&self, node: NodeId) -> bool {
        for current in self.state.open_elements.iter().rev() {
            if *current == node {
                return true;
            }
            if !self.node(*current).is_element() {
                continue;
            }
            if self.node_tag_in_set(*current, &SCOPE) {
                return false;
            }
        }
        false
    }

    fn has_an_element_in_list_scope(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], false, &LIST_SCOPE)
    }

    fn has_an_element_in_button_scope(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], false, &BUTTON_SCOPE)
    }

    fn has_an_element_in_table_scope(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], false, &TABLE_SCOPE)
    }

    fn has_an_element_in_select_scope(&self, tag: GumboTag) -> bool {
        self.has_an_element_in_specific_scope(&[tag], true, &SELECT_SCOPE)
    }

    fn generate_implied_end_tags(&mut self, exception: GumboTag) {
        while self.current_node_tag_in_set(&IMPLIED_END_TAGS) && !self.current_node_html_tag_is(exception) {
            self.pop_current_node();
        }
    }

    fn generate_all_implied_end_tags_thoroughly(&mut self) {
        while self.current_node_tag_in_set(&IMPLIED_END_TAGS_THOROUGHLY) {
            self.pop_current_node();
        }
    }

    fn close_table(&mut self) -> bool {
        if !self.has_an_element_in_table_scope(GUMBO_TAG_TABLE) {
            return false;
        }
        let mut node = self.pop();
        while !self.node_html_tag_is(node, GUMBO_TAG_TABLE) {
            node = self.pop();
        }
        self.reset_insertion_mode_appropriately();
        true
    }

    fn close_table_cell(&mut self, cell_tag: GumboTag) -> bool {
        let mut result = true;
        self.generate_implied_end_tags(GUMBO_TAG_LAST);
        if !self.current_node_html_tag_is(cell_tag) {
            self.parser_add_parse_error();
            result = false;
        }
        loop {
            let node = self.pop();
            if self.node_html_tag_is(node, cell_tag) {
                break;
            }
        }
        self.clear_active_formatting_elements();
        self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_ROW);
        result
    }

    fn close_current_cell(&mut self) -> bool {
        if self.has_an_element_in_table_scope(GUMBO_TAG_TD) {
            self.close_table_cell(GUMBO_TAG_TD)
        } else {
            self.close_table_cell(GUMBO_TAG_TH)
        }
    }

    fn close_current_select(&mut self) {
        let mut node = self.pop();
        while !self.node_html_tag_is(node, GUMBO_TAG_SELECT) {
            node = self.pop();
        }
        self.reset_insertion_mode_appropriately();
    }

    fn implicitly_close_tags(&mut self, target_ns: GumboNamespaceEnum, target: GumboTag) -> bool {
        let mut result = true;
        self.generate_implied_end_tags(target);
        if !self.node_qualified_tag_is(self.current(), target_ns, target) {
            self.parser_add_parse_error();
            while !self.node_qualified_tag_is(self.current(), target_ns, target) {
                self.pop_current_node();
            }
            result = false;
        }
        self.pop_current_node();
        result
    }

    fn maybe_implicitly_close_p_tag(&mut self) -> bool {
        if self.has_an_element_in_button_scope(GUMBO_TAG_P) {
            return self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, GUMBO_TAG_P);
        }
        true
    }

    fn maybe_implicitly_close_list_tag(&mut self, is_li: bool) {
        self.state.frameset_ok = false;
        for i in (0..self.state.open_elements.len()).rev() {
            let node = self.state.open_elements[i];
            let is_list_tag = if is_li { self.node_html_tag_is(node, GUMBO_TAG_LI) } else { self.node_tag_in_set(node, &DD_DT) };
            if is_list_tag {
                let (ns, tag) = (self.elem(node).tag_namespace, self.elem(node).tag);
                self.implicitly_close_tags(ns, tag);
                return;
            }
            if self.is_special_node(node) && !self.node_tag_in_set(node, &ADDRESS_DIV_P) {
                return;
            }
        }
    }

    fn merge_attributes(&mut self, node: NodeId) {
        let token_attr = std::mem::take(&mut self.token.start_tag_mut().attributes);
        for attr in token_attr {
            if gumbo_get_attribute(&self.elem(node).attributes, &attr.name).is_none() {
                self.elem_mut(node).attributes.push(attr);
            }
        }
    }

    fn adjust_foreign_attributes(&mut self) {
        let attributes = &mut self.token.start_tag_mut().attributes;
        for (from, local_name, ns) in kForeignAttributeReplacements.iter() {
            if let Some(i) = gumbo_get_attribute_index(attributes, from) {
                attributes[i].attr_namespace = *ns;
                attributes[i].name = local_name.to_vec();
            }
        }
    }

    fn adjust_svg_attributes(&mut self) {
        let attributes = &mut self.token.start_tag_mut().attributes;
        for (from, to) in kSvgAttributeReplacements.iter() {
            if let Some(i) = gumbo_get_attribute_index(attributes, from) {
                attributes[i].name = to.to_vec();
            }
        }
    }

    fn adjust_mathml_attributes(&mut self) {
        let attributes = &mut self.token.start_tag_mut().attributes;
        if let Some(i) = gumbo_get_attribute_index(attributes, b"definitionurl") {
            attributes[i].name = b"definitionURL".to_vec();
        }
    }

    fn maybe_add_doctype_error(&mut self) -> bool {
        let doctype = self.token.doc_type();
        let html_doctype = strcmp(doc_str(&doctype.name), kDoctypeHtml);
        if (!html_doctype
            || doctype.has_public_identifier
            || (doctype.has_system_identifier && strcmp(doc_str(&doctype.system_identifier), kSystemIdLegacyCompat)))
            && !(html_doctype
                && (doctype_matches(doctype, kPublicIdHtml4_0, kSystemIdRecHtml4_0, true)
                    || doctype_matches(doctype, kPublicIdHtml4_01, kSystemIdHtml4, true)
                    || doctype_matches(doctype, kPublicIdXhtml1_0, kSystemIdXhtmlStrict1_1, false)
                    || doctype_matches(doctype, kPublicIdXhtml1_1, kSystemIdXhtml1_1, false)))
        {
            self.parser_add_parse_error();
            return false;
        }
        true
    }

    fn remove_from_parent(&mut self, node: NodeId) {
        let Some(parent) = self.node(node).parent else { return };
        let children = self.children_mut(parent);
        let Some(index) = children.iter().position(|c| *c == node) else { return };
        children.remove(index);
        let after: Vec<NodeId> = children[index..].to_vec();
        self.nodes[node].parent = None;
        self.nodes[node].index_within_parent = NO_INDEX;
        for (k, child) in after.into_iter().enumerate() {
            self.nodes[child].index_within_parent = index + k;
        }
    }

    fn adoption_agency_algorithm(&mut self, subject: GumboTag) -> bool {
        let current_node = self.current();
        if self.elem(current_node).tag_namespace == GUMBO_NAMESPACE_HTML
            && self.elem(current_node).tag == subject
            && index_of(&self.state.active_formatting_elements, current_node) == -1
        {
            self.pop_current_node();
            return false;
        }
        for _ in 0..8 {
            let mut formatting_node: Option<NodeId> = None;
            let mut formatting_node_in_open_elements: i32 = -1;
            for j in (0..self.state.active_formatting_elements.len()).rev() {
                let node = self.state.active_formatting_elements[j];
                if node == MARKER {
                    return false;
                }
                if self.node_html_tag_is(node, subject) {
                    formatting_node = Some(node);
                    formatting_node_in_open_elements = index_of(&self.state.open_elements, node);
                    break;
                }
            }
            let Some(formatting_node) = formatting_node else { return false };
            if formatting_node_in_open_elements == -1 {
                self.parser_add_parse_error();
                if let Some(i) = self.state.active_formatting_elements.iter().position(|n| *n == formatting_node) {
                    self.state.active_formatting_elements.remove(i);
                }
                return false;
            }
            if !self.has_an_element_in_scope(self.elem(formatting_node).tag) {
                self.parser_add_parse_error();
                return false;
            }
            if Some(formatting_node) != self.get_current_node() {
                self.parser_add_parse_error();
            }
            let mut furthest_block: Option<NodeId> = None;
            for j in formatting_node_in_open_elements as usize..self.state.open_elements.len() {
                let current = self.state.open_elements[j];
                if self.is_special_node(current) {
                    furthest_block = Some(current);
                    break;
                }
            }
            let Some(furthest_block) = furthest_block else {
                while self.get_current_node() != Some(formatting_node) {
                    self.pop_current_node();
                }
                self.pop_current_node();
                if let Some(i) = self.state.active_formatting_elements.iter().position(|n| *n == formatting_node) {
                    self.state.active_formatting_elements.remove(i);
                }
                return false;
            };
            let common_ancestor = self.state.open_elements[(index_of(&self.state.open_elements, formatting_node) - 1) as usize];
            let mut bookmark = index_of(&self.state.active_formatting_elements, formatting_node) + 1;
            let mut node = furthest_block;
            let mut last_node = furthest_block;
            let mut saved_node_index = index_of(&self.state.open_elements, node);
            let mut j = 0;
            loop {
                j += 1;
                let mut node_index = index_of(&self.state.open_elements, node);
                if node_index == -1 {
                    node_index = saved_node_index;
                }
                node_index -= 1;
                saved_node_index = node_index;
                node = self.state.open_elements[node_index as usize];
                if node == formatting_node {
                    break;
                }
                let formatting_index = index_of(&self.state.active_formatting_elements, node);
                if j > 3 && formatting_index != -1 {
                    self.state.active_formatting_elements.remove(formatting_index as usize);
                    if formatting_index < bookmark {
                        bookmark -= 1;
                    }
                    continue;
                }
                if formatting_index == -1 {
                    self.state.open_elements.remove(node_index as usize);
                    continue;
                }
                node = self.clone_node(node, GUMBO_INSERTION_ADOPTION_AGENCY_CLONED);
                self.state.active_formatting_elements[formatting_index as usize] = node;
                self.state.open_elements[node_index as usize] = node;
                if last_node == furthest_block {
                    bookmark = formatting_index + 1;
                }
                self.nodes[last_node].parse_flags |= GUMBO_INSERTION_ADOPTION_AGENCY_MOVED;
                self.remove_from_parent(last_node);
                self.append_node(node, last_node);
                last_node = node;
            }
            self.remove_from_parent(last_node);
            self.nodes[last_node].parse_flags |= GUMBO_INSERTION_ADOPTION_AGENCY_MOVED;
            let location = self.get_appropriate_insertion_location(Some(common_ancestor));
            self.insert_node(last_node, location);
            let new_formatting_node = self.clone_node(formatting_node, GUMBO_INSERTION_ADOPTION_AGENCY_CLONED);
            self.nodes[formatting_node].parse_flags |= GUMBO_INSERTION_IMPLICIT_END_TAG;
            let moved = std::mem::take(&mut self.elem_mut(furthest_block).children);
            let empty = std::mem::replace(&mut self.elem_mut(new_formatting_node).children, moved);
            self.elem_mut(furthest_block).children = empty;
            let moved: Vec<NodeId> = self.elem(new_formatting_node).children.clone();
            for child in moved {
                self.nodes[child].parent = Some(new_formatting_node);
            }
            self.append_node(furthest_block, new_formatting_node);
            let formatting_node_index = index_of(&self.state.active_formatting_elements, formatting_node);
            if formatting_node_index < bookmark {
                bookmark -= 1;
            }
            self.state.active_formatting_elements.remove(formatting_node_index as usize);
            self.state.active_formatting_elements.insert(bookmark as usize, new_formatting_node);
            if let Some(i) = self.state.open_elements.iter().position(|n| *n == formatting_node) {
                self.state.open_elements.remove(i);
            }
            let insert_at = index_of(&self.state.open_elements, furthest_block) + 1;
            self.state.open_elements.insert(insert_at as usize, new_formatting_node);
        }
        true
    }

    /// `ignore_token`: the token is destroyed, which for a start tag frees its attributes.
    fn ignore_token(&mut self) {
        match &mut self.token.v {
            GumboTokenData::StartTag(t) => t.attributes.clear(),
            GumboTokenData::Text(t) => t.clear(),
            _ => {}
        }
    }

    fn finish_parsing(&mut self) {
        self.maybe_flush_text_node_buffer();
        while let Some(node) = self.pop_current_node() {
            if (self.node_html_tag_is(node, GUMBO_TAG_BODY) && self.state.closed_body_tag)
                || (self.node_html_tag_is(node, GUMBO_TAG_HTML) && self.state.closed_html_tag)
            {
                continue;
            }
            self.nodes[node].parse_flags |= GUMBO_INSERTION_IMPLICIT_END_TAG;
        }
        while self.pop_current_node().is_some() {}
    }

    // ---- Insertion modes ------------------------------------------------------------------------------------

    fn handle_initial(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_INITIAL as usize);
        match self.token.ty {
            GUMBO_TOKEN_WHITESPACE => {
                self.ignore_token();
                true
            }
            GUMBO_TOKEN_COMMENT => {
                self.append_comment_node(DOCUMENT);
                true
            }
            GUMBO_TOKEN_DOCTYPE => {
                let doctype = self.token.doc_type().clone();
                let quirks = compute_quirks_mode(&doctype);
                if let GumboNodeData::Document(document) = &mut self.nodes[DOCUMENT].v {
                    document.has_doctype = true;
                    document.name = doctype.name;
                    document.public_identifier = doctype.public_identifier;
                    document.system_identifier = doctype.system_identifier;
                    document.doc_type_quirks_mode = quirks;
                }
                self.set_insertion_mode(GUMBO_INSERTION_MODE_BEFORE_HTML);
                self.maybe_add_doctype_error()
            }
            _ => {
                self.parser_add_parse_error();
                if let GumboNodeData::Document(document) = &mut self.nodes[DOCUMENT].v {
                    document.doc_type_quirks_mode = GUMBO_DOCTYPE_QUIRKS;
                }
                self.set_insertion_mode(GUMBO_INSERTION_MODE_BEFORE_HTML);
                self.state.reprocess_current_token = true;
                true
            }
        }
    }

    fn handle_before_html(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_BEFORE_HTML as usize);
        if self.token.ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.token.ty == GUMBO_TOKEN_COMMENT {
            self.append_comment_node(DOCUMENT);
            true
        } else if self.token.ty == GUMBO_TOKEN_WHITESPACE {
            self.ignore_token();
            true
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            let html_node = self.insert_element_from_token();
            self.root = Some(html_node);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_BEFORE_HEAD);
            true
        } else if self.token.ty == GUMBO_TOKEN_END_TAG && !self.tag_in(false, &HEAD_BODY_HTML_BR) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            let html_node = self.insert_element_of_tag_type(GUMBO_TAG_HTML, GUMBO_INSERTION_IMPLIED);
            self.root = Some(html_node);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_BEFORE_HEAD);
            self.state.reprocess_current_token = true;
            true
        }
    }

    fn handle_before_head(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_BEFORE_HEAD as usize);
        if self.token.ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.token.ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.token.ty == GUMBO_TOKEN_WHITESPACE {
            self.ignore_token();
            true
        } else if self.tag_is(true, GUMBO_TAG_HEAD) {
            let node = self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_HEAD);
            self.state.head_element = Some(node);
            true
        } else if self.token.ty == GUMBO_TOKEN_END_TAG && !self.tag_in(false, &HEAD_BODY_HTML_BR) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            let node = self.insert_element_of_tag_type(GUMBO_TAG_HEAD, GUMBO_INSERTION_IMPLIED);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_HEAD);
            self.state.head_element = Some(node);
            self.state.reprocess_current_token = true;
            true
        }
    }

    fn handle_in_head(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_HEAD as usize);
        if self.token.ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if self.token.ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.token.ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_in(true, &BASE_BASEFONT_BGSOUND_LINK) || self.tag_is(true, GUMBO_TAG_META) {
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_is(true, GUMBO_TAG_TITLE) {
            self.run_generic_parsing_algorithm(GUMBO_LEX_RCDATA);
            true
        } else if self.tag_in(true, &NOFRAMES_STYLE) {
            self.run_generic_parsing_algorithm(GUMBO_LEX_RAWTEXT);
            true
        } else if self.tag_is(true, GUMBO_TAG_NOSCRIPT) {
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_HEAD_NOSCRIPT);
            true
        } else if self.tag_is(true, GUMBO_TAG_SCRIPT) {
            self.run_generic_parsing_algorithm(GUMBO_LEX_SCRIPT);
            true
        } else if self.tag_is(false, GUMBO_TAG_HEAD) {
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_HEAD);
            true
        } else if self.tag_in(false, &BODY_HTML_BR) {
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_HEAD);
            self.state.reprocess_current_token = true;
            true
        } else if self.tag_is(true, GUMBO_TAG_TEMPLATE) {
            self.insert_element_from_token();
            self.add_formatting_element(MARKER);
            self.state.frameset_ok = false;
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TEMPLATE);
            self.push_template_insertion_mode(GUMBO_INSERTION_MODE_IN_TEMPLATE);
            true
        } else if self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            if !self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.generate_all_implied_end_tags_thoroughly();
            let mut success = true;
            if !self.current_node_html_tag_is(GUMBO_TAG_TEMPLATE) {
                self.parser_add_parse_error();
                success = false;
            }
            loop {
                let node = self.pop();
                if self.node_html_tag_is(node, GUMBO_TAG_TEMPLATE) {
                    break;
                }
            }
            self.clear_active_formatting_elements();
            self.pop_template_insertion_mode();
            self.reset_insertion_mode_appropriately();
            success
        } else if self.tag_is(true, GUMBO_TAG_HEAD) || self.token.ty == GUMBO_TOKEN_END_TAG {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_HEAD);
            self.state.reprocess_current_token = true;
            true
        }
    }

    fn handle_in_head_noscript(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_HEAD_NOSCRIPT as usize);
        if self.token.ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            false
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(false, GUMBO_TAG_NOSCRIPT) {
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_HEAD);
            true
        } else if self.token.ty == GUMBO_TOKEN_WHITESPACE || self.token.ty == GUMBO_TOKEN_COMMENT || self.tag_in(true, &NOSCRIPT_IN_HEAD) {
            self.handle_in_head()
        } else if self.tag_in(true, &HEAD_NOSCRIPT) || (self.token.ty == GUMBO_TOKEN_END_TAG && !self.tag_is(false, GUMBO_TAG_BR)) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.parser_add_parse_error();
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_HEAD);
            self.state.reprocess_current_token = true;
            false
        }
    }

    fn handle_after_head(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_AFTER_HEAD as usize);
        if self.token.ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if self.token.ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.token.ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(true, GUMBO_TAG_BODY) {
            self.insert_element_from_token();
            self.state.frameset_ok = false;
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_BODY);
            true
        } else if self.tag_is(true, GUMBO_TAG_FRAMESET) {
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_FRAMESET);
            true
        } else if self.tag_in(true, &IN_HEAD_START) {
            self.parser_add_parse_error();
            self.maybe_flush_text_node_buffer();
            // The head element is set before this mode is entered; the C pushes NULL otherwise.
            let head = self.state.head_element;
            if let Some(head) = head {
                self.state.open_elements.push(head);
            }
            let result = self.handle_in_head();
            if let Some(head) = head
                && let Some(i) = self.state.open_elements.iter().position(|n| *n == head)
            {
                self.state.open_elements.remove(i);
            }
            result
        } else if self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if self.tag_is(true, GUMBO_TAG_HEAD) || (self.token.ty == GUMBO_TOKEN_END_TAG && !self.tag_in(false, &BODY_HTML_BR)) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.insert_element_of_tag_type(GUMBO_TAG_BODY, GUMBO_INSERTION_IMPLIED);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_BODY);
            self.state.reprocess_current_token = true;
            true
        }
    }

    fn handle_in_body(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_BODY as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_NULL {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_WHITESPACE {
            self.reconstruct_active_formatting_elements();
            self.insert_text_token();
            true
        } else if ty == GUMBO_TOKEN_CHARACTER || ty == GUMBO_TOKEN_CDATA {
            self.reconstruct_active_formatting_elements();
            self.insert_text_token();
            self.set_frameset_not_ok();
            true
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.parser_add_parse_error();
            if self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.ignore_token();
                return false;
            }
            if let Some(root) = self.root {
                self.merge_attributes(root);
            }
            false
        } else if self.tag_in(true, &IN_HEAD_START) || self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if self.tag_is(true, GUMBO_TAG_BODY) {
            self.parser_add_parse_error();
            let open = &self.state.open_elements;
            if open.len() < 2 || !self.node_html_tag_is(open[1], GUMBO_TAG_BODY) || self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.ignore_token();
                return false;
            }
            self.state.frameset_ok = false;
            let body = self.state.open_elements[1];
            self.merge_attributes(body);
            false
        } else if self.tag_is(true, GUMBO_TAG_FRAMESET) {
            self.parser_add_parse_error();
            let open = &self.state.open_elements;
            if open.len() < 2 || !self.node_html_tag_is(open[1], GUMBO_TAG_BODY) || !self.state.frameset_ok {
                self.ignore_token();
                return false;
            }
            let body_node = self.state.open_elements[1];
            // The C compares with open_elements->data[1], which still holds the body once it is popped.
            loop {
                let node = self.pop_current_node();
                if node == Some(body_node) || node.is_none() {
                    break;
                }
            }
            self.clear_active_formatting_elements();
            if let Some(root) = self.root {
                let children = self.children_mut(root);
                if let Some(i) = children.iter().position(|c| *c == body_node) {
                    // gumbo_vector_remove_at: the siblings after it keep their index_within_parent.
                    children.remove(i);
                }
            }
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_FRAMESET);
            true
        } else if ty == GUMBO_TOKEN_EOF {
            for i in 0..self.state.open_elements.len() {
                if !self.node_tag_in_set(self.state.open_elements[i], &BODY_EOF_ALLOWED) {
                    self.parser_add_parse_error();
                }
            }
            if self.get_current_template_insertion_mode() != GUMBO_INSERTION_MODE_INITIAL {
                return self.handle_in_template();
            }
            true
        } else if self.tag_in(false, &BODY_HTML) {
            if !self.has_an_element_in_scope(GUMBO_TAG_BODY) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            let mut success = true;
            for i in 0..self.state.open_elements.len() {
                if !self.node_tag_in_set(self.state.open_elements[i], &BODY_END_ALLOWED) {
                    self.parser_add_parse_error();
                    success = false;
                    break;
                }
            }
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_BODY);
            if self.tag_is(false, GUMBO_TAG_HTML) {
                self.state.reprocess_current_token = true;
            } else {
                let body = self.state.open_elements[1];
                self.record_end_of_element(body);
            }
            success
        } else if self.tag_in(true, &BLOCK_START) {
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            result
        } else if self.tag_in(true, &HEADINGS) {
            let mut result = self.maybe_implicitly_close_p_tag();
            if self.current_node_tag_in_set(&HEADINGS) {
                self.parser_add_parse_error();
                self.pop_current_node();
                result = false;
            }
            self.insert_element_from_token();
            result
        } else if self.tag_in(true, &PRE_LISTING) {
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            self.state.ignore_next_linefeed = true;
            self.state.frameset_ok = false;
            result
        } else if self.tag_is(true, GUMBO_TAG_FORM) {
            if self.state.form_element.is_some() && !self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            let result = self.maybe_implicitly_close_p_tag();
            let form_element = self.insert_element_from_token();
            if !self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.state.form_element = Some(form_element);
            }
            result
        } else if self.tag_is(true, GUMBO_TAG_LI) {
            self.maybe_implicitly_close_list_tag(true);
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            result
        } else if self.tag_in(true, &DD_DT) {
            self.maybe_implicitly_close_list_tag(false);
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            result
        } else if self.tag_is(true, GUMBO_TAG_PLAINTEXT) {
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            self.gumbo_tokenizer_set_state(GUMBO_LEX_PLAINTEXT);
            result
        } else if self.tag_is(true, GUMBO_TAG_BUTTON) {
            if self.has_an_element_in_scope(GUMBO_TAG_BUTTON) {
                self.parser_add_parse_error();
                self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, GUMBO_TAG_BUTTON);
                self.state.reprocess_current_token = true;
                return false;
            }
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            self.state.frameset_ok = false;
            true
        } else if self.tag_in(false, &BLOCK_END) {
            let tag = self.token.end_tag();
            if !self.has_an_element_in_scope(tag) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, tag);
            true
        } else if self.tag_is(false, GUMBO_TAG_FORM) {
            if self.has_open_element(GUMBO_TAG_TEMPLATE) {
                if !self.has_an_element_in_scope(GUMBO_TAG_FORM) {
                    self.parser_add_parse_error();
                    self.ignore_token();
                    return false;
                }
                let success = true;
                self.generate_implied_end_tags(GUMBO_TAG_LAST);
                if !self.current_node_html_tag_is(GUMBO_TAG_FORM) {
                    self.parser_add_parse_error();
                    return false;
                }
                loop {
                    let node = self.pop();
                    if self.node_html_tag_is(node, GUMBO_TAG_FORM) {
                        break;
                    }
                }
                success
            } else {
                let mut result = true;
                let node = self.state.form_element;
                self.state.form_element = None;
                let Some(node) = node.filter(|n| self.has_node_in_scope(*n)) else {
                    self.parser_add_parse_error();
                    self.ignore_token();
                    return false;
                };
                self.maybe_flush_text_node_buffer();
                self.generate_implied_end_tags(GUMBO_TAG_LAST);
                if self.get_current_node() == Some(node) {
                    self.record_end_of_element(node);
                } else {
                    self.parser_add_parse_error();
                    result = false;
                }
                if let Some(index) = self.state.open_elements.iter().position(|n| *n == node) {
                    self.state.open_elements.remove(index);
                }
                result
            }
        } else if self.tag_is(false, GUMBO_TAG_P) {
            if !self.has_an_element_in_button_scope(GUMBO_TAG_P) {
                self.parser_add_parse_error();
                self.insert_element_of_tag_type(GUMBO_TAG_P, GUMBO_INSERTION_CONVERTED_FROM_END_TAG);
                self.state.reprocess_current_token = true;
                return false;
            }
            self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, GUMBO_TAG_P)
        } else if self.tag_is(false, GUMBO_TAG_LI) {
            if !self.has_an_element_in_list_scope(GUMBO_TAG_LI) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, GUMBO_TAG_LI)
        } else if self.tag_in(false, &DD_DT) {
            let token_tag = self.token.end_tag();
            if !self.has_an_element_in_scope(token_tag) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, token_tag)
        } else if self.tag_in(false, &HEADINGS) {
            if !self.has_an_element_in_specific_scope(
                &[GUMBO_TAG_H1, GUMBO_TAG_H2, GUMBO_TAG_H3, GUMBO_TAG_H4, GUMBO_TAG_H5, GUMBO_TAG_H6],
                false,
                &SCOPE,
            ) {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            } else {
                self.generate_implied_end_tags(GUMBO_TAG_LAST);
                let end_tag = self.token.end_tag();
                let success = self.current_node_html_tag_is(end_tag);
                if !success {
                    self.parser_add_parse_error();
                }
                loop {
                    let node = self.pop();
                    if self.node_tag_in_set(node, &HEADINGS) {
                        break;
                    }
                }
                success
            }
        } else if self.tag_is(true, GUMBO_TAG_A) {
            let mut success = true;
            if self.find_last_anchor_index().is_some() {
                self.parser_add_parse_error();
                self.adoption_agency_algorithm(GUMBO_TAG_A);
                if let Some(last_a) = self.find_last_anchor_index() {
                    let last_element = self.state.active_formatting_elements.remove(last_a);
                    if let Some(i) = self.state.open_elements.iter().position(|n| *n == last_element) {
                        self.state.open_elements.remove(i);
                    }
                }
                success = false;
            }
            self.reconstruct_active_formatting_elements();
            let element = self.insert_element_from_token();
            self.add_formatting_element(element);
            success
        } else if self.tag_in(true, &FORMATTING) {
            self.reconstruct_active_formatting_elements();
            let element = self.insert_element_from_token();
            self.add_formatting_element(element);
            true
        } else if self.tag_is(true, GUMBO_TAG_NOBR) {
            let mut result = true;
            self.reconstruct_active_formatting_elements();
            if self.has_an_element_in_scope(GUMBO_TAG_NOBR) {
                result = false;
                self.parser_add_parse_error();
                self.adoption_agency_algorithm(GUMBO_TAG_NOBR);
                self.reconstruct_active_formatting_elements();
            }
            self.insert_element_from_token();
            let current = self.current();
            self.add_formatting_element(current);
            result
        } else if self.tag_in(false, &FORMATTING_END) {
            let tag = self.token.end_tag();
            self.adoption_agency_algorithm(tag)
        } else if self.tag_in(true, &APPLET_MARQUEE_OBJECT) {
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            self.add_formatting_element(MARKER);
            self.set_frameset_not_ok();
            true
        } else if self.tag_in(false, &APPLET_MARQUEE_OBJECT) {
            let token_tag = self.token.end_tag();
            if !self.has_an_element_in_table_scope(token_tag) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.implicitly_close_tags(GUMBO_NAMESPACE_HTML, token_tag);
            self.clear_active_formatting_elements();
            true
        } else if self.tag_is(true, GUMBO_TAG_TABLE) {
            if self.nodes[DOCUMENT].document().doc_type_quirks_mode != GUMBO_DOCTYPE_QUIRKS {
                self.maybe_implicitly_close_p_tag();
            }
            self.insert_element_from_token();
            self.set_frameset_not_ok();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            true
        } else if self.tag_in(true, &VOID_IN_BODY) {
            let mut success = true;
            if self.tag_is(true, GUMBO_TAG_IMAGE) {
                success = false;
                self.parser_add_parse_error();
                self.token.start_tag_mut().tag = GUMBO_TAG_IMG;
            }
            self.reconstruct_active_formatting_elements();
            let node = self.insert_element_from_token();
            // The tag is IMG by now, so this never holds and FROM_IMAGE is never set.
            if self.tag_is(true, GUMBO_TAG_IMAGE) {
                success = false;
                self.parser_add_parse_error();
                self.elem_mut(node).tag = GUMBO_TAG_IMG;
                self.nodes[node].parse_flags |= GUMBO_INSERTION_FROM_IMAGE;
            }
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            self.set_frameset_not_ok();
            success
        } else if self.tag_is(true, GUMBO_TAG_INPUT) {
            if !attribute_matches(&self.token.start_tag().attributes, b"type", b"hidden") {
                self.set_frameset_not_ok();
            }
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_in(true, &PARAM_SOURCE_TRACK) {
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_is(true, GUMBO_TAG_HR) {
            let result = self.maybe_implicitly_close_p_tag();
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            self.set_frameset_not_ok();
            result
        } else if self.tag_is(true, GUMBO_TAG_ISINDEX) {
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            if self.token.start_tag().is_self_closing {
                self.pop_current_node();
                self.acknowledge_self_closing_tag();
            }
            true
        } else if self.tag_is(true, GUMBO_TAG_TEXTAREA) {
            self.run_generic_parsing_algorithm(GUMBO_LEX_RCDATA);
            self.state.ignore_next_linefeed = true;
            self.set_frameset_not_ok();
            true
        } else if self.tag_is(true, GUMBO_TAG_XMP) {
            let result = self.maybe_implicitly_close_p_tag();
            self.reconstruct_active_formatting_elements();
            self.set_frameset_not_ok();
            self.run_generic_parsing_algorithm(GUMBO_LEX_RAWTEXT);
            result
        } else if self.tag_is(true, GUMBO_TAG_IFRAME) {
            self.set_frameset_not_ok();
            self.run_generic_parsing_algorithm(GUMBO_LEX_RAWTEXT);
            true
        } else if self.tag_is(true, GUMBO_TAG_NOEMBED) {
            self.run_generic_parsing_algorithm(GUMBO_LEX_RAWTEXT);
            true
        } else if self.tag_is(true, GUMBO_TAG_SELECT) {
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            self.set_frameset_not_ok();
            let mode = self.state.insertion_mode;
            if mode == GUMBO_INSERTION_MODE_IN_TABLE
                || mode == GUMBO_INSERTION_MODE_IN_CAPTION
                || mode == GUMBO_INSERTION_MODE_IN_TABLE_BODY
                || mode == GUMBO_INSERTION_MODE_IN_ROW
                || mode == GUMBO_INSERTION_MODE_IN_CELL
            {
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_SELECT_IN_TABLE);
            } else {
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_SELECT);
            }
            true
        } else if self.tag_in(true, &OPTION_OPTGROUP) {
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION) {
                self.pop_current_node();
            }
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            true
        } else if self.tag_in(true, &RUBY_PARTS) {
            let mut success = true;
            let exception = if self.tag_in(true, &RT_RP) { GUMBO_TAG_RTC } else { GUMBO_TAG_LAST };
            if self.has_an_element_in_scope(GUMBO_TAG_RUBY) {
                self.generate_implied_end_tags(exception);
            }
            // The C writes `!ruby && !(exception == LAST || rtc)`.
            if !(self.current_node_html_tag_is(GUMBO_TAG_RUBY)
                || exception == GUMBO_TAG_LAST
                || self.current_node_html_tag_is(GUMBO_TAG_RTC))
            {
                self.parser_add_parse_error();
                success = false;
            }
            self.insert_element_from_token();
            success
        } else if self.tag_is(false, GUMBO_TAG_BR) {
            self.parser_add_parse_error();
            self.reconstruct_active_formatting_elements();
            self.insert_element_of_tag_type(GUMBO_TAG_BR, GUMBO_INSERTION_CONVERTED_FROM_END_TAG);
            self.pop_current_node();
            false
        } else if self.tag_is(true, GUMBO_TAG_MATH) {
            self.reconstruct_active_formatting_elements();
            self.adjust_mathml_attributes();
            self.adjust_foreign_attributes();
            self.insert_foreign_element(GUMBO_NAMESPACE_MATHML);
            if self.token.start_tag().is_self_closing {
                self.pop_current_node();
                self.acknowledge_self_closing_tag();
            }
            true
        } else if self.tag_is(true, GUMBO_TAG_SVG) {
            self.reconstruct_active_formatting_elements();
            self.adjust_svg_attributes();
            self.adjust_foreign_attributes();
            self.insert_foreign_element(GUMBO_NAMESPACE_SVG);
            if self.token.start_tag().is_self_closing {
                self.pop_current_node();
                self.acknowledge_self_closing_tag();
            }
            true
        } else if self.tag_in(true, &TABLE_PARTS_IN_BODY) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_START_TAG {
            self.reconstruct_active_formatting_elements();
            self.insert_element_from_token();
            true
        } else {
            let end_tag = self.token.end_tag();
            for i in (0..self.state.open_elements.len()).rev() {
                let node = self.state.open_elements[i];
                if self.node_html_tag_is(node, end_tag) {
                    self.generate_implied_end_tags(end_tag);
                    while self.pop_current_node().is_some_and(|popped| popped != node) {}
                    return true;
                } else if self.is_special_node(node) {
                    self.parser_add_parse_error();
                    self.ignore_token();
                    return false;
                }
            }
            false
        }
    }

    fn handle_text(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_TEXT as usize);
        if self.token.ty == GUMBO_TOKEN_CHARACTER || self.token.ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
        } else {
            if self.token.ty == GUMBO_TOKEN_EOF {
                self.parser_add_parse_error();
                self.state.reprocess_current_token = true;
            }
            self.pop_current_node();
            self.set_insertion_mode(self.state.original_insertion_mode);
        }
        true
    }

    fn handle_in_table(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_TABLE as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_CHARACTER || ty == GUMBO_TOKEN_WHITESPACE {
            self.state.original_insertion_mode = self.state.insertion_mode;
            self.state.reprocess_current_token = true;
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE_TEXT);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.tag_is(true, GUMBO_TAG_CAPTION) {
            self.clear_stack_to_context(&TABLE_CONTEXT);
            self.add_formatting_element(MARKER);
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_CAPTION);
            true
        } else if self.tag_is(true, GUMBO_TAG_COLGROUP) {
            self.clear_stack_to_context(&TABLE_CONTEXT);
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_COLUMN_GROUP);
            true
        } else if self.tag_is(true, GUMBO_TAG_COL) {
            self.clear_stack_to_context(&TABLE_CONTEXT);
            self.insert_element_of_tag_type(GUMBO_TAG_COLGROUP, GUMBO_INSERTION_IMPLIED);
            self.state.reprocess_current_token = true;
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_COLUMN_GROUP);
            true
        } else if self.tag_in(true, &TABLE_SECTIONS_AND_CELLS) {
            self.clear_stack_to_context(&TABLE_CONTEXT);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE_BODY);
            if self.tag_in(true, &TD_TH_TR) {
                self.insert_element_of_tag_type(GUMBO_TAG_TBODY, GUMBO_INSERTION_IMPLIED);
                self.state.reprocess_current_token = true;
            } else {
                self.insert_element_from_token();
            }
            true
        } else if self.tag_is(true, GUMBO_TAG_TABLE) {
            self.parser_add_parse_error();
            if self.close_table() {
                self.state.reprocess_current_token = true;
            } else {
                self.ignore_token();
            }
            false
        } else if self.tag_is(false, GUMBO_TAG_TABLE) {
            if !self.close_table() {
                self.parser_add_parse_error();
                return false;
            }
            true
        } else if self.tag_in(false, &IN_TABLE_END_IGNORED) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_in(true, &STYLE_SCRIPT_TEMPLATE) || self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if self.tag_is(true, GUMBO_TAG_INPUT) && attribute_matches(&self.token.start_tag().attributes, b"type", b"hidden") {
            self.parser_add_parse_error();
            self.insert_element_from_token();
            self.pop_current_node();
            false
        } else if self.tag_is(true, GUMBO_TAG_FORM) {
            self.parser_add_parse_error();
            if self.state.form_element.is_some() || self.has_open_element(GUMBO_TAG_TEMPLATE) {
                self.ignore_token();
                return false;
            }
            let form = self.insert_element_from_token();
            self.state.form_element = Some(form);
            self.pop_current_node();
            false
        } else if ty == GUMBO_TOKEN_EOF {
            self.handle_in_body()
        } else {
            self.parser_add_parse_error();
            self.state.foster_parent_insertions = true;
            let result = self.handle_in_body();
            self.state.foster_parent_insertions = false;
            result
        }
    }

    fn handle_in_table_text(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_TABLE_TEXT as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_NULL {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_CHARACTER || ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else {
            // isspace in the C locale, then '\v' taken back out.
            let foster = self.state.text_node.buffer.iter().any(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') || *c == 0x0b);
            if foster {
                self.state.foster_parent_insertions = true;
                self.reconstruct_active_formatting_elements();
            }
            self.maybe_flush_text_node_buffer();
            self.state.foster_parent_insertions = false;
            self.state.reprocess_current_token = true;
            self.state.insertion_mode = self.state.original_insertion_mode;
            true
        }
    }

    fn handle_in_caption(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_CAPTION as usize);
        if self.tag_is(false, GUMBO_TAG_CAPTION) {
            if !self.has_an_element_in_table_scope(GUMBO_TAG_CAPTION) {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            } else {
                self.generate_implied_end_tags(GUMBO_TAG_LAST);
                let result = true;
                if !self.current_node_html_tag_is(GUMBO_TAG_CAPTION) {
                    self.parser_add_parse_error();
                }
                loop {
                    let node = self.pop();
                    if self.node_html_tag_is(node, GUMBO_TAG_CAPTION) {
                        break;
                    }
                }
                self.clear_active_formatting_elements();
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
                result
            }
        } else if self.tag_in(true, &CAPTION_CLOSERS) || self.tag_is(false, GUMBO_TAG_TABLE) {
            if !self.has_an_element_in_table_scope(GUMBO_TAG_CAPTION) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            loop {
                let node = self.pop();
                if self.node_html_tag_is(node, GUMBO_TAG_CAPTION) {
                    break;
                }
            }
            self.clear_active_formatting_elements();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            self.state.reprocess_current_token = true;
            true
        } else if self.tag_in(false, &CAPTION_END_IGNORED) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.handle_in_body()
        }
    }

    fn handle_in_column_group(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_COLUMN_GROUP as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(true, GUMBO_TAG_COL) {
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_is(false, GUMBO_TAG_COLGROUP) {
            if !self.current_node_html_tag_is(GUMBO_TAG_COLGROUP) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            false
        } else if self.tag_is(false, GUMBO_TAG_COL) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_is(true, GUMBO_TAG_TEMPLATE) || self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if ty == GUMBO_TOKEN_EOF {
            self.handle_in_body()
        } else {
            if !self.current_node_html_tag_is(GUMBO_TAG_COLGROUP) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            self.state.reprocess_current_token = true;
            true
        }
    }

    fn handle_in_table_body(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_TABLE_BODY as usize);
        if self.tag_is(true, GUMBO_TAG_TR) {
            self.clear_stack_to_context(&TABLE_BODY_CONTEXT);
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_ROW);
            true
        } else if self.tag_in(true, &TD_TH) {
            self.parser_add_parse_error();
            self.clear_stack_to_context(&TABLE_BODY_CONTEXT);
            self.insert_element_of_tag_type(GUMBO_TAG_TR, GUMBO_INSERTION_IMPLIED);
            self.state.reprocess_current_token = true;
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_ROW);
            false
        } else if self.tag_in(false, &TABLE_SECTIONS) {
            if !self.has_an_element_in_table_scope(self.token.end_tag()) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.clear_stack_to_context(&TABLE_BODY_CONTEXT);
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            true
        } else if self.tag_in(true, &TABLE_BODY_CLOSERS) || self.tag_is(false, GUMBO_TAG_TABLE) {
            if !(self.has_an_element_in_table_scope(GUMBO_TAG_TBODY)
                || self.has_an_element_in_table_scope(GUMBO_TAG_THEAD)
                || self.has_an_element_in_table_scope(GUMBO_TAG_TFOOT))
            {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.clear_stack_to_context(&TABLE_BODY_CONTEXT);
            self.pop_current_node();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE);
            self.state.reprocess_current_token = true;
            true
        } else if self.tag_in(false, &TABLE_BODY_END_IGNORED) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.handle_in_table()
        }
    }

    fn handle_in_row(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_ROW as usize);
        if self.tag_in(true, &TD_TH) {
            self.clear_stack_to_context(&TABLE_ROW_CONTEXT);
            self.insert_element_from_token();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_CELL);
            self.add_formatting_element(MARKER);
            true
        } else if self.tag_is(false, GUMBO_TAG_TR) {
            if !self.has_an_element_in_table_scope(GUMBO_TAG_TR) {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            } else {
                self.clear_stack_to_context(&TABLE_ROW_CONTEXT);
                self.pop_current_node();
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE_BODY);
                true
            }
        } else if self.tag_in(true, &ROW_CLOSERS) || self.tag_is(false, GUMBO_TAG_TABLE) {
            if !self.has_an_element_in_table_scope(GUMBO_TAG_TR) {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            } else {
                self.clear_stack_to_context(&TABLE_ROW_CONTEXT);
                self.pop_current_node();
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE_BODY);
                self.state.reprocess_current_token = true;
                true
            }
        } else if self.tag_in(false, &TABLE_SECTIONS) {
            if !self.has_an_element_in_table_scope(self.token.end_tag()) || !self.has_an_element_in_table_scope(GUMBO_TAG_TR) {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            } else {
                self.clear_stack_to_context(&TABLE_ROW_CONTEXT);
                self.pop_current_node();
                self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_TABLE_BODY);
                self.state.reprocess_current_token = true;
                true
            }
        } else if self.tag_in(false, &ROW_END_IGNORED) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else {
            self.handle_in_table()
        }
    }

    fn handle_in_cell(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_CELL as usize);
        if self.tag_in(false, &TD_TH) {
            let token_tag = self.token.end_tag();
            if !self.has_an_element_in_table_scope(token_tag) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.close_table_cell(token_tag)
        } else if self.tag_in(true, &CAPTION_CLOSERS) {
            if !self.has_an_element_in_table_scope(GUMBO_TAG_TH) && !self.has_an_element_in_table_scope(GUMBO_TAG_TD) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.state.reprocess_current_token = true;
            self.close_current_cell()
        } else if self.tag_in(false, &CELL_END_IGNORED) {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_in(false, &CELL_END_CLOSERS) {
            if !self.has_an_element_in_table_scope(self.token.end_tag()) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.state.reprocess_current_token = true;
            self.close_current_cell()
        } else {
            self.handle_in_body()
        }
    }

    fn handle_in_select(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_SELECT as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_NULL {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_CHARACTER || ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(true, GUMBO_TAG_OPTION) {
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION) {
                self.pop_current_node();
            }
            self.insert_element_from_token();
            true
        } else if self.tag_is(true, GUMBO_TAG_OPTGROUP) {
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION) {
                self.pop_current_node();
            }
            if self.current_node_html_tag_is(GUMBO_TAG_OPTGROUP) {
                self.pop_current_node();
            }
            self.insert_element_from_token();
            true
        } else if self.tag_is(true, GUMBO_TAG_HR) {
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION) {
                self.pop_current_node();
            }
            if self.current_node_html_tag_is(GUMBO_TAG_OPTGROUP) {
                self.pop_current_node();
            }
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_is(false, GUMBO_TAG_OPTGROUP) {
            let open = &self.state.open_elements;
            // open_elements->data[length - 2]: the select at least is under an option.
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION)
                && open.len() >= 2
                && self.node_html_tag_is(open[open.len() - 2], GUMBO_TAG_OPTGROUP)
            {
                self.pop_current_node();
            }
            if self.current_node_html_tag_is(GUMBO_TAG_OPTGROUP) {
                self.pop_current_node();
                true
            } else {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            }
        } else if self.tag_is(false, GUMBO_TAG_OPTION) {
            if self.current_node_html_tag_is(GUMBO_TAG_OPTION) {
                self.pop_current_node();
                true
            } else {
                self.parser_add_parse_error();
                self.ignore_token();
                false
            }
        } else if self.tag_is(false, GUMBO_TAG_SELECT) {
            if !self.has_an_element_in_select_scope(GUMBO_TAG_SELECT) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.close_current_select();
            true
        } else if self.tag_is(true, GUMBO_TAG_SELECT) {
            self.parser_add_parse_error();
            self.ignore_token();
            if self.has_an_element_in_select_scope(GUMBO_TAG_SELECT) {
                self.close_current_select();
            }
            false
        } else if self.tag_in(true, &INPUT_KEYGEN_TEXTAREA) {
            self.parser_add_parse_error();
            if !self.has_an_element_in_select_scope(GUMBO_TAG_SELECT) {
                self.ignore_token();
            } else {
                self.close_current_select();
                self.state.reprocess_current_token = true;
            }
            false
        } else if self.tag_in(true, &SCRIPT_TEMPLATE) || self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if ty == GUMBO_TOKEN_EOF {
            self.handle_in_body()
        } else {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        }
    }

    fn handle_in_select_in_table(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_SELECT_IN_TABLE as usize);
        if self.tag_in(true, &SELECT_IN_TABLE_CLOSERS) {
            self.parser_add_parse_error();
            self.close_current_select();
            self.state.reprocess_current_token = true;
            false
        } else if self.tag_in(false, &SELECT_IN_TABLE_CLOSERS) {
            self.parser_add_parse_error();
            if !self.has_an_element_in_table_scope(self.token.end_tag()) {
                self.ignore_token();
                false
            } else {
                self.close_current_select();
                self.state.reprocess_current_token = true;
                false
            }
        } else {
            self.handle_in_select()
        }
    }

    fn handle_in_template(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_TEMPLATE as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_WHITESPACE
            || ty == GUMBO_TOKEN_CHARACTER
            || ty == GUMBO_TOKEN_COMMENT
            || ty == GUMBO_TOKEN_NULL
            || ty == GUMBO_TOKEN_DOCTYPE
        {
            self.handle_in_body()
        } else if self.tag_in(true, &IN_HEAD_START) || self.tag_is(false, GUMBO_TAG_TEMPLATE) {
            self.handle_in_head()
        } else if self.tag_in(true, &TEMPLATE_TABLE_PARTS) {
            self.switch_template_mode(GUMBO_INSERTION_MODE_IN_TABLE)
        } else if self.tag_is(true, GUMBO_TAG_COL) {
            self.switch_template_mode(GUMBO_INSERTION_MODE_IN_COLUMN_GROUP)
        } else if self.tag_is(true, GUMBO_TAG_TR) {
            self.switch_template_mode(GUMBO_INSERTION_MODE_IN_TABLE_BODY)
        } else if self.tag_in(true, &TD_TH) {
            self.switch_template_mode(GUMBO_INSERTION_MODE_IN_ROW)
        } else if ty == GUMBO_TOKEN_START_TAG {
            self.switch_template_mode(GUMBO_INSERTION_MODE_IN_BODY)
        } else if ty == GUMBO_TOKEN_END_TAG {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if ty == GUMBO_TOKEN_EOF {
            if !self.has_open_element(GUMBO_TAG_TEMPLATE) {
                return true;
            }
            self.parser_add_parse_error();
            loop {
                let node = self.pop();
                if self.node_html_tag_is(node, GUMBO_TAG_TEMPLATE) {
                    break;
                }
            }
            self.clear_active_formatting_elements();
            self.pop_template_insertion_mode();
            self.reset_insertion_mode_appropriately();
            self.state.reprocess_current_token = true;
            false
        } else {
            false
        }
    }

    fn switch_template_mode(&mut self, mode: GumboInsertionMode) -> bool {
        self.pop_template_insertion_mode();
        self.push_template_insertion_mode(mode);
        self.set_insertion_mode(mode);
        self.state.reprocess_current_token = true;
        true
    }

    fn handle_after_body(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_AFTER_BODY as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_WHITESPACE || self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if ty == GUMBO_TOKEN_COMMENT {
            if let Some(html_node) = self.root {
                self.append_comment_node(html_node);
            }
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_is(false, GUMBO_TAG_HTML) {
            if self.is_fragment_parser() {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_AFTER_BODY);
            let html = self.state.open_elements[0];
            self.record_end_of_element(html);
            true
        } else if ty == GUMBO_TOKEN_EOF {
            true
        } else {
            self.parser_add_parse_error();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_BODY);
            self.state.reprocess_current_token = true;
            false
        }
    }

    fn handle_in_frameset(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_IN_FRAMESET as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(true, GUMBO_TAG_FRAMESET) {
            self.insert_element_from_token();
            true
        } else if self.tag_is(false, GUMBO_TAG_FRAMESET) {
            if self.current_node_html_tag_is(GUMBO_TAG_HTML) {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            self.pop_current_node();
            if !self.is_fragment_parser() && !self.current_node_html_tag_is(GUMBO_TAG_FRAMESET) {
                self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_FRAMESET);
            }
            true
        } else if self.tag_is(true, GUMBO_TAG_FRAME) {
            self.insert_element_from_token();
            self.pop_current_node();
            self.acknowledge_self_closing_tag();
            true
        } else if self.tag_is(true, GUMBO_TAG_NOFRAMES) {
            self.handle_in_head()
        } else if ty == GUMBO_TOKEN_EOF {
            if !self.current_node_html_tag_is(GUMBO_TAG_HTML) {
                self.parser_add_parse_error();
                return false;
            }
            true
        } else {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        }
    }

    fn handle_after_frameset(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_AFTER_FRAMESET as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_WHITESPACE {
            self.insert_text_token();
            true
        } else if ty == GUMBO_TOKEN_COMMENT {
            let current = self.current();
            self.append_comment_node(current);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        } else if self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if self.tag_is(false, GUMBO_TAG_HTML) {
            let html = self.state.open_elements[0];
            self.record_end_of_element(html);
            self.set_insertion_mode(GUMBO_INSERTION_MODE_AFTER_AFTER_FRAMESET);
            true
        } else if self.tag_is(true, GUMBO_TAG_NOFRAMES) {
            self.handle_in_head()
        } else if ty == GUMBO_TOKEN_EOF {
            true
        } else {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        }
    }

    fn handle_after_after_body(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_AFTER_AFTER_BODY as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_COMMENT {
            self.append_comment_node(DOCUMENT);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE || ty == GUMBO_TOKEN_WHITESPACE || self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if ty == GUMBO_TOKEN_EOF {
            true
        } else {
            self.parser_add_parse_error();
            self.set_insertion_mode(GUMBO_INSERTION_MODE_IN_BODY);
            self.state.reprocess_current_token = true;
            false
        }
    }

    fn handle_after_after_frameset(&mut self) -> bool {
        self.count(GUMBO_INSERTION_MODE_AFTER_AFTER_FRAMESET as usize);
        let ty = self.token.ty;
        if ty == GUMBO_TOKEN_COMMENT {
            self.append_comment_node(DOCUMENT);
            true
        } else if ty == GUMBO_TOKEN_DOCTYPE || ty == GUMBO_TOKEN_WHITESPACE || self.tag_is(true, GUMBO_TAG_HTML) {
            self.handle_in_body()
        } else if ty == GUMBO_TOKEN_EOF {
            true
        } else if self.tag_is(true, GUMBO_TAG_NOFRAMES) {
            self.handle_in_head()
        } else {
            self.parser_add_parse_error();
            self.ignore_token();
            false
        }
    }

    fn handle_html_content(&mut self) -> bool {
        match self.state.insertion_mode {
            GUMBO_INSERTION_MODE_INITIAL => self.handle_initial(),
            GUMBO_INSERTION_MODE_BEFORE_HTML => self.handle_before_html(),
            GUMBO_INSERTION_MODE_BEFORE_HEAD => self.handle_before_head(),
            GUMBO_INSERTION_MODE_IN_HEAD => self.handle_in_head(),
            GUMBO_INSERTION_MODE_IN_HEAD_NOSCRIPT => self.handle_in_head_noscript(),
            GUMBO_INSERTION_MODE_AFTER_HEAD => self.handle_after_head(),
            GUMBO_INSERTION_MODE_IN_BODY => self.handle_in_body(),
            GUMBO_INSERTION_MODE_TEXT => self.handle_text(),
            GUMBO_INSERTION_MODE_IN_TABLE => self.handle_in_table(),
            GUMBO_INSERTION_MODE_IN_TABLE_TEXT => self.handle_in_table_text(),
            GUMBO_INSERTION_MODE_IN_CAPTION => self.handle_in_caption(),
            GUMBO_INSERTION_MODE_IN_COLUMN_GROUP => self.handle_in_column_group(),
            GUMBO_INSERTION_MODE_IN_TABLE_BODY => self.handle_in_table_body(),
            GUMBO_INSERTION_MODE_IN_ROW => self.handle_in_row(),
            GUMBO_INSERTION_MODE_IN_CELL => self.handle_in_cell(),
            GUMBO_INSERTION_MODE_IN_SELECT => self.handle_in_select(),
            GUMBO_INSERTION_MODE_IN_SELECT_IN_TABLE => self.handle_in_select_in_table(),
            GUMBO_INSERTION_MODE_IN_TEMPLATE => self.handle_in_template(),
            GUMBO_INSERTION_MODE_AFTER_BODY => self.handle_after_body(),
            GUMBO_INSERTION_MODE_IN_FRAMESET => self.handle_in_frameset(),
            GUMBO_INSERTION_MODE_AFTER_FRAMESET => self.handle_after_frameset(),
            GUMBO_INSERTION_MODE_AFTER_AFTER_BODY => self.handle_after_after_body(),
            _ => self.handle_after_after_frameset(),
        }
    }

    fn handle_in_foreign_content(&mut self) -> bool {
        self.count(REACHED_FOREIGN_CONTENT);
        match self.token.ty {
            GUMBO_TOKEN_NULL => {
                self.parser_add_parse_error();
                self.token.v = GumboTokenData::Character(kUtf8ReplacementChar);
                self.insert_text_token();
                return false;
            }
            GUMBO_TOKEN_WHITESPACE => {
                self.insert_text_token();
                return true;
            }
            GUMBO_TOKEN_CDATA | GUMBO_TOKEN_CHARACTER => {
                self.insert_text_token();
                self.set_frameset_not_ok();
                return true;
            }
            GUMBO_TOKEN_COMMENT => {
                let current = self.current();
                self.append_comment_node(current);
                return true;
            }
            GUMBO_TOKEN_DOCTYPE => {
                self.parser_add_parse_error();
                self.ignore_token();
                return false;
            }
            _ => {}
        }
        if self.tag_in(true, &FOREIGN_BREAKOUT)
            || self.tag_in(false, &BR_P)
            || (self.tag_is(true, GUMBO_TAG_FONT)
                && (self.token_has_attribute(b"color") || self.token_has_attribute(b"face") || self.token_has_attribute(b"size")))
        {
            self.parser_add_parse_error();
            while !self.is_mathml_integration_point(self.current())
                && !self.is_html_integration_point(self.current())
                && self.elem(self.current()).tag_namespace != GUMBO_NAMESPACE_HTML
            {
                self.pop_current_node();
            }
            self.handle_html_content();
            return false;
        }
        if self.token.ty == GUMBO_TOKEN_START_TAG {
            let current_namespace = self.elem(self.get_adjusted_current_node().expect("an open element")).tag_namespace;
            if current_namespace == GUMBO_NAMESPACE_MATHML {
                self.adjust_mathml_attributes();
            }
            if current_namespace == GUMBO_NAMESPACE_SVG {
                self.adjust_svg_attributes();
            }
            self.adjust_foreign_attributes();
            self.insert_foreign_element(current_namespace);
            if self.token.start_tag().is_self_closing {
                self.pop_current_node();
                self.acknowledge_self_closing_tag();
            }
            true
        } else {
            let mut node = self.current();
            let mut token_tagname = self.token.original_text;
            let mut node_tagname = self.elem(node).original_tag;
            gumbo_tag_from_original_text(self.text, &mut token_tagname);
            gumbo_tag_from_original_text(self.text, &mut node_tagname);
            let equals = |p: &Self, a: &GumboStringPiece, b: &GumboStringPiece| {
                // gumbo_string_equals_ignore_case: the lengths, then strncasecmp, which stops at a NUL in both.
                a.length == b.length && {
                    let (x, y) = (a.bytes(p.text), b.bytes(p.text));
                    let n = x.len().min(y.len());
                    let stop = (0..n).find(|&k| x[k] == 0 || y[k] == 0 || !x[k].eq_ignore_ascii_case(&y[k]));
                    match stop {
                        Some(k) => x[k].eq_ignore_ascii_case(&y[k]) && x[k] == 0,
                        None => true,
                    }
                }
            };
            let mut is_success = true;
            if !equals(self, &node_tagname, &token_tagname) {
                self.parser_add_parse_error();
                is_success = false;
            }
            let mut i = self.state.open_elements.len() as i32 - 1;
            while i > 0 {
                if equals(self, &node_tagname, &token_tagname) {
                    while self.pop_current_node().is_some_and(|popped| popped != node) {}
                    return is_success;
                }
                i -= 1;
                node = self.state.open_elements[i as usize];
                if self.elem(node).tag_namespace == GUMBO_NAMESPACE_HTML {
                    break;
                }
                node_tagname = self.elem(node).original_tag;
                gumbo_tag_from_original_text(self.text, &mut node_tagname);
            }
            self.handle_html_content() && is_success
        }
    }

    fn handle_token(&mut self) -> bool {
        if self.state.ignore_next_linefeed && self.token.ty == GUMBO_TOKEN_WHITESPACE && self.token.character() == i32::from(b'\n') {
            self.state.ignore_next_linefeed = false;
            self.ignore_token();
            return true;
        }
        self.state.ignore_next_linefeed = false;
        if self.tag_is(false, GUMBO_TAG_BODY) {
            self.state.closed_body_tag = true;
        }
        if self.tag_is(false, GUMBO_TAG_HTML) {
            self.state.closed_html_tag = true;
        }
        let current_node = self.get_adjusted_current_node();
        let ty = self.token.ty;
        let html_content = match current_node {
            None => true,
            Some(current) => {
                let ns = self.elem(current).tag_namespace;
                ns == GUMBO_NAMESPACE_HTML
                    || (self.is_mathml_integration_point(current)
                        && (ty == GUMBO_TOKEN_CHARACTER
                            || ty == GUMBO_TOKEN_WHITESPACE
                            || ty == GUMBO_TOKEN_NULL
                            || (ty == GUMBO_TOKEN_START_TAG && !self.tag_in(true, &MGLYPH_MALIGNMARK))))
                    || (ns == GUMBO_NAMESPACE_MATHML
                        && self.node_qualified_tag_is(current, GUMBO_NAMESPACE_MATHML, GUMBO_TAG_ANNOTATION_XML)
                        && self.tag_is(true, GUMBO_TAG_SVG))
                    || (self.is_html_integration_point(current)
                        && (ty == GUMBO_TOKEN_START_TAG
                            || ty == GUMBO_TOKEN_CHARACTER
                            || ty == GUMBO_TOKEN_NULL
                            || ty == GUMBO_TOKEN_WHITESPACE))
                    || ty == GUMBO_TOKEN_EOF
            }
        };
        if html_content { self.handle_html_content() } else { self.handle_in_foreign_content() }
    }

    fn fragment_parser_init(&mut self, fragment_ctx: GumboTag, fragment_namespace: GumboNamespaceEnum) {
        let ctx = self.create_element(fragment_ctx);
        self.elem_mut(ctx).tag_namespace = fragment_namespace;
        self.state.fragment_ctx = Some(ctx);
        if fragment_namespace == GUMBO_NAMESPACE_HTML {
            match fragment_ctx {
                GUMBO_TAG_TITLE | GUMBO_TAG_TEXTAREA => self.gumbo_tokenizer_set_state(GUMBO_LEX_RCDATA),
                GUMBO_TAG_STYLE | GUMBO_TAG_XMP | GUMBO_TAG_IFRAME | GUMBO_TAG_NOEMBED | GUMBO_TAG_NOFRAMES => {
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_RAWTEXT)
                }
                GUMBO_TAG_SCRIPT => self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT),
                GUMBO_TAG_PLAINTEXT => self.gumbo_tokenizer_set_state(GUMBO_LEX_PLAINTEXT),
                _ => {}
            }
        }
        let root = self.insert_element_of_tag_type(GUMBO_TAG_HTML, GUMBO_INSERTION_IMPLIED);
        self.root = Some(root);
        if fragment_ctx == GUMBO_TAG_TEMPLATE {
            self.push_template_insertion_mode(GUMBO_INSERTION_MODE_IN_TEMPLATE);
        }
        self.reset_insertion_mode_appropriately();
    }
}

/// `gumbo_parse_with_options` of `buffer`, the bytes the C gets as `buffer` and `buffer_length`.
pub fn gumbo_parse_with_options(options: &GumboOptions, buffer: &[u8]) -> GumboOutput {
    let length = buffer.len();
    let document = GumboNode {
        ty: GUMBO_NODE_DOCUMENT,
        parent: None,
        index_within_parent: NO_INDEX,
        parse_flags: GUMBO_INSERTION_BY_PARSER,
        v: GumboNodeData::Document(GumboDocument::default()),
    };
    let mut parser = Parser {
        text: buffer,
        options: *options,
        errors: Errors { list: Vec::new(), max_errors: options.max_errors },
        tokenizer: GumboTokenizerState::default(),
        reached: Reached::default(),
        nodes: vec![document],
        root: None,
        truncated: false,
        state: GumboParserState::default(),
        token: GumboToken::default(),
        has_current_token: false,
    };
    parser.gumbo_tokenizer_state_init();
    parser.state = GumboParserState {
        insertion_mode: GUMBO_INSERTION_MODE_INITIAL,
        frameset_ok: true,
        text_node: TextNodeBufferState { ty: GUMBO_NODE_WHITESPACE, ..TextNodeBufferState::default() },
        ..GumboParserState::default()
    };
    if options.fragment_context != GUMBO_TAG_LAST {
        parser.fragment_parser_init(options.fragment_context, options.fragment_namespace);
    }
    let mut has_error = false;
    loop {
        if parser.state.reprocess_current_token {
            parser.state.reprocess_current_token = false;
        } else {
            if parser.state.text_node.buffer.is_empty()
                && (parser.state.open_elements.len() > kMaxOpenElements || parser.state.cloned_bytes > length.wrapping_add(kMaxClonedBytes))
            {
                parser.truncated = true;
                break;
            }
            let current_node = parser.get_current_node();
            let foreign = current_node.is_some_and(|n| parser.elem(n).tag_namespace != GUMBO_NAMESPACE_HTML);
            parser.gumbo_tokenizer_set_is_current_node_foreign(foreign);
            let mut token = std::mem::take(&mut parser.token);
            let lexed = parser.gumbo_lex(&mut token);
            parser.token = token;
            has_error = !lexed || has_error;
        }
        parser.has_current_token = true;
        parser.state.self_closing_flag_acknowledged =
            !(parser.token.ty == GUMBO_TOKEN_START_TAG && parser.token.start_tag().is_self_closing);
        has_error = !parser.handle_token() || has_error;
        if !parser.state.self_closing_flag_acknowledged
            && parser.parser_add_parse_error()
            && let Some(error) = parser.errors.list.last_mut()
        {
            error.ty = GUMBO_ERR_UNACKNOWLEDGED_SELF_CLOSING_TAG;
        }
        if !((parser.token.ty != GUMBO_TOKEN_EOF || parser.state.reprocess_current_token) && !(options.stop_on_first_error && has_error)) {
            break;
        }
    }
    parser.finish_parsing();
    if let GumboNodeData::Document(doc) = &mut parser.nodes[DOCUMENT].v {
        doc.name.get_or_insert_with(Vec::new);
        doc.public_identifier.get_or_insert_with(Vec::new);
        doc.system_identifier.get_or_insert_with(Vec::new);
    }
    GumboOutput {
        nodes: parser.nodes,
        document: DOCUMENT,
        root: parser.root,
        errors: parser.errors.list,
        truncated: parser.truncated,
        reached: parser.reached,
    }
}
