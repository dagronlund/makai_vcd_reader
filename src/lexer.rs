pub mod position;

use core::ops::Range;

use std::str;

use logos::Logos;

use crate::lexer::position::*;

pub type ByteRange = Range<usize>;

fn count_newlines(lex: &mut logos::Lexer<LogosToken>) -> (usize, usize) {
    let mut newlines = 0;
    let mut columns = 0;
    for c in lex.slice().bytes() {
        if c == b'\n' {
            newlines += 1;
            columns = 1;
        } else {
            columns += 1;
        }
    }
    (newlines, columns)
}

#[derive(Logos, Debug, PartialEq)]
enum LogosToken {
    // Unformatted blocks
    #[regex(r"\$comment[^$]*\$+([^\$e][^\$]*\$+)*end", count_newlines)]
    SectionComment((usize, usize)),
    #[regex(r"\$date[^$]*\$+([^\$e][^\$]*\$+)*end", count_newlines)]
    SectionDate((usize, usize)),
    #[regex(r"\$version[^$]*\$+([^\$e][^\$]*\$+)*end", count_newlines)]
    SectionVersion((usize, usize)),
    // Formatted blocks
    #[regex(r"\$scope[\s]+[\S]+[\s]+[\S]+[\s]+\$end", count_newlines)]
    SectionScope((usize, usize)),
    #[regex(
        r"\$timescale[\s]+(1|10|100)[\s]*(fs|ps|ns|us|ms|s)[\s]+\$end",
        count_newlines
    )]
    SectionTimescale((usize, usize)),
    #[regex(
        r"\$var[\s]+[\S]+[\s]+[1-9][0-9_]*[\s]+[\x21-\x7E]+[\s]+[\S]+[\s]+(\[(0|([1-9][0-9_]*))([:](0|([1-9][0-9_]*)))?\][\s]+)?\$end",
        count_newlines
    )]
    SectionVar((usize, usize)),
    // Empty blocks
    #[regex(r"\$upscope[\s]*\$end", count_newlines)]
    SectionUpScope((usize, usize)),
    #[regex(r"\$enddefinitions[\s]*\$end", count_newlines)]
    SectionEndDefinitions((usize, usize)),
    // Simulation commands
    #[regex(r"\$dumpall")]
    CommandDumpAll,
    #[regex(r"\$dumpoff")]
    CommandDumpOff,
    #[regex(r"\$dumpon")]
    CommandDumpOn,
    #[regex(r"\$dumpvars")]
    CommandDumpVars,
    #[regex(r"\$end")]
    CommandEnd,
    // Simulation values
    #[regex(r"#[ ]*([0]|([1-9][0-9]*))")]
    Timestamp,
    #[regex(r"[0][\x21-\x7E]+")]
    ScalarZero,
    #[regex(r"[1][\x21-\x7E]+")]
    ScalarOne,
    #[regex(r"[xX][\x21-\x7E]+")]
    ScalarUnknown,
    #[regex(r"[zZ][\x21-\x7E]+")]
    ScalarHighImpedance,
    #[regex(r"[bB][01]+[ ]+[\x21-\x7E]+", priority = 1)]
    VectorValue,
    #[regex(r"[bB][01xXzZ]+[ ]+[\x21-\x7E]+", priority = 0)]
    VectorValueFourState,
    #[regex(r"[rR](([1-9][0-9]*|[0])[.][0-9]+)[ ]+[\x21-\x7E]+")]
    RealValue,
    // Whitespace
    #[token("\n")]
    NewLine,
    #[regex(r"[ \t\f]+")] // logos::skip
    Whitespace,
    // Error
    #[error]
    Error,
}

#[derive(Clone)]
pub enum LexerToken {
    SectionComment(ByteRange, LexerPosition),
    SectionDate(ByteRange, LexerPosition),
    SectionVersion(ByteRange, LexerPosition),
    SectionScope(ByteRange, LexerPosition),
    SectionTimescale(ByteRange, LexerPosition),
    SectionVar(ByteRange, LexerPosition),
    SectionUpScope(LexerPosition),
    SectionEndDefinitions(LexerPosition),
    CommandDumpAll(LexerPosition),
    CommandDumpOff(LexerPosition),
    CommandDumpOn(LexerPosition),
    CommandDumpVars(LexerPosition),
    CommandEnd(LexerPosition),
    Timestamp(ByteRange, LexerPosition),
    ScalarZero(ByteRange, LexerPosition),
    ScalarOne(ByteRange, LexerPosition),
    ScalarUnknown(ByteRange, LexerPosition),
    ScalarHighImpedance(ByteRange, LexerPosition),
    VectorValue(ByteRange, LexerPosition),
    VectorValueFourState(ByteRange, LexerPosition),
    RealValue(ByteRange, LexerPosition),
}

impl Default for LexerToken {
    fn default() -> Self {
        Self::CommandEnd(LexerPosition::new(0, 0, 0, 0))
    }
}

pub struct Lexer<'a> {
    lexer: logos::Lexer<'a, LogosToken>,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(s: &'a str) -> Self {
        Self {
            lexer: LogosToken::lexer(s),
            line: 1,
            column: 1,
        }
    }

    pub fn get_position(&self) -> LexerPosition {
        LexerPosition::new(
            self.lexer.span().start,
            self.line,
            self.column,
            self.lexer.span().len(),
        )
    }

    fn process_newlines(&mut self, newlines: usize, columns: usize) {
        if newlines != 0 {
            self.column = columns;
            self.line += newlines;
        }
    }

    pub fn next_token(&mut self) -> Result<Option<LexerToken>, LexerPosition> {
        loop {
            let next = self.lexer.next();
            let span = self.lexer.span();
            let pos = self.get_position();
            self.column += span.len();
            let logos_token = match next {
                Some(logos_token) => logos_token,
                None => return Ok(None),
            };
            let lexer_token = match logos_token {
                // Unformatted blocks
                LogosToken::SectionComment((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$comment".len())..(span.end - b"$end".len());
                    LexerToken::SectionComment(span, pos)
                }
                LogosToken::SectionDate((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$date".len())..(span.end - b"$end".len());
                    LexerToken::SectionDate(span, pos)
                }
                LogosToken::SectionVersion((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$version".len())..(span.end - b"$end".len());
                    LexerToken::SectionVersion(span, pos)
                }
                // Formatted blocks
                LogosToken::SectionScope((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$scope".len())..(span.end - b"$end".len());
                    LexerToken::SectionScope(span, pos)
                }
                LogosToken::SectionTimescale((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$timescale".len())..(span.end - b"$end".len());
                    LexerToken::SectionTimescale(span, pos)
                }
                LogosToken::SectionVar((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    let span = (span.start + b"$var".len())..(span.end - b"$end".len());
                    LexerToken::SectionVar(span, pos)
                }
                // Empty blocks
                LogosToken::SectionUpScope((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    LexerToken::SectionUpScope(pos)
                }
                LogosToken::SectionEndDefinitions((newlines, columns)) => {
                    self.process_newlines(newlines, columns);
                    LexerToken::SectionEndDefinitions(pos)
                }
                LogosToken::CommandDumpAll => LexerToken::CommandDumpAll(pos),
                LogosToken::CommandDumpOff => LexerToken::CommandDumpOff(pos),
                LogosToken::CommandDumpOn => LexerToken::CommandDumpOn(pos),
                LogosToken::CommandDumpVars => LexerToken::CommandDumpVars(pos),
                LogosToken::CommandEnd => LexerToken::CommandEnd(pos),
                LogosToken::Timestamp => LexerToken::Timestamp(span, pos),
                LogosToken::ScalarZero => LexerToken::ScalarZero(span, pos),
                LogosToken::ScalarOne => LexerToken::ScalarOne(span, pos),
                LogosToken::ScalarUnknown => LexerToken::ScalarUnknown(span, pos),
                LogosToken::ScalarHighImpedance => LexerToken::ScalarHighImpedance(span, pos),
                LogosToken::VectorValue => LexerToken::VectorValue(span, pos),
                LogosToken::VectorValueFourState => LexerToken::VectorValueFourState(span, pos),
                LogosToken::RealValue => LexerToken::RealValue(span, pos),
                LogosToken::Whitespace => continue,
                LogosToken::NewLine => {
                    self.process_newlines(1, 1);
                    continue;
                }
                LogosToken::Error => return Err(pos),
            };
            return Ok(Some(lexer_token));
        }
    }
}
