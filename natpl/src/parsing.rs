use crate::{
    syntax::{
        DeclarationLhs, DeclarationOrEquality, Expression, HasFC, Identifier, InfixOp, LineItem,
        PrefixOp, SiPrefix, FC,
    },
    tokenising::{Span, Token},
};
use fraction::BigDecimal;
use thiserror::Error;

pub type Result<'src, T> = core::result::Result<T, ParseError<'src>>;

#[derive(Debug, Clone, Error)]
pub enum ParseError<'src> {
    #[error("Unexpected end of input")]
    UnexpectedEnd,
    #[error("Unexpected token found at col {}: {:?}", .1.start + 1, .0)]
    UnexpectedToken(Token<'src>, FC),
}

pub struct Parser<'toks, 'src> {
    toks: &'toks [(Token<'src>, Span)],
}

impl<'toks, 'src> Parser<'toks, 'src> {
    pub fn parse_line(line_toks: &'toks [(Token<'src>, Span)]) -> Result<'src, LineItem> {
        let mut this = Self { toks: line_toks };

        if line_toks.is_empty() {
            return Ok(LineItem::Empty);
        }

        match this.parse_unit_declaration() {
            Ok((fc, name, None)) => return Ok(LineItem::UnitDeclaration(fc, name)),
            Ok((fc, name, Some(expr))) => return Ok(LineItem::UnitAlias(fc, name, expr)),
            Err(_) => {
                this.toks = line_toks;
            }
        }

        if let Ok((fc, name, unit_from, unit_to, body)) = this.parse_conversion_function() {
            return Ok(LineItem::ConversionDeclaration {
                fc,
                name,
                unit_from,
                unit_to,
                body,
            });
        } else {
            this.toks = line_toks;
        };

        if let Ok((fc, expr)) = this.parse_printed_expr() {
            return Ok(LineItem::PrintedExpression(fc, expr));
        } else {
            this.toks = line_toks;
        };

        match this.parse_search() {
            Ok((fc, None)) => return Ok(LineItem::VarSearch(fc)),
            Ok((_, Some(expr))) => return Ok(LineItem::UnitSearch(expr)),
            Err(_) => {
                this.toks = line_toks;
            }
        }

        if let Ok(decl_or_eq) = this.parse_declaration_or_equality() {
            return Ok(LineItem::MaybeDeclarationOrEqualityExpression(decl_or_eq));
        } else {
            this.toks = line_toks;
        };

        let expr = this.parse_expr()?;

        if let Some((t, span)) = this.toks.first() {
            Err(ParseError::UnexpectedToken(*t, span.into()))
        } else {
            Ok(LineItem::SilentExpression(expr))
        }
    }

    fn parse_unit_declaration(&mut self) -> Result<'src, (FC, Identifier, Option<Expression>)> {
        let fc = self.expect(|fc, t| {
            if matches!(t, Token::Unit) {
                Some(fc)
            } else {
                None
            }
        })?;
        let name = self.expect_identifier()?;

        let expr = if matches!(self.peek(), Some(Token::OpEq)) {
            self.next()?;
            Some(self.parse_expr()?)
        } else {
            None
        };

        if let Some((t, span)) = self.toks.first() {
            Err(ParseError::UnexpectedToken(*t, span.into()))
        } else {
            Ok((fc.merge(name.fc()), name, expr))
        }
    }

    fn parse_conversion_function(
        &mut self,
    ) -> Result<'src, (FC, Identifier, Expression, Expression, Expression)> {
        let (fc, ()) = self.expect_and_fc(|t| matches!(t, Token::Conv))?;
        let name = self.expect_identifier()?;

        let _ = self.expect_and_fc(|t| matches!(t, Token::Colon))?;
        let unit_from = self.parse_expr_atom()?;
        let _ = self.expect_and_fc(|t| matches!(t, Token::DArrowR))?;
        let unit_to = self.parse_expr_atom()?;
        let _ = self.expect_and_fc(|t| matches!(t, Token::OpEq))?;
        let body = self.parse_expr()?;

        Ok((fc, name, unit_from, unit_to, body))
    }

    fn parse_search(&mut self) -> Result<'src, (FC, Option<Expression>)> {
        let (fc, ()) = self.expect_and_fc(|t| matches!(t, Token::QuestionMark))?;

        if self.peek().is_some() {
            let rhs = self.parse_expr()?;
            Ok((fc, Some(rhs)))
        } else {
            Ok((fc, None))
        }
    }

    fn parse_declaration_or_equality(&mut self) -> Result<'src, DeclarationOrEquality> {
        let name = self.expect_identifier()?;
        let fc_start = name.fc();

        let lhs = match self.peek() {
            Some(Token::ParenOpen) => {
                // function call / definition
                let args = self.paren_list(|p| p.expect_identifier())?;
                DeclarationLhs::Function {
                    fc: name.fc().merge(args.fc),
                    name,
                    args: args.elems,
                }
            }
            _ => DeclarationLhs::Variable(name),
        };

        self.expect(|_, t| matches!(t, Token::OpEq))?;

        let rhs = self.parse_expr()?;

        let fc = fc_start.merge(rhs.fc());

        if let Some((t, span)) = self.toks.first() {
            Err(ParseError::UnexpectedToken(*t, span.into()))
        } else {
            Ok(DeclarationOrEquality { fc, lhs, rhs })
        }
    }

    fn parse_printed_expr(&mut self) -> Result<'src, (FC, Expression)> {
        let (fc, ()) = self.expect_and_fc(|t| matches!(t, Token::OpGt))?;
        let expr = self.parse_expr()?;
        if let Some((t, span)) = self.toks.first() {
            Err(ParseError::UnexpectedToken(*t, span.into()))
        } else {
            Ok((fc.merge(expr.fc()), expr))
        }
    }

    fn parse_expr(&mut self) -> Result<'src, Expression> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<'src, Expression> {
        let (t, span) = self.toks.first().ok_or(ParseError::UnexpectedEnd)?;

        let mut lhs = if let Some(((), bp)) = prefix_binding_power(*t) {
            self.next()?;
            let rhs = self.parse_expr_bp(bp)?;
            match *t {
                Token::OpAdd | Token::OpSub => {
                    let op = match *t {
                        Token::OpAdd => PrefixOp::Pos,
                        Token::OpSub => PrefixOp::Neg,
                        _ => unreachable!(),
                    };
                    Expression::PrefixOp {
                        fc: FC::from(span).merge(rhs.fc()),
                        op,
                        expr: Box::new(rhs),
                    }
                }
                Token::OpSqrt => {
                    let t_fc = FC::from_span(span.clone());
                    Expression::Call {
                        fc: t_fc.merge(rhs.fc()),
                        base: Box::new(Expression::Variable(Identifier(t_fc, "sqrt".to_string()))),
                        args: vec![rhs],
                    }
                }
                _ => unreachable!(),
            }
        } else {
            self.parse_expr_atom()?
        };

        #[allow(clippy::while_let_loop)]
        loop {
            let (fc, t) = match self.toks.first() {
                Some((t, span)) => (span.into(), *t),
                None => break,
            };

            if let Some((l_bp, ())) = postfix_binding_power(t) {
                if l_bp < min_bp {
                    break;
                }

                match t {
                    Token::ParenOpen => {
                        let args = self.paren_list(Self::parse_expr)?;
                        lhs = Expression::Call {
                            fc: lhs.fc().merge(args.fc),
                            base: Box::new(lhs),
                            args: args.elems,
                        };
                    }
                    Token::OpPowNum(n) => {
                        self.next()?;
                        lhs = Expression::InfixOp {
                            fc: lhs.fc().merge(fc),
                            op: InfixOp::Pow,
                            lhs: Box::new(lhs),
                            rhs: Box::new(Expression::IntegerLit {
                                fc,
                                val: BigDecimal::from(n),
                            }),
                        };
                    }
                    Token::BracketOpen => {
                        self.next()?;
                        let index = Box::new(self.parse_expr()?);
                        let (fc_end, ()) =
                            self.expect_and_fc(|t| matches!(t, Token::BracketClose))?;

                        lhs = Expression::Indexed {
                            fc: lhs.fc().merge(fc_end),
                            expr: Box::new(lhs),
                            index,
                        };
                    }
                    _ => unreachable!(),
                }
                continue;
            }

            if let Some((l_bp, r_bp, op)) = infix_binding_power(t) {
                if l_bp < min_bp {
                    break;
                }

                if !matches!(t, Token::Identifier(_)) {
                    self.next()?;
                }

                let rhs = self.parse_expr_bp(r_bp)?;

                lhs = Expression::InfixOp {
                    fc: lhs.fc().merge(rhs.fc()),
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
            } else {
                break;
            };
        }

        Ok(lhs)
    }

    fn parse_expr_atom(&mut self) -> Result<'src, Expression> {
        let (t, fc) = self.peek_with_fc().ok_or(ParseError::UnexpectedEnd)?;
        match t {
            Token::BracketOpen => {
                let _ = self.next()?;
                let expr = self.parse_expr()?;
                let (fc_end, ()) = self.expect_and_fc(|t| matches!(t, Token::BracketClose))?;
                Ok(Expression::UnitOf(fc.merge(fc_end), Box::new(expr)))
            }
            Token::ParenOpen => {
                let mut res = self.paren_list(Self::parse_expr)?;
                if res.elems.len() == 1 && !res.trailing_comma {
                    Ok(Expression::Parenthesised(
                        res.fc,
                        Box::new(res.elems.pop().unwrap()),
                    ))
                } else {
                    Ok(Expression::Vector(res.fc, res.elems))
                }
            }
            Token::Identifier(name) => {
                let _ = self.next()?;
                if let Some((prefix, stripped)) = identifier_maybe_unit_prefix(name) {
                    Ok(Expression::MaybeUnitPrefix {
                        fc,
                        prefix,
                        full_name: name.into(),
                        name: stripped.into(),
                    })
                } else {
                    Ok(Expression::Variable(Identifier(fc, name.into())))
                }
            }
            Token::IntegerLit(val) => {
                let _ = self.next()?;
                let val = BigDecimal::from_decimal_str(val).unwrap();
                Ok(Expression::IntegerLit { fc, val })
            }
            Token::FloatLit((int, dec)) => {
                let _ = self.next()?;
                Ok(Expression::FloatLit {
                    fc,
                    val: BigDecimal::from_decimal_str(&format!("{}.{}", int, dec)).unwrap(),
                })
            }
            Token::ScientificFloatLit((int, dec, exp)) => {
                let _ = self.next()?;
                Ok(Expression::FloatLit {
                    fc,
                    val: crate::num::from_decimal_str_and_exp(
                        &format!("{}.{}", int, dec),
                        exp as _,
                    ),
                })
            }
            Token::ScientificIntegerLit((val, exp)) => {
                let _ = self.next()?;
                Ok(Expression::FloatLit {
                    fc,
                    val: crate::num::from_decimal_str_and_exp(&val.to_string(), exp as _),
                })
            }
            t => {
                let _ = self.next()?;
                Err(ParseError::UnexpectedToken(t, fc))
            }
        }
    }

    fn expect_identifier(&mut self) -> Result<'src, Identifier> {
        self.expect(|fc, t| match t {
            Token::Identifier(name) => Some(Identifier(fc, name.into())),
            _ => None,
        })
    }

    fn paren_list<T>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<'src, T>,
    ) -> Result<'src, ParenListParseResult<T>> {
        let (fc, ()) = self.expect_and_fc(|t| matches!(t, Token::ParenOpen))?;

        let mut vals = vec![];
        let mut trailing_comma = false;

        loop {
            if let Some((Token::ParenClose, span)) = self.toks.first() {
                self.next()?;
                return Ok(ParenListParseResult {
                    fc: fc.merge(FC::from(span)),
                    elems: vals,
                    trailing_comma,
                });
            }

            vals.push(f(self)?);
            trailing_comma = false;

            let (fc_, end) = self.expect(|fc, t| {
                if matches!(t, Token::ParenClose) {
                    Some((fc, true))
                } else if matches!(t, Token::Comma) {
                    trailing_comma = true;
                    Some((fc, false))
                } else {
                    None
                }
            })?;

            if end {
                return Ok(ParenListParseResult {
                    fc: fc.merge(fc_),
                    elems: vals,
                    trailing_comma,
                });
            }
        }
    }

    fn peek(&self) -> Option<Token<'src>> {
        match self.toks {
            [] => None,
            [(t, _), ..] => Some(*t),
        }
    }

    fn peek_with_fc(&self) -> Option<(Token<'src>, FC)> {
        match self.toks {
            [] => None,
            [(t, span), _rest @ ..] => Some((*t, FC::from_span(span.clone()))),
        }
    }

    fn next(&mut self) -> Result<'src, Token<'src>> {
        match self.toks {
            [] => Err(ParseError::UnexpectedEnd),
            [(t, _), rest @ ..] => {
                self.toks = rest;
                Ok(*t)
            }
        }
    }

    fn expect<T: ExpectRet>(
        &mut self,
        f: impl FnOnce(FC, Token<'src>) -> T,
    ) -> Result<'src, T::RetType> {
        match self.toks {
            [] => Err(ParseError::UnexpectedEnd),
            [(t, span), rest @ ..] => {
                let fc = FC::from(span);
                match f(fc, *t).into_option() {
                    Some(res) => {
                        self.toks = rest;
                        Ok(res)
                    }
                    None => Err(ParseError::UnexpectedToken(*t, fc)),
                }
            }
        }
    }

    fn expect_and_fc<T: ExpectRet>(
        &mut self,
        f: impl FnOnce(Token<'src>) -> T,
    ) -> Result<'src, (FC, T::RetType)> {
        match self.toks {
            [] => Err(ParseError::UnexpectedEnd),
            [(t, span), rest @ ..] => {
                let fc = FC::from(span);
                match f(*t).into_option() {
                    Some(res) => {
                        self.toks = rest;
                        Ok((fc, res))
                    }
                    None => Err(ParseError::UnexpectedToken(*t, fc)),
                }
            }
        }
    }
}

fn prefix_binding_power(t: Token<'_>) -> Option<((), u8)> {
    match t {
        Token::OpAdd => Some(((), 100)),
        Token::OpSub => Some(((), 100)),
        Token::OpSqrt => Some(((), 120)),
        _ => None,
    }
}

fn infix_binding_power(t: Token<'_>) -> Option<(u8, u8, InfixOp)> {
    match t {
        Token::OpPow => Some((151, 150, InfixOp::Pow)),
        Token::Identifier(_) => Some((90, 91, InfixOp::Mul)),
        Token::OpMul => Some((80, 81, InfixOp::Mul)),
        Token::OpDiv => Some((80, 81, InfixOp::Div)),
        Token::OpMod => Some((80, 81, InfixOp::Mod)),
        Token::OpAdd => Some((70, 71, InfixOp::Add)),
        Token::OpSub => Some((70, 71, InfixOp::Sub)),

        Token::OpIn => Some((50, 51, InfixOp::In)),

        Token::OpEq => Some((20, 21, InfixOp::Eq)),
        Token::OpNeq => Some((20, 21, InfixOp::Neq)),
        Token::OpGt => Some((20, 21, InfixOp::Gt)),

        _ => None,
    }
}

fn postfix_binding_power(t: Token<'_>) -> Option<(u8, ())> {
    match t {
        Token::ParenOpen => Some((255, ())),
        Token::OpPowNum(_) => Some((91, ())),
        Token::BracketOpen => Some((255, ())),
        _ => None,
    }
}

fn identifier_maybe_unit_prefix(name: &str) -> Option<(SiPrefix, &str)> {
    use SiPrefix::*;

    const PREFIXES: &[(&str, SiPrefix)] = &[
        // long form
        ("femto", Femto),
        ("pico", Pico),
        ("nano", Nano),
        ("micro", Micro),
        ("milli", Milli),
        ("centi", Centi),
        ("deci", Deci),
        ("deca", Deca),
        ("hecto", Hecto),
        ("kilo", Kilo),
        ("mega", Mega),
        ("giga", Giga),
        ("tera", Tera),
        ("peta", Peta),
        // short form
        ("f", Femto),
        ("p", Pico),
        ("n", Nano),
        ("μ", Micro),
        ("m", Milli),
        ("c", Centi),
        ("d", Deci),
        ("da", Deca),
        ("h", Hecto),
        ("k", Kilo),
        ("M", Mega),
        ("G", Giga),
        ("T", Tera),
        ("P", Peta),
    ];

    for (prefix, si) in PREFIXES {
        match name.strip_prefix(prefix) {
            Some("") => continue,
            Some(n) => return Some((*si, n)),
            None => continue,
        }
    }
    None
}

struct ParenListParseResult<T> {
    fc: FC,
    elems: Vec<T>,
    trailing_comma: bool,
}

trait ExpectRet {
    type RetType;

    fn into_option(self) -> Option<Self::RetType>;
}

impl<T> ExpectRet for Option<T> {
    type RetType = T;

    fn into_option(self) -> Option<Self::RetType> {
        self
    }
}

impl ExpectRet for bool {
    type RetType = ();

    fn into_option(self) -> Option<Self::RetType> {
        if self {
            Some(())
        } else {
            None
        }
    }
}
