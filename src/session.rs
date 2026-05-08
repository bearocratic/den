use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    let h = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(h)
}

pub fn den_dir() -> PathBuf {
    home_dir().join(".den")
}

pub fn legacy_dir() -> PathBuf {
    home_dir().join(".config/den")
}

pub fn sessions_dir() -> PathBuf {
    den_dir().join("sessions")
}

pub fn session_dir(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

pub fn ensure_dir(p: &Path) {
    if !p.exists() {
        let _ = std::fs::create_dir_all(p);
    }
}

fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while out.starts_with('-') {
        out.remove(0);
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn slug_for(bases: &[PathBuf]) -> String {
    let parts: Vec<String> = bases
        .iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .map(sanitize)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return short_hash(bases);
    }
    parts.join("-")
}

fn short_hash(bases: &[PathBuf]) -> String {
    let mut sorted: Vec<&PathBuf> = bases.iter().collect();
    sorted.sort();
    let mut h = DefaultHasher::new();
    for p in sorted {
        p.hash(&mut h);
    }
    format!("{:08x}", h.finish() as u32)
}

fn read_bases_file(file: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Look up the session id for the given bases, migrating any
/// existing session directory (e.g., legacy hex-named) to the slug
/// form. Returns the directory name to use under sessions_dir.
pub fn resolve_session(bases: &[PathBuf]) -> String {
    let dir = sessions_dir();
    ensure_dir(&dir);

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let existing = read_bases_file(&p.join("bases.txt"));
            if !existing.is_empty() && existing == *bases {
                let current_name = e.file_name().to_string_lossy().to_string();
                let target = slug_for(bases);
                if current_name == target {
                    return current_name;
                }
                let target_dir = dir.join(&target);
                if !target_dir.exists() {
                    let _ = std::fs::rename(&p, &target_dir);
                    return target;
                }
                let suffixed = format!("{}-{}", target, &short_hash(bases)[..4]);
                let suffixed_dir = dir.join(&suffixed);
                if !suffixed_dir.exists() {
                    let _ = std::fs::rename(&p, &suffixed_dir);
                    return suffixed;
                }
                return current_name;
            }
        }
    }

    let target = slug_for(bases);
    let target_dir = dir.join(&target);
    if !target_dir.exists() {
        return target;
    }
    format!("{}-{}", target, &short_hash(bases)[..4])
}

pub fn load_path_set(file: &Path) -> HashSet<PathBuf> {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn save_path_set(file: &Path, set: &HashSet<PathBuf>) {
    if let Some(parent) = file.parent() {
        ensure_dir(parent);
    }
    let mut entries: Vec<String> = set
        .iter()
        .filter_map(|p| p.to_str().map(String::from))
        .collect();
    entries.sort();
    let _ = std::fs::write(file, entries.join("\n"));
}

pub fn save_lines(file: &Path, lines: &[String]) {
    if let Some(parent) = file.parent() {
        ensure_dir(parent);
    }
    let _ = std::fs::write(file, lines.join("\n"));
}

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub sort_mode: String,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default)]
    pub last_filter: String,
}

pub fn settings_path(session_id: &str) -> PathBuf {
    session_dir(session_id).join("settings.toml")
}

pub fn load_settings(session_id: &str) -> Settings {
    let toml_path = settings_path(session_id);
    if let Ok(content) = std::fs::read_to_string(&toml_path) {
        if let Ok(s) = toml::from_str::<Settings>(&content) {
            return s;
        }
    }
    let kv_path = session_dir(session_id).join("settings.kv");
    if let Ok(content) = std::fs::read_to_string(&kv_path) {
        let s = parse_legacy_kv(&content);
        let _ = std::fs::write(&toml_path, toml::to_string_pretty(&s).unwrap_or_default());
        let _ = std::fs::remove_file(&kv_path);
        return s;
    }
    Settings::default()
}

pub fn save_settings(session_id: &str, s: &Settings) {
    let path = settings_path(session_id);
    if let Some(parent) = path.parent() {
        ensure_dir(parent);
    }
    if let Ok(body) = toml::to_string_pretty(s) {
        let _ = std::fs::write(&path, body);
    }
}

fn parse_legacy_kv(content: &str) -> Settings {
    let mut s = Settings::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "sort_ci_first" => {
                if v.trim() == "true" {
                    s.sort_mode = "ci_red_first".to_string();
                }
            }
            "sort_mode" => s.sort_mode = v.trim().to_string(),
            "show_hidden" => s.show_hidden = v.trim() == "true",
            "last_filter" => s.last_filter = v.trim().to_string(),
            _ => {}
        }
    }
    s
}

/// Migrate `~/.config/den/{pins,hidden}.txt` into the new layout.
/// Pins go into the current session's pins.txt. Hidden goes to ~/.den/hidden.txt.
/// The old directory is removed if migration succeeded.
pub fn migrate_legacy(current_session_id: &str) -> bool {
    let legacy = legacy_dir();
    if !legacy.exists() {
        return false;
    }
    let mut migrated = false;
    let den = den_dir();
    ensure_dir(&den);

    let legacy_hidden = legacy.join("hidden.txt");
    let new_hidden = den.join("hidden.txt");
    if legacy_hidden.exists() && !new_hidden.exists() {
        if let Ok(s) = std::fs::read_to_string(&legacy_hidden) {
            let _ = std::fs::write(&new_hidden, s);
            migrated = true;
        }
    }

    let legacy_pins = legacy.join("pins.txt");
    let session_pins = session_dir(current_session_id).join("pins.txt");
    if legacy_pins.exists() && !session_pins.exists() {
        if let Ok(s) = std::fs::read_to_string(&legacy_pins) {
            if let Some(parent) = session_pins.parent() {
                ensure_dir(parent);
            }
            let _ = std::fs::write(&session_pins, s);
            migrated = true;
        }
    }

    if migrated {
        let _ = std::fs::remove_dir_all(&legacy);
    }
    migrated
}

/// Rename any session directory whose name doesn't match the slug
/// derivable from its bases.txt. Returns the number of renames done.
pub fn migrate_session_dirs() -> usize {
    let dir = sessions_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    let mut renamed = 0usize;
    for e in entries {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let bases = read_bases_file(&p.join("bases.txt"));
        if bases.is_empty() {
            continue;
        }
        let current = e.file_name().to_string_lossy().to_string();
        let target = slug_for(&bases);
        if current == target {
            continue;
        }
        let target_dir = dir.join(&target);
        if !target_dir.exists() {
            if std::fs::rename(&p, &target_dir).is_ok() {
                renamed += 1;
            }
            continue;
        }
        let suffixed = format!("{}-{}", target, &short_hash(&bases)[..4]);
        let suffixed_dir = dir.join(&suffixed);
        if !suffixed_dir.exists() && std::fs::rename(&p, &suffixed_dir).is_ok() {
            renamed += 1;
        }
    }
    renamed
}

pub fn list_sessions() -> Vec<(String, Vec<PathBuf>, usize)> {
    let dir = sessions_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let id = e.file_name().to_string_lossy().to_string();
        let bases = read_bases_file(&e.path().join("bases.txt"));
        let pin_count = std::fs::read_to_string(e.path().join("pins.txt"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        out.push((id, bases, pin_count));
    }
    out
}
