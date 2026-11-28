//! Generate regular expressions that match Japanese text from romaji queries.
//!
//! A Rust port of [C/Migemo](https://github.com/koron/cmigemo). A query such as
//! `kensaku` expands to a pattern matching the romaji itself, its hiragana
//! and katakana readings, their full-width and half-width forms, and every
//! dictionary word whose reading starts with them:
//!
//! ```no_run
//! let migemo = rsmigemo::Migemo::open("dict/utf-8/migemo-dict")?;
//! assert_eq!(
//!     migemo.query("kensaku"),
//!     "(kensaku|けんさく|ケンサク|建策|憲[作冊]|検索|献策|研削|羂索|ｋｅｎｓａｋｕ|ｹﾝｻｸ)"
//! );
//! # Ok::<(), rsmigemo::Error>(())
//! ```
//!
//! The data files are cmigemo's: `migemo-dict` text dictionaries (UTF-8,
//! CP932 or EUC-JP), the `*.dat` conversion tables, and `migemo-sdict` static
//! dictionaries. The dictionary is not bundled (it is derived from the GPL
//! SKK-JISYO.L); the four conversion tables are, with the default
//! `embedded-tables` feature.
//!
//! # Features
//!
//! - `embedded-tables` (default): built-in conversion tables.
//! - `zstd`: read `.zst`-compressed dictionaries.

#![warn(missing_docs)]

mod convert;
mod ctype;
mod dict;
mod encoding;
mod error;
mod io;
mod migemo;
mod pattern;
mod syntax;

pub use convert::{ConversionTable, TableKind};
pub use dict::{Dictionary, DictionaryBuilder, Words};
pub use encoding::Encoding;
pub use error::{Error, ParseErrorKind, Result};
pub use migemo::{Migemo, MigemoBuilder};
pub use pattern::PatternBuilder;
pub use syntax::{EscapeSet, Syntax};
