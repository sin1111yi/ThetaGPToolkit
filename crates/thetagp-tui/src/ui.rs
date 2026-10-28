//! What the terminal shows: the board, its keys and the profiles it carries.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine, Points};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
};

use thetagp_client::KeyType;

use crate::app::{App, Pane, Prompt, Target};
use crate::labels;

// The palette, one meaning each: blue is what is being written, amber is what a
// reader checks before acting, mint is the key to press, grey is what is only
// there to be read. The three colours sit in a soft triad, each a shade off the
// terminal's own hues so none of them shouts over the text they mark.
pub(crate) const ACCENT: Color = Color::Rgb(90, 156, 255);
pub(crate) const HIGHLIGHT: Color = Color::Rgb(232, 176, 96);
pub(crate) const KEY: Color = Color::Rgb(122, 208, 180);
pub(crate) const MUTED: Color = Color::Rgb(139, 146, 156);

pub fn draw(frame: &mut Frame, app: &App) {
    // The device on the left, what the line in focus means in a narrow column
    // beside it, and what the screen has said in the room the choices used to
    // take; the keys run under all three, as wide as the terminal.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(9), Constraint::Length(4)])
        .split(frame.area());
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(rows[0]);
    // The column beside the device is answered in two: what the line in focus
    // means holds the top third, and what the screen has said fills the rest.
    let aside = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)])
        .split(columns[1]);
    if app.show_input {
        // The live view takes the whole of the device's side, and the remap
        // table moves to the column beside it: what the inputs are wired to is
        // what a reader checks a press against.
        draw_input(frame, app, columns[0]);
        draw_remap(frame, app, aside[0]);
    } else {
        draw_body(frame, app, columns[0]);
        draw_about(frame, app, aside[0]);
    }
    draw_log(frame, app, aside[1]);
    draw_status(frame, app, rows[1]);
    // A box sits over the device alone: what it is writing is read against the
    // panes, never against the description of it.
    draw_edit(frame, app, columns[0]);
    draw_prompt(frame, app, columns[0]);
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

/// The live input view: the keys the host reads as held and where every axis
/// stands, taken from the driver beside the command channel once per turn.
fn draw_input(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default().borders(Borders::ALL).title(" input ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(state) = &app.input else {
        frame.render_widget(
            Paragraph::new(Line::from("the daemon has not answered yet")),
            inner,
        );
        return;
    };

    if !state.get("available").and_then(|v| v.as_bool()).unwrap_or(false) {
        let reason = state
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("no input node");
        frame.render_widget(Paragraph::new(Line::from(reason.to_string())), inner);
        return;
    }

    // The keys the host reads as held, named the way the profile names the
    // button each stands for: the kernel's own names stop at the pad's fifteenth
    // button, and the rest of a report would read as a bucket's ordinal.
    let keys: Vec<String> = state
        .get("keys")
        .and_then(|v| v.as_array())
        .map(|held| {
            held.iter()
                .map(|key| {
                    let code = key.get("code").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                    if let Some(index) = thetagp_client::input::pad_button_index(code) {
                        if let Some(name) = labels::button(index as i32) {
                            return name;
                        }
                    }
                    match key.get("name").and_then(|v| v.as_str()) {
                        Some(name) => name.to_string(),
                        None => format!("0x{code:03x}"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let mut lines = vec![Line::from(vec![
        Span::styled("held  ", Style::default().fg(MUTED)),
        Span::styled(
            if keys.is_empty() {
                "no key".to_string()
            } else {
                keys.join("  ")
            },
            Style::default().fg(if keys.is_empty() { MUTED } else { KEY }),
        ),
    ])];

    // The buttons a report can carry, as a pad's face: one lit for each the host
    // reads as held, so a press is found by the button it is rather than by the
    // order its name came in.
    let held_buttons: Vec<u32> = state
        .get("keys")
        .and_then(|keys| keys.as_array())
        .map(|keys| {
            keys.iter()
                .filter_map(|key| key.get("code").and_then(|code| code.as_u64()))
                .filter_map(|code| thetagp_client::input::pad_button_index(code as u16))
                .collect()
        })
        .unwrap_or_default();

    let mut row: Vec<Span> = Vec::new();
    let mut cells = 0;
    for index in 0..32i32 {
        let Some(name) = labels::button(index) else {
            continue;
        };
        let lit = held_buttons.contains(&(index as u32));
        let style = if lit {
            Style::default().fg(KEY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        row.push(Span::styled(format!("{name:<5}"), style));
        row.push(Span::raw(" "));
        cells += 1;
        if cells % 12 == 0 {
            lines.push(Line::from(std::mem::take(&mut row)));
        }
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }
    lines.push(Line::from(""));

    // A direction is not a value along a range: the hat's two axes are read as
    // the direction they name, and the bars are left to the analog channels.
    let mut hat = (0i64, 0i64);
    if let Some(axes) = state.get("axes").and_then(|v| v.as_array()) {
        for axis in axes {
            let code = axis.get("code").and_then(|v| v.as_u64()).unwrap_or(0);
            let value = axis.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
            match code {
                0x10 => hat.0 = value,
                0x11 => hat.1 = value,
                _ => {}
            }
        }
    }
    let direction = match (hat.0.signum(), hat.1.signum()) {
        (0, -1) => "↑",
        (0, 1) => "↓",
        (-1, 0) => "←",
        (1, 0) => "→",
        (-1, -1) => "↖",
        (1, -1) => "↗",
        (-1, 1) => "↙",
        (1, 1) => "↘",
        _ => "·",
    };
    lines.push(Line::from(vec![
        Span::styled("dpad  ", Style::default().fg(MUTED)),
        Span::styled(
            direction.to_string(),
            Style::default().fg(if direction == "·" { MUTED } else { KEY }),
        ),
    ]));
    lines.push(Line::from(""));

    // A stick is a place on a plane, not two numbers: the pairs a pad carries
    // are drawn as one circle each, and the bars are left to the axes that
    // stand alone.
    let pairs = [
        ("left", 0x00u16, 0x01u16),
        ("right", 0x03u16, 0x04u16),
    ];
    let axis_of = |code: u16| -> Option<(i64, i64)> {
        state
            .get("axes")
            .and_then(|axes| axes.as_array())
            .and_then(|axes| {
                axes.iter().find(|axis| {
                    axis.get("code").and_then(|v| v.as_u64()).unwrap_or(0xffff) as u16 == code
                })
            })
            .map(|axis| {
                (
                    axis.get("value").and_then(|v| v.as_i64()).unwrap_or(0),
                    axis.get("position").and_then(|v| v.as_i64()).unwrap_or(0),
                )
            })
    };

    // Two sticks are what a pad carries: four axes and no more. A wheel carries
    // its own five, and its steering and throttle are not a stick.
    let plain: Vec<u16> = state
        .get("axes")
        .and_then(|axes| axes.as_array())
        .map(|axes| {
            axes.iter()
                .filter_map(|axis| axis.get("code").and_then(|code| code.as_u64()))
                .map(|code| code as u16)
                .filter(|code| !thetagp_client::input::is_hat(*code))
                .collect()
        })
        .unwrap_or_default();
    let is_pad = plain.len() == 4
        && [0x00u16, 0x01, 0x03, 0x04].iter().all(|code| plain.contains(code));

    let mut paired = Vec::new();
    if is_pad {
        for (name, horizontal, vertical) in pairs {
            if let (Some((_, hp)), Some((_, vp))) = (axis_of(horizontal), axis_of(vertical)) {
                paired.push((name, hp, vp));
            }
        }
    }

    let paired_codes = [0x00u16, 0x01, 0x03, 0x04];
    let head_rows = lines.len() as u16;
    let stick_rows = if paired.is_empty() { 0 } else { STICK_ROWS as u16 + 1 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(head_rows),
            Constraint::Length(stick_rows),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(Paragraph::new(std::mem::take(&mut lines)), rows[0]);
    if !paired.is_empty() {
        draw_sticks(frame, &paired, rows[1]);
    }
    let mut bars: Vec<Line> = Vec::new();

    if let Some(axes) = state.get("axes").and_then(|v| v.as_array()) {
        for axis in axes {
            let code = axis.get("code").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            if thetagp_client::input::is_hat(code) {
                continue;
            }
            if !paired.is_empty() && paired_codes.contains(&code) {
                continue;
            }
            let name = axis.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let position = axis.get("position").and_then(|v| v.as_i64()).unwrap_or(0);
            let value = axis.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
            // The travel the axis is drawn against, as wide as the pane
            // leaves beside its name and its numbers.
            let travel = (inner.width as i32 - 30).clamp(10, 80) as usize;
            let filled = ((position.clamp(0, 1000) * travel as i64) / 1000) as usize;
            let bar: String = (0..travel)
                .map(|at| if at < filled { '=' } else { '.' })
                .collect();
            bars.push(Line::from(vec![
                Span::styled(format!("{name:<9} "), Style::default().fg(MUTED)),
                Span::styled(bar, Style::default().fg(ACCENT)),
                Span::styled(
                    // The value the axis reads, and how far along its range
                    // that is: the bar counts to a hundred, not to a thousand.
                    format!(" {value:>6}  {:>3}%", position / 10),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(bars), rows[2]);
}

/// The sticks as the places they stand on: a circle drawn in a canvas whose
/// dots are square, one per half a column and a quarter of a row, with a mark
/// where the pair points.
fn draw_sticks(frame: &mut Frame, paired: &[(&str, i64, i64)], area: ratatui::layout::Rect) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(STICK_COLS as u16),
            Constraint::Length(2),
            Constraint::Length(STICK_COLS as u16),
            Constraint::Min(0),
        ])
        .split(area);

    for (index, (name, horizontal, vertical)) in paired.iter().enumerate() {
        let at = if index == 0 { halves[0] } else { halves[2] };
        let field = ratatui::layout::Rect {
            height: STICK_ROWS as u16,
            ..at
        };

        // A braille cell is two dots wide and four tall, so a square in dots is
        // two columns wide for every row it is tall: the bounds below are that
        // square, and the circle is round within it.
        let half_width = STICK_COLS as f64;
        let half_height = (STICK_ROWS as f64) * 2.0;
        let radius = half_height - 2.0;
        let mark_x = ((*horizontal).clamp(0, 1000) as f64 / 500.0 - 1.0) * (radius - 1.0);
        let mark_y = -((*vertical).clamp(0, 1000) as f64 / 500.0 - 1.0) * (radius - 1.0);

        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-half_width, half_width])
            .y_bounds([-half_height, half_height])
            .paint(|ctx| {
                ctx.draw(&Circle {
                    x: 0.0,
                    y: 0.0,
                    radius,
                    color: MUTED,
                });
                ctx.draw(&CanvasLine {
                    x1: -radius,
                    y1: 0.0,
                    x2: radius,
                    y2: 0.0,
                    color: Color::DarkGray,
                });
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: -radius,
                    x2: 0.0,
                    y2: radius,
                    color: Color::DarkGray,
                });
                // A single dot is easy to miss among the ring's own: the mark
                // is a small block, drawn last so it sits on top of the
                // crosshair it lands on.
                ctx.draw(&Points {
                    coords: &[
                        (mark_x - 1.0, mark_y - 1.0),
                        (mark_x, mark_y - 1.0),
                        (mark_x + 1.0, mark_y - 1.0),
                        (mark_x - 1.0, mark_y),
                        (mark_x, mark_y),
                        (mark_x + 1.0, mark_y),
                        (mark_x - 1.0, mark_y + 1.0),
                        (mark_x, mark_y + 1.0),
                        (mark_x + 1.0, mark_y + 1.0),
                    ],
                    color: KEY,
                });
            });
        frame.render_widget(canvas, field);

        let caption = ratatui::layout::Rect {
            x: at.x,
            y: area.y + STICK_ROWS as u16,
            width: at.width + 1,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{name:<6}"), Style::default().fg(MUTED)),
                Span::styled(
                    format!("{:>3}%{:>3}%", horizontal / 10, vertical / 10),
                    Style::default().fg(MUTED),
                ),
            ])),
            caption,
        );
    }
}

/// The columns a stick's canvas takes, and its rows: a braille cell is two dots
/// wide and four tall, so twice the rows in columns is a square in dots.
const STICK_COLS: usize = 20;
const STICK_ROWS: usize = 10;

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
        keys_line(&[("enter", "write"), ("esc", "cancel")])
    } else {
        keys_line(&[("↑↓", "value"), ("enter", "write"), ("esc", "cancel")])
    };
    frame.render_widget(Paragraph::new(footer), rows[2]);
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
        keys_line(&[("←→", "value"), ("enter", "write"), ("esc", "cancel")]),
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

/// A run of keys and what each does: the key in the colour kept for something
/// to press, the words beside it grey, so the two are told apart at a glance.
fn keys_line(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    for (at, (key, action)) in pairs.iter().enumerate() {
        if at > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().fg(KEY).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {action}"),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans)
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
fn draw_prompt(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let Some(prompt) = &app.prompt else {
        return;
    };

    let dialog = dialog_in(area, 46, 6);
    frame.render_widget(Clear, dialog);

    // What would be lost is the fact the box exists for, so it carries the
    // colour kept for what is checked before acting.
    let (title, lines) = match prompt {
        Prompt::Delete { id, name } => {
            let mut going = vec![Span::raw("  ")];
            going.push(Span::styled(
                format!("#{id}"),
                Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
            ));
            if !name.is_empty() {
                going.push(Span::styled(
                    format!(" \"{name}\""),
                    Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
                ));
            }
            (
                " delete profile? ",
                vec![
                    Line::from(going),
                    Line::from(Span::styled(
                        "  is deleted from the board",
                        Style::default().fg(MUTED),
                    )),
                    Line::from(""),
                    keys_line(&[("enter", "delete"), ("esc", "cancel")]),
                ],
            )
        }
        // The device holds the values already, and only a save puts them on the
        // flash: the box names what would be lost with the screen.
        Prompt::Quit => {
            let unsaved = app.unsaved();
            (
                " save before quitting? ",
                vec![
                    Line::from(Span::styled(
                        format!(
                            "  {} of the values are not on the flash:",
                            unsaved.len()
                        ),
                        Style::default().fg(MUTED),
                    )),
                    Line::from(Span::styled(
                        format!("  {}", unsaved.join(", ")),
                        Style::default().fg(HIGHLIGHT),
                    )),
                    Line::from(""),
                    keys_line(&[("s", "save"), ("d", "discard"), ("esc", "cancel")]),
                ],
            )
        }
    };
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
/// What the line in focus is and what it is for. The values it may take are
/// read where they are written, in the box a value is typed in, so this column
/// is free to say what the value means.
fn draw_about(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    // The column is narrow, so the name of the line in focus is what the title
    // cannot carry: it opens the body, in the colour kept for what is read
    // before acting, and the account of it follows.
    let (name, text) = app.about();
    let mut lines = Vec::new();
    if !name.is_empty() {
        lines.push(Line::from(Span::styled(
            name,
            Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }
    if !text.is_empty() {
        lines.push(Line::from(Span::styled(text, Style::default().fg(MUTED))));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" info ")),
        area,
    );
}

/// The lines the firmware logged, broken to the panel's width: what the device
/// said as it ran, newest at the bottom, with the level it said it at.
fn draw_log(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default().borders(Borders::ALL).title(" log ");
    let room = usize::from(block.inner(area).height);
    let width = usize::from(area.width.saturating_sub(2)).max(8);

    // The level a line was written at survives as the mark the reader put in
    // front of it: a failure is red, a warning amber.
    let style_of = |line: &str| match line.split(' ').next() {
        Some("E") => Style::default().fg(Color::Red),
        Some("W") => Style::default().fg(HIGHLIGHT),
        Some("D") => Style::default().fg(MUTED),
        _ => Style::default(),
    };

    let mut rows: Vec<Line> = Vec::new();
    for entry in &app.log {
        let style = style_of(entry);
        for part in broken_to(entry, width) {
            rows.push(Line::from(Span::styled(part, style)));
        }
    }
    // The tail is what a reader wants: the rows above it fall off the top.
    let shown: Vec<Line> = rows.into_iter().rev().take(room).rev().collect();
    frame.render_widget(Paragraph::new(shown).block(block), area);
}

/// One line broken to a width, at its spaces where it can be: a word longer
/// than the panel is cut where the column ends.
fn broken_to(line: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in line.split(' ').filter(|word| !word.is_empty()) {
        let mut word = word.to_string();
        if row.chars().count() + 1 + word.chars().count() > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        while word.chars().count() > width {
            let cut: String = word.chars().take(width).collect();
            rows.push(cut);
            word = word.chars().skip(width).collect();
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(&word);
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
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

    // The buttons the host reads as held this turn, so a slot feeding one of
    // them is lit: a press is checked against the input it is wired to. The
    // numbering is the generic pad's, which is the one this firmware's buttons
    // travel as; a console mode numbers its buttons its own way, and a wrong
    // light is worse than none.
    let held: Vec<i32> = if app.input_mode() != Some(14) {
        Vec::new()
    } else {
        app
        .input
        .as_ref()
        .and_then(|state| state.get("keys"))
        .and_then(|keys| keys.as_array())
        .map(|keys| {
            keys.iter()
                .filter_map(|key| key.get("code").and_then(|code| code.as_u64()))
                .filter_map(|code| thetagp_client::input::pad_button_index(code as u16))
                .map(|index| index as i32)
                .collect()
        })
        .unwrap_or_default()
    };

    let slots = app.remap_len();
    let rows: Vec<Row> = (0..slots)
        .map(|index| {
            // The slot is the board's own input index, as its key map numbers
            // it; the button it feeds is named from the schema's table.
            let target = app.remap_text(index).parse::<i32>().unwrap_or(0);
            let lit = held.contains(&target);
            let from_style = if lit {
                Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            };
            let from = Line::from(Span::styled(format!("input {index:02}"), from_style));
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
                Cell::from("→").style(if lit {
                    Style::default().fg(HIGHLIGHT)
                } else {
                    Style::default().fg(MUTED)
                }),
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
    // A line of keys: the key in the colour of something to press, the words
    // beside it grey, so a reader scans the keys and not the prose.
    let help = vec![
        keys_line(&[
            ("tab", "pane"),
            ("←→", "out of / into a level"),
            ("enter", "connect / open / edit / activate"),
            ("x", "delete"),
            ("i", "input"),
        ]),
        keys_line(&[
            ("n", "new profile"),
            ("e", "name it"),
            ("w", "save"),
            ("l", "load"),
            ("t", "snapshot"),
            ("r", "restore"),
            ("d", "disconnect"),
            ("q", "quit"),
        ]),
    ];
    let mut lines = vec![Line::from(message)];
    lines.extend(help);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::TOP)),
        area,
    );
}
