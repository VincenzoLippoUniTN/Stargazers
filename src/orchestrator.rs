use std::collections::HashSet;
// Loading "common-game" imports
use crate::first_explorer::FirstExplorer;
use common_game::components::forge::Forge;
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::ExplorerToPlanet;
use crossbeam_channel::unbounded;
use std::thread;
use std::time::Duration;
// Loading explorers and giving them an alias
use crate::explorer::{BagItem, Explorer};
// Loading "create_planet" functions and giving them an alias
use the_compiler_strikes_back::planet::create_planet as new_csb;
use huston::{houston_we_have_a_borrow as new_hus, RocketStrategy};
use one_million_crabs::planet::create_planet as new_omc;
use ara_kees::planet::create_planet as new_bas;
use common_game::logging::{ActorType, Channel, EventType, LogEvent, Participant, Payload};
use trip::trip as new_trp;
use immutable_cosmic_borrow::create_planet as new_icb;
use rusty_crab_ap2025::planet::create_planet as new_ryc;
// Other imports
use log::error;


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


//struct helper to manage the planet's channels
struct PlanetChannels{
    to_planet: crossbeam_channel::Sender<OrchestratorToPlanet>,
    from_planet: crossbeam_channel::Receiver<PlanetToOrchestrator>,
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

fn spawn_planet_thread(
    create_fn: impl FnOnce(//this function can be use only 1 time because he can consume his environment
        crossbeam_channel::Receiver<OrchestratorToPlanet>,
        crossbeam_channel::Sender<PlanetToOrchestrator>,
        crossbeam_channel::Receiver<common_game::protocols::planet_explorer::ExplorerToPlanet>

    ) -> common_game::components::planet::Planet + Send + 'static,//no lifetime problem
) -> (PlanetChannels) {
    let (tx_to_planet, rx_to_planet) = unbounded::<OrchestratorToPlanet>();
    let (tx_from_planet, rx_from_planet) = unbounded::<PlanetToOrchestrator>();
    let (_tx_dummy, rx_dummy) = unbounded::<ExplorerToPlanet>();

    let mut planet = create_fn(rx_to_planet, tx_from_planet.clone(), rx_dummy);

    thread::spawn(move || {
        planet.run();
    });

    PlanetChannels {
        to_planet: tx_to_planet,
        from_planet: rx_from_planet,
    }
}

fn build_orchestrator() -> Result<Orchestrator, String> {
    let forge = Forge::new()?;

    let (csb_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_csb(rx, tx, rx_exp, 1) });
    let (hus_chan) = spawn_planet_thread(|rx, tx, rx_exp| {
        new_hus(rx, tx, rx_exp, 2, RocketStrategy::Safe, None).expect("Failed to create HUS")
    });
    let (omc_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_omc(rx, tx, rx_exp, 3).expect("Failed to create OMC") });
    let (bas_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_bas(rx, tx, rx_exp, 4).expect("Failed to create BAS") });
    let (trp_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_trp(5, rx, tx, rx_exp).expect("Failed to create TRP") });
    let (icb_chan) = spawn_planet_thread(|rx, tx, rx_exp| {
        new_icb(false, 1.0, 1.0, Duration::from_secs(60), Duration::from_secs(10), 6, (rx, tx), rx_exp).expect("Failed to create ICB")
    });
    let (ryc_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_ryc(rx, tx, rx_exp, 7) });

    let anon = Explorer::new("Anon".to_string());
    let eleanor = Explorer::new("Eleanor".to_string());

    Ok(Orchestrator::new(
        forge,
        csb_chan,
        hus_chan,
        omc_chan,
        bas_chan,
        trp_chan,
        icb_chan,
        ryc_chan,
        anon, eleanor,
    ))
}


fn run_orchestrator() {

    let mut dead_planets: HashSet<&str> = HashSet::new();

    let orchestrator = match build_orchestrator() {
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
        let resp = chan.to_planet.send(OrchestratorToPlanet::StartPlanetAI);

        //println!("{:?}",resp);

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
                | PlanetToOrchestrator::StopPlanetAIResult { planet_id } => { // <-- aggiunto
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
    // TEST 4 — START AI
    // ============================
    for (name, chan) in &planets {
        println!("Orchestrator → {} : StartPlanetAI", name);
        let resp = chan.to_planet.send(OrchestratorToPlanet::StartPlanetAI);

        //println!("{:?}",resp);

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
    // TEST 3 — SUNRAY
    // ============================
    for (name, chan) in &planets {
        let sunray = orchestrator.forge.generate_sunray();
        println!("Orchestrator → {} : Sunray", name);
        let resp = chan.to_planet.send(OrchestratorToPlanet::Sunray(sunray));
        println!("{:?}",resp);

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

    // =========================================================================
    // INIRIALIZE CHANNELS AND THREAD OF THE EXPLORER (Con FirstExplorer)
    // =========================================================================

    // configuration Explorer "Anon"
    let (tx_to_anon, rx_to_anon) = unbounded::<OrchestratorToExplorer>();
    let (tx_from_anon, rx_from_anon) = unbounded::<ExplorerToOrchestrator<()>>();
    let explorer_anon = FirstExplorer::new("Anon".to_string());
    thread::spawn(move || {
        explorer_anon.run(rx_to_anon, tx_from_anon, 101);
    });

    // configuration Explorer "Eleanor"
    let (tx_to_eleanor, rx_to_eleanor) = unbounded::<OrchestratorToExplorer>();
    let (tx_from_eleanor, rx_from_eleanor) = unbounded::<ExplorerToOrchestrator<()>>();
    let explorer_eleanor = FirstExplorer::new("Eleanor".to_string());
    thread::spawn(move || {
        explorer_eleanor.run(rx_to_eleanor, tx_from_eleanor, 102);
    });
    
    //vector to iterate in the different explorer's channels
    let explorers = vec![
        ("Anon", &tx_to_anon, &rx_from_anon),
        ("Eleanor", &tx_to_eleanor, &rx_from_eleanor),
    ];

    //FIRST EXPLORER TEST\\
    println!("\n--- START TEST TRANSITION EXPLORER-ORCHESTRATOR ---");

    for (name, tx_to_exp, rx_from_exp) in &explorers {

        // 1. Send
        println!("Orchestrator → Explorer ({}) : SupportedResourceRequest", name);
        let _ = tx_to_exp.send(OrchestratorToExplorer::SupportedResourceRequest);

        // Response
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

    println!("--- END TEST EXPLORER-ORCHESTRATOR ---\n");

    // ============================
    // TEST 4 — ASTEROID
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

                        // Poi invia KillPlanet
                        let _ = chan.to_planet.send(OrchestratorToPlanet::KillPlanet);

                        // Attende il risultato del kill
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

                        // Ferma l'AI del pianeta
                        let _ = chan.to_planet.send(OrchestratorToPlanet::StopPlanetAI);

                        // Chiude il canale verso il pianeta
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
    // TEST 3 — SUNRAY
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



}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orchestrator_full_flow() {
        run_orchestrator();
    }
}