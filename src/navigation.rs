//! Names to actions.
//!
//! All navigation decisions live in [`crate::focus`]. This module only maps
//! key names onto actions.
//!
//! # What stayed behind, and why
//!
//! In `ranortv-os` this module did three more things: it described the
//! current tab shapes to the focus model, pushed the resulting state onto a
//! Slint window, and looked up which app the focused index pointed at. All
//! three named `LauncherState`, `AppWindow` and RaNorTV's four-tab order
//! (`TAB_HOME`, `TAB_APPS`, `TAB_STORE`, `TAB_SETTINGS`), so all three stayed
//! there and this module kept only the half that names nothing.
//!
//! ADR-0011's table calls this module "action names → focus actions", and
//! that description turns out to be exactly [`parse_action`] rather than the
//! whole file. The rest was product wiring that happened to share a filename.
//! `platform-boundary.md`'s point 3 is the test that separates them:
//! *product-neutral is not the same as reusable*.

use crate::focus::Action;

/// Map the string the UI sends onto an action.
///
/// The UI deliberately sends names rather than key codes, so remapping a
/// remote or adding a gamepad does not require touching the state machine.
pub fn parse_action(name: &str) -> Option<Action> {
    Some(match name {
        "up" => Action::Up,
        "down" => Action::Down,
        "left" => Action::Left,
        "right" => Action::Right,
        "select" => Action::Select,
        "back" => Action::Back,
        "home" => Action::Home,
        "tab_prev" => Action::TabPrev,
        "tab_next" => Action::TabNext,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_navigation_name_the_ui_sends_is_understood() {
        for name in [
            "up", "down", "left", "right", "select", "back", "home", "tab_prev", "tab_next",
        ] {
            assert!(parse_action(name).is_some(), "{name} not handled");
        }
    }

    #[test]
    fn unknown_navigation_names_are_rejected() {
        assert!(parse_action("sideways").is_none());
        assert!(parse_action("").is_none());
    }

    /// The join between the two crates, asserted rather than assumed.
    ///
    /// `fabric_input::Intent::as_name` and [`parse_action`] are the two halves
    /// of the controller path: the pump emits names, this turns them back
    /// into actions. Nothing else forces the two vocabularies to agree, and
    /// they live in different repositories, so a name added on one side and
    /// not the other would fail silently as an ignored press.
    #[test]
    fn every_intent_name_the_pump_emits_parses_back_to_an_action() {
        use fabric_input::Intent;
        for intent in [
            Intent::Up,
            Intent::Down,
            Intent::Left,
            Intent::Right,
            Intent::Select,
            Intent::Back,
            Intent::TabPrev,
            Intent::TabNext,
        ] {
            assert!(
                parse_action(intent.as_name()).is_some(),
                "{:?} emits {:?}, which parse_action does not understand",
                intent,
                intent.as_name()
            );
        }
    }
}
