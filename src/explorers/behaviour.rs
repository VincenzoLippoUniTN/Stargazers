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

use common_game::logging::Channel;
use rand::Rng;
use super::explorer::AI;

/// ROAMER — hops to the first neighbour it can find, harvests a little on each
/// planet, and keeps moving. Good for the "explore the map" instance.
pub fn roaming_explorer(ai: AI) {
    loop {
        if ai.is_killed() { return; }
        // Learn what this planet actually supports before doing anything.
        let _ = ai.discover_resources();
        let _ = ai.discover_combinations();

        for r in ai.known_resources() {
            let _ = ai.generate(r);
        }
        for c in ai.known_combinations() {
            let _ = ai.combine(c);
        }

        ai.log(Channel::Debug, &format!("harvested on planet {}", ai.current_planet()));
        thread::sleep(Duration::from_millis(1000));

        // Move to a random live neighbour.
        if let Err(e) = ai.request_neighbors() {
            ai.log(Channel::Warning, &format!("neighbors failed: {e}"));
            thread::sleep(Duration::from_millis(1000));
            continue;
        }

        let neighbors = ai.neighbors();
        if neighbors.is_empty() {
            ai.log(Channel::Warning, "no live neighbours, waiting...");
            thread::sleep(Duration::from_millis(2000));
            continue;
        }

        let dst = neighbors[rand::thread_rng().gen_range(0..neighbors.len())];
        if let Err(e) = ai.travel(dst) {
            ai.log(Channel::Warning, &format!("travel to {dst} failed: {e}"));
            thread::sleep(Duration::from_millis(1000));
        }
    }
}

/// HOMEBODY — never travels. Sits on its starting planet and farms it as hard as
/// it can. Good for the "steady production" instance.
pub fn harvesting_explorer(ai: AI) {
    let mut consecutive_failures = 0u32;
    const GIVE_UP_AFTER: u32 = 5;

    loop {
        if ai.is_killed() { return; }
        let _ = ai.discover_resources();
        let _ = ai.discover_combinations();

        let mut any_success = false;

        for r in ai.known_resources() {
            if ai.generate(r).is_ok() {
                any_success = true;
            }
        }
        for c in ai.known_combinations() {
            if ai.combine(c).is_ok() {
                any_success = true;
            }
        }

        if any_success {
            consecutive_failures = 0;
            ai.log(Channel::Debug, &format!(
                "harvest tick — planet={}", ai.current_planet()
            ));
        } else {
            consecutive_failures += 1;
            ai.log(Channel::Warning, &format!(
                "planet {} uncooperative ({consecutive_failures}/{GIVE_UP_AFTER})",
                ai.current_planet()
            ));

            if consecutive_failures >= GIVE_UP_AFTER {
                consecutive_failures = 0;
                if ai.request_neighbors().is_ok() {
                    let neighbors = ai.neighbors();
                    if let Some(&dst) = neighbors.first() {
                        ai.log(Channel::Warning, &format!(
                            "giving up on planet {}, moving to {dst}", ai.current_planet()
                        ));
                        let _ = ai.travel(dst);
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(3000));
    }
}