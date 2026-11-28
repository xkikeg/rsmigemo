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
    /// A static dictionary (sdict) is malformed.
    InvalidSdict {
        /// The file, if the data came from one.
        path: Option<PathBuf>,
        /// What was wrong with it.
        reason: &'static str,
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
    /// A conversion-table `REMAIN` is not shorter than its key, so
    /// converting could loop forever.
    RemainNotShorter,
}

impl Error {
    /// An [`Error::Io`] for `path`.
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    /// An [`Error::Parse`] for in-memory text; [`with_path`](Self::with_path)
    /// adds the file once it is known.
    pub(crate) fn parse(line: usize, kind: ParseErrorKind) -> Self {
        Error::Parse {
            path: None,
            line,
            kind,
        }
    }

    /// Attach `path` to an error produced while parsing in-memory data.
    /// The path is converted only if the error has none yet.
    pub(crate) fn with_path(mut self, file: impl Into<PathBuf>) -> Self {
        match &mut self {
            Error::Parse {
                path: path @ None, ..
            }
            | Error::InvalidSdict {
                path: path @ None, ..
            } => *path = Some(file.into()),
            _ => {}
        }
        self
    }
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ParseErrorKind::MissingValue => "key has no value",
            ParseErrorKind::MissingTab => "key has no tab-separated words",
            ParseErrorKind::RemainNotShorter => "remain is not shorter than the key",
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
            Error::InvalidSdict { path, reason } => match path {
                Some(path) => write!(f, "{}: invalid static dictionary: {reason}", path.display()),
                None => write!(f, "invalid static dictionary: {reason}"),
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
