//! Command-line interface (roadmap Phase 3).
//!
//! Parsing lives here; the actual protocol work is delegated to
//! [`crate::device::Device`]. A `--fake` flag swaps the USB transport for the
//! in-memory [`crate::device::fake::FakeDevice`], so every subcommand is usable
//! (and testable) without hardware.

mod render;

use anyhow::{Context as _, Result};
use clap::{Args, Parser, Subcommand};

use crate::config::Config;
use crate::device::fake::FakeDevice;
use crate::device::{Device, Transport};
use crate::preset;
use crate::proto::peq::{FilterType, GainEncoding, PeqBand, PeqState, PresetState, BAND_COUNT};
use crate::proto::state::UacMode;

/// Runtime control for the FiiO JA11 and KT02H20-based dongles.
#[derive(Debug, Parser)]
#[command(name = "ktctl", version, about, long_about = None)]
pub struct Cli {
    /// Use the in-memory fake device instead of real USB hardware.
    #[arg(long, global = true)]
    pub fake: bool,

    /// Emit machine-readable JSON where a command supports it.
    #[arg(long, global = true)]
    pub json: bool,

    /// Print the raw frames sent/received.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override the master-gain (0x17) encoding: `x2560-le` (default) or `x10-be`.
    #[arg(long, global = true, value_name = "ENCODING")]
    pub gain_encoding: Option<String>,

    /// Override the save/commit opcode: `0x19` (default) or `0x18`. Only a real
    /// write -> save -> power-cycle -> re-read test can tell which one persists.
    #[arg(long, global = true, value_name = "CMD")]
    pub save_command: Option<String>,

    /// Subcommand to run; when omitted, the interactive TUI launches.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Identify a connected device.
    Probe,
    /// Read / write the parametric EQ.
    #[command(subcommand)]
    Peq(PeqCommand),
    /// Switch the active preset or disable PEQ.
    Preset {
        /// `vocal`/`classic`/`bass`/`user1`/`off`, or `0`-`4`.
        value: String,
    },
    /// Set the master / makeup gain in dB.
    Gain {
        /// Gain in dB (e.g. `-3.0`).
        #[arg(allow_hyphen_values = true)]
        db: f32,
    },
    /// Read the device Status screen (volume, sample rate, firmware, mic, UAC).
    State,
    /// Switch USB Audio Class mode.
    Uac {
        /// `1` or `2`.
        mode: String,
    },
    /// Set the device volume (raw device units).
    Volume {
        /// Volume value.
        level: u8,
    },
    /// List connected JA11 / KT02H20-family USB devices.
    List {
        /// Include non-matching USB devices too.
        #[arg(long)]
        all: bool,
    },
    /// Generate a shell completion script (`bash`/`zsh`/`fish`/`powershell`/`elvish`).
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
}

/// `ktctl peq …` subcommands.
#[derive(Debug, Subcommand)]
pub enum PeqCommand {
    /// Read all 5 bands + gain + preset.
    Get,
    /// Write a single band.
    Set(SetBandArgs),
    /// Commit PEQ edits to the device's persistent storage (opcode unconfirmed).
    Save,
    /// Export the current PEQ to a file (`.json` = ktctl JSON, else AutoEQ text).
    Export {
        /// Output file path (`-` for stdout).
        file: String,
    },
    /// Import a PEQ from a file (ktctl JSON or AutoEQ text) and write it.
    Import {
        /// Input file path.
        file: String,
        /// Also persist to the device after writing (runs `peq save`).
        #[arg(long)]
        save: bool,
    },
}

/// Arguments for `ktctl peq set`.
#[derive(Debug, Args)]
pub struct SetBandArgs {
    /// Band index (0-4).
    pub band: u8,
    /// Centre / corner frequency in Hz.
    #[arg(long)]
    pub freq: Option<u16>,
    /// Gain in dB.
    #[arg(long, allow_hyphen_values = true)]
    pub gain: Option<f32>,
    /// Quality factor Q.
    #[arg(long, allow_hyphen_values = true)]
    pub q: Option<f32>,
    /// Filter type (`peak`, `low-shelf`, `high-shelf`, or a raw integer).
    #[arg(long = "type")]
    pub filter: Option<String>,
}

/// Conservative client-side validation ranges (roadmap Phase 3 item 9). The
/// device's true limits are unconfirmed; these match what FIIO's EQ-screen
/// screenshots imply (±12 dB) and stop obviously-nonsense values before they
/// hit the wire.
mod limits {
    /// Minimum accepted frequency in Hz.
    pub const FREQ_MIN: u16 = 20;
    /// Maximum accepted frequency in Hz.
    pub const FREQ_MAX: u16 = 20_000;
    /// Per-band gain bound (±) in dB (screenshots show a ±12 dB range).
    pub const GAIN_ABS_MAX: f32 = 12.0;
    /// Master-gain bound (±) in dB. Under the likely `×2560` i16 encoding, the
    /// hard wire limit is ~±12.79 dB; ±12 keeps a margin.
    pub const MASTER_GAIN_ABS_MAX: f32 = 12.0;
    /// Minimum Q.
    pub const Q_MIN: f32 = 0.1;
    /// Maximum Q.
    pub const Q_MAX: f32 = 20.0;
}

fn validate_band(b: &PeqBand) -> Result<()> {
    anyhow::ensure!(
        (b.index as usize) < BAND_COUNT,
        "band index {} out of range (0-{})",
        b.index,
        BAND_COUNT - 1
    );
    anyhow::ensure!(
        (limits::FREQ_MIN..=limits::FREQ_MAX).contains(&b.freq_hz),
        "frequency {} Hz out of range ({}-{} Hz)",
        b.freq_hz,
        limits::FREQ_MIN,
        limits::FREQ_MAX
    );
    anyhow::ensure!(
        b.gain_db.abs() <= limits::GAIN_ABS_MAX,
        "gain {} dB out of range (±{} dB)",
        b.gain_db,
        limits::GAIN_ABS_MAX
    );
    anyhow::ensure!(
        (limits::Q_MIN..=limits::Q_MAX).contains(&b.q),
        "Q {} out of range ({}-{})",
        b.q,
        limits::Q_MIN,
        limits::Q_MAX
    );
    Ok(())
}

fn validate_gain(db: f32) -> Result<()> {
    anyhow::ensure!(
        db.abs() <= limits::MASTER_GAIN_ABS_MAX,
        "gain {} dB out of range (±{} dB)",
        db,
        limits::MASTER_GAIN_ABS_MAX
    );
    Ok(())
}

/// Parse args and run. Returns a process exit code.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

/// Resolve the effective gain encoding: CLI flag wins, else config file.
fn resolve_gain_encoding(cli: &Cli, cfg: &Config) -> Result<GainEncoding> {
    match cli.gain_encoding.as_deref() {
        None => Ok(cfg.gain_encoding()),
        Some(s) => match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "x2560-le" | "x2560le" | "2560" => Ok(GainEncoding::X2560Le),
            "x10-be" | "x10be" | "10" => Ok(GainEncoding::X10Be),
            other => anyhow::bail!("unknown gain encoding '{other}' (x2560-le or x10-be)"),
        },
    }
}

/// Resolve the effective save/commit opcode from the `--save-command` flag.
fn resolve_save_command(cli: &Cli) -> Result<crate::proto::opcode::SaveCommand> {
    match cli.save_command.as_deref() {
        None => Ok(crate::proto::opcode::SaveCommand::default()),
        Some(s) => crate::proto::opcode::SaveCommand::parse(s)
            .ok_or_else(|| anyhow::anyhow!("unknown save command '{s}' (0x19 or 0x18)")),
    }
}

/// Run against an explicitly-provided CLI (used by tests).
pub fn dispatch(mut cli: Cli) -> Result<()> {
    let cfg = Config::load();

    // No subcommand → launch the TUI dashboard.
    let Some(command) = cli.command.take() else {
        return crate::tui::run(cli.fake || cfg.default_fake);
    };

    // Commands that don't need a device open.
    match &command {
        Command::Completions { shell } => return run_completions(*shell),
        Command::List { all } => return run_list(*all, &cli),
        _ => {}
    }

    let encoding = resolve_gain_encoding(&cli, &cfg)?;
    let save_command = resolve_save_command(&cli)?;

    if cli.fake || cfg.default_fake {
        run_command(
            &command,
            &cli,
            Device::new(FakeDevice::new())
                .with_gain_encoding(encoding)
                .with_save_command(save_command)
                .with_verbose(cli.verbose),
        )
    } else {
        #[cfg(feature = "usb")]
        {
            use crate::device::usb::{UsbConfig, UsbTransport};
            let transport = UsbTransport::open(&UsbConfig::default())
                .context("opening USB device (try --fake to run without hardware)")?;
            run_command(
                &command,
                &cli,
                Device::new(transport)
                    .with_gain_encoding(encoding)
                    .with_save_command(save_command)
                    .with_verbose(cli.verbose),
            )
        }
        #[cfg(not(feature = "usb"))]
        {
            let _ = (&command, encoding, save_command);
            anyhow::bail!("built without the `usb` feature; re-run with --fake");
        }
    }
}

/// Print a shell completion script to stdout.
fn run_completions(shell: clap_complete::Shell) -> Result<()> {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    clap_complete::generate(shell, &mut cmd, "ktctl", &mut std::io::stdout());
    Ok(())
}

/// List connected devices (needs the usb feature; helpful message otherwise).
fn run_list(all: bool, cli: &Cli) -> Result<()> {
    #[cfg(feature = "usb")]
    {
        let devices = crate::device::usb::list_devices(!all).context("enumerating USB devices")?;
        if cli.json {
            // FoundDevice isn't Serialize; build a small JSON view.
            let items: Vec<_> = devices
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "vid": format!("{:#06x}", d.vid),
                        "pid": format!("{:#06x}", d.pid),
                        "bus": d.bus,
                        "address": d.address,
                        "label": d.label,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        } else if devices.is_empty() {
            println!("no matching devices found");
        } else {
            for d in devices {
                println!(
                    "{:#06x}:{:#06x}  bus {} addr {}  {}",
                    d.vid, d.pid, d.bus, d.address, d.label
                );
            }
        }
        Ok(())
    }
    #[cfg(not(feature = "usb"))]
    {
        let _ = (all, cli);
        anyhow::bail!("built without the `usb` feature; device enumeration unavailable");
    }
}

fn run_command<T: Transport>(cmd: &Command, cli: &Cli, mut dev: Device<T>) -> Result<()> {
    if cli.verbose {
        eprintln!("[ktctl] transport: {}", dev.transport().describe());
    }
    match cmd {
        Command::Probe => {
            println!("device: {}", dev.transport().describe());
            // Best-effort reads to prove the channel works.
            match dev.get_firmware() {
                Ok(v) => println!("firmware: {v}"),
                Err(e) => println!("firmware: <unreadable: {e}>"),
            }
            match dev.get_preset() {
                Ok(p) => println!("preset: {p}"),
                Err(e) => println!("preset: <unreadable: {e}>"),
            }
            Ok(())
        }
        Command::Peq(PeqCommand::Get) => {
            let state = dev.get_state().context("reading PEQ state")?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                render::print_state(&state);
            }
            Ok(())
        }
        Command::Peq(PeqCommand::Set(args)) => {
            // Read-modify-write so unspecified fields keep their current value.
            let mut band = dev
                .get_band(args.band)
                .with_context(|| format!("reading band {} before update", args.band))?;
            if let Some(f) = args.freq {
                band.freq_hz = f;
            }
            if let Some(g) = args.gain {
                band.gain_db = g;
            }
            if let Some(q) = args.q {
                band.q = q;
            }
            if let Some(t) = &args.filter {
                band.filter = FilterType::parse(t).map_err(|e| anyhow::anyhow!(e))?;
            }
            validate_band(&band)?;
            dev.set_band(&band).context("writing band")?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&band)?);
            } else {
                println!("band {} updated:", band.index);
                render::print_band(&band);
            }
            Ok(())
        }
        Command::Peq(PeqCommand::Save) => {
            let (cmd_byte, payload) = dev.save().context("saving PEQ to device")?;
            println!(
                "save issued via cmd {cmd_byte:#04x} payload {payload:02x?} (opcode unconfirmed)"
            );
            Ok(())
        }
        Command::Preset { value } => {
            let preset = PresetState::parse(value).map_err(|e| anyhow::anyhow!(e))?;
            dev.set_preset(preset).context("setting preset")?;
            println!("preset set to {preset}");
            Ok(())
        }
        Command::Gain { db } => {
            validate_gain(*db)?;
            dev.set_gain(*db).context("setting gain")?;
            println!("gain set to {db:+.1} dB");
            Ok(())
        }
        Command::State => {
            let state = dev.get_device_state().context("reading device state")?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                render::print_device_state(&state);
            }
            Ok(())
        }
        Command::Uac { mode } => {
            let mode = UacMode::parse(mode).map_err(|e| anyhow::anyhow!(e))?;
            dev.set_uac(mode).context("setting UAC mode")?;
            println!("UAC mode set to {mode}");
            Ok(())
        }
        Command::Volume { level } => {
            dev.set_volume(*level).context("setting volume")?;
            println!("volume set to {level}");
            Ok(())
        }
        Command::Peq(PeqCommand::Export { file }) => {
            let state = dev.get_state().context("reading PEQ state to export")?;
            let text = preset::export_by_extension(&state, file)
                .map_err(|e| anyhow::anyhow!(e))
                .context("serializing preset")?;
            if file == "-" {
                print!("{text}");
            } else {
                std::fs::write(file, &text).with_context(|| format!("writing {file}"))?;
                eprintln!("exported {} bands to {file}", state.bands.len());
            }
            Ok(())
        }
        Command::Peq(PeqCommand::Import { file, save }) => {
            let text = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
            let state = preset::import_auto(&text, file)
                .map_err(|e| anyhow::anyhow!(e))
                .context("parsing preset")?;
            write_state(&mut dev, &state)?;
            println!("imported {} bands from {file}", state.bands.len());
            if !cli.json {
                render::print_state(&state);
            }
            if *save {
                match dev.save() {
                    Ok((c, p)) => println!("saved via cmd {c:#04x} payload {p:02x?}"),
                    Err(e) => eprintln!("warning: save failed: {e}"),
                }
            }
            Ok(())
        }
        // Handled before a device is opened (see `dispatch`); kept exhaustive.
        Command::List { all } => run_list(*all, cli),
        Command::Completions { shell } => run_completions(*shell),
    }
}

/// Write a full [`PeqState`] to the device: every band, gain, and preset.
fn write_state<T: Transport>(dev: &mut Device<T>, state: &PeqState) -> Result<()> {
    for band in &state.bands {
        let mut b = *band;
        // Ensure indices are sane before writing.
        if (b.index as usize) >= BAND_COUNT {
            b.index = 0;
        }
        validate_band(&b).with_context(|| format!("band {} out of range", b.index))?;
        dev.set_band(&b)
            .with_context(|| format!("writing band {}", b.index))?;
    }
    validate_gain(state.gain_db)?;
    dev.set_gain(state.gain_db).context("writing gain")?;
    dev.set_preset(state.preset).context("writing preset")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("ktctl").chain(args.iter().copied()))
    }

    #[test]
    fn parses_peq_set() {
        let c = cli(&[
            "peq", "set", "0", "--freq", "1000", "--gain", "-3.0", "--q", "0.7",
        ]);
        match c.command {
            Some(Command::Peq(PeqCommand::Set(a))) => {
                assert_eq!(a.band, 0);
                assert_eq!(a.freq, Some(1000));
                assert_eq!(a.gain, Some(-3.0));
                assert_eq!(a.q, Some(0.7));
            }
            _ => panic!("wrong parse"),
        }
    }

    #[test]
    fn fake_peq_set_and_get_roundtrip() {
        let set = cli(&[
            "--fake", "peq", "set", "1", "--freq", "440", "--gain", "5.0", "--q", "1.2",
        ]);
        dispatch(set).unwrap();
        // Each dispatch spins up a fresh FakeDevice, so this only asserts the
        // command path doesn't error; state persistence is covered in device tests.
        let get = cli(&["--fake", "--json", "peq", "get"]);
        dispatch(get).unwrap();
    }

    #[test]
    fn validate_band_rejects_out_of_range() {
        let mut b = PeqBand::flat(0);
        b.gain_db = 99.0;
        assert!(validate_band(&b).is_err());
        b.gain_db = 0.0;
        b.freq_hz = 1;
        assert!(validate_band(&b).is_err());
    }

    #[test]
    fn gain_command_validates() {
        assert!(validate_gain(3.0).is_ok());
        assert!(validate_gain(-100.0).is_err());
    }
}
