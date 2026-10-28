//! Compile errors that point at the offending source text.
use std::fmt;

/// A half-open byte range in the source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub(crate) fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// A rejected program: what is wrong, and where.
#[derive(Debug)]
pub struct Error {
    /// The source range the message is about.
    pub span: Span,
    /// What is wrong, and when possible what to write instead.
    pub message: String,
}

impl Error {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    /// Renders the error with its file position and the offending line.
    ///
    /// ```
    /// let source = "pin led: out = nowhere\n";
    /// let error = spatial::compile(source).unwrap_err();
    /// let text = error.render("main.spt", source);
    /// assert!(text.starts_with("main.spt:1:16: "));
    /// assert!(text.contains("^^^^^^^"));
    /// ```
    pub fn render(&self, path: &str, source: &str) -> String {
        let start = self.span.start.min(source.len());
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |i| start + i);
        let line_number = source[..start].matches('\n').count() + 1;
        let column = source[line_start..start].chars().count() + 1;
        let width = source[start..self.span.end.clamp(start, line_end)]
            .chars()
            .count()
            .max(1);
        format!(
            "{path}:{line_number}:{column}: {}\n  |\n  | {}\n  | {}{}",
            self.message,
            &source[line_start..line_end],
            " ".repeat(column - 1),
            "^".repeat(width)
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;
