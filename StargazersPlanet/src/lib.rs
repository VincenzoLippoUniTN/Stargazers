use std::sync::mpsc;
use common_game::components::planet::{Planet, PlanetAI, PlanetState, PlanetType};
use common_game::components::resource::{BasicResourceType, ComplexResourceType, Combinator, Generator, BasicResource, ComplexResourceRequest, ComplexResource};
use common_game::components::rocket::Rocket;
use common_game::protocols::messages;

// Group-defined AI struct
struct AI {
    started: bool,
    stopped: bool,
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
                state.charge_cell(s); // Carica la cella
                Some(messages::PlanetToOrchestrator::SunrayAck { planet_id: state.id() })
            }

            messages::OrchestratorToPlanet::InternalStateRequest => {
                let dummy_state = state.to_dummy();
                Some(messages::PlanetToOrchestrator::InternalStateResponse {
                    planet_id: state.id(),
                    planet_state: dummy_state,
                })
            }

            messages::OrchestratorToPlanet::IncomingExplorerRequest { explorer_id: _explorer_id, new_mpsc_sender: _new_mpsc_sender } => {
                Some(messages::PlanetToOrchestrator::IncomingExplorerResponse {
                    planet_id: state.id(),
                    res: Ok(()),
                })
            }

            messages::OrchestratorToPlanet::OutgoingExplorerRequest { explorer_id: _explorer_id } => {
                Some(messages::PlanetToOrchestrator::OutgoingExplorerResponse {
                    planet_id: state.id(),
                    res: Ok(()),
                })
            }

            _ => None,

            /*
            messages::OrchestratorToPlanet::Asteroid(_) => {
                if let Some(_) = self.handle_asteroid(state, generator, combinator) {
                    Some(messages::PlanetToOrchestrator::AsteroidAck { planet_id: state.id(), destroyed: false })
                } else {
                    Some(messages::PlanetToOrchestrator::AsteroidAck { planet_id: state.id(), destroyed: true })
                }
            },

            messages::OrchestratorToPlanet::StartPlanetAI => {
                self.start(state);
                Some(messages::PlanetToOrchestrator::StartPlanetAIResult { planet_id: state.id() })
            },

            messages::OrchestratorToPlanet::StopPlanetAI => {
                self.stop(state);
                Some(messages::PlanetToOrchestrator::StopPlanetAIResult { planet_id: state.id() })
            }
             */
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
            messages::ExplorerToPlanet::SupportedResourceRequest { explorer_id: _ } => {
                Some(messages::PlanetToExplorer::SupportedResourceResponse { resource_list: generator.all_available_recipes() })
            },

            // This variant is used to ask the Planet for the available [ComplexResourceType]
            messages::ExplorerToPlanet::SupportedCombinationRequest { explorer_id: _ } => {
                Some(messages::PlanetToExplorer::SupportedCombinationResponse { combination_list: combinator.all_available_recipes() })
            },

            // This variant is used to ask the Planet to generate a [BasicResource].
            // Three expected outcomes:
            //  - None => the planet is not able to generate said resource, no charge available
            //  - Some(GenerateResourceResponse { resource: None }) => the planet could generate the resource, but generation failed
            //  - Some(GenerateResourceResponse { resource: Some(requested_resource) }) => the planet successfully generated the resource
            // Explorer is expected to handle said outcomes.
            messages::ExplorerToPlanet::GenerateResourceRequest { explorer_id: _ , resource } => {
                // Checking with planet state if charged cell is available
                if let Some((charged_cell, _)) = state.full_cell() {
                    // Matching generation with requested type
                    match resource {
                        BasicResourceType::Oxygen => {
                            match generator.make_oxygen(charged_cell) {
                                Ok(oxygen) => {
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Oxygen(oxygen)) })
                                }
                                Err(msg) => {
                                    // TODO: Log unsuccessful resource creation & show msg
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Carbon => {
                            match generator.make_carbon(charged_cell) {
                                Ok(carbon) => {
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Carbon(carbon)) })
                                }
                                Err(msg) => {
                                    // TODO: Log unsuccessful resource creation & show msg
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Hydrogen => {
                            match generator.make_hydrogen(charged_cell) {
                                Ok(hydrogen) => {
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Hydrogen(hydrogen)) })
                                }
                                Err(msg) => {
                                    // TODO: Log unsuccessful resource creation & show msg
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                        BasicResourceType::Silicon => {
                            match generator.make_silicon(charged_cell) {
                                Ok(silicon) => {
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: Some(BasicResource::Silicon(silicon)) })
                                }
                                Err(msg) => {
                                    // TODO: Log unsuccessful resource creation & show msg
                                    Some(messages::PlanetToExplorer::GenerateResourceResponse { resource: None })
                                }
                            }
                        },
                    }
                } else {
                    // TODO: Log failed request as no charges available
                    None
                }
            },

            // This variant is used to ask the Planet to generate a [ComplexResource] using the [ComplexResourceRequest]
            // [ComplexResourceRequest] = [ComplexResource]([BasicResource], [BasicResource])
            // Two expected outcomes:
            //  - Some(CombineResourceResponse { complex_response: Err((err_msg, r1, r2)) }) => the planet couldn't create the resource
            //  - Some(CombineResourceResponse { complex_response: Ok(requested_resource) }) => the planet successfully create the resource
            // Explorer is expected to handle said outcomes.
            messages::ExplorerToPlanet::CombineResourceRequest { explorer_id: _ , msg } => {
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
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::CombineResourceResponse {
                                        complex_response: Ok(ComplexResource::Robot(robot))
                                    })
                                },
                                Err((msg, r1, r2)) => {
                                    // TODO: Log failed resource creation
                                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1.to_generic(), r2.to_generic())) })
                                }
                            }
                        }
                        _ => {
                            // TODO: Log failed request as recipe not exists
                            let msg = match r_type {
                                ComplexResourceType::Diamond => "Recipe not available, failed combination - returning [r1 : Carbon] and [r2 : Carbon]".to_string(),
                                ComplexResourceType::Water => "Recipe not available, failed combination - returning [r1 : Hydrogen] and [r2 : Oxygen]".to_string(),
                                ComplexResourceType::Life => "Recipe not available, failed combination - returning [r1 : Water] and [r2 : Carbon]".to_string(),
                                ComplexResourceType::Dolphin => "Recipe not available, failed combination - returning [r1 : Water] and [r2 : Life]".to_string(),
                                ComplexResourceType::AIPartner => "Recipe not available, failed combination - returning [r1 : Robot] and [r2 : Diamond]".to_string(),
                                _ => "Pretty strange behaviour... Shouldn't be possible to be here, but here we are...".to_string()
                            };
                            Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1, r2)) })
                        }
                    }
                } else {
                    // TODO: Log failed request as no charges available
                    let msg = match r_type {
                        ComplexResourceType::Diamond => "No charged cell, failed combination - returning [r1 : Carbon] and [r2 : Carbon]".to_string(),
                        ComplexResourceType::Water => "No charged cell, failed combination - returning [r1 : Hydrogen] and [r2 : Oxygen]".to_string(),
                        ComplexResourceType::Life => "No charged cell, failed combination - returning [r1 : Water] and [r2 : Carbon]".to_string(),
                        ComplexResourceType::Robot => "No charged cell, failed combination - returning [r1 : Silicon] and [r2 : Life]".to_string(),
                        ComplexResourceType::Dolphin => "No charged cell, failed combination - returning [r1 : Water] and [r2 : Life]".to_string(),
                        ComplexResourceType::AIPartner => "No charged cell, failed combination - returning [r1 : Robot] and [r2 : Diamond]".to_string()
                    };
                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Err((msg, r1, r2)) })
                }
            },

            // This variant is used to ask the Planet for the available energy_cells number
            messages::ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: _ } => {
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
                println!("Planet {}: NO charged cells, asteroid will destroy the planet!", state.id());
                None
            }

            Some((_cell, idx)) => {
                match state.build_rocket(idx) {
                    Ok(_) => {
                        println!("Planet {}: Rocket successfully built!", state.id());
                        state.take_rocket()
                    }
                    Err(e) => {
                        println!(
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
        println!("Planet {}: AI started!", state.id());
        self.started = true;
        self.stopped = false;
    }

    fn stop(&mut self, state: &PlanetState) {
        println!("Planet {}: AI stopped!", state.id());
        self.stopped = true;
        self.started = false;
    }
}

// This is the group's "export" function. It will be called by
// the orchestrator to spawn your planet.
pub fn create_planet(
    rx_orchestrator: mpsc::Receiver<messages::OrchestratorToPlanet>,
    tx_orchestrator: mpsc::Sender<messages::PlanetToOrchestrator>,
    rx_explorer: mpsc::Receiver<messages::ExplorerToPlanet>
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
            println!("Planet {} created!", planet.id());
            planet
        }
        Err(msg) => {
            // TODO: Log planet creation failure
            panic!("Planet {} created with error: {}", id, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use common_game::components::asteroid::Asteroid;
    use common_game::components::sunray::Sunray;
    use common_game::protocols::messages::{
        ExplorerToPlanet, OrchestratorToPlanet, PlanetToExplorer, PlanetToOrchestrator,
    };

    // --- Helper for creating dummy channels ---
    // Returns the halves required by Planet::new
    type PlanetOrchHalfChannels = (
        mpsc::Receiver<OrchestratorToPlanet>,
        mpsc::Sender<PlanetToOrchestrator>,
    );

    type PlanetExplHalfChannels = (
        mpsc::Receiver<ExplorerToPlanet>,
        mpsc::Sender<PlanetToExplorer>,
    );

    type OrchPlanetHalfChannels = (
        mpsc::Sender<OrchestratorToPlanet>,
        mpsc::Receiver<PlanetToOrchestrator>,
    );

    type ExplPlanetHalfChannels = (
        mpsc::Sender<ExplorerToPlanet>,
        mpsc::Receiver<PlanetToExplorer>,
    );

    fn get_test_channels() -> (
        PlanetOrchHalfChannels,
        PlanetExplHalfChannels,
        OrchPlanetHalfChannels,
        ExplPlanetHalfChannels,
    ) {
        // Channel 1: Orchestrator -> Planet
        let (tx_orch_in, rx_orch_in) = mpsc::channel::<OrchestratorToPlanet>();
        // Channel 2: Planet -> Orchestrator
        let (tx_orch_out, rx_orch_out) = mpsc::channel::<PlanetToOrchestrator>();

        // Channel 3: Explorer -> Planet
        let (tx_expl_in, rx_expl_in) = mpsc::channel::<ExplorerToPlanet>();
        // Channel 4: Planet -> Explorer
        let (tx_expl_out, rx_expl_out) = mpsc::channel::<PlanetToExplorer>();

        (
            (rx_orch_in, tx_orch_out),
            (rx_expl_in, tx_expl_out),
            (tx_orch_in, rx_orch_out),
            (tx_expl_in, rx_expl_out),
        )
    }

    // --- Integration Tests: Constructor ---

    #[test]
    fn test_explorer_comms() {
        // 1. Setup Channels using the new helper
        let (
            (rec_OrcToPla, sen_PlaToOrc),
            (rec_ExpToPla, sen_PlaToExp),
            (sen_OrcToPla, rec_PlaToOrc),
            (sen_ExpToPla, rec_PlaToExp)
        ) = get_test_channels();


        let mut planet = create_planet(rec_OrcToPla, sen_PlaToOrc, rec_ExpToPla);

        // Spawn planet thread
        let handle = thread::spawn(move || {
            let _ = planet.run();
        });

        // 3. Start Planet
        sen_OrcToPla.send(OrchestratorToPlanet::StartPlanetAI).unwrap();
        thread::sleep(Duration::from_millis(50));

        // 4. Setup Local Explorer Channels (Simulating Explorer 101)
        // We create a dedicated channel for this specific explorer interaction
        let explorer_id = 101;
        let (expl_tx_local, expl_rx_local) = mpsc::channel::<PlanetToExplorer>();

        // 5. Send IncomingExplorerRequest (Orchestrator -> Planet)
        sen_OrcToPla
            .send(OrchestratorToPlanet::IncomingExplorerRequest {
                explorer_id,
                new_mpsc_sender: expl_tx_local,
            })
            .unwrap();

        // 6. Verify Ack from Planet
        match rec_PlaToOrc.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::IncomingExplorerResponse { planet_id, res }) => {
                assert_eq!(planet_id, 1);
                assert!(res.is_ok());
            }
            _ => panic!("Expected IncomingExplorerResponse"),
        }

        // 7. Test Interaction (Explorer -> Planet -> Explorer)
        // Explorer sends a request using the GLOBAL channel, but includes its ID
        sen_ExpToPla
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id })
            .unwrap();

        // Verify Explorer receives response on the LOCAL channel
        match expl_rx_local.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToExplorer::AvailableEnergyCellResponse { available_cells }) => {
                println!("Available cells: {:?}", available_cells);
            }
            _ => panic!("Expected AvailableEnergyCellResponse"),
        }

        // 8. Send OutgoingExplorerRequest (Orchestrator -> Planet)
        sen_OrcToPla
            .send(OrchestratorToPlanet::OutgoingExplorerRequest { explorer_id })
            .unwrap();

        // 9. Verify Ack from Planet
        match rec_PlaToOrc.recv_timeout(Duration::from_millis(200)) {
            Ok(PlanetToOrchestrator::OutgoingExplorerResponse { planet_id, res }) => {
                assert_eq!(planet_id, 1);
                assert!(res.is_ok());
            }
            _ => panic!("Expected OutgoingExplorerResponse"),
        }

        // 10. Verify Isolation
        // Explorer sends another request
        sen_ExpToPla
            .send(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id })
            .unwrap();

        // We expect NO response on expl_rx_local
        let result = expl_rx_local.recv_timeout(Duration::from_millis(200));
        assert!(
            result.is_err(),
            "Planet responded to explorer after it left!"
        );

        // 11. Cleanup
        drop(sen_OrcToPla);
        let _ = handle.join();
    }

    #[test]
    fn test_1() {
        // --- Channel orchestrator ↔ planet ---
        let (tx_to_planet, rx_from_orch) = mpsc::channel::<OrchestratorToPlanet>();
        let (tx_from_planet, rx_to_orch) = mpsc::channel::<PlanetToOrchestrator>();

        // --- create the planet ---
        let mut planet = create_planet(rx_from_orch, tx_from_planet, mpsc::channel().1);

        // --- launch planet in a thread ---
        let handle = thread::spawn(move || {
            if let Err(e) = planet.run() {
                eprintln!("Planet thread error: {}", e);
            }
        });

        // --- Start AI ---
        tx_to_planet.send(OrchestratorToPlanet::StartPlanetAI).unwrap();
        if let Ok(PlanetToOrchestrator::StartPlanetAIResult { planet_id }) =
            rx_to_orch.recv_timeout(Duration::from_secs(2))
        {
            println!("Orchestrator: Planet {} started!", planet_id);
        } else {
            println!("Orchestrator: Start timeout");
        }

        // --- Send SUNRAY ---
        tx_to_planet.send(OrchestratorToPlanet::Sunray(Sunray::default())).unwrap();
        if let Ok(PlanetToOrchestrator::SunrayAck { planet_id }) =
            rx_to_orch.recv_timeout(Duration::from_secs(2))
        {
            println!("Orchestrator: SunrayAck received from Planet {}", planet_id);
        } else {
            println!("Orchestrator: SunrayAck timeout");
        }

        // --- Send ASTEROID ---
        println!("\nOrchestrator: Sending Asteroid...");
        tx_to_planet.send(OrchestratorToPlanet::Asteroid(Asteroid::default())).unwrap();

        match rx_to_orch.recv_timeout(Duration::from_secs(2)) {
            Ok(PlanetToOrchestrator::AsteroidAck { planet_id, destroyed}) => {
                println!("Orchestrator: AsteroidAck received from Planet {}", planet_id);
                if destroyed {
                    println!("Orchestrator: Planet {} destroyed the asteroid.", planet_id);
                } else {
                    println!("Orchestrator: Planet {} did NOT destroyed the asteroid!", planet_id);
                }
            }
            Ok(_) => println!("Orchestrator: Unexpected message received"),
            Err(_) => println!("Orchestrator: AsteroidAck timeout"),
        }

        // --- Stop AI ---
        tx_to_planet.send(OrchestratorToPlanet::StopPlanetAI).unwrap();
        if let Ok(PlanetToOrchestrator::StopPlanetAIResult { planet_id }) =
            rx_to_orch.recv_timeout(Duration::from_secs(2))
        {
            println!("Orchestrator: Planet {} stopped!", planet_id);
        } else {
            println!("Orchestrator: Stop timeout");
        }

        // --- wait thread of the planet ---
        handle.join().unwrap();
    }
}
