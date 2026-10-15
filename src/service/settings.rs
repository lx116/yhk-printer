//! Owns printer settings, defaults, legacy configuration migration, and persistence.

use crate::{
    imaging::{ContentKind, Definition},
    protocol::PrintMode,
    typeset::{Align, Font},
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_MM: f32 = 30.0;
const MAX_CARD_MARGIN_MM: f32 = 24.0;
const MIN_BACK_OFFSET_MM: f32 = -5.0;
const MAX_BACK_OFFSET_MM: f32 = 5.0;
const DOTS_PER_MM: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
    Custom(u8),
}

impl Quality {
    pub const fn from_density(density: u8) -> Self {
        match density {
            20 => Self::Low,
            25 => Self::Medium,
            30 => Self::High,
            value => Self::Custom(value),
        }
    }

    pub const fn density(self) -> u8 {
        match self {
            Self::Low => 20,
            Self::Medium => 25,
            Self::High => 30,
            Self::Custom(value) => value,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Custom(_) => "Custom",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BatchMode {
    #[default]
    Advance,
    Pause,
    Roll,
}

impl BatchMode {
    /// Returns the next selectable batch mode.
    pub const fn next(self) -> Self {
        match self {
            Self::Advance => Self::Pause,
            Self::Pause => Self::Roll,
            Self::Roll => Self::Advance,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Flip {
    #[default]
    Book,
    Calendar,
}

impl<'de> Deserialize<'de> for BatchMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match String::deserialize(deserializer)?.as_str() {
            "pause" => Self::Pause,
            "roll" => Self::Roll,
            _ => Self::Advance,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default)]
    pub mode: PrintMode,
    #[serde(default)]
    pub batch_mode: BatchMode,
    #[serde(default, skip_serializing)]
    pub gap_rows: u16,
    #[serde(default = "default_gap_mm")]
    pub gap_mm: f32,
    #[serde(default = "default_cut_marks")]
    pub cut_marks: bool,
    #[serde(default)]
    pub lead_mm: f32,
    #[serde(default)]
    pub content: ContentKind,
    #[serde(default)]
    pub definition: Definition,
    #[serde(default)]
    pub doc_font: Font,
    #[serde(default = "default_doc_size")]
    pub doc_size_pt: f32,
    #[serde(default)]
    pub doc_align: Align,
    #[serde(default = "default_doc_margin")]
    pub doc_margin: u16,
    #[serde(default)]
    pub flip: Flip,
    #[serde(default = "default_card_margin_mm")]
    pub card_margin_mm: f32,
    #[serde(default)]
    pub back_offset_mm: f32,
    #[serde(default)]
    pub frame: bool,
    #[serde(default = "default_min_pt")]
    pub min_pt: f32,
    pub density: u8,
    pub chunk: usize,
    pub delay_ms: u64,
    pub row_delay_ms: u64,
    pub acked: bool,
    pub name_prefix: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: PrintMode::Rows,
            batch_mode: BatchMode::Advance,
            gap_rows: 0,
            gap_mm: default_gap_mm(),
            cut_marks: default_cut_marks(),
            lead_mm: 0.0,
            density: 25,
            content: ContentKind::Auto,
            definition: Definition::Standard,
            doc_font: Font::Sans,
            doc_size_pt: default_doc_size(),
            doc_align: Align::Left,
            doc_margin: default_doc_margin(),
            flip: Flip::Book,
            card_margin_mm: default_card_margin_mm(),
            back_offset_mm: 0.0,
            frame: false,
            min_pt: default_min_pt(),
            chunk: 20,
            delay_ms: 20,
            row_delay_ms: 0,
            acked: true,
            name_prefix: "YHK-".into(),
        }
    }
}

impl Settings {
    pub fn gap_rows(&self) -> u16 {
        rows_from_mm(self.gap_mm)
    }
    pub fn lead_rows(&self) -> u16 {
        rows_from_mm(self.lead_mm)
    }
}

fn rows_from_mm(mm: f32) -> u16 {
    (mm.clamp(0.0, MAX_MM) * DOTS_PER_MM).round() as u16
}
const fn default_gap_mm() -> f32 {
    5.0
}
const fn default_cut_marks() -> bool {
    true
}
const fn default_doc_size() -> f32 {
    10.0
}
const fn default_doc_margin() -> u16 {
    12
}
const fn default_card_margin_mm() -> f32 {
    4.0
}
const fn default_min_pt() -> f32 {
    7.0
}

pub(crate) fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "yhkp", "yhkp").map(|dirs| dirs.config_dir().join("config.toml"))
}

pub fn saved_settings() -> Settings {
    load_settings(config_path().as_deref())
}

pub(super) fn load_settings(path: Option<&Path>) -> Settings {
    let Some(text) = path.and_then(|path| std::fs::read_to_string(path).ok()) else {
        return Settings::default();
    };
    let mut settings: Settings = toml::from_str(&text).unwrap_or_default();
    if !text
        .lines()
        .any(|line| line.trim_start().starts_with("gap_mm"))
    {
        settings.gap_mm = f32::from(settings.gap_rows) / DOTS_PER_MM;
    }
    clamp_settings(&mut settings);
    settings
}

fn clamp_settings(settings: &mut Settings) {
    settings.gap_mm = settings.gap_mm.clamp(0.0, MAX_MM);
    settings.lead_mm = settings.lead_mm.clamp(0.0, MAX_MM);
    settings.card_margin_mm = settings.card_margin_mm.clamp(0.0, MAX_CARD_MARGIN_MM);
    settings.back_offset_mm = settings
        .back_offset_mm
        .clamp(MIN_BACK_OFFSET_MM, MAX_BACK_OFFSET_MM);
    settings.min_pt = settings.min_pt.max(default_min_pt());
}

pub(super) fn save_settings(path: Option<&Path>, settings: &Settings) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string(settings) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::temp_settings_path;

    #[test]
    fn settings_default_and_persisted_round_trip() {
        let path = temp_settings_path();
        assert_eq!(load_settings(Some(&path)).mode, PrintMode::Rows);
        let settings = Settings {
            mode: PrintMode::Gray,
            batch_mode: BatchMode::Roll,
            gap_rows: 12,
            gap_mm: 1.5,
            cut_marks: true,
            lead_mm: 0.0,
            content: ContentKind::Text,
            definition: Definition::Hd,
            doc_font: Font::Sans,
            doc_size_pt: 10.0,
            doc_align: Align::Left,
            doc_margin: 12,
            flip: Flip::Calendar,
            card_margin_mm: 8.0,
            back_offset_mm: 3.0,
            frame: true,
            min_pt: 8.0,
            density: 4,
            chunk: 64,
            delay_ms: 3,
            row_delay_ms: 2,
            acked: false,
            name_prefix: "TEST-".into(),
        };
        save_settings(Some(&path), &settings);
        let loaded = load_settings(Some(&path));
        assert_eq!(loaded.mode, settings.mode);
        assert_eq!(loaded.batch_mode, settings.batch_mode);
        assert_eq!(loaded.density, settings.density);
        assert_eq!(loaded.content, ContentKind::Text);
        assert_eq!(loaded.definition, Definition::Hd);
        assert_eq!(loaded.flip, Flip::Calendar);
        assert_eq!(loaded.card_margin_mm, 8.0);
        assert_eq!(loaded.back_offset_mm, 3.0);
        assert!(loaded.frame);
        assert_eq!(loaded.min_pt, 8.0);
        assert_eq!(loaded.chunk, settings.chunk);
        assert_eq!(loaded.delay_ms, settings.delay_ms);
        assert_eq!(loaded.row_delay_ms, settings.row_delay_ms);
        assert_eq!(loaded.acked, settings.acked);
        assert_eq!(loaded.name_prefix, settings.name_prefix);
        let missing = toml::to_string(&settings)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("mode ="))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, missing).unwrap();
        assert_eq!(load_settings(Some(&path)).mode, PrintMode::Rows);
        assert_eq!(load_settings(Some(&path)).batch_mode, BatchMode::Roll);
        assert_eq!(load_settings(Some(&path)).gap_mm, 1.5);
        let missing_batch_mode = toml::to_string(&settings)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("batch_mode ="))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, missing_batch_mode).unwrap();
        assert_eq!(load_settings(Some(&path)).batch_mode, BatchMode::Advance);
        let legacy_density = toml::to_string(&settings)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("content =") && !line.starts_with("definition ="))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, legacy_density).unwrap();
        let legacy_settings = load_settings(Some(&path));
        assert_eq!(legacy_settings.density, 4);
        assert_eq!(legacy_settings.content, ContentKind::Auto);
        assert_eq!(legacy_settings.definition, Definition::Standard);
        let legacy_without_batch_fields = toml::to_string(&settings)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("batch_mode =") && !line.starts_with("gap_rows ="))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, legacy_without_batch_fields).unwrap();
        let legacy_settings = load_settings(Some(&path));
        assert_eq!(legacy_settings.batch_mode, BatchMode::Advance);
        assert_eq!(legacy_settings.gap_mm, 1.5);
        let legacy = toml::to_string(&settings)
            .unwrap()
            .replace("gray", "raster");
        std::fs::write(&path, legacy).unwrap();
        assert_eq!(load_settings(Some(&path)).mode, PrintMode::Raster);
        let unknown = toml::to_string(&settings)
            .unwrap()
            .replace("gray", "unknown");
        std::fs::write(&path, unknown).unwrap();
        assert_eq!(load_settings(Some(&path)).mode, PrintMode::Rows);
        let unknown_batch_mode = toml::to_string(&settings)
            .unwrap()
            .replace("roll", "unknown");
        std::fs::write(&path, unknown_batch_mode).unwrap();
        assert_eq!(load_settings(Some(&path)).batch_mode, BatchMode::Advance);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quality_maps_vendor_levels_and_preserves_custom_density() {
        assert_eq!(Quality::from_density(20), Quality::Low);
        assert_eq!(Quality::from_density(25), Quality::Medium);
        assert_eq!(Quality::from_density(30), Quality::High);
        assert_eq!(Quality::from_density(10), Quality::Custom(10));
        assert_eq!(Quality::High.density(), 30);
    }

    #[test]
    fn raw_density_only_config_keeps_legacy_override() {
        let path = temp_settings_path();
        std::fs::write(&path, "density = 10\n").unwrap();
        let settings = load_settings(Some(&path));
        assert_eq!(settings.density, 10);
        assert_eq!(settings.content, ContentKind::Auto);
        assert_eq!(settings.definition, Definition::Standard);
        assert_eq!(settings.mode, PrintMode::Rows);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_gap_rows_converts_to_millimetres() {
        let path = temp_settings_path();
        std::fs::write(&path, "gap_rows = 12\n").unwrap();
        assert_eq!(load_settings(Some(&path)).gap_mm, 1.5);
        let _ = std::fs::remove_file(path);
    }
}
