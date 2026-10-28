//! Split source text into tokens. Whitespace, newlines and `//` comments are skipped.
use crate::diagnostic::{Error, Result, Span};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Ident(String),
    Int(i128),
    Str(String),
    /// Punctuation and operators, longest match first.
    Sym(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub span: Span,
}

const SYMBOLS: [&str; 26] = [
    "..=", "->", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "{", "}", "(", ")", "[", "]", ",",
    ":", "=", "!", "+", "-", "*", "/", "@", ".",
];
const SINGLE: [&str; 5] = ["<", ">", "&", "|", "^"];

pub(crate) fn tokenize(source: &str) -> Result<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if source[i..].starts_with("//") {
            i = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push(Token {
                kind: Kind::Ident(source[start..i].into()),
                span: Span::new(start, i),
            });
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let text = source[start..i].replace('_', "");
            let span = Span::new(start, i);
            let (digits, radix) = match text.get(..2) {
                Some("0x") => (&text[2..], 16),
                Some("0b") => (&text[2..], 2),
                _ => (text.as_str(), 10),
            };
            let value = i128::from_str_radix(digits, radix).map_err(|_| {
                Error::new(span, format!("`{}` is not a number", &source[start..i]))
            })?;
            tokens.push(Token {
                kind: Kind::Int(value),
                span,
            });
        } else if c == b'"' {
            let start = i;
            let end = source[i + 1..]
                .find(['"', '\n'])
                .map(|n| i + 1 + n)
                .filter(|&n| bytes[n] == b'"')
                .ok_or_else(|| Error::new(Span::new(start, start + 1), "unterminated string"))?;
            tokens.push(Token {
                kind: Kind::Str(source[start + 1..end].into()),
                span: Span::new(start, end + 1),
            });
            i = end + 1;
        } else {
            let symbol = SYMBOLS
                .iter()
                .chain(&SINGLE)
                .find(|s| source[i..].starts_with(**s))
                .ok_or_else(|| {
                    let ch = source[i..].chars().next().unwrap_or('?');
                    Error::new(
                        Span::new(i, i + ch.len_utf8()),
                        format!("unexpected `{ch}`"),
                    )
                })?;
            tokens.push(Token {
                kind: Kind::Sym(symbol),
                span: Span::new(i, i + symbol.len()),
            });
            i += symbol.len();
        }
    }
    tokens.push(Token {
        kind: Kind::Eof,
        span: Span::new(source.len(), source.len()),
    });
    Ok(tokens)
}
