//! How files are loaded.

use crate::encoding::Encoding;

/// How to read dictionary and conversion-table files.
///
/// Functions that take `impl Into<LoadOptions>` also take an [`Encoding`]
/// or an `Option<Encoding>` (`None`: detect it), which mean lenient loading.
///
/// ```no_run
/// use rsmigemo::{Dictionary, Encoding, LoadOptions};
///
/// let detected = Dictionary::open("migemo-dict", None)?;
/// let utf8 = Dictionary::open("migemo-dict", Encoding::Utf8)?;
/// let strict = Dictionary::open("migemo-dict", LoadOptions::new().strict(true))?;
/// # Ok::<(), rsmigemo::Error>(())
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct LoadOptions {
    /// The encoding of text files, or `None` (the default) to detect it from
    /// each file's contents. Static dictionaries record their own.
    pub encoding: Option<Encoding>,
    /// Reject input that lenient loading would repair. Defaults to `false`,
    /// which accepts whatever cmigemo accepts.
    ///
    /// When strict, these are errors:
    /// - bytes that are not valid in the file's encoding
    ///   ([`Error::Decode`](crate::Error::Decode)), instead of U+FFFD;
    /// - static dictionaries that neither cmigemo nor this crate would write
    ///   ([`Error::InvalidSdict`](crate::Error::InvalidSdict)): an unknown
    ///   charset, an invalid character code, unsorted children, word lists
    ///   out of depth-first order, or a node no other node points to.
    ///
    /// Malformed lines in text dictionaries and conversion tables are errors
    /// either way, except in [`MigemoBuilder::sibling_tables`] when lenient.
    ///
    /// [`MigemoBuilder::sibling_tables`]: crate::MigemoBuilder::sibling_tables
    pub strict: bool,
}

impl LoadOptions {
    /// Lenient loading, detecting the encoding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`encoding`](Self::encoding).
    pub fn encoding(mut self, encoding: Encoding) -> Self {
        self.encoding = Some(encoding);
        self
    }

    /// Set [`strict`](Self::strict).
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

impl From<Encoding> for LoadOptions {
    fn from(encoding: Encoding) -> Self {
        LoadOptions::new().encoding(encoding)
    }
}

impl From<Option<Encoding>> for LoadOptions {
    fn from(encoding: Option<Encoding>) -> Self {
        LoadOptions {
            encoding,
            ..LoadOptions::new()
        }
    }
}
