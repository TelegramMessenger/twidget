//! utf8.c: the iterator over the input's code points, with the HTML5 preprocessing (CR LF read as LF, a lone CR
//! as LF) and an error for each invalid or truncated sequence, read again each time the iterator goes back.

use super::error::{Errors, GUMBO_ERR_UTF8_INVALID, GUMBO_ERR_UTF8_TRUNCATED, GumboError, GumboErrorValue};
use super::tables::UTF8D;
use super::{GumboOptions, GumboSourcePosition};

pub const kUtf8ReplacementChar: i32 = 0xFFFD;

const UTF8_ACCEPT: u32 = 0;
const UTF8_REJECT: u32 = 12;

fn decode(state: &mut u32, codep: &mut u32, byte: u32) -> u32 {
    let ty = u32::from(UTF8D[byte as usize]);
    *codep = if *state != UTF8_ACCEPT { (byte & 0x3f) | (*codep << 6) } else { (0xff >> ty) & byte };
    *state = u32::from(UTF8D[(256 + *state + ty) as usize]);
    *state
}

pub fn utf8_is_invalid_code_point(c: i32) -> bool {
    (0x1..=0x8).contains(&c)
        || c == 0xB
        || (0xE..=0x1F).contains(&c)
        || (0x7F..=0x9F).contains(&c)
        || (0xFDD0..=0xFDEF).contains(&c)
        || (c & 0xFFFF) == 0xFFFE
        || (c & 0xFFFF) == 0xFFFF
}

/// `Utf8Iterator`: positions are offsets into the input; `_parser` is what each call is given.
#[derive(Clone, Debug, Default)]
pub struct Utf8Iterator {
    pub start: usize,
    pub mark: usize,
    pub end: usize,
    pub current: i32,
    pub width: usize,
    pub pos: GumboSourcePosition,
    pub mark_pos: GumboSourcePosition,
}

impl Utf8Iterator {
    fn add_error(&self, input: &[u8], errors: &mut Errors, ty: u32) {
        let mut code_point: u64 = 0;
        for i in 0..self.width {
            code_point = (code_point << 8) | u64::from(input[self.start + i]);
        }
        errors.add(GumboError { ty, position: self.pos, original_text: Some(self.start), v: GumboErrorValue::Codepoint(code_point) });
    }

    fn read_char(&mut self, input: &[u8], errors: &mut Errors) {
        if self.start >= self.end {
            self.current = -1;
            self.width = 0;
            return;
        }
        let mut code_point: u32 = 0;
        let mut state = UTF8_ACCEPT;
        let mut c = self.start;
        while c < self.end {
            decode(&mut state, &mut code_point, u32::from(input[c]));
            if state == UTF8_ACCEPT {
                self.width = c - self.start + 1;
                if code_point == u32::from(b'\r') {
                    let next = c + 1;
                    if next < self.end && input[next] == b'\n' {
                        self.start += 1;
                        self.pos.offset = self.pos.offset.wrapping_add(1);
                    }
                    code_point = u32::from(b'\n');
                }
                if utf8_is_invalid_code_point(code_point as i32) {
                    self.add_error(input, errors, GUMBO_ERR_UTF8_INVALID);
                    code_point = kUtf8ReplacementChar as u32;
                }
                self.current = code_point as i32;
                return;
            } else if state == UTF8_REJECT {
                self.width = c - self.start + usize::from(c == self.start);
                self.current = kUtf8ReplacementChar;
                self.add_error(input, errors, GUMBO_ERR_UTF8_INVALID);
                return;
            }
            c += 1;
        }
        self.current = kUtf8ReplacementChar;
        self.width = self.end - self.start;
        self.add_error(input, errors, GUMBO_ERR_UTF8_TRUNCATED);
    }

    fn update_position(&mut self, options: &GumboOptions) {
        self.pos.offset = self.pos.offset.wrapping_add(self.width as u32);
        if self.current == i32::from(b'\n') {
            self.pos.line = self.pos.line.wrapping_add(1);
            self.pos.column = 1;
        } else if self.current == i32::from(b'\t') {
            // unsigned arithmetic, as the C's `int tab_stop` meets an unsigned column.
            let tab_stop = options.tab_stop as u32;
            self.pos.column = (self.pos.column / tab_stop).wrapping_add(1).wrapping_mul(tab_stop);
        } else if self.current != -1 {
            self.pos.column = self.pos.column.wrapping_add(1);
        }
    }

    pub fn init(input: &[u8], errors: &mut Errors) -> Utf8Iterator {
        let mut iter = Utf8Iterator {
            start: 0,
            mark: 0,
            end: input.len(),
            current: 0,
            width: 0,
            pos: GumboSourcePosition { line: 1, column: 1, offset: 0 },
            mark_pos: GumboSourcePosition::default(),
        };
        iter.read_char(input, errors);
        iter
    }

    pub fn next(&mut self, input: &[u8], options: &GumboOptions, errors: &mut Errors) {
        self.update_position(options);
        self.start += self.width;
        self.read_char(input, errors);
    }

    pub fn current(&self) -> i32 {
        self.current
    }

    pub fn get_position(&self) -> GumboSourcePosition {
        self.pos
    }

    pub fn get_char_pointer(&self) -> usize {
        self.start
    }

    pub fn get_end_pointer(&self) -> usize {
        self.end
    }

    /// `utf8iterator_maybe_consume_match`: `prefix` is ASCII without a NUL, so `strncmp`/`strncasecmp` compare
    /// byte for byte; a match consumes one character per byte.
    pub fn maybe_consume_match(
        &mut self,
        input: &[u8],
        options: &GumboOptions,
        errors: &mut Errors,
        prefix: &[u8],
        case_sensitive: bool,
    ) -> bool {
        let length = prefix.len();
        let matched = self.start + length <= self.end && {
            let here = &input[self.start..self.start + length];
            if case_sensitive { here == prefix } else { here.eq_ignore_ascii_case(prefix) }
        };
        if matched {
            for _ in 0..length {
                self.next(input, options, errors);
            }
            true
        } else {
            false
        }
    }

    /// The match consume_named_ref makes: the input against itself, which `strncmp` finds equal up to the first
    /// NUL and so in full.
    pub fn consume(&mut self, input: &[u8], options: &GumboOptions, errors: &mut Errors, length: usize) -> bool {
        if self.start + length <= self.end {
            for _ in 0..length {
                self.next(input, options, errors);
            }
            true
        } else {
            false
        }
    }

    pub fn mark(&mut self) {
        self.mark = self.start;
        self.mark_pos = self.pos;
    }

    pub fn reset(&mut self, input: &[u8], errors: &mut Errors) {
        self.start = self.mark;
        self.pos = self.mark_pos;
        self.read_char(input, errors);
    }

    /// `utf8iterator_fill_error_at_mark`: the position and text an error at the mark has.
    pub fn error_at_mark(&self) -> (GumboSourcePosition, Option<usize>) {
        (self.mark_pos, Some(self.mark))
    }
}
