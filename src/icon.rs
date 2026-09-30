use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Represents an analyzed icon with its average color
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct IconInfo {
    pub name: String,
    pub path: Option<PathBuf>,
    pub avg_hue: f32,        // 0.0 - 360.0
    pub avg_saturation: f32, // 0.0 - 1.0
    pub avg_lightness: f32,  // 0.0 - 1.0
    pub is_monochrome: bool, // true if low saturation or dark/black or near white
}

/// Resolve an icon name to a file path on disk, falling back to standard GNOME fallback icons if not found
pub fn resolve_icon(icon_name: &str) -> Option<PathBuf> {
    let clean_icon = icon_name.trim();
    if clean_icon.is_empty() {
        return None;
    }

    if let Some(path) = resolve_icon_exact(clean_icon) {
        return Some(path);
    }

    // Standard GNOME / Freedesktop fallbacks for apps whose specific icon cannot be found
    let fallbacks = [
        "application-x-executable",
        "applications-system",
        "system-run",
        "preferences-system",
        "application-default-icon",
    ];

    for fb in &fallbacks {
        if let Some(path) = resolve_icon_exact(fb) {
            return Some(path);
        }
    }

    None
}

/// Helper to resolve a specific icon name without applying generic fallbacks
fn resolve_icon_exact(icon_name: &str) -> Option<PathBuf> {
    let direct_path = Path::new(icon_name);
    if direct_path.is_absolute() && direct_path.exists() {
        return Some(direct_path.to_path_buf());
    }

    // Strip trailing image extension if present
    let stem = if let Some(stripped) = icon_name
        .strip_suffix(".png")
        .or_else(|| icon_name.strip_suffix(".svg"))
        .or_else(|| icon_name.strip_suffix(".xpm"))
    {
        stripped
    } else {
        icon_name
    };

    let extensions = ["png", "svg", "xpm"];
    let base_dirs = get_icon_search_paths();

    // 1. Try active theme first (prioritize standard app icons over actions/symbolic)
    if let Some(active_theme) = get_active_icon_theme() {
        for base in &base_dirs {
            let theme_dir = base.join(&active_theme);
            if theme_dir.is_dir() {
                if let Some(p) = search_theme_dir(&theme_dir, stem, &extensions) {
                    return Some(p);
                }
            }
        }
    }

    // 2. Try hicolor theme (standard freedesktop fallback)
    for base in &base_dirs {
        let hicolor_dir = base.join("hicolor");
        if hicolor_dir.is_dir() {
            if let Some(p) = search_theme_dir(&hicolor_dir, stem, &extensions) {
                return Some(p);
            }
        }
    }

    // 3. Try any other themes
    for base in &base_dirs {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let theme_dir = entry.path();
                if theme_dir.is_dir() {
                    let name = theme_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name != "hicolor" {
                        if let Some(p) = search_theme_dir(&theme_dir, stem, &extensions) {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }

    // 4. Try pixmaps
    let pixmaps = PathBuf::from("/usr/share/pixmaps");
    for ext in &extensions {
        let candidate = pixmaps.join(format!("{}.{}", stem, ext));
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

/// Query active icon theme from gsettings
fn get_active_icon_theme() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let cleaned = s.trim_matches('\'').trim_matches('"').trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

/// Search within a specific icon theme directory
fn search_theme_dir(theme_dir: &Path, icon_name: &str, extensions: &[&str]) -> Option<PathBuf> {
    let sizes = [
        "scalable",
        "apps@2x/scalable",
        "apps/scalable",
        "512x512",
        "256x256",
        "128x128",
        "64x64",
        "48x48",
        "32x32",
        "24x24",
        "16x16",
    ];
    // Prioritize "apps" categories before checking secondary categories
    let categories = [
        "apps",
        "categories",
        "devices",
        "places",
        "status",
        "actions",
        "mimes",
        "emblems",
    ];

    // 1. Check size/category (e.g. hicolor/scalable/apps or hicolor/48x48/apps)
    for size in &sizes {
        for cat in &categories {
            for ext in extensions {
                let candidate = theme_dir
                    .join(size)
                    .join(cat)
                    .join(format!("{}.{}", icon_name, ext));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. Check category/size (e.g. Reversal/apps/scalable, Reversal/apps@2x/scalable)
    for cat in &categories {
        for size in &sizes {
            for ext in extensions {
                let candidate = theme_dir
                    .join(cat)
                    .join(size)
                    .join(format!("{}.{}", icon_name, ext));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        // Direct category (e.g. theme/apps/icon.svg)
        for ext in extensions {
            let candidate = theme_dir.join(cat).join(format!("{}.{}", icon_name, ext));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn get_icon_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".icons"));
    }

    if let Some(data_dir) = dirs::data_dir() {
        paths.push(data_dir.join("icons"));
    }

    if let Ok(data_dirs_env) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs_env.split(':') {
            if !dir.is_empty() {
                paths.push(PathBuf::from(dir).join("icons"));
            }
        }
    }

    paths.push(PathBuf::from("/usr/share/icons"));
    paths.push(PathBuf::from("/usr/local/share/icons"));

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    paths.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));
    paths.push(PathBuf::from("/var/lib/snapd/desktop/icons"));

    // Deduplicate
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));

    paths
}

/// Analyze an icon image to find its average color
pub fn analyze_icon(path: &Path) -> Result<IconInfo> {
    let is_svg = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false);

    if is_svg {
        // Attempt rasterization via rsvg-convert first
        if let Ok(img) = rasterize_svg(path) {
            return analyze_dynamic_image(&img, path);
        }

        // Fallback: parse colors directly from SVG XML
        if let Ok(info) = parse_svg_colors(path) {
            return Ok(info);
        }
    }

    // Standard raster image loading (PNG, JPEG, XPM, etc.)
    let img = image::open(path)?;
    analyze_dynamic_image(&img, path)
}

/// Rasterize SVG to PNG bytes using rsvg-convert
fn rasterize_svg(path: &Path) -> Result<image::DynamicImage> {
    let output = Command::new("rsvg-convert")
        .args(["-w", "64", "-h", "64", path.to_str().unwrap_or("")])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("rsvg-convert failed");
    }

    let img = image::load_from_memory(&output.stdout)?;
    Ok(img)
}

/// Fallback: parse hex color attributes from SVG text with circular hue averaging
fn parse_svg_colors(path: &Path) -> Result<IconInfo> {
    let content = std::fs::read_to_string(path)?;
    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    let mut sum_sat = 0.0f32;
    let mut sum_light = 0.0f32;
    let mut count = 0u32;

    for word in content
        .split(|c: char| c == '"' || c == '\'' || c == ';' || c == ' ' || c == '<' || c == '>')
    {
        let hex = word.trim().trim_start_matches('#');
        if (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                let (h, s, l) = rgb_to_hsl(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
                let rad = h * 2.0 * std::f32::consts::PI;
                let weight = s;
                sum_x += weight * rad.cos();
                sum_y += weight * rad.sin();
                sum_sat += s;
                sum_light += l;
                count += 1;
            }
        }
    }

    if count == 0 {
        return Ok(IconInfo {
            name: path.to_string_lossy().to_string(),
            path: Some(path.to_path_buf()),
            avg_hue: 0.0,
            avg_saturation: 0.0,
            avg_lightness: 0.5,
            is_monochrome: true,
        });
    }

    let avg_saturation = sum_sat / count as f32;
    let avg_lightness = sum_light / count as f32;

    let mut avg_rad = sum_y.atan2(sum_x);
    if avg_rad < 0.0 {
        avg_rad += 2.0 * std::f32::consts::PI;
    }
    let avg_hue = (avg_rad * 180.0) / std::f32::consts::PI;

    let chroma = avg_saturation * (1.0 - (2.0 * avg_lightness - 1.0).abs());
    let is_monochrome = chroma < 0.16
        || (avg_lightness <= 0.28 && chroma <= 0.25)
        || (avg_lightness >= 0.82 && chroma <= 0.22);

    Ok(IconInfo {
        name: path.to_string_lossy().to_string(),
        path: Some(path.to_path_buf()),
        avg_hue,
        avg_saturation,
        avg_lightness,
        is_monochrome,
    })
}

/// Analyze pixels from a loaded DynamicImage using circular statistics for hue.
/// Completely ignores transparent pixels (alpha < 32) so transparent background
/// does not dilute or skew the average color calculation.
fn analyze_dynamic_image(img: &image::DynamicImage, path: &Path) -> Result<IconInfo> {
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    let step = ((width * height) / 10000).max(1);

    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    let mut sum_sat = 0.0f32;
    let mut sum_light = 0.0f32;
    let mut total_alpha_weight = 0.0f32;
    let mut count = 0u32;

    for (x, y, pixel) in rgba_img.enumerate_pixels() {
        if (x + y * width) % step != 0 {
            continue;
        }

        let [r, g, b, a] = pixel.0;
        // Do not include transparent background pixels in average color calculation
        if a < 32 {
            continue;
        }

        let alpha_factor = a as f32 / 255.0;
        let (h, s, l) = rgb_to_hsl(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);

        let rad = h * 2.0 * std::f32::consts::PI;
        let weight = s * alpha_factor; // Weight circular vector by saturation and opacity
        sum_x += weight * rad.cos();
        sum_y += weight * rad.sin();
        sum_sat += s * alpha_factor;
        sum_light += l * alpha_factor;
        total_alpha_weight += alpha_factor;
        count += 1;
    }

    if count == 0 || total_alpha_weight < 0.001 {
        return Ok(IconInfo {
            name: path.to_string_lossy().to_string(),
            path: Some(path.to_path_buf()),
            avg_hue: 0.0,
            avg_saturation: 0.0,
            avg_lightness: 0.5,
            is_monochrome: true,
        });
    }

    let avg_saturation = sum_sat / total_alpha_weight;
    let avg_lightness = sum_light / total_alpha_weight;

    let mut avg_rad = sum_y.atan2(sum_x);
    if avg_rad < 0.0 {
        avg_rad += 2.0 * std::f32::consts::PI;
    }
    let avg_hue = (avg_rad * 180.0) / std::f32::consts::PI;

    // Effective colorfulness (chroma)
    let chroma = avg_saturation * (1.0 - (2.0 * avg_lightness - 1.0).abs());

    // Group mostly black / dark icons, whites, and low-chroma neutrals
    let is_monochrome = chroma < 0.16
        || (avg_lightness <= 0.28 && chroma <= 0.25)
        || (avg_lightness >= 0.82 && chroma <= 0.22);

    Ok(IconInfo {
        name: path.to_string_lossy().to_string(),
        path: Some(path.to_path_buf()),
        avg_hue,
        avg_saturation,
        avg_lightness,
        is_monochrome,
    })
}

/// Convert RGB to HSL
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if max == min {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if max == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };

    (h, s, l)
}

/// Get icon info for an icon name, resolving and analyzing it
pub fn get_icon_info(icon_name: &str) -> IconInfo {
    if let Some(path) = resolve_icon(icon_name) {
        if let Ok(info) = analyze_icon(&path) {
            return IconInfo {
                name: icon_name.to_string(),
                path: Some(path),
                ..info
            };
        }
        return IconInfo {
            name: icon_name.to_string(),
            path: Some(path),
            avg_hue: 0.0,
            avg_saturation: 0.0,
            avg_lightness: 0.5,
            is_monochrome: true,
        };
    }

    IconInfo {
        name: icon_name.to_string(),
        path: None,
        avg_hue: 0.0,
        avg_saturation: 0.0,
        avg_lightness: 0.5,
        is_monochrome: true,
    }
}

/// Analyze an icon identified by a key (resolved path or icon name)
pub fn get_icon_info_for_key(key: &str) -> IconInfo {
    let path = Path::new(key);
    if path.exists() {
        if let Ok(info) = analyze_icon(path) {
            return IconInfo {
                name: key.to_string(),
                path: Some(path.to_path_buf()),
                ..info
            };
        }
    }
    get_icon_info(key)
}

/// Group apps by their icon (using resolved icon path if available, or icon name)
pub fn group_apps_by_icon(
    app_ids: &[String],
    app_map: &HashMap<String, crate::desktop::DesktopEntry>,
) -> Vec<(String, Vec<String>)> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    for id in app_ids {
        if let Some(entry) = app_map.get(id) {
            let key = if entry.icon.is_empty() {
                "application-default-icon".to_string()
            } else if let Some(resolved) = resolve_icon(&entry.icon) {
                resolved.to_string_lossy().to_string()
            } else {
                entry.icon.clone()
            };
            groups.entry(key).or_default().push(id.clone());
        } else {
            groups.entry(id.clone()).or_default().push(id.clone());
        }
    }

    // Sort apps inside each group by display name
    for app_list in groups.values_mut() {
        app_list.sort_by(|a, b| {
            let name_a = app_map
                .get(a)
                .map(|e| e.name.to_lowercase())
                .unwrap_or_else(|| a.to_lowercase());
            let name_b = app_map
                .get(b)
                .map(|e| e.name.to_lowercase())
                .unwrap_or_else(|| b.to_lowercase());
            name_a.cmp(&name_b)
        });
    }

    let mut result: Vec<(String, Vec<String>)> = groups.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_hsl() {
        // Pure red
        let (h, s, l) = rgb_to_hsl(1.0, 0.0, 0.0);
        assert!((h * 360.0 - 0.0).abs() < 1e-4);
        assert!((s - 1.0).abs() < 1e-4);
        assert!((l - 0.5).abs() < 1e-4);

        // Pure green
        let (h, s, _l) = rgb_to_hsl(0.0, 1.0, 0.0);
        assert!((h * 360.0 - 120.0).abs() < 1e-4);
        assert!((s - 1.0).abs() < 1e-4);

        // Pure blue
        let (h, s, _l) = rgb_to_hsl(0.0, 0.0, 1.0);
        assert!((h * 360.0 - 240.0).abs() < 1e-4);
        assert!((s - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_analyze_dynamic_image_transparent_pixels_ignored() {
        use image::{ImageBuffer, Rgba};
        // Create an image that is 90% transparent with black [0, 0, 0, 0]
        // and 10% pure bright yellow [255, 255, 0, 255]
        let width = 20;
        let height = 20;
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                if x < 4 && y < 5 {
                    // 20 pixels of bright yellow
                    img.put_pixel(x, y, Rgba([255, 255, 0, 255]));
                } else {
                    // 380 pixels of transparent background
                    img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
        let dynamic_img = image::DynamicImage::ImageRgba8(img);
        let info = analyze_dynamic_image(&dynamic_img, Path::new("test.png")).unwrap();

        // The transparent pixels must NOT pull lightness to 0 or saturation to 0
        assert!(
            !info.is_monochrome,
            "Icon should not be classified as monochrome"
        );
        assert!(
            (info.avg_hue - 60.0).abs() < 5.0,
            "Hue should be yellow (~60deg), got {}",
            info.avg_hue
        );
        assert!(
            (info.avg_saturation - 1.0).abs() < 0.1,
            "Saturation should be near 1.0, got {}",
            info.avg_saturation
        );
        assert!(
            (info.avg_lightness - 0.5).abs() < 0.1,
            "Lightness should be near 0.5, got {}",
            info.avg_lightness
        );
    }

    #[test]
    fn test_hwloc_icon_fallback_color() {
        let info = get_icon_info("hwloc");
        // On systems with application-x-executable fallback, it should resolve a path
        if let Some(path) = &info.path {
            println!("hwloc resolved to: {:?}", path);
            println!(
                "hwloc info: hue={}, sat={}, light={}, mono={}",
                info.avg_hue, info.avg_saturation, info.avg_lightness, info.is_monochrome
            );
            assert!(!info.is_monochrome);
            // Yellow / gold / amber range (around 30-65 degrees)
            assert!(info.avg_hue > 20.0 && info.avg_hue < 70.0);
        }
    }
}
