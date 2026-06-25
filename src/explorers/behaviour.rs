//! Pluggable explorer AI behaviours.
//!
//! Each function here is a COMPLETE autonomous loop for one explorer. Pass it to
//! `FirstExplorer::new(..., behaviour)` and the explorer's AI thread will run it
//! once it has started. Behaviours only ever touch the `AI` handle's public API —
//! they never see `FirstExplorer`'s internals, which is exactly why they live in
//! their own file: you can write, swap, and test behaviours without touching the
//! explorer/concurrency plumbing.
//!
//! ADJUST THE IMPORT PATH: `crate::first_explorer::AI` assumes the explorer module
//! is `first_explorer`. Change it to wherever you put that module (e.g.
//! `crate::explorer::AI`). For this to compile, `AI` and its methods are
//! `pub(crate)` in the explorer module.

use std::thread;
use std::time::Duration;

use common_game::components::resource::{BasicResourceType, ComplexResourceType};
use common_game::logging::Channel;

use super::explorer::AI;

/// ROAMER — hops to the first neighbour it can find, harvests a little on each
/// planet, and keeps moving. Good for the "explore the map" instance.
pub fn roaming_explorer(ai: AI) {
    loop {
        if let Err(e) = ai.request_neighbors() {
            ai.log(Channel::Warning, &format!("request_neighbors failed: {e}"));
            thread::sleep(Duration::from_millis(2000));
            continue;
        }

        let neighbors = ai.neighbors();
        if let Some(&dst) = neighbors.first() {
            if let Err(e) = ai.travel(dst) {
                ai.log(Channel::Warning, &format!("travel to {dst} failed: {e}"));
                thread::sleep(Duration::from_millis(2000));
                continue;
            }
        }

        // Best-effort harvest; ignore individual failures and keep roaming.
        let _ = ai.discover_resources();
        let _ = ai.discover_combinations();
        let _ = ai.generate(BasicResourceType::Oxygen);
        let _ = ai.combine(ComplexResourceType::Water);

        ai.log(
            Channel::Debug,
            &format!("roamed; now on planet {}", ai.current_planet()),
        );
        thread::sleep(Duration::from_millis(2000));
    }
}

/// HOMEBODY — never travels. Sits on its starting planet and farms it as hard as
/// it can. Good for the "steady production" instance.
pub fn harvesting_explorer(ai: AI) {
    let _ = ai.discover_resources();
    let _ = ai.discover_combinations();

    loop {
        let _ = ai.generate(BasicResourceType::Hydrogen);
        let _ = ai.generate(BasicResourceType::Oxygen);
        if let Err(e) = ai.combine(ComplexResourceType::Water) {
            ai.log(Channel::Warning, &format!("combine Water failed: {e}"));
        }

        ai.log(
            Channel::Debug,
            &format!(
                "harvest tick — energy_cells={}, planet={}",
                ai.energy_cells(),
                ai.current_planet()
            ),
        );
        thread::sleep(Duration::from_millis(3000));
    }
}