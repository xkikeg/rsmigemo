# rsmigemo and C/Migemo

rsmigemo is a Rust port of [C/Migemo](https://github.com/koron/cmigemo)
by MURAOKA Taro. The algorithms, the data formats and the conversion tables
all come from it, and so does most of the test suite. Thanks to its author
and contributors.

This page maps rsmigemo's modules to the C sources they port and lists
where the two behave differently. Source comments describe rsmigemo on its
own terms; look here for the correspondence.

## Modules

| rsmigemo        | cmigemo                                   |
|-----------------|-------------------------------------------|
| `encoding.rs`   | `charset.c` (`charset_detect_buf`)        |
| `syntax.rs`     | `migemo_set_operator`, `migemo_set_escape_chars` in `migemo.c` |
| `convert.rs`    | `romaji.c`; its tests are from `test/romaji.c` |
| `dict/mod.rs`   | `mtree.c`; the trie layout is `stree.c`'s (`stree_fill_mnode`) |
| `dict/text.rs`  | `mtree_load` in `mtree.c` |
| `io.rs`         | `ioreader_*.c` |
| `pattern.rs`    | `rxgen.c`; its tests are from `test/rxgen.c` |

## Differences

- **Encoding detection.** The scoring is the same, but UTF-8 is validated
  by the Rust standard library, which rejects overlong forms, surrogates,
  code points above U+10FFFF and 5- and 6-byte sequences. cmigemo accepts
  them, so such files can be detected differently. Well-formed files are
  detected the same way.
- **EUC-JP.** Six JIS X 0208 codes (`〜‖−¢£¬`) decode to their JIS forms,
  as with glibc `iconv`, not to the Microsoft forms the WHATWG decoder uses.
  cmigemo converts nothing: it matches the bytes as they are.
- **Character classes.** cmigemo escapes inside `[…]` with the same set as
  outside, which breaks some classes: `+ - .` give `[\+-\.]`, a range that
  also matches `,`, and in POSIX ERE, Emacs and Vim the `\` adds a literal
  backslash. rsmigemo has a separate `Syntax::class_escape`. With the
  default and Emacs syntax nothing is escaped in a class and `\ ] ^ -` are
  written outside it (`([+.]|-)`). With Vim only `\ ] ^ -` are escaped
  (`[+\-.]`). Patterns without ASCII symbols in a class are the same.
- **Conversion-table `REMAIN`.** A `REMAIN` must be shorter than its key,
  or the line is an error (lenient loading keeps the lines before it). Such
  a rule (`a x a`) makes cmigemo loop forever on a query. Every rule in the
  standard tables shrinks.
