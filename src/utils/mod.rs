pub mod backup;
pub mod logging;

pub use logging::{data_dir, log};

/// Writes a temporary file and renames it over the old one, so a crash or power cut mid-write never leaves a
/// half-written settings file.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, path))
}
