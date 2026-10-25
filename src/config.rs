//! The settings, from `config.toml`: `ONSEN_MATRIX_CONFIG` if set, else
//! `~/.config/onsen-matrix/config.toml`. A setting left out keeps its
//! default, which is what the `config.toml` in the repository shows. The
//! command line overrides the file.

use bevy::prelude::*;
use serde::Deserialize;

#[derive(Resource, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub run: Run,
    pub camera: Camera,
    pub rain: Rain,
    pub droplet: Droplet,
    pub glyph: Glyph,
    pub media: Media,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Run {
    pub fps: u32,
    pub limit: f32,
    pub outputs: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Camera {
    pub walk: f32,
    pub rise: f32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Rain {
    pub droplets: usize,
    pub rate: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Droplet {
    pub fall: f32,
    pub spawn: f32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Glyph {
    pub size: f32,
    pub life: f32,
    pub tick: f32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Media {
    pub show: bool,
    pub size: f32,
    pub margin: f32,
}

impl Default for Run {
    fn default() -> Self {
        Run { fps: 60, limit: 30.0, outputs: vec![], exclude: vec![] }
    }
}
impl Default for Camera {
    fn default() -> Self {
        Camera { walk: 0.2, rise: 0.0 }
    }
}
impl Default for Rain {
    fn default() -> Self {
        Rain { droplets: 1120, rate: 40.0, near: 0.3, far: 14.0 }
    }
}
impl Default for Droplet {
    fn default() -> Self {
        Droplet { fall: 0.28125, spawn: 0.125 }
    }
}
impl Default for Glyph {
    fn default() -> Self {
        Glyph { size: 0.03125, life: 10.0, tick: 0.75 }
    }
}
impl Default for Media {
    fn default() -> Self {
        Media { show: true, size: 0.043, margin: 0.05 }
    }
}

impl Config {
    /// The file's settings over the defaults; the defaults alone when there
    /// is no file, and when the file does not parse, which is said.
    pub fn load() -> Self {
        let path = std::env::var("ONSEN_MATRIX_CONFIG").map(std::path::PathBuf::from).unwrap_or_else(|_| {
            std::env::var("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
                .join("onsen-matrix/config.toml")
        });
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}: {e}; using the defaults", path.display());
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_is_the_defaults() {
        let shipped: Config = toml::from_str(include_str!("../config.toml")).expect("config.toml parses");
        let d = Config::default();
        assert_eq!((shipped.run.fps, shipped.run.limit, shipped.run.outputs.len()), (d.run.fps, d.run.limit, 0));
        assert_eq!((shipped.camera.walk, shipped.camera.rise), (d.camera.walk, d.camera.rise));
        assert_eq!((shipped.rain.droplets, shipped.rain.rate, shipped.rain.near, shipped.rain.far), (d.rain.droplets, d.rain.rate, d.rain.near, d.rain.far));
        assert_eq!((shipped.droplet.fall, shipped.droplet.spawn), (d.droplet.fall, d.droplet.spawn));
        assert_eq!((shipped.glyph.size, shipped.glyph.life, shipped.glyph.tick), (d.glyph.size, d.glyph.life, d.glyph.tick));
        assert_eq!((shipped.media.show, shipped.media.size, shipped.media.margin), (d.media.show, d.media.size, d.media.margin));
    }

    #[test]
    fn a_partial_file_keeps_the_rest() {
        let c: Config = toml::from_str("[rain]\ndroplets = 10\n").unwrap();
        assert_eq!(c.rain.droplets, 10);
        assert_eq!(c.rain.far, Config::default().rain.far);
        assert_eq!(c.run.fps, 60);
    }
}
