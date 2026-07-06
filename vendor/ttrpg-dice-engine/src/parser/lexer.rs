use crate::error::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(i64),
    Dice,
    Plus,
    Minus,
    Star,
    LParen,
    RParen,
    BangBang,
    Bang,
    Gte,
    Gt,
    Lte,
    Lt,
    KeepHigh,
    KeepLow,
    DropHigh,
    DropLow,
    Fate,
    Percentile,
    RerollOnce,
    Reroll,
    MinVal,
    MaxVal,
    Eof,
}

pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok == Token::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let c = self.input.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat_if(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_if_icase(&mut self, lower: u8) -> bool {
        match self.peek() {
            Some(c) if c.to_ascii_lowercase() == lower => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn read_number(&mut self) -> Result<i64, ParseError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .expect("digit bytes are always valid UTF-8");
        s.parse::<i64>()
            .map_err(|_| ParseError::InvalidNumber(format!("number too large: {s}")))
    }

    fn next_token(&mut self) -> Result<Token, ParseError> {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }

        let Some(c) = self.peek() else {
            return Ok(Token::Eof);
        };

        match c {
            b'0'..=b'9' => Ok(Token::Number(self.read_number()?)),
            b'+' => {
                self.advance();
                Ok(Token::Plus)
            }
            b'-' => {
                self.advance();
                Ok(Token::Minus)
            }
            b'*' => {
                self.advance();
                Ok(Token::Star)
            }
            b'(' => {
                self.advance();
                Ok(Token::LParen)
            }
            b')' => {
                self.advance();
                Ok(Token::RParen)
            }
            b'%' => {
                self.advance();
                Ok(Token::Percentile)
            }
            b'!' => {
                self.advance();
                if self.eat_if(b'!') {
                    Ok(Token::BangBang)
                } else {
                    Ok(Token::Bang)
                }
            }
            b'>' => {
                self.advance();
                if self.eat_if(b'=') {
                    Ok(Token::Gte)
                } else {
                    Ok(Token::Gt)
                }
            }
            b'<' => {
                self.advance();
                if self.eat_if(b'=') {
                    Ok(Token::Lte)
                } else {
                    Ok(Token::Lt)
                }
            }
            b'd' | b'D' => {
                self.advance();
                let next = self.peek().map(|b| b.to_ascii_lowercase());
                match next {
                    Some(b'h') => {
                        self.advance();
                        Ok(Token::DropHigh)
                    }
                    Some(b'l') => {
                        self.advance();
                        Ok(Token::DropLow)
                    }
                    Some(b'r') => {
                        // 'dr' could be a drop-reroll alias in some systems. treat as DropLow
                        // for now (most systems use dl but need to research this further)
                        self.advance();
                        Ok(Token::DropLow)
                    }
                    _ => Ok(Token::Dice),
                }
            }
            b'k' | b'K' => {
                self.advance();
                let next = self.peek().map(|b| b.to_ascii_lowercase());
                match next {
                    Some(b'l') => {
                        self.advance();
                        Ok(Token::KeepLow)
                    }
                    Some(b'h') => {
                        self.advance();
                        Ok(Token::KeepHigh)
                    }
                    _ => Ok(Token::KeepHigh), // bare 'k' = keep highest
                }
            }
            b'f' | b'F' => {
                self.advance();
                Ok(Token::Fate)
            }
            b'r' | b'R' => {
                self.advance();
                if self.eat_if_icase(b'o') {
                    Ok(Token::RerollOnce)
                } else {
                    Ok(Token::Reroll)
                }
            }
            b'm' | b'M' => {
                self.advance();
                let next = self.peek().map(|b| b.to_ascii_lowercase());
                match next {
                    Some(b'i') => {
                        self.advance();
                        Ok(Token::MinVal)
                    }
                    Some(b'a') => {
                        self.advance();
                        Ok(Token::MaxVal)
                    }
                    _ => Err(ParseError::UnexpectedChar(c as char, self.pos)),
                }
            }
            _ => Err(ParseError::UnexpectedChar(c as char, self.pos)),
        }
    }
}
