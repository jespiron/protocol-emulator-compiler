//! Compile errors that point at the offending source text, laid out like
//! rustc's: the message, the lines involved with marks under them, notes, and
//! a suggested edit.
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

/// Source text called out by an error, with a short remark under it.
#[derive(Debug)]
pub struct Label {
    /// The text called out.
    pub span: Span,
    /// The remark.
    pub text: String,
}

/// An edit that would fix the error: replace `span` with `replacement`, in
/// which `{}` stands for the text being replaced.
#[derive(Debug)]
pub struct Help {
    /// What the edit does.
    pub message: String,
    /// The text to replace.
    pub span: Span,
    /// What to write instead.
    pub replacement: String,
}

/// A rejected program: what is wrong, and where.
#[derive(Debug)]
pub struct Error {
    /// The source range the message is about.
    pub span: Span,
    /// What is wrong, and when possible what to write instead.
    pub message: String,
    /// A remark under `span`.
    pub label: Option<String>,
    /// Other source text that explains the error, such as where a value came from.
    pub related: Vec<Label>,
    /// Lines under the source, in order: `note` for an extra fact, `help`
    /// for advice in words when a fix is not a single edit.
    pub asides: Vec<(&'static str, String)>,
    /// A suggested fix.
    pub help: Option<Box<Help>>,
}

impl Error {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            label: None,
            related: Vec::new(),
            asides: Vec::new(),
            help: None,
        }
    }

    pub(crate) fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    pub(crate) fn related(mut self, span: Span, text: impl Into<String>) -> Self {
        self.related.push(Label {
            span,
            text: text.into(),
        });
        self
    }

    pub(crate) fn note(mut self, text: impl Into<String>) -> Self {
        self.asides.push(("note", text.into()));
        self
    }

    pub(crate) fn hint(mut self, text: impl Into<String>) -> Self {
        self.asides.push(("help", text.into()));
        self
    }

    pub(crate) fn help(
        mut self,
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
    ) -> Self {
        self.help = Some(Box::new(Help {
            message: message.into(),
            span,
            replacement: replacement.into(),
        }));
        self
    }

    /// Renders the error with its file position and the lines involved, in
    /// ANSI colors when `color` is set.
    ///
    /// ```
    /// let source = "pin led: out = nowhere\n";
    /// let error = spatial::compile(source).unwrap_err();
    /// let text = error.render("main.spt", source, false);
    /// assert!(text.starts_with("error: "));
    /// assert!(text.contains(" --> main.spt:1:16\n"));
    /// assert!(text.contains("^^^^^^^"));
    /// ```
    pub fn render(&self, path: &str, source: &str, color: bool) -> String {
        let paint = |code: &str, text: &str| {
            if color && !text.is_empty() {
                format!("\x1b[{code}m{text}\x1b[0m")
            } else {
                text.to_owned()
            }
        };
        let blue = |text: &str| paint("1;34", text);
        let at = Position::of(source, self.span.start);
        let mut marks = vec![(self.span, '^', self.label.as_deref().unwrap_or(""), "1;31")];
        marks.extend(
            self.related
                .iter()
                .map(|l| (l.span, '-', l.text.as_str(), "1;34")),
        );
        marks.sort_by_key(|(span, ..)| span.start);
        let last_line = marks
            .iter()
            .map(|(span, ..)| Position::of(source, span.start).line)
            .chain(
                self.help
                    .iter()
                    .map(|h| Position::of(source, h.span.start).line),
            )
            .max()
            .unwrap_or(at.line);
        let gutter = " ".repeat(last_line.to_string().len());
        let bar = |text: &str| match text {
            "" => blue(&format!("{gutter} |")),
            text => format!("{} {text}", blue(&format!("{gutter} |"))),
        };
        let numbered = |line: usize, text: &str| {
            format!(
                "{} {text}",
                blue(&format!("{line:>width$} |", width = gutter.len()))
            )
        };

        let mut out = vec![
            format!(
                "{}{}",
                paint("1;31", "error"),
                paint("1", &format!(": {}", self.message))
            ),
            format!(
                "{} {path}:{}:{}",
                blue(&format!("{gutter}-->")),
                at.line,
                at.column
            ),
            bar(""),
        ];
        let mut previous = None;
        for (span, mark, text, code) in &marks {
            let start = Position::of(source, span.start);
            if previous != Some(start.line) {
                if previous.is_some_and(|line| start.line > line + 1) {
                    out.push(blue("..."));
                }
                out.push(numbered(start.line, start.text));
            }
            previous = Some(start.line);
            let width = source[span.start..span.end.clamp(span.start, start.line_end)]
                .chars()
                .count()
                .max(1);
            let under = format!("{} {text}", mark.to_string().repeat(width));
            out.push(bar(&format!(
                "{}{}",
                " ".repeat(start.column - 1),
                paint(code, under.trim_end())
            )));
        }
        if !self.asides.is_empty() {
            out.push(bar(""));
        }
        // Later lines of a note line up under its first word.
        let mut aside = |label: &str, text: &str| {
            let indent = " ".repeat(gutter.len() + 5 + label.len());
            let mut lines = text.lines();
            out.push(format!(
                "{} {} {}",
                blue(&format!("{gutter} =")),
                paint("1", &format!("{label}:")),
                lines.next().unwrap_or("")
            ));
            out.extend(lines.map(|line| format!("{indent}{line}")));
        };
        for (label, text) in &self.asides {
            aside(label, text);
        }
        if let Some(help) = &self.help {
            let start = Position::of(source, help.span.start);
            let end = help.span.end.clamp(help.span.start, start.line_end);
            let before = &source[start.line_start..help.span.start];
            let original = &source[help.span.start..end];
            let after = &source[end..start.line_end];
            let replacement = help.replacement.replace("{}", original);
            // Mark the inserted text with `+`, or the whole replacement with
            // `~` when it does not contain the original.
            let (head, kept, tail, mark) = match replacement.find(original) {
                Some(i) if !original.is_empty() => (
                    &replacement[..i],
                    original,
                    &replacement[i + original.len()..],
                    '+',
                ),
                _ => (replacement.as_str(), "", "", '~'),
            };
            let green = |text: &str| paint("32", text);
            let marks = |text: &str| mark.to_string().repeat(text.chars().count());
            out.push(format!(
                "{}{}",
                paint("1;36", "help"),
                paint("1", &format!(": {}", help.message))
            ));
            out.push(bar(""));
            out.push(numbered(
                start.line,
                &format!("{before}{}{kept}{}{after}", green(head), green(tail)),
            ));
            out.push(bar(&format!(
                "{}{}{}{}",
                " ".repeat(before.chars().count()),
                green(&marks(head)),
                " ".repeat(kept.chars().count()),
                green(&marks(tail)),
            )));
        }
        out.join("\n")
    }
}

/// Where a byte offset falls: its line and column, and that line's text.
struct Position<'a> {
    line: usize,
    column: usize,
    line_start: usize,
    line_end: usize,
    text: &'a str,
}

impl<'a> Position<'a> {
    fn of(source: &'a str, offset: usize) -> Self {
        let offset = offset.min(source.len());
        let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[offset..]
            .find('\n')
            .map_or(source.len(), |i| offset + i);
        Self {
            line: source[..offset].matches('\n').count() + 1,
            column: source[line_start..offset].chars().count() + 1,
            line_start,
            line_end,
            text: &source[line_start..line_end],
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;
