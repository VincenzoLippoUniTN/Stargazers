use std::collections::HashSet;
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};

// Protocols
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};

// Resource Types
use common_game::components::resource::{BasicResource, BasicResourceType, ComplexResource, ComplexResourceRequest, ComplexResourceType, GenericResource
};pub struct FirstExplorer {
    pub name: String,
}

impl FirstExplorer {
    pub fn new(name: String) -> Self {
        FirstExplorer { name }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 1: E ->> P (SupportedResource)
    // =========================================================================
    /// E ->> P: SupportedResourceRequest(explorer_id)
    /// P ->> E: SupportedResourceResponse(resource_list)
    fn ask_planet_for_resources(
        &self,
        tx_to_planet: &Sender<ExplorerToPlanet>,
        rx_from_planet: &Receiver<PlanetToExplorer>,
        explorer_id: u32,
    ) -> HashSet<BasicResourceType> {
        // 1. E --> Planet
        let req = ExplorerToPlanet::SupportedResourceRequest { explorer_id };
        if let Err(e) = tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending resource req to Planet AI: {}", self.name, e);
            return HashSet::new();
        }

        // 2. waiting
        match rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::SupportedResourceResponse { resource_list }) => {
                println!("[FirstExplorer {}] ✅ Resources received from Planet AI: {:?}", self.name, resource_list);
                resource_list
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                HashSet::new()
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Resources)! Returning empty set.", self.name);
                HashSet::new()
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 2: E ->> P (SupportedCombination)
    // =========================================================================
    /// E ->> P: SupportedCombinationRequest(explorer_id)
    /// P ->> E: SupportedCombinationResponse(combination_list)
    fn ask_planet_for_combinations(
        &self,
        tx_to_planet: &Sender<ExplorerToPlanet>,
        rx_from_planet: &Receiver<PlanetToExplorer>,
        explorer_id: u32,
    ) -> HashSet<ComplexResourceType> {
        // 1. E --> P combination
        let req = ExplorerToPlanet::SupportedCombinationRequest { explorer_id };
        if let Err(e) = tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending combination req to Planet AI: {}", self.name, e);
            return HashSet::new();
        }

        match rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::SupportedCombinationResponse { combination_list }) => {
                println!("[FirstExplorer {}] ✅ Combinations received from Planet AI: {:?}", self.name, combination_list);
                combination_list
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                HashSet::new()
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Combinations)! Returning empty set.", self.name);
                HashSet::new()
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 3: E ->> P (GenerateResource)
    // =========================================================================
    fn generate_resource_from_planet(
        &self,
        tx_to_planet: &Sender<ExplorerToPlanet>,
        rx_from_planet: &Receiver<PlanetToExplorer>,
        explorer_id: u32,
        resource: BasicResourceType,
    ) -> Option<BasicResource> {
        let req = ExplorerToPlanet::GenerateResourceRequest {
            explorer_id,
            resource
        };

        if let Err(e) = tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending GenerateResource req to Planet AI: {}", self.name, e);
            return None;
        }

        match rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::GenerateResourceResponse { resource: res }) => {
                res
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                None
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Generation)! Returning None.", self.name);
                None
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 4: E ->> P (CombineResource)
    // =========================================================================
    /// E ->> P: CombineResourceRequest(req, explorer_id)
    /// alt Complex Resource is generated
    ///     P ->> E: CombineResourceResponse(Ok(ComplexResource))
    /// else Complex Resource is not generated
    ///     P ->> E: CombineResourceResponse(Err((String, Resource1, Resource2)))
    /// end
    pub fn ask_planet_to_combine_resource(
        &self,
        tx_to_planet: &Sender<ExplorerToPlanet>,
        rx_from_planet: &Receiver<PlanetToExplorer>,
        explorer_id: u32,
        msg: ComplexResourceRequest,
    ) -> Result<ComplexResource, (String, BasicResource, BasicResource)> {

        let request = ExplorerToPlanet::CombineResourceRequest { explorer_id, msg };
        let _ = tx_to_planet.send(request);

        match rx_from_planet.recv_timeout(Duration::from_secs(1)) {
            Ok(PlanetToExplorer::CombineResourceResponse { complex_response }) => {
                match complex_response {
                    Ok(complex) => Ok(complex),
                    Err((err_msg, gen1, gen2)) => {
                        let basic1 = match gen1 {
                            GenericResource::BasicResources(b) => b,
                            _ => panic!("Expected BasicResource for gen1"),
                        };
                        let basic2 = match gen2 {
                            GenericResource::BasicResources(b) => b,
                            _ => panic!("Expected BasicResource for gen2"),
                        };
                        Err((err_msg, basic1, basic2))
                    }
                }
            }
            _ => panic!("Error to receive the message"),
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM: E ->> P (AvailableEnergyCell)
    // =========================================================================
    /// E ->> P: AvailableEnergyCellRequest(explorer_id)
    /// P ->> E: AvailableEnergyCellResponse(available_cells)
    pub fn ask_planet_for_available_energy_cells(
        &self,
        tx_to_planet: &Sender<ExplorerToPlanet>,
        rx_from_planet: &Receiver<PlanetToExplorer>,
        explorer_id: u32,
    ) -> usize {
        // 1. E --> Planet
        let req = ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id };
        if let Err(e) = tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending energy cell req to Planet AI: {}", self.name, e);
            return 0;
        }

        // 2. waiting
        match rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::AvailableEnergyCellResponse { available_cells }) => {
                println!("[FirstExplorer {}] ✅ Energy cells received from Planet AI: {}", self.name, available_cells);
                available_cells as usize
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                0
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Energy Cells)! Returning 0.", self.name);
                0
            }
        }
    }



    // =========================================================================
    // MAIN RUN LOOP
    // =========================================================================
    pub fn run(
        &self,
        rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
        tx_to_orchestrator: Sender<ExplorerToOrchestrator<()>>,
        tx_to_planet: Sender<ExplorerToPlanet>,
        rx_from_planet: Receiver<PlanetToExplorer>,
        explorer_id: u32,
    ) {
        println!("[FirstExplorer {}] Active and waiting...", self.name);

        while let Ok(msg) = rx_from_orchestrator.recv() {
            match msg {
                // Flow 1
                OrchestratorToExplorer::SupportedResourceRequest => {
                    println!("[FirstExplorer {}] Resource request from Orchestrator. Calling planet function...", self.name);

                    // Call sequence diagram 1
                    let resources = self.ask_planet_for_resources(&tx_to_planet, &rx_from_planet, explorer_id);

                    // Replay Orchestrator
                    let response = ExplorerToOrchestrator::SupportedResourceResult {
                        explorer_id,
                        supported_resources: resources,
                    };
                    let _ = tx_to_orchestrator.send(response);
                }

                // Flow 2
                OrchestratorToExplorer::SupportedCombinationRequest => {
                    println!("[FirstExplorer {}] 🔍 Combinations request from Orchestrator. Calling planet function...", self.name);

                    // Call directly Diagram 2
                    let combinations = self.ask_planet_for_combinations(&tx_to_planet, &rx_from_planet, explorer_id);

                    // Replay to Orchestrator
                    let orchestrator_res = ExplorerToOrchestrator::SupportedCombinationResult {
                        explorer_id,
                        combination_list: combinations,
                    };
                    let _ = tx_to_orchestrator.send(orchestrator_res);
                }

                // Flow 3
                OrchestratorToExplorer::GenerateResourceRequest { to_generate } => {
                    println!("[FirstExplorer {}] 🛠️ Orchestrator asked to generate a resource. Calling planet...", self.name);

                    let generation_result = self.generate_resource_from_planet(
                        &tx_to_planet,
                        &rx_from_planet,
                        explorer_id,
                        to_generate
                    );

                    let result_for_orchestrator = match generation_result {
                        Some(_res) => {
                            println!("[FirstExplorer {}] ✅ Resource generated successfully!", self.name);
                            Ok(())
                        }
                        None => {
                            println!("[FirstExplorer {}] ❌ Planet failed to generate resource.", self.name);
                            Err("Resource generation failed or timed out".to_string())
                        }
                    };

                    let response = ExplorerToOrchestrator::GenerateResourceResponse {
                        explorer_id,
                        generated: result_for_orchestrator,
                    };

                    let _ = tx_to_orchestrator.send(response);
                }

                // Flow 4
                OrchestratorToExplorer::CombineResourceRequest { to_generate } => {
                    println!("[FirstExplorer {}] 🔄 Orchestrator asked to combine a resource. Calling planet...", self.name);

                    let mock_hydrogen: common_game::components::resource::Hydrogen = unsafe { std::mem::zeroed() };
                    let mock_oxygen: common_game::components::resource::Oxygen = unsafe { std::mem::zeroed() };
                    let fake_complex_req = ComplexResourceRequest::Water(mock_hydrogen, mock_oxygen);

                    let combine_result = self.ask_planet_to_combine_resource(
                        &tx_to_planet,
                        &rx_from_planet,
                        explorer_id,
                        fake_complex_req
                    );

                    let result_for_orchestrator = match combine_result {
                        Ok(_) => Ok(()),
                        Err((err_msg, _, _)) => Err(err_msg),
                    };

                    let response = ExplorerToOrchestrator::GenerateResourceResponse {
                        explorer_id,
                        generated: result_for_orchestrator,
                    };

                    let _ = tx_to_orchestrator.send(response);
                }

                _ => {}
            }
        }

        println!("[FirstExplorer {}] Thread terminated.", self.name);
    }
}

// =========================================================================
// UNIT TESTS
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use common_game::components::resource::{BasicResourceType, ComplexResourceType};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_orchestrator_explorer_planet_resource_flow() {
        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<()>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = FirstExplorer::new("PipelineBot".to_string());
        let explorer_id = 42;

        thread::spawn(move || {
            explorer.run(rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id);
        });

        tx_orch_to_exp.send(OrchestratorToExplorer::SupportedResourceRequest).unwrap();

        match rx_planet_from_exp.recv_timeout(Duration::from_secs(1)) {
            Ok(ExplorerToPlanet::SupportedResourceRequest { explorer_id: id }) => {
                let mut mock_planet_resources = HashSet::new();
                mock_planet_resources.insert(BasicResourceType::Oxygen);

                tx_planet_to_exp.send(PlanetToExplorer::SupportedResourceResponse {
                    resource_list: mock_planet_resources
                }).unwrap();
            }
            _ => panic!("Planet AI didn't receive the correct SupportedResourceRequest!"),
        }

        match rx_orch_from_exp.recv_timeout(Duration::from_secs(1)) {
            Ok(ExplorerToOrchestrator::SupportedResourceResult { explorer_id: res_id, supported_resources }) => {
                assert!(supported_resources.contains(&BasicResourceType::Oxygen));
            }
            _ => panic!("Orchestrator TIMEOUT for resources!"),
        }
    }

    #[test]
    fn test_orchestrator_explorer_planet_combination_flow() {
        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<()>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = FirstExplorer::new("ComboBot".to_string());
        let explorer_id = 99;

        thread::spawn(move || {
            explorer.run(rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id);
        });

        println!("\n[Test Combinations] Orchestrator sends SupportedCombinationRequest...");
        tx_orch_to_exp.send(OrchestratorToExplorer::SupportedCombinationRequest).unwrap();

        match rx_planet_from_exp.recv_timeout(Duration::from_secs(1)) {
            Ok(ExplorerToPlanet::SupportedCombinationRequest { explorer_id: id }) => {
                assert_eq!(id, explorer_id);

                let mut mock_combinations = HashSet::new();
                mock_combinations.insert(ComplexResourceType::Water);

                tx_planet_to_exp.send(PlanetToExplorer::SupportedCombinationResponse {
                    combination_list: mock_combinations
                }).unwrap();
            }
            _ => panic!("Planet AI didn't receive the correct SupportedCombinationRequest!"),
        }

        match rx_orch_from_exp.recv_timeout(Duration::from_secs(1)) {
            Ok(ExplorerToOrchestrator::SupportedCombinationResult { explorer_id: res_id, combination_list }) => {
                assert_eq!(res_id, explorer_id);
                assert!(combination_list.contains(&ComplexResourceType::Water));
                println!("🎉 SUCCESS! Combinations successfully arrived at the Orchestrator: {:?}", combination_list);
            }
            _ => panic!("Orchestrator TIMEOUT for combinations!"),
        }
    }

    #[test]
    fn test_orchestrator_explorer_planet_generate_flow() {
        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<()>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = FirstExplorer::new("GenBot".to_string());
        let explorer_id = 15;

        thread::spawn(move || {
            explorer.run(rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id);
        });

        println!("\n[Test Generate] Orchestrator sends GenerateResourceRequest...");

        tx_orch_to_exp.send(OrchestratorToExplorer::GenerateResourceRequest {
            to_generate: BasicResourceType::Oxygen
        }).unwrap();

        match rx_planet_from_exp.recv_timeout(Duration::from_secs(1)) {
            // Usa 'resource' perché il protocollo ExplorerToPlanet usa quello
            Ok(ExplorerToPlanet::GenerateResourceRequest { explorer_id: id, resource }) => {
                assert_eq!(id, explorer_id);
                println!("[Test - Mock Planet AI] Received generate req for {:?} from {}", resource, id);

                tx_planet_to_exp.send(PlanetToExplorer::GenerateResourceResponse {
                    resource: None
                }).unwrap();
            }
            _ => panic!("Planet AI didn't receive the correct GenerateResourceRequest!"),
        }

        match rx_orch_from_exp.recv_timeout(Duration::from_secs(1)) {
            // Usa 'generated' perché il protocollo ExplorerToOrchestrator usa quello
            Ok(ExplorerToOrchestrator::GenerateResourceResponse { explorer_id: res_id, generated }) => {
                assert_eq!(res_id, explorer_id);

                assert!(generated.is_err());
                println!("🎉 SUCCESS! Orchestrator correctly received the failure Result: {:?}", generated);
            }
            _ => panic!("Orchestrator TIMEOUT for generation!"),
        }
    }

    #[test]
    fn test_ask_planet_to_combine_resource_success() {
        let (tx_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = FirstExplorer::new("CombinerSuccessBot".to_string());
        let explorer_id = 123;

        // Inizializziamo i componenti finti
        let mock_hydrogen: common_game::components::resource::Hydrogen = unsafe { std::mem::zeroed() };
        let mock_oxygen: common_game::components::resource::Oxygen = unsafe { std::mem::zeroed() };
        let mock_water_struct: common_game::components::resource::Water = unsafe { std::mem::zeroed() };

        let mock_request = ComplexResourceRequest::Water(mock_hydrogen, mock_oxygen);
        let mock_complex_response = ComplexResource::Water(mock_water_struct);

        // Thread del finto Pianeta
        let planet_handle = thread::spawn(move || {
            println!("\n🌍 [MOCK PLANET] In attesa di richieste di combinazione...");
            match rx_planet_from_exp.recv_timeout(Duration::from_secs(1)) {
                Ok(ExplorerToPlanet::CombineResourceRequest { explorer_id: id, msg: _msg }) => {
                    println!("🌍 [MOCK PLANET] Richiesta ricevuta dall'explorer #{}!", id);
                    assert_eq!(id, 123);

                    println!("🌍 [MOCK PLANET] Invio risposta di successo (Ok(ComplexResource::Water))...");
                    tx_planet_to_exp.send(PlanetToExplorer::CombineResourceResponse {
                        complex_response: Ok(mock_complex_response),
                    }).unwrap();
                }
                _ => panic!("Il pianeta AI non ha ricevuto la richiesta corretta!"),
            }
        });

        println!("\n🚀 [EXPLORER TEST] Chiamata a ask_planet_to_combine_resource...");
        let result = explorer.ask_planet_to_combine_resource(
            &tx_to_planet,
            &rx_exp_from_planet,
            explorer_id,
            mock_request,
        );

        // Verifichiamo il risultato stampando l'esito
        println!("🚀 [EXPLORER TEST] Risultato ricevuto della funzione: {:?}", result.is_ok());
        assert!(result.is_ok());

        println!("🎉 [EXPLORER TEST] Test completato con successo!");
        planet_handle.join().unwrap();
    }

    #[test]
    fn test_ask_planet_for_available_energy_cells() {
        let (tx_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = FirstExplorer::new("EnergyCellBot".to_string());
        let explorer_id = 42;

        // Thread del finto Pianeta
        let planet_handle = thread::spawn(move || {
            println!("\n🌍 [MOCK PLANET] In attesa di richieste per energy cells...");
            match rx_planet_from_exp.recv_timeout(Duration::from_secs(1)) {
                Ok(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: id }) => {
                    println!("🌍 [MOCK PLANET] Richiesta ricevuta dall'explorer #{}!", id);
                    assert_eq!(id, 42);

                    println!("🌍 [MOCK PLANET] Invio risposta di successo (10 energy cells)...");
                    tx_planet_to_exp.send(PlanetToExplorer::AvailableEnergyCellResponse {
                        available_cells: 10,
                    }).unwrap();
                }
                _ => panic!("Il pianeta AI non ha ricevuto la richiesta corretta!"),
            }
        });

        println!("\n🚀 [EXPLORER TEST] Chiamata a ask_planet_for_available_energy_cells...");
        let result = explorer.ask_planet_for_available_energy_cells(
            &tx_to_planet,
            &rx_exp_from_planet,
            explorer_id,
        );

        // Verifichiamo il risultato stampando l'esito
        println!("🚀 [EXPLORER TEST] Risultato ricevuto della funzione: {}", result);
        assert_eq!(result, 10);

        println!("🎉 [EXPLORER TEST] Test completato con successo!");
        planet_handle.join().unwrap();
    }
}