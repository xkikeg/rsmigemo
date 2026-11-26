//! The query engine.

use std::path::Path;

use crate::convert::{ConversionTable, TableKind};
use crate::dict::{Dictionary, DictionaryBuilder};
use crate::encoding::{self, Encoding};
use crate::error::Result;
use crate::options::LoadOptions;
use crate::pattern::PatternBuilder;
use crate::syntax::Syntax;

/// Turns romaji queries into regular expressions that match Japanese text.
///
/// `Migemo` is immutable once built, and `Send + Sync`: share one instance
/// across threads.
///
#[cfg_attr(feature = "embedded-tables", doc = "```")]
#[cfg_attr(not(feature = "embedded-tables"), doc = "```ignore")]
/// let migemo = rsmigemo::Migemo::builder()
///     .dictionary(rsmigemo::Dictionary::parse("けんさく\t検索\n")?)
///     .build();
/// assert_eq!(
///     migemo.query("kensaku"),
///     "(kensaku|けんさく|ケンサク|検索|ｋｅｎｓａｋｕ|ｹﾝｻｸ)"
/// );
/// # Ok::<(), rsmigemo::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Migemo {
    dict: Dictionary,
    has_dict: bool,
    tables: [ConversionTable; 4],
    syntax: Syntax,
}

impl Migemo {
    /// Start configuring a `Migemo`.
    pub fn builder() -> MigemoBuilder {
        MigemoBuilder::default()
    }

    /// Open a dictionary file (text or static) with the conversion tables
    /// next to it.
    ///
    /// Conversion tables are read from `roma2hira.dat`, `hira2kata.dat`,
    /// `han2zen.dat` and `zen2han.dat` in the same directory, in the
    /// dictionary's encoding. Missing ones fall back to the built-in tables
    /// (with the `embedded-tables` feature) or to empty tables. A table with
    /// a malformed line keeps the entries before it, and the error is
    /// ignored. Use [`MigemoBuilder::table_file`] for strict
    /// loading.
    pub fn open(path: impl AsRef<Path>) -> Result<Migemo> {
        Ok(Migemo::builder()
            .dictionary_file(path.as_ref())?
            .sibling_tables(path.as_ref())?
            .build())
    }

    /// Generate a pattern for `query` with this instance's [`Syntax`]. See
    /// [`query_with`](Self::query_with) for how the query is read.
    pub fn query(&self, query: &str) -> String {
        self.query_with(query, &self.syntax)
    }

    /// Generate a pattern for `query` with the given syntax.
    ///
    /// The query is split where ASCII letter case changes (`kensakuEngine` →
    /// `kensaku`, `Engine`); each part becomes a sub-pattern and the results
    /// are concatenated.
    ///
    /// As in cmigemo, where queries are C strings, a NUL ends the query:
    /// anything after it is ignored.
    pub fn query_with(&self, query: &str, syntax: &Syntax) -> String {
        // A NUL ends the query, as it would for a C string.
        let query = query.split('\0').next().unwrap_or_default();
        let mut out = String::new();
        let mut builder = PatternBuilder::new();
        for segment in segments(query) {
            builder.clear();
            self.collect(segment, &mut builder);
            builder.write_to(&mut out, syntax);
        }
        out
    }

    /// Compile the pattern for `query` with the [`regex`] crate. This uses
    /// [`Syntax::rust_regex`], whatever this instance's syntax is.
    #[cfg(feature = "regex")]
    pub fn regex(&self, query: &str) -> Result<regex::Regex, regex::Error> {
        regex::Regex::new(&self.query_with(query, &Syntax::rust_regex()))
    }

    /// The syntax used by [`query`](Self::query).
    pub fn syntax(&self) -> &Syntax {
        &self.syntax
    }

    /// The dictionary. Empty if none was loaded.
    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
    }

    /// Whether a dictionary was loaded. Without one,
    /// queries still produce kana and width variants.
    pub fn has_dictionary(&self) -> bool {
        self.has_dict
    }

    /// One of the conversion tables.
    pub fn table(&self, kind: TableKind) -> &ConversionTable {
        &self.tables[kind.index()]
    }

    /// Add every candidate for one query segment.
    fn collect(&self, query: &str, out: &mut PatternBuilder) {
        let [roma2hira, hira2kata, han2zen, zen2han] = &self.tables;
        out.add(query);
        let lower = query.to_ascii_lowercase();
        self.add_lookup(&lower, out);
        for zen in han2zen.convert_all(query) {
            out.add(&zen);
        }
        for han in zen2han.convert_all(query) {
            out.add(&han);
        }
        for hira in roma2hira.convert_all(&lower) {
            out.add(&hira);
            self.add_lookup(&hira, out);
            for kata in hira2kata.convert_all(&hira) {
                out.add(&kata);
                self.add_lookup(&kata, out);
                for han in zen2han.convert_all(&kata) {
                    out.add(&han);
                }
            }
        }
    }

    fn add_lookup(&self, reading: &str, out: &mut PatternBuilder) {
        for word in self.dict.lookup(reading) {
            out.add(word);
        }
    }
}

/// Split a query where ASCII letter case changes.
///
/// A segment that starts with two uppercase letters runs until the first
/// character that isn't one (`ABCdef` → `ABC`, `def`). Any other segment runs
/// until the next uppercase letter (`kensakuEngine` → `kensaku`, `Engine`).
/// Non-ASCII characters never split.
fn segments(query: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut chars = query.char_indices().peekable();
    while let Some((start, first)) = chars.next() {
        let upper = first.is_ascii_uppercase()
            && chars.peek().is_some_and(|&(_, c)| c.is_ascii_uppercase());
        let mut end = query.len();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_ascii() && c.is_ascii_uppercase() != upper {
                end = i;
                break;
            }
            chars.next();
        }
        out.push(&query[start..end]);
    }
    out
}

/// Configures a [`Migemo`].
///
/// Files are read when the method is called, so errors come back right away.
#[derive(Debug, Clone)]
pub struct MigemoBuilder {
    encoding: Option<Encoding>,
    /// When no encoding is set: the encoding the first dictionary file was
    /// detected as, used for every file after it.
    detected: Option<Encoding>,
    strict: bool,
    dict: DictState,
    tables: [Option<ConversionTable>; 4],
    syntax: Syntax,
}

impl Default for MigemoBuilder {
    fn default() -> Self {
        MigemoBuilder {
            encoding: None,
            detected: None,
            strict: false,
            dict: DictState::None,
            tables: Default::default(),
            syntax: Syntax::cmigemo(),
        }
    }
}

#[derive(Debug, Clone, Default)]
enum DictState {
    #[default]
    None,
    Built(Dictionary),
    Building(DictionaryBuilder),
}

impl MigemoBuilder {
    /// The encoding of files read from now on. If none is set (the
    /// default), the first dictionary file's encoding is detected and then
    /// used for all later files.
    pub fn encoding(mut self, encoding: Encoding) -> Self {
        self.encoding = Some(encoding);
        self
    }

    /// Load files from now on strictly or not; see [`LoadOptions::strict`].
    /// Strict loading also makes [`sibling_tables`](Self::sibling_tables)
    /// fail on a malformed table. [`Migemo::open`] is always lenient.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    fn file_options(&self) -> LoadOptions {
        LoadOptions {
            encoding: self.encoding.or(self.detected),
            strict: self.strict,
        }
    }

    /// Load a dictionary file, text or static. Repeat to merge several
    /// dictionaries.
    pub fn dictionary_file(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let (path, options) = (path.as_ref(), self.file_options());
        let detected = if let DictState::None = self.dict {
            let (dict, detected) = Dictionary::open_detect(path, options)?;
            self.dict = DictState::Built(dict);
            detected
        } else {
            // Merging: read straight into the builder rather than building a
            // trie only to unpack it again.
            self.builder().add_file_detect(path, options)?
        };
        self.detected.get_or_insert(detected);
        Ok(self)
    }

    /// The builder that merges the dictionaries given so far.
    fn builder(&mut self) -> &mut DictionaryBuilder {
        if let DictState::None | DictState::Built(_) = self.dict {
            self.dict = match std::mem::take(&mut self.dict) {
                DictState::Built(dict) => DictState::Building(DictionaryBuilder::from(&dict)),
                _ => DictState::Building(DictionaryBuilder::new()),
            };
        }
        match &mut self.dict {
            DictState::Building(builder) => builder,
            _ => unreachable!("converted above"),
        }
    }

    /// Use a dictionary. Repeat to merge several.
    pub fn dictionary(mut self, dict: Dictionary) -> Self {
        if let DictState::None = self.dict {
            self.dict = DictState::Built(dict);
        } else {
            self.builder().add_dictionary(&dict);
        }
        self
    }

    /// Use a conversion table. Pass [`ConversionTable::new()`] to disable one.
    pub fn table(mut self, kind: TableKind, table: ConversionTable) -> Self {
        self.tables[kind.index()] = Some(table);
        self
    }

    /// Load a conversion table file.
    pub fn table_file(self, kind: TableKind, path: impl AsRef<Path>) -> Result<Self> {
        let table = ConversionTable::from_file(path, self.file_options())?;
        Ok(self.table(kind, table))
    }

    /// Load the conversion tables found next to `dict_path`, the way
    /// [`Migemo::open`] does: missing files are skipped, and a malformed
    /// table keeps the entries before the bad line. With
    /// [`strict`](Self::strict), a malformed table is an error instead.
    pub fn sibling_tables(mut self, dict_path: impl AsRef<Path>) -> Result<Self> {
        let dir = dict_path
            .as_ref()
            .parent()
            .filter(|d| !d.as_os_str().is_empty());
        let dir = dir.unwrap_or(Path::new("."));
        for kind in TableKind::ALL {
            let file = dir.join(kind.file_name());
            if !file.is_file() {
                continue;
            }
            let table = if self.strict {
                ConversionTable::from_file(&file, self.file_options())?
            } else {
                let bytes = crate::io::read_file(&file)?;
                let text = encoding::resolve(self.file_options().encoding, &bytes).decode(&bytes);
                ConversionTable::parse_partial(&text).0
            };
            self = self.table(kind, table);
        }
        Ok(self)
    }

    /// The syntax of generated patterns. Defaults to [`Syntax::cmigemo()`].
    pub fn syntax(mut self, syntax: Syntax) -> Self {
        self.syntax = syntax;
        self
    }

    /// Finish. Tables that were not set are the built-in ones (with the
    /// `embedded-tables` feature) or empty.
    pub fn build(self) -> Migemo {
        let (dict, has_dict) = match self.dict {
            DictState::None => (Dictionary::new(), false),
            DictState::Built(dict) => (dict, true),
            DictState::Building(builder) => (builder.build(), true),
        };
        let mut kinds = TableKind::ALL.into_iter();
        let tables = self.tables.map(|table| {
            let kind = kinds.next().unwrap();
            table.unwrap_or_else(|| default_table(kind))
        });
        Migemo {
            dict,
            has_dict,
            tables,
            syntax: self.syntax,
        }
    }
}

#[cfg(feature = "embedded-tables")]
fn default_table(kind: TableKind) -> ConversionTable {
    ConversionTable::embedded(kind)
}

#[cfg(not(feature = "embedded-tables"))]
fn default_table(_: TableKind) -> ConversionTable {
    ConversionTable::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segmentation() {
        assert_eq!(segments("kensakuEngine"), ["kensaku", "Engine"]);
        assert_eq!(segments("ABCdef"), ["ABC", "def"]);
        assert_eq!(segments("AB1"), ["AB", "1"]);
        assert_eq!(segments("aB"), ["a", "B"]);
        assert_eq!(segments("Aかな"), ["Aかな"]);
        assert_eq!(segments(""), Vec::<&str>::new());
    }

    #[test]
    fn send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Migemo>();
    }

    // No dictionary, no tables.
    #[test]
    fn no_dictionary_custom_escape() {
        let mut builder = Migemo::builder();
        for kind in TableKind::ALL {
            builder = builder.table(kind, ConversionTable::new());
        }
        let syntax = Syntax {
            escape: crate::EscapeSet::new("+?"),
            ..Syntax::cmigemo()
        };
        let migemo = builder.syntax(syntax).build();
        assert!(!migemo.has_dictionary());
        assert_eq!(migemo.query("a+b?c.d"), r"a\+b\?c.d");
    }
}
