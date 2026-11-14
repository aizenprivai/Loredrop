//! Loading and saving `settings.toml` on disk.

use ld_config::Settings;
use std::path::Path;

/// Loads settings, reporting (not failing on) any problems, then writes the
/// file back so it contains every current setting with its description.
///
/// If the existing file is not valid TOML it is kept as `<name>.broken`
/// instead of being silently overwritten.
pub fn load_or_create(path: &Path) -> Settings {
    let settings = match std::fs::read_to_string(path) {
        Ok(text) => {
            let (s, warnings) = Settings::from_toml(&text);
            for w in &warnings {
                log::warn!("settings: {w}");
            }
            if warnings.iter().any(|w| w.contains("not valid TOML")) {
                let backup = path.with_extension("toml.broken");
                if std::fs::copy(path, &backup).is_ok() {
                    log::warn!("settings: kept the unreadable file as {}", backup.display());
                }
            }
            s
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("settings: {} not found, creating it with defaults", path.display());
            Settings::default()
        }
        Err(e) => {
            log::warn!("settings: could not read {}: {e}; using defaults", path.display());
            return Settings::default(); // don't overwrite a file we couldn't read
        }
    };
    save(path, &settings);
    settings
}

/// Writes settings atomically: write a temporary file, then rename it over the
/// real one, so a crash mid-write can never leave a half-written file.
pub fn save(path: &Path, settings: &Settings) {
    let tmp = path.with_extension("toml.tmp");
    let result = std::fs::write(&tmp, settings.to_toml()).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = result {
        log::warn!("settings: could not save {}: {e}", path.display());
    }
}
