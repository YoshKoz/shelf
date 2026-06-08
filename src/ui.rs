use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};

use crate::app::{App, FormField, Mode};
use crate::models::ItemType;

// ── Palette ──────────────────────────────────────────────────────────────────
const ACCENT: Color = Color::Cyan;
const WARN: Color = Color::Yellow;
const DANGER: Color = Color::Red;
const DIM: Color = Color::DarkGray;

// ── Top-level render ─────────────────────────────────────────────────────────

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: header / main / status
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, vert[0], app);
    render_main(frame, vert[1], app);
    render_status(frame, vert[2], app);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let search_visible = app.mode == Mode::Search;

    let title = if search_visible {
        " Search:".to_string()
    } else {
        " 📚 shelf".to_string()
    };

    let search_text: String = if search_visible {
        app.search_query.clone()
    } else {
        String::new()
    };

    let cursor = if search_visible { "█" } else { "" };

    // Combine: " Search: <query>█          [N]ew  [/]Find  [Q]uit"
    let left_len = title.len() + search_text.len() + cursor.len();
    let hints = "  [N]ew  [/]Find  [t:tag]  [↵/O]pen  [Y]ank  [E]dit  [D]el  [Q]uit";
    // Pad so hints are right-aligned
    let pad = if left_len + hints.len() + 2 < area.width as usize {
        area.width as usize - left_len - hints.len()
    } else {
        1
    };

    let line = format!(
        "{}{}{}{:>pad$}",
        title, search_text, cursor, hints, pad = pad,
    );

    let header_style = Style::default()
        .fg(Color::White)
        .bg(Color::Reset);

    let p = Paragraph::new(line).style(header_style);
    frame.render_widget(p, area);

    // Draw a subtle separator
    let sep_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let sep = Paragraph::new(
        "─".repeat(area.width as usize),
    )
    .style(Style::default().fg(DIM));
    frame.render_widget(sep, sep_area);
}

// ── Main area ──────────────────────────────────────────────────────────────

fn render_main(frame: &mut Frame, area: Rect, app: &App) {
    if area.width < 40 || area.height < 5 {
        let p = Paragraph::new("Terminal too small").style(Style::default().fg(WARN));
        frame.render_widget(p, area);
        return;
    }

    // Split: list (left) | detail (right)
    let split = if area.width >= 100 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)])
            .split(area)
    } else {
        // Stack vertically
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(area)
    };

    render_list(frame, split[0], app);

    if app.mode == Mode::Normal || app.mode == Mode::Search {
        render_detail(frame, split[1], app);
    }

    // Overlays
    match &app.mode {
        Mode::Create | Mode::Edit => {
            if let Some(form) = &app.form {
                render_form_overlay(frame, area, form, app.mode == Mode::Edit);
            }
        }
        Mode::ConfirmDelete => {
            render_delete_overlay(frame, area, app);
        }
        _ => {}
    }
}

// ── List panel ─────────────────────────────────────────────────────────────

fn render_list(frame: &mut Frame, area: Rect, app: &App) {
    let items = &app.items;

    // Compute visible range
    let list_height = area.height.saturating_sub(2) as usize; // header + margin
    let selected = app.selected;
    let scroll = if selected >= list_height {
        selected - list_height + 1
    } else {
        0
    };

    // Build rows
    let mut rows: Vec<Row> = Vec::with_capacity(items.len().min(list_height + 1));

    let type_hint = |t: &ItemType| match t {
        ItemType::Bookmark => "🔗",
        ItemType::Note => "📝",
    };

    for (i, item) in items.iter().enumerate().skip(scroll).take(list_height) {
        let is_selected = i == selected;
        let style = if is_selected {
            Style::default().fg(ACCENT).bg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let indicator = if is_selected { "▶" } else { " " };

        let title = if item.title.len() > 30 {
            format!("{}…", &item.title[..29])
        } else {
            item.title.clone()
        };

        let tags: String = if item.tags.is_empty() {
            String::new()
        } else {
            let t: Vec<&str> = item.tags.iter().map(|s| s.as_str()).collect();
            t.join(", ")
        };
        let tags = if tags.len() > 18 {
            format!("{}…", &tags[..17])
        } else {
            tags
        };

        let created = item.created.format("%Y-%m-%d").to_string();

        let cells = vec![
            Cell::from(indicator).style(style),
            Cell::from(type_hint(&item.item_type)).style(style),
            Cell::from(title).style(style),
            Cell::from(tags).style(Style::default().fg(DIM)),
            Cell::from(created).style(Style::default().fg(DIM)),
        ];
        rows.push(Row::new(cells).height(1));
    }

    // Header row
    let header_cells = vec!["", " ", "Title", "Tags", "Date"]
        .iter()
        .map(|s| Cell::from(*s).style(Style::default().fg(DIM).add_modifier(Modifier::DIM)))
        .collect::<Vec<_>>();
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Black))
        .height(1);

    let widths = [
        Constraint::Length(2),   // indicator
        Constraint::Length(2),   // type icon
        Constraint::Ratio(2, 4), // title
        Constraint::Ratio(1, 4), // tags
        Constraint::Ratio(1, 4), // date
    ];

    let list = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .highlight_style(Style::default().fg(ACCENT));
    let placeholder = if items.is_empty() {
        "  No items yet. Press N to create one."
    } else {
        ""
    };

    frame.render_widget(list, area);

    if !placeholder.is_empty() {
        let p = Paragraph::new(placeholder)
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        frame.render_widget(p, area);
    }
}

// ── Detail panel ───────────────────────────────────────────────────────────

fn render_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(item) = app.selected_item() else {
        let p = Paragraph::new("No item selected")
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center);
        frame.render_widget(p, area);
        return;
    };

    // Type badge
    let type_badge = match &item.item_type {
        ItemType::Bookmark => " BOOKMARK ",
        ItemType::Note => " NOTE ",
    };

    let badge_style = match &item.item_type {
        ItemType::Bookmark => Style::default().fg(Color::Black).bg(Color::Cyan),
        ItemType::Note => Style::default().fg(Color::Black).bg(Color::Green),
    };

    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(Span::styled(
        &item.title,
        Style::default().add_modifier(Modifier::BOLD).fg(Color::White),
    )));

    // Badge
    lines.push(Line::from(Span::styled(type_badge, badge_style)));

    // Tags
    if !item.tags.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("tags: {}", item.tags.join(", ")),
            Style::default().fg(DIM),
        )));
    }

    // URL
    if let Some(url) = &item.url {
        lines.push(Line::from(Span::styled(
            format!("url:  {}", url),
            Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
        )));
    }

    // Dates
    lines.push(Line::from(Span::styled(
        format!("created: {}  modified: {}", item.created, item.modified),
        Style::default().fg(DIM),
    )));

    // Separator
    let sep: String = "─".repeat(area.width.saturating_sub(2) as usize);
    lines.push(Line::from(Span::styled(sep, Style::default().fg(DIM))));

    // Content (with basic scrolling)
    let content = if item.content.is_empty() {
        "(no content)"
    } else {
        &item.content
    };

    // Compute visible lines for content
    let used_lines = lines.len() + 1; // +1 for spacing after separator
    let available = area.height.saturating_sub(used_lines as u16).saturating_sub(1);
    let scroll = app.detail_scroll;

    for line in content.lines().skip(scroll).take(available as usize) {
        // Truncate long lines
        let max_w = area.width.saturating_sub(2) as usize;
        let text = if line.len() > max_w {
            format!("{}…", &line[..max_w.saturating_sub(1)])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::raw(text)));
    }

    // "more" indicator
    let total_lines = content.lines().count();
    if total_lines > scroll + available as usize {
        lines.push(Line::from(Span::styled("… more …", Style::default().fg(DIM))));
    }

    let p = Paragraph::new(lines).style(Style::default().bg(Color::Reset));
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(DIM));
    frame.render_widget(p.block(block), area);
}

// ── Form overlay (create / edit) ──────────────────────────────────────────

fn render_form_overlay(frame: &mut Frame, area: Rect, form: &crate::app::Form, is_edit: bool) {
    let title = if is_edit { " Edit Item " } else { " New Item " };

    // Draw a centered popup
    let popup_w = area.width.min(60).saturating_sub(4);
    let popup_h = 16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    // Dim background
    let overlay = Block::default()
        .style(Style::default().bg(Color::Black).add_modifier(Modifier::DIM));
    frame.render_widget(overlay, area);

    // Popup block
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title)
        .title_alignment(Alignment::Center);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Form fields
    let fields = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // URL
            Constraint::Length(3), // Tags
            Constraint::Length(3), // Type + submit hint
            Constraint::Min(3),    // Content (multiline-ish)
        ])
        .margin(1)
        .split(inner);

    let labels = ["Title:", "URL:", "Tags:", "Type:", "Content:"];
    let values = [
        &form.title,
        &form.url,
        &form.tags,
        "", // type toggle
        &form.content,
    ];
    let field_list = [
        FormField::Title,
        FormField::Url,
        FormField::Tags,
        FormField::TypeToggle,
        FormField::Content,
    ];

    for (i, field) in field_list.iter().enumerate() {
        let is_focused = form.field == *field;
        let fg = if is_focused { ACCENT } else { Color::White };

        let value_str = if *field == FormField::TypeToggle {
            match form.item_type {
                ItemType::Bookmark => "BOOKMARK (press Tab/T to toggle)".to_string(),
                ItemType::Note => "NOTE (press Tab/T to toggle)".to_string(),
            }
        } else {
            values[i].to_string()
        };

        let line = Line::from(vec![
            Span::styled(
                format!("{} ", labels[i]),
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                value_str,
                if is_focused && *field != FormField::TypeToggle && *field != FormField::Content {
                    Style::default().fg(Color::White).bg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]);

        let area = if i < fields.len() { fields[i] } else { inner };
        frame.render_widget(Paragraph::new(line), area);
    }

    // Hint at bottom
    let hint = Line::from(Span::styled(
        "Ctrl+S save  Esc cancel  Tab/Shift+Tab next/prev  T toggle type",
        Style::default().fg(DIM),
    ));
    // Place hint above the border
    if popup_h > 2 {
        let hint_area = Rect::new(popup.x + 1, popup.y + popup_h - 2, popup_w.saturating_sub(2), 1);
        frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), hint_area);
    }
}

// ── Delete confirmation overlay ───────────────────────────────────────────

fn render_delete_overlay(frame: &mut Frame, area: Rect, app: &App) {
    let Some(item) = app.selected_item() else {
        return;
    };

    let popup_w = area.width.min(50).saturating_sub(4);
    let popup_h = 5;
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    let overlay = Block::default()
        .style(Style::default().bg(Color::Black).add_modifier(Modifier::DIM));
    frame.render_widget(overlay, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DANGER))
        .title(" Delete ")
        .title_alignment(Alignment::Center);
    let inner = block.inner(popup);

    let text = vec![
        Line::from(Span::styled(
            format!("Delete \"{}\"?", item.title),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "Press Y to confirm, any other key to cancel",
            Style::default().fg(DIM),
        )),
    ];

    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
    frame.render_widget(block, popup);
}

// ── Status bar ─────────────────────────────────────────────────────────────

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let text = if !app.status.is_empty() {
        app.status.clone()
    } else {
        format!("{} items", app.items.len())
    };

    let style = Style::default()
        .fg(Color::Black)
        .bg(if app.status.contains("Error") {
            DANGER
        } else if app.status.contains("Created") || app.status.contains("Updated") {
            Color::Green
        } else {
            Color::White
        });

    let p = Paragraph::new(text)
        .style(style)
        .alignment(Alignment::Left);
    frame.render_widget(p, area);
}
