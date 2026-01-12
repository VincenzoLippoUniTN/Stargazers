// Loading "common-game" imports
use common_game::components::forge::Forge;
use common_game::components::planet::Planet;
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::ExplorerToPlanet;

// Loading explorers and giving them an alias
use crate::explorer::{BagItem, Explorer};

// Loading "create_planet" functions and giving them an alias
use the_compiler_strikes_back::planet::create_planet as new_csb;
use huston::houston_we_have_a_borrow as new_hus;
use one_million_crabs::planet::create_planet as new_omc;
use ara_kees::planet::create_planet as new_bas;
use trip::trip as new_trp;
use immutable_cosmic_borrow::create_planet as new_icb;
use rusty_crab_ap2025::planet::create_planet as new_ryc;

// Other imports
use log::error;
use crossbeam_channel::unbounded;

struct Orchestator {
    forge: Forge,
    csb: Planet, hus: Planet, omc: Planet, bas: Planet, trp: Planet, icb: Planet, ryc: Planet,
    anonymous: Explorer, eleanor: Explorer
}

impl Orchestator {
    fn new(forge: Forge,
           csb: Planet, hus: Planet, omc: Planet, bas: Planet, trp: Planet, icb: Planet, ryc: Planet,
           anonymous: Explorer, eleanor: Explorer) -> Orchestator {
        Orchestator{forge, csb, hus, omc, bas, trp, icb, ryc, anonymous, eleanor}
    }
}

fn build_orchestrator() -> Result<Orchestator, String>{

    // TODO: Setup all the crossbeam channels (Orchestrator <-> Planet, Orchestrator <-> Explorer, Explorer <-> Planet)

    let forge = Forge::new()?;  // Creating Forge
    // These are the only functions of the forge struct
    // let _ = forge.generate_asteroid();
    // let _ = forge.generate_sunray();

    // TODO: After setting up the channels, fill the functions below with the proper parameters
    let csb : Planet = new_csb();        // Creating CompilerStrikesBack planet
    let hus : Planet = new_hus()?;       // Creating HustonWeHaveABorrow planet
    let omc : Planet = new_omc()?;       // Creating OneMillionCrabs planet
    let bas : Planet = new_bas()?;       // Creating BlackAdidasShoe planet
    let trp : Planet = new_trp()?;       // Creating TRIP planet
    let icb : Planet = new_icb()?;       // Creating ImmutableCosmicBorrow planet
    let ryc : Planet = new_ryc();        // Creating RustyCrab planet

    // TODO: Just a placeholder. We're going to change this stuff when we properly start working on explorers
    let anon = Explorer::new("Anon".to_string());           // Creating Anon
    let eleanor = Explorer::new("Eleanor".to_string());     // Creating Eleanor

    Ok(Orchestator::new(forge, csb, hus, omc, bas, trp, icb, ryc, anon, eleanor))
}

fn run_orchestrator() {
    let orchestrator : Orchestator;
    match build_orchestrator() {
        Ok(orch) => { orchestrator = orch; },
        Err(e) => { error!("Orchestrator creation failed - {}", e); return }
    }

    // TODO: I don't know exactly what to do here, but we'll see going forward
}