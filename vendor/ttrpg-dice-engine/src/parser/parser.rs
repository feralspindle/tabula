use super::ast::*;
use super::lexer::{Lexer, Token};
use crate::error::ParseError;

pub const MAX_DICE_COUNT: u32 = 1_000;
pub const MAX_DICE_SIDES: u32 = 1_000;

pub fn parse(input: &str) -> Result<DiceExpr, ParseError> {
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    if parser.peek() != &Token::Eof {
        return Err(ParseError::UnexpectedToken(
            format!("{:?}", parser.peek()),
            parser.pos,
            "end of input".into(),
        ));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek2(&self) -> &Token {
        self.tokens.get(self.pos + 1).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = self.tokens.get(self.pos).unwrap_or(&Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect_number(&mut self) -> Result<i64, ParseError> {
        match self.advance().clone() {
            Token::Number(n) => Ok(n),
            tok => Err(ParseError::UnexpectedToken(
                format!("{tok:?}"),
                self.pos,
                "number".into(),
            )),
        }
    }

    fn expect_u32(&mut self, context: &str) -> Result<u32, ParseError> {
        let n = self.expect_number()?;
        if n < 0 {
            return Err(ParseError::InvalidNumber(format!(
                "{context} cannot be negative: {n}"
            )));
        }
        if n > u32::MAX as i64 {
            return Err(ParseError::InvalidNumber(format!(
                "{context} is too large: {n}"
            )));
        }
        Ok(n as u32)
    }

    fn parse_expr(&mut self) -> Result<DiceExpr, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek().clone() {
                Token::Plus => {
                    self.advance();
                    let right = self.parse_term()?;
                    left = DiceExpr::Add(Box::new(left), Box::new(right));
                }
                Token::Minus => {
                    self.advance();
                    let right = self.parse_term()?;
                    left = DiceExpr::Sub(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<DiceExpr, ParseError> {
        let mut left = self.parse_unary()?;
        while self.peek() == &Token::Star {
            self.advance();
            let right = self.parse_unary()?;
            left = DiceExpr::Mul(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<DiceExpr, ParseError> {
        if self.peek() == &Token::Minus {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(DiceExpr::Neg(Box::new(inner)));
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<DiceExpr, ParseError> {
        match self.peek().clone() {
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                match self.advance().clone() {
                    Token::RParen => Ok(expr),
                    tok => Err(ParseError::UnexpectedToken(
                        format!("{tok:?}"),
                        self.pos,
                        "')'".into(),
                    )),
                }
            }
            Token::Number(n) => {
                if self.peek2() == &Token::Dice {
                    self.parse_dice_expr()
                } else {
                    self.advance();
                    Ok(DiceExpr::Literal(n))
                }
            }
            Token::Dice => self.parse_dice_expr(),
            Token::Fate => {
                self.advance();
                Ok(DiceExpr::Dice(DiceNode {
                    count: 1,
                    sides: DiceSides::Fate,
                    modifiers: vec![],
                }))
            }
            tok => Err(ParseError::UnexpectedToken(
                format!("{tok:?}"),
                self.pos,
                "number, '(' or dice expression".into(),
            )),
        }
    }

    fn parse_dice_expr(&mut self) -> Result<DiceExpr, ParseError> {
        let count = match self.peek().clone() {
            Token::Number(n) => {
                self.advance();
                if n <= 0 || n > MAX_DICE_COUNT as i64 {
                    return Err(ParseError::InvalidDiceCount(n));
                }
                n as u32
            }
            Token::Dice => 1, // bare 'd' means 1dX
            _ => 1,
        };

        match self.advance().clone() {
            Token::Dice => {}
            tok => {
                return Err(ParseError::UnexpectedToken(
                    format!("{tok:?}"),
                    self.pos,
                    "'d'".into(),
                ))
            }
        }

        let sides = match self.peek().clone() {
            Token::Fate => {
                self.advance();
                DiceSides::Fate
            }
            Token::Percentile => {
                self.advance();
                DiceSides::Percentile
            }
            Token::Number(n) => {
                self.advance();
                if n <= 0 || n > MAX_DICE_SIDES as i64 {
                    return Err(ParseError::InvalidDiceSides(n));
                }
                DiceSides::Numeric(n as u32)
            }
            tok => {
                return Err(ParseError::UnexpectedToken(
                    format!("{tok:?}"),
                    self.pos,
                    "die sides (number, F, or %)".into(),
                ))
            }
        };

        let modifiers = self.parse_modifiers(&sides)?;

        Ok(DiceExpr::Dice(DiceNode {
            count,
            sides,
            modifiers,
        }))
    }

    fn parse_modifiers(&mut self, sides: &DiceSides) -> Result<Vec<DiceModifier>, ParseError> {
        let mut mods = Vec::new();
        loop {
            let next = self.peek().clone();
            match next {
                Token::BangBang => {
                    self.advance();
                    mods.push(DiceModifier::ExplodeCompounding);
                }
                Token::Bang => {
                    self.advance();
                    if let Some(cp) = self.try_parse_compare_point()? {
                        let threshold = u32::try_from(cp.value).map_err(|_| {
                            ParseError::InvalidNumber(format!(
                                "explosion threshold must be a positive number: {}",
                                cp.value
                            ))
                        })?;
                        mods.push(DiceModifier::ExplodeGte(threshold));
                    } else {
                        mods.push(DiceModifier::ExplodeOnMax);
                    }
                }
                Token::KeepHigh => {
                    self.advance();
                    let n = self.expect_u32("keep highest count")?;
                    mods.push(DiceModifier::KeepHighest(n));
                }
                Token::KeepLow => {
                    self.advance();
                    let n = self.expect_u32("keep lowest count")?;
                    mods.push(DiceModifier::KeepLowest(n));
                }
                Token::DropHigh => {
                    self.advance();
                    let n = self.expect_u32("drop highest count")?;
                    mods.push(DiceModifier::DropHighest(n));
                }
                Token::DropLow => {
                    self.advance();
                    let n = self.expect_u32("drop lowest count")?;
                    mods.push(DiceModifier::DropLowest(n));
                }
                Token::RerollOnce => {
                    self.advance();
                    let cp = self.parse_compare_point_or_default(sides)?;
                    mods.push(DiceModifier::RerollOnce(cp));
                }
                Token::Reroll => {
                    self.advance();
                    let cp = self.parse_compare_point_or_default(sides)?;
                    mods.push(DiceModifier::RerollAlways(cp));
                }
                Token::MinVal => {
                    self.advance();
                    let n = self.expect_number()?;
                    mods.push(DiceModifier::MinValue(n));
                }
                Token::MaxVal => {
                    self.advance();
                    let n = self.expect_number()?;
                    mods.push(DiceModifier::MaxValue(n));
                }
                Token::Gte | Token::Gt | Token::Lte | Token::Lt => {
                    let cp = self.parse_compare_point_forced()?;
                    mods.push(DiceModifier::CountSuccess(cp));
                }
                _ => break,
            }
        }
        Ok(mods)
    }

    fn try_parse_compare_point(&mut self) -> Result<Option<ComparePoint>, ParseError> {
        let op = match self.peek().clone() {
            Token::Gte => CompareOp::Gte,
            Token::Gt => CompareOp::Gt,
            Token::Lte => CompareOp::Lte,
            Token::Lt => CompareOp::Lt,
            _ => return Ok(None),
        };
        self.advance();
        let value = self.expect_number()?;
        Ok(Some(ComparePoint { op, value }))
    }

    fn parse_compare_point_forced(&mut self) -> Result<ComparePoint, ParseError> {
        let op = match self.advance().clone() {
            Token::Gte => CompareOp::Gte,
            Token::Gt => CompareOp::Gt,
            Token::Lte => CompareOp::Lte,
            Token::Lt => CompareOp::Lt,
            tok => {
                return Err(ParseError::UnexpectedToken(
                    format!("{tok:?}"),
                    self.pos,
                    "comparison operator".into(),
                ))
            }
        };
        let value = self.expect_number()?;
        Ok(ComparePoint { op, value })
    }

    fn parse_compare_point_or_default(
        &mut self,
        _sides: &DiceSides,
    ) -> Result<ComparePoint, ParseError> {
        if let Some(cp) = self.try_parse_compare_point()? {
            Ok(cp)
        } else if let Token::Number(n) = self.peek().clone() {
            self.advance();
            Ok(ComparePoint {
                op: CompareOp::Eq,
                value: n,
            })
        } else {
            Ok(ComparePoint {
                op: CompareOp::Eq,
                value: 1,
            })
        }
    }
}
