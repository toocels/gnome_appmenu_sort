use anyhow::{Context, Result};
use std::process::Command;

/// Represents a GNOME Shell app folder
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AppFolder {
    pub id: String,
    pub name: String,
    pub apps: Vec<String>,
}

/// Read the current app-picker-layout from dconf
pub fn read_layout() -> Result<String> {
    let output = Command::new("dconf")
        .args(["read", "/org/gnome/shell/app-picker-layout"])
        .output()
        .context("Failed to run dconf read")?;

    if !output.status.success() {
        anyhow::bail!(
            "dconf read failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Write a new app-picker-layout to dconf
pub fn write_layout(layout_str: &str) -> Result<()> {
    let output = Command::new("dconf")
        .args(["write", "/org/gnome/shell/app-picker-layout", layout_str])
        .output()
        .context("Failed to run dconf write")?;

    if !output.status.success() {
        anyhow::bail!(
            "dconf write failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Read folder-children IDs from org.gnome.desktop.app-folders
pub fn read_folder_children() -> Result<Vec<String>> {
    let output = Command::new("dconf")
        .args(["read", "/org/gnome/desktop/app-folders/folder-children"])
        .output()
        .context("Failed to read folder-children")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_string_list(stdout.trim()))
}

/// Write folder-children IDs to org.gnome.desktop.app-folders
pub fn write_folder_children(folders: &[String]) -> Result<()> {
    let val = if folders.is_empty() {
        "@as []".to_string()
    } else {
        let quoted: Vec<String> = folders.iter().map(|f| format!("'{}'", f)).collect();
        format!("[{}]", quoted.join(", "))
    };

    let output = Command::new("dconf")
        .args([
            "write",
            "/org/gnome/desktop/app-folders/folder-children",
            &val,
        ])
        .output()
        .context("Failed to write folder-children")?;

    if !output.status.success() {
        anyhow::bail!(
            "dconf write folder-children failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

/// Create or update a GNOME app folder in dconf
pub fn create_or_update_app_folder(id: &str, name: &str, apps: &[String]) -> Result<()> {
    let name_val = format!("'{}'", name.replace('\'', "\\'"));
    let apps_quoted: Vec<String> = apps.iter().map(|a| format!("'{}'", a)).collect();
    let apps_val = format!("[{}]", apps_quoted.join(", "));

    Command::new("dconf")
        .args([
            "write",
            &format!("/org/gnome/desktop/app-folders/folders/{}/name", id),
            &name_val,
        ])
        .output()
        .context("Failed to write folder name")?;

    Command::new("dconf")
        .args([
            "write",
            &format!("/org/gnome/desktop/app-folders/folders/{}/apps", id),
            &apps_val,
        ])
        .output()
        .context("Failed to write folder apps")?;

    Command::new("dconf")
        .args([
            "write",
            &format!("/org/gnome/desktop/app-folders/folders/{}/translate", id),
            "false",
        ])
        .output()
        .context("Failed to write folder translate")?;

    Ok(())
}

/// Clear all folder groupings in GNOME (un-groups all apps)
#[allow(dead_code)]
pub fn clear_folder_children() -> Result<()> {
    write_folder_children(&[])
}

/// Read configured app folders from org.gnome.desktop.app-folders
#[allow(dead_code)]
pub fn read_app_folders() -> Result<Vec<AppFolder>> {
    let folder_ids = read_folder_children()?;
    let mut folders = Vec::new();

    for fid in folder_ids {
        // Read folder name
        let name_out = Command::new("dconf")
            .args([
                "read",
                &format!("/org/gnome/desktop/app-folders/folders/{}/name", fid),
            ])
            .output();

        let mut display_name = fid.clone();
        if let Ok(out) = name_out {
            if out.status.success() {
                let raw_name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let clean_name = raw_name.trim_matches('\'').trim_matches('"');
                if !clean_name.is_empty() {
                    display_name = clean_name
                        .trim_start_matches("X-GNOME-Shell-")
                        .trim_end_matches(".directory")
                        .to_string();
                }
            }
        }

        // Read apps inside this folder
        let apps_out = Command::new("dconf")
            .args([
                "read",
                &format!("/org/gnome/desktop/app-folders/folders/{}/apps", fid),
            ])
            .output();

        let mut apps = Vec::new();
        if let Ok(out) = apps_out {
            if out.status.success() {
                let raw_apps = String::from_utf8_lossy(&out.stdout).trim().to_string();
                apps = parse_string_list(&raw_apps);
            }
        }

        folders.push(AppFolder {
            id: fid,
            name: display_name,
            apps,
        });
    }

    Ok(folders)
}

/// Helper to parse GVariant/Python-like string list: ['a', 'b', 'c']
pub fn parse_string_list(raw: &str) -> Vec<String> {
    let mut result = Vec::new();
    let trimmed = raw.trim();
    let inner = trimmed
        .trim_start_matches(|c| c == '@' || c == 'a' || c == 's' || c == ' ' || c == '[')
        .trim_end_matches(']')
        .trim();

    if inner.is_empty() {
        return result;
    }

    let mut in_quote = false;
    let mut quote_char = '\'';
    let mut current = String::new();

    for c in inner.chars() {
        if !in_quote && (c == '\'' || c == '"') {
            in_quote = true;
            quote_char = c;
            current.clear();
        } else if in_quote && c == quote_char {
            in_quote = false;
            result.push(current.clone());
            current.clear();
        } else if in_quote {
            current.push(c);
        }
    }

    result
}

/// Parse the dconf layout string into pages of (app_id, position) pairs
pub fn parse_layout(layout_str: &str) -> Result<Vec<Vec<(String, u32)>>> {
    let mut trimmed = layout_str.trim();

    // Strip type prefix if present, e.g. "@aa{sv} [...]" or "@a{sv} [...]"
    if trimmed.starts_with('@') {
        if let Some((_, rest)) = trimmed.split_once(' ') {
            trimmed = rest.trim();
        }
    }

    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(vec![]);
    }

    // Remove outer brackets
    let inner = trimmed.trim_start_matches('[').trim_end_matches(']').trim();

    if inner.is_empty() {
        return Ok(vec![]);
    }

    // Split pages by matching braces at depth 0
    let mut page_strings = Vec::new();
    let mut depth = 0usize;
    let mut start_idx = None;

    for (i, c) in inner.char_indices() {
        match c {
            '{' => {
                if depth == 0 {
                    start_idx = Some(i + 1);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(start) = start_idx {
                        page_strings.push(&inner[start..i]);
                        start_idx = None;
                    }
                }
            }
            _ => {}
        }
    }

    let mut result = Vec::new();

    for page_str in page_strings {
        let mut apps = Vec::new();
        let mut cursor = 0;

        while cursor < page_str.len() {
            // Find key starting quote
            if let Some(q1_rel) = page_str[cursor..].find('\'') {
                let q1 = cursor + q1_rel + 1;
                if let Some(q2_rel) = page_str[q1..].find('\'') {
                    let q2 = q1 + q2_rel;
                    let id = page_str[q1..q2].to_string();

                    // Search for position within this entry
                    let rest = &page_str[q2 + 1..];
                    if let Some(pos_rel) = rest.find("position") {
                        let after_pos = &rest[pos_rel..];
                        if let Some(b1_rel) = after_pos.find('<') {
                            let after_b1 = &after_pos[b1_rel + 1..];
                            if let Some(b2_rel) = after_b1.find('>') {
                                let num_str = after_b1[..b2_rel].trim();
                                let digits: String = num_str
                                    .split_whitespace()
                                    .last()
                                    .unwrap_or(num_str)
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .collect();

                                if let Ok(pos) = digits.parse::<u32>() {
                                    apps.push((id, pos));
                                }
                            }
                        }

                        // Advance cursor past this entry's closing "}>"
                        if let Some(close_rel) = rest.find("}>") {
                            cursor = q2 + 1 + close_rel + 2;
                        } else {
                            cursor = q2 + 1;
                        }
                    } else {
                        cursor = q2 + 1;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        apps.sort_by_key(|(_, pos)| *pos);
        result.push(apps);
    }

    Ok(result)
}

/// Serialize pages back to dconf GVariant format
pub fn serialize_layout(pages: &[Vec<(String, u32)>]) -> String {
    if pages.is_empty() {
        return "@aa{sv} []".to_string();
    }

    let page_strs: Vec<String> = pages
        .iter()
        .map(|page| {
            let entries: Vec<String> = page
                .iter()
                .map(|(id, pos)| format!("'{}': <{{'position': <{}>}}>", id, pos))
                .collect();
            format!("{{{}}}", entries.join(", "))
        })
        .collect();

    format!("[{}]", page_strs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_serialize_layout() {
        let input = "[{'org.gnome.Geary.desktop': <{'position': <0>}>, 'org.gnome.Contacts.desktop': <{'position': <1>}>}]";
        let parsed = parse_layout(input).expect("Failed to parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].len(), 2);
        assert_eq!(parsed[0][0], ("org.gnome.Geary.desktop".to_string(), 0));
        assert_eq!(parsed[0][1], ("org.gnome.Contacts.desktop".to_string(), 1));

        let serialized = serialize_layout(&parsed);
        assert_eq!(serialized, input);
    }

    #[test]
    fn test_empty_layout() {
        assert_eq!(parse_layout("").unwrap(), Vec::<Vec<(String, u32)>>::new());
        assert_eq!(
            parse_layout("[]").unwrap(),
            Vec::<Vec<(String, u32)>>::new()
        );
        assert_eq!(
            parse_layout("@aa{sv} []").unwrap(),
            Vec::<Vec<(String, u32)>>::new()
        );
        assert_eq!(serialize_layout(&[]), "@aa{sv} []");
    }

    #[test]
    fn test_typed_positions() {
        let input =
            "[{'a.desktop': <{'position': <uint32 0>}>}, {'b.desktop': <{'position': <int32 1>}>}]";
        let parsed = parse_layout(input).expect("Failed to parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0][0], ("a.desktop".to_string(), 0));
        assert_eq!(parsed[1][0], ("b.desktop".to_string(), 1));
    }
}
