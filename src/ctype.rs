//! Character classes as C's `<ctype.h>` defines them, for the text formats
//! shared with cmigemo.

/// C `isspace` in the "C" locale: ASCII whitespace, plus the vertical tab
/// (`\x0b`) that [`char::is_ascii_whitespace`] leaves out.
pub(crate) fn is_space(c: char) -> bool {
    c.is_ascii_whitespace() || c == '\x0b'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_c_isspace() {
        for b in 0u8..=0x7f {
            let c_isspace = matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
            assert_eq!(is_space(char::from(b)), c_isspace, "{b:#04x}");
        }
        assert!(!is_space('\u{3000}'));
    }
}
