//! Generate regular expressions that match Japanese text from romaji queries.
//!
//! A Rust port of [C/Migemo](https://github.com/koron/cmigemo).

#![warn(missing_docs)]

mod convert;
mod ctype;
mod dict;
mod encoding;
mod error;
mod io;
mod pattern;
mod syntax;

pub use convert::{ConversionTable, TableKind};
pub use dict::{Dictionary, DictionaryBuilder, Words};
pub use encoding::Encoding;
pub use error::{Error, ParseErrorKind, Result};
pub use pattern::PatternBuilder;
pub use syntax::{EscapeSet, Syntax};
