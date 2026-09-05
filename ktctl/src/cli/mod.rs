//! Command-line interface (roadmap Phase 3).
//!
//! Parsing lives here; the actual protocol work is delegated to
//! [`crate::device::Device`]. A `--fake` flag swaps the USB transport for the
//! in-memory [`crate::device::fake::FakeDevice`], so every subcommand is usable
//! (and testable) without hardware.

mod render;

use anyhow::{Context as _, Result};
use clap::{Args, Parser, Subcommand};

use crate::device::fake::FakeDevice;
use crate::device::{Device, Transport};
use crate::proto::peq::{FilterType, PeqBand, PresetState, BAND_COUNT};

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
    /// Switch the active preset slot or disable PEQ (`0`-`3` or `off`).
    Preset {
        /// Preset slot 0-3, or `off`.
        value: String,
    },
    /// Set the global / makeup gain in dB.
    Gain {
        /// Gain in dB (e.g. `-3.0`).
        #[arg(allow_hyphen_values = true)]
        db: f32,
    },
}

/// `ktctl peq …` subcommands.
#[derive(Debug, Subcommand)]
pub enum PeqCommand {
    /// Read all 5 bands + gain + preset.
    Get,
    /// Write a single band.
    Set(SetBandArgs),
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
    /// Filter type (`peaking`, `low-shelf`, `high-shelf`, or a raw integer).
    #[arg(long = "type")]
    pub filter: Option<String>,
}

/// Conservative client-side validation ranges (roadmap Phase 3 item 6). The
/// device's true limits are unknown until hardware validation; these just stop
/// obviously-nonsense values before they hit the wire.
mod limits {
    /// Minimum accepted frequency in Hz.
    pub const FREQ_MIN: u16 = 20;
    /// Maximum accepted frequency in Hz.
    pub const FREQ_MAX: u16 = 20_000;
    /// Gain bound (±) in dB.
    pub const GAIN_ABS_MAX: f32 = 24.0;
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
        db.abs() <= limits::GAIN_ABS_MAX,
        "gain {} dB out of range (±{} dB)",
        db,
        limits::GAIN_ABS_MAX
    );
    Ok(())
}

/// Parse args and run. Returns a process exit code.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

/// Run against an explicitly-provided CLI (used by tests).
pub fn dispatch(mut cli: Cli) -> Result<()> {
    // No subcommand → launch the TUI dashboard.
    let Some(command) = cli.command.take() else {
        return crate::tui::run(cli.fake);
    };

    if cli.fake {
        run_command(&command, &cli, Device::new(FakeDevice::new()))
    } else {
        #[cfg(feature = "usb")]
        {
            use crate::device::usb::{UsbConfig, UsbTransport};
            let transport = UsbTransport::open(&UsbConfig::default())
                .context("opening USB device (try --fake to run without hardware)")?;
            run_command(&command, &cli, Device::new(transport))
        }
        #[cfg(not(feature = "usb"))]
        {
            let _ = &command;
            anyhow::bail!("built without the `usb` feature; re-run with --fake");
        }
    }
}

fn run_command<T: Transport>(cmd: &Command, cli: &Cli, mut dev: Device<T>) -> Result<()> {
    if cli.verbose {
        eprintln!("[ktctl] transport: {}", dev.transport().describe());
    }
    match cmd {
        Command::Probe => {
            println!("device: {}", dev.transport().describe());
            // Best-effort read to prove the channel works.
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("ktctl").chain(args.iter().copied()))
    }

    #[test]
    fn parses_peq_set() {
        let c = cli(&["peq", "set", "0", "--freq", "1000", "--gain", "-3.0", "--q", "0.7"]);
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
