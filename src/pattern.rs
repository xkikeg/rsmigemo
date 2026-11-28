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
    /// Sorted by `ch`.
    //
    // Two inline edges cover 95% of the nodes in patterns built from the
    // standard dictionary (63% have none, 29% one, 3% two), and two are free:
    // with smallvec's `union` feature, `SmallVec<[Edge; 2]>` is 24 bytes, the
    // size of a `Vec`. Each further inline edge adds 8 bytes to every node.
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
        if self.is_empty() {
            return;
        }
        // A word nests one level per character, so the levels still to
        // write are kept here rather than on the call stack.
        let mut pending = vec![Pending::Level(0)];
        while let Some(next) = pending.pop() {
            match next {
                Pending::Level(node) => self.write_level(out, syntax, node, &mut pending),
                Pending::Char(ch) => write_char(out, syntax.escape, ch),
                Pending::Text(text) => out.push_str(text),
            }
        }
    }

    /// Write the alternatives below `parent`: single-character leaves first
    /// (as a class if more than one), then each branch as char + subpattern.
    ///
    /// For children `a`, `b`, `c…` and `d…` this is `([ab]|c…|d…)`.
    ///
    /// The group's start, the class and the standalone leaves are written
    /// at once; the branches and the group's end go on `pending`.
    fn write_level<'s>(
        &self,
        out: &mut String,
        syntax: &'s Syntax,
        parent: usize,
        pending: &mut Vec<Pending<'s>>,
    ) {
        let children = &self.nodes[parent].children;
        // A leaf ends a word, so it is a single character. A branch is a
        // character followed by a subpattern.
        let is_leaf = |e: &&Edge| self.nodes[e.node.index()].children.is_empty();
        // A leaf goes in the class unless it is special there and the
        // syntax cannot escape it; then it stands alone, as an alternative
        // of its own. Only `\ ] ^ -` can stand alone, so `in_class` is
        // checked before the node lookup in `is_leaf`.
        let in_class = |e: &&Edge| {
            !EscapeSet::CLASS_SPECIAL.contains(e.ch) || syntax.class_escape.contains(e.ch)
        };
        let class_leaves = || children.iter().filter(is_leaf).filter(in_class);
        let standalone = || children.iter().filter(|e| !in_class(e)).filter(is_leaf);
        let branches = || children.iter().filter(|e| !is_leaf(e));
        // One pass over the children: the root can have thousands.
        let (mut leaf_count, mut standalone_count) = (0, 0);
        for e in children {
            if is_leaf(&e) {
                leaf_count += 1;
                standalone_count += usize::from(!in_class(&e));
            }
        }
        let class_count = leaf_count - standalone_count;
        let branch_count = children.len() - leaf_count;

        // Every part outside the class (a branch or a standalone leaf) is
        // joined by `|`: group unless the class holds every child, or there
        // is only one child.
        let needs_group = children.len() > 1 && class_count != children.len();

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
        if class_count > 0 {
            push_or(out);
            write_class(out, syntax, class_count, class_leaves());
        }
        if standalone_count > 0 {
            for e in standalone() {
                push_or(out);
                write_char(out, syntax.escape, e.ch);
            }
        }
        let wrote_part = !first;

        // The rest is pushed last first, so it pops in writing order.
        if needs_group {
            pending.push(Pending::Text(&syntax.group_close));
        }
        let mut before = branch_count;
        for e in branches().rev() {
            before -= 1;
            pending.push(Pending::Level(e.node.index()));
            pending.push(Pending::Text(&syntax.newline));
            pending.push(Pending::Char(e.ch));
            if before > 0 || wrote_part {
                pending.push(Pending::Text(&syntax.or));
            }
        }
    }
}

/// Output that [`PatternBuilder::write_to`] has yet to write.
enum Pending<'s> {
    /// The alternatives below a node.
    Level(usize),
    /// A character, escaped as outside a class.
    Char(char),
    Text(&'s str),
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

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn node_is_as_small_as_a_vec() {
        assert_eq!(size_of::<Node>(), size_of::<Vec<Edge>>());
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
    fn long_and_deep_words() {
        // Deeper than the call stack would allow with one frame per level.
        let long = "a".repeat(1_000_000);
        assert_eq!(build(&[&long], &Syntax::cmigemo()), long);
        // A branch at every level.
        assert_eq!(
            build(&["b", "ab", "aab"], &Syntax::cmigemo()),
            "(b|a(b|ab))"
        );
        let words: Vec<String> = (0..2_000).map(|n| "a".repeat(n) + "b").collect();
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let pattern = build(&words, &Syntax::cmigemo());
        assert!(pattern.starts_with("(b|a(b|a("), "{}", &pattern[..20]);
        assert_eq!(pattern.matches('|').count(), 1_999);
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
        let c = Syntax::cmigemo();
        // Nothing is escaped in a cmigemo class; `\ ] ^ -` go outside it.
        assert_eq!(build(&["+", "."], &c), "[+.]");
        assert_eq!(build(&["a", "b", "-"], &c), "([ab]|-)");
        assert_eq!(build(&["+", "-", "."], &c), "([+.]|-)");
        assert_eq!(build(&["A", "]"], &c), "(A|])");
        assert_eq!(build(&["x", "\\"], &c), r"(x|\\)");
        assert_eq!(build(&["a", "^"], &c), r"(a|\^)");
        assert_eq!(build(&["-", "^"], &c), r"(-|\^)");
        assert_eq!(build(&["-"], &c), "-");
        // Moving `-` out leaves a single leaf, written with the outer escape.
        assert_eq!(build(&["+", "-"], &c), r"(\+|-)");
        // A single leaf is not in a class, so the outer escape applies.
        assert_eq!(build(&["C++", "C#"], &c), r"C(#|\+\+)");
        assert_eq!(build(&["a."], &Syntax::vim()), r"a\_s*\.");
        // `[` is literal in a POSIX class.
        assert_eq!(build(&["[", "a"], &c), "[[a]");
        // Standalone leaves next to a branch, with and without a class.
        assert_eq!(build(&["a", "b", "-", "cd"], &c), "([ab]|-|cd)");
        assert_eq!(build(&["-", "cd"], &c), "(-|cd)");
        // Japanese next to `-`: `-` sorts first and stays outside.
        assert_eq!(build(&["-", "ー", "－"], &c), "([ー－]|-)");

        let e = Syntax::emacs();
        assert_eq!(build(&["a", "b", "-"], &e), r"\([ab]\|-\)");

        // Vim escapes `\ ] ^ -` in a class and nothing else.
        let v = Syntax::vim();
        assert_eq!(build(&["+", "-", "."], &v), r"[+\-.]");
        assert_eq!(build(&["a", "]", "^", "\\"], &v), r"[\\\]\^a]");

        let mut s = Syntax::cmigemo();
        s.class_escape = EscapeSet::new("+-");
        assert_eq!(build(&["+", "-", "]"], &s), r"([\+\-]|])");
    }
}
