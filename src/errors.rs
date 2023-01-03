use crate::lexer::position::*;
use crate::tokenizer::token::*;

#[derive(Debug)]
pub enum TokenizerError {
    UnexpectedTermination(LexerPosition),
    IntegerParseError(std::num::ParseIntError, LexerPosition),
    ScalarParseError(LexerPosition),
    VectorParseError(LexerPosition),
    RealParseError(std::num::ParseFloatError, LexerPosition),
    IncorrectVariableWidth(usize, usize, LexerPosition),
    IncorrectRealWidth(LexerPosition),
    LexerError(LexerPosition),
}

impl From<LexerPosition> for TokenizerError {
    fn from(pos: LexerPosition) -> Self {
        TokenizerError::LexerError(pos)
    }
}

impl From<TokenizerError> for TokenizerResult<Token> {
    fn from(err: TokenizerError) -> Self {
        Err(err)
    }
}

pub type TokenizerResult<T> = Result<T, TokenizerError>;

#[derive(Debug)]
pub enum ParserError {
    UnexpectedTermination,
    Tokenizer(TokenizerError),
    UnexpectedToken(Token),
    UnexpectedUpscope(LexerPosition),
    UnexpectedEndDefinitions(LexerPosition),
    UnexpectedVariable(LexerPosition),
    UnmatchedIdcode(LexerPosition),
    MismatchedWidth(LexerPosition),
    Custom(String, Option<Token>),
}

impl From<TokenizerError> for ParserError {
    fn from(err: TokenizerError) -> Self {
        ParserError::Tokenizer(err)
    }
}

impl From<ParserError> for ParserResult<Token> {
    fn from(err: ParserError) -> Self {
        Err(err)
    }
}

pub type ParserResult<T> = Result<T, ParserError>;
