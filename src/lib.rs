//! The focus model, and the controller pump that feeds it.
//!
//! This crate is the logic layer of a Fabric shell. It sits below both
//! products described in
//! [ADR-0011](https://github.com/ranson21/ranortv-os/blob/master/docs/adr/0011-fabric-shell.md):
//! `ranortv-os`'s launcher, where all of it was written and is still running,
//! and Apex's shell, which is the second consumer that made extracting it a
//! move rather than a guess.
//!
//! # Strictly logic
//!
//! Nothing here draws anything, and nothing here names a toolkit. The focus
//! model works entirely in zones, tab indices and item indices, which is what
//! makes the whole of the navigation behaviour testable without a display —
//! the state machine is the part that has to be right, not the pixels.
//!
//! ADR-0011's open question asks whether this crate should also carry a
//! widget vocabulary. It does not, and the lean recorded there is that it
//! should not: a shared widget set is where product identity leaks across the
//! boundary this whole structure exists to hold.
//!
//! # What this crate is not
//!
//! **It is not the whole of `ranortv-os`'s `launcher/`.** Six of its modules
//! deliberately stayed: `config`, `paths`, `loader`, `models`, `store` and
//! `actions`. They are neutral *code* carrying product-shaped *values* —
//! filesystem locations, manifest schemas, launch mechanics — and the
//! superproject's `docs/platform-boundary.md` puts the reason plainly:
//!
//! > **Product-neutral is not the same as reusable.** A component can contain
//! > no RaNorTV concepts at all and still encode RaNorTV's *shape*.
//!
//! **It is not, yet, a shape Apex is happy with.** That same document
//! observes that the focus model here is built around tabs, grids and a
//! two-zone model, which is RaNorTV's interface shape, and that Apex may want
//! something else — stacked lanes within one tab, each remembering its own
//! horizontal position, is the concrete example. That is a change for the
//! consumer who needs it to make, afterwards and on its own. This crate is
//! the extraction: the same code, the same behaviour, the same tests,
//! somewhere both products can reach it.
//!
//! # Layout
//!
//! | Module | Role |
//! |---|---|
//! | [`focus`] | The state machine: zones, tabs, indices, and what each action does |
//! | [`navigation`] | Action names to actions, the one direction that is neutral |
//! | [`gamepad`] | The pump: `fabric-input` devices and intents, drained per poll |
//!
//! # Visibility
//!
//! This crate is **private**, and [`fabric_input`] beneath it is **public**.
//! That asymmetry has a consequence worth knowing before depending on this
//! from CI: a workflow's default `GITHUB_TOKEN` can read only its own
//! repository, so a bare `actions/checkout` followed by `cargo` cannot fetch
//! this crate. Consuming it from CI needs a deploy key or a fine-grained PAT
//! with read-only Contents, provisioned in the same change that adds the
//! dependency. The full statement of the cost, and who owns it, is in
//! `ranor-fabric` at `environments/global/repos/fabric-shell/terragrunt.hcl`.

pub mod focus;
pub mod gamepad;
pub mod navigation;

pub use focus::{Action, FocusModel, Layout, Outcome, Tab, Zone};
pub use gamepad::{Pump, POLL};
pub use navigation::parse_action;
