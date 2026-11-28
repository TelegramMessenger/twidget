//! error.h and error.c's `gumbo_add_error`: the errors a parse records, each with the part of the C union its kind
//! sets. The messages (`gumbo_error_to_string`, the caret diagnostics) are not ported: nothing in the fork prints
//! them.

use super::{GumboSourcePosition, GumboStringPiece, GumboTag};

pub type GumboErrorType = u32;
pub const GUMBO_ERR_UTF8_INVALID: GumboErrorType = 0;
pub const GUMBO_ERR_UTF8_TRUNCATED: GumboErrorType = 1;
pub const GUMBO_ERR_UTF8_NULL: GumboErrorType = 2;
pub const GUMBO_ERR_NUMERIC_CHAR_REF_NO_DIGITS: GumboErrorType = 3;
pub const GUMBO_ERR_NUMERIC_CHAR_REF_WITHOUT_SEMICOLON: GumboErrorType = 4;
pub const GUMBO_ERR_NUMERIC_CHAR_REF_INVALID: GumboErrorType = 5;
pub const GUMBO_ERR_NAMED_CHAR_REF_WITHOUT_SEMICOLON: GumboErrorType = 6;
pub const GUMBO_ERR_NAMED_CHAR_REF_INVALID: GumboErrorType = 7;
pub const GUMBO_ERR_TAG_STARTS_WITH_QUESTION: GumboErrorType = 8;
pub const GUMBO_ERR_TAG_EOF: GumboErrorType = 9;
pub const GUMBO_ERR_TAG_INVALID: GumboErrorType = 10;
pub const GUMBO_ERR_CLOSE_TAG_EMPTY: GumboErrorType = 11;
pub const GUMBO_ERR_CLOSE_TAG_EOF: GumboErrorType = 12;
pub const GUMBO_ERR_CLOSE_TAG_INVALID: GumboErrorType = 13;
pub const GUMBO_ERR_SCRIPT_EOF: GumboErrorType = 14;
pub const GUMBO_ERR_ATTR_NAME_EOF: GumboErrorType = 15;
pub const GUMBO_ERR_ATTR_NAME_INVALID: GumboErrorType = 16;
pub const GUMBO_ERR_ATTR_DOUBLE_QUOTE_EOF: GumboErrorType = 17;
pub const GUMBO_ERR_ATTR_SINGLE_QUOTE_EOF: GumboErrorType = 18;
pub const GUMBO_ERR_ATTR_UNQUOTED_EOF: GumboErrorType = 19;
pub const GUMBO_ERR_ATTR_UNQUOTED_RIGHT_BRACKET: GumboErrorType = 20;
pub const GUMBO_ERR_ATTR_UNQUOTED_EQUALS: GumboErrorType = 21;
pub const GUMBO_ERR_ATTR_AFTER_EOF: GumboErrorType = 22;
pub const GUMBO_ERR_ATTR_AFTER_INVALID: GumboErrorType = 23;
pub const GUMBO_ERR_DUPLICATE_ATTR: GumboErrorType = 24;
pub const GUMBO_ERR_SOLIDUS_EOF: GumboErrorType = 25;
pub const GUMBO_ERR_SOLIDUS_INVALID: GumboErrorType = 26;
pub const GUMBO_ERR_DASHES_OR_DOCTYPE: GumboErrorType = 27;
pub const GUMBO_ERR_COMMENT_EOF: GumboErrorType = 28;
pub const GUMBO_ERR_COMMENT_INVALID: GumboErrorType = 29;
pub const GUMBO_ERR_COMMENT_BANG_AFTER_DOUBLE_DASH: GumboErrorType = 30;
pub const GUMBO_ERR_COMMENT_DASH_AFTER_DOUBLE_DASH: GumboErrorType = 31;
pub const GUMBO_ERR_COMMENT_SPACE_AFTER_DOUBLE_DASH: GumboErrorType = 32;
pub const GUMBO_ERR_COMMENT_END_BANG_EOF: GumboErrorType = 33;
pub const GUMBO_ERR_DOCTYPE_EOF: GumboErrorType = 34;
pub const GUMBO_ERR_DOCTYPE_INVALID: GumboErrorType = 35;
pub const GUMBO_ERR_DOCTYPE_SPACE: GumboErrorType = 36;
pub const GUMBO_ERR_DOCTYPE_RIGHT_BRACKET: GumboErrorType = 37;
pub const GUMBO_ERR_DOCTYPE_SPACE_OR_RIGHT_BRACKET: GumboErrorType = 38;
pub const GUMBO_ERR_DOCTYPE_END: GumboErrorType = 39;
pub const GUMBO_ERR_PARSER: GumboErrorType = 40;
pub const GUMBO_ERR_UNACKNOWLEDGED_SELF_CLOSING_TAG: GumboErrorType = 41;

pub type GumboTokenizerErrorState = u32;
pub const GUMBO_ERR_TOKENIZER_DATA: GumboTokenizerErrorState = 0;
pub const GUMBO_ERR_TOKENIZER_CHAR_REF: GumboTokenizerErrorState = 1;
pub const GUMBO_ERR_TOKENIZER_RCDATA: GumboTokenizerErrorState = 2;
pub const GUMBO_ERR_TOKENIZER_RAWTEXT: GumboTokenizerErrorState = 3;
pub const GUMBO_ERR_TOKENIZER_PLAINTEXT: GumboTokenizerErrorState = 4;
pub const GUMBO_ERR_TOKENIZER_SCRIPT: GumboTokenizerErrorState = 5;
pub const GUMBO_ERR_TOKENIZER_TAG: GumboTokenizerErrorState = 6;
pub const GUMBO_ERR_TOKENIZER_SELF_CLOSING_TAG: GumboTokenizerErrorState = 7;
pub const GUMBO_ERR_TOKENIZER_ATTR_NAME: GumboTokenizerErrorState = 8;
pub const GUMBO_ERR_TOKENIZER_ATTR_VALUE: GumboTokenizerErrorState = 9;
pub const GUMBO_ERR_TOKENIZER_MARKUP_DECLARATION: GumboTokenizerErrorState = 10;
pub const GUMBO_ERR_TOKENIZER_COMMENT: GumboTokenizerErrorState = 11;
pub const GUMBO_ERR_TOKENIZER_DOCTYPE: GumboTokenizerErrorState = 12;
pub const GUMBO_ERR_TOKENIZER_CDATA: GumboTokenizerErrorState = 13;

/// The part of `GumboError::v` an error's kind sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GumboErrorValue {
    /// GUMBO_ERR_NUMERIC_CHAR_REF_NO_DIGITS sets none.
    None,
    /// `v.codepoint`: the bytes of an invalid UTF-8 sequence, or a numeric reference's value as an `int`
    /// widened to 64 bits.
    Codepoint(u64),
    /// `v.text`: a named reference's text.
    Text(GumboStringPiece),
    /// `v.tokenizer`.
    Tokenizer { codepoint: i32, state: GumboTokenizerErrorState },
    /// `v.duplicate_attr`.
    DuplicateAttr { name: Vec<u8>, original_index: u32, new_index: u32 },
    /// `v.parser`: the token, the insertion mode and the tags of the open elements.
    Parser { input_type: u32, input_tag: GumboTag, parser_state: u32, tag_stack: Vec<GumboTag> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GumboError {
    pub ty: GumboErrorType,
    pub position: GumboSourcePosition,
    /// `original_text`, an offset into the input.
    pub original_text: Option<usize>,
    pub v: GumboErrorValue,
}

/// The output's errors and the options' `max_errors`, which `gumbo_add_error` checks.
#[derive(Debug, Default)]
pub struct Errors {
    pub list: Vec<GumboError>,
    pub max_errors: i32,
}

impl Errors {
    /// Whether `gumbo_add_error` would give an error now.
    pub fn room(&self) -> bool {
        !(self.max_errors >= 0 && self.list.len() as u32 >= self.max_errors as u32)
    }

    /// `gumbo_add_error` with the error filled in; false where the C gets NULL and records nothing.
    pub fn add(&mut self, error: GumboError) -> bool {
        if !self.room() {
            return false;
        }
        self.list.push(error);
        true
    }
}
