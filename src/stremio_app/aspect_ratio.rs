use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::stremio_app::{
    stremio_player::{BoolProp, FpProp, InMsg, InMsgArgs, InMsgFn, PropKey, PropVal, StrProp},
    window_helper,
};
use flume::Sender;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| {
    env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("StremioBorderBreaker")
});

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AspectMode {
    AutoDetect,
    FillCrop,
    FitToScreen,
    Ratio16x9,
    Ratio4x3,
    Ratio1x1,
    Ratio21x9,
    Ratio32x9,
    Cinema,
}

impl AspectMode {
    pub fn display_name(self) -> &'static str {
        match self {
            AspectMode::AutoDetect => "Auto",
            AspectMode::FillCrop => "Fill (Crop)",
            AspectMode::FitToScreen => "Fit to Screen",
            AspectMode::Ratio16x9 => "16:9",
            AspectMode::Ratio4x3 => "4:3",
            AspectMode::Ratio1x1 => "1:1",
            AspectMode::Ratio21x9 => "21:9 Ultrawide",
            AspectMode::Ratio32x9 => "32:9 Super Ultrawide",
            AspectMode::Cinema => "Cinema",
        }
    }

    pub fn overlay_label(self, display_ratio: f32) -> String {
        match self {
            AspectMode::AutoDetect => {
                format!("Auto ({:.2}:1)", display_ratio.max(0.01))
            }
            mode => mode.display_name().to_string(),
        }
    }
}

struct AspectSpec {
    aspect_override: Option<f64>,
    keep_aspect: bool,
    panscan: f64,
    video_unscaled: Option<&'static str>,
}

impl AspectMode {
    fn spec(self, display_ratio: f32) -> AspectSpec {
        match self {
            AspectMode::AutoDetect => AspectSpec {
                aspect_override: Some(display_ratio.max(0.1) as f64),
                keep_aspect: true,
                panscan: 0.0,
                video_unscaled: Some("no"),
            },
            AspectMode::FillCrop => AspectSpec {
                aspect_override: None,
                keep_aspect: true,
                panscan: 1.0,
                video_unscaled: Some("no"),
            },
            AspectMode::FitToScreen => AspectSpec {
                aspect_override: None,
                keep_aspect: true,
                panscan: 0.0,
                video_unscaled: Some("no"),
            },
            AspectMode::Ratio16x9 => AspectSpec::ratio(16.0 / 9.0),
            AspectMode::Ratio4x3 => AspectSpec::ratio(4.0 / 3.0),
            AspectMode::Ratio1x1 => AspectSpec::ratio(1.0),
            AspectMode::Ratio21x9 => AspectSpec::ratio(21.0 / 9.0),
            AspectMode::Ratio32x9 => AspectSpec::ratio(32.0 / 9.0),
            AspectMode::Cinema => AspectSpec::ratio(2.39),
        }
    }
}

impl AspectSpec {
    fn ratio(value: f64) -> Self {
        AspectSpec {
            aspect_override: Some(value),
            keep_aspect: true,
            panscan: 0.0,
            video_unscaled: Some("no"),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AspectConfig {
    mode: AspectMode,
}

pub struct AspectController {
    config_path: PathBuf,
    order: Vec<AspectMode>,
    current_index: usize,
    display_ratio: f32,
}

impl AspectController {
    pub fn new() -> Self {
        Self::with_paths(
            CONFIG_DIR.join("aspect.json"),
            window_helper::primary_monitor_ratio(),
        )
    }

    fn with_paths(config_path: PathBuf, display_ratio: f32) -> Self {
        let order = vec![
            AspectMode::AutoDetect,
            AspectMode::FillCrop,
            AspectMode::FitToScreen,
            AspectMode::Ratio16x9,
            AspectMode::Ratio4x3,
            AspectMode::Ratio1x1,
            AspectMode::Ratio21x9,
            AspectMode::Ratio32x9,
            AspectMode::Cinema,
        ];
        let saved_mode = Self::read_config(&config_path).map(|c| c.mode);
        let current_index = saved_mode
            .and_then(|mode| order.iter().position(|m| m == &mode))
            .unwrap_or(0);
        AspectController {
            config_path,
            order,
            current_index,
            display_ratio,
        }
    }

    pub fn current_mode(&self) -> AspectMode {
        self.order[self.current_index]
    }

    pub fn cycle(&mut self) -> AspectMode {
        self.current_index = (self.current_index + 1) % self.order.len();
        self.persist();
        self.current_mode()
    }

    pub fn apply_current(&self, player_tx: &Sender<String>) {
        self.apply_mode(player_tx, self.current_mode());
    }

    pub fn apply_mode(&self, player_tx: &Sender<String>, mode: AspectMode) {
        let spec = mode.spec(self.display_ratio);
        if let Some(ratio) = spec.aspect_override {
            send_fp_prop(player_tx, FpProp::VideoAspectOverride, ratio);
        } else {
            send_fp_prop(player_tx, FpProp::VideoAspectOverride, 0.0);
        }
        send_bool_prop(player_tx, BoolProp::Keepaspect, spec.keep_aspect);
        send_fp_prop(player_tx, FpProp::Panscan, spec.panscan);
        if let Some(value) = spec.video_unscaled {
            send_str_prop(player_tx, StrProp::VideoUnscaled, value);
        }
    }

    pub fn display_ratio(&self) -> f32 {
        self.display_ratio
    }

    fn persist(&self) {
        if let Some(parent) = self.config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let config = AspectConfig {
            mode: self.current_mode(),
        };
        if let Ok(data) = serde_json::to_vec(&config) {
            if let Ok(mut file) = File::create(&self.config_path) {
                let _ = file.write_all(&data);
            }
        }
    }

    fn read_config(path: &Path) -> Option<AspectConfig> {
        let mut file = File::open(path).ok()?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        serde_json::from_slice(&buf).ok()
    }
}

impl Default for AspectController {
    fn default() -> Self {
        Self::new()
    }
}

fn send_fp_prop(player_tx: &Sender<String>, prop: FpProp, value: f64) {
    let msg = InMsg(
        InMsgFn::MpvSetProp,
        InMsgArgs::StProp(PropKey::Fp(prop), PropVal::Num(value)),
    );
    if let Ok(serialized) = serde_json::to_string(&msg) {
        let _ = player_tx.send(serialized);
    }
}

fn send_bool_prop(player_tx: &Sender<String>, prop: BoolProp, value: bool) {
    let msg = InMsg(
        InMsgFn::MpvSetProp,
        InMsgArgs::StProp(PropKey::Bool(prop), PropVal::Bool(value)),
    );
    if let Ok(serialized) = serde_json::to_string(&msg) {
        let _ = player_tx.send(serialized);
    }
}

fn send_str_prop(player_tx: &Sender<String>, prop: StrProp, value: &str) {
    let msg = InMsg(
        InMsgFn::MpvSetProp,
        InMsgArgs::StProp(PropKey::Str(prop), PropVal::Str(value.to_string())),
    );
    if let Ok(serialized) = serde_json::to_string(&msg) {
        let _ = player_tx.send(serialized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{distributions::Alphanumeric, Rng};
    use std::fs;

    fn temp_config_path() -> PathBuf {
        let mut name: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        name.push_str(".json");
        env::temp_dir().join(name)
    }

    #[test]
    fn cycles_modes_and_persists() {
        let path = temp_config_path();
        let mut controller = AspectController::with_paths(path.clone(), 2.33);
        assert_eq!(controller.current_mode(), AspectMode::AutoDetect);
        controller.cycle();
        assert_eq!(controller.current_mode(), AspectMode::FillCrop);
        controller.cycle();
        assert_eq!(controller.current_mode(), AspectMode::FitToScreen);
        // ensure persisted
        let loaded = AspectController::with_paths(path.clone(), 2.33);
        assert_eq!(loaded.current_mode(), AspectMode::FitToScreen);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn overlay_labels_match_modes() {
        let ratio = 21.0 / 9.0;
        assert_eq!(
            AspectMode::AutoDetect.overlay_label(ratio),
            format!("Auto ({:.2}:1)", ratio)
        );
        assert_eq!(AspectMode::Ratio21x9.overlay_label(ratio), "21:9 Ultrawide");
        assert_eq!(AspectMode::Cinema.overlay_label(ratio), "Cinema");
    }
}
