# rsmigemo — design

A Rust port of [C/Migemo](https://github.com/koron/cmigemo) (MIT, ported from
v1.8.0 @ `7365959`). It turns a romaji query into a regular expression that
matches Japanese text. For example, `kensaku` becomes `(kensaku|けんさく|ケンサク|検索|…)`.

## Goals

1. **Same data.** It reads the same files cmigemo reads, unchanged:
   - `migemo-dict` text dictionaries in UTF-8, CP932 or EUC-JP, optionally `.zst`
   - `roma2hira.dat`, `hira2kata.dat`, `han2zen.dat`, `zen2han.dat`
   - `migemo-sdict` binary static dictionaries (`MGS1`)
2. **Same output.** With a UTF-8 dictionary, a query returns exactly the same
   pattern bytes as `migemo_query()`. Golden tests check this against cmigemo.
3. **A normal Rust API.** It takes `&str` and returns `String`. Fallible
   calls return `Result`. Setup goes through a builder. `Migemo` is immutable
   and `Send + Sync`, so `query(&self)` can run from many threads. There are
   no out-parameters, numeric dict IDs or function-pointer charset hooks.

Non-goals: a C ABI or `migemo.h` compatibility layer. Also out of scope for
now: WASM bindings, and a dictionary *generation* pipeline (SKK → migemo-dict
stays in cmigemo's perl scripts).

## Key decision: Unicode inside, legacy encodings only at the I/O edge

cmigemo works on raw bytes. A pair of `char2int`/`int2char` function pointers
turns them into `unsigned int` "codes": Unicode scalars for UTF-8, and
`(lead << 8) | trail` for CP932/EUC-JP. Every tree is keyed by those codes,
and the output pattern is written in the dictionary's encoding.

rsmigemo decodes each input file to UTF-8 once, when it is loaded:

| Input                 | How                                                          |
|-----------------------|--------------------------------------------------------------|
| UTF-8 text            | `String::from_utf8_lossy`, leading BOM stripped              |
| CP932 / EUC-JP text   | `encoding_rs` (`SHIFT_JIS`/`EUC_JP`) → `String`              |
| `Encoding::Auto`      | port of `charset_detect_buf` (first 4 KiB heuristic)         |
| sdict, charset=UTF8   | codes are already `char`s; word buffer is validated as UTF-8 |
| sdict, CP932/EUC-JP   | each 1–2 byte code → `char` and word buffer decoded at load  |

All the internal structures work on `char` and `str`.

**Consequences**
- UTF-8 dictionaries produce byte-identical output. This is the default
  everywhere except Windows.
- CP932/EUC-JP dictionaries produce an *equivalent* regex in UTF-8, but the
  order of characters inside `[...]` and of `|` branches can differ, because
  siblings sort by Unicode scalar rather than by the legacy code. Callers who
  need legacy bytes re-encode the `String`. **This is accepted.**
- Invalid byte sequences, which cmigemo passes through as raw codes, are not
  representable. They are replaced with U+FFFD (see *Encoding / I/O*).

## Crate layout

```
rsmigemo/
├─ Cargo.toml            # lib + bin; features below; MIT OR Apache-2.0
├─ LICENSE-MIT, LICENSE-APACHE
├─ data/                 # the 4 *.dat tables copied from cmigemo (MIT, with LICENSE)
├─ src/
│  ├─ lib.rs             # re-exports; crate docs
│  ├─ migemo.rs          # Migemo, MigemoBuilder, query pipeline
│  ├─ syntax.rs          # Syntax (operators + escape set), presets
│  ├─ pattern.rs         # PatternBuilder  (≙ rxgen.c)
│  ├─ convert.rs         # ConversionTable (≙ romaji.c), .dat parser
│  ├─ dict/
│  │  ├─ mod.rs          # Dictionary (flat trie), lookup, DictionaryBuilder
│  │  ├─ text.rs         # migemo-dict text parser   (≙ mtree_load)
│  │  └─ sdict.rs        # MGS1 read/write           (≙ stree.c)
│  ├─ encoding.rs        # Encoding enum, detection, decode helpers (≙ charset.c)
│  ├─ io.rs              # open file, transparent .zst (feature "zstd") (≙ ioreader)
│  ├─ error.rs           # Error, ParseErrorKind, Result
│  └─ main.rs            # `rsmigemo` CLI compatible with `cmigemo`
├─ scripts/
│  └─ compare-with-cmigemo.sh  # byte-for-byte comparison against a cmigemo binary
└─ tests/
   ├─ migemo.rs          # cmigemo's test/migemo.c cases (text + sdict), merging
   ├─ regex.rs           # Syntax::rust_regex always compiles (feature "regex")
   └─ data/              # test migemo-dict fixture (from cmigemo test/_testdata)
```

Features: `default = ["embedded-tables"]`, `zstd`, `regex`.
Dependencies: `encoding_rs`, `smallvec`. Optional: `zstd`, `regex`.
No parser-combinator or argument-parser crate is used; see *Parsing* and *CLI*.

## Public API (sketch)

```rust
use rsmigemo::{Migemo, Syntax, Encoding};

// The usual case: like migemo_open(). Text or sdict (sniffed by the MGS1
// header). Loads the sibling *.dat files if present, else the built-in ones.
let migemo = Migemo::open("dict/utf-8/migemo-dict")?;
let pattern: String = migemo.query("kensaku");

// Full control
let migemo = Migemo::builder()
    .encoding(Encoding::Auto)                 // Auto | Utf8 | Cp932 | EucJp
    .dictionary_file("migemo-dict")?          // can be repeated (subdicts, -s)
    .dictionary_file("user-dict")?
    .table_file(TableKind::Roma2Hira, "roma2hira.dat")?   // strict
    .table(TableKind::Han2Zen, ConversionTable::new())    // disable one
    .sibling_tables("migemo-dict")?           // lenient, like Migemo::open
    .syntax(Syntax::vim().without_newline())
    .build();                                 // unset tables → built-in

// Static dictionary (sdict)
migemo.dictionary().write_sdict(File::create("out.sdict")?)?;
let dict = Dictionary::from_sdict(&std::fs::read("out.sdict")?)?;

// Per-call syntax override; optional compiled regex
let p = migemo.query_with("aka", &Syntax::rust_regex());
#[cfg(feature = "regex")] let re: regex::Regex = migemo.regex("aka")?;
```

### Types

| Rust                         | cmigemo equivalent                                              |
|------------------------------|-----------------------------------------------------------------|
| `Migemo` (immutable)         | `struct migemo`, `migemo_query`, `migemo_release`               |
| `MigemoBuilder`              | `migemo_open`, `migemo_load(dict_id, …)`, `migemo_set_*`        |
| `Syntax`                     | `migemo_set_operator`, `migemo_set_escape_chars`, `-v`/`-e`/`-n` |
| `Dictionary`                 | `mtree` *and* `stree` (see below)                               |
| `DictionaryBuilder`          | incremental `mtree_load`                                        |
| `ConversionTable`            | `romaji` (used for all 4 tables)                                |
| `PatternBuilder`             | `rxgen`                                                         |
| `Encoding`                   | `CHARSET_*`, `charset_detect_file`                              |
| `Error`                      | `NULL` / 0 return codes                                         |

What is dropped or reshaped:
- `migemo_setproc_char2int/int2char`: replaced by `Encoding`.
- `migemo_is_enable`: becomes `Migemo::has_dictionary()`. A `Migemo` without a
  dictionary still does kana/width conversion, as in C.
- `migemo_switch_sdict` with its PRESERVE/RELEASE flags: gone, because there
  is only one dictionary representation (next section).
- Operator max length (8 bytes): gone.

## Components

### `Dictionary`: one flat trie, which is also the sdict layout

cmigemo has two structures. `mtree` is a weight-balanced ternary search tree
built during load. `stree` is a flat array of `{code, start, end, word_idx}`
with binary search over children, which is faster and is what gets
serialized. rsmigemo keeps **only the flat form**:

```rust
struct Node { ch: char, children: Range<u32>, words: u32 /* offset or NONE */ }
pub struct Dictionary { nodes: Vec<Node>, words: String /* "\0"-separated lists */ }
```

- `DictionaryBuilder` collects `key → Vec<word>` (a `BTreeMap<String, Vec<String>>`,
  or a temporary `char` trie) across any number of text files, then
  `build()` lays the nodes out with the same recursion as `stree_fill_mnode`.
  A `DictionaryBuilder::from(&Dictionary)` lets you add more files later,
  which replaces C's "preserve mdict".
- **Layout.** Each node's children sit in one contiguous block, sorted by
  `char`. Blocks are placed depth-first: a node's children block comes first,
  then the blocks of the first child's subtree, and so on. As a result every
  subtree occupies one contiguous range of `nodes`, and its words occupy one
  contiguous range of `words`. This is true of every sdict cmigemo writes.
- **Lookup.** `lookup(prefix) -> Words<'_>` binary-searches each level's
  children block, one step per char of `prefix`. At the matching node it reads
  that node's precomputed `word_span: Range<u32>`, which covers the node's own
  words and all of its descendants' words. It returns that slice of `words`
  and iterates it by splitting on `\0`, so there is no recursion and no
  pointer-chasing. The spans are kept in a side array, `Vec<Range<u32>>`,
  computed once when the dictionary is built or loaded. When reading an sdict,
  if a subtree turns out not to be contiguous (a hand-made file), the
  dictionary is laid out again instead of being rejected.

**Why a flat trie.** cmigemo builds a pointer ternary search tree (TST) and
offers its flat sdict form as the faster option ("cache locality"). The flat
form wins on every part of a query:

| Step | Pointer TST (mtree) | Flat trie |
|---|---|---|
| Find the prefix node | Pointer chase per sibling and child, nodes scattered across the heap | Binary search in a small contiguous block per char |
| Gather descendant words | Recursive walk through every descendant, each word a separate allocation | One sequential scan of a contiguous `&str` |
| Memory | 40 B per node plus a heap allocation per word | 16 B per node plus 8 B for the span, one string buffer |
| Load | Allocates during parse | Two `Vec`s; sdict is almost a `memcpy` |

Prefix lookup is not the bottleneck, though. A short query like `a` or `k`
matches tens of thousands of words, and the cost is adding them to the
`PatternBuilder`. That is where the optimization effort goes (next section).
Benchmarks settle the details.
- `read_sdict(R)` / `write_sdict(W)` use the exact C on-disk layout:
  ```
  header  16 B : id "MGS1" | u32 node_count | u32 word_buf_size | u8 charset | 3 B pad
  nodes   16 B × node_count : u32 code | u32 start | u32 end | u32 word_idx
  words   word_buf_size B   : per node: word\0 word\0 … \0
  ```
  All values are little-endian. C writes native-endian, which is LE on every
  platform it ships on. `word_idx`/node-id `0xFFFFFFFF` means none. Node 0 is
  the virtual root. We always write `charset = 3` (UTF-8).

Word order within a key and traversal order do not affect the output, because
`PatternBuilder` is order-independent (below).

### Parsing: hand-written (no `winnow`)

Both text formats are line-based. Their rules are quirky ("##" means a literal
`#`, ASCII-only `isspace`, `;` comments, tab-only separators in migemo-dict),
and those quirks have to match cmigemo exactly. A hand-written parser is a
`split(b'\n')` loop over the decoded `&str` plus a small field splitter, about
50 lines per format. It never allocates per line, since keys and words are
borrowed until they are inserted. It reports line numbers for errors, so
`winnow` would add a dependency without simplifying anything. The binary sdict
reader is hand-written too, using fixed-size little-endian reads with
`u32::from_le_bytes` and bounds checks.

### `PatternBuilder` hot path

Every dictionary hit passes through `PatternBuilder::add`, so it must be cheap:
- Nodes live in a `Vec<Node>` arena that is cleared and reused between
  segments, so it allocates nothing once it has warmed up.
- Each node's children are a sorted `SmallVec<(char, u32)>`: a linear scan
  while short, binary search once long. The root level is the one that gets
  wide.
- A word stops being added at the first node already marked `wordtail`
  (shorter-wins), which ends most long words early.

### `ConversionTable` (≙ `romaji.c`)

One type serves all four `.dat` tables.
- **Parser rules, kept exactly**: fields are separated by ASCII `isspace`, as
  `KEY VALUE [REMAIN]`. A line starting with `#` is a comment; one starting
  with `##` is a key that starts with `#`. Blank lines are skipped.
- **Insert rules, kept exactly**: if a shorter key already has a value, the
  longer key is ignored. A new key drops the subtree of any longer keys under
  it. A duplicate key keeps the **first** value. (`romaji.c` has a "replace"
  branch, but it can never be reached.)
- `convert_all(&self, &str) -> Vec<String>` runs the greedy longest-match
  conversion. The `REMAIN` string (for example `kk → っ` + push back `k`) is
  pushed back onto the input. An unmatched char passes through. Pending input
  at the end expands to *every* completion below the pending node, deduped.
  This is why `ak` gives `あ[かきくけこっ]`.
- The trie is a small arena (`Vec<Node>`) whose children are sorted index
  lists, so inserts can prune subtrees in place. Tables have a few hundred
  entries, so the flat layout would buy nothing here.
- `parse` is strict: it returns the error for the first malformed line.
  `Migemo::open` and `MigemoBuilder::sibling_tables` copy cmigemo instead.
  They keep the entries before the bad line and ignore the error.
- The four tables are compiled in with `include_str!` (feature `embedded-tables`),
  so `Migemo::builder().build()` works with no files present.

### `PatternBuilder` (≙ `rxgen.c`)

- A word trie with **shorter-wins pruning**. Adding `赤ちゃん` after `赤` is a
  no-op. Adding `赤` after `赤ちゃん` cuts the subtree. The final trie is
  therefore the set of minimal words, independent of insertion order.
- Children sort by `char`, which is the same order as C's in-order BST walk
  over codes, so the output is byte-identical for UTF-8.
- `build(&Syntax) -> String` emits each level as follows:
  1. Emit `nest_in` if `siblings > 1 && any has children`.
  2. Emit the leaf siblings: one leaf bare, several wrapped in `select_in … select_out`.
  3. Emit the siblings that have children, joined by `or` (with a leading
     `or` if leaves were emitted). Each is written as char, then `newline`,
     then the child level.
  4. Emit `nest_out`.
  Characters in the escape set (ASCII 32–126 only) get a `\` prefix, including inside `[...]`.
- It is created fresh for each query, so `Migemo::query` needs only `&self`.
  It uses a `Vec<Node>` arena that is reused within one query.

### `Syntax`

```rust
pub struct Syntax {
    pub or: Cow<'static, str>,        // "|"
    pub nest_in: Cow<'static, str>,   // "("
    pub nest_out: Cow<'static, str>,  // ")"
    pub select_in: Cow<'static, str>, // "["
    pub select_out: Cow<'static, str>,// "]"
    pub newline: Cow<'static, str>,   // ""
    pub escape: EscapeSet,            // default "\\.*+^$/"
}
```
Presets: `Syntax::default()` (cmigemo default), `vim()`, `emacs()`, each with
`.with_newline(bool)` matching `-n`, and `rust_regex()`. The last escapes all
regex metacharacters (`\.+*?()|[]{}^$#&-~`), so the result is always valid
for the `regex` crate. cmigemo's default leaves `?|()[]{}` in dictionary words
unescaped.

### Query pipeline (`Migemo::query`, ≙ `migemo_query`)

```
query
 └─ segment by ASCII case:  "kensakuEngine" → ["kensaku", "Engine"]
    (a segment starting with ≥2 uppercase letters runs until the first non-uppercase char)
    for each segment s:                          (fresh PatternBuilder)
      add s
      lower = ASCII-lowercase(s)
      add dict.lookup(lower)
      add han2zen(s), zen2han(s)
      for h in roma2hira(lower):  add h, dict.lookup(h)
        for k in hira2kata(h):    add k, dict.lookup(k)
          for z in zen2han(k):    add z
      out += builder.build(syntax)
 └─ concatenate the segment patterns
```
Edge cases such as empty queries, a trailing uppercase letter and non-ASCII
input get pinned down by golden tests, not guessed.

### `Encoding` / I/O

- `enum Encoding { Auto, Utf8, Cp932, EucJp }`. With `Auto`, the first
  dictionary file is detected, and every file after it is read in the same
  encoding (as in C, where `mo->charset` is fixed by the first load). Tables
  loaded before any dictionary are detected on their own.
- Decoding is always lossy (U+FFFD). cmigemo passes invalid bytes through,
  so failing outright would be a regression.
- `encoding_rs` follows WHATWG, which decodes EUC-JP with the Microsoft
  (CP932) mapping for six JIS X 0208 characters. glibc `iconv` uses the JIS
  mapping, as does the UTF-8 dictionary cmigemo builds:

  | EUC-JP | CP932 | `encoding_rs` (both) | `iconv` EUC-JP | `iconv` CP932 |
  |---|---|---|---|---|
  | A1C1 | 8160 | U+FF5E `～` | U+301C `〜` | U+FF5E `～` |
  | A1C2 | 8161 | U+2225 `∥` | U+2016 `‖` | U+2225 `∥` |
  | A1DD | 817C | U+FF0D `－` | U+2212 `−` | U+FF0D `－` |
  | A1F1 | 8191 | U+FFE0 `￠` | U+00A2 `¢` | U+FFE0 `￠` |
  | A1F2 | 8192 | U+FFE1 `￡` | U+00A3 `£` | U+FFE1 `￡` |
  | A2CC | 81CA | U+FFE2 `￢` | U+00AC `¬` | U+FFE2 `￢` |

  So CP932 decodes as everywhere else, but an EUC-JP dictionary yields the
  right-hand column where the UTF-8 dictionary has the JIS characters. SKK-JISYO.L
  uses them in 47 entries (for example `kara → 〜`, `c → ¢`).
- `io::open(path)` wraps the file in a `zstd::Decoder` when the path ends in
  `.zst` (feature `zstd`). Text is read whole and then decoded. Dictionaries
  are a few MB, so streaming isn't needed.
- `Migemo::open` without the `zstd` feature returns `Error::UnsupportedCompression`
  for `.zst`.

### Errors

```rust
#[non_exhaustive]
pub enum Error {
    Io { path: PathBuf, source: io::Error },
    Parse { path: Option<PathBuf>, line: usize, kind: ParseErrorKind }, // MissingValue | MissingTab
    InvalidSdict { path: Option<PathBuf>, reason: &'static str },      // magic / truncation / ranges
    UnsupportedCompression { path: PathBuf },
}
```
cmigemo's silent behaviors are kept only where the C API documents them, which
is auto-loading sibling `.dat` files in `Migemo::open`.

The sdict reader validates every range: children must come after their
parent, and no node may be shared. A hostile file therefore can't cause an
infinite or exponential traversal. A file whose layout isn't searchable as is
(legacy charset, unsorted children, words not contiguous) is laid out again
through `DictionaryBuilder` instead of being rejected.

## CLI (`rsmigemo`, `src/main.rs`)

It takes the same flags as `tools/cmigemo.c`: `-d`, `-s/-u` (repeatable),
`--convert`, `--save`, `--sdict`, `-w`, `-v`, `-e`, `-n`, `-q`, and the same
dictionary search path order. It adds `-r/--rust` for `Syntax::rust_regex()`.
`--convert` is accepted and does nothing, because the dictionary is always
flat. Arguments are parsed by hand, as cmigemo does, so the binary needs no
feature flag or dependency. It reads queries line by line from stdin and
writes patterns to stdout in UTF-8.

## Testing

1. **Unit tests** per module, written from C behavior: `.dat` parse quirks,
   shorter/longer key rules, remain pushback, rxgen pruning and grouping.
2. **cmigemo's own cases** live in `tests/migemo.rs`, against its fixture:
   `ak → ([悪明朱秋紅赤]|ak|あ[かきくけこっ])` and `n → [nなにぬねのん]`.
3. **Byte-for-byte comparison** with `scripts/compare-with-cmigemo.sh <cmigemo>
   <migemo-dict> [flags]`. It runs 23,314 queries (all 1–3 letter strings,
   5,000 random syllable strings, case and symbol edge cases) through both
   binaries. Outputs from the full dictionary aren't committed, because they
   are derived from the GPL dictionary.
4. **sdict compatibility**: Rust reads a sdict that cmigemo wrote, and cmigemo
   `--sdict` reads one that Rust wrote. Both give identical results.

### Results (cmigemo 1.8.0 @ `7365959`, full SKK-JISYO.L dictionary)

| Check | Result |
|---|---|
| UTF-8, default / `-v` / `-e` / `-v -n` / `-e -n` | 23,314 / 23,314 identical, all five |
| `--save` sdict file | **byte-identical** to cmigemo's (8,570,159 B) |
| sdict, C → Rust and Rust → C | identical query output |
| CP932 text and C-written CP932 sdict | same characters per pattern (order differs, as accepted) |
| EUC-JP | same, except the six JIS/Microsoft variants (`〜‖−¢£¬`, see *Encoding*) |
| Query speed, 2,028 queries on sdict | 88 ms (C: 84 ms) |
| Open + query, text dict | 0.21 s (C: 0.05 s): per-word `String` allocations in `DictionaryBuilder` |
| Open + query, sdict | 22 ms (C: 5 ms): UTF-8 validation plus copying the word buffer |

## Next steps

- Faster text loading: keep borrowed `&str` entries plus one sort instead of
  `BTreeMap<String, Vec<String>>`.
- Faster sdict loading: take ownership of the file buffer instead of copying
  it, so there is no `to_owned`.
- Reuse the `PatternBuilder` arena between queries (thread-local).
- A `criterion` benchmark suite.

## Licensing / data

- rsmigemo is dual-licensed **MIT OR Apache-2.0** (`license = "MIT OR Apache-2.0"`
  in `Cargo.toml`, with `LICENSE-MIT` and `LICENSE-APACHE` at the root).
- The `.dat` tables and the ported algorithms come from cmigemo (MIT,
  © MURAOKA Taro). MIT permits relicensing a derived work as long as the
  original notice is kept, so cmigemo's notice is reproduced in
  `LICENSE-MIT` and `data/LICENSE-cmigemo`, and the README credits it.
- `migemo-dict` comes from SKK-JISYO.L, which is **GPL**. It is not bundled
  or embedded. Users build it with cmigemo's pipeline or download it, and
  tests that need it are skipped when it is absent.

## Implementation order (done)

1. `encoding`, `error`, `io`
2. `ConversionTable` + embedded tables
3. `PatternBuilder` + `Syntax`
4. `Dictionary` (text → flat) + `lookup`
5. `Migemo` + query pipeline
6. sdict read/write + compatibility checks
7. CLI and full-dictionary comparison
