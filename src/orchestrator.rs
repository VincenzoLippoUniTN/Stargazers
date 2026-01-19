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


struct Orchestrator {
    forge: Forge,
    csb: Planet, hus: Planet, omc: Planet, bas: Planet, trp: Planet, icb: Planet, ryc: Planet,
    anonymous: Explorer, eleanor: Explorer
}

//struct helper to manage the planet's channels
struct PlanetChannels{
    to_planet: crossbeam_channel::Sender<OrchestratorToPlanet>,
    from_planet: crossbeam_channel::Receiver<PlanetToOrchestrator>,
}

impl Orchestrator {
    fn new(forge: Forge,
           csb: Planet, hus: Planet, omc: Planet, bas: Planet, trp: Planet, icb: Planet, ryc: Planet,
           anonymous: Explorer, eleanor: Explorer) -> Orchestrator {
        Orchestrator{forge, csb, hus, omc, bas, trp, icb, ryc, anonymous, eleanor}
    }
}

fn spawn_planet_thread(
    create_fn: impl FnOnce(//this function can be use only 1 time because he can consume his environment
        crossbeam_channel::Receiver<OrchestratorToPlanet>,
        crossbeam_channel::Sender<PlanetToOrchestrator>,
        crossbeam_channel::Receiver<common_game::protocols::planet_explorer::ExplorerToPlanet>

    ) -> common_game::components::planet::Planet + Send + 'static,//no lifetime problem
) -> (common_game::components::planet::Planet, PlanetChannels) {
    let (tx_to_planet, rx_to_planet) = unbounded::<OrchestratorToPlanet>();//create an illimitate channel
    let (tx_from_planet, rx_from_planet) = unbounded::<PlanetToOrchestrator>();
    let (tx_dummy, rx_dummy) = unbounded::<common_game::protocols::planet_explorer::ExplorerToPlanet>(); // placeholder per explorer

    // Creiamo il thread del pianeta
    //1 planet hear orchestrator message
    //2 planet responde to orchestrator
    //placeholder for explorer's messages
    let planet = create_fn(rx_to_planet, tx_from_planet.clone(), rx_dummy);
    //send message and receive it
    (planet, PlanetChannels { to_planet: tx_to_planet, from_planet: rx_from_planet })
}


fn build_orchestrator() -> Result<Orchestrator, String>{

    // TODO: Setup all the crossbeam channels (Orchestrator <-> Planet, Orchestrator <-> Explorer, Explorer <-> Planet)

    let forge = Forge::new()?;  // Creating Forge
    // These are the only functions of the forge struct
    // let _ = forge.generate_asteroid();
    // let _ = forge.generate_sunray();

    // TODO: After setting up the channels, fill the functions below with the proper parameters
    let (csb, csb_chan) = spawn_planet_thread(|rx, tx, rx_exp| {new_csb(rx, tx, rx_exp, 1)});        // Creating CompilerStrikesBack planet
    let (hus, hus_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_hus(rx, tx, rx_exp, 2, RocketStrategy::Safe, None).expect("Failed to create HustonWeHaveABorrow planet") });
    let (omc, omc_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_omc(rx, tx, rx_exp, 3,).expect("Failed to create OMC") });
    let (bas, bas_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_bas(rx, tx, rx_exp, 4).expect("Failed to create BAS") });
    let (trp, trp_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_trp(5, rx, tx, rx_exp).expect("Failed to create TRP") });
    let (icb, icb_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_icb(false,1.0,1.0,Duration::from_secs(60),Duration::from_secs(10),6, (rx,tx), rx_exp).expect("Failed to create ICB") });
    let (ryc, ryc_chan) = spawn_planet_thread(|rx, tx, rx_exp| { new_ryc(rx, tx, rx_exp, 7) });     // Creating RustyCrab planet

    // TODO: Just a placeholder. We're going to change this stuff when we properly start working on explorers
    let anon = Explorer::new("Anon".to_string());           // Creating Anon
    let eleanor = Explorer::new("Eleanor".to_string());     // Creating Eleanor

    Ok(Orchestrator::new(forge, csb, hus, omc, bas, trp, icb, ryc, anon, eleanor))
}

fn run_orchestrator() {
    let orchestrator : Orchestrator;
    match build_orchestrator() {
        Ok(orch) => { orchestrator = orch; },
        Err(e) => { error!("Orchestrator creation failed - {}", e); return }
    }

    // TODO: I don't know exactly what to do here, but we'll see going forward
}