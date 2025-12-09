use crossbeam_channel::{Sender, Receiver};
use common_game::components::planet::{Planet, PlanetAI, PlanetState, PlanetType};
use common_game::components::resource::{BasicResourceType, ComplexResourceType, Combinator, Generator, BasicResource, ComplexResourceRequest, ComplexResource};
use common_game::components::rocket::Rocket;
use common_game::protocols::messages;
use common_game::logging::{LogEvent, ActorType, EventType, Channel, Payload};
use log::{info, error};

// Group-defined AI struct
struct AI {
    started: bool,
    stopped: bool,
}

impl AI {
    fn log_resource_generation(
        explorer_id: u32,
        planet_id: u32,
        resource_type: BasicResourceType,
        success: bool,
        amount: Option<u32>,
        error_msg: Option<String>,
    ) {
        let mut payload = Payload::new();
        payload.insert("action".to_string(), "generate_resource".to_string());
        match resource_type {
            BasicResourceType::Oxygen =>  payload.insert("resource_type".to_string(), "Oxygen".to_string()),
            BasicResourceType::Hydrogen =>  payload.insert("resource_type".to_string(), "Hydrogen".to_string()),
            BasicResourceType::Carbon =>  payload.insert("resource_type".to_string(), "Carbon".to_string()),
            BasicResourceType::Silicon =>  payload.insert("resource_type".to_string(), "Silicon".to_string())
        };
        payload.insert("status".to_string(), if success { "success" } else { "failed" }.to_string());

        if let Some(amt) = amount {
            payload.insert("amount".to_string(), amt.to_string());
        }
        if let Some(err) = error_msg {
            payload.insert("error".to_string(), err);
        }

        LogEvent::new(
            ActorType::Explorer,
            explorer_id,
            ActorType::Planet,
            planet_id.to_string(),
            EventType::InternalPlanetAction,
            if success { Channel::Info } else { Channel::Warning },
            payload,
        ).emit();
    }
}

impl PlanetAI for AI {
    fn handle_orchestrator_msg(
        &mut self,
        state: &mut PlanetState,
        _generator: &Generator,
        _combinator: &Combinator,
        msg: messages::OrchestratorToPlanet
    ) -> Option<messages::PlanetToOrchestrator> {
        match msg {
            messages::OrchestratorToPlanet::Sunray(s) => {
                info!("Planet {} receive the Sunray from the Orchestrator",state.id());
                state.charge_cell(s);
                info!("Planet {}: Cell charged", state.id());
                Some(messages::PlanetToOrchestrator::SunrayAck { planet_id: state.id() })
            }

            messages::OrchestratorToPlanet::InternalStateRequest => {
                info!("Planet {} receive the request of information from the Orchestrator",state.id());
                let dummy_state = state.to_dummy();
                Some(messages::PlanetToOrchestrator::InternalStateResponse {
                    planet_id: state.id(),
                    planet_state: dummy_state,
                })
            }

            _ => None
        }
    }

    fn handle_explorer_msg(
        &mut self,
        state: &mut PlanetState,
        generator: &Generator,
        combinator: &Combinator,
        msg: messages::ExplorerToPlanet
    ) -> Option<messages::PlanetToExplorer> {
        match msg {
            // This variant is used to ask the Planet for the available [BasicResourceType]
            messages::ExplorerToPlanet::SupportedResourceRequest { explorer_id } => {
                // Log incoming request
                let mut payload = Payload::new();
                payload.insert("action".to_string(), "supported_resource_request".to_string());
                LogEvent::new(
                    ActorType::Explorer,
                    explorer_id,
                    ActorType::Planet,
                    state.id().to_string(),
                    EventType::MessageExplorerToPlanet,
                    Channel::Debug,
                    payload,
                ).emit();

                Some(messages::PlanetToExplorer::SupportedResourceResponse { resource_list: generator.all_available_recipes() })
            },

            // This variant is used to ask the Planet for the available [ComplexResourceType]
            messages::ExplorerToPlanet::SupportedCombinationRequest { explorer_id } => {
                // Log incoming request
                let mut payload = Payload::new();
                payload.insert("action".to_string(), "supported_combination_request".to_string());
                LogEvent::new(
                    ActorType::Explorer,
                    explorer_id,
                    ActorType::Planet,
                    state.id().to_string(),
                    EventType::MessageExplorerToPlanet,
                    Channel::Debug,
                    payload,
                ).emit();

                Some(messages::PlanetToExplorer::SupportedCombinationResponse { combination_list: combinator.all_available_recipes() })
            },

            // This variant is used to ask the Planet to generate a [BasicResource].
            // Three expected outcomes:
            //  - None => the planet is not able to generate said resource, no charge available
            //  - Some(GenerateResourceResponse { resource: None }) => the planet could generate the resource, but generation failed
            //  - Some(GenerateResourceResponse { resource: Some(requested_resource) }) => the planet successfully generated the resource
            // Explorer is expected to handle said outcomes.
            messages::ExplorerToPlanet::GenerateResourceRequest { explorer_id , resource } => {
                // Checking with planet state if charged cell is available
                if let Some((charged_cell, _)) = state.full_cell() {
                    // Matching generation with requested type
                    match resource {
                        BasicResourceType::Oxygen => {
                            match generator.make_oxygen(charged_cell) {
                                Ok(oxygen) => {
                                    // Log successful resource creation
                                    AI::log_resource_generation(explorer_id, state.id(), resource, true, Some(1), None);
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Oxygen(oxygen)) })
                                }
                                Err(msg) => {
                                    // Log unsuccessful resource creation & show msg
                                    AI::log_resource_generation(explorer_id, state.id(), resource, false, None, Some(msg.to_string()));
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Carbon => {
                            match generator.make_carbon(charged_cell) {
                                Ok(carbon) => {
                                    // Log successful resource creation
                                    AI::log_resource_generation(explorer_id, state.id(), resource, true, Some(1), None);
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Carbon(carbon)) })
                                }
                                Err(msg) => {
                                    // Log unsuccessful resource creation & show msg
                                    AI::log_resource_generation(explorer_id, state.id(), resource, false, None, Some(msg.to_string()));
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Hydrogen => {
                            match generator.make_hydrogen(charged_cell) {
                                Ok(hydrogen) => {
                                    // Log successful resource creation
                                    AI::log_resource_generation(explorer_id, state.id(), resource, true, Some(1), None);
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Hydrogen(hydrogen)) })
                                }
                                Err(msg) => {
                                    // Log unsuccessful resource creation & show msg
                                    AI::log_resource_generation(explorer_id, state.id(), resource, false, None, Some(msg.to_string()));
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Silicon => {
                            match generator.make_silicon(charged_cell) {
                                Ok(silicon) => {
                                    // Log successful resource creation
                                    AI::log_resource_generation(explorer_id, state.id(), resource, true, Some(1), None);
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Silicon(silicon)) })
                                }
                                Err(msg) => {
                                    // Log unsuccessful resource creation & show msg
                                    AI::log_resource_generation(explorer_id, state.id(), resource, false, None, Some(msg.to_string()));
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                    }
                } else {
                    // Log failed request as no charges available
                    let mut payload = Payload::new();
                    payload.insert("action".to_string(), "generate_resource".to_string());
                    match resource {
                        BasicResourceType::Oxygen =>  payload.insert("resource_type".to_string(), "Oxygen".to_string()),
                        BasicResourceType::Hydrogen =>  payload.insert("resource_type".to_string(), "Hydrogen".to_string()),
                        BasicResourceType::Carbon =>  payload.insert("resource_type".to_string(), "Carbon".to_string()),
                        BasicResourceType::Silicon =>  payload.insert("resource_type".to_string(), "Silicon".to_string())
                    };
                    payload.insert("status".to_string(), "rejected".to_string());
                    payload.insert("reason".to_string(), "no_charged_cells".to_string());
                    LogEvent::new(
                        ActorType::Explorer,
                        explorer_id,
                        ActorType::Planet,
                        state.id().to_string(),
                        EventType::InternalPlanetAction,
                        Channel::Warning,
                        payload,
                    ).emit();

                    None
                }
            },

            // This variant is used to ask the Planet to generate a [ComplexResource] using the [ComplexResourceRequest]
            // [ComplexResourceRequest] = [ComplexResource]([BasicResource], [BasicResource])
            // Two expected outcomes:
            //  - Some(CombineResourceResponse { complex_response: Err((err_msg, r1, r2)) }) => the planet couldn't create the resource
            //  - Some(CombineResourceResponse { complex_response: Ok(requested_resource) }) => the planet successfully create the resource
            // Explorer is expected to handle said outcomes.
            messages::ExplorerToPlanet::CombineResourceRequest { explorer_id , msg } => {
                // Parsing [ComplexResourceRequest] into a [ComplexResourceType] and [GenericResource]s
                let (r_type, r1, r2) = match msg {
                    ComplexResourceRequest::Robot(r1, r2) => {
                        (ComplexResourceType::Robot, r1.to_generic(), r2.to_generic())
                    },
                    ComplexResourceRequest::Water(r1, r2) => {
                        (ComplexResourceType::Water, r1.to_generic(), r2.to_generic())
                    },
                    ComplexResourceRequest::Diamond(r1, r2) => {
                        (ComplexResourceType::Diamond, r1.to_generic(), r2.to_generic())
                    },
                    ComplexResourceRequest::Life(r1, r2) => {
                        (ComplexResourceType::Life, r1.to_generic(), r2.to_generic())
                    },
                    ComplexResourceRequest::Dolphin(r1, r2) => {
                        (ComplexResourceType::Dolphin, r1.to_generic(), r2.to_generic())
                    },
                    ComplexResourceRequest::AIPartner(r1, r2) => {
                        (ComplexResourceType::AIPartner, r1.to_generic(), r2.to_generic())
                    }
                };

                // Checking with planet state if charged cell is available
                if let Some((charged_cell, _)) = state.full_cell(){
                    // Matching generation with requested type
                    match r_type {
                        ComplexResourceType::Robot => {
                            let silicon = match r1.to_silicon() {
                                Ok(r) => r,
                                Err(msg) => { panic!("Failed conversion from [GenericResource] to [Silicon] - Err: {}", msg) }
                            };
                            let life = match r2.to_life() {
                                Ok(r) => r,
                                Err(msg) => { panic!("Failed conversion from [GenericResource] to [Life] - Err: {}", msg) }
                            };

                            match combinator.make_robot(silicon, life, charged_cell) {
                                Ok(robot) => {
                                    // Log successful resource creation
                                    let mut payload = Payload::new();
                                    payload.insert("action".to_string(), "combine_resource".to_string());
                                    payload.insert("resource_type".to_string(), "Robot".to_string());
                                    payload.insert("status".to_string(), "success".to_string());
                                    payload.insert("input_1".to_string(), "Silicon".to_string());
                                    payload.insert("input_2".to_string(), "Life".to_string());
                                    LogEvent::new(
                                        ActorType::Explorer,
                                        explorer_id,
                                        ActorType::Planet,
                                        state.id().to_string(),
                                        EventType::InternalPlanetAction,
                                        Channel::Info,
                                        payload,
                                    ).emit();

                                    Some(messages::PlanetToExplorer::CombineResourceResponse {
                                        complex_response: Ok(ComplexResource::Robot(robot))
                                    })
                                },
                                Err((msg, r1, r2)) => {
                                    // Log failed resource creation
                                    let mut payload = Payload::new();
                                    payload.insert("action".to_string(), "combine_resource".to_string());
                                    payload.insert("resource_type".to_string(), "Robot".to_string());
                                    payload.insert("status".to_string(), "failed".to_string());
                                    payload.insert("error".to_string(), msg.clone());
                                    payload.insert("input_1".to_string(), "Silicon".to_string());
                                    payload.insert("input_2".to_string(), "Life".to_string());
                                    LogEvent::new(
                                        ActorType::Explorer,
                                        explorer_id,
                                        ActorType::Planet,
                                        state.id().to_string(),
                                        EventType::InternalPlanetAction,
                                        Channel::Warning,
                                        payload,
                                    ).emit();

                                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1.to_generic(), r2.to_generic())) })
                                }
                            }
                        }
                        _ => {
                            let msg = match r_type {
                                ComplexResourceType::Diamond => "Recipe not available, failed combination - returning [r1 : Carbon] and [r2 : Carbon]".to_string(),
                                ComplexResourceType::Water => "Recipe not available, failed combination - returning [r1 : Hydrogen] and [r2 : Oxygen]".to_string(),
                                ComplexResourceType::Life => "Recipe not available, failed combination - returning [r1 : Water] and [r2 : Carbon]".to_string(),
                                ComplexResourceType::Dolphin => "Recipe not available, failed combination - returning [r1 : Water] and [r2 : Life]".to_string(),
                                ComplexResourceType::AIPartner => "Recipe not available, failed combination - returning [r1 : Robot] and [r2 : Diamond]".to_string(),
                                _ => "Pretty strange behaviour... Shouldn't be possible to be here, but here we are...".to_string()
                            };

                            // Log failed request as recipe not exists
                            let mut payload = Payload::new();
                            payload.insert("action".to_string(), "combine_resource".to_string());
                            payload.insert("resource_type".to_string(), format!("{:?}", r_type));
                            payload.insert("status".to_string(), "rejected".to_string());
                            payload.insert("reason".to_string(), "recipe_not_available".to_string());
                            payload.insert("error".to_string(), msg.clone());
                            LogEvent::new(
                                ActorType::Explorer,
                                explorer_id,
                                ActorType::Planet,
                                state.id().to_string(),
                                EventType::InternalPlanetAction,
                                Channel::Warning,
                                payload,
                            ).emit();

                            Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1, r2)) })
                        }
                    }
                } else {
                    let msg = match r_type {
                        ComplexResourceType::Diamond => "No charged cell, failed combination - returning [r1 : Carbon] and [r2 : Carbon]".to_string(),
                        ComplexResourceType::Water => "No charged cell, failed combination - returning [r1 : Hydrogen] and [r2 : Oxygen]".to_string(),
                        ComplexResourceType::Life => "No charged cell, failed combination - returning [r1 : Water] and [r2 : Carbon]".to_string(),
                        ComplexResourceType::Robot => "No charged cell, failed combination - returning [r1 : Silicon] and [r2 : Life]".to_string(),
                        ComplexResourceType::Dolphin => "No charged cell, failed combination - returning [r1 : Water] and [r2 : Life]".to_string(),
                        ComplexResourceType::AIPartner => "No charged cell, failed combination - returning [r1 : Robot] and [r2 : Diamond]".to_string()
                    };

                    // Log failed request as no charges available
                    let mut payload = Payload::new();
                    payload.insert("action".to_string(), "combine_resource".to_string());
                    payload.insert("resource_type".to_string(), format!("{:?}", r_type));
                    payload.insert("status".to_string(), "rejected".to_string());
                    payload.insert("reason".to_string(), "no_charged_cells".to_string());
                    payload.insert("error".to_string(), msg.clone());
                    LogEvent::new(
                        ActorType::Explorer,
                        explorer_id,
                        ActorType::Planet,
                        state.id().to_string(),
                        EventType::InternalPlanetAction,
                        Channel::Warning,
                        payload,
                    ).emit();

                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1, r2)) })
                }
            },

            // This variant is used to ask the Planet for the available energy_cells number
            messages::ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id } => {
                // Log incoming energy cell availability request
                let mut payload = Payload::new();
                payload.insert("action".to_string(), "available_energy_cell_request".to_string());
                payload.insert("available_cells".to_string(), state.cells_count().to_string());
                LogEvent::new(
                    ActorType::Explorer,
                    explorer_id,
                    ActorType::Planet,
                    state.id().to_string(),
                    EventType::MessageExplorerToPlanet,
                    Channel::Debug,
                    payload,
                ).emit();

                Some(messages::PlanetToExplorer::AvailableEnergyCellResponse { available_cells: state.cells_count() as u32 })
            }
        }
    }

    fn handle_asteroid(
        &mut self,
        state: &mut PlanetState,
        _generator: &Generator,
        _combinator: &Combinator,
    ) -> Option<Rocket> {
        // 1. Search a cell charged
        match state.full_cell() {
            None => {
                info!("Planet {}: NO charged cells, asteroid will destroy the planet!", state.id());
                None
            }

            Some((_cell, idx)) => {
                match state.build_rocket(idx) {
                    Ok(_) => {
                        info!("Planet {}: Rocket successfully built!", state.id());
                        state.take_rocket()
                    }
                    Err(e) => {
                        error!(
                            "Planet {}: Failed to build rocket: {}",
                            state.id(),
                            e
                        );
                        None
                    }
                }
            }
        }
    }

    fn start(&mut self, state: &PlanetState) {
        info!("Planet {}: AI started!", state.id());
        self.started = true;
        self.stopped = false;
    }

    fn stop(&mut self, state: &PlanetState) {
        info!("Planet {}: AI stopped!", state.id());
        self.stopped = true;
        self.started = false;
    }
}

// This is the group's "export" function. It will be called by
// the orchestrator to spawn your planet.
pub fn create_planet(
    rx_orchestrator: Receiver<messages::OrchestratorToPlanet>,
    tx_orchestrator: Sender<messages::PlanetToOrchestrator>,
    rx_explorer: Receiver<messages::ExplorerToPlanet>
) -> Planet {
    let id = 1;
    let ai = AI {
        started: false,
        stopped: false,
    };
    let gen_rules = vec![
        BasicResourceType::Oxygen,
        BasicResourceType::Carbon,
        BasicResourceType::Hydrogen,
        BasicResourceType::Silicon
    ];
    let comb_rules = vec![
        ComplexResourceType::Robot,
    ];

    // Construct the planet and return it
    match Planet::new(
        id,
        PlanetType::B,
        Box::new(ai),
        gen_rules,
        comb_rules,
        (rx_orchestrator, tx_orchestrator),
        rx_explorer,
    ) {
        Ok(planet) => {
            // TODO: Log planet creation success
            info!("Planet {} created!", planet.id());
            planet
        }
        Err(msg) => {
            // TODO: Log planet creation failure
            error!("Planet {} creation failed: {}", id, msg);
            panic!("Planet {} created with error: {}", id, msg);
        }
    }
}


// =============================================================================
// COMPREHENSIVE UNIT TESTS FOR PLANET AI (lib.rs)
// =============================================================================
// Features:
// - Logging enabled via env_logger (all levels shown)
// - Similar tests compacted where appropriate
// - Full integration test coverage
//
// To use:
// 1. Add to Cargo.toml under [dev-dependencies]: env_logger = "0.11"
// 2. Replace the existing #[cfg(test)] mod tests block in lib.rs with this
// 3. Run with: cargo test -- --nocapture
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::sync::Once;
    use std::thread;
    use std::time::Duration;

    use common_game::components::asteroid::Asteroid;
    use common_game::components::planet::{Planet, PlanetType};
    use common_game::components::resource::{
        BasicResource, BasicResourceType, ComplexResourceType,
    };
    use common_game::components::sunray::Sunray;
    use common_game::protocols::messages::{
        ExplorerToPlanet, OrchestratorToPlanet, PlanetToExplorer, PlanetToOrchestrator,
    };

    // =========================================================================
    // LOGGING INITIALIZATION
    // =========================================================================

    static INIT: Once = Once::new();

    fn init_logger() {
        INIT.call_once(|| {
            env_logger::builder()
                .is_test(true)
                .filter_level(log::LevelFilter::Trace) // Show all log levels
                .init();
        });
    }

    // =========================================================================
    // TEST HELPERS
    // =========================================================================

    type PlanetToOrchChannels = (
        crossbeam_channel::Receiver<OrchestratorToPlanet>,
        crossbeam_channel::Sender<PlanetToOrchestrator>,
    );
    type OrchToPlanetChannels = (
        crossbeam_channel::Sender<OrchestratorToPlanet>,
        crossbeam_channel::Receiver<PlanetToOrchestrator>,
    );

    fn setup_test_channels() -> (
        PlanetToOrchChannels,
        crossbeam_channel::Receiver<ExplorerToPlanet>,
        OrchToPlanetChannels,
        crossbeam_channel::Sender<ExplorerToPlanet>,
    ) {
        let (tx_orch_to_planet, rx_orch_to_planet) = unbounded::<OrchestratorToPlanet>();
        let (tx_planet_to_orch, rx_planet_to_orch) = unbounded::<PlanetToOrchestrator>();
        let (tx_expl_to_planet, rx_expl_to_planet) = unbounded::<ExplorerToPlanet>();

        (
            (rx_orch_to_planet, tx_planet_to_orch),
            rx_expl_to_planet,
            (tx_orch_to_planet, rx_planet_to_orch),
            tx_expl_to_planet,
        )
    }

    fn setup_planet_with_channels() -> (
        Planet,
        OrchToPlanetChannels,
        crossbeam_channel::Sender<ExplorerToPlanet>,
    ) {
        let (planet_channels, rx_explorer, orch_channels, tx_explorer) = setup_test_channels();
        let planet = create_planet(planet_channels.0, planet_channels.1, rx_explorer);
        (planet, orch_channels, tx_explorer)
    }

    fn spawn_planet() -> (
        thread::JoinHandle<()>,
        OrchToPlanetChannels,
        crossbeam_channel::Sender<ExplorerToPlanet>,
    ) {
        let (mut planet, orch_channels, tx_explorer) = setup_planet_with_channels();

        let handle = thread::spawn(move || {
            if let Err(e) = planet.run() {
                eprintln!("Planet thread error: {}", e);
            }
        });

        (handle, orch_channels, tx_explorer)
    }

    fn start_planet(
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) {
        tx.send(OrchestratorToPlanet::StartPlanetAI).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::StartPlanetAIResult { .. }) => {}
            Ok(_) => panic!("Expected StartPlanetAIResult, got different message"),
            Err(e) => panic!("Expected StartPlanetAIResult, got error: {}", e),
        }
        thread::sleep(Duration::from_millis(10));
    }

    fn stop_planet(
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) {
        tx.send(OrchestratorToPlanet::StopPlanetAI).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::StopPlanetAIResult { .. }) => {}
            Ok(_) => panic!("Expected StopPlanetAIResult, got different message"),
            Err(e) => panic!("Expected StopPlanetAIResult, got error: {}", e),
        }
    }

    fn kill_planet(
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) {
        tx.send(OrchestratorToPlanet::KillPlanet).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::KillPlanetResult { .. }) => {}
            Ok(_) => panic!("Expected KillPlanetResult, got different message"),
            Err(e) => panic!("Expected KillPlanetResult, got error: {}", e),
        }
    }

    fn register_explorer(
        explorer_id: u32,
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) -> crossbeam_channel::Receiver<PlanetToExplorer> {
        let (expl_tx, expl_rx) = unbounded::<PlanetToExplorer>();
        tx.send(OrchestratorToPlanet::IncomingExplorerRequest {
            explorer_id,
            new_mpsc_sender: expl_tx,
        })
            .unwrap();

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::IncomingExplorerResponse { res, .. }) => {
                assert!(res.is_ok(), "Explorer registration should succeed");
            }
            Ok(_) => panic!("Expected IncomingExplorerResponse, got different message"),
            Err(e) => panic!("Expected IncomingExplorerResponse, got error: {}", e),
        }

        expl_rx
    }

    fn unregister_explorer(
        explorer_id: u32,
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) {
        tx.send(OrchestratorToPlanet::OutgoingExplorerRequest { explorer_id })
            .unwrap();

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::OutgoingExplorerResponse { res, .. }) => {
                assert!(res.is_ok(), "Explorer unregistration should succeed");
            }
            Ok(_) => panic!("Expected OutgoingExplorerResponse, got different message"),
            Err(e) => panic!("Expected OutgoingExplorerResponse, got error: {}", e),
        }
    }

    fn send_sunray(
        tx: &crossbeam_channel::Sender<OrchestratorToPlanet>,
        rx: &crossbeam_channel::Receiver<PlanetToOrchestrator>,
    ) {
        tx.send(OrchestratorToPlanet::Sunray(Sunray::default()))
            .unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::SunrayAck { .. }) => {}
            Ok(_) => panic!("Expected SunrayAck, got different message"),
            Err(e) => panic!("Expected SunrayAck, got error: {}", e),
        }
    }

    // =========================================================================
    // UNIT TESTS: AI STRUCT
    // =========================================================================

    #[test]
    fn test_ai_initial_state() {
        init_logger();

        let ai = AI {
            started: false,
            stopped: false,
        };
        assert!(!ai.started);
        assert!(!ai.stopped);

        info!("AI initial state verified: started={}, stopped={}", ai.started, ai.stopped);
    }

    // =========================================================================
    // UNIT TESTS: CREATE_PLANET FUNCTION (COMPACTED)
    // =========================================================================

    #[test]
    fn test_create_planet_properties() {
        init_logger();

        let (planet_channels, rx_explorer, _, _) = setup_test_channels();
        let planet = create_planet(planet_channels.0, planet_channels.1, rx_explorer);

        // ID check
        assert_eq!(planet.id(), 1, "Planet ID should be 1");

        // Type check
        assert!(matches!(planet.planet_type(), PlanetType::B), "Planet should be Type B");

        // Energy cells check (Type B has 1 cell)
        assert_eq!(planet.state().cells_count(), 1, "Type B should have 1 energy cell");

        // Rocket capability check (Type B cannot have rockets)
        assert!(!planet.state().can_have_rocket(), "Type B cannot have rockets");
        assert!(!planet.state().has_rocket(), "Planet should start without rocket");

        // Generator recipes check
        let generator = planet.generator();
        assert!(generator.contains(BasicResourceType::Oxygen), "Should have Oxygen recipe");
        assert!(generator.contains(BasicResourceType::Hydrogen), "Should have Hydrogen recipe");
        assert!(generator.contains(BasicResourceType::Carbon), "Should have Carbon recipe");
        assert!(generator.contains(BasicResourceType::Silicon), "Should have Silicon recipe");

        // Combinator recipes check
        let comb = planet.combinator();
        assert!(comb.contains(ComplexResourceType::Robot), "Should have Robot recipe");
        assert!(!comb.contains(ComplexResourceType::Water), "Should NOT have Water recipe");
        assert!(!comb.contains(ComplexResourceType::Diamond), "Should NOT have Diamond recipe");
        assert!(!comb.contains(ComplexResourceType::Life), "Should NOT have Life recipe");
        assert!(!comb.contains(ComplexResourceType::Dolphin), "Should NOT have Dolphin recipe");
        assert!(!comb.contains(ComplexResourceType::AIPartner), "Should NOT have AIPartner recipe");

        info!("Planet properties verified: id={}, type=B, cells=1, recipes=4 basic + 1 complex", planet.id());
    }

    // =========================================================================
    // INTEGRATION TESTS: PLANET LIFECYCLE
    // =========================================================================

    #[test]
    fn test_planet_starts_in_stopped_state() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();

        // Send a request without starting - should get Stopped response
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::Stopped { planet_id }) => {
                assert_eq!(planet_id, 1);
                info!("Planet correctly responded with Stopped when not started");
            }
            Ok(_) => panic!("Expected Stopped, got different message"),
            Err(e) => panic!("Expected Stopped, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_planet_start_stop_restart_cycle() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();

        // Start
        info!("Starting planet...");
        start_planet(&tx, &rx);

        // Verify running by sending a request
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::InternalStateResponse { .. }) => {
                info!("Planet is running - responded to state request");
            }
            Ok(_) => panic!("Expected InternalStateResponse while running"),
            Err(e) => panic!("Expected response while running, got error: {}", e),
        }

        // Stop
        info!("Stopping planet...");
        stop_planet(&tx, &rx);

        // Verify stopped
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::Stopped { .. }) => {
                info!("Planet is stopped - responded with Stopped");
            }
            Ok(_) => panic!("Expected Stopped after stopping"),
            Err(e) => panic!("Expected Stopped, got error: {}", e),
        }

        // Restart
        info!("Restarting planet...");
        start_planet(&tx, &rx);

        // Verify running again
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::InternalStateResponse { .. }) => {
                info!("Planet restarted successfully");
            }
            Ok(_) => panic!("Expected InternalStateResponse after restart"),
            Err(e) => panic!("Expected response after restart, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_planet_kill_from_any_state() {
        init_logger();

        // Test kill from stopped state (never started)
        {
            let (handle, (tx, rx), _) = spawn_planet();
            info!("Testing kill from initial stopped state...");

            tx.send(OrchestratorToPlanet::KillPlanet).unwrap();
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PlanetToOrchestrator::KillPlanetResult { planet_id }) => {
                    assert_eq!(planet_id, 1);
                    info!("Kill from stopped state: OK");
                }
                Ok(_) => panic!("Expected KillPlanetResult"),
                Err(e) => panic!("Expected KillPlanetResult, got error: {}", e),
            }
            handle.join().expect("Planet thread panicked");
        }

        // Test kill from running state
        {
            let (handle, (tx, rx), _) = spawn_planet();
            info!("Testing kill from running state...");

            start_planet(&tx, &rx);

            tx.send(OrchestratorToPlanet::KillPlanet).unwrap();
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PlanetToOrchestrator::KillPlanetResult { planet_id }) => {
                    assert_eq!(planet_id, 1);
                    info!("Kill from running state: OK");
                }
                Ok(_) => panic!("Expected KillPlanetResult"),
                Err(e) => panic!("Expected KillPlanetResult, got error: {}", e),
            }
            handle.join().expect("Planet thread panicked");
        }
    }

    // =========================================================================
    // INTEGRATION TESTS: SUNRAY HANDLING
    // =========================================================================

    #[test]
    fn test_sunray_handling() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();
        start_planet(&tx, &rx);

        // Single sunray
        info!("Sending single sunray...");
        send_sunray(&tx, &rx);

        // Verify cell is charged via internal state
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::InternalStateResponse { planet_state, .. }) => {
                assert_eq!(planet_state.charged_cells_count, 1, "Cell should be charged");
                info!("Cell charged: {}/1", planet_state.charged_cells_count);
            }
            Ok(_) => panic!("Expected InternalStateResponse"),
            Err(e) => panic!("Expected state response, got error: {}", e),
        }

        // Multiple sunrays (Type B only has 1 cell, extras should be ignored/returned)
        info!("Sending additional sunrays...");
        for i in 0..3 {
            tx.send(OrchestratorToPlanet::Sunray(Sunray::default())).unwrap();
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PlanetToOrchestrator::SunrayAck { .. }) => {
                    info!("Sunray {} acknowledged", i + 2);
                }
                Ok(_) => panic!("Expected SunrayAck"),
                Err(e) => panic!("Expected SunrayAck, got error: {}", e),
            }
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: ASTEROID HANDLING
    // =========================================================================

    #[test]
    fn test_asteroid_handling_type_b() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();
        start_planet(&tx, &rx);

        // Test without charged cell
        info!("Sending asteroid without charged cell...");
        tx.send(OrchestratorToPlanet::Asteroid(Asteroid::default())).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::AsteroidAck { planet_id, rocket }) => {
                assert_eq!(planet_id, 1);
                assert!(rocket.is_none(), "No rocket without charged cell");
                info!("Asteroid without charge: no rocket (expected)");
            }
            Ok(_) => panic!("Expected AsteroidAck"),
            Err(e) => panic!("Expected AsteroidAck, got error: {}", e),
        }

        // Charge cell and test again
        info!("Charging cell and sending asteroid...");
        send_sunray(&tx, &rx);

        tx.send(OrchestratorToPlanet::Asteroid(Asteroid::default())).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::AsteroidAck { planet_id, rocket }) => {
                assert_eq!(planet_id, 1);
                // Type B CANNOT build rockets even with charged cell!
                assert!(rocket.is_none(), "Type B cannot build rockets");
                info!("Asteroid with charge (Type B): no rocket (Type B can't build rockets)");
            }
            Ok(_) => panic!("Expected AsteroidAck"),
            Err(e) => panic!("Expected AsteroidAck, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_asteroid_handling_type_a() {
        init_logger();

        // Create Type A planet which CAN build rockets
        let (planet_channels, rx_explorer, (tx, rx), _tx_explorer) = setup_test_channels();

        let ai = AI {
            started: false,
            stopped: false,
        };

        let mut planet = Planet::new(
            99,
            PlanetType::A,
            Box::new(ai),
            vec![BasicResourceType::Oxygen],
            vec![],
            planet_channels,
            rx_explorer,
        )
            .expect("Failed to create Type A planet");

        let handle = thread::spawn(move || {
            let _ = planet.run();
        });

        start_planet(&tx, &rx);

        // Charge cell
        info!("Charging Type A planet cell...");
        send_sunray(&tx, &rx);

        // Send asteroid - Type A should survive
        info!("Sending asteroid to Type A planet...");
        tx.send(OrchestratorToPlanet::Asteroid(Asteroid::default())).unwrap();

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::AsteroidAck { planet_id, rocket }) => {
                assert_eq!(planet_id, 99);
                assert!(rocket.is_some(), "Type A should build rocket and survive");
                info!("Type A planet survived asteroid with rocket!");
            }
            Ok(_) => panic!("Expected AsteroidAck"),
            Err(e) => panic!("Expected AsteroidAck, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: EXPLORER REGISTRATION
    // =========================================================================

    #[test]
    fn test_explorer_registration_lifecycle() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();
        start_planet(&tx, &rx);

        // Register multiple explorers
        info!("Registering explorers 100, 200, 300...");
        let _expl_rx_1 = register_explorer(100, &tx, &rx);
        let _expl_rx_2 = register_explorer(200, &tx, &rx);
        let _expl_rx_3 = register_explorer(300, &tx, &rx);
        info!("All explorers registered");

        // Unregister one
        info!("Unregistering explorer 200...");
        unregister_explorer(200, &tx, &rx);
        info!("Explorer 200 unregistered");

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: EXPLORER QUERIES
    // =========================================================================

    #[test]
    fn test_explorer_supported_queries() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();
        start_planet(&tx, &rx);

        let expl_rx = register_explorer(42, &tx, &rx);

        // Test SupportedResourceRequest
        info!("Querying supported resources...");
        tx_expl
            .send(ExplorerToPlanet::SupportedResourceRequest { explorer_id: 42 })
            .unwrap();

        match expl_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::SupportedResourceResponse { resource_list }) => {
                assert!(resource_list.contains(&BasicResourceType::Oxygen));
                assert!(resource_list.contains(&BasicResourceType::Hydrogen));
                assert!(resource_list.contains(&BasicResourceType::Carbon));
                assert!(resource_list.contains(&BasicResourceType::Silicon));
                assert_eq!(resource_list.len(), 4);
                info!("Supported resources: {:?}", resource_list);
            }
            Ok(_) => panic!("Expected SupportedResourceResponse"),
            Err(e) => panic!("Expected SupportedResourceResponse, got error: {}", e),
        }

        // Test SupportedCombinationRequest
        info!("Querying supported combinations...");
        tx_expl
            .send(ExplorerToPlanet::SupportedCombinationRequest { explorer_id: 42 })
            .unwrap();

        match expl_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::SupportedCombinationResponse { combination_list }) => {
                assert!(combination_list.contains(&ComplexResourceType::Robot));
                assert_eq!(combination_list.len(), 1);
                info!("Supported combinations: {:?}", combination_list);
            }
            Ok(_) => panic!("Expected SupportedCombinationResponse"),
            Err(e) => panic!("Expected SupportedCombinationResponse, got error: {}", e),
        }

        // Test AvailableEnergyCellRequest
        info!("Querying available energy cells...");
        tx_expl
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: 42 })
            .unwrap();

        match expl_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::AvailableEnergyCellResponse { available_cells }) => {
                assert_eq!(available_cells, 1);
                info!("Available energy cells: {}", available_cells);
            }
            Ok(_) => panic!("Expected AvailableEnergyCellResponse"),
            Err(e) => panic!("Expected AvailableEnergyCellResponse, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: RESOURCE GENERATION (ALL TYPES COMPACTED)
    // =========================================================================

    #[test]
    fn test_explorer_generate_all_basic_resources() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();
        start_planet(&tx, &rx);

        let expl_rx = register_explorer(42, &tx, &rx);

        let resources_to_test = [
            (BasicResourceType::Oxygen, "Oxygen"),
            (BasicResourceType::Hydrogen, "Hydrogen"),
            (BasicResourceType::Carbon, "Carbon"),
            (BasicResourceType::Silicon, "Silicon"),
        ];

        for (resource_type, name) in resources_to_test {
            info!("Testing generation of {}...", name);

            // Charge cell
            send_sunray(&tx, &rx);

            // Request resource
            tx_expl
                .send(ExplorerToPlanet::GenerateResourceRequest {
                    explorer_id: 42,
                    resource: resource_type,
                })
                .unwrap();

            match expl_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(PlanetToExplorer::GenerateResourceResponse { resource }) => {
                    assert!(resource.is_some(), "{} should be generated", name);

                    // Verify correct type
                    let res = resource.unwrap();
                    let matches = match (&res, resource_type) {
                        (BasicResource::Oxygen(_), BasicResourceType::Oxygen) => true,
                        (BasicResource::Hydrogen(_), BasicResourceType::Hydrogen) => true,
                        (BasicResource::Carbon(_), BasicResourceType::Carbon) => true,
                        (BasicResource::Silicon(_), BasicResourceType::Silicon) => true,
                        _ => false,
                    };
                    assert!(matches, "Generated resource type should match requested type");
                    info!("{} generated successfully", name);
                }
                Ok(_) => panic!("Expected GenerateResourceResponse for {}", name),
                Err(e) => panic!("Expected GenerateResourceResponse for {}, got error: {}", name, e),
            }
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_explorer_generate_resource_no_charge() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();
        start_planet(&tx, &rx);

        let expl_rx = register_explorer(42, &tx, &rx);

        // Don't charge the cell!
        info!("Requesting resource without charged cell...");
        tx_expl
            .send(ExplorerToPlanet::GenerateResourceRequest {
                explorer_id: 42,
                resource: BasicResourceType::Oxygen,
            })
            .unwrap();

        // When no charged cell, the handler returns None (no response sent)
        let result = expl_rx.recv_timeout(Duration::from_millis(200));
        assert!(result.is_err(), "Should timeout when no charged cell");
        info!("No response received (expected - no charged cell)");

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: EXPLORER ISOLATION
    // =========================================================================

    #[test]
    fn test_explorer_isolation() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();
        start_planet(&tx, &rx);

        // Register two explorers
        info!("Registering explorers 100 and 200...");
        let expl_rx_100 = register_explorer(100, &tx, &rx);
        let expl_rx_200 = register_explorer(200, &tx, &rx);

        // Explorer 100 sends request
        info!("Explorer 100 sending request...");
        tx_expl
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: 100 })
            .unwrap();

        // Only explorer 100 should receive response
        match expl_rx_100.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::AvailableEnergyCellResponse { .. }) => {
                info!("Explorer 100 received its response");
            }
            Ok(_) => panic!("Explorer 100 expected AvailableEnergyCellResponse"),
            Err(e) => panic!("Explorer 100 expected response, got error: {}", e),
        }

        // Explorer 200 should NOT receive anything
        let result = expl_rx_200.recv_timeout(Duration::from_millis(100));
        assert!(result.is_err(), "Explorer 200 should not receive Explorer 100's response");
        info!("Explorer 200 correctly received nothing");

        // Unregister explorer 100 and verify isolation
        info!("Unregistering explorer 100...");
        unregister_explorer(100, &tx, &rx);

        // Send request from unregistered explorer
        tx_expl
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: 100 })
            .unwrap();

        let result = expl_rx_100.recv_timeout(Duration::from_millis(200));
        assert!(result.is_err(), "Unregistered explorer should get no response");
        info!("Unregistered explorer correctly received nothing");

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_explorer_receives_stopped_when_planet_stopped() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();
        start_planet(&tx, &rx);

        let expl_rx = register_explorer(42, &tx, &rx);

        info!("Stopping planet...");
        stop_planet(&tx, &rx);

        // Send request to stopped planet
        info!("Explorer sending request to stopped planet...");
        tx_expl
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: 42 })
            .unwrap();

        match expl_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::Stopped) => {
                info!("Explorer received Stopped response (expected)");
            }
            Ok(_) => panic!("Expected Stopped"),
            Err(e) => panic!("Expected Stopped, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: EDGE CASES (COMPACTED)
    // =========================================================================

    #[test]
    fn test_messages_when_stopped() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();
        // Don't start the planet

        // Test Sunray when stopped
        info!("Sending Sunray to stopped planet...");
        tx.send(OrchestratorToPlanet::Sunray(Sunray::default())).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::Stopped { planet_id }) => {
                assert_eq!(planet_id, 1);
                info!("Sunray to stopped planet: got Stopped (expected)");
            }
            Ok(_) => panic!("Expected Stopped for Sunray"),
            Err(e) => panic!("Expected Stopped for Sunray, got error: {}", e),
        }

        // Test Asteroid when stopped
        info!("Sending Asteroid to stopped planet...");
        tx.send(OrchestratorToPlanet::Asteroid(Asteroid::default())).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::Stopped { planet_id }) => {
                assert_eq!(planet_id, 1);
                info!("Asteroid to stopped planet: got Stopped (expected)");
            }
            Ok(_) => panic!("Expected Stopped for Asteroid"),
            Err(e) => panic!("Expected Stopped for Asteroid, got error: {}", e),
        }

        // Test Stop when already stopped
        info!("Sending Stop to already stopped planet...");
        tx.send(OrchestratorToPlanet::StopPlanetAI).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::Stopped { planet_id }) => {
                assert_eq!(planet_id, 1);
                info!("Stop to stopped planet: got Stopped (expected)");
            }
            Ok(_) => panic!("Expected Stopped for Stop"),
            Err(e) => panic!("Expected Stopped for Stop, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    #[test]
    fn test_duplicate_start_ignored() {
        init_logger();

        let (handle, (tx, rx), _) = spawn_planet();

        start_planet(&tx, &rx);
        info!("Planet started");

        // Send another start - should be ignored
        info!("Sending duplicate Start...");
        tx.send(OrchestratorToPlanet::StartPlanetAI).unwrap();
        thread::sleep(Duration::from_millis(50));

        // Planet should still be running normally
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::InternalStateResponse { .. }) => {
                info!("Planet still running after duplicate Start (expected)");
            }
            Ok(_) => panic!("Expected InternalStateResponse"),
            Err(e) => panic!("Expected InternalStateResponse, got error: {}", e),
        }

        kill_planet(&tx, &rx);
        handle.join().expect("Planet thread panicked");
    }

    // =========================================================================
    // INTEGRATION TESTS: FULL SCENARIO
    // =========================================================================

    #[test]
    fn test_full_lifecycle_scenario() {
        init_logger();

        let (handle, (tx, rx), tx_expl) = spawn_planet();

        info!("=== FULL LIFECYCLE TEST ===");

        // 1. Start planet
        info!("Step 1: Starting planet...");
        start_planet(&tx, &rx);

        // 2. Register explorer
        info!("Step 2: Registering explorer 42...");
        let expl_rx = register_explorer(42, &tx, &rx);

        // 3. Check supported resources
        info!("Step 3: Querying supported resources...");
        tx_expl
            .send(ExplorerToPlanet::SupportedResourceRequest { explorer_id: 42 })
            .unwrap();
        let _ = expl_rx.recv_timeout(Duration::from_millis(200));

        // 4. Receive sunray
        info!("Step 4: Receiving sunray...");
        send_sunray(&tx, &rx);

        // 5. Generate a resource
        info!("Step 5: Generating Silicon...");
        tx_expl
            .send(ExplorerToPlanet::GenerateResourceRequest {
                explorer_id: 42,
                resource: BasicResourceType::Silicon,
            })
            .unwrap();

        match expl_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::GenerateResourceResponse { resource }) => {
                assert!(resource.is_some());
                info!("Silicon generated successfully");
            }
            Ok(_) => panic!("Expected GenerateResourceResponse"),
            Err(e) => panic!("Expected resource, got error: {}", e),
        }

        // 6. Check internal state
        info!("Step 6: Checking internal state...");
        tx.send(OrchestratorToPlanet::InternalStateRequest).unwrap();
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::InternalStateResponse { planet_state, .. }) => {
                assert_eq!(planet_state.charged_cells_count, 0, "Cell should be discharged");
                info!("Internal state: {} charged cells", planet_state.charged_cells_count);
            }
            Ok(_) => panic!("Expected InternalStateResponse"),
            Err(e) => panic!("Expected state, got error: {}", e),
        }

        // 7. Unregister explorer
        info!("Step 7: Unregistering explorer 42...");
        unregister_explorer(42, &tx, &rx);

        // 8. Stop and kill
        info!("Step 8: Stopping planet...");
        stop_planet(&tx, &rx);

        info!("Step 9: Killing planet...");
        kill_planet(&tx, &rx);

        handle.join().expect("Planet thread panicked");
        info!("=== FULL LIFECYCLE TEST COMPLETE ===");
    }

    #[test]
    fn test_orchestrator_disconnect_handling() {
        init_logger();

        let (handle, (tx, _rx), _) = spawn_planet();

        info!("Simulating orchestrator disconnect...");
        drop(tx);

        let result = handle.join();
        assert!(result.is_ok(), "Planet should handle disconnect gracefully");
        info!("Planet handled disconnect gracefully");
    }
}