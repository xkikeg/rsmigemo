//! Strict loading against files on disk.

use std::fs;
use std::path::PathBuf;

use rsmigemo::{
    ConversionTable, Dictionary, Encoding, Error, LoadOptions, Migemo, ParseErrorKind, TableKind,
};

/// A fresh, empty directory under Cargo's per-target temporary directory.
fn dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn malformed_text_dictionary() {
    let path = dir("strict-text").join("migemo-dict");
    fs::write(&path, b"\xE3\x81\x82\t\xFF\n").unwrap(); // "あ\t" + an invalid byte
    let lenient = Dictionary::open(&path, Encoding::Utf8).unwrap();
    assert_eq!(lenient.lookup("あ").collect::<Vec<_>>(), ["\u{FFFD}"]);

    let strict = LoadOptions::new().encoding(Encoding::Utf8).strict(true);
    let err = Dictionary::open(&path, strict).unwrap_err();
    assert!(
        matches!(err, Error::Decode { offset: 4, path: Some(ref p) } if *p == path),
        "{err}"
    );
    let builder = Migemo::builder().encoding(Encoding::Utf8).strict(true);
    assert!(builder.dictionary_file(&path).is_err());
}

#[test]
fn malformed_table() {
    let dir = dir("strict-tables");
    let dict = dir.join("migemo-dict");
    fs::write(&dict, "あか\t赤\n").unwrap();
    let table = dir.join("roma2hira.dat");
    fs::write(&table, "a\tあ\nka\tか\nbroken\n").unwrap();

    // Lenient: the entries before the bad line are kept.
    let migemo = Migemo::open(&dict).unwrap();
    let roma2hira = migemo.table(TableKind::Roma2Hira);
    assert_eq!(roma2hira.convert_all("aka"), ["あか"]);

    let err = Migemo::builder()
        .strict(true)
        .dictionary_file(&dict)
        .unwrap()
        .sibling_tables(&dict)
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Parse {
                line: 3,
                kind: ParseErrorKind::MissingValue,
                ..
            }
        ),
        "{err}"
    );

    // Invalid bytes in a table.
    fs::write(&table, b"a\t\xFF\n").unwrap();
    assert!(ConversionTable::from_file(&table, Encoding::Utf8).is_ok());
    let strict = LoadOptions::new().encoding(Encoding::Utf8).strict(true);
    assert!(matches!(
        ConversionTable::from_file(&table, strict),
        Err(Error::Decode { offset: 2, .. })
    ));
}
