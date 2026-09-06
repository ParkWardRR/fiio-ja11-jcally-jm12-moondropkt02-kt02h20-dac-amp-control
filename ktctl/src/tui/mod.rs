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
//! * `p` — cycle preset slot
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
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};

use crate::device::fake::FakeDevice;
use crate::device::{Device, Transport};
use crate::proto::peq::{PeqState, PresetState};

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
            KeyCode::Char('p') => app.cycle_preset(),
            KeyCode::Char('w') => match write_state(dev, app) {
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

fn write_state<T: Transport>(dev: &mut Device<T>, app: &App) -> Result<()> {
    for b in &app.state.bands {
        dev.set_band(b)?;
    }
    dev.set_gain(app.state.gain_db)?;
    dev.set_preset(app.state.preset)?;
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
    // Bar heights are gain shifted into a non-negative range for display.
    let bars: Vec<Bar> = app
        .state
        .bands
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let height = ((b.gain_db + 12.0).round().max(0.0)) as u64;
            let style = if i == app.selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Blue)
            };
            Bar::default()
                .value(height)
                .label(Line::from(fmt_hz(b.freq_hz)))
                .text_value(format!("{:+.1}", b.gain_db))
                .style(style)
        })
        .collect();

    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" EQ curve (bar height = gain, +12 dB offset) "),
        )
        .data(BarGroup::default().bars(&bars))
        .bar_width(9)
        .bar_gap(2);
    f.render_widget(chart, area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let dirty = if app.dirty { " [unsaved]" } else { "" };
    let help = "←/→ band · ↑/↓ gain · [/] freq · p preset · w write · q quit";
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

fn fmt_hz(hz: u16) -> String {
    if hz >= 1000 {
        format!("{:.1}k", hz as f32 / 1000.0)
    } else {
        format!("{hz}")
    }
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
}
