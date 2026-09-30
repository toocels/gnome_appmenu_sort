use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Represents a parsed .desktop file
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub id: String, // desktop ID (e.g. "firefox.desktop" or "wine-Programs-MagiPacks-Road Rash.desktop")
    pub name: String, // Display name
    pub icon: String, // Icon name or path
    pub categories: Vec<String>,
    pub is_app: bool, // Meets criteria to be shown in GNOME app grid
}

/// Compute the Freedesktop / GNOME desktop file ID from file path and base directory
pub fn compute_desktop_id(file_path: &Path, base_dir: Option<&Path>) -> String {
    if let Some(base) = base_dir {
        if let Ok(rel) = file_path.strip_prefix(base) {
            let rel_str = rel.to_string_lossy();
            return rel_str.replace('/', "-");
        }
    }
    file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Find all .desktop files in standard locations with their base directory
pub fn find_desktop_files() -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut files = Vec::new();
    let search_paths = get_search_paths();

    for base_dir in search_paths {
        if base_dir.exists() {
            for entry in walkdir::WalkDir::new(&base_dir)
                .max_depth(8)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("desktop") {
                    files.push((p.to_path_buf(), base_dir.clone()));
                }
            }
        }
    }

    Ok(files)
}

fn get_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User applications: $XDG_DATA_HOME/applications (default ~/.local/share/applications)
    if let Some(data_dir) = dirs::data_dir() {
        paths.push(data_dir.join("applications"));
    }

    // $XDG_DATA_DIRS
    if let Ok(data_dirs_env) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs_env.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("applications"));
            }
        }
    }

    // Standard system applications
    paths.push(PathBuf::from("/usr/share/applications"));
    paths.push(PathBuf::from("/usr/local/share/applications"));

    // Flatpak exports
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));

    // Snap applications
    paths.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    // Deduplicate paths while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));

    paths
}

/// Parse a .desktop file directly
#[allow(dead_code)]
pub fn parse_desktop_file(path: &Path) -> Result<DesktopEntry> {
    parse_desktop_file_with_base(path, None)
}

/// Parse a .desktop file, computing desktop ID relative to base search directory if provided
pub fn parse_desktop_file_with_base(path: &Path, base_dir: Option<&Path>) -> Result<DesktopEntry> {
    let content = std::fs::read_to_string(path)?;
    let id = compute_desktop_id(path, base_dir);

    let mut name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let mut icon = String::new();
    let mut categories = Vec::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut is_application = false;
    let mut only_show_in: Option<Vec<String>> = None;
    let mut not_show_in: Option<Vec<String>> = None;
    let mut exec: Option<String> = None;
    let mut try_exec: Option<String> = None;

    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "Name" => {
                    // Only take the first unlocalized or default Name
                    if name == path.file_stem().and_then(|s| s.to_str()).unwrap_or("") {
                        name = value.to_string();
                    }
                }
                "Icon" => icon = value.to_string(),
                "Categories" => {
                    categories = value
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                }
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
                "Type" => is_application = value == "Application",
                "OnlyShowIn" => {
                    only_show_in = Some(
                        value
                            .split(';')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_uppercase())
                            .collect(),
                    );
                }
                "NotShowIn" => {
                    not_show_in = Some(
                        value
                            .split(';')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_uppercase())
                            .collect(),
                    );
                }
                "Exec" => exec = Some(value.to_string()),
                "TryExec" => try_exec = Some(value.to_string()),
                _ => {}
            }
        }
    }

    // Check desktop environment visibility for GNOME
    let mut matches_desktop = true;
    if let Some(ref only) = only_show_in {
        if !only.iter().any(|d| d == "GNOME") {
            matches_desktop = false;
        }
    }
    if let Some(ref not) = not_show_in {
        if not.iter().any(|d| d == "GNOME") {
            matches_desktop = false;
        }
    }

    // Check Exec command exists (if specified)
    let exec_ok = match exec {
        Some(ref cmd) => check_command_exists(cmd),
        None => true, // default true when not specified (e.g. mock test files)
    };

    // Check TryExec if present
    let try_exec_ok = match try_exec {
        Some(ref bin) => check_command_exists(bin),
        None => true,
    };

    let is_app =
        is_application && !no_display && !hidden && matches_desktop && exec_ok && try_exec_ok;

    Ok(DesktopEntry {
        id,
        name,
        icon,
        categories,
        is_app,
    })
}

fn check_command_exists(cmd: &str) -> bool {
    let clean = cmd.trim();
    if clean.is_empty() {
        return false;
    }
    let first = clean.split_whitespace().next().unwrap_or("");
    let bin = first.trim_matches('"').trim_matches('\'');
    if bin.is_empty() {
        return false;
    }
    let p = Path::new(bin);
    if p.is_absolute() {
        return p.exists();
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            if !dir.is_empty() && Path::new(dir).join(bin).exists() {
                return true;
            }
        }
    }
    false
}

/// Get all valid app desktop entries across both local user and global system locations
pub fn get_all_apps() -> Result<Vec<DesktopEntry>> {
    let files = find_desktop_files()?;
    let mut apps = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for (file, base_dir) in files {
        if let Ok(entry) = parse_desktop_file_with_base(&file, Some(&base_dir)) {
            if entry.is_app && seen_ids.insert(entry.id.clone()) {
                apps.push(entry);
            }
        }
    }

    Ok(apps)
}

/// Build a map from app id to desktop entry
pub fn get_app_map() -> Result<HashMap<String, DesktopEntry>> {
    let apps = get_all_apps()?;
    let mut map = HashMap::new();
    for app in apps {
        map.insert(app.id.clone(), app);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_desktop_file() {
        use std::io::Write;
        let tmp = tempfile_name("calc.desktop");
        let mut file = std::fs::File::create(&tmp).unwrap();
        writeln!(
            file,
            "[Desktop Entry]\nName=Calculator\nType=Application\nIcon=org.gnome.Calculator\nNoDisplay=false"
        )
        .unwrap();

        let entry = parse_desktop_file(&tmp).unwrap();
        assert_eq!(entry.id, "calc.desktop");
        assert_eq!(entry.name, "Calculator");
        assert_eq!(entry.icon, "org.gnome.Calculator");
        assert!(entry.is_app);

        let _ = std::fs::remove_file(tmp);
    }

    #[test]
    fn test_compute_desktop_id_nested() {
        let base = PathBuf::from("/home/user/.local/share/applications");
        let file = base.join("wine/Programs/MagiPacks/Game.desktop");
        let id = compute_desktop_id(&file, Some(&base));
        assert_eq!(id, "wine-Programs-MagiPacks-Game.desktop");
    }

    fn tempfile_name(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }
}
