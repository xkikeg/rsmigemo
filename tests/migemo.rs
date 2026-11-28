//! End-to-end queries against the test fixture.

use rsmigemo::{ConversionTable, Dictionary, Migemo, MigemoBuilder, TableKind};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/migemo-dict");
const SDICT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/migemo-sdict");
const ROMA2HIRA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/roma2hira.dat");

/// Like the C test: the fixture dictionary plus roma2hira only.
fn builder() -> MigemoBuilder {
    let mut builder = Migemo::builder();
    for kind in TableKind::ALL {
        builder = builder.table(kind, ConversionTable::new());
    }
    builder.table_file(TableKind::Roma2Hira, ROMA2HIRA).unwrap()
}

fn check(migemo: &Migemo) {
    assert_eq!(migemo.query("ak"), "([悪明朱秋紅赤]|ak|あ[かきくけこっ])");
    assert_eq!(migemo.query("n"), "[nなにぬねのん]");
    assert_eq!(migemo.query(""), "");
}

#[test]
fn text_dictionary() {
    let migemo = builder().dictionary_file(FIXTURE).unwrap().build();
    assert!(migemo.has_dictionary());
    check(&migemo);
}

#[test]
fn static_dictionary() {
    let text = builder().dictionary_file(FIXTURE).unwrap().build();
    let mut sdict = Vec::new();
    text.dictionary().write_sdict(&mut sdict).unwrap();
    let migemo = builder()
        .dictionary(Dictionary::from_sdict(&sdict).unwrap())
        .build();
    check(&migemo);
}

#[cfg(feature = "embedded-tables")]
#[test]
fn open_uses_sibling_and_embedded_tables() {
    // tests/data has no *.dat files, so the built-in tables are used.
    let migemo = Migemo::open(FIXTURE).unwrap();
    assert_eq!(migemo.query("aka"), "([朱紅赤]|aka|あか|アカ|ａｋａ|ｱｶ)");
}

#[test]
fn merged_dictionaries() {
    let extra = Dictionary::parse("あかい\t赤い\nあお\t青\n").unwrap();
    let migemo = builder()
        .dictionary_file(FIXTURE)
        .unwrap()
        .dictionary(extra)
        .build();
    assert_eq!(migemo.query("ao"), "(青|ao|あお)");
    assert_eq!(migemo.query("ak"), "([悪明朱秋紅赤]|ak|あ[かきくけこっ])");
}

#[test]
fn merged_dictionary_files() {
    // The second file is read straight into the merging builder.
    let migemo = builder()
        .dictionary_file(FIXTURE)
        .unwrap()
        .dictionary_file(SDICT)
        .unwrap()
        .dictionary(Dictionary::parse("あお\t青\n").unwrap())
        .build();
    check(&migemo);
    assert_eq!(migemo.query("ao"), "(青|ao|あお)");
}
