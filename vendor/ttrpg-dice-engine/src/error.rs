use thiserror::Error;

/// errors that can occur while parsing dice notation
#[non_exhaustive]
#[derive(Debug, Error, Clone)]
pub enum ParseError {
    #[error("unexpected character '{0}' at position {1}")]
    UnexpectedChar(char, usize),

    #[error("unexpected end of input")]
    UnexpectedEof,

    #[error("unexpected token {0:?} at position {1}, expected {2}")]
    UnexpectedToken(String, usize, String),

    #[error("invalid number: {0}")]
    InvalidNumber(String),

    #[error("dice count must be positive, got {0}")]
    InvalidDiceCount(i64),

    #[error("dice sides must be positive, got {0}")]
    InvalidDiceSides(i64),
}

/// errors that can occur during expression evaluation or distribution computation
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),

    /// too complex - this is returned for expressions such as (d6) * (d8) where both sides of a
    /// multiplication are random, or for keep/drop on more than 100 dice
    #[error("expression too complex for exact computation")]
    TooComplex,

    ///idk what you did but please don't do it again
    #[error("probability computation failed: {0}")]
    Probability(String),
}
