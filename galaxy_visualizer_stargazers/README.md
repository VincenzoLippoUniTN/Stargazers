# Galaxy Visualizer

A 3D visualization of the Stargazers Advanced Programming galaxy, built with [Bevy](https://bevyengine.org/).

It runs in Demo or via live data.

## Demo

```bash
cargo run --release
```

## Connecting a simulation

The crate is also a library. A producer builds [`GalaxySnapshot`] values from its
own state and sends them through a channel; the visualizer reconciles the scene
with the latest snapshot it receives.

```rust
use galaxy_visualizer_stargazers as viz;
use std::thread;

fn main() {
    let (sender, feed) = viz::galaxy_channel();

    // Produce snapshots on a background thread.
    thread::spawn(move || loop {
        let snapshot = viz::GalaxySnapshot {
            planets: vec![viz::PlanetSnapshot {
                id: 1,
                kind: viz::PlanetKind::B,
                cells: vec![true],
                has_rocket: false,
                alive: true,
            }],
            explorers: vec![],
            edges: vec![],
        };
        if sender.send(snapshot).is_err() {
            break; // visualizer closed
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });

    // The window must run on the main thread.
    viz::run_with_feed(feed);
}
```

### Snapshot model

| Type | Fields |
|------|--------|
| `GalaxySnapshot` | `planets`, `explorers`, `edges` (pairs of planet ids; empty = default ring) |
| `PlanetSnapshot` | `id`, `kind` (`PlanetKind::A..D`), `cells: Vec<bool>`, `has_rocket`, `alive` |
| `ExplorerSnapshot` | `id`, `at_planet` (planet id) |

The first snapshot fixes the layout (how many planets, their kinds and ids).

Other snapshots update the dynamic parts, that is, which cells are charged, whether a
rocket is built, whether a planet is still alive, and where each explorer is.

The Stargazers orchestrator already speaks the `common-game` protocol; a ready-made
bridge that converts its `DummyPlanetState` into these snapshots lives in the
Stargazers repo - see `CONNECTING_THE_VISUALIZER.md` there.
