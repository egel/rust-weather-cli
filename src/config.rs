use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::geo::Place;

/// Measurement units displayed (and requested from the API).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Units {
    #[default]
    #[serde(rename = "metric")]
    Metric,
    #[serde(rename = "imperial")]
    Imperial,
}

impl Units {
    pub fn toggle(self) -> Self {
        match self {
            Self::Metric => Self::Imperial,
            Self::Imperial => Self::Metric,
        }
    }

    pub fn temp(self) -> &'static str {
        match self {
            Self::Metric => "°C",
            Self::Imperial => "°F",
        }
    }

    pub fn wind(self) -> &'static str {
        match self {
            Self::Metric => "km/h",
            Self::Imperial => "mph",
        }
    }

    pub fn precip(self) -> &'static str {
        match self {
            Self::Metric => "mm",
            Self::Imperial => "in",
        }
    }
}

/// Persisted app settings.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<Units>,
}

impl Config {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("rust-weather-cli").join("config.json"))
    }

    /// Load config, falling back to defaults when missing or unreadable.
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = Self::path() else {
            anyhow::bail!("could not determine config directory");
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
