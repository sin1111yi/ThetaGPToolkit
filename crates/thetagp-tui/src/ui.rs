//! What the terminal shows: the board, its keys and the profiles it carries.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, TableState,
};

use thetagp_client::KeyType;

use crate::app::{App, Pane, Target};
use crate::labels;

const ACCENT: Color = Color::Rgb(90, 156, 255);
const MUTED: Color = Color::Rgb(139, 146, 156);
// What a reader checks before acting -- the button a value goes into, the profile
// a change lands in -- is set apart from the blue of what is being written.
pub(crate) const HIGHLIGHT: Color = Color::Rgb(232, 176, 96);

pub fn draw(frame: &mut Frame, app: &App) {
    // The body on the left, the choices the item in focus offers on the right,
    // and the keys on a line under both: the help is as wide as the terminal,
    // which is what keeps it from being cut at the body's edge.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(9), Constraint::Length(4)])
        .split(frame.area());
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(rows[0]);
    draw_body(frame, app, columns[0]);
    draw_options(frame, app, columns[1]);
    // The choices column stays whole: the box a value is typed in sits over the
    // device alone, so what the value may be is read beside it.
    draw_status(frame, app, rows[1]);
    draw_edit(frame, app, columns[0]);
    draw_confirm(frame, app, columns[0]);
}

fn draw_body(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut constraints = vec![
        Constraint::Length(4), // header
        Constraint::Length(4), // sources
        Constraint::Min(9),    // keys
    ];
    let has_remap = app.remap_len() > 0;
    if has_remap {
        // The remap pane is as tall as the inputs this board wires: a board with
        // four does not leave room for four it does not have.
        let height = (app.remap_len() as u16 + 4).min(10);
        constraints.push(Constraint::Length(height));
    }
    // A board with two profiles does not reserve the height of six, and the
    // room goes to the menu above it.
    constraints.push(Constraint::Length(
        (app.profiles.len() as u16 + 2).clamp(3, 6),
    )); // profiles

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
}

/// The value being typed, in a box of its own over the device: a row of the
/// table is too narrow to hold a value and the names it may take beside it.
fn draw_edit(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
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
            // name is what a reader is looking at while choosing its number.
            Some(key) => match &key.value_enum {
                Some(named) => format!("{named} · {}..{}", key.min, key.max),
                None => format!("a number in {}..{}", key.min, key.max),
            },
            None => String::new(),
        },
        Target::Remap(_) => "the button this input feeds".to_string(),
        Target::ProfileName(_) => "the name its body carries".to_string(),
    };

    // Whatever is under the keys wins over the value on the device: it is the
    // one the reader is choosing between.
    let text = editing.text.as_str();
    let typed: Option<i32> = text.trim().parse().ok();
    let held = typed.or(current);

    // A colour is a place on a scale rather than a number to be looked up: the
    // box shows the scale itself, drawn from the rest of the colour the profile
    // carries, and the mark where the value stands on it.
    if let (Target::Key(row), Some(value)) = (editing.target, held) {
        let band = app.keys.get(row).and_then(|key| match key.name.as_str() {
            "led.hue" => Some(Band::Hue),
            "led.sat" => Some(Band::Saturation),
            _ => None,
        });
        if let Some(band) = band {
            draw_scale_box(frame, app, area, band, title, text, value);
            return;
        }
    }

    // The box keeps to the device's column and holds a handful of names at a
    // time: the list scrolls under the value, so a reader picks from the names
    // around the one held rather than reading a wall of them.
    let screen = area;
    let width = screen.width.saturating_sub(2).clamp(24, 46);
    let height = if choices.is_empty() { 6 } else { 12 }.min(screen.height);
    let dialog = dialog_in(screen, width, height);
    frame.render_widget(Clear, dialog);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let head = vec![
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(MUTED)),
            Span::styled(format!("{text}▏"), Style::default().fg(ACCENT)),
        ]),
        Line::from(Span::styled(
            format!("  {hint}"),
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(head), rows[0]);

    if !choices.is_empty() {
        let count = choices.len();
        let visible = usize::from(rows[1].height).max(1);
        let held_at = held.and_then(|held| choices.iter().position(|(_, number)| *number == held));
        // The name held sits in the middle of the window wherever the list
        // allows, so the way up and the way down are both in sight.
        let first = match held_at {
            Some(at) => at
                .saturating_sub(visible / 2)
                .min(count.saturating_sub(visible)),
            None => 0,
        };

        let items: Vec<ListItem> = choices
            .iter()
            .skip(first)
            .take(visible)
            .map(|(name, number)| {
                let held_now = Some(*number) == held;
                let label = format!(
                    " {} {name} ({number})",
                    if held_now { "→" } else { " " }
                );
                ListItem::new(label).style(if held_now {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                })
            })
            .collect();
        frame.render_widget(List::new(items), rows[1]);

        if count > visible {
            let mut bar = ScrollbarState::new(count).position(first);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                rows[1],
                &mut bar,
            );
        }
    }

    let footer = if choices.is_empty() {
        "  enter write · esc cancel"
    } else {
        "  ↑↓ value · enter write · esc cancel"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(MUTED),
        ))),
        rows[2],
    );
}

/// The colour at a hue, on the wheel the strip draws: six sectors of sixty
/// degrees, red at zero, taken down to the saturation the profile carries.
fn hue_colour(degrees: u16, saturation: u8) -> (u8, u8, u8) {
    let turn = u32::from(degrees % 360);
    let ramp = ((turn % 60) * 255 / 60) as u8;
    let (r, g, b) = match turn / 60 {
        0 => (255, ramp, 0),
        1 => (255 - ramp, 255, 0),
        2 => (0, 255, ramp),
        3 => (0, 255 - ramp, 255),
        4 => (ramp, 0, 255),
        _ => (255, 0, 255 - ramp),
    };

    // A saturation below full colour takes every channel towards white.
    let mixed = |channel: u8| {
        ((u32::from(channel) * u32::from(saturation) + 255 * (255 - u32::from(saturation)))
            / 255) as u8
    };
    (mixed(r), mixed(g), mixed(b))
}

/// Which colour key a band is drawn for: the wheel itself, or one hue taken
/// from white to full colour.
#[derive(Clone, Copy)]
enum Band {
    Hue,
    Saturation,
}

impl Band {
    /// The numbers the band spans, both ends included.
    fn range(self) -> (i32, i32) {
        match self {
            Band::Hue => (0, 360),
            Band::Saturation => (0, 255),
        }
    }

    /// What the band is read against: the three colours most readers name, or
    /// the two ends a saturation runs between.
    fn hint(self) -> &'static str {
        match self {
            Band::Hue => "red 0° · green 120° · blue 240°",
            Band::Saturation => "white 0 · the hue at 255",
        }
    }

    /// The colour a point of the band stands for, at the rest of the colour the
    /// profile carries.
    fn colour(self, at: i32, hue: u16, saturation: u8) -> (u8, u8, u8) {
        match self {
            Band::Hue => hue_colour(at as u16, saturation),
            Band::Saturation => hue_colour(hue, at as u8),
        }
    }
}

/// The box a colour is picked in: the scale as a band, the mark where the value
/// stands on it, and the keys that move the mark along.
fn draw_scale_box(
    frame: &mut Frame,
    app: &App,
    screen: ratatui::layout::Rect,
    band: Band,
    title: String,
    text: &str,
    value: i32,
) {
    let hint = band.hint();
    let hue = app.hue_degrees();
    let saturation = app.saturation();
    let dialog = dialog_in(screen, 46, 8);
    frame.render_widget(Clear, dialog);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let cells = usize::from(inner.width.saturating_sub(2)).max(2);
    let last = cells - 1;
    let (min, max) = band.range();
    let value = value.clamp(min, max);

    // One cell per point of the scale, so the band reads as the colour it names.
    let mut cells_line = vec![Span::raw(" ")];
    cells_line.extend((0..cells).map(|at| {
        let at = min + (at as i32 * (max - min) / last as i32);
        let (r, g, b) = band.colour(at, hue, saturation);
        Span::styled(" ", Style::default().bg(Color::Rgb(r, g, b)))
    }));

    // The mark sits on the cell the value names, at the band's own width.
    let at = ((value - min) * last as i32 / (max - min)) as usize;
    let mut mark = vec![Span::raw(" ")];
    mark.extend((0..cells).map(|cell| {
        Span::styled(
            if cell == at { "▲" } else { " " },
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )
    }));

    let lines = vec![
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(MUTED)),
            Span::styled(format!("{text}▏"), Style::default().fg(ACCENT)),
        ]),
        Line::from(Span::styled(format!("  {hint}"), Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(cells_line),
        Line::from(mark),
        Line::from(Span::styled(
            "  ←→ value · enter write · esc cancel",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The pane's title: the menu's top, or the path the reader has gone down.
fn menu_title(app: &App) -> String {
    if app.path.is_empty() {
        return format!(" options ({}) ", app.menu_count());
    }
    format!(" options › {} ", app.path.join(" › "))
}

/// How many keys a segment holds, in words: a segment of one is not a segment
/// of many.
fn key_count(count: usize) -> String {
    format!("{count} key{}", if count == 1 { "" } else { "s" })
}

/// A box over the device's column, centred in it: the panes it is read against
/// keep their own space.
fn dialog_in(area: ratatui::layout::Rect, width: u16, height: u16) -> ratatui::layout::Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    ratatui::layout::Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// The question `x` asks before a profile goes: the board keeps its profiles
/// itself, so what would be lost is named before it is lost.
fn draw_confirm(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(confirm) = &app.confirm else {
        return;
    };

    let dialog = dialog_in(area, 46, 6);
    frame.render_widget(Clear, dialog);

    // The profile that goes is the fact this box exists for, so it carries the
    // colour kept for what is checked before acting.
    let mut going = vec![Span::raw("  ")];
    going.push(Span::styled(
        format!("#{}", confirm.id),
        Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
    ));
    if !confirm.name.is_empty() {
        going.push(Span::styled(
            format!(" \"{}\"", confirm.name),
            Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
        ));
    }

    let lines = vec![
        Line::from(going),
        Line::from(Span::styled(
            "  is deleted from the board",
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  enter delete · esc cancel",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" delete profile? ")),
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
            Span::styled(
                format!("  {editing}"),
                Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
            ),
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
        " choices ".to_string()
    } else {
        format!(" choices · {key} ")
    };
    let block = Block::default().borders(Borders::ALL).title(title);

    if choices.is_empty() {
        // A domain line holds keys rather than a value, and what the reader can
        // do with it is the fold: the hint says so where the choices would be.
        let hint = match app.pane {
            Pane::Keys => match app.rows().get(app.selected) {
                Some(crate::app::Row::Group { keys, .. }) => {
                    format!(" {} · enter goes in", key_count(*keys))
                }
                Some(crate::app::Row::Key(row)) => match app.keys.get(*row) {
                    Some(key) => format!(" a number in {}..{}", key.min, key.max),
                    None => " nothing to choose here".to_string(),
                },
                None => " nothing to choose here".to_string(),
            },
            Pane::Remap => match app.remap_key() {
                Some(key) => format!(" a number in {}..{}", key.min, key.max),
                None => " nothing to choose here".to_string(),
            },
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
        .title(menu_title(app));

    let rows: Vec<Row> = app
        .rows()
        .iter()
        .map(|line| match line {
            // A segment is a line of its own, and the way into the level under
            // it: the arrow says there is more below, the count says how much.
            crate::app::Row::Group { segment, keys } => Row::new(vec![Cell::from(
                Line::from(vec![
                    Span::styled(
                        format!("▸ {segment}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {}", key_count(*keys)), Style::default().fg(MUTED)),
                ]),
            )]),
            crate::app::Row::Key(index) => {
                let key = &app.keys[*index];
                key_row(app, key, *index)
            }
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
        state.select(Some(app.selected.min(app.rows().len().saturating_sub(1))));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

/// A key's line: the name a reader knows it by, and the name the wire uses.
fn key_row(app: &App, key: &thetagp_client::Key, index: usize) -> Row<'static> {
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
                spans.push(Span::styled(
                    " · active",
                    Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
                ));
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
            "  tab pane · ←→ out of / into a level · enter connect / open / edit / activate · x delete",
            Style::default(),
        )),
        Line::from(Span::styled(
            "  n new profile · e name it · w save · l load · t snapshot · r restore · d disconnect · q quit",
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
