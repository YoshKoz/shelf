mod app;
mod db;
mod models;
mod storage;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{App, FormField, Mode};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn xdg_data_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.data_local_dir().join("shelf"))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local/share/shelf")
        })
}

fn xdg_config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_local_dir().join("shelf"))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config/shelf")
        })
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let data_dir = xdg_data_dir();
    let config_dir = xdg_config_dir();
    std::fs::create_dir_all(&data_dir).context("creating data directory")?;
    std::fs::create_dir_all(&config_dir).context("creating config directory")?;
    let db_path = data_dir.join("shelf.db");

    let app = App::new(db_path, data_dir)?;

    // Terminal setup
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let res = run(&mut terminal, app);

    // Teardown
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    terminal.show_cursor()?;

    res?;
    Ok(())
}

// ── Event loop ──────────────────────────────────────────────────────────────

fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, mut app: App) -> anyhow::Result<()> {
    // Show initial status
    app.status = format!("{} items. Type / to search, N to add.", app.items.len());

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if app.should_quit {
            break;
        }

        // Poll for events (~100ms timeout for responsive Ctrl+C)
        if !event::poll(Duration::from_millis(100))? {
            // Tick – rebuild index if needed
            if app.needs_rebuild {
                app.rebuild_index()?;
                app.refresh_items()?;
            }
            continue;
        }

        let ev = event::read()?;
        match ev {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                handle_key(&mut app, key)?;
            }
            Event::Resize(_, _) => {
                // ratatui handles resize on next draw
            }
            _ => {}
        }
    }

    Ok(())
}

// ── Input dispatch ─────────────────────────────────────────────────────────

fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // Global Ctrl+C / Ctrl+Q
    if key.modifiers == KeyModifiers::CONTROL {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => {
                app.should_quit = true;
                return Ok(());
            }
            _ => {}
        }
    }

    match app.mode {
        Mode::Normal => handle_normal(app, key),
        Mode::Search => handle_search(app, key),
        Mode::Create | Mode::Edit => handle_form(app, key),
        Mode::ConfirmDelete => handle_delete_confirm(app, key),
    }
}

// ── Normal mode ────────────────────────────────────────────────────────────

fn handle_normal(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // Clear transient status messages
    app.status.clear();

    match key.code {
        // Quit
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        // Search
        KeyCode::Char('/') => {
            app.start_search();
        }
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            app.next_item();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.prev_item();
        }
        KeyCode::Char('g') => {
            app.selected = 0;
            app.detail_scroll = 0;
        }
        KeyCode::Char('G') => {
            if !app.items.is_empty() {
                app.selected = app.items.len() - 1;
                app.detail_scroll = 0;
            }
        }
        // Open URL
        KeyCode::Char('o') | KeyCode::Enter => {
            app.open_url();
        }
        // Yank URL to clipboard
        KeyCode::Char('y') => {
            app.yank_url();
        }
        // Create
        KeyCode::Char('n') => {
            app.start_create();
        }
        // Edit
        KeyCode::Char('e') => {
            app.start_edit()?;
        }
        // Delete
        KeyCode::Char('d') => {
            app.start_delete();
        }
        // Scrolling
        KeyCode::Char('u') => {
            app.detail_scroll = app.detail_scroll.saturating_sub(5);
        }
        KeyCode::Char('D') | KeyCode::PageDown => {
            app.detail_scroll = app.detail_scroll.saturating_add(5);
        }
        _ => {}
    }
    Ok(())
}

// ── Search mode ────────────────────────────────────────────────────────────

fn handle_search(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.clear_search();
        }
        KeyCode::Enter => {
            app.commit_search();
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            // Live-filter as user types
            app.refresh_items()?;
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.refresh_items()?;
        }
        _ => {}
    }
    Ok(())
}

// ── Form mode (create / edit) ─────────────────────────────────────────────

fn handle_form(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    let Some(ref mut form) = app.form else {
        app.mode = Mode::Normal;
        return Ok(());
    };

    // Check for Ctrl shortcuts on non-TypeToggle fields
    if key.modifiers == KeyModifiers::CONTROL {
        match key.code {
            KeyCode::Char('s') => {
                app.save_form()?;
                return Ok(());
            }
            _ => {}
        }
    }

    // Tab / Shift+Tab for field navigation (even on TypeToggle)
    match key.code {
        KeyCode::Tab => {
            form.field = form.field.next();
            return Ok(());
        }
        KeyCode::BackTab => {
            form.field = form.field.prev();
            return Ok(());
        }
        _ => {}
    }

    // Esc always cancels
    if key.code == KeyCode::Esc {
        app.cancel_form();
        return Ok(());
    }

    // Delegate to the focused field
    match form.field {
        FormField::TypeToggle => {
            // Toggle between Bookmark and Note
            form.item_type = match form.item_type {
                app::ItemType::Bookmark => app::ItemType::Note,
                app::ItemType::Note => app::ItemType::Bookmark,
            };
            // On Enter, treat as submit
            if key.code == KeyCode::Enter {
                app.save_form()?;
            }
        }
        FormField::Content => {
            match key.code {
                KeyCode::Enter => {
                    form.content.push('\n');
                }
                KeyCode::Backspace => {
                    form.content.pop();
                }
                KeyCode::Char(c) => {
                    form.content.push(c);
                }
                _ => {}
            }
        }
        _ => {
            // Text fields
            let field_str = match form.field {
                FormField::Title => &mut form.title,
                FormField::Url => &mut form.url,
                FormField::Tags => &mut form.tags,
                _ => unreachable!(),
            };
            match key.code {
                KeyCode::Backspace => {
                    field_str.pop();
                }
                KeyCode::Enter => {
                    // Move to next field, or save on Content
                    let next = form.field.next();
                    if next == FormField::Content {
                        app.save_form()?;
                    } else {
                        form.field = next;
                    }
                }
                KeyCode::Char(c) => {
                    field_str.push(c);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ── Delete confirmation ───────────────────────────────────────────────────

fn handle_delete_confirm(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.confirm_delete()?;
        }
        _ => {
            app.cancel_delete();
        }
    }
    Ok(())
}
