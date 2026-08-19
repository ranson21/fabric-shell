//! Controller input, in a form a UI event loop can pump.
//!
//! ADR-0006's premise is that a controller drives the whole of a product, not
//! only a game. This is that: `fabric-input` resolves devices to an abstract
//! pad and to navigation intents, and this pulls one poll's worth of them out
//! in a shape a caller can hand straight to whatever its key presses already
//! go through.
//!
//! **No second input path.** In `ranortv-os`, `AppWindow.slint` funnels every
//! key through one `navigate(string)` callback and `main.rs` routes mouse
//! clicks the same way, deliberately. Feeding controller intents through that
//! same callback means the focus model cannot behave differently for a
//! controller than for a remote, and the existing headless tests keep
//! covering both. Any product consuming this crate should do the same: emit
//! [`fabric_input::Intent::as_name`] into whatever
//! [`crate::navigation::parse_action`] is called from.
//!
//! # This owns no thread, and no timer
//!
//! `ranortv-os`'s version of this module owned a `slint::Timer` and did the
//! whole job in one `start(&AppWindow)` call. That could not come across: a
//! Fabric crate that depends on Slint makes one product's toolkit the
//! platform's toolkit, which is the outcome ADR-0011's open question ("stay
//! strictly logic, each product bringing its own Slint") leans against.
//!
//! So the split is at the timer. The loop body — drain, notice a change in
//! the connected set, take the first pad, project intents — is here, verbatim
//! from where it was. The eight lines that create a `slint::Timer` and invoke
//! a callback stayed in the launcher, which is the only part that was ever
//! about Slint. `fabric-input` made the same call for the same reason and
//! says so: "the crate does not own a thread", because a launcher has a UI
//! event loop and an emulator a frame loop, and a thread here would make one
//! of them wrong.

use std::time::Duration;

use fabric_input::{DeviceId, Devices, EventSource, Intent, Intents};
use tracing::{debug, info};

/// How often the controller should be polled.
///
/// A frame at 120Hz. Fast enough that a press never feels late, and cheap
/// because a poll with nothing pending does almost nothing — spike I1
/// measured an idle launcher at zero CPU across six seconds, and this must
/// not change that.
///
/// Advisory rather than enforced: this crate owns no timer, so it is the
/// caller's interval to honour.
pub const POLL: Duration = Duration::from_millis(8);

/// Device tracking and intent projection for one controller-driven shell.
///
/// Holds the event source, the device set it drains into, and the edge
/// detector that turns pad state into intents. Call [`Pump::poll`] from
/// whatever loop the product already has, every [`POLL`].
pub struct Pump<S: EventSource> {
    source: S,
    devices: Devices,
    intents: Intents,
}

#[cfg(feature = "gilrs-backend")]
impl Pump<fabric_input::GilrsSource> {
    /// A pump over the real backend.
    ///
    /// Fails when no input backend is available. That is not an error worth
    /// failing a shell over: ADR-0006 decision 9 requires keyboard and remote
    /// to remain first-class, so a device with no controller support must
    /// still reach a usable screen — the same reasoning that makes a
    /// malformed config file fall back to defaults rather than refuse to
    /// start. The caller is expected to log it and carry on, which is what
    /// `ranortv-os`'s `gamepad::start` does.
    pub fn new() -> Result<Self, fabric_input::Error> {
        Ok(Self::with_source(fabric_input::GilrsSource::new()?))
    }
}

impl<S: EventSource> Pump<S> {
    /// A pump over any event source, including
    /// [`fabric_input::Synthetic`].
    ///
    /// Generic rather than hard-wired to the gilrs backend so the loop below
    /// is testable without hardware — the same boundary `fabric-input`'s
    /// `EventSource` exists to hold, honoured rather than closed over here.
    pub fn with_source(source: S) -> Self {
        Self {
            source,
            devices: Devices::new(),
            intents: Intents::new(),
        }
    }

    /// The device currently driving the interface, if any.
    ///
    /// Only the first connected pad drives it. Player assignment is ADR-0006
    /// decision 6 and a later stage; until then, taking the lowest id is
    /// stable and predictable rather than dependent on which pad moved last.
    pub fn active(&self) -> Option<DeviceId> {
        self.devices.connected().first().copied()
    }

    /// Drain everything pending and return the intents it implies.
    ///
    /// Empty is the overwhelmingly common answer, and is not a failure: it
    /// means nothing moved since the last call.
    pub fn poll(&mut self) -> Vec<Intent> {
        let before = self.devices.connected();
        self.source.drain_into(&mut self.devices);
        let after = self.devices.connected();
        if before != after {
            info!(count = after.len(), "controllers changed");
        }

        let Some(id) = after.first().copied() else {
            return Vec::new();
        };
        let Some(pad) = self.devices.pad(id) else {
            return Vec::new();
        };

        let intents = self.intents.update(pad);
        for intent in &intents {
            debug!(?intent, %id, "controller intent");
        }
        intents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_input::{Button, DeviceInfo, Event, MappingSource, ModelId, Synthetic};

    fn connected(id: u32) -> Event {
        Event::Connected {
            id: DeviceId(id),
            info: DeviceInfo {
                name: "test pad".into(),
                model: ModelId([1; 16]),
                mapping: MappingSource::None,
            },
        }
    }

    fn press(id: u32, button: Button, pressed: bool) -> Event {
        Event::Button {
            id: DeviceId(id),
            button,
            pressed,
        }
    }

    #[test]
    fn a_pump_with_nothing_connected_yields_nothing() {
        let mut pump = Pump::with_source(Synthetic::default());
        assert!(pump.poll().is_empty());
        assert_eq!(pump.active(), None);
    }

    #[test]
    fn a_press_becomes_an_intent() {
        let mut pump = Pump::with_source(Synthetic::new([
            connected(0),
            press(0, Button::DpadRight, true),
        ]));
        assert_eq!(pump.poll(), vec![Intent::Right]);
        assert_eq!(pump.active(), Some(DeviceId(0)));
    }

    #[test]
    fn holding_a_button_fires_once() {
        // The edge detection is `fabric_input::Intents`', not this crate's;
        // asserted here because the pump is what would break it, by
        // constructing a fresh `Intents` per poll instead of keeping one.
        let mut pump = Pump::with_source(Synthetic::new([
            connected(0),
            press(0, Button::DpadRight, true),
        ]));
        assert_eq!(pump.poll(), vec![Intent::Right]);
        assert!(pump.poll().is_empty(), "held, not pressed again");
    }

    #[test]
    fn the_lowest_connected_id_drives_the_interface() {
        // Two pads, the second connecting first. Player assignment is a later
        // stage; until then this must not depend on arrival order.
        let mut pump = Pump::with_source(Synthetic::new([connected(3), connected(1)]));
        pump.poll();
        assert_eq!(pump.active(), Some(DeviceId(1)));
    }
}
