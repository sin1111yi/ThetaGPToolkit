//! What the terminal shows: the board, its keys and the profiles it carries.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
};

use thetagp_client::KeyType;

use crate::app::{App, Pane, Target};
use crate::labels;

const ACCENT: Color = Color::Rgb(90, 156, 255);
const MUTED: Color = Color::Rgb(139, 146, 156);

pub fn draw(frame: &mut Frame, app: &App) {
    // The body on the left, and the choices the item in focus offers on the
    // right: a value's names are the schema's, and reading them beside the
    // field is what saves a lookup.
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(frame.area());
    draw_body(frame, app, columns[0]);
    draw_options(frame, app, columns[1]);
    draw_edit(frame, app);
}

fn draw_body(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut constraints = vec![
        Constraint::Length(4), // header
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
        .split(area);

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

/// The value being typed, in a box of its own over the device: a row of the
/// table is too narrow to hold a value and the names it may take beside it.
fn draw_edit(frame: &mut Frame, app: &App) {
    let Some(editing) = &app.editing else {
        return;
    };

    let (key, choices, current) = app.options();
    let title = match editing.target {
        Target::Key(_) => format!(" edit · {key} "),
        Target::Remap(slot) => format!(" edit · input {slot:02} "),
        Target::ProfileName(row) => format!(
            " edit · profile {} ",
            app.profiles.get(row).map(|entry| entry.id).unwrap_or(0)
        ),
    };
    let hint = match editing.target {
        Target::Key(row) => match app.keys.get(row) {
            // A value drawn from the schema's enum is read by name, and the
            // name is what a reader is looking at while typing its number.
            Some(key) => match &key.value_enum {
                Some(named) => format!("{named}: a number in {}..{}", key.min, key.max),
                None => format!("a number in {}..{}", key.min, key.max),
            },
            None => String::new(),
        },
        Target::Remap(_) => "the button this input feeds".to_string(),
        Target::ProfileName(_) => "the name its body carries".to_string(),
    };

    // Whatever is being typed wins over the value on the device: it is the one
    // the reader is choosing between.
    let typed: Option<i32> = editing.text.trim().parse().ok();
    let held = typed.or(current);

    let screen = frame.area();
    let width = ((u32::from(screen.width) * 2 / 3) as u16)
        .max(44)
        .min(screen.width);
    let inner = usize::from(width.saturating_sub(2));

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{}\u{258F}", editing.text),
                Style::default().fg(ACCENT),
            ),
        ]),
        Line::from(Span::styled(format!("  {hint}"), Style::default().fg(MUTED))),
        Line::from(""),
    ];

    // The choices are packed into a run of rows rather than one per line: the
    // whole set at once is what makes a number pickable.
    let mut rows: Vec<Vec<(String, bool)>> = Vec::new();
    let mut row: Vec<(String, bool)> = Vec::new();
    let mut used = 0;
    for (name, number) in &choices {
        // The one held is marked in the run itself, so a monochrome terminal
        // reads it too.
        let label = format!(
            "{}{name} ({number})",
            if Some(*number) == held { "\u{2192}" } else { " " }
        );
        let length = label.chars().count();
        if !row.is_empty() && used + 2 + length > inner.saturating_sub(3) {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        used += if row.is_empty() { length } else { 2 + length };
        row.push((label, Some(*number) == held));
    }
    if !row.is_empty() {
        rows.push(row);
    }

    let mut choice_lines = Vec::new();
    for row in &rows {
        let mut spans = vec![Span::raw("  ")];
        for (at, (label, held_now)) in row.iter().enumerate() {
            if at > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                label.clone(),
                if *held_now {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ));
        }
        choice_lines.push(Line::from(spans));
    }
    // A set wider than the box is cut rather than scrolled: the options column
    // still carries all of it, one row each.
    let room = usize::from(screen.height.saturating_sub(2)).saturating_sub(5);
    lines.extend(choice_lines.into_iter().take(room.max(1)));
    lines.push(Line::from(Span::styled(
        "  enter write · esc cancel",
        Style::default().fg(MUTED),
    )));

    let height = (lines.len() as u16 + 2).min(screen.height);
    let dialog = ratatui::layout::Rect {
        x: screen.x + (screen.width.saturating_sub(width)) / 2,
        y: screen.y + (screen.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        dialog,
    );
}

fn draw_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let named = match &app.identity {
        // The time of the build is the device's too, and the date is what tells
        // one build from another at a glance: the hour is left to the tools.
        Some(named) => format!("{} · {} · {}", named.board, named.version, named.build_date),
        None => "no device".to_string(),
    };
    let source = app.connected.as_deref().unwrap_or("disconnected");

    // The profile a change is written into when it is saved: the one the device
    // runs from, named as its body carries it.
    let editing = app
        .profiles
        .iter()
        .find(|entry| entry.active)
        .map(|entry| {
            let name = if entry.name.is_empty() {
                String::new()
            } else {
                format!(" \"{}\"", entry.name)
            };
            format!("editing #{}", entry.id) + &name
        })
        .unwrap_or_else(|| "no profile".to_string());

    let lines = vec![
        Line::from(vec![
            Span::styled(" ThetaGP ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {named}"), Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled(format!(" [{source}]"), Style::default()),
            Span::styled(format!("  {editing}"), Style::default().fg(ACCENT)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

/// The values the item in focus can take, the one it holds marked.
fn draw_options(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let (key, choices, current) = app.options();
    let title = if key.is_empty() {
        " options ".to_string()
    } else {
        format!(" options · {key} ")
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    if choices.is_empty() {
        let hint = match app.keys.get(app.selected) {
            Some(key) if app.pane == Pane::Keys || app.pane == Pane::Remap => format!(
                " a number in {}..{}",
                key.min, key.max
            ),
            _ => " nothing to choose here".to_string(),
        };
        frame.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(MUTED))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = choices
        .iter()
        .map(|(name, number)| {
            let held = Some(*number) == current;
            let label = format!("{}{} ({})", if held { "→ " } else { "  " }, name, number);
            ListItem::new(label).style(if held {
                Style::default().fg(ACCENT)
            } else {
                Style::default()
            })
        })
        .collect();
    frame.render_widget(List::new(items).block(block), area);
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
