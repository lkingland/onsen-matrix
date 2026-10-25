//! What a key or the pointer asks of the run. The window gets the raw
//! input; this turns it into one `Action` per press, and the parts that
//! act (the host ending the run, the now-playing panel) listen for theirs.
//!
//! The rule of a screensaver is that any input ends it, and that stays the
//! rule. The exceptions are the keys the now-playing panel takes, and they
//! depend on whether the panel is there: Space brings it in when it is
//! not, and drives it when it is. The panel says which through `Focus`,
//! and the whole key table is `action` below, so the modality is in one
//! place and can be read as a table. Modifier keys on their own do nothing,
//! so a chord can be pressed.

use bevy::input::keyboard::KeyboardInput;
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
}

/// Which part of the picture has the keys. The now-playing panel sets
/// `media` while it is in or on its way in.
#[derive(Resource, Default)]
pub struct Focus {
    pub media: bool,
}

/// The key table: what a press means, given who has the keys. `None` is a
/// press that means nothing, such as a modifier on its own.
fn action(key: KeyCode, media: bool) -> Option<Action> {
    use KeyCode::*;
    match (key, media) {
        (Space, _) => Some(Action::ShowMedia),
        (ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft | SuperRight, _) => None,
        (other, _) => Some(Action::End(format!("key {other:?}"))),
    }
}

/// Each key press, as it is first pressed (not held), to its action.
fn keys(mut presses: MessageReader<KeyboardInput>, focus: Res<Focus>, mut actions: MessageWriter<Action>) {
    for press in presses.read() {
        if press.state != ButtonState::Pressed || press.repeat {
            continue;
        }
        if let Some(a) = action(press.key_code, focus.media) {
            actions.write(a);
        }
    }
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

    #[test]
    fn space_brings_the_panel_in_and_any_other_key_ends_the_run() {
        assert_eq!(action(KeyCode::Space, false), Some(Action::ShowMedia));
        assert_eq!(action(KeyCode::Space, true), Some(Action::ShowMedia));
        assert!(matches!(action(KeyCode::KeyA, false), Some(Action::End(_))));
        assert!(matches!(action(KeyCode::Escape, true), Some(Action::End(_))));
    }

    #[test]
    fn a_modifier_alone_is_nothing() {
        assert_eq!(action(KeyCode::ShiftLeft, false), None);
        assert_eq!(action(KeyCode::ControlRight, true), None);
    }
}
