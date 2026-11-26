//! File reading with transparent zstd decompression.

use std::path::Path;

use crate::error::{Error, Result};

/// Read a whole file. Files whose name ends in `.zst` (in any case) are
/// decompressed.
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    let is_zstd = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"));
    if !is_zstd {
        return std::fs::read(path).map_err(|e| Error::io(path, e));
    }
    #[cfg(feature = "zstd")]
    {
        let file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
        zstd::decode_all(file).map_err(|e| Error::io(path, e))
    }
    #[cfg(not(feature = "zstd"))]
    Err(Error::UnsupportedCompression {
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in the system temp directory, removed when dropped.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(name: &str, contents: &[u8]) -> Self {
            let path =
                std::env::temp_dir().join(format!("rsmigemo-io-{}-{name}", std::process::id()));
            std::fs::write(&path, contents).unwrap();
            TempFile(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn plain_file() {
        let f = TempFile::new("plain.txt", b"hello");
        assert_eq!(read_file(&f.0).unwrap(), b"hello");
    }

    #[test]
    fn missing_file() {
        let err = read_file(Path::new("/nonexistent/rsmigemo-dict")).unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err}");
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn zstd_file() {
        let compressed = zstd::encode_all(&b"hello"[..], 0).unwrap();
        for name in ["dict.zst", "DICT.ZST"] {
            let f = TempFile::new(name, &compressed);
            assert_eq!(read_file(&f.0).unwrap(), b"hello", "{name}");
        }
    }

    #[cfg(not(feature = "zstd"))]
    #[test]
    fn zstd_needs_the_feature() {
        let f = TempFile::new("nofeature.zst", b"not really zstd");
        let err = read_file(&f.0).unwrap_err();
        assert!(matches!(err, Error::UnsupportedCompression { .. }), "{err}");
    }
}
