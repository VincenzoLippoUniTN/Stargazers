# Connecting Stargazers to the Galaxy Visualizer

This repo can drive the 3D galaxy visualizer (vendored alongside it in
[`galaxy_visualizer_stargazers/`](galaxy_visualizer_stargazers)) so you can *see*
the galaxy your orchestrator is running: planets, charged energy cells, rockets,
dead planets and explorers.

The visualizer never talks to your planets directly. You keep ownership of all the
`common-game` channels; you just hand the visualizer a stream of plain
**snapshots** describing the galaxy. A ready-made bridge that turns the
`DummyPlanetState` you already poll into those snapshots lives in
[`src/visualizer.rs`](src/visualizer.rs).

```
planets ──InternalStateRequest──► orchestrator ──VizBridge──► GalaxySnapshot ──► visualizer
```

## 1. It's already wired (optionally)

The visualizer is a workspace member of this repo and is wired up as an **optional**
dependency behind the `visualizer` feature, so a normal `cargo build` stays light
and doesn't compile Bevy. The relevant bits in the root `Cargo.toml`:

```toml
[workspace]
members = ["StargazersPlanet", "galaxy_visualizer_stargazers"]

[dependencies]
galaxy_visualizer_stargazers = { path = "galaxy_visualizer_stargazers", optional = true }

[features]
visualizer = ["dep:galaxy_visualizer_stargazers"]
```

And the bridge module is feature-gated in `src/main.rs`:

```rust
#[cfg(feature = "visualizer")]
mod visualizer;
```

Run the orchestrator *with* the view by enabling the feature (this is the build
that pulls in Bevy, so the first one takes a few minutes):

```bash
cargo run --features visualizer
```

You can also run the standalone demo without touching the orchestrator:

```bash
cargo run -p galaxy_visualizer_stargazers
```

## 2. Drive it from the orchestrator

The bridge has a tiny API:

| Method | When to call it |
|--------|-----------------|
| `VizBridge::new(sender)` | once, at startup |
| `register_planet(id, kind)` | once per planet, with the type you created it as |
| `update_planet(id, &dummy_state)` | every time you receive an `InternalStateResponse` |
| `set_alive(id, false)` | when a planet fails to deflect an asteroid |
| `set_explorer(explorer_id, planet_id)` | when an explorer moves (optional) |
| `set_edges(vec![(a, b), ...])` | optional; otherwise planets are laid out in a ring |
| `publish()` | whenever you want the view to refresh |

`use crate::visualizer::{kind_of, VizBridge};` gives you `kind_of(...)` to convert a
`common_game::components::planet::PlanetType` into the visualizer's `PlanetKind`.

### Wiring it into `run_orchestrator`

The visualizer window must run on the **main thread**, so run the orchestrator on a
background thread and keep the main thread for the window:

```rust
use galaxy_visualizer_stargazers as viz;
use crate::visualizer::{kind_of, VizBridge};
use common_game::components::planet::PlanetType;

fn main() {
    let (sender, feed) = viz::galaxy_channel();

    std::thread::spawn(move || {
        let mut bridge = VizBridge::new(sender);

        // Register every planet (with the type you spawned it as) before the
        // first publish, so they all appear.
        bridge.register_planet(1, kind_of(PlanetType::A)); // CSB
        bridge.register_planet(2, kind_of(PlanetType::A)); // HUS
        bridge.register_planet(3, kind_of(PlanetType::C)); // OMC
        // ...the rest...

        loop {
            // Poll each planet, reusing your existing channels.
            for (_name, chan) in &planets {
                chan.to_planet.send(OrchestratorToPlanet::InternalStateRequest).ok();
                if let Ok(PlanetToOrchestrator::InternalStateResponse { planet_id, planet_state }) =
                    chan.from_planet.recv_timeout(Duration::from_secs(2))
                {
                    bridge.update_planet(planet_id, &planet_state);
                }
            }

            // When a planet can't build a rocket against an asteroid:
            // bridge.set_alive(planet_id, false);

            // Refresh the view; stop once the window is closed.
            if !bridge.publish() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });

    viz::run_with_feed(feed); // blocks until the window closes
}
```

The snippet above is illustrative - drop the `update_planet` + `publish` calls into
whatever loop your orchestrator already runs to keep the picture live.

## 3. Manual operations (the input channel)

State flows *out* as snapshots; manual operations flow back *in* as commands. The
visualizer only ever **emits an intent** - it never touches your planets. Your
orchestrator drains those intents and runs them through the *same* code its AI
uses, so a button press and an automatic action are one and the same. The result
shows up on the next snapshot, keeping the orchestrator the single source of truth.

Swap `run_with_feed` for `run_with_io` and pass it a command sink; keep the
matching source on the orchestrator side:

```rust
use crate::visualizer::{command_channel, galaxy_channel, run_with_io, CommandSource, GalaxyCommand};

let (sender, feed) = galaxy_channel();      // snapshots: orchestrator -> view
let (sink, commands) = command_channel();   // commands:  view -> orchestrator

std::thread::spawn(move || {
    let mut bridge = VizBridge::new(sender);
    // ...register planets, then your loop...
    loop {
        // 1. drain manual operations and run them like the AI would
        for cmd in commands.drain() {
            handle_command(&forge, &channels, cmd);
        }
        // 2. poll planets, update the bridge, publish (as in section 2)
        // ...
        if !bridge.publish() { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
});

run_with_io(feed, sink); // blocks until the window closes
```

`handle_command` is the bridge between an intent and a real `OrchestratorToPlanet`
message - and the key point is that each arm calls the **same helper your AI
calls**, so there's exactly one code path per operation:

```rust
fn handle_command(forge: &Forge, channels: &PlanetChannels, cmd: GalaxyCommand) {
    match cmd {
        GalaxyCommand::Sunray { planet_id } => {
            // identical to the automatic sun ray your AI sends
            let ray = forge.generate_sunray();
            channels.of(planet_id).to_planet.send(OrchestratorToPlanet::Sunray(ray)).ok();
        }
        GalaxyCommand::Asteroid { planet_id } => {
            let rock = forge.generate_asteroid();
            channels.of(planet_id).to_planet.send(OrchestratorToPlanet::Asteroid(rock)).ok();
        }
        GalaxyCommand::SetAi { planet_id, running } => {
            let msg = if running { OrchestratorToPlanet::StartPlanetAI }
                      else       { OrchestratorToPlanet::StopPlanetAI };
            channels.of(planet_id).to_planet.send(msg).ok();
        }
        GalaxyCommand::Kill { planet_id } => {
            channels.of(planet_id).to_planet.send(OrchestratorToPlanet::KillPlanet).ok();
        }
        GalaxyCommand::MoveExplorer { explorer_id, to_planet } => {
            // route your explorer here (IncomingExplorerRequest / OutgoingExplorerRequest)
        }
    }
}
```

The UI now has a button *and* a keyboard shortcut for **every** operation a user
can perform, all routed through one `dispatch` so a click and a keypress do the
exact same thing. The full `GalaxyCommand` surface:

| Command | Button / key | Target |
|---------|--------------|--------|
| `Sunray` | Sun ray `S` | focused planet |
| `Asteroid` | Asteroid `A` | focused planet |
| `Kill` | Kill planet `K` | focused planet |
| `SetAi` | Toggle AI `I` | whole simulation |
| `MoveExplorer` | Move explorer `E` | selected explorer |
| `KillExplorer` | Kill explorer `J` | selected explorer |
| `ResetExplorer` | Reset explorer `R` | selected explorer |
| `BagContent` | Bag `B` | selected explorer |
| `SupportedResources` | Resources `1` | selected explorer |
| `SupportedCombinations` | Combines `2` | selected explorer |
| `Generate` | Basic+ `N` to pick, Generate `G` | selected explorer |
| `Combine` | Complex+ `M` to pick, Combine `C` | selected explorer |

The view controls (mode `P`, focus `←`/`→`, zoom, pause `Space`) stay client-side
and never reach you. Pick which explorer the explorer-ops act on with **Sel
explorer `X`** (it cycles through the live explorers; the first is auto-selected).

In demo mode (no command sink) the operations that make sense animate locally and
the rest are no-ops, so the standalone demo keeps working without a backend.

## 4. Query answers (the report channel)

`Sunray`/`Asteroid`/`Kill`/`Move` change the galaxy, so their result shows up on
the next **snapshot**. But the *query* commands - `BagContent`,
`SupportedResources`, `SupportedCombinations`, and the outcome of
`Generate`/`Combine` - ask a question a snapshot can't answer (a snapshot only
describes physical galaxy state, not an explorer's inventory or a planet's recipe
list). Those answers come back on a separate **report** channel and are shown in
the HUD.

Use `run_with_reports` instead of `run_with_io` and keep the matching
`ReportSender` on the orchestrator side:

```rust
use galaxy_visualizer_stargazers as viz;

let (sender, feed)      = viz::galaxy_channel();  // snapshots: orchestrator -> view
let (sink, commands)    = viz::command_channel(); // commands:  view -> orchestrator
let (reports, feed_rep) = viz::report_channel();  // answers:   orchestrator -> view

std::thread::spawn(move || {
    // ...build the orchestrator with `commands` and `reports`, then your loop...
    // When an explorer replies with its bag / recipe list, forward it:
    //   reports.send(viz::GalaxyReport::Bag { explorer_id, basic, complex });
});

viz::run_with_reports(feed, sink, feed_rep); // blocks until the window closes
```

`GalaxyReport` carries pre-formatted strings, so the visualizer never needs to
know your resource or bag types. The orchestrator maps each explorer result
(`BagContentResponse`, `SupportedResourceResult`, …) to the matching
`GalaxyReport` variant.

## What the visualizer shows

- **Planet kind** sets colour, size and rings. `register_planet` decides this.
- **Energy cells** glow yellow when `DummyPlanetState.energy_cells[i]` is `true`.
- **Rocket** appears when `DummyPlanetState.has_rocket` is `true`.
- **Dead planets** disappear once you call `set_alive(id, false)`.
- **Explorers** present when the galaxy is first built fly between planets as you
  update them with `set_explorer`. An explorer **disappears** as soon as it stops
  being reported in a snapshot - so call `remove_explorer(id)` when one dies and
  it leaves the screen on the next `publish()`.

The **first** snapshot fixes the layout (planet count, ids and kinds); later
snapshots only change the dynamic state. Register every planet - and record any
explorers - before the first `publish()` so they all appear.

## Snapshot model (if you'd rather not use the bridge)

You can build `viz::GalaxySnapshot` yourself and call `sender.send(...)`:

```rust
viz::GalaxySnapshot {
    planets: vec![viz::PlanetSnapshot {
        id: 1,
        kind: viz::PlanetKind::B,
        cells: vec![true],     // one entry per energy cell
        has_rocket: false,
        alive: true,
    }],
    explorers: vec![viz::ExplorerSnapshot { id: 0, at_planet: 1 }],
    edges: vec![],             // empty = default ring
};
```
