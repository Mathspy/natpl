use logos::Logos;

pub type Span = logos::Span;

#[derive(Logos, Debug, Copy, Clone)]
pub enum Token<'input> {
    #[token("unit")]
    Unit,

    #[token("conv")]
    Conv,

    #[token(":")]
    Colon,

    #[token("⇒")]
    #[token("=>")]
    DArrowR,

    #[token("?")]
    QuestionMark,

    #[token("=")]
    OpEq,

    #[token("≠")]
    #[token("/=")]
    #[token("!=")]
    #[token("<>")]
    OpNeq,

    #[token(">")]
    OpGt,

    #[token("+")]
    OpAdd,

    #[token("-")]
    OpSub,

    #[token("*")]
    OpMul,

    #[token("/")]
    #[token("÷")]
    OpDiv,

    #[token("in")]
    OpIn,

    #[token("mod")]
    OpMod,

    #[token("^")]
    OpPow,

    #[regex(r"(⁰|¹|²|³|⁴|⁵|⁶|⁷|⁸|⁹)+", |lex| unicode_power_num(lex.slice()))]
    OpPowNum(u64),

    #[token("√")]
    OpSqrt,

    #[token("[")]
    BracketOpen,

    #[token("]")]
    BracketClose,

    #[token("(")]
    ParenOpen,

    #[token(")")]
    ParenClose,

    #[token(",")]
    Comma,

    #[regex("(\\p{XID_Start}|_|°)(\\p{XID_Continue}|'|°)*")]
    Identifier(&'input str),

    #[regex("[0-9]+")]
    IntegerLit(&'input str),

    #[regex(r"[0-9]+\.[0-9]+", |lex| {
        let mut parts = lex.slice().split('.');
        let integer = parts.next()?;
        let decimal = parts.next()?;
        Some((integer, decimal))
    })]
    FloatLit((&'input str, &'input str)),

    #[regex(r"[0-9]+\.[0-9]+x10\^-?[0-9]+", |lex| {
        let mut s = lex.slice().split("x10^");
        let (integer, decimal) = {
            let s = s.next()?;
            let mut parts = s.split('.');
            let integer = parts.next()?;
            let decimal = parts.next()?;
            (integer, decimal)
        };
        let exp = s.next()?.parse().ok()?;
        Some((integer, decimal, exp))
    })]
    #[regex(r"[0-9]+\.[0-9]+x10(⁰|¹|²|³|⁴|⁵|⁶|⁷|⁸|⁹)+", |lex| {
        let mut s = lex.slice().split("x10");
        let (integer, decimal) = {
            let s = s.next()?;
            let mut parts = s.split('.');
            let integer = parts.next()?;
            let decimal = parts.next()?;
            (integer, decimal)
        };
        let exp = unicode_power_num(s.next()?)?;
        Some((integer, decimal, exp as i64))
    })]
    ScientificFloatLit((&'input str, &'input str, i64)),

    #[regex(r"[0-9]+x10\^(-)?[0-9]+", |lex| {
        let mut s = lex.slice().split("x10^");
        let mantissa = s.next()?;
        let exp = s.next()?.parse().ok()?;
        Some((mantissa, exp))
    })]
    #[regex(r"[0-9]++x10(⁰|¹|²|³|⁴|⁵|⁶|⁷|⁸|⁹)+", |lex| {
        let mut s = lex.slice().split("x10");
        let mantissa = s.next()?;
        let exp = unicode_power_num(s.next()?)?;
        Some((mantissa, exp as i64))
    })]
    ScientificIntegerLit((&'input str, i64)),

    #[error]
    // skip whitespace
    #[regex(r"[ \t\n\f]+", logos::skip)]
    // line comments
    #[regex(r"#[^\n]*", logos::skip)]
    Error,
}

fn unicode_power_num(input: &str) -> Option<u64> {
    fn unicode_pow_to_digit_val(c: char) -> Option<u64> {
        match c {
            '⁰' => Some(0),
            '¹' => Some(1),
            '²' => Some(2),
            '³' => Some(3),
            '⁴' => Some(4),
            '⁵' => Some(5),
            '⁶' => Some(6),
            '⁷' => Some(7),
            '⁸' => Some(8),
            '⁹' => Some(9),
            _ => None,
        }
    }

    input.chars().try_fold(0u64, |acc, c| {
        acc.checked_mul(10)?
            .checked_add(unicode_pow_to_digit_val(c)?)
    })
}

pub fn tokenise(line: &'_ str) -> Vec<(Token<'_>, Span)> {
    Token::lexer(line).spanned().collect()
}
