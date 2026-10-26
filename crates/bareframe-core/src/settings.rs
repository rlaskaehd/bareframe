//! Appearance settings read from `~/.config/bareframe/config` (`key = value`, like Ghostty).
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 6.0..=72.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemeName,
    /// Ghostty's built-in font when absent.
    pub font_family: Option<String>,
    pub font_size: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeName::default(),
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl Settings {
    /// Invalid lines keep the default and are reported, so a typo never blocks startup.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut warnings = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("line {}: expected key = value", number + 1));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "theme" => match value.to_ascii_lowercase().as_str() {
                    "dark" => settings.theme = ThemeName::Dark,
                    "light" => settings.theme = ThemeName::Light,
                    _ => warnings.push(format!("line {}: unknown theme {value:?}", number + 1)),
                },
                "font-family" if !value.is_empty() => settings.font_family = Some(value.into()),
                "font-family" => settings.font_family = None,
                "font-size" => match value.parse::<f32>() {
                    Ok(size) if FONT_SIZE_RANGE.contains(&size) => settings.font_size = size,
                    _ => warnings.push(format!(
                        "line {}: font-size must be a number from {} to {}",
                        number + 1,
                        FONT_SIZE_RANGE.start(),
                        FONT_SIZE_RANGE.end()
                    )),
                },
                _ => warnings.push(format!("line {}: unknown key {key:?}", number + 1)),
            }
        }
        (settings, warnings)
    }

    /// Returns the size after `steps` points, clamped to the supported range.
    pub fn step_font_size(&mut self, steps: f32) -> f32 {
        self.font_size =
            (self.font_size + steps).clamp(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end());
        self.font_size
    }
}

pub fn config_path(xdg_config_home: Option<&Path>, home: &Path) -> PathBuf {
    xdg_config_home
        .filter(|path| path.is_absolute())
        .map_or_else(|| home.join(".config"), Path::to_path_buf)
        .join("bareframe/config")
}

/// Where the window layout is saved, next to the config file.
pub fn state_path(xdg_config_home: Option<&Path>, home: &Path) -> PathBuf {
    config_path(xdg_config_home, home).with_file_name("state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_keys_apply_and_bad_lines_keep_defaults() {
        let (settings, warnings) = Settings::parse(
            "# comment\ntheme = Light\nfont-family = JetBrains Mono\nfont-size = 99\nbogus\ncolor = red\n",
        );
        assert_eq!(settings.theme, ThemeName::Light);
        assert_eq!(settings.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(settings.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].starts_with("line 4:"));
    }

    #[test]
    fn font_size_steps_are_clamped() {
        let mut settings = Settings::default();
        assert_eq!(settings.step_font_size(1.0), 14.0);
        assert_eq!(settings.step_font_size(-100.0), 6.0);
    }

    #[test]
    fn config_path_prefers_absolute_xdg_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            config_path(None, home),
            Path::new("/Users/me/.config/bareframe/config")
        );
        assert_eq!(
            config_path(Some(Path::new("relative")), home),
            Path::new("/Users/me/.config/bareframe/config")
        );
        assert_eq!(
            config_path(Some(Path::new("/xdg")), home),
            Path::new("/xdg/bareframe/config")
        );
    }
}
