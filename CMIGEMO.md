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

## Differences

- **Encoding detection.** The scoring is the same, but UTF-8 is validated
  by the Rust standard library, which rejects overlong forms, surrogates,
  code points above U+10FFFF and 5- and 6-byte sequences. cmigemo accepts
  them, so such files can be detected differently. Well-formed files are
  detected the same way.
- **EUC-JP.** Six JIS X 0208 codes (`〜‖−¢£¬`) decode to their JIS forms,
  as with glibc `iconv`, not to the Microsoft forms the WHATWG decoder uses.
  cmigemo converts nothing: it matches the bytes as they are.
