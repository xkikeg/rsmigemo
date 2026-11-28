//! Character encodings of dictionary files.

use std::borrow::Cow;

/// How many leading bytes [`Encoding::detect`] inspects.
const DETECT_LEN: usize = 4096;

/// The character encoding of a dictionary or conversion-table file.
///
/// Everything is decoded to UTF-8 on load, so this only matters for reading
/// files. Patterns are always returned as UTF-8 [`String`]s. Loaders take an
/// `Option<Encoding>`, where `None` means "detect it" (see [`detect`](Self::detect)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Encoding {
    /// UTF-8.
    Utf8,
    /// Shift_JIS with Microsoft extensions (Windows-31J).
    Cp932,
    /// EUC-JP. Six JIS X 0208 characters (`〜‖−¢£¬`) decode to their JIS
    /// forms, as with glibc `iconv` and in cmigemo's UTF-8 dictionary, rather
    /// than to the Microsoft forms (`～∥－￠￡￢`) the WHATWG decoder uses.
    EucJp,
}

impl Encoding {
    /// Guess the encoding of `bytes` from the first 4 KiB, with cmigemo's
    /// scoring for well-formed files, so both read them the same way.
    /// Returns `None` when no encoding wins (for example, for pure ASCII).
    ///
    /// Each candidate scores one point per well-formed byte pair (Shift_JIS,
    /// EUC-JP) or UTF-8 continuation byte. UTF-8 is validated by the
    /// standard library: the first invalid sequence rules it out and costs
    /// a point, while a character cut off at the 4 KiB limit does not.
    /// cmigemo's own validator is looser (see `CMIGEMO.md`), so files with
    /// overlong forms, surrogates or 5- and 6-byte sequences can be detected
    /// differently.
    pub fn detect(bytes: &[u8]) -> Option<Encoding> {
        let buf = &bytes[..bytes.len().min(DETECT_LEN)];
        let (mut sjis, mut smode) = (0i32, false);
        let (mut euc, mut emode) = (0i32, false);
        for &c in buf {
            // Shift_JIS: lead byte followed by a trail byte.
            if smode {
                if (0x40..=0x7e).contains(&c) || (0x80..=0xfc).contains(&c) {
                    sjis += 1;
                }
                smode = false;
            } else if (0x81..=0x9f).contains(&c) || (0xe0..=0xf0).contains(&c) {
                smode = true;
            }
            // EUC-JP: two bytes in 0xA1..=0xFE.
            if (0xa1..=0xfe).contains(&c) {
                if emode {
                    euc += 1;
                }
                emode = !emode;
            } else {
                emode = false;
            }
        }
        // UTF-8: continuation bytes before the first error.
        let (valid, ufailed) = match std::str::from_utf8(buf) {
            Ok(_) => (buf, false),
            // `error_len() == None`: a character cut off at the end (such as
            // by the 4 KiB limit) is not an error.
            Err(e) if e.error_len().is_none() => (buf, false),
            Err(e) => (&buf[..e.valid_up_to()], true),
        };
        let continuation = valid.iter().filter(|&&c| c & 0xc0 == 0x80).count();
        let utf8 = (continuation as i32 - i32::from(ufailed)).max(0);
        if euc > sjis && euc > utf8 {
            Some(Encoding::EucJp)
        } else if !ufailed && utf8 > euc && utf8 > sjis {
            Some(Encoding::Utf8)
        } else if sjis > euc && sjis > utf8 {
            Some(Encoding::Cp932)
        } else {
            None
        }
    }

    /// Decode `bytes` to UTF-8, replacing malformed sequences with U+FFFD.
    /// A leading UTF-8 byte order mark is removed.
    pub fn decode(self, bytes: &[u8]) -> Cow<'_, str> {
        match self {
            Encoding::Utf8 => {
                String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes))
            }
            Encoding::Cp932 => encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0,
            Encoding::EucJp => decode_euc_jp(bytes),
        }
    }

    /// Decode one sdict node code: a Unicode scalar for UTF-8, or
    /// `(lead << 8) | trail` (or a single byte) for the legacy encodings.
    pub(crate) fn decode_code(self, code: u32) -> Option<char> {
        let legacy = match self {
            Encoding::Cp932 => encoding_rs::SHIFT_JIS,
            Encoding::EucJp => encoding_rs::EUC_JP,
            Encoding::Utf8 => return char::from_u32(code),
        };
        let buf = [(code >> 8) as u8, code as u8];
        let bytes = match code {
            0..=0xff => &buf[1..],
            0x100..=0xffff => &buf[..],
            _ => return None,
        };
        if self == Encoding::EucJp
            && let Some(ch) = <[u8; 2]>::try_from(bytes).ok().and_then(euc_jp_jis_form)
        {
            return Some(ch);
        }
        let s = legacy.decode_without_bom_handling_and_without_replacement(bytes)?;
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }

    /// The encoding for the ID byte in an sdict header, or `None` if the ID
    /// is unknown.
    pub(crate) fn from_sdict_id(id: u8) -> Option<Encoding> {
        match id {
            1 => Some(Encoding::Cp932),
            2 => Some(Encoding::EucJp),
            // 0 ("none") is raw bytes, which we read as UTF-8.
            0 | 3 => Some(Encoding::Utf8),
            _ => None,
        }
    }
}

/// The encoding to read `bytes` in: `encoding` if given, otherwise the
/// detected one, falling back to UTF-8.
pub(crate) fn resolve(encoding: Option<Encoding>, bytes: &[u8]) -> Encoding {
    encoding
        .or_else(|| Encoding::detect(bytes))
        .unwrap_or(Encoding::Utf8)
}

/// EUC-JP codes that the WHATWG decoder (and so `encoding_rs`) maps to their
/// Microsoft (CP932) forms, with the JIS X 0208 characters that glibc `iconv`
/// and cmigemo's UTF-8 dictionary use instead.
const EUC_JP_JIS_FORMS: [([u8; 2], char); 6] = [
    ([0xA1, 0xC1], '\u{301C}'), // 〜 wave dash, not ～ U+FF5E
    ([0xA1, 0xC2], '\u{2016}'), // ‖ double vertical line, not ∥ U+2225
    ([0xA1, 0xDD], '\u{2212}'), // − minus sign, not － U+FF0D
    ([0xA1, 0xF1], '\u{00A2}'), // ¢ cent sign, not ￠ U+FFE0
    ([0xA1, 0xF2], '\u{00A3}'), // £ pound sign, not ￡ U+FFE1
    ([0xA2, 0xCC], '\u{00AC}'), // ¬ not sign, not ￢ U+FFE2
];

fn euc_jp_jis_form(code: [u8; 2]) -> Option<char> {
    EUC_JP_JIS_FORMS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|&(_, ch)| ch)
}

/// Decode EUC-JP with `encoding_rs`, except for [`EUC_JP_JIS_FORMS`].
///
/// The input is split at those two-byte codes, found by walking character
/// boundaries (so a trail byte or a JIS X 0212 code is never mistaken for
/// one), and the pieces between them are decoded normally.
fn decode_euc_jp(bytes: &[u8]) -> Cow<'_, str> {
    let decode = |b| encoding_rs::EUC_JP.decode_without_bom_handling(b).0;
    let mut out = String::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let len = match bytes[i] {
            0x8E | 0xA1..=0xFE => 2,
            0x8F => 3,
            _ => 1,
        };
        if let Some(&[lead, trail]) = bytes.get(i..i + 2)
            && let Some(ch) = euc_jp_jis_form([lead, trail])
        {
            out.push_str(&decode(&bytes[start..i]));
            out.push(ch);
            start = i + 2;
        }
        i += len;
    }
    if start == 0 {
        return decode(bytes);
    }
    out.push_str(&decode(&bytes[start..]));
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect() {
        assert_eq!(
            Encoding::detect("あか\t赤".as_bytes()),
            Some(Encoding::Utf8)
        );
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("あか\t赤\nあき\t秋");
        assert_eq!(Encoding::detect(&sjis), Some(Encoding::Cp932));
        let (euc, _, _) = encoding_rs::EUC_JP.encode("あか\t赤\nあき\t秋");
        assert_eq!(Encoding::detect(&euc), Some(Encoding::EucJp));
        assert_eq!(Encoding::detect(b"ascii only"), None);
    }

    #[test]
    fn detect_corner_cases() {
        assert_eq!(Encoding::detect(b""), None);
        // A BOM alone is two continuation bytes.
        assert_eq!(Encoding::detect(b"\xEF\xBB\xBF"), Some(Encoding::Utf8));
        assert_eq!(Encoding::detect(b"\xEF\xBB\xBFabc"), Some(Encoding::Utf8));
        // The 4 KiB cut falls after the lead byte of the 1366th あ: not an error.
        let cut = "あ".repeat(1366);
        assert_eq!(Encoding::detect(cut.as_bytes()), Some(Encoding::Utf8));
        // Only the first 4 KiB count.
        let late = [
            vec![b'a'; DETECT_LEN],
            encoding_rs::SHIFT_JIS.encode("あいう").0.to_vec(),
        ];
        assert_eq!(Encoding::detect(&late.concat()), None);
        // An invalid byte rules UTF-8 out, wherever it is.
        let bad_last = ["あいうえお".as_bytes(), b"\xFF"].concat();
        assert_eq!(Encoding::detect(&bad_last), None);
        let bad_first = [b"\xFF".as_slice(), "あいうえお".as_bytes()].concat();
        assert_eq!(Encoding::detect(&bad_first), Some(Encoding::Cp932));
        assert_eq!(Encoding::detect(b"\x80\x80\x80"), None);
    }

    /// Sequences cmigemo's validator accepts as UTF-8 but the standard
    /// library rejects. cmigemo detects the commented encoding instead.
    #[test]
    fn detect_malformed_utf8() {
        let cases: [(&[u8], Option<Encoding>); 5] = [
            // Overlong `/` (cmigemo: None).
            (b"\xC0\xAF\xC0\xAF\xC0\xAF", Some(Encoding::EucJp)),
            // Surrogates U+D800, U+DC00 (cmigemo: UTF-8).
            (b"\xED\xA0\x80\xED\xB0\x80", Some(Encoding::Cp932)),
            // Obsolete 5- and 6-byte forms (cmigemo: UTF-8).
            (b"\xF8\x88\x80\x80\x80", Some(Encoding::Cp932)),
            (b"\xFC\x84\x80\x80\x80\x80", Some(Encoding::Cp932)),
            // Above U+10FFFF (cmigemo: UTF-8).
            (b"\xF4\x90\x80\x80\xF4\x90\x80\x80", Some(Encoding::Cp932)),
        ];
        for (bytes, expected) in cases {
            assert_eq!(Encoding::detect(bytes), expected, "{bytes:02X?}");
        }
    }

    #[test]
    fn euc_jp_jis_forms() {
        // あ〜い¬, and JIS X 0212 0x2237 (8F A2 B7), which stays as WHATWG has it.
        let bytes = b"\xA4\xA2\xA1\xC1\xA4\xA4\xA2\xCC\x8F\xA2\xB7";
        let whatwg = encoding_rs::EUC_JP
            .decode_without_bom_handling(b"\x8F\xA2\xB7")
            .0;
        assert_eq!(Encoding::EucJp.decode(bytes), format!("あ〜い¬{whatwg}"));
        let all: Vec<u8> = EUC_JP_JIS_FORMS.iter().flat_map(|(c, _)| *c).collect();
        assert_eq!(Encoding::EucJp.decode(&all), "〜‖−¢£¬");
        // CP932 keeps the Microsoft forms, as iconv's CP932 does.
        assert_eq!(Encoding::Cp932.decode(b"\x81\x60"), "～");
        assert_eq!(Encoding::EucJp.decode(b"plain \xA4\xA2"), "plain あ");
    }

    #[test]
    fn resolve() {
        let sjis = encoding_rs::SHIFT_JIS.encode("あか\t赤").0;
        assert_eq!(super::resolve(None, &sjis), Encoding::Cp932);
        assert_eq!(super::resolve(Some(Encoding::Utf8), &sjis), Encoding::Utf8);
        assert_eq!(super::resolve(None, b"ascii"), Encoding::Utf8);
    }

    #[test]
    fn decode_code() {
        assert_eq!(Encoding::Utf8.decode_code(0x3042), Some('あ'));
        assert_eq!(Encoding::Cp932.decode_code(0x82a0), Some('あ'));
        assert_eq!(Encoding::EucJp.decode_code(0xa4a2), Some('あ'));
        assert_eq!(Encoding::EucJp.decode_code(0xa1c1), Some('〜'));
        assert_eq!(Encoding::Cp932.decode_code(0x8160), Some('～'));
        assert_eq!(Encoding::Cp932.decode_code(b'a' as u32), Some('a'));
        assert_eq!(Encoding::Cp932.decode_code(0xb1), Some('ｱ'));
    }

    #[test]
    fn decode_strips_bom() {
        assert_eq!(Encoding::Utf8.decode(b"\xEF\xBB\xBFabc"), "abc");
    }
}
