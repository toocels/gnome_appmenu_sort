mod backup;
mod dconf;
mod desktop;
mod icon;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Alignment,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Standard GNOME Shell 40+ app grid dimensions: 4 rows x 6 columns = 24 items per page
const ITEMS_PER_PAGE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppState {
    MainMenu,
    Sorting,
    SortComplete,
    RandomSorting,
    RandomComplete,
    AlphaSorting,
    AlphaComplete,
    RestoreList,
    Restoring,
    RestoreComplete,
    Error,
}

struct App {
    state: AppState,
    error_msg: String,
    backup_files: Vec<PathBuf>,
    backup_state: ListState,
    status_message: String,
    current_dir: PathBuf,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::MainMenu,
            error_msg: String::new(),
            backup_files: Vec::new(),
            backup_state: ListState::default(),
            status_message: String::new(),
            current_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;

            // Process asynchronous actions after rendering their processing state
            match self.state {
                AppState::Sorting => {
                    self.perform_sort();
                    continue;
                }
                AppState::RandomSorting => {
                    self.perform_random_sort();
                    continue;
                }
                AppState::AlphaSorting => {
                    self.perform_alpha_sort();
                    continue;
                }
                AppState::Restoring => {
                    self.perform_restore();
                    continue;
                }
                _ => {}
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    match self.state {
                        AppState::MainMenu => {
                            if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                                break;
                            }
                            self.handle_main_menu(key.code);
                        }
                        AppState::RestoreList => {
                            self.handle_restore_list(key.code);
                        }
                        AppState::SortComplete
                        | AppState::RandomComplete
                        | AppState::AlphaComplete
                        | AppState::RestoreComplete
                        | AppState::Error => {
                            if key.code == KeyCode::Esc
                                || key.code == KeyCode::Enter
                                || key.code == KeyCode::Char(' ')
                                || key.code == KeyCode::Char('q')
                            {
                                self.state = AppState::MainMenu;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_main_menu(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('1') | KeyCode::Char('s') => {
                self.state = AppState::Sorting;
            }
            KeyCode::Char('2') | KeyCode::Char('r') => {
                self.state = AppState::RandomSorting;
            }
            KeyCode::Char('3') | KeyCode::Char('a') => {
                self.state = AppState::AlphaSorting;
            }
            KeyCode::Char('4') | KeyCode::Char('b') => {
                self.state = AppState::RestoreList;
                self.load_backup_files();
            }
            _ => {}
        }
    }

    fn handle_restore_list(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.backup_files.is_empty() {
                    let i = self
                        .backup_state
                        .selected()
                        .unwrap_or(self.backup_files.len() - 1);
                    let new_i = if i == 0 {
                        self.backup_files.len() - 1
                    } else {
                        i - 1
                    };
                    self.backup_state.select(Some(new_i));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.backup_files.is_empty() {
                    let i = self.backup_state.selected().unwrap_or(0);
                    let new_i = (i + 1) % self.backup_files.len();
                    self.backup_state.select(Some(new_i));
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = self.backup_state.selected() {
                    if idx < self.backup_files.len() {
                        self.state = AppState::Restoring;
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.state = AppState::MainMenu;
            }
            _ => {}
        }
    }

    fn load_backup_files(&mut self) {
        self.backup_files = backup::find_backups(&self.current_dir).unwrap_or_default();
        if !self.backup_files.is_empty() {
            self.backup_state.select(Some(0));
        } else {
            self.backup_state.select(None);
        }
    }

    fn perform_sort(&mut self) {
        match sort_gradient_logic(&self.current_dir) {
            Ok((backup_path, total_apps, total_pages)) => {
                self.status_message = format!(
                    "Gradient sort complete!
Sorted {} apps across {} pages.

Backup saved to:
{}

Press Super key twice to view your updated grid.",
                    total_apps,
                    total_pages,
                    backup_path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.state = AppState::SortComplete;
            }
            Err(e) => {
                self.error_msg = format!("Failed to sort by gradient: {:#}", e);
                self.state = AppState::Error;
            }
        }
    }

    fn perform_random_sort(&mut self) {
        match sort_random_logic(&self.current_dir) {
            Ok((backup_path, total_items, total_pages)) => {
                self.status_message = format!(
                    "Random sort complete!\nRandomized {} ungrouped apps across {} pages.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_items,
                    total_pages,
                    backup_path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.state = AppState::RandomComplete;
            }
            Err(e) => {
                self.error_msg = format!("Failed to sort randomly: {:#}", e);
                self.state = AppState::Error;
            }
        }
    }

    fn perform_alpha_sort(&mut self) {
        match sort_alpha_logic(&self.current_dir) {
            Ok((backup_path, total_items, total_pages)) => {
                self.status_message = format!(
                    "Alphabetical sort complete!\nSorted {} ungrouped apps across {} pages from A to Z.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_items,
                    total_pages,
                    backup_path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.state = AppState::AlphaComplete;
            }
            Err(e) => {
                self.error_msg = format!("Failed to sort alphabetically: {:#}", e);
                self.state = AppState::Error;
            }
        }
    }

    fn perform_restore(&mut self) {
        if let Some(idx) = self.backup_state.selected() {
            if idx >= self.backup_files.len() {
                return;
            }
            let file = &self.backup_files[idx];
            match restore_backup_file(file) {
                Ok(_) => {
                    self.status_message = format!(
                        "Successfully restored layout and folders from:\n{}\n\nPress Super key twice to view your restored grid.",
                        file.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.state = AppState::RestoreComplete;
                }
                Err(e) => {
                    self.error_msg = format!("Failed to restore backup: {:#}", e);
                    self.state = AppState::Error;
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        match self.state {
            AppState::MainMenu => self.draw_main_menu(frame),
            AppState::Sorting => self.draw_processing(frame, "Sorting apps by gradient..."),
            AppState::SortComplete => {
                self.draw_complete(frame, "Sort Complete", &self.status_message)
            }
            AppState::RandomSorting => self.draw_processing(frame, "Sorting apps randomly..."),
            AppState::RandomComplete => {
                self.draw_complete(frame, "Random Sort Complete", &self.status_message)
            }
            AppState::AlphaSorting => self.draw_processing(frame, "Sorting apps alphabetically..."),
            AppState::AlphaComplete => {
                self.draw_complete(frame, "Alphabetical Sort Complete", &self.status_message)
            }
            AppState::RestoreList => self.draw_restore_list(frame),
            AppState::Restoring => self.draw_processing(frame, "Restoring layout from backup..."),
            AppState::RestoreComplete => {
                self.draw_complete(frame, "Restore Complete", &self.status_message)
            }
            AppState::Error => self.draw_error(frame),
        }
    }

    fn draw_main_menu(&self, frame: &mut Frame) {
        let area = frame.area();

        let block = Block::default()
            .title(" GNOME App Menu Gradient Sorter ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let text = Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                "Organize your GNOME Shell application grid with ease!",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from("  [1] / [s]  Sort apps by color gradient (rainbow hue)"),
            Line::from("  [2] / [r]  Sort apps randomly"),
            Line::from("  [3] / [a]  Sort apps alphabetically (A to Z)"),
            Line::from("  [4] / [b]  Restore from backup"),
            Line::from(""),
            Line::from(Span::styled(
                "Press 'q' or ESC to exit",
                Style::default().fg(Color::DarkGray),
            )),
        ]);

        let p = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(p, area);
    }

    fn draw_processing(&self, frame: &mut Frame, msg: &str) {
        let area = frame.area();
        let block = Block::default()
            .title(" Processing ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue));

        let text = Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(msg, Style::default().fg(Color::Blue))),
            Line::from(""),
            Line::from("Analyzing desktop entries and icons... Please wait."),
        ]);

        let p = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center);

        frame.render_widget(p, area);
    }

    fn draw_complete(&self, frame: &mut Frame, title: &str, message: &str) {
        let area = frame.area();
        let block = Block::default()
            .title(format!(" {} ", title))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let mut lines = vec![Line::from("")];
        for line in message.lines() {
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::Green),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press Enter or ESC to go back to main menu",
            Style::default().fg(Color::DarkGray),
        )));

        let p = Paragraph::new(Text::from(lines))
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(p, area);
    }

    fn draw_restore_list(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let block = Block::default()
            .title(" Select Backup to Restore ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        if self.backup_files.is_empty() {
            let text = Text::from(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No backup files found in current directory.",
                    Style::default().fg(Color::Red),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("Looking for: *{}", backup::get_extension()),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from("Press ESC to return to main menu"),
            ]);

            let p = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center);

            frame.render_widget(p, area);
            return;
        }

        let items: Vec<ListItem> = self
            .backup_files
            .iter()
            .map(|f| {
                let name = f.file_name().unwrap_or_default().to_string_lossy();
                ListItem::new(name)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Yellow))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut self.backup_state);
    }

    fn draw_error(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let block = Block::default()
            .title(" Error ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        let text = Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                &self.error_msg,
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from("Press Enter or ESC to return to main menu"),
        ]);

        let p = Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        frame.render_widget(p, area);
    }
}

/// Collect all installed application IDs ungrouped (without any initial folder groupings)
fn collect_all_apps_ungrouped(
    app_map: &HashMap<String, desktop::DesktopEntry>,
    pages: &[Vec<(String, u32)>],
) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    // 1. Add all visible desktop apps
    for (id, entry) in app_map {
        if entry.is_app && seen.insert(id.clone()) {
            items.push(id.clone());
        }
    }

    // 2. Include any desktop files that were in existing pages
    for page in pages {
        for (id, _) in page {
            if id.ends_with(".desktop") && seen.insert(id.clone()) {
                items.push(id.clone());
            }
        }
    }

    items
}

/// Chunk items into pages with position 0..23 per page (4x6 grid)
fn pack_into_pages(item_ids: &[String]) -> Vec<Vec<(String, u32)>> {
    let mut pages = Vec::new();
    let mut current_page = Vec::new();
    let mut pos = 0u32;

    for id in item_ids {
        current_page.push((id.clone(), pos));
        pos += 1;
        if current_page.len() >= ITEMS_PER_PAGE {
            pages.push(current_page);
            current_page = Vec::new();
            pos = 0;
        }
    }

    if !current_page.is_empty() {
        pages.push(current_page);
    }

    pages
}

struct GridItemSortData {
    id: String, // Folder ID or desktop application ID
    display_name: String,
    avg_hue: f32,
    avg_lightness: f32,
    is_monochrome: bool,
}

/// Perform gradient sorting logic:
/// 1. Start analysis without considering previous groupings (undo all groupings)
/// 2. If multiple apps share the same icon, put them into the same group (GNOME app folder)
/// 3. Sort those groups by color gradient and place them at the beginning of the grid
/// 4. Follow with all single-app items, sorted by color gradient (rainbow flow + dark/black items grouped)
pub fn sort_gradient_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    // Backup both layout and folder configuration before making changes
    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let all_apps = collect_all_apps_ungrouped(&app_map, &pages);
    let total_apps = all_apps.len();

    let mut items: Vec<GridItemSortData> = Vec::new();
    for id in &all_apps {
        let name = app_map.get(id).map(|e| e.name.clone()).unwrap_or_else(|| id.clone());
        let icon_name = app_map.get(id).map(|e| e.icon.as_str()).unwrap_or("");
        let icon_info = icon::get_icon_info(icon_name);
        items.push(GridItemSortData {
            id: id.clone(),
            display_name: name,
            avg_hue: icon_info.avg_hue,
            avg_lightness: icon_info.avg_lightness,
            is_monochrome: icon_info.is_monochrome,
        });
    }

    let sort_items = |a: &GridItemSortData, b: &GridItemSortData| match (a.is_monochrome, b.is_monochrome) {
        (false, false) => a.avg_hue.partial_cmp(&b.avg_hue).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.avg_lightness.partial_cmp(&b.avg_lightness).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase())),
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        (true, true) => a.avg_lightness.partial_cmp(&b.avg_lightness).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase())),
    };

    items.sort_by(sort_items);
    let final_grid_ids: Vec<String> = items.into_iter().map(|i| i.id).collect();
    let new_pages = pack_into_pages(&final_grid_ids);
    let total_pages = new_pages.len();

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_apps, total_pages))
}

/// Perform random sorting logic on all ungrouped apps
pub fn sort_random_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize)> {
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let mut all_apps = collect_all_apps_ungrouped(&app_map, &pages);
    let total_items = all_apps.len();

    let original = all_apps.clone();
    let mut rng = thread_rng();
    let mut attempts = 0;
    loop {
        all_apps.shuffle(&mut rng);
        attempts += 1;
        if all_apps != original || attempts > 50 {
            break;
        }
    }

    let new_pages = pack_into_pages(&all_apps);
    let total_pages = new_pages.len();

    dconf::clear_folder_children().context("Failed to clear folder groupings")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_items, total_pages))
}

/// Perform alphabetical sorting logic on all ungrouped apps
pub fn sort_alpha_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let mut all_apps = collect_all_apps_ungrouped(&app_map, &pages);
    let total_items = all_apps.len();

    all_apps.sort_by(|a, b| {
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

    let new_pages = pack_into_pages(&all_apps);
    let total_pages = new_pages.len();

    dconf::clear_folder_children().context("Failed to clear folder groupings")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_items, total_pages))
}

/// Restore layout and folder configuration from a backup file
pub fn restore_backup_file(path: &Path) -> Result<()> {
    let backup_data =
        backup::load_backup(path).context("Failed to read and parse backup TOML file")?;

    let layout_str = dconf::serialize_layout(&backup_data.pages);
    dconf::write_layout(&layout_str).context("Failed to write layout to dconf")?;

    // Restore folder configuration
    dconf::write_folder_children(&backup_data.folder_children)
        .context("Failed to restore folder-children to dconf")?;

    Ok(())
}

fn print_help() {
    println!(
        "\
GNOME App Menu Gradient Sorter (appmenu-gradient-sort)

Sort and organize your GNOME Shell application grid.

USAGE:
    appmenu-gradient-sort [OPTIONS]

OPTIONS:
    -g, --gradient          Sort apps by color gradient (groups shared-icon apps into folders at beginning)
    -r, --random            Sort apps randomly
    -a, --alpha             Sort apps alphabetically by name (A to Z)
    -b, --restore <FILE>    Restore layout and folders from a backup TOML file
    -h, --help              Show this help information
    -v, --version           Show version information

When run without arguments in a terminal, launches the interactive TUI.
"
    );
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if args.len() > 1 {
        match args[1].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-v" | "--version" => {
                println!("appmenu-gradient-sort v{}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-g" | "--gradient" => {
                println!("Sorting GNOME app menu by color gradient...");
                let (backup_path, total_apps, total_pages) =
                    sort_gradient_logic(&current_dir)?;
                println!(
                    "Success! Organized {} apps across {} pages.",
                    total_apps, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-r" | "--random" => {
                println!("Sorting GNOME app menu randomly...");
                let (backup_path, total_items, total_pages) = sort_random_logic(&current_dir)?;
                println!(
                    "Success! Randomized {} apps across {} pages.",
                    total_items, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-a" | "--alpha" => {
                println!("Sorting GNOME app menu alphabetically...");
                let (backup_path, total_items, total_pages) = sort_alpha_logic(&current_dir)?;
                println!(
                    "Success! Sorted {} apps across {} pages from A to Z.",
                    total_items, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-b" | "--restore" => {
                if args.len() < 3 {
                    eprintln!("Error: Missing backup file argument for --restore.");
                    eprintln!("Usage: appmenu-gradient-sort --restore <path/to/backup.toml>");
                    std::process::exit(1);
                }
                let backup_file = Path::new(&args[2]);
                restore_backup_file(backup_file)?;
                println!(
                    "Successfully restored GNOME app menu from {}",
                    backup_file.display()
                );
                println!("Press Super key twice to view your restored grid.");
                return Ok(());
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                print_help();
                std::process::exit(1);
            }
        }
    }

    // Check if terminal is available for interactive TUI
    if !io::stdout().is_terminal() {
        print_help();
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = app.run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_into_pages() {
        let items: Vec<String> = (0..50).map(|i| format!("app{}.desktop", i)).collect();
        let pages = pack_into_pages(&items);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].len(), 24);
        assert_eq!(pages[1].len(), 24);
        assert_eq!(pages[2].len(), 2);
        assert_eq!(pages[0][0], ("app0.desktop".to_string(), 0));
        assert_eq!(pages[0][23], ("app23.desktop".to_string(), 23));
        assert_eq!(pages[1][0], ("app24.desktop".to_string(), 0));
    }

}
