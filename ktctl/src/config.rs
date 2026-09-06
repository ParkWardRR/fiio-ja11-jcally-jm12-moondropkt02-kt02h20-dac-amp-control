//! Optional on-disk configuration (`~/.config/ktctl/config.toml`).
//!
//! Everything here has a sensible default, so the file is entirely optional. It
//! exists so an operator who has hardware-confirmed one of the protocol
//! ambiguities (e.g. the master-gain encoding) can pin it once instead of
//! passing a flag every invocation. CLI flags always override the file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::proto::peq::GainEncoding;

/// User configuration, deserialized from TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// Which master-gain (`0x17`) encoding to use.
    pub gain_encoding: GainEncodingPref,
    /// Default to the fake device even without `--fake` (handy for development).
    pub default_fake: bool,
}

/// Serializable mirror of [`GainEncoding`] with a TOML-friendly default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GainEncodingPref {
    /// `gain×2560` little-endian (default; two hardware drivers agree).
    #[default]
    X2560Le,
    /// `gain×10` big-endian (this project's original static RE).
    X10Be,
}

impl From<GainEncodingPref> for GainEncoding {
    fn from(p: GainEncodingPref) -> Self {
        match p {
            GainEncodingPref::X2560Le => GainEncoding::X2560Le,
            GainEncodingPref::X10Be => GainEncoding::X10Be,
        }
    }
}

impl Config {
    /// The default config path (`$XDG_CONFIG_HOME`/ktctl or `~/.config/ktctl`).
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("ktctl").join("config.toml"))
    }

    /// Load config from `path`, returning defaults if the file is absent.
    pub fn load_from(path: &Path) -> Result<Config, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(ConfigError::Io(e.to_string())),
        }
    }

    /// Load from the default path, or defaults if unavailable.
    pub fn load() -> Config {
        Self::default_path()
            .and_then(|p| Self::load_from(&p).ok())
            .unwrap_or_default()
    }

    /// The resolved [`GainEncoding`].
    pub fn gain_encoding(&self) -> GainEncoding {
        self.gain_encoding.into()
    }
}

/// Errors loading configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A filesystem error reading the file.
    #[error("config I/O error: {0}")]
    Io(String),
    /// A TOML parse error.
    #[error("config parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_x2560_le() {
        let c = Config::default();
        assert_eq!(c.gain_encoding(), GainEncoding::X2560Le);
        assert!(!c.default_fake);
    }

    #[test]
    fn parses_toml() {
        let text = "gain-encoding = \"x10-be\"\ndefault-fake = true\n";
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.gain_encoding(), GainEncoding::X10Be);
        assert!(c.default_fake);
    }

    #[test]
    fn missing_file_is_default() {
        let c = Config::load_from(Path::new("/nonexistent/ktctl/config.toml")).unwrap();
        assert_eq!(c.gain_encoding(), GainEncoding::X2560Le);
    }
}
