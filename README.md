# fabric-shell

The focus model and controller pump for
[RaNor Fabric](https://github.com/ranson21/ranor-fabric) — the shared device
platform beneath RaNorTV and Apex.

This is the logic layer of a shell: what focus is, what each action does to
it, and how a controller's intents get in. It draws nothing and names no
toolkit. Each product brings its own presentation.

## Why now

The rule is **extract on demand** — a component moves when a second consumer
proves where the boundary is, not before. `docs/platform-boundary.md` in the
superproject listed `focus.rs` and `navigation.rs` under *"fitted to RaNorTV's
shape — do not extract yet"*, and it was right to: extracting them then would
have made one product's interface shape the platform's, by accident, before
anything existed to disagree with it.

Apex is now that second consumer.
[ADR-0011](https://github.com/ranson21/apex-os/blob/master/docs/adr/0011-fabric-shell.md)
is the decision, and the finding behind it is that almost none of RaNorTV's
launcher is RaNorTV-specific: the product-specific part is the Slint UI and
five built-in tiles, and every supporting module was written without product
assumptions.

## What moved, and what did not

| Module | Role |
|---|---|
| `focus.rs` | The state machine: zones, tabs, indices, and what each action does |
| `navigation.rs` | Action names to actions |
| `gamepad.rs` | The pump: `fabric-input` devices and intents, drained per poll |

Six modules deliberately stayed in `ranortv-os`: `config`, `paths`, `loader`,
`models`, `store` and `actions`. They are neutral *code* carrying
product-shaped *values* — filesystem locations, manifest schemas, launch
mechanics — and the boundary document's third test is the one that catches
them:

> **Product-neutral is not the same as reusable.** A component can contain no
> RaNorTV concepts at all and still encode RaNorTV's *shape*.

Two smaller things stayed for the same reason, and are worth knowing because
ADR-0011's table implies otherwise:

- **Most of `navigation.rs`.** The table calls the module "action names →
  focus actions", and that description turns out to be `parse_action` alone.
  The other four functions named `LauncherState`, `AppWindow` and RaNorTV's
  four-tab order, so they stayed where those types are.
- **The timer in `gamepad.rs`.** The loop body came across verbatim; the eight
  lines that create a `slint::Timer` did not. A Fabric crate that depends on
  Slint makes one product's toolkit the platform's toolkit. `fabric-input`
  made the same call — "the crate does not own a thread" — because a launcher
  has a UI event loop and an emulator a frame loop.

## What this is not

**A redesign.** This is the same code, the same behaviour and the same tests,
somewhere both products can reach it. Apex needs a shape this model does not
have — stacked lanes within one tab, each remembering its own horizontal
position — and that is a change for the consumer who needs it to make,
separately, so that an extraction and a redesign can be reviewed apart.

## The focus model's deliberate behaviours

These are choices, not accidents, and the tests pin every one of them down:

- **Focus clamps, it does not wrap.** On a 10-foot interface a wrap looks like
  the list jumped, and the user has no cursor to reorient by.
- **Left/right stay on their row in a grid.** Row changes are up/down only.
- **Down from a short last row lands on the last item**, rather than stranding
  the user.
- **Each tab remembers where focus was.**
- **Up from the first row reaches the tab bar.**
- **Lists change underneath focus.** `reconcile` re-settles it when a
  catalogue arrives or an install finishes.

## Using it

```rust
use fabric_shell::{Action, FocusModel, Layout, Outcome, Tab};

let tabs = vec![Tab::new(Layout::Row, 5), Tab::new(Layout::Grid { columns: 3 }, 7)];
let mut focus = FocusModel::new(tabs.len());

match focus.handle(Action::Right, &tabs) {
    Outcome::Moved => { /* redraw, scroll into view */ }
    Outcome::Activated { tab, index } => { /* launch it */ }
    Outcome::Ignored => {}
    Outcome::ExitRequested => { /* the caller decides */ }
}
```

The controller path runs through the same actions, deliberately:

```rust
let mut pump = fabric_shell::Pump::new()?;           // needs `gilrs-backend`
// every fabric_shell::POLL, from whatever loop you already have:
for intent in pump.poll() {
    if let Some(action) = fabric_shell::parse_action(intent.as_name()) {
        focus.handle(action, &tabs);
    }
}
```

Routing controller intents through the same name-parsing a key press uses is
the point: the focus model then cannot behave differently for a controller
than for a remote, and one set of headless tests covers both.

## Features

`gilrs-backend` is **off by default**, forwarded to `fabric-input` rather than
re-decided. `gilrs` pulls in `libudev-sys`, whose build script probes
pkg-config at configure time, so a default-on feature would make every build
require libudev headers. `Pump::with_source` needs no backend at all; only
`Pump::new` does.

## Visibility, and what it costs a consumer

This repository is **private**. `fabric-input` beneath it is public, and that
asymmetry has a consequence:

> A workflow's default `GITHUB_TOKEN` can read only its own repository. A bare
> `actions/checkout` followed by `cargo` **cannot fetch this crate.**

Consuming it from CI needs a deploy key on this repository, or a fine-grained
PAT with read-only Contents, stored as an Actions secret and consumed by
`actions/checkout`'s `token:` or a git credential helper. Whoever adds the
dependency provisions the credential **in the same change**, because the
failure lands on that change and not on a later unrelated one. The full
statement lives in `ranor-fabric` at
`environments/global/repos/fabric-shell/terragrunt.hcl`.

## Building

```sh
cargo test          # 49 tests, no system dependencies
```

The `gilrs-backend` feature needs libudev headers; nothing else does.
