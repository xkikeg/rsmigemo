//! The migemo dictionary: reading (yomigana, the kana spelling of a word)
//! → words.

mod sdict;
mod text;

use std::collections::BTreeMap;
use std::path::Path;

use crate::encoding::{self, Encoding};
use crate::error::Result;
use crate::options::LoadOptions;

/// `word_idx` of a node with no words of its own.
const NONE: u32 = u32::MAX;

/// A read-only dictionary mapping readings (such as `けんさく`) to words
/// (such as `検索`), searchable by prefix.
///
/// It is stored as a flat trie with the same layout as cmigemo's static
/// dictionary (sdict). Each node's children are one sorted, contiguous block,
/// and the blocks are laid out depth first, so every subtree's words form one
/// contiguous slice. [`lookup`](Self::lookup) is a binary search per character
/// followed by a scan of one slice.
///
/// Build one from text with [`DictionaryBuilder`], or load either format
/// with [`Dictionary::open`].
//
// Offsets are `u32`, not `usize`, because the sdict format stores them that
// way and it halves the size of `Node`. That limits a dictionary to 4 GiB of
// words; `DictionaryBuilder::build` checks it. The full SKK dictionary is
// about 8 MB.
#[derive(Debug, Clone)]
pub struct Dictionary {
    /// `nodes[0]` is a virtual root with no character.
    nodes: Vec<Node>,
    /// Per node, the byte range in `words` covering its whole subtree.
    spans: Vec<(u32, u32)>,
    /// Per node with words: `word\0word\0…\0\0`, in node order. The NULs
    /// end each word (an offset table would cost 4 bytes a word instead of
    /// 1), and this is byte for byte the sdict word buffer.
    words: String,
}

/// One trie node, exactly as an sdict stores it.
#[derive(Debug, Clone, Copy)]
struct Node {
    ch: char,
    /// Children are `nodes[start..end]`.
    start: u32,
    end: u32,
    /// Offset of this node's own words in `words`, or `NONE`.
    word_idx: u32,
}

impl Default for Dictionary {
    fn default() -> Self {
        DictionaryBuilder::new().build()
    }
}

impl Dictionary {
    /// An empty dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a dictionary in migemo-dict text format.
    pub fn parse(text: &str) -> Result<Self> {
        let mut builder = DictionaryBuilder::new();
        builder.add_from_text(text)?;
        Ok(builder.build())
    }

    /// Load a dictionary file, either migemo-dict text or a static dictionary
    /// (detected by its `MGS1` header). The encoding in `options` applies to
    /// text files and is detected if unset; static dictionaries record their
    /// own.
    pub fn open(path: impl AsRef<Path>, options: impl Into<LoadOptions>) -> Result<Self> {
        Ok(Self::open_detect(path.as_ref(), options.into())?.0)
    }

    /// Like [`open`](Self::open), also returning the encoding the file was
    /// read in.
    pub(crate) fn open_detect(path: &Path, options: LoadOptions) -> Result<(Self, Encoding)> {
        let bytes = crate::io::read_file(path)?;
        if sdict::is_sdict(&bytes) {
            return sdict::read_with(&bytes, options.strict).map_err(|e| e.with_path(path));
        }
        let encoding = encoding::resolve(options.encoding, &bytes);
        let text = encoding
            .decode_with(&bytes, options.strict)
            .map_err(|e| e.with_path(path))?;
        let dict = Self::parse(&text).map_err(|e| e.with_path(path))?;
        Ok((dict, encoding))
    }

    /// Read a static dictionary (sdict) from memory.
    pub fn from_sdict(bytes: &[u8]) -> Result<Self> {
        Ok(sdict::read(bytes)?.0)
    }

    /// Like [`from_sdict`](Self::from_sdict), with the checks of
    /// [`LoadOptions::strict`].
    pub fn from_sdict_strict(bytes: &[u8]) -> Result<Self> {
        Ok(sdict::read_with(bytes, true)?.0)
    }

    /// Load a static dictionary (sdict) file, which may be `.zst`
    /// compressed. Unlike [`open`](Self::open), a text dictionary is an
    /// error. Only [`LoadOptions::strict`] applies: an sdict records its own
    /// encoding.
    pub fn open_sdict(path: impl AsRef<Path>, options: impl Into<LoadOptions>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = crate::io::read_file(path)?;
        let (dict, _) =
            sdict::read_with(&bytes, options.into().strict).map_err(|e| e.with_path(path))?;
        Ok(dict)
    }

    /// Write this dictionary in cmigemo's static dictionary (sdict) format,
    /// with UTF-8 text.
    pub fn write_sdict<W: std::io::Write>(&self, writer: W) -> std::io::Result<()> {
        sdict::write(self, writer)
    }

    /// Whether the dictionary has no entries.
    pub fn is_empty(&self) -> bool {
        self.nodes[0].start == self.nodes[0].end
    }

    /// All words whose reading starts with `prefix`, including `prefix`
    /// itself. An empty prefix matches nothing.
    pub fn lookup(&self, prefix: &str) -> Words<'_> {
        if prefix.is_empty() {
            return Words { rest: "" };
        }
        let mut cur = 0;
        for ch in prefix.chars() {
            let node = self.nodes[cur];
            let range = node.start as usize..node.end as usize;
            match self.nodes[range].binary_search_by_key(&ch, |n| n.ch) {
                Ok(i) => cur = node.start as usize + i,
                Err(_) => return Words { rest: "" },
            }
        }
        let (begin, end) = self.spans[cur];
        Words {
            rest: &self.words[begin as usize..end as usize],
        }
    }

    /// Call `f(reading, words)` for every entry, in reading order.
    pub fn for_each_entry(&self, mut f: impl FnMut(&str, Words<'_>)) {
        let mut key = String::new();
        self.visit(0, &mut key, &mut f);
    }

    fn visit(&self, id: usize, key: &mut String, f: &mut impl FnMut(&str, Words<'_>)) {
        let node = self.nodes[id];
        if node.word_idx != NONE {
            f(key, self.own_words(node.word_idx));
        }
        for child in node.start..node.end {
            key.push(self.nodes[child as usize].ch);
            self.visit(child as usize, key, f);
            key.pop();
        }
    }

    /// The words starting at `word_idx`, up to the empty word that ends them.
    fn own_words(&self, word_idx: u32) -> Words<'_> {
        let rest = &self.words[word_idx as usize..];
        if rest.starts_with('\0') {
            // An empty list, as an sdict may store: searching for `\0\0`
            // would run into the next node's words.
            return Words { rest: "" };
        }
        let end = rest.find("\0\0").map_or(rest.len(), |i| i + 1);
        Words { rest: &rest[..end] }
    }
}

/// Iterator over words from a [`Dictionary`].
#[derive(Debug, Clone)]
pub struct Words<'a> {
    /// Words separated (and possibly ended) by NULs, with empty words as
    /// list terminators.
    rest: &'a str,
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        loop {
            if self.rest.is_empty() {
                return None;
            }
            let (word, rest) = self.rest.split_once('\0').unwrap_or((self.rest, ""));
            self.rest = rest;
            if !word.is_empty() {
                return Some(word);
            }
        }
    }
}

impl std::iter::FusedIterator for Words<'_> {}

/// Accumulates dictionary entries from any number of sources, then builds a
/// [`Dictionary`].
///
/// Words for the same reading from different sources are merged.
#[derive(Debug, Clone, Default)]
pub struct DictionaryBuilder {
    entries: BTreeMap<String, Vec<String>>,
}

impl DictionaryBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `word` under `reading`. Empty readings, empty words and words
    /// containing NUL (which ends a word in the stored format) are ignored.
    pub fn insert(&mut self, reading: &str, word: &str) -> &mut Self {
        if reading.is_empty() || word.is_empty() || word.contains('\0') {
            return self;
        }
        // Not `entry()`: it takes an owned key, which would allocate a
        // `String` for every word even when the reading already exists.
        match self.entries.get_mut(reading) {
            Some(words) => words.push(word.to_owned()),
            None => {
                self.entries
                    .insert(reading.to_owned(), vec![word.to_owned()]);
            }
        }
        self
    }

    /// Add every entry of a migemo-dict text.
    pub fn add_from_text(&mut self, text: &str) -> Result<&mut Self> {
        text::parse(text, |reading, word| {
            self.insert(reading, word);
        })?;
        Ok(self)
    }

    /// Add every entry of a dictionary file (text or static). The encoding
    /// of a text file is detected if `options` doesn't set one.
    pub fn add_from_file(
        &mut self,
        path: impl AsRef<Path>,
        options: impl Into<LoadOptions>,
    ) -> Result<&mut Self> {
        self.add_file_detect(path.as_ref(), options.into())?;
        Ok(self)
    }

    /// Like [`add_from_file`](Self::add_from_file), returning the encoding
    /// the file was read in.
    pub(crate) fn add_file_detect(
        &mut self,
        path: &Path,
        options: LoadOptions,
    ) -> Result<Encoding> {
        let bytes = crate::io::read_file(path)?;
        if sdict::is_sdict(&bytes) {
            let (dict, encoding) =
                sdict::read_with(&bytes, options.strict).map_err(|e| e.with_path(path))?;
            self.add_dictionary(&dict);
            return Ok(encoding);
        }
        let encoding = encoding::resolve(options.encoding, &bytes);
        let text = encoding
            .decode_with(&bytes, options.strict)
            .map_err(|e| e.with_path(path))?;
        self.add_from_text(&text).map_err(|e| e.with_path(path))?;
        Ok(encoding)
    }

    /// Add every entry of an existing dictionary.
    pub fn add_dictionary(&mut self, dict: &Dictionary) -> &mut Self {
        dict.for_each_entry(|reading, words| {
            for word in words {
                self.insert(reading, word);
            }
        });
        self
    }

    /// Lay the entries out as a flat trie.
    pub fn build(&self) -> Dictionary {
        let entries: Vec<(&str, &[String])> = self
            .entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        // Each word and each list ends in a NUL.
        let words_len = entries
            .iter()
            .map(|(_, words)| words.iter().map(|w| w.len() + 1).sum::<usize>() + 1)
            .sum();
        let mut layout = Layout::new(words_len);
        layout.fill(0, &entries, 0);
        assert!(
            layout.words.len() < NONE as usize && layout.nodes.len() < NONE as usize,
            "dictionary exceeds 4 GiB"
        );
        Dictionary {
            nodes: layout.nodes,
            spans: layout.spans,
            words: layout.words,
        }
    }
}

impl From<&Dictionary> for DictionaryBuilder {
    fn from(dict: &Dictionary) -> Self {
        let mut builder = DictionaryBuilder::new();
        builder.add_dictionary(dict);
        builder
    }
}

/// The parts of a [`Dictionary`] while [`DictionaryBuilder::build`] lays the
/// sorted entries out in sdict order.
struct Layout {
    nodes: Vec<Node>,
    spans: Vec<(u32, u32)>,
    words: String,
}

impl Layout {
    /// Just the root, with room for `words_len` bytes of words.
    fn new(words_len: usize) -> Self {
        Layout {
            nodes: vec![Node {
                ch: '\0',
                start: 1,
                end: 1,
                word_idx: NONE,
            }],
            spans: vec![(0, 0)],
            words: String::with_capacity(words_len),
        }
    }

    /// Fill node `id`, whose reading is the first `depth` bytes shared by all
    /// of `group` (sorted), in the sdict layout order: own
    /// words, then the children block, then each child's subtree.
    fn fill(&mut self, id: usize, group: &[(&str, &[String])], depth: usize) {
        let begin = self.words.len() as u32;
        let rest = match group.first() {
            Some((key, words)) if key.len() == depth => {
                self.nodes[id].word_idx = begin;
                for word in *words {
                    self.words.push_str(word);
                    self.words.push('\0');
                }
                self.words.push('\0');
                &group[1..]
            }
            _ => group,
        };

        // Keys sharing the next char are adjacent, because UTF-8 byte order
        // is code point order.
        let mut children = Vec::new();
        let mut i = 0;
        while i < rest.len() {
            let ch = rest[i].0[depth..]
                .chars()
                .next()
                .expect("longer than depth");
            let j = i + rest[i..]
                .iter()
                .take_while(|(k, _)| k[depth..].starts_with(ch))
                .count();
            children.push((ch, i, j));
            i = j;
        }

        let start = self.nodes.len();
        for &(ch, _, _) in &children {
            self.nodes.push(Node {
                ch,
                start: 0,
                end: 0,
                word_idx: NONE,
            });
            self.spans.push((0, 0));
        }
        self.nodes[id].start = start as u32;
        self.nodes[id].end = self.nodes.len() as u32;
        for (k, &(ch, i, j)) in children.iter().enumerate() {
            self.fill(start + k, &rest[i..j], depth + ch.len_utf8());
        }
        self.spans[id] = (begin, self.words.len() as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Dictionary {
        Dictionary::parse(concat!(
            "あか\t赤\t紅\t朱\n",
            "あけ\t明\n",
            "あき\t秋\n",
            "あく\t悪\n",
            "あかい\t赤い\n",
            "いか\t烏賊\n",
        ))
        .unwrap()
    }

    fn sorted(words: Words<'_>) -> Vec<&str> {
        let mut v: Vec<_> = words.collect();
        v.sort();
        v
    }

    #[test]
    fn lookup_prefix() {
        let d = sample();
        assert_eq!(sorted(d.lookup("あか")), ["朱", "紅", "赤", "赤い"]);
        assert_eq!(sorted(d.lookup("あ")).len(), 7);
        assert_eq!(sorted(d.lookup("あかい")), ["赤い"]);
        assert!(d.lookup("あかいろ").next().is_none());
        assert!(d.lookup("").next().is_none());
        assert!(d.lookup("う").next().is_none());
    }

    #[test]
    fn merge_and_entries() {
        let mut b = DictionaryBuilder::from(&sample());
        b.add_from_text("あか\t垢\n").unwrap();
        let d = b.build();
        assert_eq!(sorted(d.lookup("あか")), ["垢", "朱", "紅", "赤", "赤い"]);
        let mut keys = Vec::new();
        d.for_each_entry(|k, _| keys.push(k.to_owned()));
        assert_eq!(keys, ["あか", "あかい", "あき", "あく", "あけ", "いか"]);
    }

    #[test]
    fn empty_word_list_stays_empty() {
        // An sdict may give a node an empty list; its words must not run
        // into the next node's.
        let d = Dictionary {
            nodes: vec![
                Node {
                    ch: '\0',
                    start: 1,
                    end: 3,
                    word_idx: NONE,
                },
                Node {
                    ch: 'a',
                    start: 3,
                    end: 3,
                    word_idx: 0,
                },
                Node {
                    ch: 'b',
                    start: 3,
                    end: 3,
                    word_idx: 1,
                },
            ],
            spans: vec![(0, 6), (0, 1), (1, 6)],
            words: "\0foo\0\0".to_owned(),
        };
        assert!(d.own_words(0).next().is_none());
        assert_eq!(d.lookup("b").collect::<Vec<_>>(), ["foo"]);
    }

    #[test]
    fn empty() {
        let d = Dictionary::new();
        assert!(d.is_empty());
        assert!(d.lookup("a").next().is_none());
    }
}
