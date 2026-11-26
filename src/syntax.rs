//! Regular-expression syntax used when writing patterns.

use std::borrow::Cow;
use std::fmt;

/// The operators and escaping rules used to write a pattern.
///
/// Every field can be set; presets cover the common regex flavors.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Syntax {
    /// Alternation, typically `|`.
    pub or: Cow<'static, str>,
    /// Group start, typically `(`.
    pub nest_in: Cow<'static, str>,
    /// Group end, typically `)`.
    pub nest_out: Cow<'static, str>,
    /// Character class start, typically `[`.
    pub select_in: Cow<'static, str>,
    /// Character class end, typically `]`.
    pub select_out: Cow<'static, str>,
    /// Written between the characters of a word, so that a match can run
    /// across a line break. With [`vim()`](Self::vim) it is `\_s*`, and
    /// `あい` becomes `あ\_s*い`. Empty to match within a line only.
    pub newline: Cow<'static, str>,
    /// Characters written with a `\` in front.
    pub escape: EscapeSet,
}

impl Syntax {
    /// The syntax of the `cmigemo` command with no options: `|`, `(…)`,
    /// `[…]`, no newline pattern, and [`EscapeSet::cmigemo()`].
    ///
    /// It is close to POSIX ERE but is not a standard flavor; pick the
    /// preset that matches your regex engine.
    pub fn cmigemo() -> Self {
        Syntax {
            or: "|".into(),
            nest_in: "(".into(),
            nest_out: ")".into(),
            select_in: "[".into(),
            select_out: "]".into(),
            newline: "".into(),
            escape: EscapeSet::cmigemo(),
        }
    }

    /// Vim, as `cmigemo -v`: `\|`, `\%(…\)`, and `\_s*` between characters.
    pub fn vim() -> Self {
        Syntax {
            or: r"\|".into(),
            nest_in: r"\%(".into(),
            nest_out: r"\)".into(),
            newline: r"\_s*".into(),
            ..Syntax::cmigemo()
        }
    }

    /// Emacs, as `cmigemo -e`: `\|`, `\(…\)`, and `\s-*` between characters.
    pub fn emacs() -> Self {
        Syntax {
            or: r"\|".into(),
            nest_in: r"\(".into(),
            nest_out: r"\)".into(),
            newline: r"\s-*".into(),
            ..Syntax::cmigemo()
        }
    }

    /// The same syntax with no [`newline`](Self::newline) pattern, as
    /// `cmigemo -n`.
    pub fn without_newline(mut self) -> Self {
        self.newline = "".into();
        self
    }
}

/// A set of ASCII punctuation characters and space to escape.
///
/// Only [punctuation](u8::is_ascii_punctuation) and `' '` can be escaped.
/// Letters and digits cannot, because a `\` in front of them means something
/// else in most regex flavors (`\d`, `\w`, `\b`). Neither can control
/// characters such as `\n` or `\t`, nor anything outside ASCII.
//
// One bit per ASCII code. The allowed characters span 0x20..=0x7e, which is
// 95 codes, so a u64 cannot hold them even shifted; packing the 33 allowed
// characters into a u64 would need a lookup table. A u128 is simpler, and
// testing a bit in it costs no more than in a u64 on 64-bit targets.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EscapeSet(u128);

impl EscapeSet {
    /// Escape nothing.
    pub const NONE: EscapeSet = EscapeSet(0);

    /// The characters of `chars`; anything but ASCII punctuation and `' '` is
    /// ignored.
    pub const fn new(chars: &str) -> Self {
        let bytes = chars.as_bytes();
        let mut bits = 0u128;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b.is_ascii_punctuation() || b == b' ' {
                bits |= 1 << b;
            }
            i += 1;
        }
        EscapeSet(bits)
    }

    /// The characters the `cmigemo` command escapes: `\.*+^$/`.
    ///
    /// This is cmigemo's own choice and not enough for most flavors: `?`,
    /// `(`, `[`, `{` and `|` are left as they are.
    pub const fn cmigemo() -> Self {
        EscapeSet::new(r"\.*+^$/")
    }

    /// Whether `c` is in the set.
    pub fn contains(self, c: char) -> bool {
        let c = u32::from(c);
        c < 128 && self.0 & (1 << c) != 0
    }
}

impl fmt::Debug for EscapeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let chars: String = (b' '..=b'~')
            .map(char::from)
            .filter(|&c| self.contains(c))
            .collect();
        f.debug_tuple("EscapeSet").field(&chars).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_set_takes_punctuation_and_space() {
        let set = EscapeSet::new(" !/~{}a0\n\t\u{7f}あ");
        assert_eq!(format!("{set:?}"), r#"EscapeSet(" !/{}~")"#);
        assert!(set.contains(' '));
        assert!(!set.contains('a'));
        assert!(!set.contains('\n'));
        assert!(!set.contains('あ'));
        assert_eq!(EscapeSet::NONE, EscapeSet::new("abc\n"));
    }

    #[test]
    fn cmigemo_escape_set() {
        assert_eq!(
            format!("{:?}", EscapeSet::cmigemo()),
            r#"EscapeSet("$*+./\\^")"#
        );
    }
}
