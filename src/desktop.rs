use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Represents a parsed .desktop file
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub id: String,   // filename WITH .desktop (e.g. "firefox.desktop")
    pub name: String, // Display name
    pub icon: String, // Icon name or path
    pub categories: Vec<String>,
    pub is_app: bool, // Meets criteria to be shown in GNOME app grid
}

/// Find all .desktop files in standard locations
pub fn find_desktop_files() -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let search_paths = get_search_paths();

    for path in search_paths {
        if path.exists() {
            for entry in walkdir::WalkDir::new(&path)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("desktop") {
                    files.push(p.to_path_buf());
                }
            }
        }
    }

    // Deduplicate by filename (first one encountered wins, e.g. user overrides system)
    let mut seen = std::collections::HashSet::new();
    files.retain(|f| {
        if let Some(name) = f.file_name() {
            seen.insert(name.to_string_lossy().to_string())
        } else {
            false
        }
    });

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

/// Parse a .desktop file
pub fn parse_desktop_file(path: &Path) -> Result<DesktopEntry> {
    let content = std::fs::read_to_string(path)?;
    // The desktop ID in GNOME Shell is the filename including .desktop
    let id = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

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

    // Check TryExec if present
    let try_exec_ok = match try_exec {
        Some(ref bin) => check_command_exists(bin),
        None => true,
    };

    let is_app = is_application && !no_display && !hidden && matches_desktop && try_exec_ok;

    Ok(DesktopEntry {
        id,
        name,
        icon,
        categories,
        is_app,
    })
}

fn check_command_exists(cmd: &str) -> bool {
    let p = Path::new(cmd);
    if p.is_absolute() {
        return p.exists();
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            if Path::new(dir).join(cmd).exists() {
                return true;
            }
        }
    }
    false
}

/// Get all valid app desktop entries
pub fn get_all_apps() -> Result<Vec<DesktopEntry>> {
    let files = find_desktop_files()?;
    let mut apps = Vec::new();

    for file in files {
        if let Ok(entry) = parse_desktop_file(&file) {
            if entry.is_app {
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

    fn tempfile_name(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }
}
