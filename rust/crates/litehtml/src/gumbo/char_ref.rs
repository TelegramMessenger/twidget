//! char_ref.c: character references, numeric ones by hand and named ones through the Ragel machine char_ref.rl
//! compiles to (`ragel -F0`), its tables in [`super::tables`] as char_ref.c has them and its driver here, step for
//! step.

use super::error::{
    Errors, GUMBO_ERR_NAMED_CHAR_REF_INVALID, GUMBO_ERR_NAMED_CHAR_REF_WITHOUT_SEMICOLON, GUMBO_ERR_NUMERIC_CHAR_REF_INVALID,
    GUMBO_ERR_NUMERIC_CHAR_REF_NO_DIGITS, GUMBO_ERR_NUMERIC_CHAR_REF_WITHOUT_SEMICOLON, GumboError, GumboErrorType, GumboErrorValue,
};
use super::tables::*;
use super::utf8::{Utf8Iterator, utf8_is_invalid_code_point};
use super::{GumboOptions, GumboStringPiece};

pub const kGumboNoChar: i32 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OneOrTwoCodepoints {
    pub first: i32,
    pub second: i32,
}

/// What a case of the machine's transition actions does (tools/gen_gumbo_tables.py reads it from char_ref.c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum Action {
    /// No case for it in that switch.
    NONE,
    /// `te = p+1;`
    TE_NEXT,
    /// `te = p+1;` then the code points, `p++` and out.
    MATCH_NEXT,
    /// `te = p; p--;` then the code points, `p++` and out.
    MATCH_HERE,
    /// `p = te-1;` then the code points, `p++` and out.
    MATCH_TE,
}

/// The input, options and errors every step of a reference reads or adds to.
#[derive(Debug)]
pub struct Ctx<'a, 'b> {
    pub input: &'a [u8],
    pub options: &'a GumboOptions,
    pub errors: &'b mut Errors,
}

fn parse_digit(c: i32, allow_hex: bool) -> i32 {
    if (i32::from(b'0')..=i32::from(b'9')).contains(&c) {
        return c - i32::from(b'0');
    }
    if allow_hex && (i32::from(b'a')..=i32::from(b'f')).contains(&c) {
        return c - i32::from(b'a') + 10;
    }
    if allow_hex && (i32::from(b'A')..=i32::from(b'F')).contains(&c) {
        return c - i32::from(b'A') + 10;
    }
    -1
}

fn add_error_at_mark(ctx: &mut Ctx, input: &Utf8Iterator, ty: GumboErrorType, v: GumboErrorValue) {
    let (position, original_text) = input.error_at_mark();
    ctx.errors.add(GumboError { ty, position, original_text, v });
}

/// `add_codepoint_error`: the code point is an `int` stored in the union's `uint64_t`.
fn add_codepoint_error(ctx: &mut Ctx, input: &Utf8Iterator, ty: GumboErrorType, codepoint: i32) {
    add_error_at_mark(ctx, input, ty, GumboErrorValue::Codepoint(i64::from(codepoint) as u64));
}

fn maybe_replace_codepoint(codepoint: i32) -> i32 {
    let mut i = 0;
    while CHAR_REPLACEMENTS[i].0 != -1 {
        if CHAR_REPLACEMENTS[i].0 == codepoint {
            return CHAR_REPLACEMENTS[i].1;
        }
        i += 1;
    }
    -1
}

fn consume_numeric_ref(ctx: &mut Ctx, input: &mut Utf8Iterator, output: &mut i32) -> bool {
    input.next(ctx.input, ctx.options, ctx.errors);
    let mut is_hex = false;
    let mut c = input.current();
    if c == i32::from(b'x') || c == i32::from(b'X') {
        is_hex = true;
        input.next(ctx.input, ctx.options, ctx.errors);
        c = input.current();
    }
    let mut digit = parse_digit(c, is_hex);
    if digit == -1 {
        add_error_at_mark(ctx, input, GUMBO_ERR_NUMERIC_CHAR_REF_NO_DIGITS, GumboErrorValue::None);
        input.reset(ctx.input, ctx.errors);
        *output = kGumboNoChar;
        return false;
    }
    // `unsigned int`, so a long reference wraps.
    let mut codepoint: u32 = 0;
    let mut status = true;
    loop {
        codepoint = codepoint.wrapping_mul(if is_hex { 16 } else { 10 }).wrapping_add(digit as u32);
        input.next(ctx.input, ctx.options, ctx.errors);
        digit = parse_digit(input.current(), is_hex);
        if digit == -1 {
            break;
        }
    }
    if input.current() != i32::from(b';') {
        add_codepoint_error(ctx, input, GUMBO_ERR_NUMERIC_CHAR_REF_WITHOUT_SEMICOLON, codepoint as i32);
        status = false;
    } else {
        input.next(ctx.input, ctx.options, ctx.errors);
    }
    let replacement = maybe_replace_codepoint(codepoint as i32);
    if replacement != -1 {
        add_codepoint_error(ctx, input, GUMBO_ERR_NUMERIC_CHAR_REF_INVALID, codepoint as i32);
        *output = replacement;
        return false;
    }
    if (0xd800..=0xdfff).contains(&codepoint) || codepoint > 0x10ffff {
        add_codepoint_error(ctx, input, GUMBO_ERR_NUMERIC_CHAR_REF_INVALID, codepoint as i32);
        *output = 0xfffd;
        return false;
    }
    if utf8_is_invalid_code_point(codepoint as i32) || codepoint == 0xb {
        add_codepoint_error(ctx, input, GUMBO_ERR_NUMERIC_CHAR_REF_INVALID, codepoint as i32);
        status = false;
    }
    *output = codepoint as i32;
    status
}

fn maybe_add_invalid_named_reference(ctx: &mut Ctx, input: &mut Utf8Iterator) -> bool {
    let start = input.get_char_pointer();
    let mut c = input.current();
    while (i32::from(b'a')..=i32::from(b'z')).contains(&c)
        || (i32::from(b'A')..=i32::from(b'Z')).contains(&c)
        || (i32::from(b'0')..=i32::from(b'9')).contains(&c)
    {
        input.next(ctx.input, ctx.options, ctx.errors);
        c = input.current();
    }
    if c == i32::from(b';') {
        let bad_ref = GumboStringPiece { data: Some(start), length: input.get_char_pointer() - start };
        add_error_at_mark(ctx, input, GUMBO_ERR_NAMED_CHAR_REF_INVALID, GumboErrorValue::Text(bad_ref));
        return false;
    }
    true
}

/// Where the generated machine is between its labels.
enum Step {
    Resume,
    EofTrans(usize),
    TestEof,
    Out,
}

fn consume_named_ref(ctx: &mut Ctx, input: &mut Utf8Iterator, is_in_attribute: bool, output: &mut OneOrTwoCodepoints) -> bool {
    let bytes = ctx.input;
    let mut p = input.get_char_pointer();
    let pe = input.get_end_pointer();
    let eof = pe;
    // `te` starts as a NULL pointer; `ts` and `act` are written but never read.
    let mut te: Option<usize> = None;
    let start = p;
    let mut cs: i32 = CHAR_REF_START;

    let mut step = if p == pe {
        Step::TestEof
    } else if cs == 0 {
        Step::Out
    } else {
        Step::Resume
    };
    loop {
        match step {
            Step::Resume => {
                // The from-state actions only set `ts`.
                let state = cs as usize;
                let keys = state << 1;
                let inds = usize::from(CHAR_REF_INDEX_OFFSETS[state]);
                let slen = i32::from(CHAR_REF_KEY_SPANS[state]);
                // `*p` is a `char`: signed on the platforms the helper runs on, and every key is ASCII either way.
                let ch = i32::from(bytes[p] as i8);
                let (k0, k1) = (i32::from(CHAR_REF_TRANS_KEYS[keys]), i32::from(CHAR_REF_TRANS_KEYS[keys + 1]));
                let index = if slen > 0 && k0 <= ch && ch <= k1 { (ch - k0) as usize } else { slen as usize };
                step = Step::EofTrans(CHAR_REF_INDICIES[inds + index] as usize);
            }
            Step::EofTrans(trans) => {
                cs = i32::from(CHAR_REF_TRANS_TARGS[trans]);
                let mut out = false;
                if CHAR_REF_TRANS_ACTIONS[trans] != 0 {
                    let mut acts = CHAR_REF_TRANS_ACTIONS[trans] as usize;
                    let nacts = CHAR_REF_ACTIONS[acts];
                    acts += 1;
                    for _ in 0..nacts {
                        let (action, first, second) = CHAR_REF_ACTION_CODE[CHAR_REF_ACTIONS[acts] as usize];
                        acts += 1;
                        match action {
                            Action::NONE => {}
                            Action::TE_NEXT => te = Some(p + 1),
                            Action::MATCH_NEXT | Action::MATCH_HERE | Action::MATCH_TE => {
                                match action {
                                    Action::MATCH_NEXT => te = Some(p + 1),
                                    Action::MATCH_HERE => te = Some(p),
                                    // `p = te-1`: p is not read once the machine is out.
                                    _ => {}
                                }
                                output.first = first;
                                if second != -1 {
                                    output.second = second;
                                }
                                out = true;
                                break;
                            }
                        }
                    }
                }
                step = if out {
                    Step::Out
                } else if cs == 0 {
                    // The to-state actions only clear `ts`.
                    Step::Out
                } else {
                    p += 1;
                    if p != pe { Step::Resume } else { Step::TestEof }
                };
            }
            Step::TestEof => {
                step = Step::Out;
                if p == eof && CHAR_REF_EOF_TRANS[cs as usize] > 0 {
                    step = Step::EofTrans((CHAR_REF_EOF_TRANS[cs as usize] - 1) as usize);
                }
            }
            Step::Out => break,
        }
    }

    // A final state with `te` still NULL is out of the machine's reach (each way into one sets it); the C would
    // read before the input there, and this takes it for no match.
    match te {
        Some(te) if cs >= CHAR_REF_START => {
            let last_char = bytes[te - 1];
            let len = te - start;
            if last_char == b';' {
                input.consume(bytes, ctx.options, ctx.errors, len);
                true
            } else if is_in_attribute && {
                // `*te` is the byte after the match: the NUL the C string ends with when the match ends the input.
                let next = bytes.get(te).copied().unwrap_or(0);
                next == b'=' || next.is_ascii_alphanumeric()
            } {
                output.first = kGumboNoChar;
                output.second = kGumboNoChar;
                input.reset(bytes, ctx.errors);
                true
            } else {
                let bad_ref = GumboStringPiece { data: Some(start), length: te - start };
                add_error_at_mark(ctx, input, GUMBO_ERR_NAMED_CHAR_REF_WITHOUT_SEMICOLON, GumboErrorValue::Text(bad_ref));
                input.consume(bytes, ctx.options, ctx.errors, len);
                false
            }
        }
        _ => {
            output.first = kGumboNoChar;
            output.second = kGumboNoChar;
            let status = maybe_add_invalid_named_reference(ctx, input);
            input.reset(bytes, ctx.errors);
            status
        }
    }
}

/// `consume_char_ref`, the iterator at the `&`.
pub fn consume_char_ref(
    ctx: &mut Ctx,
    input: &mut Utf8Iterator,
    additional_allowed_char: i32,
    is_in_attribute: bool,
    output: &mut OneOrTwoCodepoints,
) -> bool {
    input.mark();
    input.next(ctx.input, ctx.options, ctx.errors);
    let c = input.current();
    output.first = kGumboNoChar;
    output.second = kGumboNoChar;
    if c == additional_allowed_char {
        input.reset(ctx.input, ctx.errors);
        output.first = kGumboNoChar;
        return true;
    }
    match input.current() {
        0x09 | 0x0a | 0x0c | 0x20 | 0x3c | 0x26 | -1 => {
            input.reset(ctx.input, ctx.errors);
            true
        }
        0x23 => consume_numeric_ref(ctx, input, &mut output.first),
        _ => consume_named_ref(ctx, input, is_in_attribute, output),
    }
}
