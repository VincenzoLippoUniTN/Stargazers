//! Synthetic feed: scripted move, kill and add phases so the explorer sync
//! path can be verified end to end without the orchestrator.

use galaxy_visualizer_stargazers as viz;
use std::thread;
use std::time::Duration;

fn snapshot(explorers: &[(u32, u32)]) -> viz::GalaxySnapshot {
    viz::GalaxySnapshot {
        planets: [10, 20, 30]
            .into_iter()
            .map(|id| viz::PlanetSnapshot {
                id,
                kind: viz::PlanetKind::A,
                cells: vec![true, false, true, false, false],
                has_rocket: id == 20,
                alive: true,
            })
            .collect(),
        explorers: explorers
            .iter()
            .map(|&(id, at_planet)| viz::ExplorerSnapshot { id, at_planet })
            .collect(),
        edges: vec![(10, 20), (20, 30), (30, 10)],
    }
}

fn main() {
    let (sender, feed) = viz::galaxy_channel();

    thread::spawn(move || {
        let phases: [(&str, Vec<(u32, u32)>); 4] = [
            ("initial: 1@10 2@20", vec![(1, 10), (2, 20)]),
            ("move: 1 -> 20", vec![(1, 20), (2, 20)]),
            ("kill 2", vec![(1, 20)]),
            ("add 7@20", vec![(1, 20), (7, 20)]),
        ];
        for (label, explorers) in phases {
            eprintln!("[sim] phase: {label}");
            for _ in 0..25 {
                if sender.send(snapshot(&explorers)).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
        eprintln!("[sim] all phases done; holding final state");
        loop {
            if sender.send(snapshot(&[(1, 20), (7, 20)])).is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
    });

    viz::run_with_feed(feed);
}
