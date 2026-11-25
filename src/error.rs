use std::fmt;
use std::io;
use std::path::PathBuf;

/// A specialized [`Result`](std::result::Result) for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned while loading dictionaries and conversion tables.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Reading or writing a file failed.
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
    /// A text dictionary or conversion table has a malformed line.
    Parse {
        /// The file, if the text came from one.
        path: Option<PathBuf>,
        /// 1-based line number.
        line: usize,
        /// What was wrong with the line.
        kind: ParseErrorKind,
    },
}

/// The kind of [`Error::Parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// A conversion-table line has a key but no value.
    MissingValue,
    /// A dictionary line has a key but no tab-separated words.
    MissingTab,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ParseErrorKind::MissingValue => "key has no value",
            ParseErrorKind::MissingTab => "key has no tab-separated words",
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Parse { path, line, kind } => match path {
                Some(path) => write!(f, "{}:{line}: {kind}", path.display()),
                None => write!(f, "line {line}: {kind}"),
            },
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
