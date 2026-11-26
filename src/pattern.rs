//! Builds a regular expression from a set of words.

use smallvec::SmallVec;

use crate::syntax::{EscapeSet, Syntax};

/// Collects words into a trie and writes a regular expression that matches
/// any of them.
///
/// A word that has an already-added word as a prefix is redundant for
/// matching and is dropped. Likewise, adding a prefix of existing words
/// replaces them. So the result does not depend on insertion order.
///
/// ```
/// use rsmigemo::{PatternBuilder, Syntax};
///
/// let mut b = PatternBuilder::new();
/// for w in ["赤ちゃん", "赤", "秋", "あか", "あき"] {
///     b.add(w);
/// }
/// assert_eq!(b.build(&Syntax::cmigemo()), "([秋赤]|あ[かき])");
/// ```
#[derive(Debug, Clone)]
pub struct PatternBuilder {
    /// `nodes[0]` is the root. Every other node with no children is the end
    /// of a word; every node on the way to it has children.
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
struct Node {
    /// Sorted by `ch`. Most nodes have two children or fewer, so those are
    /// kept inline.
    children: SmallVec<[Edge; 2]>,
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    ch: char,
    node: NodeId,
}

/// An index into [`PatternBuilder::nodes`], stored as `u32` to keep [`Edge`]
/// at 8 bytes.
#[derive(Debug, Clone, Copy)]
struct NodeId(u32);

// `NodeId::index` widens without loss.
const _: () = assert!(usize::BITS >= u32::BITS);

impl NodeId {
    /// `None` past `u32::MAX` nodes: one node per character of the words
    /// added, so about 100 GB of trie. Patterns from the standard
    /// dictionary use at most a few thousand.
    fn try_new(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(NodeId)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

impl Default for PatternBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        PatternBuilder {
            nodes: vec![Node::default()],
        }
    }

    /// Remove all words, keeping the allocation.
    pub fn clear(&mut self) {
        self.nodes.truncate(1);
        self.nodes[0].children.clear();
    }

    /// Whether no words are present.
    pub fn is_empty(&self) -> bool {
        self.nodes[0].children.is_empty()
    }

    /// Add a word.
    ///
    /// The empty string is ignored.
    ///
    /// # Panics
    ///
    /// If the builder would hold more than `u32::MAX` nodes, one per
    /// character added.
    pub fn add(&mut self, word: &str) {
        if word.is_empty() {
            return;
        }
        let mut cur = 0;
        for ch in word.chars() {
            let children = &self.nodes[cur].children;
            cur = match children.binary_search_by_key(&ch, |e| e.ch) {
                Ok(i) => {
                    let next = children[i].node.index();
                    if self.nodes[next].children.is_empty() {
                        // The child is a leaf: a shorter word that `word`
                        // starts with was already added, so `word` is
                        // redundant.
                        return;
                    }
                    next
                }
                Err(i) => {
                    let next = self.nodes.len();
                    let node =
                        NodeId::try_new(next).expect("PatternBuilder: more than u32::MAX nodes");
                    self.nodes.push(Node::default());
                    self.nodes[cur].children.insert(i, Edge { ch, node });
                    next
                }
            };
        }
        // `word` ends here, so the longer words below are redundant. Their
        // nodes stay in the arena until `clear`: at most one per character
        // added, the same as if they had not been pruned.
        self.nodes[cur].children.clear();
    }

    /// Write the pattern as a new string.
    pub fn build(&self, syntax: &Syntax) -> String {
        let mut out = String::new();
        self.write_to(&mut out, syntax);
        out
    }

    /// Append the pattern to `out`. Writes nothing if there are no words.
    pub fn write_to(&self, out: &mut String, syntax: &Syntax) {
        if !self.is_empty() {
            self.write_level(out, syntax, 0);
        }
    }

    /// Write the alternatives below `parent`: single-character leaves first
    /// (as a class if more than one), then each branch as char + subpattern.
    ///
    /// For children `a`, `b`, `c…` and `d…` this is `([ab]|c…|d…)`.
    fn write_level(&self, out: &mut String, syntax: &Syntax, parent: usize) {
        let children = &self.nodes[parent].children;
        // A leaf ends a word, so it is a single character. A branch is a
        // character followed by a subpattern.
        let is_leaf = |e: &&Edge| self.nodes[e.node.index()].children.is_empty();
        let leaves = || children.iter().filter(is_leaf);
        let branches = || children.iter().filter(|e| !is_leaf(e));
        let leaf_count = leaves().count();
        let branch_count = children.len() - leaf_count;

        // A `|` appears at this level only when a branch sits next to another
        // child. A class alone, or a single branch, needs no group.
        let needs_group = children.len() > 1 && branch_count > 0;

        if needs_group {
            out.push_str(&syntax.group_open);
        }
        // Every part but the first is preceded by `|`.
        let mut first = true;
        let mut push_or = |out: &mut String| {
            if !first {
                out.push_str(&syntax.or);
            }
            first = false;
        };
        if leaf_count > 0 {
            push_or(out);
            write_class(out, syntax, leaf_count, leaves());
        }
        for e in branches() {
            push_or(out);
            write_char(out, syntax.escape, e.ch);
            out.push_str(&syntax.newline);
            self.write_level(out, syntax, e.node.index());
        }
        if needs_group {
            out.push_str(&syntax.group_close);
        }
    }
}

/// Write `len` leaves as a character class. A single leaf is written bare,
/// so it is escaped as outside a class.
fn write_class<'a>(
    out: &mut String,
    syntax: &Syntax,
    len: usize,
    leaves: impl Iterator<Item = &'a Edge>,
) {
    if len == 1 {
        for e in leaves {
            write_char(out, syntax.escape, e.ch);
        }
        return;
    }
    out.push_str(&syntax.class_open);
    for e in leaves {
        write_char(out, syntax.class_escape, e.ch);
    }
    out.push_str(&syntax.class_close);
}

fn write_char(out: &mut String, escape: EscapeSet, ch: char) {
    if escape.contains(ch) {
        out.push('\\');
    }
    out.push(ch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::EscapeSet;

    fn build(words: &[&str], syntax: &Syntax) -> String {
        let mut b = PatternBuilder::new();
        for w in words {
            b.add(w);
        }
        b.build(syntax)
    }

    #[test]
    fn shorter_wins_in_any_order() {
        let d = Syntax::cmigemo();
        assert_eq!(build(&["赤", "赤ちゃん"], &d), "赤");
        assert_eq!(build(&["赤ちゃん", "赤"], &d), "赤");
        assert_eq!(build(&["国際便", "国際", "国"], &d), "国");
        assert_eq!(build(&[], &d), "");
    }

    #[test]
    fn structure() {
        let d = Syntax::cmigemo();
        assert_eq!(build(&["a"], &d), "a");
        assert_eq!(build(&["b", "a"], &d), "[ab]");
        assert_eq!(build(&["ab", "ac"], &d), "a[bc]");
        assert_eq!(build(&["ab", "c"], &d), "(c|ab)");
        assert_eq!(build(&["ab", "cd"], &d), "(ab|cd)");
        assert_eq!(build(&["ab", "c", "d"], &d), "([cd]|ab)");
        assert_eq!(build(&["abc", "abd"], &Syntax::vim()), r"a\_s*b\_s*[cd]");
        assert_eq!(
            build(&["ab", "c"], &Syntax::emacs().without_newline()),
            r"\(c\|ab\)"
        );
    }

    #[test]
    fn clear_and_reuse() {
        let mut b = PatternBuilder::new();
        b.add("abc");
        b.add("abd");
        b.clear();
        assert!(b.is_empty());
        b.add("ab");
        b.add("x");
        assert_eq!(b.build(&Syntax::cmigemo()), "(x|ab)");
    }

    #[test]
    fn empty_word_is_ignored() {
        assert_eq!(build(&["abc", "", "x"], &Syntax::cmigemo()), "(x|abc)");
        assert_eq!(build(&[""], &Syntax::cmigemo()), "");
    }

    #[test]
    fn escapes() {
        let mut s = Syntax::cmigemo();
        assert_eq!(
            build(&[r"a+b.c*d^e$f/g\h?i"], &s),
            r"a\+b\.c\*d\^e\$f\/g\\h?i"
        );
        s.escape = EscapeSet::new("\x01?+\u{ff}");
        assert_eq!(build(&["a+b.c?d"], &s), r"a\+b.c\?d");
        s.escape = EscapeSet::NONE;
        assert_eq!(build(&[r"a+b.c\d"], &s), r"a+b.c\d");
    }

    #[test]
    fn class_escapes() {
        // A class uses `class_escape`: nothing for cmigemo, `\ ] ^ -` for Vim.
        let c = Syntax::cmigemo();
        assert_eq!(build(&["+", "."], &c), "[+.]");
        let v = Syntax::vim();
        assert_eq!(build(&["+", "-", "."], &v), r"[+\-.]");
        assert_eq!(build(&["a", "]", "^", "\\"], &v), r"[\\\]\^a]");
        // A single leaf is not in a class, so the outer escape applies.
        assert_eq!(build(&["C++", "C#"], &c), r"C(#|\+\+)");
        assert_eq!(build(&["a."], &v), r"a\_s*\.");

        let mut s = Syntax::cmigemo();
        s.class_escape = EscapeSet::new("+-");
        assert_eq!(build(&["+", "-", "."], &s), r"[\+\-.]");
    }
}
