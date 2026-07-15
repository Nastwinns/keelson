use std::path::Path;

use super::{Manifest, ManifestError, ManifestLoader};

/// The reference [`ManifestLoader`]: reads and validates a `haw.toml`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TomlLoader;

impl ManifestLoader for TomlLoader {
    fn load(&self, path: &Path) -> Result<Manifest, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        text.parse()
    }
}
