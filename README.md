# rsmigemo

Generate regular expressions that match Japanese text from romaji queries.
This is a Rust port of [C/Migemo](https://github.com/koron/cmigemo).

```rust
let migemo = rsmigemo::Migemo::open("dict/utf-8/migemo-dict")?;
assert_eq!(
    migemo.query("kensaku"),
    "(kensaku|けんさく|ケンサク|建策|憲[作冊]|検索|献策|研削|羂索|ｋｅｎｓａｋｕ|ｹﾝｻｸ)"
);
```

- **Same data as cmigemo.** It reads `migemo-dict` text dictionaries (UTF-8,
  CP932, EUC-JP, optionally `.zst`), the `*.dat` conversion tables, and
  `migemo-sdict` static dictionaries. Static dictionaries it writes are
  byte-identical to cmigemo's.
- **Same output.** With a UTF-8 dictionary, patterns are byte-identical to
  `cmigemo` for the default, vim and emacs syntaxes, except where cmigemo
  writes a broken character class: about 20 of the 23,314 queries the
  comparison script tries (see [CMIGEMO.md](CMIGEMO.md)). Check it yourself
  with `scripts/compare-with-cmigemo.sh`.
- **A normal Rust API.** It takes `&str` and returns `String`, setup goes
  through a builder, errors come back as `Result`, and `Migemo` is
  `Send + Sync`. `Syntax::rust_regex()` and the `regex` feature give patterns
  that always compile with the `regex` crate.

## Dictionary

The dictionary is not included, because it is generated from SKK-JISYO.L,
which is GPL. Build it with cmigemo (`cmake -B build && cmake --build build`
produces `build/dict/mdict/utf-8/migemo-dict`) or get it from a cmigemo
release. The four conversion tables are built in (feature `embedded-tables`).

## CLI

```console
$ cargo run --release -- -d path/to/migemo-dict -w kensaku
$ cargo run --release -- -d path/to/migemo-dict --save migemo-sdict -w a
$ echo nihongo | cargo run --release -- --sdict migemo-sdict -q -v
```

The flags are the same as `cmigemo`'s, plus `-r/--rust` for `regex`-crate syntax.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

The algorithms and the conversion tables in `data/` are derived from C/Migemo
by MURAOKA Taro, under the MIT license (see `data/LICENSE-cmigemo`).
