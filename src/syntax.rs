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
    pub group_open: Cow<'static, str>,
    /// Group end, typically `)`.
    pub group_close: Cow<'static, str>,
    /// Character class start, typically `[`.
    pub class_open: Cow<'static, str>,
    /// Character class end, typically `]`.
    pub class_close: Cow<'static, str>,
    /// Written between the characters of a word, so that a match can run
    /// across a line break. With [`vim()`](Self::vim) it is `\_s*`, and
    /// `あい` becomes `あ\_s*い`. Empty to match within a line only.
    pub newline: Cow<'static, str>,
    /// Characters written with a `\` in front, outside character classes.
    pub escape: EscapeSet,
    /// Characters written with a `\` in front inside a character class.
    ///
    /// Flavors differ here: Vim escapes only [`EscapeSet::CLASS_SPECIAL`]
    /// inside a class, and POSIX ERE and Emacs cannot escape at all, because a
    /// `\` there is literal. A character of [`EscapeSet::CLASS_SPECIAL`] that
    /// is not in this set is written outside the class, as an alternative of
    /// its own: `a`, `b` and `-` become `([ab]|-)`.
    pub class_escape: EscapeSet,
}

impl Syntax {
    /// The syntax of the `cmigemo` command with no options: `|`, `(…)`,
    /// `[…]`, no newline pattern, and [`EscapeSet::cmigemo()`].
    ///
    /// Nothing is escaped inside a class, so [`EscapeSet::CLASS_SPECIAL`]
    /// characters go outside it. That makes classes valid both in POSIX ERE,
    /// where a `\` in a class is literal, and in Perl-style flavors. cmigemo
    /// escapes classes like the rest of the pattern, which breaks both.
    ///
    /// It is close to POSIX ERE but is not a standard flavor; pick the
    /// preset that matches your regex engine.
    pub fn cmigemo() -> Self {
        Syntax {
            or: "|".into(),
            group_open: "(".into(),
            group_close: ")".into(),
            class_open: "[".into(),
            class_close: "]".into(),
            newline: "".into(),
            escape: EscapeSet::cmigemo(),
            class_escape: EscapeSet::NONE,
        }
    }

    /// Vim, as `cmigemo -v`: `\|`, `\%(…\)`, and `\_s*` between characters.
    ///
    /// Inside a class only [`EscapeSet::CLASS_SPECIAL`] is escaped: Vim keeps
    /// the `\` of any other `\x` there as a literal backslash.
    pub fn vim() -> Self {
        Syntax {
            or: r"\|".into(),
            group_open: r"\%(".into(),
            group_close: r"\)".into(),
            newline: r"\_s*".into(),
            class_escape: EscapeSet::CLASS_SPECIAL,
            ..Syntax::cmigemo()
        }
    }

    /// Emacs, as `cmigemo -e`: `\|`, `\(…\)`, and `\s-*` between characters.
    ///
    /// A `\` in an Emacs class is literal, so nothing is escaped there and
    /// [`EscapeSet::CLASS_SPECIAL`] characters go outside the class.
    pub fn emacs() -> Self {
        Syntax {
            or: r"\|".into(),
            group_open: r"\(".into(),
            group_close: r"\)".into(),
            newline: r"\s-*".into(),
            ..Syntax::cmigemo()
        }
    }

    /// The [`regex`](https://docs.rs/regex) crate: non-capturing groups, and
    /// every metacharacter in dictionary words escaped, inside classes too,
    /// so the pattern always compiles.
    pub fn rust_regex() -> Self {
        const META: EscapeSet = EscapeSet::new(r"\.+*?()|[]{}^$#&-~");
        Syntax {
            group_open: "(?:".into(),
            escape: META,
            class_escape: META,
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

    /// The characters that can change the meaning of a character class:
    /// `\ ] ^ -`.
    ///
    /// `[` is not among them: it is literal in a POSIX or Emacs class, and the
    /// characters that would make it special there (`[:`, `[.`, `[=`) sort
    /// before it, so they never follow it. Flavors with nested classes, such
    /// as the `regex` crate, escape it in [`Syntax::class_escape`].
    pub const CLASS_SPECIAL: EscapeSet = EscapeSet::new(r"\]^-");

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
    fn class_special() {
        assert_eq!(
            format!("{:?}", EscapeSet::CLASS_SPECIAL),
            r#"EscapeSet("-\\]^")"#
        );
        assert_eq!(Syntax::vim().class_escape, EscapeSet::CLASS_SPECIAL);
        assert_eq!(Syntax::cmigemo().class_escape, EscapeSet::NONE);
        assert_eq!(Syntax::emacs().class_escape, EscapeSet::NONE);
    }

    #[test]
    fn cmigemo_escape_set() {
        assert_eq!(
            format!("{:?}", EscapeSet::cmigemo()),
            r#"EscapeSet("$*+./\\^")"#
        );
    }
}
