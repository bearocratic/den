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

pub fn session_id(bases: &[PathBuf]) -> String {
    let mut sorted: Vec<&PathBuf> = bases.iter().collect();
    sorted.sort();
    let mut h = DefaultHasher::new();
    for p in sorted {
        p.hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

pub fn session_dir(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

pub fn ensure_dir(p: &Path) {
    if !p.exists() {
        let _ = std::fs::create_dir_all(p);
    }
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

#[derive(Default)]
pub struct Settings {
    pub sort_ci_first: bool,
    pub show_hidden: bool,
    pub last_filter: String,
}

pub fn load_settings(file: &Path) -> Settings {
    let mut s = Settings::default();
    let content = std::fs::read_to_string(file).unwrap_or_default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "sort_ci_first" => s.sort_ci_first = v.trim() == "true",
            "show_hidden" => s.show_hidden = v.trim() == "true",
            "last_filter" => s.last_filter = v.trim().to_string(),
            _ => {}
        }
    }
    s
}

pub fn save_settings(file: &Path, s: &Settings) {
    if let Some(parent) = file.parent() {
        ensure_dir(parent);
    }
    let body = format!(
        "sort_ci_first={}\nshow_hidden={}\nlast_filter={}\n",
        s.sort_ci_first, s.show_hidden, s.last_filter
    );
    let _ = std::fs::write(file, body);
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
        let bases_file = e.path().join("bases.txt");
        let pins_file = e.path().join("pins.txt");
        let bases: Vec<PathBuf> = std::fs::read_to_string(&bases_file)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        let pin_count = std::fs::read_to_string(&pins_file)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        out.push((id, bases, pin_count));
    }
    out
}
