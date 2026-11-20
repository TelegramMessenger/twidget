//! `encodings.h` and `encodings.cpp`, as far as the ported stages need them: decoding
//! (https://encoding.spec.whatwg.org/#decode) with the BOM sniff, and the UTF-8 and UTF-16 decoders, which
//! are all a stylesheet meets (it is read as UTF-8 unless a BOM says UTF-16). The legacy decoders the HTML
//! parser may choose come with stage L3.

use crate::utf8_strings::append_char;

/// `encoding`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// An error or no encoding.
    null,
    utf_8,

    // Legacy single-byte encodings
    ibm866,
    iso_8859_2,
    iso_8859_3,
    iso_8859_4,
    iso_8859_5,
    iso_8859_6,
    iso_8859_7,
    iso_8859_8,
    iso_8859_8_i,
    iso_8859_10,
    iso_8859_13,
    iso_8859_14,
    iso_8859_15,
    iso_8859_16,
    koi8_r,
    koi8_u,
    macintosh,
    windows_874,
    windows_1250,
    windows_1251,
    windows_1252,
    windows_1253,
    windows_1254,
    windows_1255,
    windows_1256,
    windows_1257,
    windows_1258,
    x_mac_cyrillic,

    // Legacy multi-byte East Asian encodings
    gbk,
    gb18030,
    big5,
    euc_jp,
    iso_2022_jp,
    shift_jis,
    euc_kr,

    // Legacy miscellaneous encodings
    replacement,
    utf_16be,
    utf_16le,
    x_user_defined,
}

/// `decoder::result`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeResult {
    /// No more bytes.
    Finished,
    /// A decoding error: U+FFFD is emitted.
    Error,
    /// More bytes are needed (handler), or keep going (process_an_item).
    Continue,
    /// A code point.
    CodePoint(u32),
}

/// `decoder::null`.
const NULL: i32 = -2;

/// The decoders ported so far.
#[derive(Debug)]
enum Decoder {
    /// `utf_8_decoder`.
    Utf8 { code_point: u32, bytes_seen: i32, bytes_needed: i32, lower_boundary: u8, upper_boundary: u8 },
    /// `utf_16_decoder`.
    Utf16 { lead_byte: i32, lead_surrogate: i32, utf_16be: bool },
    /// What a legacy encoding decodes to until it is ported: nothing.
    Unported,
}

impl Decoder {
    /// `get_decoder(encoding)`.
    fn new(encoding: Encoding) -> Decoder {
        match encoding {
            Encoding::utf_8 => Decoder::Utf8 { code_point: 0, bytes_seen: 0, bytes_needed: 0, lower_boundary: 0x80, upper_boundary: 0xBF },
            Encoding::utf_16be | Encoding::utf_16le => {
                Decoder::Utf16 { lead_byte: NULL, lead_surrogate: NULL, utf_16be: encoding == Encoding::utf_16be }
            }
            _ => Decoder::Unported,
        }
    }

    /// `handler(input, index, ch)`: the next item of the queue. A decoder may put bytes back (UTF-16 inserts
    /// the code unit it could not pair).
    fn handler(&mut self, input: &mut Vec<u8>, index: &mut usize) -> DecodeResult {
        // read byte from input
        let byte = if *index == input.len() {
            None
        } else {
            *index += 1;
            Some(input[*index - 1])
        };
        match self {
            // https://encoding.spec.whatwg.org/#utf-8-decoder
            Decoder::Utf8 { code_point, bytes_seen, bytes_needed, lower_boundary, upper_boundary } => {
                let Some(b) = byte else {
                    // 1, 2. End of queue: an error with bytes still needed, else finished.
                    if *bytes_needed != 0 {
                        *bytes_needed = 0;
                        return DecodeResult::Error;
                    }
                    return DecodeResult::Finished;
                };
                // 3.
                if *bytes_needed == 0 {
                    match b {
                        0x00..=0x7F => return DecodeResult::CodePoint(u32::from(b)),
                        0xC2..=0xDF => {
                            *bytes_needed = 1;
                            *code_point = u32::from(b & 0x1F);
                        }
                        0xE0..=0xEF => {
                            if b == 0xE0 {
                                *lower_boundary = 0xA0;
                            }
                            if b == 0xED {
                                *upper_boundary = 0x9F;
                            }
                            *bytes_needed = 2;
                            *code_point = u32::from(b & 0xF);
                        }
                        0xF0..=0xF4 => {
                            if b == 0xF0 {
                                *lower_boundary = 0x90;
                            }
                            if b == 0xF4 {
                                *upper_boundary = 0x8F;
                            }
                            *bytes_needed = 3;
                            *code_point = u32::from(b & 0x7);
                        }
                        _ => return DecodeResult::Error,
                    }
                    return DecodeResult::Continue;
                }
                // 4. Out of the boundaries: reset, restore the byte, error.
                if b < *lower_boundary || b > *upper_boundary {
                    *code_point = 0;
                    *bytes_needed = 0;
                    *bytes_seen = 0;
                    *lower_boundary = 0x80;
                    *upper_boundary = 0xBF;
                    *index -= 1;
                    return DecodeResult::Error;
                }
                // 5 to 8.
                *lower_boundary = 0x80;
                *upper_boundary = 0xBF;
                *code_point = (*code_point << 6) | u32::from(b & 0x3F);
                *bytes_seen += 1;
                if *bytes_seen != *bytes_needed {
                    return DecodeResult::Continue;
                }
                // 9 to 11.
                let codepoint = *code_point;
                *code_point = 0;
                *bytes_needed = 0;
                *bytes_seen = 0;
                DecodeResult::CodePoint(codepoint)
            }
            // https://encoding.spec.whatwg.org/#shared-utf-16-decoder
            Decoder::Utf16 { lead_byte, lead_surrogate, utf_16be } => {
                let Some(b) = byte else {
                    // 1, 2. End of queue: an error with a lead byte or surrogate pending, else finished.
                    if *lead_byte != NULL || *lead_surrogate != NULL {
                        *lead_byte = NULL;
                        *lead_surrogate = NULL;
                        return DecodeResult::Error;
                    }
                    return DecodeResult::Finished;
                };
                let b = i32::from(b);
                // 3.
                if *lead_byte == NULL {
                    *lead_byte = b;
                    return DecodeResult::Continue;
                }
                // 4.
                let code_unit = if *utf_16be { (*lead_byte << 8) + b } else { (b << 8) + *lead_byte };
                *lead_byte = NULL;
                // 5.
                if *lead_surrogate != NULL {
                    let lead = *lead_surrogate;
                    *lead_surrogate = NULL;
                    if (0xDC00..=0xDFFF).contains(&code_unit) {
                        return DecodeResult::CodePoint((0x10000 + ((lead - 0xD800) << 10) + (code_unit - 0xDC00)) as u32);
                    }
                    // Restore the code unit's bytes to the queue and return an error.
                    let b1 = (code_unit >> 8) as u8;
                    let b2 = (code_unit & 0xFF) as u8;
                    let bytes = if *utf_16be { [b1, b2] } else { [b2, b1] };
                    input.splice(*index..*index, bytes);
                    return DecodeResult::Error;
                }
                // 6.
                if (0xD800..=0xDBFF).contains(&code_unit) {
                    *lead_surrogate = code_unit;
                    return DecodeResult::Continue;
                }
                // 7.
                if (0xDC00..=0xDFFF).contains(&code_unit) {
                    return DecodeResult::Error;
                }
                // 8.
                DecodeResult::CodePoint(code_unit as u32)
            }
            Decoder::Unported => DecodeResult::Finished,
        }
    }

    /// `process_a_queue(input, output, replacement)`: every item, an error as U+FFFD.
    fn process_a_queue(&mut self, input: &mut Vec<u8>, output: &mut Vec<u8>) {
        let mut index = 0;
        loop {
            match self.handler(input, &mut index) {
                DecodeResult::Finished => return,
                DecodeResult::CodePoint(ch) => append_char(output, ch),
                DecodeResult::Error => append_char(output, 0xFFFD),
                DecodeResult::Continue => {}
            }
        }
    }
}

/// `bom_sniff(str)` (https://encoding.spec.whatwg.org/#bom-sniff).
pub fn bom_sniff(str: &[u8]) -> Encoding {
    if str.starts_with(b"\xEF\xBB\xBF") {
        return Encoding::utf_8;
    }
    if str.starts_with(b"\xFE\xFF") {
        return Encoding::utf_16be;
    }
    if str.starts_with(b"\xFF\xFE") {
        return Encoding::utf_16le;
    }
    Encoding::null
}

/// `decode(input, encoding)` (https://encoding.spec.whatwg.org/#decode): a BOM picks the encoding and goes,
/// then the bytes decode to UTF-8 with U+FFFD for every error.
pub fn decode(input: &[u8], encoding: Encoding) -> Vec<u8> {
    let mut input = input.to_vec();
    let mut encoding = encoding;
    let bom_encoding = bom_sniff(&input);
    if bom_encoding != Encoding::null {
        encoding = bom_encoding;
        let len = if encoding == Encoding::utf_8 { 3 } else { 2 };
        input.drain(..len);
    }
    let mut output = Vec::new();
    Decoder::new(encoding).process_a_queue(&mut input, &mut output);
    output
}
