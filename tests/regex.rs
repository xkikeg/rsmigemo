#![cfg(feature = "regex")]

#[cfg(feature = "embedded-tables")]
use rsmigemo::{Dictionary, Migemo};
use rsmigemo::{PatternBuilder, Syntax};

// Uses the built-in tables.
#[cfg(feature = "embedded-tables")]
#[test]
fn rust_regex_syntax_always_compiles() {
    // Words full of metacharacters, under readings reachable from "a".
    let mut text = String::new();
    for (i, c) in (b'!'..=b'~').filter(u8::is_ascii_punctuation).enumerate() {
        let c = c as char;
        text += &format!("あ{i}\t{c}\tx{c}y\t[{c}]\t{c}{c}\n");
    }
    let migemo = Migemo::builder()
        .dictionary(Dictionary::parse(&text).unwrap())
        .build();
    for q in [
        "a", "A", "aA", "a+b?c.d", "[x]", "(", r"\", "^$", "a|b", "#", "-", "&&", "~~",
    ] {
        let re = migemo.regex(q).unwrap_or_else(|e| panic!("{q:?}: {e}"));
        assert!(re.is_match(q), "{q:?} → {re}");
    }
}

// Uses the built-in tables.
#[cfg(feature = "embedded-tables")]
#[test]
fn matches_japanese_text() {
    let dict = Dictionary::parse("けんさく\t検索\n").unwrap();
    let migemo = Migemo::builder().dictionary(dict).build();
    let re = migemo.regex("kensaku").unwrap();
    for text in [
        "全文検索",
        "けんさくする",
        "ケンサク",
        "ｹﾝｻｸ",
        "kensaku",
        "ｋｅｎｓａｋｕ",
    ] {
        assert!(re.is_match(text), "{text}");
    }
    assert!(!re.is_match("検査"));
}

#[test]
fn rust_regex_class_matches_exactly_its_characters() {
    // Every ASCII punctuation character and space as one-character words:
    // one class, with `\ ] ^ -` and the class-set operators escaped.
    let words: Vec<char> = (b' '..=b'~')
        .filter(|&b| b.is_ascii_punctuation() || b == b' ')
        .map(char::from)
        .collect();
    let mut builder = PatternBuilder::new();
    for c in &words {
        builder.add(&c.to_string());
    }
    let pattern = builder.build(&Syntax::rust_regex());
    let re = regex::Regex::new(&format!("^(?:{pattern})$")).unwrap();
    for c in (0u8..=0x7f).map(char::from) {
        assert_eq!(
            re.is_match(&c.to_string()),
            words.contains(&c),
            "{c:?} in {pattern}"
        );
    }
}
