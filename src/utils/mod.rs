pub mod backup;
pub mod logging;

pub use logging::{data_dir, log};

/// Writes a temporary file and renames it over the old one, so a crash or power cut mid-write never leaves a
/// half-written settings file.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, path))
}

/// String value for `key` in a flat JSON object (Epic manifests, NVIDIA's driver lookup): the first match.
pub fn json_str(text: &str, key: &str) -> Option<String> {
    let start = text.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = text[start..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}
