use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use bevy::{
    prelude::*,
    window::{Monitor, MonitorSelection, PresentMode, VideoMode, VideoModeSelection, WindowMode},
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "Game";
const APPLICATION: &str = "Game";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Resource, Debug, Clone)]
pub(crate) struct ClientSettingsStore {
    path: PathBuf,
}

impl ClientSettingsStore {
    pub(crate) fn platform_default() -> Result<Self> {
        let project_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .context("could not resolve the platform config directory")?;
        Ok(Self::new(project_dirs.config_dir().join(SETTINGS_FILE)))
    }

    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub(crate) fn load(&self) -> Result<ClientSettings> {
        if !self.path.exists() {
            return Ok(ClientSettings::default());
        }

        let json = fs::read_to_string(&self.path)
            .with_context(|| format!("could not read settings {}", self.path.display()))?;
        let settings = serde_json::from_str::<ClientSettings>(&json)
            .with_context(|| format!("could not parse settings {}", self.path.display()))?;
        Ok(settings.sanitized())
    }

    pub(crate) fn save(&self, settings: &ClientSettings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("could not create settings directory {}", parent.display())
            })?;
        }
        let json = serde_json::to_string_pretty(&settings.clone().sanitized())
            .context("could not serialize client settings")?;
        fs::write(&self.path, json)
            .with_context(|| format!("could not write settings {}", self.path.display()))
    }

    #[cfg(test)]
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ClientSettings {
    #[serde(default)]
    pub(crate) display: DisplaySettings,
    #[serde(default)]
    pub(crate) audio: AudioSettings,
    #[serde(default)]
    pub(crate) input: InputSettings,
    #[serde(default)]
    pub(crate) hud: HudSettings,
}

impl ClientSettings {
    pub(crate) fn sanitized(mut self) -> Self {
        self.display.resolution = self.display.resolution.sanitized();
        self.audio.music_volume = self.audio.music_volume.clamp(0.0, 1.0);
        self.audio.ui_volume = self.audio.ui_volume.clamp(0.0, 1.0);
        self.input.mouse_sensitivity = self.input.mouse_sensitivity.clamp(0.25, 3.0);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DisplaySettings {
    #[serde(default)]
    pub(crate) mode: DisplayMode,
    #[serde(default = "default_resolution")]
    pub(crate) resolution: DisplayResolution,
    #[serde(default = "default_vsync")]
    pub(crate) vsync: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            mode: DisplayMode::Windowed,
            resolution: DisplayResolution::new(1280, 720),
            vsync: true,
        }
    }
}

impl DisplaySettings {
    pub(crate) fn present_mode(self) -> PresentMode {
        if self.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        }
    }

    pub(crate) fn window_mode(self, monitor: Option<&Monitor>) -> WindowMode {
        match self.mode {
            DisplayMode::Windowed => WindowMode::Windowed,
            DisplayMode::BorderlessFullscreen => {
                WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
            }
            DisplayMode::Fullscreen => WindowMode::Fullscreen(
                MonitorSelection::Primary,
                best_video_mode(monitor, self.resolution)
                    .map(VideoModeSelection::Specific)
                    .unwrap_or(VideoModeSelection::Current),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DisplayMode {
    #[default]
    Windowed,
    BorderlessFullscreen,
    Fullscreen,
}

impl DisplayMode {
    pub(crate) const ALL: [Self; 3] =
        [Self::Windowed, Self::BorderlessFullscreen, Self::Fullscreen];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::BorderlessFullscreen => "Borderless Fullscreen",
            Self::Fullscreen => "Fullscreen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DisplayResolution {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DisplayResolution {
    pub(crate) const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub(crate) fn label(self) -> String {
        format!("{} x {}", self.width, self.height)
    }

    fn sanitized(self) -> Self {
        if self.width < 640 || self.height < 360 {
            default_resolution()
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct AudioSettings {
    #[serde(default = "default_volume")]
    pub(crate) music_volume: f32,
    #[serde(default = "default_volume")]
    pub(crate) ui_volume: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            music_volume: 1.0,
            ui_volume: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct InputSettings {
    #[serde(default = "default_mouse_sensitivity")]
    pub(crate) mouse_sensitivity: f32,
    #[serde(default)]
    pub(crate) invert_mouse_y: bool,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            invert_mouse_y: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HudSettings {
    #[serde(default = "default_show_fps")]
    pub(crate) show_fps: bool,
}

impl Default for HudSettings {
    fn default() -> Self {
        Self { show_fps: true }
    }
}

const FALLBACK_RESOLUTIONS: [DisplayResolution; 6] = [
    DisplayResolution::new(1280, 720),
    DisplayResolution::new(1366, 768),
    DisplayResolution::new(1600, 900),
    DisplayResolution::new(1920, 1080),
    DisplayResolution::new(2560, 1440),
    DisplayResolution::new(3840, 2160),
];

fn default_resolution() -> DisplayResolution {
    DisplayResolution::new(1280, 720)
}

fn default_vsync() -> bool {
    true
}

fn default_volume() -> f32 {
    1.0
}

fn default_mouse_sensitivity() -> f32 {
    1.0
}

fn default_show_fps() -> bool {
    true
}

pub(crate) fn display_resolutions(
    monitor: Option<&Monitor>,
    display_mode: DisplayMode,
) -> Vec<DisplayResolution> {
    let mut resolutions = monitor
        .map(|monitor| {
            monitor
                .video_modes
                .iter()
                .map(video_mode_resolution)
                .filter(|resolution| {
                    display_mode != DisplayMode::Fullscreen
                        || resolution_matches_monitor_aspect(monitor, *resolution)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if resolutions.is_empty() {
        resolutions.extend(FALLBACK_RESOLUTIONS);
        if let Some(monitor) = monitor {
            resolutions.push(DisplayResolution::new(
                monitor.physical_width,
                monitor.physical_height,
            ));
            if display_mode == DisplayMode::Fullscreen {
                resolutions
                    .retain(|resolution| resolution_matches_monitor_aspect(monitor, *resolution));
            }
        }
    }

    resolutions.sort_by_key(|resolution| {
        (
            u64::from(resolution.width) * u64::from(resolution.height),
            resolution.width,
            resolution.height,
        )
    });
    resolutions.dedup();
    resolutions
}

fn best_video_mode(monitor: Option<&Monitor>, resolution: DisplayResolution) -> Option<VideoMode> {
    let monitor = monitor?;
    if !resolution_matches_monitor_aspect(monitor, resolution) {
        return None;
    }

    monitor
        .video_modes
        .iter()
        .copied()
        .filter(|mode| video_mode_resolution(mode) == resolution)
        .max_by_key(|mode| (mode.refresh_rate_millihertz, mode.bit_depth))
}

fn video_mode_resolution(mode: &VideoMode) -> DisplayResolution {
    DisplayResolution::new(mode.physical_size.x, mode.physical_size.y)
}

fn resolution_matches_monitor_aspect(monitor: &Monitor, resolution: DisplayResolution) -> bool {
    if monitor.physical_width == 0 || monitor.physical_height == 0 || resolution.height == 0 {
        return false;
    }

    let monitor_aspect = monitor.physical_width as f32 / monitor.physical_height as f32;
    let resolution_aspect = resolution.width as f32 / resolution.height as f32;
    (monitor_aspect - resolution_aspect).abs() <= 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(video_modes: Vec<VideoMode>) -> Monitor {
        monitor_with_size(1920, 1080, video_modes)
    }

    fn monitor_with_size(width: u32, height: u32, video_modes: Vec<VideoMode>) -> Monitor {
        Monitor {
            name: Some("Display".to_owned()),
            physical_width: width,
            physical_height: height,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 1.0,
            video_modes,
        }
    }

    #[test]
    fn default_settings_match_startup_window() {
        let settings = ClientSettings::default();

        assert_eq!(settings.display.mode, DisplayMode::Windowed);
        assert_eq!(
            settings.display.resolution,
            DisplayResolution::new(1280, 720)
        );
        assert_eq!(settings.display.present_mode(), PresentMode::AutoVsync);
        assert!(settings.hud.show_fps);
    }

    #[test]
    fn settings_store_round_trips_json() {
        let root = std::env::temp_dir().join(format!("game-settings-{}", uuid::Uuid::new_v4()));
        let store = ClientSettingsStore::new(root.join("settings.json"));
        let mut settings = ClientSettings::default();
        settings.display.mode = DisplayMode::BorderlessFullscreen;
        settings.audio.music_volume = 0.42;
        settings.input.invert_mouse_y = true;

        store.save(&settings).expect("settings should save");
        let loaded = store.load().expect("settings should load");

        assert_eq!(loaded.display.mode, DisplayMode::BorderlessFullscreen);
        assert_eq!(loaded.audio.music_volume, 0.42);
        assert!(loaded.input.invert_mouse_y);
        assert!(store.path().exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loaded_settings_are_sanitized() {
        let settings = ClientSettings {
            display: DisplaySettings {
                resolution: DisplayResolution::new(1, 1),
                ..Default::default()
            },
            audio: AudioSettings {
                music_volume: 2.0,
                ui_volume: -1.0,
            },
            input: InputSettings {
                mouse_sensitivity: 20.0,
                invert_mouse_y: false,
            },
            hud: HudSettings::default(),
        }
        .sanitized();

        assert_eq!(settings.display.resolution, default_resolution());
        assert_eq!(settings.audio.music_volume, 1.0);
        assert_eq!(settings.audio.ui_volume, 0.0);
        assert_eq!(settings.input.mouse_sensitivity, 3.0);
    }

    #[test]
    fn display_resolutions_use_monitor_modes_when_available() {
        let monitor = monitor(vec![
            VideoMode {
                physical_size: UVec2::new(1920, 1080),
                bit_depth: 24,
                refresh_rate_millihertz: 60_000,
            },
            VideoMode {
                physical_size: UVec2::new(1280, 720),
                bit_depth: 24,
                refresh_rate_millihertz: 60_000,
            },
            VideoMode {
                physical_size: UVec2::new(1920, 1080),
                bit_depth: 24,
                refresh_rate_millihertz: 120_000,
            },
        ]);

        assert_eq!(
            display_resolutions(Some(&monitor), DisplayMode::Windowed),
            vec![
                DisplayResolution::new(1280, 720),
                DisplayResolution::new(1920, 1080),
            ]
        );
    }

    #[test]
    fn exclusive_fullscreen_resolutions_match_monitor_aspect_ratio() {
        let monitor = monitor_with_size(
            5120,
            2880,
            vec![
                VideoMode {
                    physical_size: UVec2::new(5120, 2880),
                    bit_depth: 30,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(2560, 1440),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(2048, 1080),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(1920, 1200),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
            ],
        );

        assert_eq!(
            display_resolutions(Some(&monitor), DisplayMode::Fullscreen),
            vec![
                DisplayResolution::new(1920, 1080),
                DisplayResolution::new(2560, 1440),
                DisplayResolution::new(5120, 2880),
            ]
        );
    }

    #[test]
    fn exclusive_fullscreen_prefers_best_matching_video_mode() {
        let monitor = monitor(vec![
            VideoMode {
                physical_size: UVec2::new(1920, 1080),
                bit_depth: 24,
                refresh_rate_millihertz: 60_000,
            },
            VideoMode {
                physical_size: UVec2::new(1920, 1080),
                bit_depth: 30,
                refresh_rate_millihertz: 120_000,
            },
        ]);
        let settings = DisplaySettings {
            mode: DisplayMode::Fullscreen,
            resolution: DisplayResolution::new(1920, 1080),
            vsync: true,
        };

        assert_eq!(
            settings.window_mode(Some(&monitor)),
            WindowMode::Fullscreen(
                MonitorSelection::Primary,
                VideoModeSelection::Specific(VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 30,
                    refresh_rate_millihertz: 120_000,
                })
            )
        );
    }
}
