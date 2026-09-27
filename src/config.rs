use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read configuration")]
    Read(#[source] io::Error),
    #[error("invalid configuration value for '{key}': '{value}'")]
    InvalidValue { key: String, value: String },
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub profile: String,
    pub sounds_dir: PathBuf,
    pub master_volume: f32,
    pub enabled: bool,
    pub auto_start: bool,
    pub spatial_audio: bool,
    pub per_key_feel: bool,
    pub home_row_softness: f32,
    pub volume_normalization: bool,
    pub mute_modifiers: bool,
    pub tone_lpf: f32,
    pub tone_pitch: f32,
    pub mouse_tone_lpf: f32,
    pub mouse_tone_pitch: f32,
    pub mouse_volume: f32,
    pub enter_tone_lpf: f32,
    pub enter_tone_pitch: f32,
    pub mouse_sound: String,
    pub favorites: String,
    pub hover_preview: bool,
    pub enter_sound: String,
    pub enter_volume: f32,
    pub visualizer_enabled: bool,
    pub visualizer_timeout: f32,
    pub visualizer_scale: f32,
    pub visualizer_follow_cursor: bool,
    pub visualizer_position: String,
    pub visualizer_cursor_gap: f32,
    pub visualizer_glow: String,
    pub visualizer_frame: String,
    pub visualizer_keycap: String,
    pub notch_overlay_enabled: bool,
    pub notch_overlay_auto_hide: bool,
    pub show_lizard_switch: bool,
    pub show_faah_enter_sound: bool,
    pub ui_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            profile: "gateron-ink-black".into(),
            sounds_dir: default_data_dir().join("keebyd/sounds"),
            master_volume: 1.0,
            enabled: true,
            auto_start: true,
            spatial_audio: true,
            per_key_feel: true,
            home_row_softness: 1.0,
            volume_normalization: true,
            mute_modifiers: false,
            tone_lpf: 0.5,
            tone_pitch: 1.0,
            mouse_tone_lpf: 0.5,
            mouse_tone_pitch: 1.0,
            mouse_volume: 1.0,
            enter_tone_lpf: 0.5,
            enter_tone_pitch: 1.0,
            mouse_sound: String::new(),
            favorites: String::new(),
            hover_preview: true,
            enter_sound: String::new(),
            enter_volume: 1.0,
            visualizer_enabled: true,
            visualizer_timeout: 0.6,
            visualizer_scale: 1.0,
            visualizer_follow_cursor: true,
            visualizer_position: "BottomRight".into(),
            visualizer_cursor_gap: 16.0,
            visualizer_glow: "FFFFFF".into(),
            visualizer_frame: "1A1A1C".into(),
            visualizer_keycap: "2C2C2E".into(),
            notch_overlay_enabled: true,
            notch_overlay_auto_hide: false,
            show_lizard_switch: false,
            show_faah_enter_sound: false,
            ui_port: 7777,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(ConfigError::Read)?;
        let mut config = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            config.set(key.trim(), value.trim())?;
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file =
            tempfile::NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
        writeln!(file, "# keebyd configuration")?;
        for (key, value) in self.entries() {
            writeln!(file, "{key} = {value}")?;
        }
        file.as_file().sync_all()?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let boolean = || parse_bool(key, value);
        match key {
            "profile" => self.profile = value.into(),
            "sounds_dir" => self.sounds_dir = value.into(),
            "master_volume" => {
                self.master_volume = parse_range(key, value, 0.0..=4.0)?.min(1.0);
            }
            "enabled" => self.enabled = boolean()?,
            "auto_start" => self.auto_start = boolean()?,
            "spatial_audio" => self.spatial_audio = boolean()?,
            "per_key_feel" => self.per_key_feel = boolean()?,
            "home_row_softness" => self.home_row_softness = parse_range(key, value, 0.0..=2.0)?,
            "volume_normalization" => self.volume_normalization = boolean()?,
            "mute_modifiers" => self.mute_modifiers = boolean()?,
            "tone_lpf" => self.tone_lpf = parse_range(key, value, 0.0..=1.0)?,
            "tone_pitch" => self.tone_pitch = parse_range(key, value, 0.1..=4.0)?,
            "mouse_tone_lpf" => self.mouse_tone_lpf = parse_range(key, value, 0.0..=1.0)?,
            "mouse_tone_pitch" => self.mouse_tone_pitch = parse_range(key, value, 0.1..=4.0)?,
            "mouse_volume" => {
                self.mouse_volume = parse_range(key, value, 0.0..=2.0)?.min(1.0);
            }
            "enter_tone_lpf" => self.enter_tone_lpf = parse_range(key, value, 0.0..=1.0)?,
            "enter_tone_pitch" => self.enter_tone_pitch = parse_range(key, value, 0.1..=4.0)?,
            "mouse_sound" => self.mouse_sound = value.into(),
            "mouse_clicks" => {
                self.mouse_sound = if boolean()? {
                    "default".into()
                } else {
                    String::new()
                };
            }
            "favorites" => self.favorites = value.into(),
            "hover_preview" => self.hover_preview = boolean()?,
            "enter_sound" => self.enter_sound = value.into(),
            "enter_volume" => {
                self.enter_volume = parse_range(key, value, 0.0..=2.0)?.min(1.0);
            }
            "visualizer_enabled" => self.visualizer_enabled = boolean()?,
            "visualizer_timeout" => {
                self.visualizer_timeout = parse_range(key, value, 0.2..=3.0)?;
            }
            "visualizer_scale" => self.visualizer_scale = parse_range(key, value, 0.5..=2.0)?,
            "visualizer_follow_cursor" => self.visualizer_follow_cursor = boolean()?,
            "visualizer_position" => self.visualizer_position = value.into(),
            "visualizer_cursor_gap" => {
                self.visualizer_cursor_gap = parse_range(key, value, 0.0..=80.0)?;
            }
            "visualizer_glow" => self.visualizer_glow = value.into(),
            "visualizer_frame" => self.visualizer_frame = value.into(),
            "visualizer_keycap" => self.visualizer_keycap = value.into(),
            "notch_overlay_enabled" => self.notch_overlay_enabled = boolean()?,
            "notch_overlay_auto_hide" => self.notch_overlay_auto_hide = boolean()?,
            "show_lizard_switch" => self.show_lizard_switch = boolean()?,
            "show_faah_enter_sound" => self.show_faah_enter_sound = boolean()?,
            "ui_port" => self.ui_port = parse_value(key, value)?,
            _ => {}
        }
        Ok(())
    }

    fn entries(&self) -> Vec<(&str, String)> {
        vec![
            ("profile", self.profile.clone()),
            ("sounds_dir", self.sounds_dir.display().to_string()),
            ("master_volume", format!("{:.2}", self.master_volume)),
            ("enabled", self.enabled.to_string()),
            ("auto_start", self.auto_start.to_string()),
            ("spatial_audio", self.spatial_audio.to_string()),
            ("per_key_feel", self.per_key_feel.to_string()),
            (
                "home_row_softness",
                format!("{:.2}", self.home_row_softness),
            ),
            (
                "volume_normalization",
                self.volume_normalization.to_string(),
            ),
            ("mute_modifiers", self.mute_modifiers.to_string()),
            ("tone_lpf", format!("{:.2}", self.tone_lpf)),
            ("tone_pitch", format!("{:.2}", self.tone_pitch)),
            ("mouse_tone_lpf", format!("{:.2}", self.mouse_tone_lpf)),
            ("mouse_tone_pitch", format!("{:.2}", self.mouse_tone_pitch)),
            ("mouse_volume", format!("{:.2}", self.mouse_volume)),
            ("enter_tone_lpf", format!("{:.2}", self.enter_tone_lpf)),
            ("enter_tone_pitch", format!("{:.2}", self.enter_tone_pitch)),
            ("mouse_sound", self.mouse_sound.clone()),
            ("favorites", self.favorites.clone()),
            ("hover_preview", self.hover_preview.to_string()),
            ("enter_sound", self.enter_sound.clone()),
            ("enter_volume", format!("{:.2}", self.enter_volume)),
            ("visualizer_enabled", self.visualizer_enabled.to_string()),
            (
                "visualizer_timeout",
                format!("{:.2}", self.visualizer_timeout),
            ),
            ("visualizer_scale", format!("{:.2}", self.visualizer_scale)),
            (
                "visualizer_follow_cursor",
                self.visualizer_follow_cursor.to_string(),
            ),
            ("visualizer_position", self.visualizer_position.clone()),
            (
                "visualizer_cursor_gap",
                format!("{:.0}", self.visualizer_cursor_gap),
            ),
            ("visualizer_glow", self.visualizer_glow.clone()),
            ("visualizer_frame", self.visualizer_frame.clone()),
            ("visualizer_keycap", self.visualizer_keycap.clone()),
            (
                "notch_overlay_enabled",
                self.notch_overlay_enabled.to_string(),
            ),
            (
                "notch_overlay_auto_hide",
                self.notch_overlay_auto_hide.to_string(),
            ),
            ("show_lizard_switch", self.show_lizard_switch.to_string()),
            (
                "show_faah_enter_sound",
                self.show_faah_enter_sound.to_string(),
            ),
            ("ui_port", self.ui_port.to_string()),
        ]
    }
}

#[must_use]
pub fn default_config_path() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME").map_or_else(
        || home_dir().join(".config/keebyd/config.conf"),
        |path| PathBuf::from(path).join("keebyd/config.conf"),
    )
}

fn default_data_dir() -> PathBuf {
    env::var_os("XDG_DATA_HOME").map_or_else(|| home_dir().join(".local/share"), PathBuf::from)
}

fn home_dir() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

fn parse_value<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, ConfigError> {
    value.parse().map_err(|_| invalid_value(key, value))
}

fn parse_range(
    key: &str,
    value: &str,
    range: std::ops::RangeInclusive<f32>,
) -> Result<f32, ConfigError> {
    let parsed: f32 = parse_value(key, value)?;
    if parsed.is_finite() && range.contains(&parsed) {
        Ok(parsed)
    } else {
        Err(invalid_value(key, value))
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, ConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(invalid_value(key, value)),
    }
}

fn invalid_value(key: &str, value: &str) -> ConfigError {
    ConfigError::InvalidValue {
        key: key.into(),
        value: value.into(),
    }
}
