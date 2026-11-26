//! Character conversion tables such as romaji → hiragana.

use std::path::Path;

use crate::ctype::is_space;
use crate::encoding::{self, Encoding};
use crate::error::{Error, ParseErrorKind, Result};

/// Which of the four standard conversion tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableKind {
    /// Romaji → hiragana (`roma2hira.dat`).
    Roma2Hira,
    /// Hiragana → katakana (`hira2kata.dat`).
    Hira2Kata,
    /// Half-width → full-width (`han2zen.dat`).
    Han2Zen,
    /// Full-width → half-width (`zen2han.dat`).
    Zen2Han,
}

impl TableKind {
    /// All kinds, in the order they are loaded.
    pub const ALL: [TableKind; 4] = [
        TableKind::Roma2Hira,
        TableKind::Hira2Kata,
        TableKind::Han2Zen,
        TableKind::Zen2Han,
    ];

    /// The standard file name, looked for next to the dictionary.
    pub const fn file_name(self) -> &'static str {
        match self {
            TableKind::Roma2Hira => "roma2hira.dat",
            TableKind::Hira2Kata => "hira2kata.dat",
            TableKind::Han2Zen => "han2zen.dat",
            TableKind::Zen2Han => "zen2han.dat",
        }
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// A prefix table that rewrites strings, such as `kya` → `きゃ`.
///
/// Tables use cmigemo's `.dat` format: one `KEY VALUE [REMAIN]` entry per
/// line, separated by whitespace. `REMAIN` is pushed back onto the input after
/// `VALUE` is emitted (`kk → っ`, pushing back `k`). Lines starting with `#`
/// are comments; a line starting with `##` defines a key that starts with `#`.
/// `REMAIN` must be shorter than `KEY`, so that every conversion ends.
///
/// An empty table converts nothing: [`convert_all`](Self::convert_all)
/// returns no candidates.
#[derive(Debug, Clone)]
pub struct ConversionTable {
    /// `nodes[0]` is the root. Children are sorted by `ch`.
    nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
struct Node {
    ch: char,
    /// Indices into `nodes`, sorted by their `ch`.
    children: Vec<u32>,
    // `Box<str>` rather than `String`: set once, never grown, and 8 bytes
    // smaller (no capacity); `Option` costs nothing on top.
    value: Option<Box<str>>,
    remain: Option<Box<str>>,
}

impl Node {
    fn new(ch: char) -> Self {
        Node {
            ch,
            children: Vec::new(),
            value: None,
            remain: None,
        }
    }
}

impl Default for ConversionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversionTable {
    /// An empty table.
    pub fn new() -> Self {
        ConversionTable {
            nodes: vec![Node::new('\0')],
        }
    }

    /// Whether the table has no entries.
    pub fn is_empty(&self) -> bool {
        self.nodes[0].children.is_empty()
    }

    /// The built-in copy of cmigemo's standard table.
    #[cfg(feature = "embedded-tables")]
    pub fn embedded(kind: TableKind) -> Self {
        let text = match kind {
            TableKind::Roma2Hira => include_str!("../data/roma2hira.dat"),
            TableKind::Hira2Kata => include_str!("../data/hira2kata.dat"),
            TableKind::Han2Zen => include_str!("../data/han2zen.dat"),
            TableKind::Zen2Han => include_str!("../data/zen2han.dat"),
        };
        Self::parse(text).expect("embedded tables are valid")
    }

    /// Parse a table in `.dat` format.
    pub fn parse(text: &str) -> Result<Self> {
        let (table, error) = Self::parse_partial(text);
        match error {
            Some(e) => Err(e),
            None => Ok(table),
        }
    }

    /// Parse up to the first malformed line, returning the entries before it
    /// along with the error. Lenient loading reads tables this way.
    pub(crate) fn parse_partial(text: &str) -> (Self, Option<Error>) {
        let mut table = Self::new();
        for (i, line) in text.lines().enumerate() {
            let result = parse_line(line).and_then(|entry| match entry {
                Some(e) => table.insert(e.key, e.value, e.remain),
                None => Ok(()),
            });
            if let Err(kind) = result {
                return (table, Some(Error::parse(i + 1, kind)));
            }
        }
        (table, None)
    }

    /// Read and parse a `.dat` file. With `None`, the encoding is detected.
    pub fn from_file(path: impl AsRef<Path>, encoding: Option<Encoding>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = crate::io::read_file(path)?;
        let text = encoding::resolve(encoding, &bytes).decode(&bytes);
        Self::parse(&text).map_err(|e| e.with_path(path))
    }

    /// Add an entry. The first key wins: if `key` or a prefix of it
    /// already has a value, nothing changes; otherwise existing longer keys
    /// that start with `key` are removed.
    ///
    /// # Errors
    ///
    /// [`ParseErrorKind::RemainNotShorter`] if `remain` has as many
    /// characters as `key` or more. Pushing it back would not shrink the
    /// input, so [`convert_all`](Self::convert_all) could loop forever
    /// (`a → x`, push back `a`). Every standard rule shrinks, as in
    /// `kk → っ`, push back `k`.
    pub fn insert(
        &mut self,
        key: &str,
        value: &str,
        remain: Option<&str>,
    ) -> std::result::Result<(), ParseErrorKind> {
        if remain.is_some_and(|r| r.chars().count() >= key.chars().count()) {
            return Err(ParseErrorKind::RemainNotShorter);
        }
        let mut cur = 0;
        for ch in key.chars() {
            let next = self.dig(cur, ch);
            if self.nodes[next].value.is_some() {
                return Ok(()); // `key` or a prefix of it is already defined
            }
            cur = next;
        }
        if cur == 0 {
            return Ok(()); // empty key
        }
        let node = &mut self.nodes[cur];
        node.children.clear();
        node.value = Some(value.into());
        node.remain = remain.map(Into::into);
        Ok(())
    }

    /// The child of `parent` for `ch`: `Ok(index into nodes)`, or
    /// `Err(position in children)` where it would be inserted.
    fn find(&self, parent: usize, ch: char) -> Result<usize, usize> {
        let children = &self.nodes[parent].children;
        children
            .binary_search_by_key(&ch, |&i| self.nodes[i as usize].ch)
            .map(|i| children[i] as usize)
    }

    /// The child of `parent` for `ch`, created (with no value) if missing.
    fn dig(&mut self, parent: usize, ch: char) -> usize {
        match self.find(parent, ch) {
            Ok(i) => i,
            Err(pos) => {
                let id = self.nodes.len();
                self.nodes.push(Node::new(ch));
                self.nodes[parent].children.insert(pos, id as u32);
                id
            }
        }
    }

    /// Convert `src` by greedy longest match, returning every candidate.
    ///
    /// Characters that start no key pass through unchanged. If `src` ends in
    /// the middle of a key, the result has one candidate per completion of
    /// that key (for romaji, `ky` gives `きゃ`, `きぇ`, `きぃ`, `きょ`, `きゅ`).
    /// Otherwise it has exactly one. An empty table returns no candidates.
    pub fn convert_all(&self, src: &str) -> Vec<String> {
        if self.is_empty() {
            return Vec::new();
        }
        // Input as a stack, so pushing text back is cheap.
        let mut input: Vec<char> = src.chars().rev().collect();
        let mut out = String::with_capacity(src.len() * 3);
        let mut pending: Vec<char> = Vec::new();
        let mut node = 0;
        while let Some(ch) = input.pop() {
            match self.find(node, ch) {
                Err(_) => {
                    // No key continues here: emit the first pending char as-is
                    // and retry the rest from the root.
                    pending.push(ch);
                    out.push(pending[0]);
                    input.extend(pending[1..].iter().rev());
                    pending.clear();
                    node = 0;
                }
                Ok(next) => {
                    let n = &self.nodes[next];
                    if let Some(value) = &n.value {
                        out.push_str(value);
                        if let Some(remain) = &n.remain {
                            input.extend(remain.chars().rev());
                        }
                        pending.clear();
                        node = 0;
                    } else {
                        pending.push(ch);
                        node = next;
                    }
                }
            }
        }
        if pending.is_empty() {
            return vec![out];
        }
        let mut candidates = Vec::new();
        self.complete(node, &out, &mut candidates);
        candidates
    }

    /// Append `prefix + value` for every value below `node`, depth-first in
    /// key order, without duplicates.
    fn complete(&self, node: usize, prefix: &str, out: &mut Vec<String>) {
        for &child in &self.nodes[node].children {
            let child = child as usize;
            if let Some(value) = &self.nodes[child].value {
                let candidate = [prefix, value].concat();
                // A key has at most a dozen completions (`k` has 12), so a
                // linear check is cheaper than hashing, and unlike sorting
                // it keeps the key order.
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
            self.complete(child, prefix, out);
        }
    }
}

/// A parsed `.dat` line.
struct Entry<'a> {
    key: &'a str,
    value: &'a str,
    remain: Option<&'a str>,
}

/// Parse one `.dat` line; `None` for blank and comment lines.
fn parse_line(line: &str) -> Result<Option<Entry<'_>>, ParseErrorKind> {
    let s = line.trim_start_matches(is_space);
    if s.is_empty() {
        return Ok(None);
    }
    let s = match s.strip_prefix('#') {
        Some(rest) if rest.starts_with('#') => rest,
        Some(_) => return Ok(None),
        None => s,
    };
    let (key, rest) = split_field(s);
    let (value, rest) = split_field(rest);
    if value.is_empty() {
        return Err(ParseErrorKind::MissingValue);
    }
    let (remain, _) = split_field(rest);
    Ok(Some(Entry {
        key,
        value,
        remain: (!remain.is_empty()).then_some(remain),
    }))
}

/// The first whitespace-delimited field of `s` after leading whitespace,
/// and the rest after the delimiter.
fn split_field(s: &str) -> (&str, &str) {
    let s = s.trim_start_matches(is_space);
    s.split_once(is_space).unwrap_or((s, ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "embedded-tables")]
    fn roma() -> ConversionTable {
        ConversionTable::embedded(TableKind::Roma2Hira)
    }

    #[track_caller]
    fn check(table: &ConversionTable, src: &str, want: &[&str]) {
        assert_eq!(table.convert_all(src), want, "src={src:?}");
    }

    #[cfg(feature = "embedded-tables")]
    #[test]
    fn embedded_tables_parse() {
        for kind in TableKind::ALL {
            assert!(!ConversionTable::embedded(kind).is_empty(), "{kind:?}");
        }
    }

    #[cfg(feature = "embedded-tables")]
    #[test]
    fn roma2hira() {
        let t = roma();
        check(&t, "a", &["あ"]);
        check(&t, "aki", &["あき"]);
        check(
            &t,
            "k",
            &[
                "か", "け", "き", "っ", "こ", "く", "くぁ", "きゃ", "きぇ", "きぃ", "きょ", "きゅ",
            ],
        );
        check(&t, "nya", &["にゃ"]);
        check(&t, "ny", &["にゃ", "にぇ", "にぃ", "にょ", "にゅ"]);
        check(
            &t,
            "n",
            &[
                "ん", "な", "ね", "に", "の", "ぬ", "にゃ", "にぇ", "にぃ", "にょ", "にゅ",
            ],
        );
        check(&t, "", &[""]);
        check(&t, "nn", &["ん"]);
        check(&t, "n'", &["ん"]);
        check(&t, "ltu", &["っ"]);
        check(
            &t,
            "nk",
            &[
                "んか",
                "んけ",
                "んき",
                "んっ",
                "んこ",
                "んく",
                "んくぁ",
                "んきゃ",
                "んきぇ",
                "んきぃ",
                "んきょ",
                "んきゅ",
            ],
        );
        check(&t, "nba", &["んば"]);
        check(&t, "bba", &["っば"]);
        check(&t, "ai", &["あい"]);
        check(&t, "kate", &["かて"]);
    }

    #[cfg(feature = "embedded-tables")]
    #[test]
    fn hira2kata() {
        let t = ConversionTable::embedded(TableKind::Hira2Kata);
        check(&t, "あ", &["ア"]);
        check(&t, "にゃ", &["ニャ"]);
        check(&t, "きゃんでぃ", &["キャンディ"]);
        check(&t, "っ", &["ッ"]);
        check(&t, "しゃ", &["シャ"]);
        check(&t, "きゃ", &["キャ"]);
        check(&t, "きゃんでいー", &["キャンデイー"]);
    }

    #[test]
    fn passthrough_and_empty_table() {
        let t = ConversionTable::parse("a\tあ\n").unwrap();
        check(&t, "x1a", &["x1あ"]);
        assert!(ConversionTable::new().convert_all("ka").is_empty());
    }

    #[test]
    fn remain_must_be_shorter() {
        let mut t = ConversionTable::new();
        assert_eq!(t.insert("kk", "っ", Some("k")), Ok(()));
        for (key, remain) in [("a", "a"), ("a", "b"), ("ab", "cd"), ("a", "bc")] {
            assert_eq!(
                t.insert(key, "x", Some(remain)),
                Err(ParseErrorKind::RemainNotShorter),
                "{key} {remain}"
            );
        }
        // Lenient loading keeps the entries before the bad line.
        let (t, err) = ConversionTable::parse_partial("i\tい\na\tx\ta\nu\tう\n");
        assert!(matches!(
            err,
            Some(Error::Parse {
                line: 2,
                kind: ParseErrorKind::RemainNotShorter,
                ..
            })
        ));
        check(&t, "iau", &["いau"]);
    }

    #[test]
    fn insert_rules() {
        let t = ConversionTable::parse("ab\tX\na\tY\nab\tZ\nc\tP\ncd\tQ\nc\tR\n").unwrap();
        // "a" removed the longer "ab"; the later "ab" is blocked by "a".
        check(&t, "ab", &["Yb"]);
        // First definition wins for duplicates; "cd" is blocked by "c".
        check(&t, "cd", &["Pd"]);
    }

    #[test]
    fn parse_quirks() {
        let t = ConversionTable::parse("# comment\n  # indented comment\n##\t＃\n\nxx  y\tz w\n")
            .unwrap();
        check(&t, "#", &["＃"]);
        // Remain "z" is pushed back and passes through; "w" is ignored.
        check(&t, "xx", &["yz"]);
        let t = ConversionTable::parse("xx\ty\tz\r\nz\tZ").unwrap();
        check(&t, "xx", &["yZ"]);
        let err = ConversionTable::parse("a\tあ\nkey\n").unwrap_err();
        assert!(matches!(
            err,
            Error::Parse {
                line: 2,
                kind: ParseErrorKind::MissingValue,
                ..
            }
        ));
    }
}
