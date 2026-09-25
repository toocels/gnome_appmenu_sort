use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BACKUP_EXTENSION: &str = ".appmenu-backup.toml";

/// Represents a backup of the app picker layout and folder configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct LayoutBackup {
    pub timestamp: String,
    #[serde(default)]
    pub folder_children: Vec<String>,
    pub pages: Vec<Vec<(String, u32)>>,
}

/// Create a backup of the current layout and folder configuration
pub fn create_backup(
    pages: &[Vec<(String, u32)>],
    folder_children: &[String],
    output_dir: &Path,
) -> Result<PathBuf> {
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let filename = format!("appmenu-{}{}", timestamp, BACKUP_EXTENSION);
    let filepath = output_dir.join(filename);

    let backup = LayoutBackup {
        timestamp,
        folder_children: folder_children.to_vec(),
        pages: pages.to_vec(),
    };

    let toml_str = toml::to_string_pretty(&backup).context("Failed to serialize backup to TOML")?;

    std::fs::write(&filepath, toml_str).context("Failed to write backup file")?;

    Ok(filepath)
}

/// Find all backup files in a directory, sorted newest first
pub fn find_backups(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut backups = Vec::new();

    if !dir.exists() {
        return Ok(backups);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(BACKUP_EXTENSION) {
                backups.push(path);
            }
        }
    }

    // Sort newest first by filename (which starts with timestamp)
    backups.sort_by(|a, b| b.cmp(a));

    Ok(backups)
}

/// Load a backup from a file
pub fn load_backup(path: &Path) -> Result<LayoutBackup> {
    let content = std::fs::read_to_string(path).context("Failed to read backup file")?;

    let backup: LayoutBackup = toml::from_str(&content).context("Failed to parse backup TOML")?;

    Ok(backup)
}

/// Get the backup file extension
pub fn get_extension() -> &'static str {
    BACKUP_EXTENSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_serialization() {
        let backup = LayoutBackup {
            timestamp: "2026-09-30_12-00-00".to_string(),
            folder_children: vec!["System".to_string()],
            pages: vec![vec![("firefox.desktop".to_string(), 0)]],
        };

        let toml_str = toml::to_string(&backup).unwrap();
        let loaded: LayoutBackup = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.timestamp, backup.timestamp);
        assert_eq!(loaded.folder_children, backup.folder_children);
        assert_eq!(loaded.pages, backup.pages);
    }

    #[test]
    fn test_backwards_compatible_deserialization() {
        let old_toml = r#"
timestamp = "2026-09-30_12-00-00"
pages = [
    [["firefox.desktop", 0]]
]
"#;
        let loaded: LayoutBackup = toml::from_str(old_toml).unwrap();
        assert_eq!(loaded.timestamp, "2026-09-30_12-00-00");
        assert!(loaded.folder_children.is_empty());
        assert_eq!(loaded.pages.len(), 1);
    }
}
