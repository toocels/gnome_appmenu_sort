use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

/// Default assumption: GNOME Shell 40+ standard 4 rows x 6 columns grid. The real
/// capacity depends on the user's monitor/resolution/icon size and isn't readable
/// from dconf, so it's a tunable calibration value rather than a fixed constant.
pub const DEFAULT_ITEMS_PER_PAGE: usize = 24;
pub const MIN_ITEMS_PER_PAGE: usize = 4;
pub const MAX_ITEMS_PER_PAGE: usize = 200;

fn default_items_per_page() -> usize {
    DEFAULT_ITEMS_PER_PAGE
}

/// User-adjustable preferences, toggled from the TUI main menu and persisted to disk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Settings {
    /// Group apps that share the same icon into a folder during sorting.
    #[serde(default = "default_true")]
    pub group_icons: bool,
    /// Place icon-groups at the very beginning of the grid. When false, groups are
    /// interleaved with apps instead (sorted/shuffled alongside them, per sort mode).
    #[serde(default = "default_true")]
    pub groups_at_beginning: bool,
    /// How many items fit on one app-grid page. GNOME computes this at runtime from
    /// screen geometry, so it can't be read automatically — tune it to match what you
    /// actually see in the GNOME Overview to avoid leftover blank slots on each page.
    #[serde(default = "default_items_per_page")]
    pub items_per_page: usize,
    /// Don't place dash/taskbar-favorited apps in the grid. On some GNOME Shell
    /// versions a favorited app gets no tile in the app grid even though a slot can
    /// still be reserved for it there, leaving a visibly blank cell. Off if your
    /// GNOME version renders favorited apps in the grid fine.
    #[serde(default = "default_true")]
    pub skip_favorite_apps: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            group_icons: true,
            groups_at_beginning: true,
            items_per_page: DEFAULT_ITEMS_PER_PAGE,
            skip_favorite_apps: true,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("appmenu-gradient-sort").join("config.toml"))
}

impl Settings {
    /// Load settings from disk, falling back to defaults if missing or unreadable.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist settings to disk.
    pub fn save(&self) -> Result<()> {
        let path = config_path().context("Could not determine config directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        let toml_str =
            toml::to_string_pretty(self).context("Failed to serialize settings to TOML")?;
        std::fs::write(&path, toml_str).context("Failed to write config file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_roundtrip() {
        let settings = Settings {
            group_icons: false,
            groups_at_beginning: false,
            items_per_page: 30,
            skip_favorite_apps: false,
        };
        let toml_str = toml::to_string(&settings).unwrap();
        let loaded: Settings = toml::from_str(&toml_str).unwrap();
        assert!(!loaded.group_icons);
        assert!(!loaded.groups_at_beginning);
        assert_eq!(loaded.items_per_page, 30);
        assert!(!loaded.skip_favorite_apps);
    }

    #[test]
    fn test_settings_default_backwards_compatible() {
        let loaded: Settings = toml::from_str("").unwrap();
        assert!(loaded.group_icons);
        assert!(loaded.groups_at_beginning);
        assert_eq!(loaded.items_per_page, DEFAULT_ITEMS_PER_PAGE);
        assert!(loaded.skip_favorite_apps);
    }
}
