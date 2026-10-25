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
//! shown; the play and pause marks are drawn (`images.rs`).
//!
//! The panel is not always there. It fades in when the track changes, or
//! when asked for with a key (`input.rs`), stays `[media] hold` seconds,
//! and fades out again, each fade taking `[media] fade` seconds; that is
//! `Presence` (`presence.rs`). While it is in, it tells the input it has
//! the keys, and the keys drive the player through it.
//!
//! Nothing here reaches the bus. The watcher's change counter says when to
//! redraw the text; the position is carried forward by the watcher and
//! only read once a frame.

mod images;
mod presence;

use bevy::prelude::*;
use bevy::text::FontSource;
use bevy::ui::UiTargetCamera;
use nowplaying::{Status, Watcher};

use crate::atlas;
use crate::config::Config;
use crate::input::{Action, Focus};
use crate::scene::Walker;
use presence::Presence;

// ------------------------------------------------------------------- look
//
// Everything is relative to the thumbnail's height, which `[media] size`
// sets as a fraction of the canvas's.
const TITLE: f32 = 0.26;      // the title's text height
const ARTIST: f32 = 0.19;     // the artist's
const DETAIL: f32 = 0.17;     // the position line's
const MARK: f32 = 0.17;       // the play or pause mark's height, before the position
const GAP: f32 = 0.16;        // between the thumbnail and the text
const LEADING: f32 = 0.06;    // between the lines
const TITLE_COLOUR: Color = Color::srgb(0.92, 1.0, 0.94);
const ARTIST_COLOUR: Color = Color::srgb(0.55, 0.9, 0.65);
const DETAIL_COLOUR: Color = Color::srgb(0.35, 0.55, 0.42);

// -------------------------------------------------------------- behaviour
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
                open_panel.run_if(resource_exists::<Player>).run_if(not(resource_exists::<Panel>)),
                // In order: what changed, what was asked, then the frame's
                // presence and the drawing of it all.
                (follow, act, tick_presence, load_art, tick_position, paint).chain().run_if(resource_exists::<Panel>),
            ),
        );
    }
}

// -------------------------------------------------------------- resources

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

/// The panel's state, once it has been spawned: what it shows, so only
/// what changed is redrawn, and whether it is there.
#[derive(Resource, Default)]
struct Panel {
    /// The watcher's change counter as last read.
    generation: Option<u64>,
    /// The track on screen, by what identifies it to the eye; a change
    /// brings the panel in.
    track: Option<(String, String, String)>,
    art: Art,
    presence: Presence,
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

// ------------------------------------------------------------- components

/// The row holding the thumbnail and the text; laid out only while seen.
#[derive(Component)]
struct Row;

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

impl Line {
    fn colour(self) -> Color {
        match self {
            Line::Title => TITLE_COLOUR,
            Line::Artist => ARTIST_COLOUR,
            Line::Detail => DETAIL_COLOUR,
        }
    }
}

// ---------------------------------------------------------------- systems

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
    commands.insert_resource(Marks { playing: images.add(images::mark(true)), paused: images.add(images::mark(false)) });
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
    let line = |which: Line, size: f32| (which, Text::new(""), TextFont { font: font.clone(), font_size: FontSize::Vh(100.0 * cfg.media.size * size), ..default() }, TextColor(which.colour()));
    commands.spawn((
        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, padding: UiRect::bottom(Val::Vh(100.0 * cfg.media.margin)), ..default() },
        UiTargetCamera(camera),
        children![(
            Row,
            Node { display: Display::None, column_gap: vh(GAP), align_items: AlignItems::Center, ..default() },
            children![
                (ArtNode, ImageNode::default(), Node { width: vh(1.0), height: vh(1.0), flex_shrink: 0.0, ..default() }),
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: vh(LEADING), ..default() },
                    children![
                        line(Line::Title, TITLE),
                        line(Line::Artist, ARTIST),
                        (
                            Node { column_gap: vh(MARK * 0.6), align_items: AlignItems::Center, ..default() },
                            children![
                                (MarkNode, ImageNode { image: marks.playing.clone(), color: DETAIL_COLOUR, ..default() }, Node { width: vh(MARK), height: vh(MARK), ..default() }),
                                line(Line::Detail, DETAIL),
                            ],
                        ),
                    ],
                ),
            ],
        )],
    ));
    commands.init_resource::<Panel>();
}

/// When the watcher's change counter moves: the lines, the thumbnail to
/// load, and whether the track changed, which brings the panel in.
fn follow(player: Res<Player>, cfg: Res<Config>, time: Res<Time>, mut panel: ResMut<Panel>, mut lines: Query<(&Line, &mut Text)>) {
    let generation = player.0.generation();
    if panel.generation == Some(generation) {
        return;
    }
    panel.generation = Some(generation);
    let status = player.0.status();
    let track = status.as_ref().and_then(|s| s.track.as_ref());
    // A different track, or none any more: the panel comes in, or goes.
    let key = track.map(|t| (t.title.clone(), t.artist.clone(), t.art_url.clone()));
    if key != panel.track {
        panel.track = key;
        match track {
            Some(_) => panel.presence.show(time.elapsed_secs(), cfg.media.hold),
            None => panel.presence.hide(),
        }
    }
    set(&mut lines, Line::Title, track.map(|t| t.title.as_str()).unwrap_or(""));
    set(&mut lines, Line::Artist, track.map(|t| t.artist.as_str()).unwrap_or(""));
    let Some(track) = track else { return panel.art = Art::None };
    if panel.art.url() == track.art_url {
        return;
    }
    panel.art = match track.art_path() {
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

/// The keys' asks. A control keeps the panel in for another hold, so what
/// it did can be seen.
fn act(player: Res<Player>, cfg: Res<Config>, time: Res<Time>, mut panel: ResMut<Panel>, mut actions: MessageReader<Action>) {
    for action in actions.read() {
        let control = match action {
            Action::ShowMedia => Ok(()),
            Action::HideMedia => {
                panel.presence.hide();
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
        panel.presence.show(time.elapsed_secs(), cfg.media.hold);
    }
}

/// The frame's presence, and the keys claimed while the panel is in.
fn tick_presence(cfg: Res<Config>, time: Res<Time>, mut panel: ResMut<Panel>, mut focus: ResMut<Focus>) {
    panel.presence.tick(time.elapsed_secs(), time.delta_secs(), cfg.media.fade);
    if focus.media != panel.presence.visible() {
        focus.media = panel.presence.visible();
    }
}

/// Loads the thumbnail named, retrying for a couple of seconds while the
/// file is incomplete; the node is cleared when there is none.
fn load_art(mut panel: ResMut<Panel>, mut images: ResMut<Assets<Image>>, mut art: Query<&mut ImageNode, With<ArtNode>>) {
    let handle = match &mut panel.art {
        Art::Loaded(_) => return,
        Art::None => Handle::default(),
        Art::Loading { path, retries, url } => match images::thumbnail(path) {
            Ok(image) => images.add(image),
            Err(_) if *retries > 0 => return *retries -= 1,
            Err(e) => {
                warn!("media: thumbnail {url}: {e}");
                Handle::default()
            }
        },
    };
    panel.art = match std::mem::take(&mut panel.art) {
        Art::Loading { url, .. } => Art::Loaded(url),
        none => none,
    };
    for mut node in &mut art {
        node.image = handle.clone();
    }
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

/// The presence, drawn: the alpha on every colour, and the row laid out
/// only while there is something to see. Touched only when the alpha moved.
#[allow(clippy::too_many_arguments)]
fn paint(panel: Res<Panel>, mut painted: Local<Option<f32>>, mut row: Query<&mut Node, With<Row>>, mut art: Query<&mut ImageNode, (With<ArtNode>, Without<MarkNode>)>, mut mark: Query<&mut ImageNode, With<MarkNode>>, mut lines: Query<(&Line, &mut TextColor)>) {
    let alpha = panel.presence.alpha();
    if *painted == Some(alpha) {
        return;
    }
    *painted = Some(alpha);
    for mut node in &mut row {
        node.display = if alpha > 0.0 { Display::Flex } else { Display::None };
    }
    for mut node in &mut art {
        node.color = Color::WHITE.with_alpha(alpha);
    }
    for mut node in &mut mark {
        node.color = DETAIL_COLOUR.with_alpha(alpha);
    }
    for (line, mut colour) in &mut lines {
        colour.0 = line.colour().with_alpha(alpha);
    }
}

// ------------------------------------------------------------------- text

/// `1:23 / 4:56`, or `1:23` alone with no length known.
fn detail(s: &Status) -> String {
    match s.track.as_ref().and_then(|t| t.length) {
        Some(length) => format!("{} / {}", clock(s.position()), clock(length)),
        None => clock(s.position()),
    }
}

/// Minutes and seconds, with hours once there are any.
fn clock(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A line's text, touched only when it differs.
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
