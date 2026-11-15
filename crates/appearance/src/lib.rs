//! User-requested system and toolkit appearance settings.
mod environment;
mod files;
pub mod packs;
mod system_theme;
pub mod text_size;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemThemeMode {
    Light,
    Dark,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeTargetStatus {
    Detected,
    Applied,
    Unchanged,
    Unavailable,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ThemeTargetResult {
    pub name: String,
    pub status: ThemeTargetStatus,
    pub detail: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SystemThemeState {
    pub mode: Option<SystemThemeMode>,
    pub targets: Vec<ThemeTargetResult>,
}

pub use system_theme::{APPLY_TO_SYSTEM, DesktopTheme};
