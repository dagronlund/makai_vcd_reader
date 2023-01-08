pub mod token;

use core::ops::Range;
use std::io;
use std::str;

use bytes::Bytes;
use makai::utils::bytes::ByteStorage;
use makai_waveform_db::bitvector::BitVector;

use crate::errors::*;
use crate::lexer::position::*;
use crate::lexer::*;
use crate::tokenizer::token::*;

pub type ByteRange = Range<usize>;

fn split_bytes(bytes: &[u8]) -> (ByteRange, ByteRange) {
    let mut first = 0;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b' ' | b'\t' | b'\n' => {
                first = i;
                break;
            }
            _ => {}
        }
    }
    let mut second = 0;
    for (i, b) in bytes.iter().enumerate().skip(first) {
        match b {
            b' ' | b'\t' | b'\n' => {}
            _ => {
                second = i;
                break;
            }
        }
    }
    (0..first, second..bytes.len())
}

fn tokenize_timestamp(bytes: &[u8]) -> TokenizerResult<u64> {
    let mut result = 0u64;
    for b in bytes.iter().skip(1) {
        result *= 10;
        result += (b - b'0') as u64;
    }
    Ok(result)
}

fn tokenize_idcode(bs: &mut ByteStorage, bytes: &[u8]) -> TokenIdCode {
    let usize_bytes = (usize::BITS / 8) as usize;
    if bytes.len() > usize_bytes
        || (bytes.len() == usize_bytes && (bytes[usize_bytes - 1] >> 7) == 0)
    {
        TokenIdCode::new(bs.insert(Bytes::copy_from_slice(bytes)) | (1 << (usize::BITS - 1)))
    } else {
        let mut id: usize = 0;
        for i in (0..bytes.len()).rev() {
            id <<= 8;
            id |= bytes[i] as usize;
        }
        TokenIdCode::new(id)
    }
}

fn tokenize_vector(bs: &mut ByteStorage, bytes: &[u8]) -> (BitVector, TokenIdCode) {
    let (vector_range, idcode_range) = split_bytes(bytes);
    let vector = BitVector::from_ascii(&bytes[vector_range][1..]);
    let idcode = tokenize_idcode(bs, &bytes[idcode_range]);
    (vector, idcode)
}

fn tokenize_vector_four_state(bs: &mut ByteStorage, bytes: &[u8]) -> (BitVector, TokenIdCode) {
    let (vector_range, idcode_range) = split_bytes(bytes);
    let vector = BitVector::from_ascii_four_state(&bytes[vector_range][1..]);
    let idcode = tokenize_idcode(bs, &bytes[idcode_range]);
    (vector, idcode)
}

fn tokenize_real(
    bs: &mut ByteStorage,
    bytes: &[u8],
    pos: LexerPosition,
) -> TokenizerResult<(f64, TokenIdCode)> {
    let (real_range, idcode_range) = split_bytes(bytes);
    let real = match String::from_utf8_lossy(&bytes[real_range][1..])
        .trim()
        .parse::<f64>()
    {
        Ok(result) => result,
        Err(err) => return Err(TokenizerError::RealParseError(err, pos)),
    };
    let idcode = tokenize_idcode(bs, &bytes[idcode_range]);
    Ok((real, idcode))
}

fn tokenize_scope(
    bs: &mut ByteStorage,
    bytes: Bytes,
    pos: LexerPosition,
) -> TokenizerResult<(TokenScopeType, usize)> {
    let (scope_type_range, scope_name_range) = split_bytes(&bytes[..]);
    let scope_type = TokenScopeType::from_byte_str(&bytes.slice(scope_type_range))
        .ok_or(TokenizerError::LexerError(pos))?;
    let scope_name = bs.insert(bytes.slice(scope_name_range));
    Ok((scope_type, scope_name))
}

fn tokenize_timescale(bytes: Bytes) -> TokenizerResult<(TokenTimescale, TokenTimescaleOffset)> {
    let offset = match bytes[1] {
        b'0' => match bytes[2] {
            b'0' => TokenTimescaleOffset::Hundred,
            _ => TokenTimescaleOffset::Ten,
        },
        _ => TokenTimescaleOffset::One,
    };
    let timescale = match bytes[bytes.len() - 2] {
        b'f' => TokenTimescale::Femtoseconds,
        b'p' => TokenTimescale::Picoseconds,
        b'n' => TokenTimescale::Nanoseconds,
        b'u' => TokenTimescale::Microseconds,
        b'm' => TokenTimescale::Milliseconds,
        _ => TokenTimescale::Seconds,
    };
    Ok((timescale, offset))
}

fn tokenize_variable_description(
    bs: &mut ByteStorage,
    bytes: Bytes,
    pos: LexerPosition,
) -> TokenizerResult<TokenVariableDescription> {
    // Check if a width is even specified, split by whitespace
    let (id_range, width_range) = split_bytes(&bytes[..]);
    if id_range.is_empty() {
        let id = bs.insert(bytes);
        return Ok(TokenVariableDescription::Unspecified { id });
    }
    // Check that the width is wrapped by square brackets
    let id = bs.insert(bytes.slice(0..id_range.end));
    let width_range = if width_range.len() <= 2
        || bytes[width_range.start] != b'['
        || bytes[width_range.end - 1] != b']'
    {
        return Err(TokenizerError::LexerError(pos));
    } else {
        width_range.start + 1..width_range.end - 1
    };
    // Search for a colon splitting the msb and lsb
    let colon = width_range.clone().find(|i| bytes[*i] == b':');
    // Parse the width into integers
    if let Some(colon) = colon {
        let msb_bytes = bytes.slice(width_range.start..colon);
        let lsb_bytes = bytes.slice(colon + 1..width_range.end);
        let msb = match String::from_utf8_lossy(&msb_bytes).trim().parse::<usize>() {
            Ok(result) => result,
            Err(err) => return Err(TokenizerError::IntegerParseError(err, pos)),
        };
        let lsb = match String::from_utf8_lossy(&lsb_bytes).trim().parse::<usize>() {
            Ok(result) => result,
            Err(err) => return Err(TokenizerError::IntegerParseError(err, pos)),
        };
        Ok(TokenVariableDescription::VectorSelect { id, msb, lsb })
    } else {
        let bytes = bytes.slice(width_range);
        let width = match String::from_utf8_lossy(&bytes).trim().parse::<usize>() {
            Ok(result) => result,
            Err(err) => return Err(TokenizerError::IntegerParseError(err, pos)),
        };
        Ok(TokenVariableDescription::Vector { id, width })
    }
}

fn tokenize_variable(
    bs: &mut ByteStorage,
    bytes: Bytes,
    pos: LexerPosition,
) -> TokenizerResult<(
    TokenVariableNetType,
    usize,
    TokenIdCode,
    TokenVariableDescription,
)> {
    let (net_type_range, range) = split_bytes(&bytes[..]);
    let net_type = TokenVariableNetType::from_byte_str(&bytes.slice(net_type_range))
        .ok_or(TokenizerError::LexerError(pos))?;
    let bytes = bytes.slice(range);
    let (width_range, range) = split_bytes(&bytes[..]);
    let width = match String::from_utf8_lossy(&bytes.slice(width_range))
        .trim()
        .parse::<usize>()
    {
        Ok(width) => width,
        Err(err) => return Err(TokenizerError::IntegerParseError(err, pos)),
    };
    let bytes = bytes.slice(range);
    let (idcode_range, variable_description_range) = split_bytes(&bytes[..]);
    let idcode = tokenize_idcode(bs, &bytes[idcode_range]);
    let variable_description =
        tokenize_variable_description(bs, bytes.slice(variable_description_range), pos)?;
    if width != variable_description.get_width() {
        // Ignore the width mis-match if the variable reference didn't declare a width
        match &variable_description {
            TokenVariableDescription::Unspecified { id: _ } => {}
            _ => {
                return Err(TokenizerError::IncorrectVariableWidth(
                    width,
                    variable_description.get_width(),
                    pos,
                ))
            }
        }
    }
    match net_type {
        TokenVariableNetType::Real | TokenVariableNetType::Realtime => {
            if width != 64 {
                return Err(TokenizerError::IncorrectRealWidth(pos));
            }
        }
        _ => {}
    }
    Ok((net_type, width, idcode, variable_description))
}

pub struct Tokenizer {
    bytes: Bytes,
}

impl Tokenizer {
    pub fn new(s: &str) -> Self {
        Self {
            bytes: Bytes::copy_from_slice(s.as_bytes()),
        }
    }

    pub fn get_bytes(&self, range: ByteRange) -> Bytes {
        self.bytes.slice(range)
    }

    pub fn get_bytes_trimmed(&self, range: ByteRange) -> Bytes {
        let mut range = range;
        for i in range.start..range.end {
            match self.bytes[i] {
                b' ' | b'\t' | b'\n' => range.start += 1,
                _ => break,
            }
        }
        for i in (range.start..range.end).rev() {
            match self.bytes[i] {
                b' ' | b'\t' | b'\n' => range.end -= 1,
                _ => break,
            }
        }
        self.bytes.slice(range)
    }

    pub fn write_range(&self, range: ByteRange, writer: &mut dyn io::Write) -> io::Result<usize> {
        writer.write(&self.bytes[range])
    }

    pub fn next(
        &mut self,
        lexer_result: Option<LexerToken>,
        bs: &mut ByteStorage,
    ) -> TokenizerResult<Option<Token>> {
        let lexer_token = match lexer_result {
            Some(lexer_token) => lexer_token,
            None => return Ok(None),
        };
        let token = match lexer_token {
            // Unformatted blocks
            LexerToken::SectionComment(span, pos) => {
                Token::Comment(bs.insert(self.get_bytes(span)), pos)
            }
            LexerToken::SectionDate(span, pos) => Token::Date(bs.insert(self.get_bytes(span)), pos),
            LexerToken::SectionVersion(span, pos) => {
                Token::Version(bs.insert(self.get_bytes(span)), pos)
            }
            // Formatted blocks
            LexerToken::SectionScope(span, pos) => {
                let (scope_type, scope_id) = tokenize_scope(bs, self.get_bytes_trimmed(span), pos)?;
                Token::Scope {
                    scope_type,
                    scope_id,
                    pos,
                }
            }
            LexerToken::SectionTimescale(span, pos) => {
                let (timescale, offset) = tokenize_timescale(self.get_bytes_trimmed(span))?;
                Token::Timescale {
                    timescale,
                    offset,
                    pos,
                }
            }
            LexerToken::SectionVar(span, pos) => {
                let (net_type, width, token_idcode, variable_description) =
                    tokenize_variable(bs, self.get_bytes_trimmed(span), pos)?;
                Token::Var {
                    net_type,
                    width,
                    token_idcode,
                    variable_description,
                    pos,
                }
            }
            // Empty blocks
            LexerToken::SectionUpScope(pos) => Token::UpScope(pos),
            LexerToken::SectionEndDefinitions(pos) => Token::EndDefinitions(pos),
            // Single word blocks
            LexerToken::CommandDumpAll(pos) => Token::DumpAll(pos),
            LexerToken::CommandDumpOff(pos) => Token::DumpOff(pos),
            LexerToken::CommandDumpOn(pos) => Token::DumpOn(pos),
            LexerToken::CommandDumpVars(pos) => Token::DumpVars(pos),
            LexerToken::CommandEnd(pos) => Token::End(pos),
            // Waveform events
            LexerToken::Timestamp(span, pos) => {
                Token::Timestamp(tokenize_timestamp(&self.bytes[span])?, pos)
            }
            LexerToken::ScalarZero(span, pos) => {
                let idcode = tokenize_idcode(bs, &self.bytes[span][1..]);
                Token::VectorValue(BitVector::new_zero_bit(), idcode, pos)
            }
            LexerToken::ScalarOne(span, pos) => {
                let idcode = tokenize_idcode(bs, &self.bytes[span][1..]);
                Token::VectorValue(BitVector::new_one_bit(), idcode, pos)
            }
            LexerToken::ScalarUnknown(span, pos) => {
                let idcode = tokenize_idcode(bs, &self.bytes[span][1..]);
                Token::VectorValue(BitVector::new_unknown_bit(), idcode, pos)
            }
            LexerToken::ScalarHighImpedance(span, pos) => {
                let idcode = tokenize_idcode(bs, &self.bytes[span][1..]);
                Token::VectorValue(BitVector::new_high_impedance_bit(), idcode, pos)
            }
            LexerToken::VectorValue(span, pos) => {
                let (vector, idcode) = tokenize_vector(bs, &self.bytes[span]);
                Token::VectorValue(vector, idcode, pos)
            }
            LexerToken::VectorValueFourState(span, pos) => {
                let (vector, idcode) = tokenize_vector_four_state(bs, &self.bytes[span]);
                Token::VectorValue(vector, idcode, pos)
            }
            LexerToken::RealValue(span, pos) => {
                let (real, idcode) = tokenize_real(bs, &self.bytes[span], pos)?;
                Token::RealValue(real, idcode, pos)
            }
        };
        Ok(Some(token))
    }
}
