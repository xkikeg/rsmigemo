//! migemo-dict text format.
//!
//! ```text
//! ; comment (only at the start of a line)
//! あか<TAB>赤<TAB>紅<TAB>朱
//! ```
//!
//! The reading is everything up to the first tab, after leading whitespace.
//! Words are separated by tabs; leading whitespace of each word is dropped,
//! and empty words are skipped. A reading with no words is ignored.
//!
//! Trailing whitespace is kept, as cmigemo keeps it: it skips whitespace
//! before each field and then reads up to the next tab. Only a CR before the
//! LF is dropped.

use crate::ctype::is_space;
use crate::error::{Error, ParseErrorKind, Result};

pub(super) fn parse(text: &str, mut add: impl FnMut(&str, &str)) -> Result<()> {
    // `lines` drops a CR before each LF.
    for (i, line) in text.lines().enumerate() {
        if line.starts_with(';') {
            continue;
        }
        let line = line.trim_start_matches(is_space);
        if line.is_empty() {
            continue;
        }
        let Some((reading, words)) = line.split_once('\t') else {
            return Err(Error::parse(i + 1, ParseErrorKind::MissingTab));
        };
        for word in words.split('\t') {
            let word = word.trim_start_matches(is_space);
            if !word.is_empty() {
                add(reading, word);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> Result<Vec<(String, String)>> {
        let mut v = Vec::new();
        parse(text, |k, w| v.push((k.to_owned(), w.to_owned())))?;
        Ok(v)
    }

    #[test]
    fn format() {
        let got = entries(concat!(
            ";comment\n",
            "\n",
            "  \t\n",
            "あか\t赤\t\t 紅\r\n",
            "  いか\t烏賊 \n",
            "うに\t\n",
        ))
        .unwrap();
        let want = [("あか", "赤"), ("あか", "紅"), ("いか", "烏賊 ")];
        assert_eq!(got, want.map(|(k, w)| (k.to_owned(), w.to_owned())));
    }

    #[test]
    fn missing_tab() {
        let err = entries("あか\t赤\nいか 烏賊\n").unwrap_err();
        assert!(matches!(
            err,
            Error::Parse {
                line: 2,
                kind: ParseErrorKind::MissingTab,
                ..
            }
        ));
    }
}
