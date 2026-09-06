//! Interactive TUI dashboard (roadmap Phase 4).
//!
//! A two-view `ratatui` dashboard mirroring the official FiiO Control app's own
//! Status/EQ tab structure (its third tab, "Guide", is static help text and
//! isn't reproduced here). Firmware flashing is deliberately **out of scope**
//! — that's `ktflash`'s job; this TUI only ever touches runtime state, the
//! same things the app's own Status and EQ screens change. Visual theme
//! (the `ACCENT`/`GREEN`/`AMBER`/`RED`/`DIM` palette) matches `ktflash`'s TUI
//! on purpose, so the two tools feel like a matched pair.
//!
//! It drives the same [`crate::device::Device`] the CLI does, so it works
//! against either the fake device (`--fake`, or when built without USB) or
//! real hardware.
//!
//! Keybindings:
//! * `Tab` / `1` / `2` — switch between the Status and EQ views
//! * `q` / `Esc` — quit
//!
//! Status view:
//! * `↑`/`↓` or `k`/`j` — adjust volume ±1 (applied immediately, like the app)
//! * `u` — toggle UAC 1.0/2.0 (applied immediately)
//! * `r` — refresh from the device
//!
//! EQ view:
//! * `←`/`→` or `h`/`l` — select band
//! * `↑`/`↓` or `k`/`j` — adjust selected band's gain (±0.5 dB)
//! * `[`/`]` — adjust selected band's frequency
//! * `,`/`.` — adjust selected band's Q
//! * `t` — cycle the selected band's filter type
//! * `p` — cycle preset (vocal/classic/bass/user1/off)
//! * `r` — reload EQ state from the device (discards unsaved edits)
//! * `w` — write the current EQ state to the device
//! * `s` — save (commit) EQ edits to the device's persistent storage

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, Paragraph, Tabs};
use ratatui::{Frame, Terminal};

use crate::device::fake::FakeDevice;
use crate::device::{Device, Transport};
use crate::proto::peq::{FilterType, PeqState, PresetState};
use crate::proto::response::sample_curve;
use crate::proto::state::{DeviceState, UacMode};

// Same palette as `ktflash`'s TUI (flasher/src/tui.rs) — deliberately shared
// so the two tools read as a matched pair despite being separate binaries.
const ACCENT: Color = Color::Rgb(34, 211, 238); // cyan
const GREEN: Color = Color::Rgb(74, 222, 128);
const AMBER: Color = Color::Rgb(251, 191, 36);
const RED: Color = Color::Rgb(248, 113, 113);
const DIM: Color = Color::Rgb(100, 116, 139);
const FG: Color = Color::Rgb(226, 232, 240);

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

/// Which of the two tabs is active — mirrors the app's own Status/EQ tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Status,
    Eq,
}

impl View {
    fn next(self) -> Self {
        match self {
            View::Status => View::Eq,
            View::Eq => View::Status,
        }
    }
}

/// In-memory UI state, decoupled from the device so EQ edits are staged
/// until `w`. Status-view changes (volume, UAC) apply immediately, matching
/// the app's own Status screen — there's no "unsaved" concept for a single
/// scalar toggle the way there is for a whole 5-band EQ edit.
struct App {
    view: View,
    state: PeqState,
    device: Option<DeviceState>,
    selected: usize,
    status: String,
    dirty: bool,
}

impl App {
    fn new(state: PeqState, device: Option<DeviceState>) -> Self {
        App {
            view: View::Status,
            state,
            device,
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
    let device_state = dev.get_device_state().ok();
    let mut app = App::new(state, device_state);

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
            KeyCode::Tab => app.view = app.view.next(),
            KeyCode::Char('1') => app.view = View::Status,
            KeyCode::Char('2') => app.view = View::Eq,
            _ => match app.view {
                View::Status => on_key_status(key.code, app, dev),
                View::Eq => on_key_eq(key.code, app, dev),
            },
        }
    }
    Ok(())
}

fn on_key_status<T: Transport>(code: KeyCode, app: &mut App, dev: &mut Device<T>) {
    match code {
        KeyCode::Char('r') => match dev.get_device_state() {
            Ok(s) => {
                app.device = Some(s);
                app.status = "device status refreshed".into();
            }
            Err(e) => app.status = format!("refresh failed: {e}"),
        },
        KeyCode::Up | KeyCode::Char('k') => adjust_volume(app, dev, 1),
        KeyCode::Down | KeyCode::Char('j') => adjust_volume(app, dev, -1),
        KeyCode::Char('u') => {
            let Some(ds) = &app.device else {
                return;
            };
            let next = match ds.uac {
                UacMode::Uac1 => UacMode::Uac2,
                UacMode::Uac2 | UacMode::Raw(_) => UacMode::Uac1,
            };
            match dev.set_uac(next) {
                Ok(()) => {
                    if let Some(ds) = &mut app.device {
                        ds.uac = next;
                    }
                    app.status = format!("UAC mode set to {next}");
                }
                Err(e) => app.status = format!("UAC set failed: {e}"),
            }
        }
        _ => {}
    }
}

fn adjust_volume<T: Transport>(app: &mut App, dev: &mut Device<T>, delta: i16) {
    let Some(ds) = &app.device else {
        return;
    };
    let next = (ds.volume as i16 + delta).clamp(0, 100) as u8;
    match dev.set_volume(next) {
        Ok(()) => {
            if let Some(ds) = &mut app.device {
                ds.volume = next;
            }
            app.status = format!("volume set to {next}");
        }
        Err(e) => app.status = format!("volume set failed: {e}"),
    }
}

fn on_key_eq<T: Transport>(code: KeyCode, app: &mut App, dev: &mut Device<T>) {
    match code {
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
                app.status = "written to device".into();
            }
            Err(e) => app.status = format!("write failed: {e}"),
        },
        KeyCode::Char('s') => match dev.save() {
            Ok((cmd, _)) => app.status = format!("saved to device (cmd {cmd:#04x})"),
            Err(e) => app.status = format!("save failed: {e}"),
        },
        _ => {}
    }
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

    render_tabs(f, chunks[0], app);
    match app.view {
        View::Status => render_status(f, chunks[1], app),
        View::Eq => render_eq(f, chunks[1], app),
    }
    render_footer(f, chunks[2], app);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let titles = vec![Line::from("Status"), Line::from("EQ")];
    let selected = match app.view {
        View::Status => 0,
        View::Eq => 1,
    };
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " ktctl · JA11 ",
                    Style::default().fg(FG).add_modifier(Modifier::BOLD),
                )),
        )
        .select(selected)
        .style(Style::default().fg(DIM))
        .highlight_style(
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
    f.render_widget(tabs, area);
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    let Some(ds) = &app.device else {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no device status available (r to retry)",
                Style::default().fg(RED),
            )))
            .block(Block::default().borders(Borders::ALL).title(" Status ")),
            area,
        );
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .margin(1)
        .split(area);

    f.render_widget(
        Block::default().borders(Borders::ALL).title(" Status "),
        area,
    );

    let mic = if ds.mic_present {
        Span::styled("present", Style::default().fg(GREEN))
    } else {
        Span::styled("not detected", Style::default().fg(DIM))
    };
    let uac1 = ds.uac == UacMode::Uac1;
    let uac2 = ds.uac == UacMode::Uac2;

    f.render_widget(
        line_row("Firmware", Span::styled(&ds.firmware, Style::default().fg(FG))),
        rows[0],
    );
    f.render_widget(
        line_row(
            "Sample rate",
            Span::styled(&ds.sample_rate, Style::default().fg(FG)),
        ),
        rows[1],
    );
    f.render_widget(line_row("In-line mic", mic), rows[2]);
    f.render_widget(
        line_row(
            "UAC mode (u)",
            Line::from(vec![
                uac_choice("UAC 1.0", uac1),
                Span::raw("   "),
                uac_choice("UAC 2.0", uac2),
            ]),
        ),
        rows[3],
    );

    let gauge = Gauge::default()
        .block(Block::default().title(" Device volume (↑↓) "))
        .gauge_style(Style::default().fg(ACCENT))
        .ratio((ds.volume as f64 / 100.0).clamp(0.0, 1.0))
        .label(format!("{}", ds.volume));
    f.render_widget(gauge, rows[4]);
}

fn line_row<'a>(label: &'a str, value: impl Into<Line<'a>>) -> Paragraph<'a> {
    let mut line = vec![Span::styled(
        format!("{label:<14}"),
        Style::default().fg(DIM),
    )];
    line.extend(value.into().spans);
    Paragraph::new(Line::from(line))
}

fn uac_choice(label: &str, selected: bool) -> Span<'_> {
    if selected {
        Span::styled(
            format!("● {label}"),
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!("○ {label}"), Style::default().fg(DIM))
    }
}

fn render_eq(f: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(6)])
        .margin(1)
        .split(area);

    f.render_widget(Block::default().borders(Borders::ALL).title(" EQ "), area);

    let sel = app.state.bands.get(app.selected);
    let detail = match sel {
        Some(b) => format!(
            "band {}  {} Hz  {:+.1} dB  Q {:.2}  {}",
            b.index, b.freq_hz, b.gain_db, b.q, b.filter
        ),
        None => "no bands".into(),
    };
    let dirty = if app.dirty {
        Span::styled(" [unsaved]", Style::default().fg(AMBER))
    } else {
        Span::raw("")
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("gain {:+.1} dB", app.state.gain_db),
                Style::default().fg(FG),
            ),
            Span::raw("  ·  "),
            Span::styled(
                format!("preset {}", app.state.preset),
                Style::default().fg(ACCENT),
            ),
            Span::raw("  ·  "),
            Span::styled(detail, Style::default().fg(FG)),
            dirty,
        ])),
        rows[0],
    );

    render_chart(f, rows[1], app);
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
            .style(Style::default().fg(ACCENT))
            .data(&data),
        Dataset::default()
            .name("bands")
            .marker(symbols::Marker::Dot)
            .graph_type(GraphType::Scatter)
            .style(Style::default().fg(AMBER))
            .data(&band_marks),
    ];

    let chart = Chart::new(datasets)
        .block(Block::default().title(" response (dB vs log-frequency) "))
        .x_axis(
            Axis::default()
                .title("20 Hz → 20 kHz")
                .style(Style::default().fg(DIM))
                .bounds([0.0, (POINTS - 1) as f64]),
        )
        .y_axis(
            Axis::default()
                .title("dB")
                .style(Style::default().fg(DIM))
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
    let help = match app.view {
        View::Status => "Tab switch · ↑↓ volume · u UAC · r refresh · q quit",
        View::Eq => "Tab switch · ←→ band · ↑↓ gain · [] freq · ,. Q · t type · p preset · r reload · w write · s save · q quit",
    };
    let text = Line::from(vec![
        Span::styled(app.status.clone(), Style::default().fg(AMBER)),
        Span::raw("   "),
        Span::styled(help, Style::default().fg(DIM)),
    ]);
    f.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Left),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps() {
        let mut app = App::new(PeqState::flat(), None);
        app.select_prev();
        assert_eq!(app.selected, app.band_count() - 1);
        app.select_next();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn gain_adjust_clamps_and_marks_dirty() {
        let mut app = App::new(PeqState::flat(), None);
        for _ in 0..100 {
            app.adjust_gain(0.5);
        }
        assert!(app.dirty);
        assert!(app.state.bands[0].gain_db <= 12.0);
    }

    #[test]
    fn preset_cycles_through_off() {
        let mut app = App::new(PeqState::flat(), None);
        app.state.preset = PresetState::User1;
        app.cycle_preset();
        assert_eq!(app.state.preset, PresetState::Off);
        app.cycle_preset();
        assert_eq!(app.state.preset, PresetState::Vocal);
    }

    #[test]
    fn q_adjust_clamps() {
        let mut app = App::new(PeqState::flat(), None);
        for _ in 0..500 {
            app.adjust_q(-0.1);
        }
        assert!(app.state.bands[0].q >= 0.1);
    }

    #[test]
    fn filter_cycles_back_to_peak() {
        let mut app = App::new(PeqState::flat(), None);
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

    #[test]
    fn view_toggles_between_status_and_eq() {
        assert_eq!(View::Status.next(), View::Eq);
        assert_eq!(View::Eq.next(), View::Status);
    }
}
