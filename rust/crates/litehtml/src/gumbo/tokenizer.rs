//! tokenizer.c (with token_type.h and tokenizer_states.h): the HTML5 tokenizer, state for state. Its state lives in
//! the [`Parser`], as the C's `GumboParser` holds both; `gumbo_lex` fills the token it is given, as the C fills
//! the one the parser reuses.

use super::char_ref::{Ctx, OneOrTwoCodepoints, consume_char_ref, kGumboNoChar};
use super::error::*;
use super::parser::Parser;
use super::tag::gumbo_tagn_enum;
use super::utf8::kUtf8ReplacementChar;
use super::{GUMBO_ATTR_NAMESPACE_NONE, GUMBO_TAG_LAST, GumboAttribute, GumboSourcePosition, GumboStringPiece, GumboTag};

pub type GumboTokenType = u32;
pub const GUMBO_TOKEN_DOCTYPE: GumboTokenType = 0;
pub const GUMBO_TOKEN_START_TAG: GumboTokenType = 1;
pub const GUMBO_TOKEN_END_TAG: GumboTokenType = 2;
pub const GUMBO_TOKEN_COMMENT: GumboTokenType = 3;
pub const GUMBO_TOKEN_WHITESPACE: GumboTokenType = 4;
pub const GUMBO_TOKEN_CHARACTER: GumboTokenType = 5;
pub const GUMBO_TOKEN_CDATA: GumboTokenType = 6;
pub const GUMBO_TOKEN_NULL: GumboTokenType = 7;
pub const GUMBO_TOKEN_EOF: GumboTokenType = 8;

pub type GumboTokenizerEnum = u32;
pub const GUMBO_LEX_DATA: GumboTokenizerEnum = 0;
pub const GUMBO_LEX_CHAR_REF_IN_DATA: GumboTokenizerEnum = 1;
pub const GUMBO_LEX_RCDATA: GumboTokenizerEnum = 2;
pub const GUMBO_LEX_CHAR_REF_IN_RCDATA: GumboTokenizerEnum = 3;
pub const GUMBO_LEX_RAWTEXT: GumboTokenizerEnum = 4;
pub const GUMBO_LEX_SCRIPT: GumboTokenizerEnum = 5;
pub const GUMBO_LEX_PLAINTEXT: GumboTokenizerEnum = 6;
pub const GUMBO_LEX_TAG_OPEN: GumboTokenizerEnum = 7;
pub const GUMBO_LEX_END_TAG_OPEN: GumboTokenizerEnum = 8;
pub const GUMBO_LEX_TAG_NAME: GumboTokenizerEnum = 9;
pub const GUMBO_LEX_RCDATA_LT: GumboTokenizerEnum = 10;
pub const GUMBO_LEX_RCDATA_END_TAG_OPEN: GumboTokenizerEnum = 11;
pub const GUMBO_LEX_RCDATA_END_TAG_NAME: GumboTokenizerEnum = 12;
pub const GUMBO_LEX_RAWTEXT_LT: GumboTokenizerEnum = 13;
pub const GUMBO_LEX_RAWTEXT_END_TAG_OPEN: GumboTokenizerEnum = 14;
pub const GUMBO_LEX_RAWTEXT_END_TAG_NAME: GumboTokenizerEnum = 15;
pub const GUMBO_LEX_SCRIPT_LT: GumboTokenizerEnum = 16;
pub const GUMBO_LEX_SCRIPT_END_TAG_OPEN: GumboTokenizerEnum = 17;
pub const GUMBO_LEX_SCRIPT_END_TAG_NAME: GumboTokenizerEnum = 18;
pub const GUMBO_LEX_SCRIPT_ESCAPED_START: GumboTokenizerEnum = 19;
pub const GUMBO_LEX_SCRIPT_ESCAPED_START_DASH: GumboTokenizerEnum = 20;
pub const GUMBO_LEX_SCRIPT_ESCAPED: GumboTokenizerEnum = 21;
pub const GUMBO_LEX_SCRIPT_ESCAPED_DASH: GumboTokenizerEnum = 22;
pub const GUMBO_LEX_SCRIPT_ESCAPED_DASH_DASH: GumboTokenizerEnum = 23;
pub const GUMBO_LEX_SCRIPT_ESCAPED_LT: GumboTokenizerEnum = 24;
pub const GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_OPEN: GumboTokenizerEnum = 25;
pub const GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_NAME: GumboTokenizerEnum = 26;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_START: GumboTokenizerEnum = 27;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED: GumboTokenizerEnum = 28;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH: GumboTokenizerEnum = 29;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH_DASH: GumboTokenizerEnum = 30;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_LT: GumboTokenizerEnum = 31;
pub const GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_END: GumboTokenizerEnum = 32;
pub const GUMBO_LEX_BEFORE_ATTR_NAME: GumboTokenizerEnum = 33;
pub const GUMBO_LEX_ATTR_NAME: GumboTokenizerEnum = 34;
pub const GUMBO_LEX_AFTER_ATTR_NAME: GumboTokenizerEnum = 35;
pub const GUMBO_LEX_BEFORE_ATTR_VALUE: GumboTokenizerEnum = 36;
pub const GUMBO_LEX_ATTR_VALUE_DOUBLE_QUOTED: GumboTokenizerEnum = 37;
pub const GUMBO_LEX_ATTR_VALUE_SINGLE_QUOTED: GumboTokenizerEnum = 38;
pub const GUMBO_LEX_ATTR_VALUE_UNQUOTED: GumboTokenizerEnum = 39;
pub const GUMBO_LEX_CHAR_REF_IN_ATTR_VALUE: GumboTokenizerEnum = 40;
pub const GUMBO_LEX_AFTER_ATTR_VALUE_QUOTED: GumboTokenizerEnum = 41;
pub const GUMBO_LEX_SELF_CLOSING_START_TAG: GumboTokenizerEnum = 42;
pub const GUMBO_LEX_BOGUS_COMMENT: GumboTokenizerEnum = 43;
pub const GUMBO_LEX_MARKUP_DECLARATION: GumboTokenizerEnum = 44;
pub const GUMBO_LEX_COMMENT_START: GumboTokenizerEnum = 45;
pub const GUMBO_LEX_COMMENT_START_DASH: GumboTokenizerEnum = 46;
pub const GUMBO_LEX_COMMENT: GumboTokenizerEnum = 47;
pub const GUMBO_LEX_COMMENT_END_DASH: GumboTokenizerEnum = 48;
pub const GUMBO_LEX_COMMENT_END: GumboTokenizerEnum = 49;
pub const GUMBO_LEX_COMMENT_END_BANG: GumboTokenizerEnum = 50;
pub const GUMBO_LEX_DOCTYPE: GumboTokenizerEnum = 51;
pub const GUMBO_LEX_BEFORE_DOCTYPE_NAME: GumboTokenizerEnum = 52;
pub const GUMBO_LEX_DOCTYPE_NAME: GumboTokenizerEnum = 53;
pub const GUMBO_LEX_AFTER_DOCTYPE_NAME: GumboTokenizerEnum = 54;
pub const GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_KEYWORD: GumboTokenizerEnum = 55;
pub const GUMBO_LEX_BEFORE_DOCTYPE_PUBLIC_ID: GumboTokenizerEnum = 56;
pub const GUMBO_LEX_DOCTYPE_PUBLIC_ID_DOUBLE_QUOTED: GumboTokenizerEnum = 57;
pub const GUMBO_LEX_DOCTYPE_PUBLIC_ID_SINGLE_QUOTED: GumboTokenizerEnum = 58;
pub const GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_ID: GumboTokenizerEnum = 59;
pub const GUMBO_LEX_BETWEEN_DOCTYPE_PUBLIC_SYSTEM_ID: GumboTokenizerEnum = 60;
pub const GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_KEYWORD: GumboTokenizerEnum = 61;
pub const GUMBO_LEX_BEFORE_DOCTYPE_SYSTEM_ID: GumboTokenizerEnum = 62;
pub const GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED: GumboTokenizerEnum = 63;
pub const GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED: GumboTokenizerEnum = 64;
pub const GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_ID: GumboTokenizerEnum = 65;
pub const GUMBO_LEX_BOGUS_DOCTYPE: GumboTokenizerEnum = 66;
pub const GUMBO_LEX_CDATA: GumboTokenizerEnum = 67;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboTokenDocType {
    pub name: Option<Vec<u8>>,
    pub public_identifier: Option<Vec<u8>>,
    pub system_identifier: Option<Vec<u8>>,
    pub force_quirks: bool,
    pub has_public_identifier: bool,
    pub has_system_identifier: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GumboTokenStartTag {
    pub tag: GumboTag,
    pub attributes: Vec<GumboAttribute>,
    pub is_self_closing: bool,
}

/// The token's union.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GumboTokenData {
    DocType(GumboTokenDocType),
    StartTag(GumboTokenStartTag),
    EndTag(GumboTag),
    /// A comment's text.
    Text(Vec<u8>),
    /// A character, whitespace, NULL or EOF token's code point.
    Character(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GumboToken {
    pub ty: GumboTokenType,
    pub position: GumboSourcePosition,
    pub original_text: GumboStringPiece,
    pub v: GumboTokenData,
}

impl Default for GumboToken {
    fn default() -> GumboToken {
        GumboToken {
            ty: GUMBO_TOKEN_EOF,
            position: GumboSourcePosition::default(),
            original_text: GumboStringPiece::default(),
            v: GumboTokenData::Character(-1),
        }
    }
}

impl GumboToken {
    pub fn character(&self) -> i32 {
        match self.v {
            GumboTokenData::Character(c) => c,
            _ => 0,
        }
    }

    pub fn start_tag(&self) -> &GumboTokenStartTag {
        match &self.v {
            GumboTokenData::StartTag(t) => t,
            _ => panic!("a token of type {} read as a start tag", self.ty),
        }
    }

    pub fn start_tag_mut(&mut self) -> &mut GumboTokenStartTag {
        match &mut self.v {
            GumboTokenData::StartTag(t) => t,
            _ => panic!("a token read as a start tag"),
        }
    }

    pub fn end_tag(&self) -> GumboTag {
        match self.v {
            GumboTokenData::EndTag(t) => t,
            _ => panic!("a token of type {} read as an end tag", self.ty),
        }
    }

    pub fn doc_type(&self) -> &GumboTokenDocType {
        match &self.v {
            GumboTokenData::DocType(d) => d,
            _ => panic!("a token of type {} read as a doctype", self.ty),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
enum StateResult {
    RETURN_ERROR,
    RETURN_SUCCESS,
    NEXT_CHAR,
}

/// A `bool` a state function returns as its result: `true` is RETURN_SUCCESS, `false` RETURN_ERROR.
fn from_bool(b: bool) -> StateResult {
    if b { StateResult::RETURN_SUCCESS } else { StateResult::RETURN_ERROR }
}

use StateResult::*;

#[derive(Clone, Debug, Default)]
pub struct GumboTagState {
    pub buffer: Vec<u8>,
    pub original_text: usize,
    pub tag: GumboTag,
    pub start_pos: GumboSourcePosition,
    pub attributes: Vec<GumboAttribute>,
    pub drop_next_attr_value: bool,
    pub attr_value_state: GumboTokenizerEnum,
    pub last_start_tag: GumboTag,
    pub is_start_tag: bool,
    pub is_self_closing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GumboTokenizerState {
    pub state: GumboTokenizerEnum,
    pub reconsume_current_input: bool,
    pub is_current_node_foreign: bool,
    pub is_in_cdata: bool,
    pub buffered_emit_char: i32,
    pub temporary_buffer: Vec<u8>,
    /// `_temporary_buffer_emit`: the index of the next byte of the temporary buffer to emit, or NULL.
    pub temporary_buffer_emit: Option<usize>,
    pub script_data_buffer: Vec<u8>,
    pub token_start: usize,
    pub token_start_pos: GumboSourcePosition,
    pub tag_state: GumboTagState,
    pub doc_type_state: GumboTokenDocType,
    pub input: super::utf8::Utf8Iterator,
}

const NUL: i32 = 0;
const EOF: i32 = -1;
const TAB: i32 = 0x09;
const LF: i32 = 0x0a;
const FF: i32 = 0x0c;
const CR: i32 = 0x0d;
const SPACE: i32 = 0x20;
const BANG: i32 = b'!' as i32;
const DQUOTE: i32 = b'"' as i32;
const AMP: i32 = b'&' as i32;
const SQUOTE: i32 = b'\'' as i32;
const DASH: i32 = b'-' as i32;
const SLASH: i32 = b'/' as i32;
const LT: i32 = b'<' as i32;
const EQ: i32 = b'=' as i32;
const GT: i32 = b'>' as i32;
const QUESTION: i32 = b'?' as i32;
const BACKTICK: i32 = b'`' as i32;

fn is_alpha(c: i32) -> bool {
    (i32::from(b'A')..=i32::from(b'Z')).contains(&c) || (i32::from(b'a')..=i32::from(b'z')).contains(&c)
}

fn ensure_lowercase(c: i32) -> i32 {
    if (i32::from(b'A')..=i32::from(b'Z')).contains(&c) { c + 0x20 } else { c }
}

fn get_char_token_type(is_in_cdata: bool, c: i32) -> GumboTokenType {
    if is_in_cdata && c > 0 {
        return GUMBO_TOKEN_CDATA;
    }
    match c {
        TAB | LF | CR | FF | SPACE => GUMBO_TOKEN_WHITESPACE,
        NUL => GUMBO_TOKEN_NULL,
        EOF => GUMBO_TOKEN_EOF,
        _ => GUMBO_TOKEN_CHARACTER,
    }
}

/// `gumbo_string_buffer_append_codepoint`: UTF-8 of `c`, as the C writes it for any `int` (no check of the range).
pub fn append_codepoint(c: i32, output: &mut Vec<u8>) {
    let (num_bytes, prefix): (i32, i32) = if c <= 0x7f {
        (0, 0)
    } else if c <= 0x7ff {
        (1, 0xc0)
    } else if c <= 0xffff {
        (2, 0xe0)
    } else {
        (3, 0xf0)
    };
    output.push((prefix | (c >> (num_bytes * 6))) as u8);
    let mut i = num_bytes - 1;
    while i >= 0 {
        output.push((0x80 | (0x3f & (c >> (i * 6)))) as u8);
        i -= 1;
    }
}

fn error_state(state: GumboTokenizerEnum) -> GumboTokenizerErrorState {
    match state {
        GUMBO_LEX_DATA => GUMBO_ERR_TOKENIZER_DATA,
        GUMBO_LEX_CHAR_REF_IN_DATA | GUMBO_LEX_CHAR_REF_IN_RCDATA | GUMBO_LEX_CHAR_REF_IN_ATTR_VALUE => GUMBO_ERR_TOKENIZER_CHAR_REF,
        GUMBO_LEX_RCDATA | GUMBO_LEX_RCDATA_LT | GUMBO_LEX_RCDATA_END_TAG_OPEN | GUMBO_LEX_RCDATA_END_TAG_NAME => {
            GUMBO_ERR_TOKENIZER_RCDATA
        }
        GUMBO_LEX_RAWTEXT | GUMBO_LEX_RAWTEXT_LT | GUMBO_LEX_RAWTEXT_END_TAG_OPEN | GUMBO_LEX_RAWTEXT_END_TAG_NAME => {
            GUMBO_ERR_TOKENIZER_RAWTEXT
        }
        GUMBO_LEX_PLAINTEXT => GUMBO_ERR_TOKENIZER_PLAINTEXT,
        GUMBO_LEX_SCRIPT | GUMBO_LEX_SCRIPT_LT..=GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_END => GUMBO_ERR_TOKENIZER_SCRIPT,
        GUMBO_LEX_TAG_OPEN | GUMBO_LEX_END_TAG_OPEN | GUMBO_LEX_TAG_NAME | GUMBO_LEX_BEFORE_ATTR_NAME => GUMBO_ERR_TOKENIZER_TAG,
        GUMBO_LEX_SELF_CLOSING_START_TAG => GUMBO_ERR_TOKENIZER_SELF_CLOSING_TAG,
        GUMBO_LEX_ATTR_NAME | GUMBO_LEX_AFTER_ATTR_NAME | GUMBO_LEX_BEFORE_ATTR_VALUE => GUMBO_ERR_TOKENIZER_ATTR_NAME,
        GUMBO_LEX_ATTR_VALUE_DOUBLE_QUOTED
        | GUMBO_LEX_ATTR_VALUE_SINGLE_QUOTED
        | GUMBO_LEX_ATTR_VALUE_UNQUOTED
        | GUMBO_LEX_AFTER_ATTR_VALUE_QUOTED => GUMBO_ERR_TOKENIZER_ATTR_VALUE,
        GUMBO_LEX_BOGUS_COMMENT | GUMBO_LEX_COMMENT_START..=GUMBO_LEX_COMMENT_END_BANG => GUMBO_ERR_TOKENIZER_COMMENT,
        GUMBO_LEX_MARKUP_DECLARATION => GUMBO_ERR_TOKENIZER_DOCTYPE,
        GUMBO_LEX_DOCTYPE..=GUMBO_LEX_BOGUS_DOCTYPE => GUMBO_ERR_TOKENIZER_DOCTYPE,
        GUMBO_LEX_CDATA => GUMBO_ERR_TOKENIZER_CDATA,
        // The C switch has every state; an unknown one leaves the field as the allocator gave it.
        _ => 0,
    }
}

impl Parser<'_> {
    fn ctx(&mut self) -> Ctx<'_, '_> {
        Ctx { input: self.text, options: &self.options, errors: &mut self.errors }
    }

    fn input_next(&mut self) {
        self.tokenizer.input.next(self.text, &self.options, &mut self.errors);
    }

    fn input_current(&self) -> i32 {
        self.tokenizer.input.current()
    }

    fn input_reset(&mut self) {
        self.tokenizer.input.reset(self.text, &mut self.errors);
    }

    fn consume_match(&mut self, prefix: &[u8], case_sensitive: bool) -> bool {
        self.tokenizer.input.maybe_consume_match(self.text, &self.options, &mut self.errors, prefix, case_sensitive)
    }

    fn tokenizer_add_parse_error(&mut self, ty: GumboErrorType) {
        let input = &self.tokenizer.input;
        let error = GumboError {
            ty,
            position: input.get_position(),
            original_text: Some(input.get_char_pointer()),
            v: GumboErrorValue::Tokenizer { codepoint: input.current(), state: error_state(self.tokenizer.state) },
        };
        self.errors.add(error);
    }

    fn clear_temporary_buffer(&mut self) {
        self.tokenizer.input.mark();
        self.tokenizer.temporary_buffer.clear();
        self.tokenizer.script_data_buffer.clear();
    }

    fn append_char_to_temporary_buffer(&mut self, codepoint: i32) {
        append_codepoint(codepoint, &mut self.tokenizer.temporary_buffer);
    }

    fn doc_type_state_init(&mut self) {
        self.tokenizer.doc_type_state = GumboTokenDocType::default();
    }

    fn reset_token_start_point(&mut self) {
        self.tokenizer.token_start = self.tokenizer.input.get_char_pointer();
        self.tokenizer.token_start_pos = self.tokenizer.input.get_position();
    }

    fn reset_tag_buffer_start_point(&mut self) {
        self.tokenizer.tag_state.start_pos = self.tokenizer.input.get_position();
        self.tokenizer.tag_state.original_text = self.tokenizer.input.get_char_pointer();
    }

    fn finish_temporary_buffer(&mut self) -> Vec<u8> {
        let out = self.tokenizer.temporary_buffer.clone();
        self.clear_temporary_buffer();
        out
    }

    fn finish_token(&mut self, token: &mut GumboToken) {
        if !self.tokenizer.reconsume_current_input {
            self.input_next();
        }
        token.position = self.tokenizer.token_start_pos;
        let data = self.tokenizer.token_start;
        token.original_text.data = Some(data);
        self.reset_token_start_point();
        token.original_text.length = self.tokenizer.token_start.wrapping_sub(data);
        if token.original_text.length > 0 && self.text.get(data.wrapping_add(token.original_text.length - 1)) == Some(&b'\r') {
            token.original_text.length -= 1;
        }
    }

    fn finish_doctype_public_id(&mut self) {
        let id = self.finish_temporary_buffer();
        self.tokenizer.doc_type_state.public_identifier = Some(id);
        self.tokenizer.doc_type_state.has_public_identifier = true;
    }

    fn finish_doctype_system_id(&mut self) {
        let id = self.finish_temporary_buffer();
        self.tokenizer.doc_type_state.system_identifier = Some(id);
        self.tokenizer.doc_type_state.has_system_identifier = true;
    }

    fn emit_char(&mut self, c: i32, output: &mut GumboToken) {
        output.ty = get_char_token_type(self.tokenizer.is_in_cdata, c);
        output.v = GumboTokenData::Character(c);
        self.finish_token(output);
    }

    fn emit_replacement_char(&mut self, output: &mut GumboToken) -> StateResult {
        self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
        self.emit_char(kUtf8ReplacementChar, output);
        RETURN_ERROR
    }

    fn emit_eof(&mut self, output: &mut GumboToken) -> StateResult {
        self.emit_char(-1, output);
        RETURN_SUCCESS
    }

    fn emit_current_char(&mut self, output: &mut GumboToken) -> bool {
        let c = self.input_current();
        self.emit_char(c, output);
        true
    }

    fn emit_doctype(&mut self, output: &mut GumboToken) {
        output.ty = GUMBO_TOKEN_DOCTYPE;
        output.v = GumboTokenData::DocType(std::mem::take(&mut self.tokenizer.doc_type_state));
        self.finish_token(output);
        self.doc_type_state_init();
    }

    fn emit_current_tag(&mut self, output: &mut GumboToken) -> StateResult {
        let tag_state = &mut self.tokenizer.tag_state;
        if tag_state.is_start_tag {
            output.ty = GUMBO_TOKEN_START_TAG;
            output.v = GumboTokenData::StartTag(GumboTokenStartTag {
                tag: tag_state.tag,
                attributes: std::mem::take(&mut tag_state.attributes),
                is_self_closing: tag_state.is_self_closing,
            });
            tag_state.last_start_tag = tag_state.tag;
        } else {
            output.ty = GUMBO_TOKEN_END_TAG;
            output.v = GumboTokenData::EndTag(tag_state.tag);
            tag_state.attributes.clear();
        }
        tag_state.buffer.clear();
        self.finish_token(output);
        RETURN_SUCCESS
    }

    fn abandon_current_tag(&mut self) {
        let tag_state = &mut self.tokenizer.tag_state;
        tag_state.attributes.clear();
        tag_state.buffer.clear();
    }

    fn emit_char_ref(&mut self, additional_allowed_char: i32, output: &mut GumboToken) -> StateResult {
        let mut char_ref = OneOrTwoCodepoints { first: kGumboNoChar, second: kGumboNoChar };
        let mut input = std::mem::take(&mut self.tokenizer.input);
        let status = consume_char_ref(&mut self.ctx(), &mut input, additional_allowed_char, false, &mut char_ref);
        self.tokenizer.input = input;
        if char_ref.first != kGumboNoChar {
            self.tokenizer.reconsume_current_input = true;
            self.emit_char(char_ref.first, output);
            self.tokenizer.buffered_emit_char = char_ref.second;
        } else {
            self.emit_char(i32::from(b'&'), output);
        }
        if status { RETURN_SUCCESS } else { RETURN_ERROR }
    }

    fn emit_comment(&mut self, output: &mut GumboToken) -> StateResult {
        output.ty = GUMBO_TOKEN_COMMENT;
        output.v = GumboTokenData::Text(self.finish_temporary_buffer());
        self.finish_token(output);
        RETURN_SUCCESS
    }

    fn maybe_emit_from_temporary_buffer(&mut self, output: &mut GumboToken) -> bool {
        let Some(c) = self.tokenizer.temporary_buffer_emit.filter(|c| *c < self.tokenizer.temporary_buffer.len()) else {
            self.tokenizer.temporary_buffer_emit = None;
            return false;
        };
        let saved_reconsume_state = self.tokenizer.reconsume_current_input;
        self.tokenizer.reconsume_current_input = false;
        // `*c` is a `char`, signed where the helper runs.
        let byte = i32::from(self.tokenizer.temporary_buffer[c] as i8);
        self.emit_char(byte, output);
        self.tokenizer.temporary_buffer_emit = Some(c + 1);
        self.tokenizer.reconsume_current_input = saved_reconsume_state;
        true
    }

    fn emit_temporary_buffer(&mut self, output: &mut GumboToken) -> bool {
        self.input_reset();
        self.tokenizer.temporary_buffer_emit = Some(0);
        self.maybe_emit_from_temporary_buffer(output)
    }

    fn append_char_to_tag_buffer(&mut self, codepoint: i32, reinitilize_position_on_first: bool) {
        if self.tokenizer.tag_state.buffer.is_empty() && reinitilize_position_on_first {
            self.reset_tag_buffer_start_point();
        }
        append_codepoint(codepoint, &mut self.tokenizer.tag_state.buffer);
    }

    fn initialize_tag_buffer(&mut self) {
        self.tokenizer.tag_state.buffer.clear();
        self.reset_tag_buffer_start_point();
    }

    fn start_new_tag(&mut self, is_start_tag: bool) {
        let c = ensure_lowercase(self.input_current());
        self.initialize_tag_buffer();
        append_codepoint(c, &mut self.tokenizer.tag_state.buffer);
        let tag_state = &mut self.tokenizer.tag_state;
        tag_state.attributes = Vec::new();
        tag_state.drop_next_attr_value = false;
        tag_state.is_start_tag = is_start_tag;
        tag_state.is_self_closing = false;
    }

    fn copy_over_original_tag_text(&self) -> (GumboStringPiece, GumboSourcePosition, GumboSourcePosition) {
        let tag_state = &self.tokenizer.tag_state;
        let data = tag_state.original_text;
        let mut length = self.tokenizer.input.get_char_pointer().wrapping_sub(data);
        if length > 0 && self.text.get(data.wrapping_add(length - 1)) == Some(&b'\r') {
            length -= 1;
        }
        (GumboStringPiece { data: Some(data), length }, tag_state.start_pos, self.tokenizer.input.get_position())
    }

    fn reinitialize_tag_buffer(&mut self) {
        self.initialize_tag_buffer();
    }

    fn finish_tag_name(&mut self) {
        self.tokenizer.tag_state.tag = gumbo_tagn_enum(&self.tokenizer.tag_state.buffer);
        self.reinitialize_tag_buffer();
    }

    /// `add_duplicate_attr_error`: with no room for an error the C returns before it takes the name and
    /// reinitialises the tag buffer.
    fn add_duplicate_attr_error(&mut self, original_index: u32, new_index: u32) {
        if !self.errors.room() {
            return;
        }
        let tag_state = &self.tokenizer.tag_state;
        self.errors.add(GumboError {
            ty: GUMBO_ERR_DUPLICATE_ATTR,
            position: tag_state.start_pos,
            original_text: Some(tag_state.original_text),
            v: GumboErrorValue::DuplicateAttr { name: tag_state.buffer.clone(), original_index, new_index },
        });
        self.reinitialize_tag_buffer();
    }

    fn finish_attribute_name(&mut self) -> bool {
        self.tokenizer.tag_state.drop_next_attr_value = false;
        let tag_state = &self.tokenizer.tag_state;
        for (i, attr) in tag_state.attributes.iter().enumerate() {
            // strlen of the name, then memcmp.
            if super::c_str(&attr.name).len() == tag_state.buffer.len()
                && attr.name.get(..tag_state.buffer.len()) == Some(&tag_state.buffer[..])
            {
                let count = tag_state.attributes.len() as u32;
                self.add_duplicate_attr_error(i as u32, count);
                self.tokenizer.tag_state.drop_next_attr_value = true;
                return false;
            }
        }
        let (original_name, name_start, name_end) = self.copy_over_original_tag_text();
        let attr = GumboAttribute {
            attr_namespace: GUMBO_ATTR_NAMESPACE_NONE,
            name: self.tokenizer.tag_state.buffer.clone(),
            original_name,
            value: Vec::new(),
            original_value: original_name,
            name_start,
            name_end,
            value_start: GumboSourcePosition::default(),
            value_end: GumboSourcePosition::default(),
        };
        self.tokenizer.tag_state.attributes.push(attr);
        self.reinitialize_tag_buffer();
        true
    }

    fn finish_attribute_value(&mut self) {
        if self.tokenizer.tag_state.drop_next_attr_value {
            self.tokenizer.tag_state.drop_next_attr_value = false;
            self.reinitialize_tag_buffer();
            return;
        }
        let (original_value, value_start, value_end) = self.copy_over_original_tag_text();
        let tag_state = &mut self.tokenizer.tag_state;
        let value = tag_state.buffer.clone();
        // The C writes through attributes.data[length - 1] unchecked; a tag gets here only after an attribute.
        if let Some(attr) = tag_state.attributes.last_mut() {
            attr.value = value;
            attr.original_value = original_value;
            attr.value_start = value_start;
            attr.value_end = value_end;
        }
        self.reinitialize_tag_buffer();
    }

    fn is_appropriate_end_tag(&self) -> bool {
        let tag_state = &self.tokenizer.tag_state;
        tag_state.last_start_tag != GUMBO_TAG_LAST && tag_state.last_start_tag == gumbo_tagn_enum(&tag_state.buffer)
    }

    pub(crate) fn gumbo_tokenizer_state_init(&mut self) {
        let mut errors = std::mem::take(&mut self.errors);
        self.tokenizer = GumboTokenizerState {
            state: GUMBO_LEX_DATA,
            buffered_emit_char: kGumboNoChar,
            token_start: 0,
            input: super::utf8::Utf8Iterator::init(self.text, &mut errors),
            ..GumboTokenizerState::default()
        };
        self.errors = errors;
        self.tokenizer.tag_state.last_start_tag = GUMBO_TAG_LAST;
        self.tokenizer.token_start_pos = self.tokenizer.input.get_position();
        self.doc_type_state_init();
    }

    pub(crate) fn gumbo_tokenizer_set_state(&mut self, state: GumboTokenizerEnum) {
        self.tokenizer.state = state;
    }

    pub(crate) fn gumbo_tokenizer_set_is_current_node_foreign(&mut self, is_foreign: bool) {
        self.tokenizer.is_current_node_foreign = is_foreign;
    }

    fn handle_data_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            AMP => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_CHAR_REF_IN_DATA);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_TAG_OPEN);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(LT);
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.emit_char(c, output);
                RETURN_ERROR
            }
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_char_ref_in_data_state(&mut self, output: &mut GumboToken) -> StateResult {
        self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
        self.emit_char_ref(SPACE, output)
    }

    fn handle_rcdata_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            AMP => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_CHAR_REF_IN_RCDATA);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_RCDATA_LT);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(LT);
                NEXT_CHAR
            }
            NUL => self.emit_replacement_char(output),
            EOF => self.emit_eof(output),
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_char_ref_in_rcdata_state(&mut self, output: &mut GumboToken) -> StateResult {
        self.gumbo_tokenizer_set_state(GUMBO_LEX_RCDATA);
        self.emit_char_ref(SPACE, output)
    }

    /// RAWTEXT and script data: the same but for the state after `<`.
    fn handle_rawtext_like_state(&mut self, c: i32, output: &mut GumboToken, lt_state: GumboTokenizerEnum) -> StateResult {
        match c {
            LT => {
                self.gumbo_tokenizer_set_state(lt_state);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(LT);
                NEXT_CHAR
            }
            NUL => self.emit_replacement_char(output),
            EOF => self.emit_eof(output),
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_plaintext_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            NUL => self.emit_replacement_char(output),
            EOF => self.emit_eof(output),
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_tag_open_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            BANG => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_MARKUP_DECLARATION);
                self.clear_temporary_buffer();
                NEXT_CHAR
            }
            SLASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_END_TAG_OPEN);
                self.append_char_to_temporary_buffer(SLASH);
                NEXT_CHAR
            }
            QUESTION => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_COMMENT);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(QUESTION);
                self.tokenizer_add_parse_error(GUMBO_ERR_TAG_STARTS_WITH_QUESTION);
                NEXT_CHAR
            }
            _ => {
                if is_alpha(c) {
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_TAG_NAME);
                    self.start_new_tag(true);
                    NEXT_CHAR
                } else {
                    self.tokenizer_add_parse_error(GUMBO_ERR_TAG_INVALID);
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                    self.emit_temporary_buffer(output);
                    RETURN_ERROR
                }
            }
        }
    }

    fn handle_end_tag_open_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            GT => {
                self.tokenizer_add_parse_error(GUMBO_ERR_CLOSE_TAG_EMPTY);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_CLOSE_TAG_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                from_bool(self.emit_temporary_buffer(output))
            }
            _ => {
                if is_alpha(c) {
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_TAG_NAME);
                    self.start_new_tag(false);
                } else {
                    self.tokenizer_add_parse_error(GUMBO_ERR_CLOSE_TAG_INVALID);
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_COMMENT);
                    self.clear_temporary_buffer();
                    self.append_char_to_temporary_buffer(c);
                }
                NEXT_CHAR
            }
        }
    }

    fn handle_tag_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.finish_tag_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                NEXT_CHAR
            }
            SLASH => {
                self.finish_tag_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                NEXT_CHAR
            }
            GT => {
                self.finish_tag_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_tag_buffer(kUtf8ReplacementChar, true);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_TAG_EOF);
                self.abandon_current_tag();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => {
                self.append_char_to_tag_buffer(ensure_lowercase(c), true);
                NEXT_CHAR
            }
        }
    }

    /// The RCDATA, RAWTEXT and script data states after `<`.
    fn handle_lt_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        end_tag_open: GumboTokenizerEnum,
        back: GumboTokenizerEnum,
    ) -> StateResult {
        if c == SLASH {
            self.gumbo_tokenizer_set_state(end_tag_open);
            self.append_char_to_temporary_buffer(SLASH);
            NEXT_CHAR
        } else {
            self.gumbo_tokenizer_set_state(back);
            self.tokenizer.reconsume_current_input = true;
            from_bool(self.emit_temporary_buffer(output))
        }
    }

    /// The RCDATA, RAWTEXT, script data and escaped script data states after `</`.
    fn handle_end_tag_open_like_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        name_state: GumboTokenizerEnum,
        back: GumboTokenizerEnum,
    ) -> StateResult {
        if is_alpha(c) {
            self.gumbo_tokenizer_set_state(name_state);
            self.start_new_tag(false);
            self.append_char_to_temporary_buffer(c);
            NEXT_CHAR
        } else {
            self.gumbo_tokenizer_set_state(back);
            from_bool(self.emit_temporary_buffer(output))
        }
    }

    /// The RCDATA, RAWTEXT, script data and escaped script data end tag name states.
    fn handle_end_tag_name_like_state(&mut self, c: i32, output: &mut GumboToken, back: GumboTokenizerEnum) -> StateResult {
        if is_alpha(c) {
            self.append_char_to_tag_buffer(ensure_lowercase(c), true);
            self.append_char_to_temporary_buffer(c);
            return NEXT_CHAR;
        } else if self.is_appropriate_end_tag() {
            match c {
                TAB | LF | FF | SPACE => {
                    self.finish_tag_name();
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                    return NEXT_CHAR;
                }
                SLASH => {
                    self.finish_tag_name();
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                    return NEXT_CHAR;
                }
                GT => {
                    self.finish_tag_name();
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                    return self.emit_current_tag(output);
                }
                _ => {}
            }
        }
        self.gumbo_tokenizer_set_state(back);
        self.abandon_current_tag();
        from_bool(self.emit_temporary_buffer(output))
    }

    fn handle_script_lt_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        if c == SLASH {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_END_TAG_OPEN);
            self.append_char_to_temporary_buffer(SLASH);
            NEXT_CHAR
        } else if c == BANG {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_START);
            self.append_char_to_temporary_buffer(BANG);
            from_bool(self.emit_temporary_buffer(output))
        } else {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT);
            self.tokenizer.reconsume_current_input = true;
            from_bool(self.emit_temporary_buffer(output))
        }
    }

    fn handle_script_escaped_start_state(&mut self, c: i32, output: &mut GumboToken, dash_state: GumboTokenizerEnum) -> StateResult {
        if c == DASH {
            self.gumbo_tokenizer_set_state(dash_state);
            from_bool(self.emit_current_char(output))
        } else {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT);
            self.tokenizer.reconsume_current_input = true;
            NEXT_CHAR
        }
    }

    fn handle_script_escaped_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_DASH);
                from_bool(self.emit_current_char(output))
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_LT);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
            NUL => self.emit_replacement_char(output),
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.emit_eof(output)
            }
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_script_escaped_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_DASH_DASH);
                from_bool(self.emit_current_char(output))
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_LT);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
            NUL => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED);
                self.emit_replacement_char(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED);
                from_bool(self.emit_current_char(output))
            }
        }
    }

    fn handle_script_escaped_dash_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => from_bool(self.emit_current_char(output)),
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_LT);
                self.clear_temporary_buffer();
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT);
                from_bool(self.emit_current_char(output))
            }
            NUL => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED);
                self.emit_replacement_char(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED);
                from_bool(self.emit_current_char(output))
            }
        }
    }

    fn handle_script_escaped_lt_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        if c == SLASH {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_OPEN);
            self.append_char_to_temporary_buffer(c);
            NEXT_CHAR
        } else if is_alpha(c) {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_START);
            self.append_char_to_temporary_buffer(c);
            append_codepoint(ensure_lowercase(c), &mut self.tokenizer.script_data_buffer);
            from_bool(self.emit_temporary_buffer(output))
        } else {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_ESCAPED);
            from_bool(self.emit_temporary_buffer(output))
        }
    }

    /// The double escaped start and end states: which state `script` in the script data buffer leads to.
    fn handle_script_double_escape_edge_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        if_script: GumboTokenizerEnum,
        otherwise: GumboTokenizerEnum,
    ) -> StateResult {
        match c {
            TAB | LF | FF | SPACE | SLASH | GT => {
                let state = if self.tokenizer.script_data_buffer == b"script" { if_script } else { otherwise };
                self.gumbo_tokenizer_set_state(state);
                from_bool(self.emit_current_char(output))
            }
            _ => {
                if is_alpha(c) {
                    append_codepoint(ensure_lowercase(c), &mut self.tokenizer.script_data_buffer);
                    from_bool(self.emit_current_char(output))
                } else {
                    self.gumbo_tokenizer_set_state(otherwise);
                    self.tokenizer.reconsume_current_input = true;
                    NEXT_CHAR
                }
            }
        }
    }

    fn handle_script_double_escaped_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH);
                from_bool(self.emit_current_char(output))
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_LT);
                from_bool(self.emit_current_char(output))
            }
            NUL => self.emit_replacement_char(output),
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => from_bool(self.emit_current_char(output)),
        }
    }

    fn handle_script_double_escaped_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH_DASH);
                from_bool(self.emit_current_char(output))
            }
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_LT);
                from_bool(self.emit_current_char(output))
            }
            NUL => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED);
                self.emit_replacement_char(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED);
                from_bool(self.emit_current_char(output))
            }
        }
    }

    fn handle_script_double_escaped_dash_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => from_bool(self.emit_current_char(output)),
            LT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_LT);
                from_bool(self.emit_current_char(output))
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT);
                from_bool(self.emit_current_char(output))
            }
            NUL => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED);
                self.emit_replacement_char(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SCRIPT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                NEXT_CHAR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED);
                from_bool(self.emit_current_char(output))
            }
        }
    }

    fn handle_script_double_escaped_lt_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        if c == SLASH {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_END);
            self.tokenizer.script_data_buffer.clear();
            from_bool(self.emit_current_char(output))
        } else {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED);
            self.tokenizer.reconsume_current_input = true;
            NEXT_CHAR
        }
    }

    fn handle_before_attr_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            SLASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_NAME);
                // The C appends the replacement to the temporary buffer, not to the name.
                self.append_char_to_temporary_buffer(0xfffd);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                NEXT_CHAR
            }
            _ => {
                if matches!(c, DQUOTE | SQUOTE | LT | EQ) {
                    self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_INVALID);
                }
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_NAME);
                self.append_char_to_tag_buffer(ensure_lowercase(c), true);
                NEXT_CHAR
            }
        }
    }

    fn handle_attr_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.finish_attribute_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_AFTER_ATTR_NAME);
                NEXT_CHAR
            }
            SLASH => {
                self.finish_attribute_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                NEXT_CHAR
            }
            EQ => {
                self.finish_attribute_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_VALUE);
                NEXT_CHAR
            }
            GT => {
                self.finish_attribute_name();
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_tag_buffer(kUtf8ReplacementChar, true);
                NEXT_CHAR
            }
            EOF => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_EOF);
                NEXT_CHAR
            }
            _ => {
                if matches!(c, DQUOTE | SQUOTE | LT) {
                    self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_INVALID);
                }
                self.append_char_to_tag_buffer(ensure_lowercase(c), true);
                NEXT_CHAR
            }
        }
    }

    fn handle_after_attr_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            SLASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                NEXT_CHAR
            }
            EQ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_VALUE);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_NAME);
                self.append_char_to_temporary_buffer(0xfffd);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                NEXT_CHAR
            }
            _ => {
                if matches!(c, DQUOTE | SQUOTE | LT) {
                    self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_NAME_INVALID);
                }
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_NAME);
                self.append_char_to_tag_buffer(ensure_lowercase(c), true);
                NEXT_CHAR
            }
        }
    }

    fn handle_before_attr_value_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            DQUOTE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_VALUE_DOUBLE_QUOTED);
                self.reset_tag_buffer_start_point();
                NEXT_CHAR
            }
            AMP => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_VALUE_UNQUOTED);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            SQUOTE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_VALUE_SINGLE_QUOTED);
                self.reset_tag_buffer_start_point();
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_VALUE_UNQUOTED);
                self.append_char_to_tag_buffer(kUtf8ReplacementChar, true);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_UNQUOTED_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            GT => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_UNQUOTED_RIGHT_BRACKET);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output);
                RETURN_ERROR
            }
            _ => {
                if matches!(c, LT | EQ | BACKTICK) {
                    self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_UNQUOTED_EQUALS);
                }
                self.gumbo_tokenizer_set_state(GUMBO_LEX_ATTR_VALUE_UNQUOTED);
                self.append_char_to_tag_buffer(c, true);
                NEXT_CHAR
            }
        }
    }

    fn handle_attr_value_quoted_state(&mut self, c: i32, quote: i32, eof_error: GumboErrorType) -> StateResult {
        match c {
            AMP => {
                self.tokenizer.tag_state.attr_value_state = self.tokenizer.state;
                self.gumbo_tokenizer_set_state(GUMBO_LEX_CHAR_REF_IN_ATTR_VALUE);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_tag_buffer(kUtf8ReplacementChar, false);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(eof_error);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            _ if c == quote => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_AFTER_ATTR_VALUE_QUOTED);
                NEXT_CHAR
            }
            _ => {
                self.append_char_to_tag_buffer(c, false);
                NEXT_CHAR
            }
        }
    }

    fn handle_attr_value_unquoted_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                self.finish_attribute_value();
                NEXT_CHAR
            }
            AMP => {
                self.tokenizer.tag_state.attr_value_state = self.tokenizer.state;
                self.gumbo_tokenizer_set_state(GUMBO_LEX_CHAR_REF_IN_ATTR_VALUE);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.finish_attribute_value();
                self.emit_current_tag(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_tag_buffer(kUtf8ReplacementChar, true);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_UNQUOTED_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.reconsume_current_input = true;
                self.abandon_current_tag();
                NEXT_CHAR
            }
            _ => {
                if matches!(c, LT | EQ | DQUOTE | SQUOTE | BACKTICK) {
                    self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_UNQUOTED_EQUALS);
                }
                self.append_char_to_tag_buffer(c, true);
                NEXT_CHAR
            }
        }
    }

    fn handle_char_ref_in_attr_value_state(&mut self) -> StateResult {
        let mut char_ref = OneOrTwoCodepoints { first: kGumboNoChar, second: kGumboNoChar };
        let mut is_unquoted = false;
        let allowed_char = match self.tokenizer.tag_state.attr_value_state {
            GUMBO_LEX_ATTR_VALUE_DOUBLE_QUOTED => DQUOTE,
            GUMBO_LEX_ATTR_VALUE_SINGLE_QUOTED => SQUOTE,
            GUMBO_LEX_ATTR_VALUE_UNQUOTED => {
                is_unquoted = true;
                GT
            }
            _ => SPACE,
        };
        let mut input = std::mem::take(&mut self.tokenizer.input);
        consume_char_ref(&mut self.ctx(), &mut input, allowed_char, true, &mut char_ref);
        self.tokenizer.input = input;
        if char_ref.first != kGumboNoChar {
            self.tokenizer.reconsume_current_input = true;
            self.append_char_to_tag_buffer(char_ref.first, is_unquoted);
            if char_ref.second != kGumboNoChar {
                self.append_char_to_tag_buffer(char_ref.second, is_unquoted);
            }
        } else {
            self.append_char_to_tag_buffer(AMP, is_unquoted);
        }
        let back = self.tokenizer.tag_state.attr_value_state;
        self.gumbo_tokenizer_set_state(back);
        NEXT_CHAR
    }

    fn handle_after_attr_value_quoted_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        self.finish_attribute_value();
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                NEXT_CHAR
            }
            SLASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_SELF_CLOSING_START_TAG);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_current_tag(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_AFTER_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_ATTR_AFTER_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
        }
    }

    fn handle_self_closing_start_tag_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.tag_state.is_self_closing = true;
                self.emit_current_tag(output)
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SOLIDUS_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.abandon_current_tag();
                NEXT_CHAR
            }
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_SOLIDUS_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_ATTR_NAME);
                self.tokenizer.reconsume_current_input = true;
                NEXT_CHAR
            }
        }
    }

    fn handle_bogus_comment_state(&mut self, mut c: i32, output: &mut GumboToken) -> StateResult {
        while c != GT && c != EOF {
            if c == NUL {
                c = 0xFFFD;
            }
            self.append_char_to_temporary_buffer(c);
            self.input_next();
            c = self.input_current();
        }
        self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
        self.emit_comment(output)
    }

    fn handle_markup_declaration_state(&mut self) -> StateResult {
        if self.consume_match(b"--", true) {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_START);
            self.tokenizer.reconsume_current_input = true;
        } else if self.consume_match(b"DOCTYPE", false) {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE);
            self.tokenizer.reconsume_current_input = true;
            self.tokenizer.doc_type_state.name = Some(Vec::new());
            self.tokenizer.doc_type_state.public_identifier = Some(Vec::new());
            self.tokenizer.doc_type_state.system_identifier = Some(Vec::new());
        } else if self.tokenizer.is_current_node_foreign && self.consume_match(b"[CDATA[", true) {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_CDATA);
            self.tokenizer.is_in_cdata = true;
            self.tokenizer.reconsume_current_input = true;
        } else {
            self.tokenizer_add_parse_error(GUMBO_ERR_DASHES_OR_DOCTYPE);
            self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_COMMENT);
            self.tokenizer.reconsume_current_input = true;
            self.clear_temporary_buffer();
        }
        NEXT_CHAR
    }

    fn handle_comment_start_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_START_DASH);
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            GT => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_comment_start_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_END);
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            GT => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_comment_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_END_DASH);
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_comment_end_dash_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_END);
                NEXT_CHAR
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_comment_end_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            BANG => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_BANG_AFTER_DOUBLE_DASH);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_END_BANG);
                NEXT_CHAR
            }
            DASH => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_DASH_AFTER_DOUBLE_DASH);
                self.append_char_to_temporary_buffer(DASH);
                NEXT_CHAR
            }
            EOF => {
                // The C records GUMBO_ERR_UTF8_NULL for an end of input here.
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_comment_end_bang_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            DASH => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT_END_DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(BANG);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output)
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(BANG);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_COMMENT_END_BANG_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_comment(output);
                RETURN_ERROR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_COMMENT);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(DASH);
                self.append_char_to_temporary_buffer(BANG);
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    /// A doctype that ends here with `force_quirks` set, the error first.
    fn emit_quirky_doctype(&mut self, error: GumboErrorType, output: &mut GumboToken) -> StateResult {
        self.tokenizer_add_parse_error(error);
        self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
        self.tokenizer.doc_type_state.force_quirks = true;
        self.emit_doctype(output);
        RETURN_ERROR
    }

    fn handle_doctype_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_DOCTYPE_NAME);
                NEXT_CHAR
            }
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_SPACE);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BEFORE_DOCTYPE_NAME);
                self.tokenizer.reconsume_current_input = true;
                self.tokenizer.doc_type_state.force_quirks = true;
                NEXT_CHAR
            }
        }
    }

    fn handle_before_doctype_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_NAME);
                self.tokenizer.doc_type_state.force_quirks = true;
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            GT => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_RIGHT_BRACKET, output),
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_NAME);
                self.tokenizer.doc_type_state.force_quirks = false;
                self.append_char_to_temporary_buffer(ensure_lowercase(c));
                NEXT_CHAR
            }
        }
    }

    fn finish_doctype_name(&mut self) {
        let name = self.finish_temporary_buffer();
        self.tokenizer.doc_type_state.name = Some(name);
    }

    fn handle_doctype_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_AFTER_DOCTYPE_NAME);
                self.finish_doctype_name();
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.finish_doctype_name();
                self.emit_doctype(output);
                RETURN_SUCCESS
            }
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.doc_type_state.force_quirks = true;
                self.finish_doctype_name();
                self.emit_doctype(output);
                RETURN_ERROR
            }
            _ => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_NAME);
                self.tokenizer.doc_type_state.force_quirks = false;
                self.append_char_to_temporary_buffer(ensure_lowercase(c));
                NEXT_CHAR
            }
        }
    }

    fn handle_after_doctype_name_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_doctype(output);
                RETURN_SUCCESS
            }
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                if self.consume_match(b"PUBLIC", false) {
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_KEYWORD);
                    self.tokenizer.reconsume_current_input = true;
                } else if self.consume_match(b"SYSTEM", false) {
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_KEYWORD);
                    self.tokenizer.reconsume_current_input = true;
                } else {
                    self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_SPACE_OR_RIGHT_BRACKET);
                    self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                    self.tokenizer.doc_type_state.force_quirks = true;
                }
                NEXT_CHAR
            }
        }
    }

    /// The states after PUBLIC and SYSTEM.
    fn handle_after_doctype_keyword_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        before: GumboTokenizerEnum,
        dquoted: GumboTokenizerEnum,
        squoted: GumboTokenizerEnum,
    ) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(before);
                NEXT_CHAR
            }
            DQUOTE => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(dquoted);
                NEXT_CHAR
            }
            SQUOTE => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(squoted);
                NEXT_CHAR
            }
            GT => {
                let error =
                    if before == GUMBO_LEX_BEFORE_DOCTYPE_PUBLIC_ID { GUMBO_ERR_DOCTYPE_RIGHT_BRACKET } else { GUMBO_ERR_DOCTYPE_END };
                self.emit_quirky_doctype(error, output)
            }
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                self.tokenizer.doc_type_state.force_quirks = true;
                if before == GUMBO_LEX_BEFORE_DOCTYPE_PUBLIC_ID {
                    // After PUBLIC the C emits the doctype here and again at the bogus doctype's end.
                    self.emit_doctype(output);
                    RETURN_ERROR
                } else {
                    NEXT_CHAR
                }
            }
        }
    }

    /// The states before a public and a system identifier.
    fn handle_before_doctype_id_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        dquoted: GumboTokenizerEnum,
        squoted: GumboTokenizerEnum,
    ) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            DQUOTE => {
                self.gumbo_tokenizer_set_state(dquoted);
                NEXT_CHAR
            }
            SQUOTE => {
                self.gumbo_tokenizer_set_state(squoted);
                NEXT_CHAR
            }
            GT => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_END, output),
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                self.tokenizer.doc_type_state.force_quirks = true;
                if dquoted == GUMBO_LEX_DOCTYPE_PUBLIC_ID_DOUBLE_QUOTED {
                    self.emit_doctype(output);
                    RETURN_ERROR
                } else {
                    NEXT_CHAR
                }
            }
        }
    }

    /// The four quoted identifier states.
    fn handle_doctype_id_quoted_state(
        &mut self,
        c: i32,
        output: &mut GumboToken,
        quote: i32,
        public: bool,
        eof_error: GumboErrorType,
    ) -> StateResult {
        let finish = |p: &mut Self| if public { p.finish_doctype_public_id() } else { p.finish_doctype_system_id() };
        match c {
            NUL => {
                self.tokenizer_add_parse_error(GUMBO_ERR_UTF8_NULL);
                self.append_char_to_temporary_buffer(kUtf8ReplacementChar);
                NEXT_CHAR
            }
            GT => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_END);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.doc_type_state.force_quirks = true;
                finish(self);
                self.emit_doctype(output);
                RETURN_ERROR
            }
            EOF => {
                self.tokenizer_add_parse_error(eof_error);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.doc_type_state.force_quirks = true;
                finish(self);
                self.emit_doctype(output);
                RETURN_ERROR
            }
            _ if c == quote => {
                self.gumbo_tokenizer_set_state(if public { GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_ID } else { GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_ID });
                finish(self);
                NEXT_CHAR
            }
            _ => {
                self.append_char_to_temporary_buffer(c);
                NEXT_CHAR
            }
        }
    }

    fn handle_after_doctype_public_id_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BETWEEN_DOCTYPE_PUBLIC_SYSTEM_ID);
                NEXT_CHAR
            }
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_doctype(output);
                RETURN_SUCCESS
            }
            DQUOTE => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED);
                NEXT_CHAR
            }
            SQUOTE => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED);
                NEXT_CHAR
            }
            EOF => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_EOF);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.tokenizer.reconsume_current_input = true;
                self.tokenizer.doc_type_state.force_quirks = true;
                self.emit_doctype(output);
                RETURN_ERROR
            }
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                self.tokenizer.doc_type_state.force_quirks = true;
                NEXT_CHAR
            }
        }
    }

    fn handle_between_doctype_public_system_id_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_doctype(output);
                RETURN_SUCCESS
            }
            DQUOTE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED);
                NEXT_CHAR
            }
            SQUOTE => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED);
                NEXT_CHAR
            }
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                self.tokenizer.doc_type_state.force_quirks = true;
                self.emit_doctype(output);
                RETURN_ERROR
            }
        }
    }

    fn handle_after_doctype_system_id_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        match c {
            TAB | LF | FF | SPACE => NEXT_CHAR,
            GT => {
                self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
                self.emit_doctype(output);
                RETURN_SUCCESS
            }
            EOF => self.emit_quirky_doctype(GUMBO_ERR_DOCTYPE_EOF, output),
            _ => {
                self.tokenizer_add_parse_error(GUMBO_ERR_DOCTYPE_INVALID);
                self.gumbo_tokenizer_set_state(GUMBO_LEX_BOGUS_DOCTYPE);
                NEXT_CHAR
            }
        }
    }

    fn handle_bogus_doctype_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        if c == GT || c == EOF {
            self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
            self.emit_doctype(output);
            return RETURN_ERROR;
        }
        NEXT_CHAR
    }

    fn handle_cdata_state(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        if c == EOF || self.consume_match(b"]]>", true) {
            self.tokenizer.reconsume_current_input = true;
            self.reset_token_start_point();
            self.gumbo_tokenizer_set_state(GUMBO_LEX_DATA);
            self.tokenizer.is_in_cdata = false;
            NEXT_CHAR
        } else {
            from_bool(self.emit_current_char(output))
        }
    }

    fn dispatch(&mut self, c: i32, output: &mut GumboToken) -> StateResult {
        let state = self.tokenizer.state;
        if let Some(n) = self.reached.states.get_mut(state as usize) {
            *n = n.saturating_add(1);
        }
        match state {
            GUMBO_LEX_DATA => self.handle_data_state(c, output),
            GUMBO_LEX_CHAR_REF_IN_DATA => self.handle_char_ref_in_data_state(output),
            GUMBO_LEX_RCDATA => self.handle_rcdata_state(c, output),
            GUMBO_LEX_CHAR_REF_IN_RCDATA => self.handle_char_ref_in_rcdata_state(output),
            GUMBO_LEX_RAWTEXT => self.handle_rawtext_like_state(c, output, GUMBO_LEX_RAWTEXT_LT),
            GUMBO_LEX_SCRIPT => self.handle_rawtext_like_state(c, output, GUMBO_LEX_SCRIPT_LT),
            GUMBO_LEX_PLAINTEXT => self.handle_plaintext_state(c, output),
            GUMBO_LEX_TAG_OPEN => self.handle_tag_open_state(c, output),
            GUMBO_LEX_END_TAG_OPEN => self.handle_end_tag_open_state(c, output),
            GUMBO_LEX_TAG_NAME => self.handle_tag_name_state(c, output),
            GUMBO_LEX_RCDATA_LT => self.handle_lt_state(c, output, GUMBO_LEX_RCDATA_END_TAG_OPEN, GUMBO_LEX_RCDATA),
            GUMBO_LEX_RCDATA_END_TAG_OPEN => {
                self.handle_end_tag_open_like_state(c, output, GUMBO_LEX_RCDATA_END_TAG_NAME, GUMBO_LEX_RCDATA)
            }
            GUMBO_LEX_RCDATA_END_TAG_NAME => self.handle_end_tag_name_like_state(c, output, GUMBO_LEX_RCDATA),
            GUMBO_LEX_RAWTEXT_LT => self.handle_lt_state(c, output, GUMBO_LEX_RAWTEXT_END_TAG_OPEN, GUMBO_LEX_RAWTEXT),
            GUMBO_LEX_RAWTEXT_END_TAG_OPEN => {
                self.handle_end_tag_open_like_state(c, output, GUMBO_LEX_RAWTEXT_END_TAG_NAME, GUMBO_LEX_RAWTEXT)
            }
            GUMBO_LEX_RAWTEXT_END_TAG_NAME => self.handle_end_tag_name_like_state(c, output, GUMBO_LEX_RAWTEXT),
            GUMBO_LEX_SCRIPT_LT => self.handle_script_lt_state(c, output),
            GUMBO_LEX_SCRIPT_END_TAG_OPEN => {
                self.handle_end_tag_open_like_state(c, output, GUMBO_LEX_SCRIPT_END_TAG_NAME, GUMBO_LEX_SCRIPT)
            }
            GUMBO_LEX_SCRIPT_END_TAG_NAME => self.handle_end_tag_name_like_state(c, output, GUMBO_LEX_SCRIPT),
            GUMBO_LEX_SCRIPT_ESCAPED_START => self.handle_script_escaped_start_state(c, output, GUMBO_LEX_SCRIPT_ESCAPED_START_DASH),
            GUMBO_LEX_SCRIPT_ESCAPED_START_DASH => self.handle_script_escaped_start_state(c, output, GUMBO_LEX_SCRIPT_ESCAPED_DASH_DASH),
            GUMBO_LEX_SCRIPT_ESCAPED => self.handle_script_escaped_state(c, output),
            GUMBO_LEX_SCRIPT_ESCAPED_DASH => self.handle_script_escaped_dash_state(c, output),
            GUMBO_LEX_SCRIPT_ESCAPED_DASH_DASH => self.handle_script_escaped_dash_dash_state(c, output),
            GUMBO_LEX_SCRIPT_ESCAPED_LT => self.handle_script_escaped_lt_state(c, output),
            GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_OPEN => {
                self.handle_end_tag_open_like_state(c, output, GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_NAME, GUMBO_LEX_SCRIPT_ESCAPED)
            }
            GUMBO_LEX_SCRIPT_ESCAPED_END_TAG_NAME => self.handle_end_tag_name_like_state(c, output, GUMBO_LEX_SCRIPT_ESCAPED),
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_START => {
                self.handle_script_double_escape_edge_state(c, output, GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED, GUMBO_LEX_SCRIPT_ESCAPED)
            }
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED => self.handle_script_double_escaped_state(c, output),
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH => self.handle_script_double_escaped_dash_state(c, output),
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_DASH_DASH => self.handle_script_double_escaped_dash_dash_state(c, output),
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_LT => self.handle_script_double_escaped_lt_state(c, output),
            GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED_END => {
                self.handle_script_double_escape_edge_state(c, output, GUMBO_LEX_SCRIPT_ESCAPED, GUMBO_LEX_SCRIPT_DOUBLE_ESCAPED)
            }
            GUMBO_LEX_BEFORE_ATTR_NAME => self.handle_before_attr_name_state(c, output),
            GUMBO_LEX_ATTR_NAME => self.handle_attr_name_state(c, output),
            GUMBO_LEX_AFTER_ATTR_NAME => self.handle_after_attr_name_state(c, output),
            GUMBO_LEX_BEFORE_ATTR_VALUE => self.handle_before_attr_value_state(c, output),
            GUMBO_LEX_ATTR_VALUE_DOUBLE_QUOTED => self.handle_attr_value_quoted_state(c, DQUOTE, GUMBO_ERR_ATTR_DOUBLE_QUOTE_EOF),
            GUMBO_LEX_ATTR_VALUE_SINGLE_QUOTED => self.handle_attr_value_quoted_state(c, SQUOTE, GUMBO_ERR_ATTR_SINGLE_QUOTE_EOF),
            GUMBO_LEX_ATTR_VALUE_UNQUOTED => self.handle_attr_value_unquoted_state(c, output),
            GUMBO_LEX_CHAR_REF_IN_ATTR_VALUE => self.handle_char_ref_in_attr_value_state(),
            GUMBO_LEX_AFTER_ATTR_VALUE_QUOTED => self.handle_after_attr_value_quoted_state(c, output),
            GUMBO_LEX_SELF_CLOSING_START_TAG => self.handle_self_closing_start_tag_state(c, output),
            GUMBO_LEX_BOGUS_COMMENT => self.handle_bogus_comment_state(c, output),
            GUMBO_LEX_MARKUP_DECLARATION => self.handle_markup_declaration_state(),
            GUMBO_LEX_COMMENT_START => self.handle_comment_start_state(c, output),
            GUMBO_LEX_COMMENT_START_DASH => self.handle_comment_start_dash_state(c, output),
            GUMBO_LEX_COMMENT => self.handle_comment_state(c, output),
            GUMBO_LEX_COMMENT_END_DASH => self.handle_comment_end_dash_state(c, output),
            GUMBO_LEX_COMMENT_END => self.handle_comment_end_state(c, output),
            GUMBO_LEX_COMMENT_END_BANG => self.handle_comment_end_bang_state(c, output),
            GUMBO_LEX_DOCTYPE => self.handle_doctype_state(c, output),
            GUMBO_LEX_BEFORE_DOCTYPE_NAME => self.handle_before_doctype_name_state(c, output),
            GUMBO_LEX_DOCTYPE_NAME => self.handle_doctype_name_state(c, output),
            GUMBO_LEX_AFTER_DOCTYPE_NAME => self.handle_after_doctype_name_state(c, output),
            GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_KEYWORD => self.handle_after_doctype_keyword_state(
                c,
                output,
                GUMBO_LEX_BEFORE_DOCTYPE_PUBLIC_ID,
                GUMBO_LEX_DOCTYPE_PUBLIC_ID_DOUBLE_QUOTED,
                GUMBO_LEX_DOCTYPE_PUBLIC_ID_SINGLE_QUOTED,
            ),
            GUMBO_LEX_BEFORE_DOCTYPE_PUBLIC_ID => self.handle_before_doctype_id_state(
                c,
                output,
                GUMBO_LEX_DOCTYPE_PUBLIC_ID_DOUBLE_QUOTED,
                GUMBO_LEX_DOCTYPE_PUBLIC_ID_SINGLE_QUOTED,
            ),
            GUMBO_LEX_DOCTYPE_PUBLIC_ID_DOUBLE_QUOTED => {
                self.handle_doctype_id_quoted_state(c, output, DQUOTE, true, GUMBO_ERR_DOCTYPE_EOF)
            }
            GUMBO_LEX_DOCTYPE_PUBLIC_ID_SINGLE_QUOTED => {
                self.handle_doctype_id_quoted_state(c, output, SQUOTE, true, GUMBO_ERR_DOCTYPE_EOF)
            }
            GUMBO_LEX_AFTER_DOCTYPE_PUBLIC_ID => self.handle_after_doctype_public_id_state(c, output),
            GUMBO_LEX_BETWEEN_DOCTYPE_PUBLIC_SYSTEM_ID => self.handle_between_doctype_public_system_id_state(c, output),
            GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_KEYWORD => self.handle_after_doctype_keyword_state(
                c,
                output,
                GUMBO_LEX_BEFORE_DOCTYPE_SYSTEM_ID,
                GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED,
                GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED,
            ),
            GUMBO_LEX_BEFORE_DOCTYPE_SYSTEM_ID => self.handle_before_doctype_id_state(
                c,
                output,
                GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED,
                GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED,
            ),
            GUMBO_LEX_DOCTYPE_SYSTEM_ID_DOUBLE_QUOTED => {
                self.handle_doctype_id_quoted_state(c, output, DQUOTE, false, GUMBO_ERR_DOCTYPE_EOF)
            }
            // The single-quoted system identifier records GUMBO_ERR_DOCTYPE_INVALID at the end of input.
            GUMBO_LEX_DOCTYPE_SYSTEM_ID_SINGLE_QUOTED => {
                self.handle_doctype_id_quoted_state(c, output, SQUOTE, false, GUMBO_ERR_DOCTYPE_INVALID)
            }
            GUMBO_LEX_AFTER_DOCTYPE_SYSTEM_ID => self.handle_after_doctype_system_id_state(c, output),
            GUMBO_LEX_BOGUS_DOCTYPE => self.handle_bogus_doctype_state(c, output),
            _ => self.handle_cdata_state(c, output),
        }
    }

    /// `gumbo_lex`: the next token into `output`; false when its state reported an error.
    pub(crate) fn gumbo_lex(&mut self, output: &mut GumboToken) -> bool {
        if self.tokenizer.buffered_emit_char != kGumboNoChar {
            self.tokenizer.reconsume_current_input = true;
            let c = self.tokenizer.buffered_emit_char;
            self.emit_char(c, output);
            self.tokenizer.reconsume_current_input = false;
            self.tokenizer.buffered_emit_char = kGumboNoChar;
            return true;
        }
        if self.maybe_emit_from_temporary_buffer(output) {
            return true;
        }
        loop {
            let c = self.input_current();
            let result = self.dispatch(c, output);
            let should_advance = !self.tokenizer.reconsume_current_input;
            self.tokenizer.reconsume_current_input = false;
            match result {
                RETURN_SUCCESS => return true,
                RETURN_ERROR => return false,
                NEXT_CHAR => {}
            }
            if should_advance {
                self.input_next();
            }
        }
    }
}
