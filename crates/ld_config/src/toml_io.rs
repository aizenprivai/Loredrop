//! Reading and writing `settings.toml`.
//!
//! Saving regenerates the whole file from the schema, so every setting always
//! has an up-to-date comment above it. (Comments players add themselves are
//! not kept; the header says so.)

use crate::Settings;
use crate::schema::{Apply, Kind, Value, schema};
use toml_edit::{DocumentMut, Item};

const HEADER: &str = "\
# ===========================================================================
#  Loredrop settings
# ===========================================================================
# Every option is explained by the comment above it. Edit this file while the
# game is closed. (The in-game settings menu, coming in a later phase, will
# write this same file; the Applies note describes changes made there.)
#
# * Invalid values are reset to their default (with a warning in the log).
# * Delete a line (or the whole file) to restore defaults.
# * This file is regenerated on save: your own comments are not kept.
";

/// Word-wraps `text` into `# `-prefixed comment lines of at most ~78 columns.
fn comment_block(text: &str) -> String {
    let mut out = String::new();
    let mut line = String::from("#");
    for word in text.split_whitespace() {
        if line.len() + 1 + word.len() > 78 {
            out.push_str(&line);
            out.push('\n');
            line = String::from("#");
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        // Round to 2 decimals so 0.1 + 0.2 noise never shows up in the file.
        Value::Float(f) => toml_edit::Value::from((f * 100.0).round() / 100.0).to_string(),
        Value::Text(t) => toml_edit::Value::from(t.as_str()).to_string(),
    }
}

pub(crate) fn save(settings: &Settings) -> String {
    let defaults = Settings::default();
    let mut out = String::from(HEADER);
    let mut section = "";
    for def in schema() {
        if def.section != section {
            section = def.section;
            out.push_str(&format!("\n[{section}]\n"));
        }
        out.push('\n');
        out.push_str(&comment_block(&format!("{} — {}", def.label, def.description)));
        let applies = match def.apply {
            Apply::Immediately => "immediately",
            Apply::OnRestart => "after restart",
        };
        let mut meta = format!(
            "Allowed: {} · Default: {} · Applies: {applies}",
            def.range_text(),
            format_value(&def.get(&defaults))
        );
        if def.performance != "none" {
            meta.push_str(&format!(" · Performance: {}", def.performance));
        }
        out.push_str(&comment_block(&meta));
        out.push_str(&format!("{} = {}\n", def.name, format_value(&def.get(settings))));
    }
    out
}

fn to_value(item: &Item, kind: Kind) -> Option<Value> {
    let v = item.as_value()?;
    Some(match kind {
        Kind::Bool => Value::Bool(v.as_bool()?),
        Kind::Int { .. } => Value::Int(v.as_integer()?),
        Kind::Float { .. } => match (v.as_float(), v.as_integer()) {
            (Some(f), _) => Value::Float(f),
            (_, Some(i)) => Value::Int(i),
            _ => return None,
        },
        Kind::Choice(_) | Kind::Key => Value::Text(v.as_str()?.to_string()),
    })
}

pub(crate) fn load(text: &str) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let mut warnings = Vec::new();
    let doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            warnings.push(format!("settings file is not valid TOML, using defaults: {e}"));
            return (settings, warnings);
        }
    };
    let defs = schema();
    for def in &defs {
        let Some(item) = doc.get(def.section).and_then(|t| t.get(def.name)) else {
            continue; // missing → keep default silently
        };
        let key = format!("{}.{}", def.section, def.name);
        match to_value(item, def.kind) {
            Some(v) => {
                if let Err(e) = def.set(&mut settings, v) {
                    warnings.push(format!("{key}: {e}; using default"));
                }
            }
            None => warnings.push(format!("{key}: wrong type ({}); using default", def.range_text())),
        }
    }
    // Report keys we don't recognise (typos, or settings from a newer version).
    for (section, table) in doc.iter() {
        if let Some(t) = table.as_table_like() {
            for (name, _) in t.iter() {
                if !defs.iter().any(|d| d.section == section && d.name == name) {
                    warnings.push(format!("{section}.{name}: unknown setting, ignored"));
                }
            }
        }
    }
    (settings, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ld_input::Action;

    #[test]
    fn defaults_round_trip_without_warnings() {
        let text = Settings::default().to_toml();
        let (s, w) = Settings::from_toml(&text);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s.to_toml(), text);
    }

    #[test]
    fn every_setting_has_a_description_comment() {
        let text = Settings::default().to_toml();
        for def in schema() {
            assert!(!def.description.trim().is_empty(), "{} has no description", def.name);
            // The line right above `name = …` must be a comment.
            let idx = text.find(&format!("\n{} = ", def.name)).expect("setting missing from file");
            let before = &text[..idx];
            assert!(before.lines().last().unwrap().starts_with('#'));
        }
    }

    #[test]
    fn every_setting_round_trips_changed_values() {
        let mut s = Settings::default();
        s.graphics.fov = 95.0;
        s.graphics.backend = crate::Backend::Vulkan;
        s.controls.invert_mouse = true;
        s.keybinds.bind(Action::Jump, "KeyJ");
        let (back, w) = Settings::from_toml(&s.to_toml());
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.graphics.fov, 95.0);
        assert_eq!(back.graphics.backend, crate::Backend::Vulkan);
        assert!(back.controls.invert_mouse);
        assert_eq!(back.keybinds.input_for(Action::Jump), Some("KeyJ"));
    }

    #[test]
    fn bad_values_fall_back_or_clamp() {
        let text = "[graphics]\nfov = 500\nvsync = \"yes\"\nbackend = \"directx9\"\ntypo = 1\n";
        let (s, w) = Settings::from_toml(text);
        assert_eq!(s.graphics.fov, 110.0, "clamped to max");
        assert!(s.graphics.vsync, "wrong type keeps default");
        assert_eq!(s.graphics.backend, crate::Backend::Auto);
        assert_eq!(w.len(), 3, "{w:?}"); // vsync, backend, typo (fov clamps silently)
    }

    #[test]
    fn garbage_file_gives_defaults() {
        let (s, w) = Settings::from_toml("this is [[[ not toml");
        assert_eq!(w.len(), 1);
        assert_eq!(s.graphics.fov, 70.0);
    }
}
