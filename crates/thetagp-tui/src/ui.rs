//! What the terminal shows: the board, its keys and the profiles it carries.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState,
};

use thetagp_client::KeyType;

use crate::app::{App, Pane, Target};
use crate::labels;

const ACCENT: Color = Color::Rgb(90, 156, 255);
const MUTED: Color = Color::Rgb(139, 146, 156);

pub fn draw(frame: &mut Frame, app: &App) {
    let mut constraints = vec![
        Constraint::Length(3), // header
        Constraint::Length(4), // sources
        Constraint::Min(9),    // keys
    ];
    let has_remap = app.remap_len() > 0;
    if has_remap {
        constraints.push(Constraint::Length(10));
    }
    constraints.push(Constraint::Length(5)); // profiles
    constraints.push(Constraint::Length(4)); // status

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    draw_header(frame, app, areas[0]);
    draw_sources(frame, app, areas[1]);
    draw_keys(frame, app, areas[2]);
    let mut at = 3;
    if has_remap {
        draw_remap(frame, app, areas[at]);
        at += 1;
    }
    draw_profiles(frame, app, areas[at]);
    draw_status(frame, app, areas[at + 1]);
}

fn draw_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let named = match &app.identity {
        Some(named) => format!(
            "{} · {} · {} {}",
            named.board, named.version, named.build_date, named.build_time
        ),
        None => "no device".to_string(),
    };
    let source = app.connected.as_deref().unwrap_or("disconnected");
    let line = Line::from(vec![
        Span::styled(" ThetaGP ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {named} "), Style::default().fg(MUTED)),
        Span::styled(format!(" [{source}]"), Style::default().fg(Color::White)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_sources(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let focused = app.pane == Pane::Sources;
    let border = if focused { ACCENT } else { Color::DarkGray };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(" sources ");

    if app.sources.is_empty() {
        frame.render_widget(
            Paragraph::new(" none; no serial port was found")
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .sources
        .iter()
        .map(|name| {
            let style = if app.connected.as_deref() == Some(name.as_str()) {
                Style::default().fg(ACCENT)
            } else {
                Style::default()
            };
            ListItem::new(name.clone()).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.source.min(app.sources.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut state,
    );
}

fn draw_keys(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let focused = app.pane == Pane::Keys;
    let border = if focused { ACCENT } else { Color::DarkGray };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(format!(" keys ({}) ", app.keys.len()));

    let rows: Vec<Row> = app
        .keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let shown = match &app.editing {
                Some(editing) if editing.target == Target::Key(index) => {
                    format!("{}▏", editing.text)
                }
                _ => app.value_display(index),
            };
            let style = if matches!(&app.editing, Some(editing) if editing.target == Target::Key(index)) {
                Style::default().fg(ACCENT)
            } else {
                Style::default()
            };
            let named = match labels::label(&key.name) {
                Some(label) => Line::from(vec![
                    Span::raw(label.to_string()),
                    Span::styled(format!(" ({})", key.name), Style::default().fg(Color::DarkGray)),
                ]),
                None => Line::from(Span::styled(
                    key.name.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
            };
            Row::new(vec![
                Cell::from(named),
                Cell::from(format!(
                    "{:?}{}",
                    key.kind,
                    if matches!(key.kind, KeyType::U8Array) {
                        format!(" · {}", key.count)
                    } else {
                        String::new()
                    }
                ))
                .style(Style::default().fg(MUTED)),
                Cell::from(format!("{}..{}", key.min, key.max)).style(Style::default().fg(MUTED)),
                Cell::from(shown).style(style),
                Cell::from(if key.reboot { "at boot" } else { "" })
                    .style(Style::default().fg(MUTED)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(32),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Min(12),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["name", "shape", "range", "value", ""]).style(Style::default().fg(MUTED)),
    )
    .block(block)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    if !app.keys.is_empty() {
        state.select(Some(app.selected.min(app.keys.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_remap(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let focused = app.pane == Pane::Remap;
    let border = if focused { ACCENT } else { Color::DarkGray };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(" remap (map.btn_map → button index) ");

    let slots = app.remap_len();
    let rows: Vec<Row> = (0..slots)
        .map(|index| {
            // The slot is the board's own input index, as its key map numbers
            // it; the button it feeds is named from the schema's table.
            let from = Line::from(Span::styled(
                format!("input {index:02}"),
                Style::default().fg(MUTED),
            ));
            let shown = match &app.editing {
                Some(editing) if editing.target == Target::Remap(index) => {
                    Line::from(format!("{}▏", editing.text))
                }
                _ => Line::from(labels::slot(
                    app.remap_text(index).parse::<i32>().unwrap_or(0),
                )),
            };
            Row::new(vec![
                Cell::from(from),
                Cell::from("→").style(Style::default().fg(MUTED)),
                Cell::from(shown),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(2),
            Constraint::Min(8),
        ],
    )
    .header(Row::new(vec!["input", "", "feeds"]).style(Style::default().fg(MUTED)))
    .block(block)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    if slots > 0 {
        state.select(Some(app.remap_selected.min(slots - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_profiles(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let focused = app.pane == Pane::Profiles;
    let border = if focused { ACCENT } else { Color::DarkGray };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(" profiles ");

    if app.profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(" none carried; this device keeps no user profiles")
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .profiles
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut spans = vec![Span::styled(format!("#{}", entry.id), Style::default())];
            // The name the profile carries, while it is being typed over.
            let named = match &app.editing {
                Some(editing) if editing.target == Target::ProfileName(index) => {
                    format!("\"{}\"", editing.text)
                }
                _ if !entry.name.is_empty() => format!("\"{}\"", entry.name),
                _ => String::new(),
            };
            if !named.is_empty() {
                spans.push(Span::styled(format!(" {named}"), Style::default().fg(ACCENT)));
            }
            if entry.active {
                spans.push(Span::styled(" · active", Style::default().fg(ACCENT)));
            }
            // The factory profile is the board's own: the store refuses a write
            // to it, so the list says which one it is.
            if entry.id == 0 {
                spans.push(Span::styled(
                    " (read-only)",
                    Style::default().fg(Color::DarkGray),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        app.profile_selected.min(app.profiles.len().saturating_sub(1)),
    ));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut state,
    );
}

fn draw_status(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let message = match &app.error {
        Some(error) => Span::styled(format!(" {error}"), Style::default().fg(Color::Red)),
        // What the device said is the board talking back: it reads in the
        // window's own blue, and a failure in red.
        None => Span::styled(format!(" {}", app.message), Style::default().fg(ACCENT)),
    };
    // The keys are a line a reader scans, and the whole of them is wider than a
    // terminal: they are written on two.
    let help = vec![
        Line::from(Span::styled(
            "  tab pane · enter connect / edit a value / activate a profile · n new profile · e name it · x delete (twice)",
            Style::default(),
        )),
        Line::from(Span::styled(
            "  w save · l load · t snapshot · r restore · d disconnect · q quit",
            Style::default(),
        )),
    ];
    let mut lines = vec![Line::from(message)];
    lines.extend(help);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
        area,
    );
}
