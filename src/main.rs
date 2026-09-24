mod dconf;
mod desktop;

use anyhow::Result;

fn main() -> Result<()> {
    println!("Scanning installed desktop applications...");
    let apps = desktop::get_app_map()?;
    println!("Found {} desktop applications.", apps.len());

    let layout_str = dconf::read_layout()?;
    let pages = dconf::parse_layout(&layout_str)?;
    println!("Loaded GNOME layout with {} pages.", pages.len());

    Ok(())
}
