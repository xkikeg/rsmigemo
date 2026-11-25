//! Generate regular expressions that match Japanese text from romaji queries.
//!
//! A Rust port of [C/Migemo](https://github.com/koron/cmigemo).

#![warn(missing_docs)]

mod encoding;
mod error;

pub use encoding::Encoding;
pub use error::{Error, ParseErrorKind, Result};
