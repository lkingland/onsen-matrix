//! What a key or the pointer asks of the run. The window gets the raw
//! input; this turns it into one `Action` per press, and the parts that
//! act (the host ending the run, the now-playing panel) listen for theirs.
//!
//! The rule of a screensaver is that any input ends it, and that stays the
//! rule. The exceptions are the keys the now-playing panel takes, and they
//! depend on whether the panel is there: Space brings it in when it is
//! not, and plays or pauses when it is; Right skips to the next track when
//! it is; Shift+Space sends it out. The panel says
//! which through `Focus`, and the whole key table is `action` below, so
//! the modality is in one place and can be read as a table. Modifier keys
//! on their own do nothing, so a chord can be pressed.
//!
//! Keys are matched by what they mean (the logical key, after the
//! keyboard layout), not by where they are: Space is Space on any layout,
//! and a virtual keyboard, which has no physical layout to speak of, is
//! read right too.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::input::ButtonState;
use bevy::prelude::*;

/// How far the pointer may drift before it counts as movement, in pixels.
const POINTER_SLACK: f32 = 12.0;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Action>().init_resource::<Focus>().add_systems(PreUpdate, (keys, pointer));
    }
}

/// What an input asks for.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// End the run; the reason is for the log.
    End(String),
    /// Bring the now-playing panel in, or keep it in.
    ShowMedia,
    /// Send the now-playing panel out now.
    HideMedia,
    /// Pause the player, or play it if paused.
    PlayPause,
    /// Skip to the next track.
    Next,
}

/// Which part of the picture has the keys. The now-playing panel sets
/// `media` while it is in or on its way in.
#[derive(Resource, Default)]
pub struct Focus {
    pub media: bool,
}

/// The key table: what a press means, given whether Shift is held and who
/// has the keys. `None` is a press that means nothing: a modifier on its
/// own, or a panel chord with no panel to act on.
fn action(key: &Key, shift: bool, media: bool) -> Option<Action> {
    match (key, shift, media) {
        (Key::Space, false, false) => Some(Action::ShowMedia),
        (Key::Space, false, true) => Some(Action::PlayPause),
        (Key::Space, true, true) => Some(Action::HideMedia),
        (Key::Space, true, false) => None,
        (Key::ArrowRight, false, true) => Some(Action::Next),
        (Key::Shift | Key::Control | Key::Alt | Key::AltGraph | Key::Super | Key::Meta | Key::Hyper | Key::Fn, ..) => None,
        (other, ..) => Some(Action::End(format!("key {other:?}"))),
    }
}

/// The key events in their order, each press to its action. Whether Shift
/// is down is kept from the same stream, so a chord reads right however
/// the events fall across frames.
fn keys(mut events: MessageReader<KeyboardInput>, focus: Res<Focus>, mut shift: Local<bool>, mut actions: MessageWriter<Action>) {
    for event in events.read() {
        if let Some(a) = read(&event.logical_key, event.state == ButtonState::Pressed, event.repeat, focus.media, &mut shift) {
            debug!("key {:?} shift={} media={}: {a:?}", event.logical_key, *shift, focus.media);
            actions.write(a);
        }
    }
}

/// One key event: keeps `shift`, and gives the action of a first press.
fn read(key: &Key, pressed: bool, repeat: bool, media: bool, shift: &mut bool) -> Option<Action> {
    if *key == Key::Shift && !repeat {
        *shift = pressed;
    }
    if !pressed || repeat {
        return None;
    }
    action(key, *shift, media)
}

/// A click, the wheel, or the pointer moving more than a little: the end.
fn pointer(mut buttons: MessageReader<MouseButtonInput>, mut wheel: MessageReader<MouseWheel>, mut motion: MessageReader<MouseMotion>, mut drifted: Local<Vec2>, mut actions: MessageWriter<Action>) {
    *drifted += motion.read().map(|m| m.delta).sum::<Vec2>();
    let why = buttons.read().next().map(|b| format!("button {:?}", b.button))
        .or_else(|| wheel.read().next().map(|_| "wheel".to_string()))
        .or_else(|| (drifted.length() > POINTER_SLACK).then(|| format!("pointer moved {:.0} px", drifted.length())));
    if let Some(why) = why {
        actions.write(Action::End(why));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn space_brings_the_panel_in_then_plays_or_pauses_and_any_other_key_ends_the_run() {
        assert_eq!(action(&Key::Space, false, false), Some(Action::ShowMedia));
        assert_eq!(action(&Key::Space, false, true), Some(Action::PlayPause));
        assert!(matches!(action(&letter("a"), false, false), Some(Action::End(_))));
        assert!(matches!(action(&Key::Escape, false, true), Some(Action::End(_))));
        assert!(matches!(action(&letter("A"), true, true), Some(Action::End(_))), "shifted letters end it too");
    }

    #[test]
    fn right_skips_with_the_panel_in_and_ends_the_run_without_it() {
        assert_eq!(action(&Key::ArrowRight, false, true), Some(Action::Next));
        assert!(matches!(action(&Key::ArrowRight, false, false), Some(Action::End(_))));
        assert!(matches!(action(&Key::ArrowRight, true, true), Some(Action::End(_))), "with Shift it is some other key");
    }

    #[test]
    fn shift_space_sends_the_panel_out_and_is_nothing_without_one() {
        assert_eq!(action(&Key::Space, true, true), Some(Action::HideMedia));
        assert_eq!(action(&Key::Space, true, false), None);
    }

    #[test]
    fn a_chord_reads_right_from_the_stream_whatever_the_frames() {
        // Shift down, Space down and up, Shift up: one hide, nothing else,
        // whether the events come in one frame or several.
        let mut shift = false;
        let events = [(Key::Shift, true), (Key::Space, true), (Key::Space, false), (Key::Shift, false), (Key::Space, true)];
        let actions: Vec<_> = events.iter().filter_map(|(k, down)| read(k, *down, false, true, &mut shift)).collect();
        assert_eq!(actions, [Action::HideMedia, Action::PlayPause]);
        assert!(!shift);
        // A held key repeating is not pressed again.
        assert_eq!(read(&Key::Space, true, true, true, &mut shift), None);
    }

    #[test]
    fn a_modifier_alone_is_nothing() {
        assert_eq!(action(&Key::Shift, false, false), None);
        assert_eq!(action(&Key::Control, false, true), None);
        assert_eq!(action(&Key::Shift, true, true), None);
    }
}
