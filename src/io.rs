//! File reading.

use std::path::Path;

use crate::error::{Error, Result};

/// Read a whole file.
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::io(path, e))
}
