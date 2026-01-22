// Loading "common-game" imports
use common_game::components::forge::Forge;
use common_game::components::planet::Planet;
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::ExplorerToPlanet;
use crossbeam_channel::unbounded;
use immutable_cosmic_borrow::Ai as IcbAI;
use std::thread;
use std::time::Duration;
// Loading explorers and giving them an alias
use crate::explorer::{BagItem, Explorer};
// Loading "create_planet" functions and giving them an alias
use the_compiler_strikes_back::planet::create_planet as new_csb;
use huston::{houston_we_have_a_borrow as new_hus, RocketStrategy};
use one_million_crabs::planet::create_planet as new_omc;
use ara_kees::planet::create_planet as new_bas;
use common_game::components::resource::BasicResourceType;
use common_game::logging::{ActorType, Channel, EventType, LogEvent, Participant, Payload};
use trip::trip as new_trp;
use immutable_cosmic_borrow::create_planet as new_icb;
use rusty_crab_ap2025::planet::create_planet as new_ryc;
// Other imports
use log::error;
use common_game::components::sunray::Sunray;


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
        planet.run(); // oppure planet.start()
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

        println!("{:?}",resp);

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

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orchestrator_full_flow() {
        run_orchestrator();
    }
}