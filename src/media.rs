//! What is playing: the thumbnail, title, artist and position of the track
//! the media players on the session bus are on, at the bottom centre of
//! the picture. The `nowplaying` crate (crates/nowplaying) watches the bus
//! the way the Omarchy bar does and keeps the latest state; this module
//! only draws it.
//!
//! The panel is Bevy UI targeted at the scene's camera, so it is laid out
//! over the whole canvas, every display wide, and drawn into it after the
//! scene, bloom and tone mapping: the rain glows, the panel does not. Its
//! sizes are fractions of the canvas's height (`[media] size` in the
//! config), so it looks the same on any display. The text is set in the
//! atlas's own font, so a title in any script the sutra's font covers is
//! shown.
//!
//! Nothing here reaches the bus. The watcher's change counter says when to
//! redraw the text; the position is carried forward by the watcher and
//! only read once a frame.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::text::FontSource;
use bevy::ui::UiTargetCamera;
use nowplaying::{Status, Watcher};

use crate::atlas;
use crate::config::Config;
use crate::scene::Walker;

// ------------------------------------------------------------------- look
//
// Everything is relative to the thumbnail's height, which `[media] size`
// sets as a fraction of the canvas's.
const TITLE: f32 = 0.26;      // the title's text height
const ARTIST: f32 = 0.19;     // the artist's
const DETAIL: f32 = 0.13;     // the position line's
const GAP: f32 = 0.16;        // between the thumbnail and the text
const LEADING: f32 = 0.06;    // between the lines
const TITLE_COLOUR: Color = Color::srgb(0.92, 1.0, 0.94);
const ARTIST_COLOUR: Color = Color::srgb(0.55, 0.9, 0.65);
const DETAIL_COLOUR: Color = Color::srgb(0.35, 0.55, 0.42);
/// Frames to keep trying a thumbnail the player has named but not finished
/// writing: a browser announces the track before the file is complete.
const ART_RETRIES: u32 = 120;

pub struct MediaPlugin;

impl Plugin for MediaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start).add_systems(
            Update,
            (
                open_panel.run_if(resource_exists::<Player>).run_if(not(resource_exists::<Shown>)),
                (follow, load_art, tick_position).run_if(resource_exists::<Shown>),
            ),
        );
    }
}

/// The watcher on the bus. Absent when the panel is off, or there is no bus.
#[derive(Resource)]
struct Player(Watcher);

/// The panel's font: the atlas's, as a text asset. Absent if it did not load.
#[derive(Resource)]
struct PanelFont(Handle<Font>);

/// What the panel shows, so only what changed is redrawn. Present once the
/// panel has been spawned.
#[derive(Resource, Default)]
struct Shown {
    generation: Option<u64>,
    art: Art,
}

/// The thumbnail: none, one still being read, or the one on screen. Each
/// is keyed by the URL the player gave, which changes with the track.
#[derive(Default, PartialEq)]
enum Art {
    #[default]
    None,
    Loading { url: String, path: std::path::PathBuf, retries: u32 },
    Loaded(String),
}

impl Art {
    fn url(&self) -> &str {
        match self {
            Art::None => "",
            Art::Loading { url, .. } | Art::Loaded(url) => url,
        }
    }
}

/// The row holding the thumbnail and the text; hidden when nothing plays.
#[derive(Component)]
struct Panel;

/// The thumbnail's node.
#[derive(Component)]
struct ArtNode;

/// Which line of text a node shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    Title,
    Artist,
    Detail,
}

/// Connects to the bus and loads the font, when the panel is wanted.
fn start(mut commands: Commands, cfg: Res<Config>, mut fonts: ResMut<Assets<Font>>) {
    if !cfg.media.show {
        return;
    }
    match Watcher::start() {
        Ok(watcher) => commands.insert_resource(Player(watcher)),
        Err(e) => return warn!("media: nothing shown: {e}"),
    }
    match std::fs::read(atlas::FONT) {
        Ok(bytes) => commands.insert_resource(PanelFont(fonts.add(Font::from_bytes(bytes)))),
        Err(e) => warn!("media: {}: {e}; the panel's text is in Bevy's own font", atlas::FONT),
    }
}

/// The panel, once the scene's camera exists to draw it on: a row at the
/// bottom centre, the thumbnail then the lines, all sized by the height.
fn open_panel(mut commands: Commands, cfg: Res<Config>, font: Option<Res<PanelFont>>, cameras: Query<Entity, With<Walker>>) {
    let Some(camera) = cameras.iter().next() else { return };
    let vh = |f: f32| Val::Vh(100.0 * cfg.media.size * f);
    let font = font.map(|f| FontSource::Handle(f.0.clone())).unwrap_or_default();
    let line = |which: Line, size: f32, colour: Color| (which, Text::new(""), TextFont { font: font.clone(), font_size: FontSize::Vh(100.0 * cfg.media.size * size), ..default() }, TextColor(colour));
    commands.spawn((
        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, padding: UiRect::bottom(Val::Vh(100.0 * cfg.media.margin)), ..default() },
        UiTargetCamera(camera),
        children![(
            Panel,
            Node { display: Display::None, column_gap: vh(GAP), align_items: AlignItems::Center, ..default() },
            children![
                (ArtNode, ImageNode::default(), Node { width: vh(1.0), height: vh(1.0), flex_shrink: 0.0, ..default() }),
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: vh(LEADING), ..default() },
                    children![line(Line::Title, TITLE, TITLE_COLOUR), line(Line::Artist, ARTIST, ARTIST_COLOUR), line(Line::Detail, DETAIL, DETAIL_COLOUR)],
                ),
            ],
        )],
    ));
    commands.init_resource::<Shown>();
}

/// When the watcher's change counter moves: the lines, the panel's
/// visibility, and the thumbnail to load.
fn follow(player: Res<Player>, mut shown: ResMut<Shown>, mut panel: Query<&mut Node, With<Panel>>, mut lines: Query<(&Line, &mut Text)>) {
    let generation = player.0.generation();
    if shown.generation == Some(generation) {
        return;
    }
    shown.generation = Some(generation);
    let status = player.0.status();
    let track = status.as_ref().and_then(|s| s.track.as_ref());
    for mut node in &mut panel {
        node.display = if track.is_some() { Display::Flex } else { Display::None };
    }
    set(&mut lines, Line::Title, track.map(|t| t.title.as_str()).unwrap_or(""));
    set(&mut lines, Line::Artist, track.map(|t| t.artist.as_str()).unwrap_or(""));
    let Some(track) = track else { return shown.art = Art::None };
    if shown.art.url() == track.art_url {
        return;
    }
    shown.art = match track.art_path() {
        Some(path) => Art::Loading { url: track.art_url.clone(), path, retries: ART_RETRIES },
        None => {
            if !track.art_url.is_empty() {
                // An https URL, say: not fetched; the thumbnail stays empty.
                info!("media: artwork not a local file: {}", track.art_url);
            }
            Art::None
        }
    };
}

/// Loads the thumbnail named, retrying for a couple of seconds while the
/// file is incomplete; the node is cleared when there is none.
fn load_art(mut shown: ResMut<Shown>, mut images: ResMut<Assets<Image>>, mut art: Query<&mut ImageNode, With<ArtNode>>) {
    let handle = match &mut shown.art {
        Art::Loaded(_) => return,
        Art::None => Handle::default(),
        Art::Loading { path, retries, url } => match decode(path) {
            Ok(image) => images.add(image),
            Err(_) if *retries > 0 => return *retries -= 1,
            Err(e) => {
                warn!("media: thumbnail {url}: {e}");
                Handle::default()
            }
        },
    };
    shown.art = match std::mem::take(&mut shown.art) {
        Art::Loading { url, .. } => Art::Loaded(url),
        none => none,
    };
    for mut node in &mut art {
        node.image = handle.clone();
    }
}

/// The thumbnail file into an image. Browsers write PNG; a JPEG would need
/// Bevy's `jpeg` feature.
fn decode(path: &std::path::Path) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = match bytes.get(..4) {
        Some([0x89, b'P', b'N', b'G']) => "png",
        Some([0xFF, 0xD8, ..]) => "jpg",
        _ => return Err("not an image (yet)".into()),
    };
    Image::from_buffer(&bytes, ImageType::Extension(ext), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD).map_err(|e| e.to_string())
}

/// The position line, from the carried-forward position: it changes once a
/// second, and the text is only touched then.
fn tick_position(player: Res<Player>, mut lines: Query<(&Line, &mut Text)>) {
    let line = match player.0.status() {
        Some(s) if s.track.is_some() => detail(&s),
        _ => String::new(),
    };
    set(&mut lines, Line::Detail, &line);
}

/// `1:23 / 4:56`, or `paused  1:23 / 4:56`, or `1:23` with no length known.
fn detail(s: &Status) -> String {
    let state = if s.playing() { String::new() } else { format!("{}  ", s.playback) };
    match s.track.as_ref().and_then(|t| t.length) {
        Some(length) => format!("{state}{} / {}", clock(s.position()), clock(length)),
        None => format!("{state}{}", clock(s.position())),
    }
}

/// Minutes and seconds, with hours once there are any.
fn clock(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn set(lines: &mut Query<(&Line, &mut Text)>, line: Line, value: &str) {
    for (_, mut text) in lines.iter_mut().filter(|(l, _)| **l == line) {
        if text.0 != value {
            text.0 = value.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_reads_as_a_player_shows_it() {
        assert_eq!(clock(std::time::Duration::from_secs(0)), "0:00");
        assert_eq!(clock(std::time::Duration::from_secs(65)), "1:05");
        assert_eq!(clock(std::time::Duration::from_secs(4413)), "1:13:33");
    }
}
