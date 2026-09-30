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
    FillFirst,
    FillFirstComplete,
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
                AppState::FillFirst => {
                    self.perform_fill_first();
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
                        | AppState::FillFirstComplete
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
            KeyCode::Char('4') | KeyCode::Char('f') => {
                self.state = AppState::FillFirst;
            }
            KeyCode::Char('5') | KeyCode::Char('b') => {
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
            Ok((backup_path, total_apps, total_pages, folder_count)) => {
                self.status_message = format!(
                    "Gradient sort complete!\nSorted {} apps across {} pages.\n{} shared-icon groups created and placed at the beginning (sorted by gradient).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_apps,
                    total_pages,
                    folder_count,
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
                    "Random sort complete!\nRandomized {} items across {} pages (groups moved to beginning, zero gaps).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
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
                    "Alphabetical sort complete!\nSorted {} items across {} pages from A to Z (groups moved to beginning, zero gaps).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
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

    fn perform_fill_first(&mut self) {
        match fill_first_page_logic(&self.current_dir) {
            Ok((backup_path, total_items, total_pages, moved_count, folder_count)) => {
                let group_msg = if folder_count > 0 {
                    format!(
                        "{} folder group{} moved to the beginning.",
                        folder_count,
                        if folder_count == 1 { "" } else { "s" }
                    )
                } else {
                    "No folder groups found.".to_string()
                };
                if moved_count > 0 {
                    self.status_message = format!(
                        "First page filled!\n{}\nMoved {} items forward to completely fill the first page (24 items).\nNow {} total items across {} pages.\n\nYour folder groupings were preserved.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                        group_msg,
                        moved_count,
                        total_items,
                        total_pages,
                        backup_path.file_name().unwrap_or_default().to_string_lossy()
                    );
                } else {
                    self.status_message = format!(
                        "First page filled!\n{}\nFirst page has 24 items (total {} items across {} pages).\n\nYour folder groupings were preserved.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                        group_msg,
                        total_items,
                        total_pages,
                        backup_path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                self.state = AppState::FillFirstComplete;
            }
            Err(e) => {
                self.error_msg = format!("Failed to fill first page: {:#}", e);
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
            AppState::FillFirst => {
                self.draw_processing(frame, "Filling first page & moving groups to beginning...")
            }
            AppState::FillFirstComplete => {
                self.draw_complete(frame, "First Page Filled", &self.status_message)
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
            Line::from("  [4] / [f]  Fill first page (move groups to beginning & compact gaps)"),
            Line::from("  [5] / [b]  Restore from backup"),
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

    // 1. Add all visible desktop apps across global and local directories
    for (id, entry) in app_map {
        if entry.is_app && seen.insert(id.clone()) {
            items.push(id.clone());
        }
    }

    // 2. Include any desktop files that were in existing pages (only if valid and executable)
    for page in pages {
        for (id, _) in page {
            if id.ends_with(".desktop") && seen.insert(id.clone()) {
                if app_map.get(id).map_or(false, |e| e.is_app) {
                    items.push(id.clone());
                }
            }
        }
    }

    items
}

/// Collect grid items, placing folder groups at the beginning and gathering all
/// standalone apps across global and local applications into a unified list without gaps.
fn collect_grid_items(
    app_map: &HashMap<String, desktop::DesktopEntry>,
    pages: &[Vec<(String, u32)>],
    folder_children: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut group_ids = Vec::new();
    let mut seen_groups = HashSet::new();

    // 1. Groups from folder_children
    for f in folder_children {
        if seen_groups.insert(f.clone()) {
            group_ids.push(f.clone());
        }
    }

    // 2. Groups from current pages
    for page in pages {
        for (id, _) in page {
            let is_folder = !id.ends_with(".desktop") || folder_children.contains(id);
            if is_folder && seen_groups.insert(id.clone()) {
                group_ids.push(id.clone());
            }
        }
    }

    // 3. Groups from dconf that have at least 2 valid installed apps
    let all_folders = dconf::read_app_folders().unwrap_or_default();
    for folder in &all_folders {
        let valid_count = folder
            .apps
            .iter()
            .filter(|a| app_map.get(*a).map_or(false, |e| e.is_app))
            .count();
        if valid_count >= 2 && seen_groups.insert(folder.id.clone()) {
            group_ids.push(folder.id.clone());
        }
    }

    // 4. Apps that are contained inside folders should not be placed on the outer grid
    let mut apps_in_folders = HashSet::new();
    for folder in &all_folders {
        if seen_groups.contains(&folder.id) {
            for app_id in &folder.apps {
                apps_in_folders.insert(app_id.clone());
            }
        }
    }

    // 5. Standalone apps from app_map (both global and local together) and existing pages
    let mut standalone_apps = Vec::new();
    let mut seen_apps = HashSet::new();

    // From app_map (all visible apps installed on system)
    for (id, entry) in app_map {
        if entry.is_app && !apps_in_folders.contains(id) && seen_apps.insert(id.clone()) {
            standalone_apps.push(id.clone());
        }
    }

    // Also preserve any valid desktop items in existing pages
    for page in pages {
        for (id, _) in page {
            if id.ends_with(".desktop")
                && !apps_in_folders.contains(id)
                && seen_apps.insert(id.clone())
            {
                if app_map.get(id).map_or(false, |e| e.is_app) {
                    standalone_apps.push(id.clone());
                }
            }
        }
    }

    (group_ids, standalone_apps)
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

/// Find longest common word prefix for group naming
fn longest_common_word_prefix(names: &[&str]) -> Option<String> {
    if names.is_empty() {
        return None;
    }

    let words_list: Vec<Vec<&str>> = names
        .iter()
        .map(|n| n.split_whitespace().collect())
        .collect();

    let mut common = Vec::new();
    let min_len = words_list.iter().map(|w| w.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let first_word = words_list[0][i];
        if words_list
            .iter()
            .all(|w| w[i].eq_ignore_ascii_case(first_word))
        {
            common.push(first_word);
        } else {
            break;
        }
    }

    if common.is_empty() {
        None
    } else {
        Some(common.join(" "))
    }
}

/// Determine a human-readable folder name for apps sharing the same icon
fn determine_folder_name(
    apps: &[String],
    app_map: &HashMap<String, desktop::DesktopEntry>,
    icon_name: &str,
) -> String {
    let names: Vec<&str> = apps
        .iter()
        .filter_map(|id| app_map.get(id).map(|e| e.name.as_str()))
        .collect();

    if !names.is_empty() {
        if let Some(prefix) = longest_common_word_prefix(&names) {
            if prefix.len() >= 3 {
                return prefix;
            }
        }
    }

    let clean = icon_name
        .trim_end_matches(".png")
        .trim_end_matches(".svg")
        .trim_end_matches(".xpm")
        .replace(['-', '_'], " ");

    clean
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// Create a sanitized, stable folder ID
fn sanitize_folder_id(key: &str) -> String {
    let stem = Path::new(key)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(key);

    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    format!("appmenu-group-{}", sanitized.trim_matches('-'))
}

/// Perform gradient sorting logic:
/// 1. Start analysis without considering previous groupings (undo all groupings)
/// 2. If multiple apps share the same icon, put them into the same group (GNOME app folder)
/// 3. Sort those groups by color gradient and place them at the beginning of the grid
/// 4. Follow with all single-app items, sorted by color gradient (rainbow flow + dark/black items grouped)
pub fn sort_gradient_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize, usize)> {
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

    // Group apps by icon
    let raw_groups = icon::group_apps_by_icon(&all_apps, &app_map);

    let mut folder_items: Vec<GridItemSortData> = Vec::new();
    let mut single_items: Vec<GridItemSortData> = Vec::new();
    let mut active_folder_ids: Vec<String> = Vec::new();

    for (icon_key, app_ids) in raw_groups {
        let icon_info = icon::get_icon_info_for_key(&icon_key);

        if app_ids.len() > 1 {
            // Multiple apps share this icon -> put them in the same group (GNOME app folder)
            let folder_name = determine_folder_name(&app_ids, &app_map, &icon_key);
            let folder_id = sanitize_folder_id(&icon_key);

            dconf::create_or_update_app_folder(&folder_id, &folder_name, &app_ids)
                .context("Failed to create app folder in dconf")?;
            active_folder_ids.push(folder_id.clone());

            folder_items.push(GridItemSortData {
                id: folder_id,
                display_name: folder_name,
                avg_hue: icon_info.avg_hue,
                avg_lightness: icon_info.avg_lightness,
                is_monochrome: icon_info.is_monochrome,
            });
        } else if let Some(app_id) = app_ids.first() {
            let app_name = app_map
                .get(app_id)
                .map(|e| e.name.clone())
                .unwrap_or_else(|| app_id.clone());

            single_items.push(GridItemSortData {
                id: app_id.clone(),
                display_name: app_name,
                avg_hue: icon_info.avg_hue,
                avg_lightness: icon_info.avg_lightness,
                is_monochrome: icon_info.is_monochrome,
            });
        }
    }

    let folder_count = folder_items.len();

    // Effective hue: shift hue so red (around 345°-360° and 0°-15°) starts continuously at the beginning of the rainbow
    let effective_hue = |hue: f32| -> f32 {
        let shifted = (hue - 345.0) % 360.0;
        if shifted < 0.0 {
            shifted + 360.0
        } else {
            shifted
        }
    };

    // Sort comparator:
    // 1. Chromatic items sorted by continuous rainbow hue (effective_hue), then lightness
    // 2. Dark/monochrome items placed together, sorted by lightness (so all dark/black icons like Terminal, Antigravity, Obsidian, CMake are grouped together!)
    let sort_items =
        |a: &GridItemSortData, b: &GridItemSortData| match (a.is_monochrome, b.is_monochrome) {
            (false, false) => {
                let eff_a = effective_hue(a.avg_hue);
                let eff_b = effective_hue(b.avg_hue);
                eff_a
                    .partial_cmp(&eff_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        a.avg_lightness
                            .partial_cmp(&b.avg_lightness)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| {
                        a.display_name
                            .to_lowercase()
                            .cmp(&b.display_name.to_lowercase())
                    })
            }
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            (true, true) => a
                .avg_lightness
                .partial_cmp(&b.avg_lightness)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.display_name
                        .to_lowercase()
                        .cmp(&b.display_name.to_lowercase())
                }),
        };

    // Sort groups by gradient
    folder_items.sort_by(sort_items);
    // Sort single-app items by gradient
    single_items.sort_by(sort_items);

    // Combine: groups (same icon) at the beginning, followed by single items
    let mut final_grid_ids = Vec::new();
    for folder in folder_items {
        final_grid_ids.push(folder.id);
    }
    for item in single_items {
        final_grid_ids.push(item.id);
    }

    let new_pages = pack_into_pages(&final_grid_ids);
    let total_pages = new_pages.len();

    // Set active folders in GNOME Shell to only our created icon groups
    dconf::write_folder_children(&active_folder_ids)
        .context("Failed to update folder-children in dconf")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_apps, total_pages, folder_count))
}

/// Perform random sorting logic on all standalone apps, preserving folder groups at the beginning
/// and filling pages consecutively with zero gaps.
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
    let (group_ids, mut standalone_apps) =
        collect_grid_items(&app_map, &pages, &folder_children);

    let original = standalone_apps.clone();
    let mut rng = thread_rng();
    let mut attempts = 0;
    loop {
        standalone_apps.shuffle(&mut rng);
        attempts += 1;
        if standalone_apps != original || attempts > 50 {
            break;
        }
    }

    // Move groups to the beginning, followed by shuffled apps
    let mut all_items = group_ids.clone();
    all_items.extend(standalone_apps);

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items);
    let total_pages = new_pages.len();

    // Preserve folder-children in dconf
    dconf::write_folder_children(&group_ids)
        .context("Failed to update folder-children in dconf")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_items, total_pages))
}

/// Perform alphabetical sorting logic on all standalone apps, preserving folder groups at the beginning
/// and filling pages consecutively with zero gaps.
pub fn sort_alpha_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let (group_ids, mut standalone_apps) =
        collect_grid_items(&app_map, &pages, &folder_children);

    standalone_apps.sort_by(|a, b| {
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

    // Move groups to the beginning, followed by alphabetically sorted apps
    let mut all_items = group_ids.clone();
    all_items.extend(standalone_apps);

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items);
    let total_pages = new_pages.len();

    // Preserve folder-children in dconf
    dconf::write_folder_children(&group_ids)
        .context("Failed to update folder-children in dconf")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_items, total_pages))
}

/// Fill the first app menu page up to ITEMS_PER_PAGE (24) by moving all folder groupings
/// to the beginning of the grid and shifting single items forward to compact gaps.
/// Preserves existing manual folders and groupings without un-grouping or modifying their contents.
/// Discards phantom/uninstalled applications so no visual empty holes remain.
pub fn fill_first_page_logic(output_dir: &Path) -> Result<(PathBuf, usize, usize, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    // Backup both layout and folder configuration before making changes
    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let all_folders = dconf::read_app_folders().unwrap_or_default();

    // 1. Identify all active folder groups
    let mut active_folders: Vec<String> = Vec::new();
    let mut seen_folders = HashSet::new();

    // Folders currently in folder_children
    for f in &folder_children {
        if seen_folders.insert(f.clone()) {
            active_folders.push(f.clone());
        }
    }

    // Folder IDs currently placed on pages
    for page in &pages {
        for (id, _) in page {
            let is_folder = !id.ends_with(".desktop") || folder_children.contains(id);
            if is_folder && seen_folders.insert(id.clone()) {
                active_folders.push(id.clone());
            }
        }
    }

    // Also include any dconf folder that has at least 2 valid installed apps
    for folder in &all_folders {
        let valid_app_count = folder
            .apps
            .iter()
            .filter(|app_id| app_map.get(*app_id).map_or(false, |e| e.is_app))
            .count();
        if valid_app_count >= 2 && seen_folders.insert(folder.id.clone()) {
            active_folders.push(folder.id.clone());
        }
    }

    // 2. Identify all apps that are contained inside ANY active folder
    let mut apps_in_folders = HashSet::new();
    for folder in &all_folders {
        if seen_folders.contains(&folder.id) {
            for app_id in &folder.apps {
                apps_in_folders.insert(app_id.clone());
            }
        }
    }

    // 3. Collect standalone apps preserving existing page order
    let mut standalone_apps = Vec::new();
    let mut seen_apps = HashSet::new();

    // First: preserve apps from existing pages in their current order,
    // ensuring they are NOT in a folder and ARE valid installed executable apps.
    for page in &pages {
        for (id, _) in page {
            if id.ends_with(".desktop")
                && !apps_in_folders.contains(id)
                && app_map.get(id).map_or(false, |e| e.is_app)
                && seen_apps.insert(id.clone())
            {
                standalone_apps.push(id.clone());
            }
        }
    }

    // Second: append any installed apps that were missing from pages and not inside folders
    for (id, entry) in &app_map {
        if entry.is_app && !apps_in_folders.contains(id) && seen_apps.insert(id.clone()) {
            standalone_apps.push(id.clone());
        }
    }

    // 4. Combine: Folder groups FIRST at the beginning of Page 0, followed by standalone apps
    let folder_count = active_folders.len();
    let mut all_items = active_folders.clone();
    all_items.extend(standalone_apps);

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items);
    let total_pages = new_pages.len();

    let initial_visible_first_page_len = pages
        .first()
        .map(|p| {
            p.iter()
                .filter(|(id, _)| {
                    !id.ends_with(".desktop")
                        || (!apps_in_folders.contains(id)
                            && app_map.get(id).map_or(false, |e| e.is_app))
                })
                .count()
        })
        .unwrap_or(0);

    let new_first_page_len = new_pages.first().map(|p| p.len()).unwrap_or(0);
    let moved_count = if new_first_page_len > initial_visible_first_page_len {
        new_first_page_len - initial_visible_first_page_len
    } else {
        0
    };

    // 5. Update dconf: sync folder_children and new packed layout
    dconf::write_folder_children(&active_folders)
        .context("Failed to update folder-children in dconf")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((
        backup_path,
        total_items,
        total_pages,
        moved_count,
        folder_count,
    ))
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
    -f, --fill-first        Fill first menu page, move groups to beginning, and compact gaps
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
                let (backup_path, total_apps, total_pages, folder_count) =
                    sort_gradient_logic(&current_dir)?;
                println!(
                    "Success! Organized {} apps across {} pages ({} shared-icon groups at beginning, sorted by gradient).",
                    total_apps, total_pages, folder_count
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-r" | "--random" => {
                println!("Sorting GNOME app menu randomly...");
                let (backup_path, total_items, total_pages) = sort_random_logic(&current_dir)?;
                println!(
                    "Success! Randomized {} items across {} pages (groups placed at beginning, zero gaps).",
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
                    "Success! Sorted {} items across {} pages from A to Z (groups placed at beginning, zero gaps).",
                    total_items, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-f" | "--fill-first" => {
                println!("Filling first menu page and moving groups to beginning...");
                let (backup_path, total_items, total_pages, moved_count, folder_count) =
                    fill_first_page_logic(&current_dir)?;
                if folder_count > 0 {
                    println!("Moved {} folder group(s) to the beginning.", folder_count);
                }
                if moved_count > 0 {
                    println!(
                        "Success! Filled first page by moving {} items forward (total {} items across {} pages).",
                        moved_count, total_items, total_pages
                    );
                } else {
                    println!(
                        "First page is filled (total {} items across {} pages).",
                        total_items, total_pages
                    );
                }
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

    #[test]
    fn test_compact_and_fill_first_page_with_groups() {
        // Page 0 has a mix of apps and a group in the middle
        let mut page0 = Vec::new();
        page0.push(("app0.desktop".to_string(), 0));
        page0.push(("app1.desktop".to_string(), 1));
        page0.push(("my-custom-folder-uuid".to_string(), 2)); // Group in middle
        for i in 3..18 {
            page0.push((format!("app{}.desktop", i), i as u32));
        }

        // Page 1 has another group and some apps
        let mut page1 = Vec::new();
        page1.push(("app18.desktop".to_string(), 0));
        page1.push(("another-group-id".to_string(), 1)); // Group on page 1
        for i in 2..10 {
            page1.push((format!("app{}.desktop", i + 18), i as u32));
        }

        let pages = vec![page0, page1];
        let folder_children = vec![
            "my-custom-folder-uuid".to_string(),
            "another-group-id".to_string(),
        ];
        let folder_set: HashSet<String> = folder_children.into_iter().collect();

        let is_group = |id: &str| -> bool { folder_set.contains(id) || !id.ends_with(".desktop") };

        let mut group_items = Vec::new();
        let mut single_items = Vec::new();

        for page in &pages {
            for (id, _) in page {
                if is_group(id) {
                    group_items.push(id.clone());
                } else {
                    single_items.push(id.clone());
                }
            }
        }

        let mut all_items = group_items;
        all_items.extend(single_items);

        let repacked = pack_into_pages(&all_items);
        assert_eq!(repacked.len(), 2);
        assert_eq!(repacked[0].len(), 24);

        // Verify groups are moved to the very beginning of the first page
        assert_eq!(repacked[0][0].0, "my-custom-folder-uuid");
        assert_eq!(repacked[0][1].0, "another-group-id");

        // Verify single items follow immediately
        assert_eq!(repacked[0][2].0, "app0.desktop");
        assert_eq!(repacked[0][3].0, "app1.desktop");
    }

    #[test]
    fn test_longest_common_word_prefix() {
        let names = vec![
            "Avahi Zeroconf Browser",
            "Avahi SSH Server Browser",
            "Avahi VNC Server Browser",
        ];
        assert_eq!(
            longest_common_word_prefix(&names),
            Some("Avahi".to_string())
        );

        let names2 = vec!["OpenJDK Java 25 Console", "OpenJDK Java 25 Shell"];
        assert_eq!(
            longest_common_word_prefix(&names2),
            Some("OpenJDK Java 25".to_string())
        );
    }

    #[test]
    fn test_collect_grid_items_and_pack() {
        let mut app_map = HashMap::new();
        // Add global and local apps
        app_map.insert(
            "org.gnome.Calculator.desktop".to_string(),
            desktop::DesktopEntry {
                id: "org.gnome.Calculator.desktop".to_string(),
                name: "Calculator".to_string(),
                icon: "org.gnome.Calculator".to_string(),
                categories: vec![],
                is_app: true,
            },
        );
        app_map.insert(
            "wine-Programs-Game.desktop".to_string(),
            desktop::DesktopEntry {
                id: "wine-Programs-Game.desktop".to_string(),
                name: "Retro Game".to_string(),
                icon: "wine".to_string(),
                categories: vec![],
                is_app: true,
            },
        );
        for i in 0..30 {
            app_map.insert(
                format!("sys-app{}.desktop", i),
                desktop::DesktopEntry {
                    id: format!("sys-app{}.desktop", i),
                    name: format!("Sys App {:02}", i),
                    icon: "app".to_string(),
                    categories: vec![],
                    is_app: true,
                },
            );
        }

        let folder_children = vec!["folder-uuid-1".to_string()];
        let pages = vec![vec![
            ("folder-uuid-1".to_string(), 0),
            ("sys-app0.desktop".to_string(), 1),
        ]];

        let (group_ids, mut standalone_apps) =
            collect_grid_items(&app_map, &pages, &folder_children);
        assert_eq!(group_ids, vec!["folder-uuid-1".to_string()]);
        assert_eq!(standalone_apps.len(), 32);

        // Test alphabetical sort across unified pool (both Wine local and Gnome global)
        standalone_apps.sort_by(|a, b| {
            let name_a = app_map.get(a).map(|e| e.name.to_lowercase()).unwrap_or_else(|| a.to_lowercase());
            let name_b = app_map.get(b).map(|e| e.name.to_lowercase()).unwrap_or_else(|| b.to_lowercase());
            name_a.cmp(&name_b)
        });

        // "Calculator" should come first among apps
        assert_eq!(standalone_apps[0], "org.gnome.Calculator.desktop");
        // "Retro Game" should be sorted under 'R', between sys-apps
        assert!(standalone_apps.contains(&"wine-Programs-Game.desktop".to_string()));

        let mut all_items = group_ids;
        all_items.extend(standalone_apps);

        let new_pages = pack_into_pages(&all_items);
        assert_eq!(new_pages.len(), 2);
        // First page is fully packed with 24 items, zero gaps
        assert_eq!(new_pages[0].len(), 24);
        assert_eq!(new_pages[0][0].0, "folder-uuid-1"); // Folder group at index 0
        assert_eq!(new_pages[0][1].0, "org.gnome.Calculator.desktop"); // Calculator immediately follows
        for (pos, (_, p)) in new_pages[0].iter().enumerate() {
            assert_eq!(*p, pos as u32);
        }

        // Second page has the remaining 9 items, packed consecutively
        assert_eq!(new_pages[1].len(), 9);
        for (pos, (_, p)) in new_pages[1].iter().enumerate() {
            assert_eq!(*p, pos as u32);
        }
    }

    #[test]
    fn test_fill_first_page_logic_removes_phantoms_and_folder_apps() {
        let mut app_map = HashMap::new();
        // 30 real installed apps
        for i in 0..30 {
            app_map.insert(
                format!("real-app{}.desktop", i),
                desktop::DesktopEntry {
                    id: format!("real-app{}.desktop", i),
                    name: format!("Real App {:02}", i),
                    icon: "app".to_string(),
                    categories: vec![],
                    is_app: true,
                },
            );
        }
        // A phantom app (is_app = false)
        app_map.insert(
            "phantom.desktop".to_string(),
            desktop::DesktopEntry {
                id: "phantom.desktop".to_string(),
                name: "Phantom".to_string(),
                icon: "phantom".to_string(),
                categories: vec![],
                is_app: false,
            },
        );

        // App inside folder
        let folder_id = "folder-tools".to_string();
        let apps_in_folder = vec!["real-app0.desktop".to_string(), "real-app1.desktop".to_string()];

        let active_folders = vec![folder_id.clone()];
        let apps_in_folders_set: HashSet<String> = apps_in_folder.into_iter().collect();

        // Existing page 0 has:
        // folder-tools, real-app0 (which is in folder!), phantom.desktop, deleted-app.desktop (not in app_map),
        // and some real apps
        let mut page0 = vec![
            (folder_id.clone(), 0),
            ("real-app0.desktop".to_string(), 1),
            ("phantom.desktop".to_string(), 2),
            ("deleted-app.desktop".to_string(), 3),
        ];
        for i in 2..20 {
            page0.push((format!("real-app{}.desktop", i), i as u32 + 2));
        }

        let mut page1 = Vec::new();
        for i in 20..30 {
            page1.push((format!("real-app{}.desktop", i), (i - 20) as u32));
        }

        let pages = vec![page0, page1];

        // Process items as fill_first_page_logic does:
        let mut standalone_apps = Vec::new();
        let mut seen_apps = HashSet::new();

        for page in &pages {
            for (id, _) in page {
                if id.ends_with(".desktop")
                    && !apps_in_folders_set.contains(id)
                    && app_map.get(id).map_or(false, |e| e.is_app)
                    && seen_apps.insert(id.clone())
                {
                    standalone_apps.push(id.clone());
                }
            }
        }

        for (id, entry) in &app_map {
            if entry.is_app && !apps_in_folders_set.contains(id) && seen_apps.insert(id.clone()) {
                standalone_apps.push(id.clone());
            }
        }

        let mut all_items = active_folders.clone();
        all_items.extend(standalone_apps);

        let new_pages = pack_into_pages(&all_items);

        // 1 folder + 28 valid standalone apps (30 - 2 in folder) = 29 total items
        assert_eq!(all_items.len(), 29);
        assert_eq!(new_pages.len(), 2);
        // Page 0 is fully filled with 24 items, zero gaps!
        assert_eq!(new_pages[0].len(), 24);
        assert_eq!(new_pages[0][0].0, "folder-tools"); // folder at pos 0
        // No phantom or inside-folder apps exist in new_pages
        for page in &new_pages {
            for (id, _) in page {
                assert_ne!(id, "phantom.desktop");
                assert_ne!(id, "deleted-app.desktop");
                assert_ne!(id, "real-app0.desktop");
                assert_ne!(id, "real-app1.desktop");
            }
        }
    }
}
