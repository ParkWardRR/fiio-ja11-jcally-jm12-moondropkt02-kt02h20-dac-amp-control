//! Interactive TUI dashboard (roadmap Phase 4).
//!
//! A `ratatui` view of all five PEQ bands as a bar chart, plus gain and preset,
//! with keyboard editing of the selected band. It drives the same
//! [`crate::device::Device`] the CLI does, so it works against either the fake
//! device (`--fake`, or when built without USB) or real hardware.
//!
//! Keybindings:
//! * `←`/`→` or `h`/`l` — select band
//! * `↑`/`↓` or `k`/`j` — adjust selected band's gain (±0.5 dB)
//! * `[`/`]` — adjust selected band's frequency
//! * `,`/`.` — adjust selected band's Q
//! * `t` — cycle the selected band's filter type
//! * `p` — cycle preset (vocal/classic/bass/user1/off)
//! * `r` — reload state from the device (discards unsaved edits)
//! * `w` — write the current state to the device
//! * `q` / `Esc` — quit

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph};
use ratatui::{Frame, Terminal};

use crate::device::fake::FakeDevice;
use crate::device::{Device, Transport};
use crate::proto::peq::{FilterType, PeqState, PresetState};
use crate::proto::response::sample_curve;

/// Entry point used by the CLI when no subcommand is given.
pub fn run(fake: bool) -> Result<()> {
    if fake {
        run_with(Device::new(FakeDevice::new()))
    } else {
        #[cfg(feature = "usb")]
        {
            use crate::device::usb::{UsbConfig, UsbTransport};
            match UsbTransport::open(&UsbConfig::default()) {
                Ok(t) => run_with(Device::new(t)),
                Err(e) => {
                    eprintln!("no device ({e}); starting TUI against the fake device");
                    run_with(Device::new(FakeDevice::new()))
                }
            }
        }
        #[cfg(not(feature = "usb"))]
        {
            run_with(Device::new(FakeDevice::new()))
        }
    }
}

/// In-memory UI state, decoupled from the device so edits are staged until `w`.
struct App {
    state: PeqState,
    selected: usize,
    status: String,
    dirty: bool,
}

impl App {
    fn new(state: PeqState) -> Self {
        App {
            state,
            selected: 0,
            status: "loaded".into(),
            dirty: false,
        }
    }

    fn band_count(&self) -> usize {
        self.state.bands.len()
    }

    fn select_prev(&mut self) {
        if self.selected == 0 {
            self.selected = self.band_count().saturating_sub(1);
        } else {
            self.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.band_count().max(1);
    }

    fn adjust_gain(&mut self, delta: f32) {
        if let Some(b) = self.state.bands.get_mut(self.selected) {
            b.gain_db = (b.gain_db + delta).clamp(-12.0, 12.0);
            self.dirty = true;
            self.status = format!("band {} gain {:+.1} dB (unsaved)", b.index, b.gain_db);
        }
    }

    fn adjust_freq(&mut self, factor: f32) {
        if let Some(b) = self.state.bands.get_mut(self.selected) {
            let next = (b.freq_hz as f32 * factor).round().clamp(20.0, 20_000.0);
            b.freq_hz = next as u16;
            self.dirty = true;
            self.status = format!("band {} freq {} Hz (unsaved)", b.index, b.freq_hz);
        }
    }

    fn adjust_q(&mut self, delta: f32) {
        if let Some(b) = self.state.bands.get_mut(self.selected) {
            b.q = (b.q + delta).clamp(0.1, 20.0);
            self.dirty = true;
            self.status = format!("band {} Q {:.2} (unsaved)", b.index, b.q);
        }
    }

    fn cycle_filter(&mut self) {
        if let Some(b) = self.state.bands.get_mut(self.selected) {
            b.filter = match b.filter {
                FilterType::Peak => FilterType::LowShelf,
                FilterType::LowShelf => FilterType::HighShelf,
                FilterType::HighShelf | FilterType::Unknown(_) => FilterType::Peak,
            };
            self.dirty = true;
            self.status = format!("band {} type {} (unsaved)", b.index, b.filter);
        }
    }

    fn cycle_preset(&mut self) {
        // Cycle vocal → classic → bass → user1 → off → vocal.
        self.state.preset = match self.state.preset {
            PresetState::Vocal => PresetState::Classic,
            PresetState::Classic => PresetState::Bass,
            PresetState::Bass => PresetState::User1,
            PresetState::User1 => PresetState::Off,
            PresetState::Off => PresetState::Vocal,
            PresetState::Raw(_) => PresetState::Vocal,
        };
        self.dirty = true;
        self.status = format!("preset {} (unsaved)", self.state.preset);
    }
}

fn run_with<T: Transport>(mut dev: Device<T>) -> Result<()> {
    let state = dev.get_state().unwrap_or_else(|_| PeqState::flat());
    let mut app = App::new(state);

    let mut terminal = setup_terminal()?;
    let res = event_loop(&mut terminal, &mut app, &mut dev);
    restore_terminal(&mut terminal)?;
    res
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn event_loop<T: Transport>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    dev: &mut Device<T>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Left | KeyCode::Char('h') => app.select_prev(),
            KeyCode::Right | KeyCode::Char('l') => app.select_next(),
            KeyCode::Up | KeyCode::Char('k') => app.adjust_gain(0.5),
            KeyCode::Down | KeyCode::Char('j') => app.adjust_gain(-0.5),
            KeyCode::Char(']') => app.adjust_freq(1.1),
            KeyCode::Char('[') => app.adjust_freq(1.0 / 1.1),
            KeyCode::Char('.') | KeyCode::Char('>') => app.adjust_q(0.1),
            KeyCode::Char(',') | KeyCode::Char('<') => app.adjust_q(-0.1),
            KeyCode::Char('t') => app.cycle_filter(),
            KeyCode::Char('p') => app.cycle_preset(),
            KeyCode::Char('r') => match dev.get_state() {
                Ok(s) => {
                    app.state = s;
                    app.dirty = false;
                    app.status = "reloaded from device".into();
                }
                Err(e) => app.status = format!("reload failed: {e}"),
            },
            KeyCode::Char('w') => match write_state(dev, &app.state) {
                Ok(()) => {
                    app.dirty = false;
                    app.status = "saved to device".into();
                }
                Err(e) => app.status = format!("write failed: {e}"),
            },
            _ => {}
        }
    }
    Ok(())
}

fn write_state<T: Transport>(dev: &mut Device<T>, state: &PeqState) -> Result<()> {
    for b in &state.bands {
        dev.set_band(b)?;
    }
    dev.set_gain(state.gain_db)?;
    dev.set_preset(state.preset)?;
    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_header(f, chunks[0], app);
    render_chart(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let sel = app.state.bands.get(app.selected);
    let detail = match sel {
        Some(b) => format!(
            "band {}  {} Hz  {:+.1} dB  Q {:.2}  {}",
            b.index, b.freq_hz, b.gain_db, b.q, b.filter
        ),
        None => "no bands".into(),
    };
    let text = Line::from(vec![
        Span::styled("ktctl ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!(
            "· gain {:+.1} dB · preset {} · {}",
            app.state.gain_db, app.state.preset, detail
        )),
    ]);
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" JA11 PEQ ")),
        area,
    );
}

fn render_chart(f: &mut Frame, area: Rect, app: &App) {
    // Real magnitude response over a log-frequency grid. X is the grid index
    // (0..N) so the curve reads left→low, right→high; Y is dB.
    const POINTS: usize = 120;
    let curve = sample_curve(&app.state, POINTS);
    let data: Vec<(f64, f64)> = curve
        .iter()
        .enumerate()
        .map(|(i, &(_, db))| (i as f64, db))
        .collect();

    // Markers for each band's centre frequency, so you can see where they sit.
    let band_marks: Vec<(f64, f64)> = app
        .state
        .bands
        .iter()
        .map(|b| {
            let idx = freq_to_grid_index(b.freq_hz, POINTS);
            (idx as f64, curve[idx].1)
        })
        .collect();

    let datasets = vec![
        Dataset::default()
            .name("response")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&data),
        Dataset::default()
            .name("bands")
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(Color::Yellow))
            .data(&band_marks),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" EQ response (dB vs log-frequency) "),
        )
        .x_axis(
            Axis::default()
                .title("20 Hz → 20 kHz")
                .style(Style::default().fg(Color::DarkGray))
                .bounds([0.0, (POINTS - 1) as f64]),
        )
        .y_axis(
            Axis::default()
                .title("dB")
                .style(Style::default().fg(Color::DarkGray))
                .labels(["-12", "0", "+12"])
                .bounds([-12.0, 12.0]),
        );
    f.render_widget(chart, area);
}

/// Grid index (0..points) for a frequency on the log grid `sample_curve` uses.
fn freq_to_grid_index(hz: u16, points: usize) -> usize {
    use crate::proto::response::{GRID_MAX_HZ, GRID_MIN_HZ};
    let f = (hz as f64).clamp(GRID_MIN_HZ, GRID_MAX_HZ);
    let t = (f.log10() - GRID_MIN_HZ.log10()) / (GRID_MAX_HZ.log10() - GRID_MIN_HZ.log10());
    (t * (points - 1) as f64)
        .round()
        .clamp(0.0, (points - 1) as f64) as usize
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let dirty = if app.dirty { " [unsaved]" } else { "" };
    let help =
        "←→ band · ↑↓ gain · [] freq · ,. Q · t type · p preset · r reload · w write · q quit";
    let text = Line::from(vec![
        Span::styled(app.status.clone(), Style::default().fg(Color::Yellow)),
        Span::raw(dirty),
        Span::raw("   "),
        Span::styled(help, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps() {
        let mut app = App::new(PeqState::flat());
        app.select_prev();
        assert_eq!(app.selected, app.band_count() - 1);
        app.select_next();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn gain_adjust_clamps_and_marks_dirty() {
        let mut app = App::new(PeqState::flat());
        for _ in 0..100 {
            app.adjust_gain(0.5);
        }
        assert!(app.dirty);
        assert!(app.state.bands[0].gain_db <= 12.0);
    }

    #[test]
    fn preset_cycles_through_off() {
        let mut app = App::new(PeqState::flat());
        app.state.preset = PresetState::User1;
        app.cycle_preset();
        assert_eq!(app.state.preset, PresetState::Off);
        app.cycle_preset();
        assert_eq!(app.state.preset, PresetState::Vocal);
    }

    #[test]
    fn q_adjust_clamps() {
        let mut app = App::new(PeqState::flat());
        for _ in 0..500 {
            app.adjust_q(-0.1);
        }
        assert!(app.state.bands[0].q >= 0.1);
    }

    #[test]
    fn filter_cycles_back_to_peak() {
        let mut app = App::new(PeqState::flat());
        assert_eq!(app.state.bands[0].filter, FilterType::Peak);
        app.cycle_filter();
        assert_eq!(app.state.bands[0].filter, FilterType::LowShelf);
        app.cycle_filter();
        assert_eq!(app.state.bands[0].filter, FilterType::HighShelf);
        app.cycle_filter();
        assert_eq!(app.state.bands[0].filter, FilterType::Peak);
    }

    #[test]
    fn freq_to_grid_index_is_monotonic() {
        assert!(freq_to_grid_index(20, 100) < freq_to_grid_index(1000, 100));
        assert!(freq_to_grid_index(1000, 100) < freq_to_grid_index(20000, 100));
    }
}
