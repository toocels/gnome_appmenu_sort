mod backup;
mod dconf;
mod desktop;
mod icon;

use anyhow::Result;

fn main() -> Result<()> {
    println!("Scanning installed desktop applications and icons...");
    let apps = desktop::get_app_map()?;
    println!("Found {} desktop applications.", apps.len());

    for (id, entry) in apps.iter().take(5) {
        let info = icon::get_icon_info(&entry.icon);
        println!("{}: icon='{}' hue={:.1} sat={:.2} light={:.2}", id, entry.icon, info.avg_hue, info.avg_saturation, info.avg_lightness);
    }

    Ok(())
}
