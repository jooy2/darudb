//! The query language: text parsed into the same IR a [`Query`] builds
//! (`design/objects.md`, "The query language").
//!
//! [`Query`]: super::Query

use super::ir::{Expr, Ir, MAX_DEPTH, Op, Operand};
use crate::error::{Error, Result};
use crate::format::object::Value;

/// How deeply parentheses and `NOT` may nest in the text. The IR's own limit
/// is checked when the query runs; this one keeps the parser's recursion
/// bounded before that.
const MAX_NESTING: usize = 2 * MAX_DEPTH;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A word: a name or a keyword, which the parser tells apart.
    Word(String),
    /// A name in backticks, never a keyword.
    Quoted(String),
    String(String),
    Int(i64),
    Float(f64),
    Parameter(usize),
    Symbol(&'static str),
    End,
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Word(word) => format!("`{word}`"),
            Token::Quoted(name) => format!("`{name}`"),
            Token::String(_) => "a string".to_owned(),
            Token::Int(_) | Token::Float(_) => "a number".to_owned(),
            Token::Parameter(index) => format!("`${index}`"),
            Token::Symbol(symbol) => format!("`{symbol}`"),
            Token::End => "the end".to_owned(),
        }
    }
}

const SYMBOLS: [&str; 12] = [
    "==", "!=", "<=", ">=", "<", ">", "(", ")", "[", "]", ",", ".",
];

const KEYWORDS: [&str; 17] = [
    "AND",
    "OR",
    "NOT",
    "BETWEEN",
    "IN",
    "CONTAINS",
    "STARTSWITH",
    "ENDSWITH",
    "IS",
    "NULL",
    "TRUE",
    "FALSE",
    "SORT",
    "BY",
    "ASC",
    "DESC",
    "LIMIT",
];

pub(crate) fn is_keyword(word: &str) -> bool {
    word.eq_ignore_ascii_case("OFFSET")
        || KEYWORDS
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword))
}

fn invalid(at: usize, message: impl Into<String>) -> Error {
    Error::InvalidQuery {
        message: format!("at character {}: {}", at + 1, message.into()),
    }
}

/// The tokens of `text`, each with the character it starts at.
fn tokens(text: &str) -> Result<Vec<(Token, usize)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;

    while at < chars.len() {
        let start = at;
        let c = chars[at];

        if c.is_whitespace() {
            at += 1;
            continue;
        }

        let token = if c.is_alphabetic() || c == '_' {
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
            }

            Token::Word(chars[start..at].iter().collect())
        } else if c == '`' {
            let end = chars[at + 1..]
                .iter()
                .position(|&c| c == '`')
                .ok_or_else(|| invalid(start, "a name in backticks does not end"))?;
            let name: String = chars[at + 1..at + 1 + end].iter().collect();

            if name.is_empty() {
                return Err(invalid(start, "a name in backticks is empty"));
            }

            at += end + 2;
            Token::Quoted(name)
        } else if c == '"' {
            let (string, next) = string(&chars, at)?;

            at = next;
            Token::String(string)
        } else if c.is_ascii_digit()
            || (c == '-' && chars.get(at + 1).is_some_and(char::is_ascii_digit))
        {
            let (number, next) = number(&chars, at)?;

            at = next;
            number
        } else if c == '$' {
            at += 1;

            while at < chars.len() && chars[at].is_ascii_digit() {
                at += 1;
            }

            let digits: String = chars[start + 1..at].iter().collect();

            Token::Parameter(
                digits
                    .parse()
                    .map_err(|_| invalid(start, "`$` is followed by the number of a parameter"))?,
            )
        } else {
            let rest: String = chars[at..chars.len().min(at + 2)].iter().collect();
            let symbol = SYMBOLS
                .iter()
                .find(|symbol| rest.starts_with(**symbol))
                .ok_or_else(|| invalid(start, format!("`{c}` has no meaning here")))?;

            at += symbol.len();
            Token::Symbol(symbol)
        };

        out.push((token, start));
    }

    out.push((Token::End, chars.len()));

    Ok(out)
}

/// The string literal at `start`, and where it ends.
fn string(chars: &[char], start: usize) -> Result<(String, usize)> {
    let mut out = String::new();
    let mut at = start + 1;

    loop {
        match chars.get(at) {
            None => return Err(invalid(start, "a string does not end")),
            Some('"') => return Ok((out, at + 1)),
            Some('\\') => {
                let escaped = match chars.get(at + 1) {
                    Some('"') => '"',
                    Some('\\') => '\\',
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('u') if chars.get(at + 2) == Some(&'{') => {
                        let end = chars[at + 3..]
                            .iter()
                            .position(|&c| c == '}')
                            .ok_or_else(|| invalid(at, "a `\\u{...}` escape does not end"))?;
                        let hex: String = chars[at + 3..at + 3 + end].iter().collect();
                        let escaped = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or_else(|| {
                                invalid(at, format!("`\\u{{{hex}}}` is not a character"))
                            })?;

                        out.push(escaped);
                        at += end + 4;
                        continue;
                    }
                    _ => {
                        return Err(invalid(
                            at,
                            "a string escapes only `\\\"`, `\\\\`, `\\n`, `\\t` and `\\u{...}`",
                        ));
                    }
                };

                out.push(escaped);
                at += 2;
            }
            Some(&c) => {
                out.push(c);
                at += 1;
            }
        }
    }
}

/// The number at `start`: an int, or a float if it has a point or an
/// exponent. Returns it and where it ends.
fn number(chars: &[char], start: usize) -> Result<(Token, usize)> {
    let mut at = start;
    let mut float = false;
    let digits = |chars: &[char], mut at: usize| {
        while at < chars.len() && chars[at].is_ascii_digit() {
            at += 1;
        }

        at
    };

    if chars[at] == '-' {
        at += 1;
    }

    at = digits(chars, at);

    if chars.get(at) == Some(&'.') && chars.get(at + 1).is_some_and(char::is_ascii_digit) {
        float = true;
        at = digits(chars, at + 1);
    }

    if matches!(chars.get(at), Some('e' | 'E')) {
        let mut after = at + 1;

        if matches!(chars.get(after), Some('+' | '-')) {
            after += 1;
        }

        if chars.get(after).is_some_and(char::is_ascii_digit) {
            float = true;
            at = digits(chars, after);
        }
    }

    let text: String = chars[start..at].iter().collect();
    let token = if float {
        Token::Float(
            text.parse()
                .map_err(|_| invalid(start, format!("`{text}` is not a number")))?,
        )
    } else {
        Token::Int(
            text.parse()
                .map_err(|_| invalid(start, format!("`{text}` does not fit in 64 bits")))?,
        )
    };

    Ok((token, at))
}

struct Parser<'a> {
    tokens: Vec<(Token, usize)>,
    at: usize,
    /// The parameters' values, or `None` to keep them as parameters, for a
    /// prepared query.
    parameters: Option<&'a [Value]>,
    nesting: usize,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.at].0
    }

    fn position(&self) -> usize {
        self.tokens[self.at].1
    }

    /// Moves past the next token, unless it is the end.
    fn advance(&mut self) {
        if self.at + 1 < self.tokens.len() {
            self.at += 1;
        }
    }

    /// Whether the next token is the keyword `keyword`.
    fn at_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Token::Word(word) if word.eq_ignore_ascii_case(keyword))
    }

    /// Takes the keyword `keyword` if it is next.
    fn keyword(&mut self, keyword: &str) -> bool {
        let found = self.at_keyword(keyword);

        if found {
            self.advance();
        }

        found
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<()> {
        if self.keyword(keyword) {
            Ok(())
        } else {
            Err(self.expected(&format!("`{keyword}`")))
        }
    }

    fn symbol(&mut self, symbol: &str) -> bool {
        let found = matches!(self.peek(), Token::Symbol(next) if *next == symbol);

        if found {
            self.advance();
        }

        found
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<()> {
        if self.symbol(symbol) {
            Ok(())
        } else {
            Err(self.expected(&format!("`{symbol}`")))
        }
    }

    fn expected(&self, what: &str) -> Error {
        invalid(
            self.position(),
            format!("expected {what}, found {}", self.peek().describe()),
        )
    }

    fn query(&mut self) -> Result<Ir> {
        let mut ir = Ir::default();

        if !matches!(self.peek(), Token::End)
            && !self.at_keyword("SORT")
            && !self.at_keyword("LIMIT")
            && !self.at_keyword("OFFSET")
        {
            ir.filter = Some(self.or()?);
        }

        if self.keyword("SORT") {
            self.expect_keyword("BY")?;

            loop {
                let path = self.path()?;
                let descending = if self.keyword("DESC") {
                    true
                } else {
                    self.keyword("ASC");
                    false
                };

                ir.sort.push((path, descending));

                if !self.symbol(",") {
                    break;
                }
            }
        }

        if self.keyword("LIMIT") {
            ir.limit = Some(self.count()?);
        }

        if self.keyword("OFFSET") {
            ir.offset = self.count()?;
        }

        match self.peek() {
            Token::End => Ok(ir),
            _ => Err(self.expected("`AND`, `OR`, `SORT BY`, `LIMIT`, `OFFSET` or the end")),
        }
    }

    fn count(&mut self) -> Result<u64> {
        let Token::Int(value) = *self.peek() else {
            return Err(self.expected("a number"));
        };
        let count = u64::try_from(value)
            .map_err(|_| invalid(self.position(), "a limit or an offset is not negative"))?;

        self.advance();

        Ok(count)
    }

    fn or(&mut self) -> Result<Expr> {
        let mut terms = vec![self.and()?];

        while self.keyword("OR") {
            terms.push(self.and()?);
        }

        Ok(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::or(terms)
        })
    }

    fn and(&mut self) -> Result<Expr> {
        let mut terms = vec![self.not()?];

        while self.keyword("AND") {
            terms.push(self.not()?);
        }

        Ok(if terms.len() == 1 {
            terms.remove(0)
        } else {
            Expr::and(terms)
        })
    }

    fn not(&mut self) -> Result<Expr> {
        self.nesting += 1;

        if self.nesting > MAX_NESTING {
            return Err(invalid(
                self.position(),
                format!("the filter nests more than {MAX_NESTING} levels deep"),
            ));
        }

        let expr = if self.keyword("NOT") {
            Expr::Not(Box::new(self.not()?))
        } else if self.symbol("(") {
            let expr = self.or()?;

            self.expect_symbol(")")?;
            expr
        } else {
            self.condition()?
        };

        self.nesting -= 1;

        Ok(expr)
    }

    fn condition(&mut self) -> Result<Expr> {
        let path = self.path()?;
        let op = match self.peek().clone() {
            Token::Symbol("==") => Op::Eq,
            Token::Symbol("!=") => Op::Ne,
            Token::Symbol("<") => Op::Lt,
            Token::Symbol("<=") => Op::Le,
            Token::Symbol(">") => Op::Gt,
            Token::Symbol(">=") => Op::Ge,
            Token::Word(word) => match word.to_ascii_uppercase().as_str() {
                "BETWEEN" => {
                    self.advance();

                    let low = self.value()?;

                    self.expect_keyword("AND")?;

                    let high = self.value()?;

                    return Ok(Expr::operands(Op::Between, path, vec![low, high]));
                }
                "IN" => {
                    self.advance();
                    self.expect_symbol("[")?;

                    let mut values = Vec::new();

                    if !self.symbol("]") {
                        loop {
                            values.push(self.value()?);

                            if self.symbol("]") {
                                break;
                            }

                            self.expect_symbol(",")?;
                        }
                    }

                    return Ok(Expr::operands(Op::In, path, values));
                }
                "IS" => {
                    self.advance();

                    let not = self.keyword("NOT");

                    self.expect_keyword("NULL")?;

                    let test = Expr::test(Op::IsNull, path, Vec::new());

                    return Ok(if not { Expr::Not(Box::new(test)) } else { test });
                }
                "CONTAINS" => Op::Contains,
                "STARTSWITH" => Op::StartsWith,
                "ENDSWITH" => Op::EndsWith,
                _ => return Err(self.expected("a comparison")),
            },
            _ => return Err(self.expected("a comparison")),
        };

        self.advance();

        let value = self.value()?;

        Ok(Expr::operands(op, path, vec![value]))
    }

    fn path(&mut self) -> Result<Vec<String>> {
        let mut path = vec![match self.peek() {
            Token::Word(word) if !is_keyword(word) => word.clone(),
            Token::Quoted(name) => name.clone(),
            _ => return Err(self.expected("a field name")),
        }];

        self.advance();

        // After a `.`, a keyword is a name like any other word.
        while self.symbol(".") {
            match self.peek() {
                Token::Word(name) | Token::Quoted(name) => path.push(name.clone()),
                _ => return Err(self.expected("a field name")),
            }

            self.advance();
        }

        Ok(path)
    }

    fn value(&mut self) -> Result<Operand> {
        let value = match self.peek() {
            Token::Int(value) => Value::Int(*value),
            Token::Float(value) => Value::Float(*value),
            Token::String(value) => Value::String(value.clone()),
            Token::Word(word) if word.eq_ignore_ascii_case("true") => Value::Bool(true),
            Token::Word(word) if word.eq_ignore_ascii_case("false") => Value::Bool(false),
            Token::Word(word) if word.eq_ignore_ascii_case("null") => Value::Null,
            Token::Parameter(index) => match self.parameters {
                None => {
                    let index = *index;

                    self.advance();

                    return Ok(Operand::Param(index));
                }
                Some(parameters) => parameters.get(*index).cloned().ok_or_else(|| {
                    invalid(
                        self.position(),
                        format!(
                            "`${index}` names a parameter, and {} were given",
                            parameters.len()
                        ),
                    )
                })?,
            },
            _ => return Err(self.expected("a value")),
        };

        self.advance();

        Ok(Operand::Value(value))
    }
}

/// Parses `text` into a query, with `parameters` for `$0`, `$1` and on, or
/// keeping them as parameters if there are none to give.
pub(crate) fn parse(text: &str, parameters: Option<&[Value]>) -> Result<Ir> {
    let mut parser = Parser {
        tokens: tokens(text)?,
        at: 0,
        parameters,
        nesting: 0,
    };

    parser.query()
}
