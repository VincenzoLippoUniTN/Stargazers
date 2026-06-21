// =========================================================================
// STANDARD LIBRARY & EXTERNAL CRATES
// =========================================================================
use std::collections::HashSet;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender};
use log::error;

// =========================================================================
// COMMON-GAME IMPORTS
// =========================================================================
use common_game::components::forge::Forge;
use common_game::components::resource::BasicResourceType;
use common_game::logging::{ActorType, Channel, EventType, LogEvent, Participant, Payload};
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};

// =========================================================================
// INTERNAL MODULES
// =========================================================================
use crate::explorer::{BagItem, Explorer};
use crate::first_explorer::FirstExplorer;

// =========================================================================
// PLANET CREATION ALIASES
// =========================================================================
use the_compiler_strikes_back::planet::create_planet as new_csb;
use huston::{houston_we_have_a_borrow as new_hus, RocketStrategy};
use one_million_crabs::planet::create_planet as new_omc;
use ara_kees::planet::create_planet as new_bas;
use trip::trip as new_trp;
use immutable_cosmic_borrow::create_planet as new_icb;
use rusty_crab_ap2025::planet::create_planet as new_ryc;

// =========================================================================
// STRUCTS
// =========================================================================

/// Helper struct to manage the communication channels for each planet
struct PlanetChannels {
    to_planet: Sender<OrchestratorToPlanet>,
    from_planet: Receiver<PlanetToOrchestrator>,
}

struct Orchestrator {
    forge: Forge,

    csb_chan: PlanetChannels,
    hus_chan: PlanetChannels,
    omc_chan: PlanetChannels,
    bas_chan: PlanetChannels,
    trp_chan: PlanetChannels,
    icb_chan: PlanetChannels,
    ryc_chan: PlanetChannels,

    anonymous: Explorer,
    eleanor: Explorer,
}

impl Orchestrator {
    fn new(
        forge: Forge,
        csb_chan: PlanetChannels,
        hus_chan: PlanetChannels,
        omc_chan: PlanetChannels,
        bas_chan: PlanetChannels,
        trp_chan: PlanetChannels,
        icb_chan: PlanetChannels,
        ryc_chan: PlanetChannels,
        anonymous: Explorer,
        eleanor: Explorer,
    ) -> Orchestrator {
        Orchestrator {
            forge,
            csb_chan,
            hus_chan,
            omc_chan,
            bas_chan,
            trp_chan,
            icb_chan,
            ryc_chan,
            anonymous,
            eleanor,
        }
    }
}

// =========================================================================
// SETUP & INITIALIZATION FUNCTIONS
// =========================================================================

/// Spawns a planet in a detached thread and returns its communication channels
fn spawn_planet_thread(
    rx_exp: Receiver<ExplorerToPlanet>,
    create_fn: impl FnOnce(
        Receiver<OrchestratorToPlanet>,
        Sender<PlanetToOrchestrator>,
        Receiver<ExplorerToPlanet>
    ) -> common_game::components::planet::Planet + Send + 'static,
) -> PlanetChannels {
    let (tx_to_planet, rx_to_planet) = unbounded::<OrchestratorToPlanet>();
    let (tx_from_planet, rx_from_planet) = unbounded::<PlanetToOrchestrator>();

    let mut planet = create_fn(rx_to_planet, tx_from_planet.clone(), rx_exp);

    thread::spawn(move || {
        planet.run();
    });

    PlanetChannels {
        to_planet: tx_to_planet,
        from_planet: rx_from_planet,
    }
}

/// Builds the Orchestrator by instantiating all planets and explorers with matched channels
fn build_orchestrator(
    rx_anon_exp: Receiver<ExplorerToPlanet>,
    rx_eleanor_exp: Receiver<ExplorerToPlanet>,
) -> Result<Orchestrator, String> {
    let forge = Forge::new()?;

    let csb_chan = spawn_planet_thread(rx_anon_exp, |rx, tx, rx_exp| { new_csb(rx, tx, rx_exp, 1) });
    let hus_chan = spawn_planet_thread(rx_eleanor_exp, |rx, tx, rx_exp| {
        new_hus(rx, tx, rx_exp, 2, RocketStrategy::Safe, None).expect("Failed to create HUS")
    });

    let (_tx_d3, rx_d3) = unbounded();
    let (_tx_d4, rx_d4) = unbounded();
    let (_tx_d5, rx_d5) = unbounded();
    let (_tx_d6, rx_d6) = unbounded();
    let (_tx_d7, rx_d7) = unbounded();

    let omc_chan = spawn_planet_thread(rx_d3, |rx, tx, rx_exp| { new_omc(rx, tx, rx_exp, 3).expect("Failed to create OMC") });
    let bas_chan = spawn_planet_thread(rx_d4, |rx, tx, rx_exp| { new_bas(rx, tx, rx_exp, 4).expect("Failed to create BAS") });
    let trp_chan = spawn_planet_thread(rx_d5, |rx, tx, rx_exp| { new_trp(5, rx, tx, rx_exp).expect("Failed to create TRP") });
    let icb_chan = spawn_planet_thread(rx_d6, |rx, tx, rx_exp| {
        new_icb(false, 1.0, 1.0, Duration::from_secs(60), Duration::from_secs(10), 6, (rx, tx), rx_exp).expect("Failed to create ICB")
    });
    let ryc_chan = spawn_planet_thread(rx_d7, |rx, tx, rx_exp| { new_ryc(rx, tx, rx_exp, 7) });

    let anon = Explorer::new("Anon".to_string());
    let eleanor = Explorer::new("Eleanor".to_string());

    Ok(Orchestrator::new(
        forge, csb_chan, hus_chan, omc_chan, bas_chan, trp_chan, icb_chan, ryc_chan, anon, eleanor,
    ))
}

// =========================================================================
// MAIN RUN LOGIC
// =========================================================================

fn run_orchestrator() {
    let mut dead_planets: HashSet<&str> = HashSet::new();

    // --- Prepare Explorer Channels ---
    let (tx_to_anon, rx_to_anon) = unbounded::<OrchestratorToExplorer>();
    let (tx_from_anon, rx_from_anon) = unbounded::<ExplorerToOrchestrator<()>>();
    let (tx_anon_to_planet, rx_planet_from_anon) = unbounded::<ExplorerToPlanet>();
    let (_tx_planet_to_anon, rx_anon_from_planet) = unbounded::<PlanetToExplorer>();

    let (tx_to_eleanor, rx_to_eleanor) = unbounded::<OrchestratorToExplorer>();
    let (tx_from_eleanor, rx_from_eleanor) = unbounded::<ExplorerToOrchestrator<()>>();
    let (tx_eleanor_to_planet, rx_planet_from_eleanor) = unbounded::<ExplorerToPlanet>();
    let (_tx_planet_to_eleanor, rx_eleanor_from_planet) = unbounded::<PlanetToExplorer>();

    let orchestrator = match build_orchestrator(rx_planet_from_anon, rx_planet_from_eleanor) {
        Ok(o) => o,
        Err(e) => {
            error!("Orchestrator creation failed - {}", e);
            return;
        }
    };

    let planets = vec![
        ("CSB", &orchestrator.csb_chan),
        ("HUS", &orchestrator.hus_chan),
        ("OMC", &orchestrator.omc_chan),
        ("BAS", &orchestrator.bas_chan),
        ("TRP", &orchestrator.trp_chan),
        ("ICB", &orchestrator.icb_chan),
        ("RYC", &orchestrator.ryc_chan),
    ];

    // ============================
    // TEST 1 — START AI
    // ============================
    for (name, chan) in &planets {
        println!("Orchestrator → {} : StartPlanetAI", name);
        let _resp = chan.to_planet.send(OrchestratorToPlanet::StartPlanetAI);

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(msg) => {
                if let Some(id) = msg.as_start_planet_ai_result() {
                    println!("✅ {} AI started (planet_id={})", name, id);
                } else {
                    println!("⚠️ {} unexpected msg: {:?}", name, msg);
                }
            }
            Err(_) => {
                println!("❌ {} no StartPlanetAIResult received", name);
            }
        }
    }

    // ============================
    // TEST 2 — INTERNAL STATE
    // ============================
    for (name, chan) in &planets {
        println!("Orchestrator → {} : InternalStateRequest", name);
        let _ = chan.to_planet.send(OrchestratorToPlanet::InternalStateRequest);

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(msg) => match msg {
                PlanetToOrchestrator::InternalStateResponse { planet_id, planet_state } => {
                    println!("✅ {} state received (id={})", name, planet_id);
                    println!("   DummyPlanetState: {:?}", planet_state);
                }
                PlanetToOrchestrator::Stopped { planet_id } => {
                    println!("⚠️ {} is stopped (planet_id={})", name, planet_id);
                }
                other => {
                    println!("⚠️ {} unexpected msg {:?}", name, other);
                }
            },
            Err(_) => {
                println!("❌ {} no response received", name);
            }
        }
    }

    // ============================
    // TEST 3 — STOP AI
    // ============================
    for (name, chan) in &planets {
        println!("Orchestrator → {} : StopPlanetAI", name);
        let _ = chan.to_planet.send(OrchestratorToPlanet::StopPlanetAI);

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(msg) => match msg {
                PlanetToOrchestrator::Stopped { planet_id }
                | PlanetToOrchestrator::StopPlanetAIResult { planet_id } => {
                    println!("🛑 {} AI stopped (planet_id={})", name, planet_id);
                }
                other => {
                    println!("⚠️ {} unexpected msg {:?}", name, other);
                }
            },
            Err(_) => {
                println!("❌ {} no StopPlanetAIResult received", name);
            }
        }
    }

    // ============================
    // TEST 4 — START AI (AGAIN)
    // ============================
    for (name, chan) in &planets {
        println!("Orchestrator → {} : StartPlanetAI", name);
        let _resp = chan.to_planet.send(OrchestratorToPlanet::StartPlanetAI);

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(msg) => {
                if let Some(id) = msg.as_start_planet_ai_result() {
                    println!("✅ {} AI started (planet_id={})", name, id);
                } else {
                    println!("⚠️ {} unexpected msg: {:?}", name, msg);
                }
            }
            Err(_) => {
                println!("❌ {} no StartPlanetAIResult received", name);
            }
        }
    }

    // ============================
    // TEST 5 — SUNRAY
    // ============================
    for (name, chan) in &planets {
        let sunray = orchestrator.forge.generate_sunray();
        println!("Orchestrator → {} : Sunray", name);
        let _resp = chan.to_planet.send(OrchestratorToPlanet::Sunray(sunray));

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(PlanetToOrchestrator::SunrayAck { planet_id }) => {
                println!("✅ {} acknowledged Sunray (planet_id={})", name, planet_id);
            }
            Ok(other) => {
                println!("⚠️ {} unexpected msg {:?}", name, other);
            }
            Err(_) => {
                println!("❌ {} no SunrayAck received", name);
            }
        }
    }

    // ============================
    // TEST 6 — ASTEROID
    // ============================
    for (name, chan) in &planets {
        if dead_planets.contains(name) {
            println!("⚠️ {} is dead, skipping message", name);
            continue;
        }
        let asteroid = orchestrator.forge.generate_asteroid();
        println!("Orchestrator → {} : Asteroid", name);
        let _ = chan.to_planet.send(OrchestratorToPlanet::Asteroid(asteroid));

        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
            Ok(PlanetToOrchestrator::AsteroidAck { planet_id, rocket }) => {
                match rocket {
                    Some(_) => {
                        println!("✅ {} built a rocket and survived asteroid (planet_id={})", name, planet_id)
                    }
                    None => {
                        println!("💀 {} could not build a rocket, stopping AI and killing planet (planet_id={})", name, planet_id);

                        // Send KillPlanet
                        let _ = chan.to_planet.send(OrchestratorToPlanet::KillPlanet);

                        // Wait for the kill result
                        match chan.from_planet.recv_timeout(Duration::from_secs(2)) {
                            Ok(PlanetToOrchestrator::KillPlanetResult { planet_id }) => {
                                println!("✅ {} planet AI killed (planet_id={})", name, planet_id);
                                dead_planets.insert(*name);
                            }
                            Ok(other) => {
                                println!("⚠️ {} unexpected msg after KillPlanet: {:?}", name, other);
                            }
                            Err(_) => {
                                println!("❌ {} no KillPlanetResult received", name);
                            }
                        }

                        // Stop the planet's AI
                        let _ = chan.to_planet.send(OrchestratorToPlanet::StopPlanetAI);

                        // Close the channels connected to the planet
                        drop(chan.to_planet.clone());
                        drop(chan.from_planet.clone());
                        println!("🗑️ {} channels closed", name);
                    }
                }
            }
            Ok(PlanetToOrchestrator::Stopped { planet_id }) => {
                println!("💀 {} was already stopped/destroyed (planet_id={})", name, planet_id);
            }
            Ok(other) => {
                println!("⚠️ {} unexpected msg {:?}", name, other);
            }
            Err(_) => {
                println!("❌ {} no asteroid response received", name);
            }
        }
    }

    // ============================
    // TEST 7 — SUNRAY (POST-ASTEROID)
    // ============================
    for (name, chan) in &planets {
        if dead_planets.contains(name) {
            println!("⚠️ {} is dead, skipping Sunray", name);
            continue;
        }

        let sunray = orchestrator.forge.generate_sunray();
        println!("Orchestrator → {} : Sunray", name);
        let _ = chan.to_planet.send(OrchestratorToPlanet::Sunray(sunray));
    }

    // =========================================================================
    // EXECUTE THREAD RUNNERS FOR THE EXPLORERS
    // =========================================================================

    // --- Run Explorer "Anon" ---
    let explorer_anon = FirstExplorer::new("Anon".to_string());
    thread::spawn(move || {
        explorer_anon.run(
            rx_to_anon,
            tx_from_anon,
            tx_anon_to_planet,
            rx_anon_from_planet,
            101
        );
    });

    // --- Run Explorer "Eleanor" ---
    let explorer_eleanor = FirstExplorer::new("Eleanor".to_string());
    thread::spawn(move || {
        explorer_eleanor.run(
            rx_to_eleanor,
            tx_from_eleanor,
            tx_eleanor_to_planet,
            rx_eleanor_from_planet,
            102
        );
    });

    let explorers = vec![
        ("Anon", &tx_to_anon, &rx_from_anon),
        ("Eleanor", &tx_to_eleanor, &rx_from_eleanor),
    ];

    // =========================================================================
    // EXPLORER TEST 1: SUPPORTED RESOURCES
    // =========================================================================
    println!("\n--- START TEST TRANSITION: SUPPORTED RESOURCES ---");

    for (name, tx_to_exp, rx_from_exp) in &explorers {
        println!("Orchestrator → Explorer ({}) : SupportedResourceRequest", name);
        let _ = tx_to_exp.send(OrchestratorToExplorer::SupportedResourceRequest);

        match rx_from_exp.recv_timeout(Duration::from_secs(2)) {
            Ok(ExplorerToOrchestrator::SupportedResourceResult { explorer_id, supported_resources }) => {
                println!(
                    "✅ Received from {} (ID: {}): SupportedResourceResult. Resources on the planet: {:?}",
                    name, explorer_id, supported_resources
                );
            }
            Ok(other) => {
                println!("⚠️ Message received {}: {:?}", name, other);
            }
            Err(_) => {
                println!("❌ Error/Timeout: No response from Explorer {}", name);
            }
        }
    }
    println!("--- END TEST TRANSITION: SUPPORTED RESOURCES ---\n");

    // =========================================================================
    // EXPLORER TEST 2: SUPPORTED COMBINATIONS
    // =========================================================================
    println!("\n--- START TEST TRANSITION: SUPPORTED COMBINATIONS ---");

    for (name, tx_to_exp, rx_from_exp) in &explorers {
        println!("Orchestrator → Explorer ({}) : SupportedCombinationRequest", name);
        let _ = tx_to_exp.send(OrchestratorToExplorer::SupportedCombinationRequest);

        match rx_from_exp.recv_timeout(Duration::from_secs(2)) {
            Ok(ExplorerToOrchestrator::SupportedCombinationResult { explorer_id, combination_list }) => {
                println!(
                    "✅ Received from {} (ID: {}): SupportedCombinationResult. Combinations: {:?}",
                    name, explorer_id, combination_list
                );
            }
            Ok(other) => {
                println!("⚠️ Unexpected message from {}: {:?}", name, other);
            }
            Err(_) => {
                println!("❌ Error/Timeout: No response from Explorer {}", name);
            }
        }
    }
    println!("--- END TEST TRANSITION: SUPPORTED COMBINATIONS ---\n");

    // =========================================================================
    // EXPLORER TEST 3: GENERATE RESOURCE
    // =========================================================================
    println!("\n--- START TEST TRANSITION: GENERATE RESOURCE ---");

    // Custom commands tailored per individual explorer context
    let target_resources = vec![
        ("Anon", &tx_to_anon, &rx_from_anon, BasicResourceType::Oxygen),
        ("Eleanor", &tx_to_eleanor, &rx_from_eleanor, BasicResourceType::Silicon),
    ];

    for (name, tx_to_exp, rx_from_exp, resource_type) in target_resources {
        println!("Orchestrator → Explorer ({}) : GenerateResourceRequest for {:?}", name, resource_type);

        let _ = tx_to_exp.send(OrchestratorToExplorer::GenerateResourceRequest {
            to_generate: resource_type,
        });

        match rx_from_exp.recv_timeout(Duration::from_secs(2)) {
            Ok(ExplorerToOrchestrator::GenerateResourceResponse { explorer_id, generated }) => {
                match generated {
                    Ok(()) => {
                        println!(
                            "✅ Received from {} (ID: {}): GenerateResourceResponse -> SUCCESS! Resource successfully accumulated.",
                            name, explorer_id
                        );
                    }
                    Err(error_msg) => {
                        println!(
                            "⚠️ Received from {} (ID: {}): GenerateResourceResponse -> FAILED. Reason: {}",
                            name, explorer_id, error_msg
                        );
                    }
                }
            }
            Ok(other) => {
                println!("⚠️ Unexpected message from {}: {:?}", name, other);
            }
            Err(_) => {
                println!("❌ Error/Timeout: No response from Explorer {} during resource generation request", name);
            }
        }
    }
    println!("--- END TEST TRANSITION: GENERATE RESOURCE ---\n");

    // =========================================================================
    // EXPLORER TEST 4: COMBINE RESOURCE
    // =========================================================================
    println!("\n--- START TEST TRANSITION: COMBINE RESOURCE ---");

    let combine_targets = vec![
        ("Anon", &tx_to_anon, &rx_from_anon, common_game::components::resource::ComplexResourceType::Water),
    ];

    for (name, tx_to_exp, rx_from_exp, resource_type) in combine_targets {
        println!("Orchestrator → Explorer ({}) : CombineResourceRequest for {:?}", name, resource_type);

        let _ = tx_to_exp.send(OrchestratorToExplorer::CombineResourceRequest {
            to_generate: resource_type,
        });

        match rx_from_exp.recv_timeout(Duration::from_secs(2)) {
            Ok(ExplorerToOrchestrator::GenerateResourceResponse { explorer_id, generated }) => {
                match generated {
                    Ok(()) => {
                        println!(
                            "✅ Received from {} (ID: {}): CombineResourceResponse -> SUCCESS! Complex resource combined.",
                            name, explorer_id
                        );
                    }
                    Err(error_msg) => {
                        println!(
                            "⚠️ Received from {} (ID: {}): CombineResourceResponse -> FAILED. Reason: {}",
                            name, explorer_id, error_msg
                        );
                    }
                }
            }
            Ok(other) => {
                println!("⚠️ Unexpected message from {}: {:?}", name, other);
            }
            Err(_) => {
                println!("❌ Error/Timeout: No response from Explorer {} during resource combination request", name);
            }
        }
    }
    println!("--- END TEST TRANSITION: COMBINE RESOURCE ---\n");
}

// =========================================================================
// TESTS
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orchestrator_full_flow() {
        run_orchestrator();
    }
}