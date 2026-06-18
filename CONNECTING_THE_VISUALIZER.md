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

The snippet above is illustrative — drop the `update_planet` + `publish` calls into
whatever loop your orchestrator already runs to keep the picture live.

## What the visualizer shows

- **Planet kind** sets colour, size and rings. `register_planet` decides this.
- **Energy cells** glow yellow when `DummyPlanetState.energy_cells[i]` is `true`.
- **Rocket** appears when `DummyPlanetState.has_rocket` is `true`.
- **Dead planets** disappear once you call `set_alive(id, false)`.
- **Explorers** present when the galaxy is first built fly between planets as you
  update them with `set_explorer`.

The **first** snapshot fixes the layout (planet count, ids and kinds); later
snapshots only change the dynamic state. Register every planet — and record any
explorers — before the first `publish()` so they all appear.

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
