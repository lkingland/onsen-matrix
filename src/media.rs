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
//! The panel is not always there. It fades in when the track changes, or
//! when asked for with a key (`input.rs`), stays `[media] hold` seconds,
//! and fades out again, each fade taking `[media] fade` seconds; that is
//! `Presence`, a small state machine with no Bevy in it. While it is in,
//! it tells the input it has the keys. Nothing here reaches the bus. The
//! watcher's change counter says when to redraw the text; the position is
//! carried forward by the watcher and only read once a frame.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::text::FontSource;
use bevy::ui::UiTargetCamera;
use nowplaying::{Status, Watcher};

use crate::atlas;
use crate::config::Config;
use crate::input::{Action, Focus};
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
const MARK: f32 = 0.13;       // the play or pause mark's height, before the position
/// The marks' texture side, in pixels; they are drawn at startup.
const MARK_PIXELS: u32 = 64;
/// Frames to keep trying a thumbnail the player has named but not finished
/// writing: a browser announces the track before the file is complete.
const ART_RETRIES: u32 = 120;
/// Left this far into a track goes back to its start; before that, to the
/// previous track. What a CD player does.
const RESTART_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

pub struct MediaPlugin;

impl Plugin for MediaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start).add_systems(
            Update,
            (
                open_panel.run_if(resource_exists::<Player>).run_if(not(resource_exists::<Shown>)),
                (follow, act, load_art, tick_position, fade).chain().run_if(resource_exists::<Shown>),
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

/// The play and pause marks, as the usual triangle and two bars.
#[derive(Resource)]
struct Marks {
    playing: Handle<Image>,
    paused: Handle<Image>,
}

/// What the panel shows, so only what changed is redrawn. Present once the
/// panel has been spawned.
#[derive(Resource, Default)]
struct Shown {
    generation: Option<u64>,
    /// The track on screen, by what identifies it to the eye; a change
    /// brings the panel in.
    track: Option<(String, String, String)>,
    art: Art,
    presence: Presence,
}

/// Whether the panel is there: shown for a while after a change, then
/// gone, with a fade either way. `alpha` is what is drawn; `wanted` is
/// where it is heading, which is also what "visible" means to anything
/// that asks.
#[derive(Default, Debug, PartialEq)]
struct Presence {
    wanted: bool,
    alpha: f32,
    /// When to start fading out, on the clock `tick` is given.
    hide_at: Option<f32>,
}

impl Presence {
    /// Brings the panel in, or keeps it, until `hold` seconds from `now`.
    fn show(&mut self, now: f32, hold: f32) {
        self.wanted = true;
        self.hide_at = Some(now + hold);
    }

    /// Sends the panel out now.
    fn hide(&mut self) {
        self.wanted = false;
        self.hide_at = None;
    }

    /// A frame: the hold expiring, and the alpha moving toward what is
    /// wanted over `fade` seconds. Returns whether the alpha moved.
    fn tick(&mut self, now: f32, dt: f32, fade: f32) -> bool {
        if self.hide_at.is_some_and(|at| now >= at) {
            self.hide();
        }
        let target = if self.wanted { 1.0 } else { 0.0 };
        let step = if fade > 0.0 { dt / fade } else { 1.0 };
        let next = if target > self.alpha { (self.alpha + step).min(target) } else { (self.alpha - step).max(target) };
        let moved = next != self.alpha;
        self.alpha = next;
        moved
    }
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

/// The play or pause mark's node, before the position.
#[derive(Component)]
struct MarkNode;

/// Which line of text a node shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    Title,
    Artist,
    Detail,
}

/// Connects to the bus, loads the font and draws the marks, when the
/// panel is wanted.
fn start(mut commands: Commands, cfg: Res<Config>, mut fonts: ResMut<Assets<Font>>, mut images: ResMut<Assets<Image>>) {
    if !cfg.media.show {
        return;
    }
    match Watcher::start() {
        Ok(watcher) => commands.insert_resource(Player(watcher)),
        Err(e) => return warn!("media: nothing shown: {e}"),
    }
    commands.insert_resource(Marks { playing: images.add(mark(true)), paused: images.add(mark(false)) });
    match std::fs::read(atlas::FONT) {
        Ok(bytes) => commands.insert_resource(PanelFont(fonts.add(Font::from_bytes(bytes)))),
        Err(e) => warn!("media: {}: {e}; the panel's text is in Bevy's own font", atlas::FONT),
    }
}

/// The panel, once the scene's camera exists to draw it on: a row at the
/// bottom centre, the thumbnail then the lines, all sized by the height.
fn open_panel(mut commands: Commands, cfg: Res<Config>, font: Option<Res<PanelFont>>, marks: Res<Marks>, cameras: Query<Entity, With<Walker>>) {
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
                    children![
                        line(Line::Title, TITLE, TITLE_COLOUR),
                        line(Line::Artist, ARTIST, ARTIST_COLOUR),
                        (
                            Node { column_gap: vh(MARK * 0.6), align_items: AlignItems::Center, ..default() },
                            children![
                                (MarkNode, ImageNode { image: marks.playing.clone(), color: DETAIL_COLOUR, ..default() }, Node { width: vh(MARK), height: vh(MARK), ..default() }),
                                line(Line::Detail, DETAIL, DETAIL_COLOUR),
                            ],
                        ),
                    ],
                ),
            ],
        )],
    ));
    commands.init_resource::<Shown>();
}

/// When the watcher's change counter moves: the lines, the thumbnail to
/// load, and whether the track changed, which brings the panel in.
fn follow(player: Res<Player>, cfg: Res<Config>, time: Res<Time>, mut shown: ResMut<Shown>, mut lines: Query<(&Line, &mut Text)>) {
    let generation = player.0.generation();
    if shown.generation == Some(generation) {
        return;
    }
    shown.generation = Some(generation);
    let status = player.0.status();
    let track = status.as_ref().and_then(|s| s.track.as_ref());
    // A different track, or none any more: the panel comes in, or goes.
    let key = track.map(|t| (t.title.clone(), t.artist.clone(), t.art_url.clone()));
    if key != shown.track {
        shown.track = key;
        match track {
            Some(_) => shown.presence.show(time.elapsed_secs(), cfg.media.hold),
            None => shown.presence.hide(),
        }
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

/// The keys' asks. A control keeps the panel in for another hold, so what
/// it did can be seen.
fn act(player: Res<Player>, cfg: Res<Config>, time: Res<Time>, mut shown: ResMut<Shown>, mut actions: MessageReader<Action>) {
    for action in actions.read() {
        let control = match action {
            Action::ShowMedia => Ok(()),
            Action::HideMedia => {
                shown.presence.hide();
                continue;
            }
            Action::PlayPause => player.0.play_pause(),
            Action::Next => player.0.next(),
            Action::Previous => match player.0.status() {
                Some(s) if s.can_seek && s.position() > RESTART_AFTER => player.0.restart(),
                _ => player.0.previous(),
            },
            Action::End(_) => continue,
        };
        if let Err(e) = control {
            warn!("media: {action:?}: {e}");
        }
        shown.presence.show(time.elapsed_secs(), cfg.media.hold);
    }
}

/// The fade, applied: the alpha on every colour, the row laid out only
/// while there is something to see, and the keys claimed while it is in.
#[allow(clippy::too_many_arguments)]
fn fade(cfg: Res<Config>, time: Res<Time>, mut shown: ResMut<Shown>, mut focus: ResMut<Focus>, mut panel: Query<&mut Node, With<Panel>>, mut art: Query<&mut ImageNode, (With<ArtNode>, Without<MarkNode>)>, mut mark: Query<&mut ImageNode, With<MarkNode>>, mut lines: Query<(&Line, &mut TextColor)>) {
    let moved = shown.presence.tick(time.elapsed_secs(), time.delta_secs(), cfg.media.fade);
    if focus.media != shown.presence.wanted {
        focus.media = shown.presence.wanted;
    }
    if !moved {
        return;
    }
    let alpha = shown.presence.alpha;
    for mut node in &mut panel {
        node.display = if alpha > 0.0 { Display::Flex } else { Display::None };
    }
    for mut node in &mut art {
        node.color = Color::WHITE.with_alpha(alpha);
    }
    for mut node in &mut mark {
        node.color = DETAIL_COLOUR.with_alpha(alpha);
    }
    for (line, mut colour) in &mut lines {
        let base = match line {
            Line::Title => TITLE_COLOUR,
            Line::Artist => ARTIST_COLOUR,
            Line::Detail => DETAIL_COLOUR,
        };
        colour.0 = base.with_alpha(alpha);
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

/// The position line and the mark before it, from the carried-forward
/// position: the text changes once a second and is only touched then.
fn tick_position(player: Res<Player>, marks: Res<Marks>, mut lines: Query<(&Line, &mut Text)>, mut mark: Query<&mut ImageNode, With<MarkNode>>) {
    let status = player.0.status().filter(|s| s.track.is_some());
    set(&mut lines, Line::Detail, &status.as_ref().map(detail).unwrap_or_default());
    let wanted = if status.is_some_and(|s| s.playing()) { &marks.playing } else { &marks.paused };
    for mut node in &mut mark {
        if node.image != *wanted {
            node.image = wanted.clone();
        }
    }
}

/// `1:23 / 4:56`, or `1:23` alone with no length known.
fn detail(s: &Status) -> String {
    match s.track.as_ref().and_then(|t| t.length) {
        Some(length) => format!("{} / {}", clock(s.position()), clock(length)),
        None => clock(s.position()),
    }
}

/// The play mark (a triangle) or the pause mark (two bars), white on
/// nothing, to be tinted: `MARK_PIXELS` square, with the edges softened
/// over a pixel.
fn mark(playing: bool) -> Image {
    let n = MARK_PIXELS;
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    // Coverage at a point in 0..1 square coordinates, y down.
    let inside = |x: f32, y: f32| -> bool {
        if playing {
            // The triangle: left edge at 0.18, point at 0.92 on the middle line.
            let (left, right) = (0.18, 0.92);
            let half = 0.42 * (right - x) / (right - left);
            x >= left && x <= right && (y - 0.5).abs() <= half
        } else {
            let bar = |x0: f32| x >= x0 && x <= x0 + 0.24;
            (0.12..=0.88).contains(&y) && (bar(0.18) || bar(0.58))
        }
    };
    for py in 0..n {
        for px in 0..n {
            // Four by four samples across the pixel.
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / n as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / n as f32;
                    hits += inside(x, y) as u32;
                }
            }
            let alpha = (hits * 255 / 16) as u8;
            pixels.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    let mut image = Image::new(bevy::render::render_resource::Extent3d { width: n, height: n, depth_or_array_layers: 1 }, bevy::render::render_resource::TextureDimension::D2, pixels, bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    image.sampler = ImageSampler::linear();
    image
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
    fn presence_comes_in_holds_and_goes() {
        let mut p = Presence::default();
        assert!(!p.wanted);
        p.show(0.0, 10.0);
        assert!(p.wanted);
        // In over 0.5 s, by frames of a tenth.
        assert!(p.tick(0.1, 0.1, 0.5));
        assert!((p.alpha - 0.2).abs() < 1e-6);
        for i in 2..=5 {
            p.tick(0.1 * i as f32, 0.1, 0.5);
        }
        assert_eq!(p.alpha, 1.0);
        assert!(!p.tick(1.0, 0.1, 0.5), "steady while held");
        // The hold expires: out again, to nothing.
        assert!(p.tick(10.0, 0.1, 0.5));
        assert!(!p.wanted);
        for i in 1..=10 {
            p.tick(10.0 + 0.1 * i as f32, 0.1, 0.5);
        }
        assert_eq!(p.alpha, 0.0);
        assert!(!p.tick(12.0, 0.1, 0.5));
    }

    #[test]
    fn a_change_mid_fade_turns_it_round_and_hide_is_immediate() {
        let mut p = Presence::default();
        p.show(0.0, 1.0);
        p.tick(0.1, 0.1, 0.5);
        p.tick(1.0, 0.1, 0.5); // the hold is up: heading out
        assert!(!p.wanted);
        p.show(1.1, 1.0);
        assert!(p.wanted);
        p.tick(1.2, 0.1, 0.5);
        assert!(p.alpha > 0.0);
        p.hide();
        assert!(!p.wanted);
        p.tick(1.3, 0.1, 0.0); // no fade: straight to nothing
        assert_eq!(p.alpha, 0.0);
    }

    #[test]
    fn clock_reads_as_a_player_shows_it() {
        assert_eq!(clock(std::time::Duration::from_secs(0)), "0:00");
        assert_eq!(clock(std::time::Duration::from_secs(65)), "1:05");
        assert_eq!(clock(std::time::Duration::from_secs(4413)), "1:13:33");
    }
}
