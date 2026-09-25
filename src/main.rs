mod backup;
mod dconf;
mod desktop;

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("Scanning installed desktop applications...");
    let apps = desktop::get_app_map()?;
    println!("Found {} desktop applications.", apps.len());

    let layout_str = dconf::read_layout()?;
    let pages = dconf::parse_layout(&layout_str)?;
    println!("Loaded GNOME layout with {} pages.", pages.len());

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let backup_path = backup::create_backup(&pages, &[], &current_dir)?;
    println!("Backup created at: {}", backup_path.display());

    Ok(())
}
