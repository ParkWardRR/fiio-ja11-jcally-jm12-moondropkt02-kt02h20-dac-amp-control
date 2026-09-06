//! `ktctl` — runtime PEQ/EQ/gain control for the FiiO JA11 and KTMicro
//! KT02H20-based dongles.
//!
//! The crate is layered so the parts that need no hardware can be built and
//! tested anywhere:
//!
//! * [`proto`] — pure protocol core: CRC-8/MAXIM, the wire [`proto::FrameCodec`],
//!   and the [`proto::PeqBand`] / [`proto::PeqState`] models (roadmap Phase 1).
//! * [`device`] — the [`device::Transport`] trait, the always-available
//!   [`device::fake::FakeDevice`], and (behind the `usb` feature) the native
//!   [`device::usb::UsbTransport`] (roadmap Phase 2).
//! * [`cli`] — argument parsing and command dispatch (roadmap Phase 3).
//! * [`tui`] — the `ratatui` dashboard (roadmap Phase 4).
//!
//! Everything below Phase 0 in the roadmap is provisional: the protocol was
//! recovered by *static* reverse-engineering and has not been confirmed against
//! real hardware. See `docs/PROTOCOL.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod config;
pub mod device;
pub mod preset;
pub mod proto;
pub mod tui;

/// Crate version string, from Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
