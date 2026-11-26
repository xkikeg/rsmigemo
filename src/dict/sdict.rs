//! The static dictionary (sdict) binary format, shared with cmigemo.
//!
//! ```text
//! header  16 B : "MGS1" | u32 node_count | u32 word_buf_size | u8 encoding | 3 B padding
//! nodes   16 B × node_count : u32 code | u32 start | u32 end | u32 word_idx
//! words   word_buf_size B   : per node with words: word\0 … word\0 \0
//! ```
//!
//! Integers are little-endian. cmigemo writes native-endian, which is
//! little-endian on every platform it ships on. Node 0 is the root, node `i`'s
//! children are `start..end`, and `0xFFFFFFFF` marks "no words". The encoding
//! byte is 0 (none: raw bytes), 1 (CP932), 2 (EUC-JP) or 3 (UTF-8).

use std::io;

use super::{Dictionary, DictionaryBuilder, NONE, Node};
use crate::encoding::Encoding;
use crate::error::{Error, Result};

const MAGIC: &[u8; 4] = b"MGS1";
const HEADER_LEN: usize = 16;
const NODE_LEN: usize = 16;
const ENCODING_UTF8: u8 = 3;

pub(super) fn is_sdict(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn invalid(reason: &'static str) -> Error {
    Error::InvalidSdict { path: None, reason }
}

/// A node as stored.
#[derive(Debug, Clone, Copy)]
struct RawNode {
    /// The character: a Unicode scalar, or a legacy code (see
    /// [`Encoding::decode_code`]).
    code: u32,
    start: u32,
    end: u32,
    word_idx: u32,
}

pub(super) fn read(bytes: &[u8]) -> Result<(Dictionary, Encoding)> {
    if bytes.len() < HEADER_LEN {
        return Err(invalid("truncated header"));
    }
    if !is_sdict(bytes) {
        return Err(invalid("missing MGS1 signature"));
    }
    let u32_at = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    let node_count = u32_at(4) as usize;
    let word_len = u32_at(8) as usize;
    let encoding = Encoding::from_sdict_id(bytes[12]).ok_or(invalid("unknown encoding"))?;
    if node_count == 0 {
        return Err(invalid("no root node"));
    }
    let nodes_end = node_count
        .checked_mul(NODE_LEN)
        .and_then(|n| n.checked_add(HEADER_LEN))
        .ok_or(invalid("truncated data"))?;
    let total = nodes_end
        .checked_add(word_len)
        .ok_or(invalid("truncated data"))?;
    if bytes.len() < total {
        return Err(invalid("truncated data"));
    }

    let raw: Vec<RawNode> = (0..node_count)
        .map(|i| {
            let off = HEADER_LEN + i * NODE_LEN;
            RawNode {
                code: u32_at(off),
                start: u32_at(off + 4),
                end: u32_at(off + 8),
                word_idx: u32_at(off + 12),
            }
        })
        .collect();
    let buf = &bytes[nodes_end..total];

    // Children always come after their parent, which rules out cycles, and no
    // node has two parents, which rules out exponential traversals.
    let mut has_parent = vec![false; node_count];
    for (
        i,
        &RawNode {
            start,
            end,
            word_idx,
            ..
        },
    ) in raw.iter().enumerate()
    {
        let (start, end) = (start as usize, end as usize);
        if start > end || end > node_count || (start < end && start <= i) {
            return Err(invalid("child range out of bounds"));
        }
        if word_idx != NONE && word_idx as usize >= word_len {
            return Err(invalid("word offset out of bounds"));
        }
        for parent in &mut has_parent[start..end] {
            if std::mem::replace(parent, true) {
                return Err(invalid("node shared by several parents"));
            }
        }
    }

    let dict = match direct(&raw, buf, encoding) {
        Some(dict) => dict,
        None => rebuild(&raw, buf, encoding)?,
    };
    Ok((dict, encoding))
}

/// Use the file's layout as is. Works for any UTF-8 sdict written by cmigemo
/// or by us; returns `None` if the layout is not one we can search directly.
fn direct(raw: &[RawNode], buf: &[u8], encoding: Encoding) -> Option<Dictionary> {
    if encoding != Encoding::Utf8 {
        return None;
    }
    let words = std::str::from_utf8(buf).ok()?;
    let nodes = raw
        .iter()
        .enumerate()
        .map(
            |(
                i,
                &RawNode {
                    code,
                    start,
                    end,
                    word_idx,
                },
            )| {
                let ch = if i == 0 { '\0' } else { char::from_u32(code)? };
                Some(Node {
                    ch,
                    start,
                    end,
                    word_idx,
                })
            },
        )
        .collect::<Option<Vec<_>>>()?;
    let sorted = nodes.iter().all(|n| {
        nodes[n.start as usize..n.end as usize]
            .windows(2)
            .all(|w| w[0].ch < w[1].ch)
    });
    if !sorted {
        return None;
    }

    // Each subtree's words must be one contiguous range: own words, then each
    // child's range in order. Children have larger indices, so go backwards.
    let mut spans = vec![(0u32, 0u32); nodes.len()];
    for i in (0..nodes.len()).rev() {
        let node = nodes[i];
        let mut span: Option<(u32, u32)> = None;
        let mut extend = |(b, e): (u32, u32)| -> Option<()> {
            if b == e {
                return Some(());
            }
            span = match span {
                None => Some((b, e)),
                Some((sb, se)) if se == b => Some((sb, e)),
                Some(_) => return None,
            };
            Some(())
        };
        if node.word_idx != NONE {
            let w = node.word_idx as usize;
            if !words.is_char_boundary(w) {
                return None; // would split a character when sliced
            }
            let own_end = if buf[w] == 0 {
                w + 1
            } else {
                w + buf[w..].windows(2).position(|p| p == [0, 0])? + 2
            };
            extend((w as u32, own_end as u32))?;
        }
        for child in node.start..node.end {
            extend(spans[child as usize])?;
        }
        spans[i] = span.unwrap_or((0, 0));
    }
    Some(Dictionary {
        nodes,
        spans,
        words: words.to_owned(),
    })
}

/// Read every entry and lay the dictionary out afresh. Used for legacy
/// encodings and for files whose layout is not contiguous.
fn rebuild(raw: &[RawNode], buf: &[u8], encoding: Encoding) -> Result<Dictionary> {
    let mut builder = DictionaryBuilder::new();
    let mut visited = vec![false; raw.len()];
    let mut stack = vec![(0usize, String::new())];
    while let Some((i, reading)) = stack.pop() {
        if std::mem::replace(&mut visited[i], true) {
            return Err(invalid("node shared by several parents"));
        }
        let RawNode {
            start,
            end,
            word_idx,
            ..
        } = raw[i];
        if word_idx != NONE {
            let list = buf[word_idx as usize..]
                .split(|&b| b == 0)
                .take_while(|w| !w.is_empty());
            for word in list {
                builder.insert(&reading, &encoding.decode(word));
            }
        }
        for child in (start..end).rev() {
            let ch = encoding
                .decode_code(raw[child as usize].code)
                .unwrap_or('\u{FFFD}');
            stack.push((child as usize, format!("{reading}{ch}")));
        }
    }
    Ok(builder.build())
}

/// Write `dict` to `writer` and flush it. The words go out as they are, so
/// no second copy of the dictionary is made.
pub(super) fn write<W: io::Write>(dict: &Dictionary, mut writer: W) -> io::Result<()> {
    let mut header = [0u8; HEADER_LEN];
    header[..4].copy_from_slice(MAGIC);
    header[4..8].copy_from_slice(&(dict.nodes.len() as u32).to_le_bytes());
    header[8..12].copy_from_slice(&(dict.words.len() as u32).to_le_bytes());
    header[12] = ENCODING_UTF8;
    writer.write_all(&header)?;
    // Nodes in chunks, so an unbuffered writer is not called per node.
    let mut chunk = Vec::with_capacity(256 * NODE_LEN);
    for nodes in dict.nodes.chunks(256) {
        chunk.clear();
        for node in nodes {
            for v in [node.ch as u32, node.start, node.end, node.word_idx] {
                chunk.extend_from_slice(&v.to_le_bytes());
            }
        }
        writer.write_all(&chunk)?;
    }
    writer.write_all(dict.words.as_bytes())?;
    // A `BufWriter` passed by value would otherwise drop its tail's error.
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Dictionary {
        Dictionary::parse("あか\t赤\t紅\nあき\t秋\nあかい\t赤い\nいか\t烏賊\n").unwrap()
    }

    fn sorted(d: &Dictionary, prefix: &str) -> Vec<String> {
        let mut v: Vec<String> = d.lookup(prefix).map(str::to_owned).collect();
        v.sort();
        v
    }

    #[test]
    fn round_trip() {
        let d = sample();
        let mut bytes = Vec::new();
        d.write_sdict(&mut bytes).unwrap();
        let (back, enc) = read(&bytes).unwrap();
        assert_eq!(enc, Encoding::Utf8);
        let mut again = Vec::new();
        back.write_sdict(&mut again).unwrap();
        assert_eq!(bytes, again);
        for p in ["あ", "あか", "い", "う"] {
            assert_eq!(sorted(&d, p), sorted(&back, p));
        }
    }

    #[test]
    fn rejects_malformed() {
        let mut bytes = Vec::new();
        sample().write_sdict(&mut bytes).unwrap();
        assert!(read(&bytes[..20]).is_err());
        assert!(read(b"XXXX............").is_err());
        let mut bad = bytes.clone();
        bad[HEADER_LEN + 4..HEADER_LEN + 8].copy_from_slice(&0u32.to_le_bytes()); // root.start = 0
        assert!(read(&bad).is_err());
    }

    /// Serialize raw nodes and a word buffer as an sdict.
    fn sdict_bytes(raw: &[RawNode], words: &[u8], encoding: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(words.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[encoding, 0, 0, 0]);
        for n in raw {
            for v in [n.code, n.start, n.end, n.word_idx] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        bytes.extend_from_slice(words);
        bytes
    }

    fn node(code: char, start: u32, end: u32, word_idx: u32) -> RawNode {
        RawNode {
            code: u32::from(code),
            start,
            end,
            word_idx,
        }
    }

    #[test]
    fn rejects_shared_nodes() {
        // あ and い both claim か as their child.
        let raw = [
            node('\0', 1, 3, NONE),
            node('あ', 3, 4, NONE),
            node('い', 3, 4, NONE),
            node('か', 4, 4, 0),
        ];
        let err = read(&sdict_bytes(&raw, b"x\0\0", ENCODING_UTF8)).unwrap_err();
        assert!(err.to_string().contains("shared"), "{err}");
    }

    #[test]
    fn rejects_unknown_encoding() {
        let raw = [node('\0', 1, 1, NONE)];
        let err = read(&sdict_bytes(&raw, b"", 4)).unwrap_err();
        assert!(err.to_string().contains("encoding"), "{err}");
    }

    #[test]
    fn word_offset_inside_a_character_is_rebuilt() {
        // `word_idx` 1 points into the middle of あ. Searching the buffer
        // directly would panic when slicing, so it is rebuilt instead.
        let raw = [node('\0', 1, 2, NONE), node('a', 2, 2, 1)];
        let (d, _) = read(&sdict_bytes(&raw, "あ\0\0".as_bytes(), ENCODING_UTF8)).unwrap();
        assert!(d.lookup("a").all(|w| !w.is_empty()));
    }

    #[test]
    fn legacy_encoding_is_converted() {
        // Re-encode a UTF-8 sdict as CP932, the way cmigemo would write it.
        let d = sample();
        let mut raw: Vec<RawNode> = d
            .nodes
            .iter()
            .map(|n| RawNode {
                code: 0,
                start: n.start,
                end: n.end,
                word_idx: NONE,
            })
            .collect();
        let mut words = Vec::new();
        for (i, n) in d.nodes.iter().enumerate() {
            if i > 0 {
                let ch = n.ch.to_string();
                let (b, _, _) = encoding_rs::SHIFT_JIS.encode(&ch);
                raw[i].code = b.iter().fold(0, |acc, &x| acc << 8 | x as u32);
            }
            if n.word_idx != NONE {
                raw[i].word_idx = words.len() as u32;
                for w in d.own_words(n.word_idx) {
                    words.extend_from_slice(&encoding_rs::SHIFT_JIS.encode(w).0);
                    words.push(0);
                }
                words.push(0);
            }
        }
        let (back, enc) = read(&sdict_bytes(&raw, &words, 1)).unwrap();
        assert_eq!(enc, Encoding::Cp932);
        for p in ["あ", "あか", "い"] {
            assert_eq!(sorted(&d, p), sorted(&back, p));
        }
    }

    /// `tests/data/migemo-sdict` was written by cmigemo 1.8.0 from
    /// `tests/data/migemo-dict` (`cmigemo -d migemo-dict --save migemo-sdict`).
    /// Matching it byte for byte pins the on-disk layout, byte order
    /// included, on every host.
    #[test]
    fn matches_cmigemo_output() {
        let golden: &[u8] = include_bytes!("../../tests/data/migemo-sdict");
        let text = include_str!("../../tests/data/migemo-dict");
        let mut written = Vec::new();
        Dictionary::parse(text)
            .unwrap()
            .write_sdict(&mut written)
            .unwrap();
        assert_eq!(written, golden);
        let (back, enc) = read(golden).unwrap();
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(sorted(&back, "あ"), ["悪", "明", "朱", "秋", "紅", "赤"]);
        assert_eq!(sorted(&back, "あか"), ["朱", "紅", "赤"]);
    }
}
