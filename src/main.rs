mod backup;
mod config;
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
    settings: config::Settings,
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
            settings: config::Settings::load(),
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
            KeyCode::Char('g') => {
                self.settings.group_icons = !self.settings.group_icons;
                let _ = self.settings.save();
            }
            KeyCode::Char('p') => {
                self.settings.groups_at_beginning = !self.settings.groups_at_beginning;
                let _ = self.settings.save();
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.settings.items_per_page = self
                    .settings
                    .items_per_page
                    .saturating_sub(1)
                    .max(config::MIN_ITEMS_PER_PAGE);
                let _ = self.settings.save();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.settings.items_per_page = (self.settings.items_per_page + 1)
                    .min(config::MAX_ITEMS_PER_PAGE);
                let _ = self.settings.save();
            }
            KeyCode::Char('v') => {
                self.settings.skip_favorite_apps = !self.settings.skip_favorite_apps;
                let _ = self.settings.save();
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
        match sort_gradient_logic(&self.current_dir, &self.settings) {
            Ok((backup_path, total_apps, total_pages, folder_count)) => {
                let placement = if self.settings.groups_at_beginning {
                    "placed at the beginning"
                } else {
                    "mixed in with apps"
                };
                self.status_message = format!(
                    "Gradient sort complete!\nSorted {} apps across {} pages.\n{} shared-icon groups created ({}, sorted by gradient).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_apps,
                    total_pages,
                    folder_count,
                    placement,
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
        match sort_random_logic(&self.current_dir, &self.settings) {
            Ok((backup_path, total_items, total_pages)) => {
                let placement = if self.settings.groups_at_beginning {
                    "groups at the beginning"
                } else {
                    "groups mixed in with apps"
                };
                self.status_message = format!(
                    "Random sort complete!\nRandomized {} items across {} pages ({}, zero gaps).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_items,
                    total_pages,
                    placement,
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
        match sort_alpha_logic(&self.current_dir, &self.settings) {
            Ok((backup_path, total_items, total_pages)) => {
                let placement = if self.settings.groups_at_beginning {
                    "groups at the beginning"
                } else {
                    "groups mixed in with apps"
                };
                self.status_message = format!(
                    "Alphabetical sort complete!\nSorted {} items across {} pages from A to Z ({}, zero gaps).\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                    total_items,
                    total_pages,
                    placement,
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
        match fill_first_page_logic(&self.current_dir, &self.settings) {
            Ok((backup_path, total_items, total_pages, moved_count, folder_count)) => {
                let group_msg = if folder_count > 0 {
                    format!(
                        "{} folder group{} found.",
                        folder_count,
                        if folder_count == 1 { "" } else { "s" }
                    )
                } else {
                    "No folder groups found.".to_string()
                };
                if moved_count > 0 {
                    self.status_message = format!(
                        "Grid compacted!\n{}\nMoved {} items forward to pack every page with zero gaps (24 items per page).\nNow {} total items across {} pages.\n\nYour folder groupings were preserved.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                        group_msg,
                        moved_count,
                        total_items,
                        total_pages,
                        backup_path.file_name().unwrap_or_default().to_string_lossy()
                    );
                } else {
                    self.status_message = format!(
                        "Grid compacted!\n{}\nAll pages are already packed with zero gaps (total {} items across {} pages).\n\nYour folder groupings were preserved.\n\nBackup saved to:\n{}\n\nPress Super key twice to view your updated grid.",
                        group_msg,
                        total_items,
                        total_pages,
                        backup_path.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                self.state = AppState::FillFirstComplete;
            }
            Err(e) => {
                self.error_msg = format!("Failed to compact grid: {:#}", e);
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
                self.draw_processing(frame, "Compacting all pages (zero gaps)...")
            }
            AppState::FillFirstComplete => {
                self.draw_complete(frame, "Grid Compacted", &self.status_message)
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

        let group_icons_val = if self.settings.group_icons { "ON" } else { "OFF" };
        let group_icons_color = if self.settings.group_icons {
            Color::Green
        } else {
            Color::DarkGray
        };
        let placement_val = if self.settings.groups_at_beginning {
            "Beginning of grid"
        } else {
            "Mixed in with apps"
        };
        let skip_favorites_val = if self.settings.skip_favorite_apps {
            "ON"
        } else {
            "OFF"
        };
        let skip_favorites_color = if self.settings.skip_favorite_apps {
            Color::Green
        } else {
            Color::DarkGray
        };

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
            Line::from("  [4] / [f]  Compact grid (fill every page, zero gaps)"),
            Line::from("  [5] / [b]  Restore from backup"),
            Line::from(""),
            Line::from(Span::styled("Settings", Style::default().fg(Color::Yellow))),
            Line::from(vec![
                Span::raw("  [g]  Group apps sharing an icon into folders: "),
                Span::styled(group_icons_val, Style::default().fg(group_icons_color)),
            ]),
            Line::from(format!("  [p]  Icon-group placement: {}", placement_val)),
            Line::from(format!(
                "  [-/+]  Items per grid page: {}  (match this to your GNOME Overview to avoid blank slots)",
                self.settings.items_per_page
            )),
            Line::from(vec![
                Span::raw("  [v]  Skip dash-favorited apps in grid (some GNOME versions leave them blank): "),
                Span::styled(skip_favorites_val, Style::default().fg(skip_favorites_color)),
            ]),
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

/// Collect all installed application IDs ungrouped (without any initial folder groupings).
/// `favorites` are excluded entirely: on some GNOME Shell versions a dash-favorited app
/// gets no tile in the app grid, so placing one there just reserves a blank-looking slot.
fn collect_all_apps_ungrouped(
    app_map: &HashMap<String, desktop::DesktopEntry>,
    pages: &[Vec<(String, u32)>],
    favorites: &HashSet<String>,
) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    // 1. Add all visible desktop apps across global and local directories
    for (id, entry) in app_map {
        if entry.is_app && !favorites.contains(id) && seen.insert(id.clone()) {
            items.push(id.clone());
        }
    }

    // 2. Include any desktop files that were in existing pages (only if valid and executable)
    for page in pages {
        for (id, _) in page {
            if id.ends_with(".desktop") && !favorites.contains(id) && seen.insert(id.clone()) {
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
/// When `group_icons` is false, all folder groupings are ignored and every app
/// (including ones previously inside a folder) is returned as standalone.
/// `favorites` (dash-pinned apps) are excluded from standalone placement; see
/// `collect_all_apps_ungrouped` for why.
fn collect_grid_items(
    app_map: &HashMap<String, desktop::DesktopEntry>,
    pages: &[Vec<(String, u32)>],
    folder_children: &[String],
    group_icons: bool,
    favorites: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    if !group_icons {
        return (Vec::new(), collect_all_apps_ungrouped(app_map, pages, favorites));
    }

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
        if entry.is_app
            && !apps_in_folders.contains(id)
            && !favorites.contains(id)
            && seen_apps.insert(id.clone())
        {
            standalone_apps.push(id.clone());
        }
    }

    // Also preserve any valid desktop items in existing pages
    for page in pages {
        for (id, _) in page {
            if id.ends_with(".desktop")
                && !apps_in_folders.contains(id)
                && !favorites.contains(id)
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

/// Chunk items into pages with position 0..items_per_page-1 per page, so every page but
/// the last is filled edge-to-edge with zero gaps (matches the real on-screen grid
/// capacity, which is tunable in settings since GNOME computes it from screen geometry).
fn pack_into_pages(item_ids: &[String], items_per_page: usize) -> Vec<Vec<(String, u32)>> {
    let items_per_page = items_per_page.max(1);
    let mut pages = Vec::new();
    let mut current_page = Vec::new();
    let mut pos = 0u32;

    for id in item_ids {
        current_page.push((id.clone(), pos));
        pos += 1;
        if current_page.len() >= items_per_page {
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

/// Dash/taskbar-favorited app IDs to exclude from grid placement, per settings
fn favorites_set(settings: &config::Settings) -> HashSet<String> {
    if settings.skip_favorite_apps {
        dconf::read_favorite_apps().into_iter().collect()
    } else {
        HashSet::new()
    }
}

/// Shuffle items in place, retrying (up to 50 times) until the order actually changes
fn shuffle_distinct(items: &mut Vec<String>) {
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    if items.len() <= 1 {
        return;
    }

    let original = items.clone();
    let mut rng = thread_rng();
    let mut attempts = 0;
    loop {
        items.shuffle(&mut rng);
        attempts += 1;
        if *items != original || attempts > 50 {
            break;
        }
    }
}

/// Resolve a human-readable display name for a grid item (app or folder group) for sorting/display
fn display_name_for_id(
    id: &str,
    app_map: &HashMap<String, desktop::DesktopEntry>,
    folder_names: &HashMap<String, String>,
) -> String {
    if let Some(entry) = app_map.get(id) {
        return entry.name.to_lowercase();
    }
    if let Some(name) = folder_names.get(id) {
        return name.to_lowercase();
    }
    id.to_lowercase()
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

/// Group standalone apps that share the same icon into new GNOME folders (so the
/// `group_icons` setting is honored the same way by every sort, not just gradient).
/// Returns (newly created folder ids, apps left standalone after grouping).
fn auto_group_standalone_apps(
    standalone_apps: &[String],
    app_map: &HashMap<String, desktop::DesktopEntry>,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut new_folder_ids = Vec::new();
    let mut remaining = Vec::new();

    for (icon_key, app_ids) in icon::group_apps_by_icon(standalone_apps, app_map) {
        if app_ids.len() > 1 {
            let folder_name = determine_folder_name(&app_ids, app_map, &icon_key);
            let folder_id = sanitize_folder_id(&icon_key);
            dconf::create_or_update_app_folder(&folder_id, &folder_name, &app_ids)
                .context("Failed to create app folder in dconf")?;
            new_folder_ids.push(folder_id);
        } else {
            remaining.extend(app_ids);
        }
    }

    Ok((new_folder_ids, remaining))
}

/// Perform gradient sorting logic:
/// 1. Start analysis without considering previous groupings (undo all groupings)
/// 2. If multiple apps share the same icon, put them into the same group (GNOME app folder)
/// 3. Sort those groups by color gradient and place them at the beginning of the grid
/// 4. Follow with all single-app items, sorted by color gradient (rainbow flow + dark/black items grouped)
pub fn sort_gradient_logic(
    output_dir: &Path,
    settings: &config::Settings,
) -> Result<(PathBuf, usize, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    // Backup both layout and folder configuration before making changes
    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let favorites = favorites_set(settings);
    let all_apps = collect_all_apps_ungrouped(&app_map, &pages, &favorites);
    let total_apps = all_apps.len();

    // Group apps by icon, unless the user disabled icon-grouping
    let raw_groups: Vec<(String, Vec<String>)> = if settings.group_icons {
        icon::group_apps_by_icon(&all_apps, &app_map)
    } else {
        all_apps
            .iter()
            .map(|id| {
                let key = app_map
                    .get(id)
                    .map(icon::resolve_icon_key)
                    .unwrap_or_else(|| id.clone());
                (key, vec![id.clone()])
            })
            .collect()
    };

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

    // Combine groups and single items, honoring the group-placement setting
    let mut final_grid_ids = Vec::new();
    if settings.groups_at_beginning {
        // Groups (same icon) at the beginning, followed by single items
        folder_items.sort_by(sort_items);
        single_items.sort_by(sort_items);
        for folder in folder_items {
            final_grid_ids.push(folder.id);
        }
        for item in single_items {
            final_grid_ids.push(item.id);
        }
    } else {
        // Groups mixed in with apps, sorted together by the same gradient order
        let mut combined = folder_items;
        combined.extend(single_items);
        combined.sort_by(sort_items);
        for item in combined {
            final_grid_ids.push(item.id);
        }
    }

    let new_pages = pack_into_pages(&final_grid_ids, settings.items_per_page);
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
pub fn sort_random_logic(
    output_dir: &Path,
    settings: &config::Settings,
) -> Result<(PathBuf, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let favorites = favorites_set(settings);
    let (mut group_ids, mut standalone_apps) =
        collect_grid_items(&app_map, &pages, &folder_children, settings.group_icons, &favorites);

    if settings.group_icons {
        let (new_folder_ids, remaining) = auto_group_standalone_apps(&standalone_apps, &app_map)?;
        group_ids.extend(new_folder_ids);
        standalone_apps = remaining;
    }

    // Move groups to the beginning, followed by shuffled apps; or shuffle everything together
    let all_items = if settings.groups_at_beginning {
        let mut shuffled = standalone_apps;
        shuffle_distinct(&mut shuffled);
        let mut items = group_ids.clone();
        items.extend(shuffled);
        items
    } else {
        let mut items = group_ids.clone();
        items.extend(standalone_apps);
        shuffle_distinct(&mut items);
        items
    };

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items, settings.items_per_page);
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
pub fn sort_alpha_logic(
    output_dir: &Path,
    settings: &config::Settings,
) -> Result<(PathBuf, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;
    let favorites = favorites_set(settings);
    let (mut group_ids, mut standalone_apps) =
        collect_grid_items(&app_map, &pages, &folder_children, settings.group_icons, &favorites);

    if settings.group_icons {
        let (new_folder_ids, remaining) = auto_group_standalone_apps(&standalone_apps, &app_map)?;
        group_ids.extend(new_folder_ids);
        standalone_apps = remaining;
    }

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

    // Move groups to the beginning, followed by alphabetically sorted apps; or
    // interleave groups into the same alphabetical ordering as the apps
    let all_items = if settings.groups_at_beginning {
        let mut items = group_ids.clone();
        items.extend(standalone_apps);
        items
    } else {
        let folder_names: HashMap<String, String> = dconf::read_app_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| (f.id, f.name))
            .collect();
        let mut items = group_ids.clone();
        items.extend(standalone_apps);
        items.sort_by(|a, b| {
            display_name_for_id(a, &app_map, &folder_names)
                .cmp(&display_name_for_id(b, &app_map, &folder_names))
        });
        items
    };

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items, settings.items_per_page);
    let total_pages = new_pages.len();

    // Preserve folder-children in dconf
    dconf::write_folder_children(&group_ids)
        .context("Failed to update folder-children in dconf")?;

    let new_layout_str = dconf::serialize_layout(&new_pages);
    dconf::write_layout(&new_layout_str).context("Failed to write new layout to dconf")?;

    Ok((backup_path, total_items, total_pages))
}

/// Compact the entire grid so every page is packed with zero gaps (not just the first),
/// moving folder groupings to the beginning or interleaving them with apps per settings.
/// Preserves existing manual folders and groupings without modifying their contents,
/// unless icon-grouping is disabled in settings, in which case all folders are flattened.
/// Discards phantom/uninstalled applications so no visual empty holes remain.
pub fn fill_first_page_logic(
    output_dir: &Path,
    settings: &config::Settings,
) -> Result<(PathBuf, usize, usize, usize, usize)> {
    let layout_str = dconf::read_layout().context("Failed to read current dconf layout")?;
    let pages = dconf::parse_layout(&layout_str).context("Failed to parse dconf layout")?;
    let folder_children = dconf::read_folder_children().unwrap_or_default();

    // Backup both layout and folder configuration before making changes
    let backup_path = backup::create_backup(&pages, &folder_children, output_dir)
        .context("Failed to create layout backup")?;

    let app_map =
        desktop::get_app_map().context("Failed to scan installed desktop applications")?;

    let (active_folders, apps_in_folders): (Vec<String>, HashSet<String>) = if settings.group_icons
    {
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

        (active_folders, apps_in_folders)
    } else {
        (Vec::new(), HashSet::new())
    };

    let mut active_folders = active_folders;
    let mut apps_in_folders = apps_in_folders;
    let favorites = favorites_set(settings);

    if settings.group_icons {
        // Group any currently-standalone apps that share an icon into new folders too,
        // not just preserve folders that already existed.
        let mut seen_candidates = HashSet::new();
        let mut standalone_candidates = Vec::new();
        for page in &pages {
            for (id, _) in page {
                if id.ends_with(".desktop")
                    && !apps_in_folders.contains(id)
                    && !favorites.contains(id)
                    && app_map.get(id).map_or(false, |e| e.is_app)
                    && seen_candidates.insert(id.clone())
                {
                    standalone_candidates.push(id.clone());
                }
            }
        }
        for (id, entry) in &app_map {
            if entry.is_app
                && !apps_in_folders.contains(id)
                && !favorites.contains(id)
                && seen_candidates.insert(id.clone())
            {
                standalone_candidates.push(id.clone());
            }
        }

        let (new_folder_ids, remaining) =
            auto_group_standalone_apps(&standalone_candidates, &app_map)?;
        let remaining_set: HashSet<&String> = remaining.iter().collect();
        for id in &standalone_candidates {
            if !remaining_set.contains(id) {
                apps_in_folders.insert(id.clone());
            }
        }
        active_folders.extend(new_folder_ids);
    }

    let folder_count = active_folders.len();
    let active_folder_set: HashSet<String> = active_folders.iter().cloned().collect();

    // 3. Combine folders and standalone apps, honoring the group-placement setting
    let mut all_items = Vec::new();
    let mut seen_items = HashSet::new();

    if settings.groups_at_beginning {
        // Folder groups FIRST, then standalone apps preserving existing page order
        all_items.extend(active_folders.iter().cloned());
        seen_items.extend(active_folders.iter().cloned());

        for page in &pages {
            for (id, _) in page {
                if id.ends_with(".desktop")
                    && !apps_in_folders.contains(id)
                    && !favorites.contains(id)
                    && app_map.get(id).map_or(false, |e| e.is_app)
                    && seen_items.insert(id.clone())
                {
                    all_items.push(id.clone());
                }
            }
        }
    } else {
        // Keep folders interleaved in their original relative order among the apps
        for page in &pages {
            for (id, _) in page {
                let keep = active_folder_set.contains(id)
                    || (id.ends_with(".desktop")
                        && !apps_in_folders.contains(id)
                        && !favorites.contains(id)
                        && app_map.get(id).map_or(false, |e| e.is_app));
                if keep && seen_items.insert(id.clone()) {
                    all_items.push(id.clone());
                }
            }
        }

        // Any active folder not found on an existing page (e.g. only in folder_children)
        for f in &active_folders {
            if seen_items.insert(f.clone()) {
                all_items.push(f.clone());
            }
        }
    }

    // Append any installed apps that were missing from pages and not inside folders
    for (id, entry) in &app_map {
        if entry.is_app
            && !apps_in_folders.contains(id)
            && !favorites.contains(id)
            && seen_items.insert(id.clone())
        {
            all_items.push(id.clone());
        }
    }

    let total_items = all_items.len();
    let new_pages = pack_into_pages(&all_items, settings.items_per_page);
    let total_pages = new_pages.len();

    let initial_visible_first_page_len = pages
        .first()
        .map(|p| {
            p.iter()
                .filter(|(id, _)| {
                    !id.ends_with(".desktop")
                        || (!apps_in_folders.contains(id)
                            && !favorites.contains(id)
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

    // 4. Update dconf: sync folder_children and new packed layout
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
    -g, --gradient          Sort apps by color gradient (groups shared-icon apps into folders)
    -r, --random            Sort apps randomly
    -a, --alpha             Sort apps alphabetically by name (A to Z)
    -f, --fill-first        Compact grid: fill every page with zero gaps
    -b, --restore <FILE>    Restore layout and folders from a backup TOML file
    -h, --help              Show this help information
    -v, --version           Show version information

Icon-grouping and group-placement preferences are set from the TUI main menu
and persisted to ~/.config/appmenu-gradient-sort/config.toml; the CLI flags
above honor whatever was last set there.

When run without arguments in a terminal, launches the interactive TUI.
"
    );
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let settings = config::Settings::load();

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
                    sort_gradient_logic(&current_dir, &settings)?;
                println!(
                    "Success! Organized {} apps across {} pages ({} shared-icon groups, sorted by gradient).",
                    total_apps, total_pages, folder_count
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-r" | "--random" => {
                println!("Sorting GNOME app menu randomly...");
                let (backup_path, total_items, total_pages) =
                    sort_random_logic(&current_dir, &settings)?;
                println!(
                    "Success! Randomized {} items across {} pages (zero gaps).",
                    total_items, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-a" | "--alpha" => {
                println!("Sorting GNOME app menu alphabetically...");
                let (backup_path, total_items, total_pages) =
                    sort_alpha_logic(&current_dir, &settings)?;
                println!(
                    "Success! Sorted {} items across {} pages from A to Z (zero gaps).",
                    total_items, total_pages
                );
                println!("Backup saved to: {}", backup_path.display());
                println!("Press Super key twice to view your updated grid.");
                return Ok(());
            }
            "-f" | "--fill-first" => {
                println!("Compacting grid: filling every page with zero gaps...");
                let (backup_path, total_items, total_pages, moved_count, folder_count) =
                    fill_first_page_logic(&current_dir, &settings)?;
                if folder_count > 0 {
                    println!("{} folder group(s) found.", folder_count);
                }
                if moved_count > 0 {
                    println!(
                        "Success! Compacted grid by moving {} items forward (total {} items across {} pages).",
                        moved_count, total_items, total_pages
                    );
                } else {
                    println!(
                        "Grid is already fully packed with zero gaps (total {} items across {} pages).",
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
        let pages = pack_into_pages(&items, 24);
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

        let repacked = pack_into_pages(&all_items, 24);
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
            collect_grid_items(&app_map, &pages, &folder_children, true, &HashSet::new());
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

        let new_pages = pack_into_pages(&all_items, 24);
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

        let new_pages = pack_into_pages(&all_items, 24);

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
