//! The device in a terminal: the daemon owns it, this shows it.
//!
//! Every write goes to the daemon, which holds the board and the one connection
//! to it; this program is a client of that socket and of nothing else.

mod app;
mod labels;
mod ui;

use std::io;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use app::{App, HUE_STEP, Pane, Prompt};

fn main() -> io::Result<()> {
    let mut app = App::new();
    app.start();

    // The source a caller names is opened before anything is drawn, so a
    // machine with no keyboard can still show the board it is asked about.
    if let Some(source) = argument("--source") {
        match app.sources.iter().position(|name| name == &source) {
            Some(index) => app.source = index,
            None => {
                app.sources.push(source);
                app.source = app.sources.len() - 1;
            }
        }
        if let Err(error) = app.connect() {
            app.error = Some(error);
        }
    }

    // A frame drawn and written as text: what the screen holds, without a
    // terminal to hold it -- for a log, a test or a machine with no display.
    if std::env::args().any(|argument| argument == "--frame") {
        return print_frame(&app);
    }

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(out))?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

/// The value of `--name value` or `--name=value`.
fn argument(name: &str) -> Option<String> {
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == name {
            return arguments.next();
        }
        if let Some(value) = argument.strip_prefix(&format!("{name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn print_frame(app: &App) -> io::Result<()> {
    use std::io::Write;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| ui::draw(frame, app))?;

    let buffer = terminal.backend().buffer();
    let mut out = io::stdout();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && handle(app, key.code) {
                    return Ok(());
                }
            }
        }
    }
}

/// Acts on one key press, and answers whether the program ends.
fn handle(app: &mut App, code: KeyCode) -> bool {
    // A question is answered before anything else is read: the box holds until
    // one of its answers is given.
    if let Some(question) = app.prompt.clone() {
        match (question, code) {
            (Prompt::Delete { .. }, KeyCode::Enter) => act(app, |app| app.confirm_delete()),
            (Prompt::Delete { .. }, KeyCode::Esc | KeyCode::Char('n')) => app.cancel_prompt(),
            // Leaving with values the flash does not hold: the box asks first,
            // and a save writes them before the screen goes.
            (Prompt::Quit, KeyCode::Char('s')) => {
                act(app, |app| app.save());
                return true;
            }
            (Prompt::Quit, KeyCode::Char('d')) => return true,
            (Prompt::Quit, KeyCode::Esc | KeyCode::Char('n')) => app.cancel_prompt(),
            _ => {}
        }
        return false;
    }

    if app.editing.is_some() {
        match code {
            KeyCode::Enter => act(app, |app| app.commit_edit()),
            KeyCode::Esc => app.cancel_edit(),
            KeyCode::Backspace => app.edit_pop(),
            // The upright keys walk the choices a value offers, and the
            // sideways ones move along a scale.
            KeyCode::Up => app.edit_step(-1),
            KeyCode::Down => app.edit_step(1),
            KeyCode::Left => app.edit_nudge(-HUE_STEP),
            KeyCode::Right => app.edit_nudge(HUE_STEP),
            KeyCode::Char(character) => app.edit_push(character),
            _ => {}
        }
        return false;
    }

    match code {
        // Nothing a save would carry is left behind, so `q` is the answer; a
        // change that is only on the device is put to the reader first.
        KeyCode::Char('q') => {
            if app.unsaved().is_empty() {
                return true;
            }
            act(app, |app| app.ask_quit());
        }
        KeyCode::Tab => {
            app.pane = match app.pane {
                Pane::Sources => Pane::Keys,
                Pane::Keys if app.remap_len() > 0 => Pane::Remap,
                Pane::Keys => Pane::Profiles,
                Pane::Remap => Pane::Profiles,
                Pane::Profiles => Pane::Sources,
            };
        }
        KeyCode::Up => step(app, -1),
        KeyCode::Down => step(app, 1),
        KeyCode::Enter => match app.pane {
            // A domain opens or closes, a key is edited: the menu answers the
            // one key with what the line in focus is.
            Pane::Keys => act(app, |app| app.open_selected()),
            Pane::Remap => act(app, |app| app.begin_edit()),
            Pane::Profiles => act(app, |app| app.select_profile()),
            Pane::Sources => act(app, |app| app.connect()),
        },
        // The sideways keys walk the levels of the options menu: a segment is
        // gone into, and the way back out is the one a reader already knows.
        KeyCode::Left if app.pane == Pane::Keys => act(app, |app| app.go_up()),
        KeyCode::Right if app.pane == Pane::Keys => act(app, |app| app.go_in()),
        KeyCode::Esc if app.pane == Pane::Keys => act(app, |app| app.go_up()),
        KeyCode::Char('c') => act(app, |app| app.connect()),
        KeyCode::Char('d') => act(app, |app| app.disconnect()),
        KeyCode::Char('w') => act(app, |app| app.save()),
        KeyCode::Char('l') => act(app, |app| app.load()),
        KeyCode::Char('n') => act(app, |app| app.create_profile()),
        KeyCode::Char('e') if app.pane == Pane::Profiles => act(app, |app| app.begin_edit()),
        // The delete keeps its own pending state: the same key confirms it.
        KeyCode::Char('x') => act(app, |app| app.ask_delete()),
        KeyCode::Char('t') => act(app, |app| app.take_snapshot()),
        KeyCode::Char('r') => act(app, |app| app.restore_snapshot()),
        KeyCode::Char('R') => act(app, |app| app.refresh()),
        _ => {}
    }
    false
}

fn act(app: &mut App, action: fn(&mut App) -> Result<(), String>) {
    app.error = None;
    if let Err(error) = action(app) {
        app.error = Some(error);
    }
    // Whatever was said goes into the log, where the line after it cannot
    // cover it up.
    app.note();
}

/// Moves the selection of the pane in focus.
fn step(app: &mut App, delta: i32) {
    let move_within = |selected: &mut usize, length: usize| {
        if length == 0 {
            return;
        }
        let last = length - 1;
        *selected = if delta < 0 {
            selected.saturating_sub(1)
        } else {
            (*selected + 1).min(last)
        };
    };

    let remap_len = app.remap_len();
    match app.pane {
        Pane::Keys => {
            let lines = app.rows().len();
            move_within(&mut app.selected, lines);
        }
        Pane::Remap => move_within(&mut app.remap_selected, remap_len),
        Pane::Profiles => move_within(&mut app.profile_selected, app.profiles.len()),
        Pane::Sources => {
            let length = app.sources.len();
            move_within(&mut app.source, length);
        }
    }
}
