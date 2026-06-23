use std::collections::HashSet;
use std::time::Duration;
use crossbeam_channel::{select_biased, Receiver, Sender};

// Protocols
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};

// Resource Types
use common_game::components::resource::{BasicResource, BasicResourceType, ComplexResource, ComplexResourceRequest, ComplexResourceType};
use common_game::components::resource::BasicResourceType::{Carbon, Hydrogen, Oxygen, Silicon};
use common_game::components::resource::ComplexResourceType::{Water, Diamond, Life, Robot, Dolphin, AIPartner};
use common_game::utils::ID;
use crate::bag::{Bag, BagSnapshot};

pub struct FirstExplorer {
    pub name: String,
    rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
    tx_to_orchestrator: Sender<ExplorerToOrchestrator<BagSnapshot>>,
    tx_to_planet: Sender<ExplorerToPlanet>,
    rx_from_planet: Receiver<PlanetToExplorer>,
    explorer_id: u32,
    current_planet_id: u32,
    bag: Bag,
}

impl FirstExplorer {
    pub fn new(
        name: String,
        rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
        tx_to_orchestrator: Sender<ExplorerToOrchestrator<BagSnapshot>>,
        tx_to_planet: Sender<ExplorerToPlanet>,
        rx_from_planet: Receiver<PlanetToExplorer>,
        explorer_id: u32,
        current_planet_id: u32,
    ) -> Self {
        FirstExplorer {
            name,
            rx_from_orchestrator,
            tx_to_orchestrator,
            tx_to_planet,
            rx_from_planet,
            explorer_id,
            current_planet_id,
            bag: Bag::new(),
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 1: E ->> P (SupportedResource)
    // =========================================================================
    /// E ->> P: SupportedResourceRequest(explorer_id)
    /// P ->> E: SupportedResourceResponse(resource_list)
    fn ask_planet_for_resources(
        &self,
    ) -> HashSet<BasicResourceType> {
        // 1. E --> Planet
        let req = ExplorerToPlanet::SupportedResourceRequest { explorer_id: self.explorer_id };
        if let Err(e) = self.tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending resource req to Planet AI: {}", self.name, e);
            return HashSet::new();
        }

        // 2. waiting
        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
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
    ) -> HashSet<ComplexResourceType> {
        // 1. E --> P combination
        let req = ExplorerToPlanet::SupportedCombinationRequest { explorer_id: self.explorer_id };
        if let Err(e) = self.tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending combination req to Planet AI: {}", self.name, e);
            return HashSet::new();
        }

        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
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
        &mut self,
        resource: BasicResourceType,
    ) -> Result<(), String> {
        let req = ExplorerToPlanet::GenerateResourceRequest {
            explorer_id: self.explorer_id,
            resource
        };

        if let Err(e) = self.tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending GenerateResource req to Planet AI: {}", self.name, e);
            return Err(e.to_string());
        }

        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::GenerateResourceResponse { resource: opt_res }) => {
                match opt_res {
                    Some(resource) => {
                        println!("[FirstExplorer {}] ✅ Resource generated successfully!", self.name);

                        // Adding basic resource to bag
                        self.bag.add_basic(resource);

                        Ok(())
                    }
                    None => {
                        println!("[FirstExplorer {}] ❌ Planet failed to generate resource.", self.name);
                        Err("Resource generation failed or timed out".to_string())
                    }
                }
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                Err(format!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other))
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Generation)! Returning None.", self.name);
                Err(format!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Generation)! Returning None.", self.name))
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
        &mut self,
        resource_type: ComplexResourceType,
    ) -> Result<(), String> {
        let msg : ComplexResourceRequest = match resource_type {
            Diamond => {
                let carbon_1 = match self.bag.take_basic(Carbon) {
                    Some(resource) => resource.to_carbon()?,
                    None => { return Err("Insufficient carbon for diamond generation.".to_string()); }
                };
                let carbon_2 = match self.bag.take_basic(Carbon) {
                    Some(resource) => resource.to_carbon()?,
                    None => { self.bag.add_basic(BasicResource::Carbon(carbon_1)); return Err("Insufficient carbon for diamond generation.".to_string()); }
                };
                ComplexResourceRequest::Diamond(carbon_1, carbon_2)
            }
            Water => {
                let hydrogen = match self.bag.take_basic(Hydrogen) {
                    Some(resource) => resource.to_hydrogen()?,
                    None => { return Err("Insufficient hydrogen for water generation.".to_string()); }
                };
                let oxygen = match self.bag.take_basic(Oxygen) {
                    Some(resource) => resource.to_oxygen()?,
                    None => { self.bag.add_basic(BasicResource::Hydrogen(hydrogen)); return Err("Insufficient oxygen for water generation.".to_string()); }
                };
                ComplexResourceRequest::Water(hydrogen, oxygen)
            }
            Life => {
                let water = match self.bag.take_complex(Water) {
                    Some(resource) => resource.to_water()?,
                    None => { return Err("Insufficient water for life generation.".to_string()); }
                };
                let carbon = match self.bag.take_basic(Carbon) {
                    Some(resource) => resource.to_carbon()?,
                    None => { self.bag.add_complex(ComplexResource::Water(water)); return Err("Insufficient carbon for life generation.".to_string()); }
                };
                ComplexResourceRequest::Life(water, carbon)
            }
            Robot => {
                let silicon = match self.bag.take_basic(Silicon) {
                    Some(resource) => resource.to_silicon()?,
                    None => { return Err("Insufficient silicon for robot generation.".to_string()); }
                };
                let life = match self.bag.take_complex(Life) {
                    Some(resource) => resource.to_life()?,
                    None => { self.bag.add_basic(BasicResource::Silicon(silicon)); return Err("Insufficient life for robot generation.".to_string()); }
                };
                ComplexResourceRequest::Robot(silicon, life)
            }
            Dolphin => {
                let water = match self.bag.take_complex(Water) {
                    Some(resource) => resource.to_water()?,
                    None => { return Err("Insufficient water for dolphin generation.".to_string()); }
                };
                let life = match self.bag.take_complex(Life) {
                    Some(resource) => resource.to_life()?,
                    None => { self.bag.add_complex(ComplexResource::Water(water)); return Err("Insufficient life for dolphin generation.".to_string()); }
                };
                ComplexResourceRequest::Dolphin(water, life)
            }
            AIPartner => {
                let robot = match self.bag.take_complex(Robot) {
                    Some(resource) => resource.to_robot()?,
                    None => { return Err("Insufficient robot for AI-partner generation.".to_string()); }
                };
                let diamond = match self.bag.take_complex(Diamond) {
                    Some(resource) => resource.to_diamond()?,
                    None => { self.bag.add_complex(ComplexResource::Robot(robot)); return Err("Insufficient diamond for AI-partner generation.".to_string()); }
                };
                ComplexResourceRequest::AIPartner(robot, diamond)
            }
        };

        let request = ExplorerToPlanet::CombineResourceRequest { explorer_id: self.explorer_id, msg };
        let _ = self.tx_to_planet.send(request);

        match self.rx_from_planet.recv_timeout(Duration::from_secs(1)) {
            Ok(PlanetToExplorer::CombineResourceResponse { complex_response }) => {
                match complex_response {
                    Ok(complex) => {
                        self.bag.add_complex(complex);
                        Ok(())
                    },
                    Err((err_msg, gen1, gen2)) => {
                        self.bag.add_generic(gen1);
                        self.bag.add_generic(gen2);
                        Err(err_msg)
                    }
                }
            }
            Ok(other) => {
                eprintln!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other);
                Err(format!("[FirstExplorer {}] ⚠️ Unexpected msg from planet: {:?}", self.name, other))
            }
            Err(_) => {
                eprintln!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Generation)! Returning None.", self.name);
                Err(format!("[FirstExplorer {}] ⏱️ Timeout from Planet AI (Generation)! Returning None.", self.name))
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM: E ->> P (AvailableEnergyCell)
    // =========================================================================
    /// E ->> P: AvailableEnergyCellRequest(explorer_id)
    /// P ->> E: AvailableEnergyCellResponse(available_cells)
    pub fn ask_planet_for_available_energy_cells(
        &self,
    ) -> usize {
        // 1. E --> Planet
        let req = ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: self.explorer_id };
        if let Err(e) = self.tx_to_planet.send(req) {
            eprintln!("[FirstExplorer {}] Error sending energy cell req to Planet AI: {}", self.name, e);
            return 0;
        }

        // 2. waiting
        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
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

    // On START/STOP/KILL methods
    fn on_start (&self) { /* TODO */ }
    fn on_stop (&self) { /* TODO */ }
    fn on_kill (&self) { /* TODO */ }

    // Reset the Explorer on Orchestrator's request
    fn reset_routine (&self) { /* TODO */ }

    // Neighbors Discovery (NeighborsRequest) request method
    // TODO

    // TravelToPlanet request method
    // TODO
    /*
            OrchestratorToExplorer::MoveToPlanet { sender_to_new_planet, planet_id, } => {
                match sender_to_new_planet {
                    Some(channel) => { self.tx_to_planet = channel; }
                    None => { return Err("Failed to intercept Sender<ExplorerToPlanet> during space travel.".to_string()); }
                }
                self.current_planet_id = planet_id;
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::MovedToPlanetResult { explorer_id: self.explorer_id, planet_id: self.current_planet_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }
    */

    // Extracted helper to reduce the size of `run` and keep Clippy happy.
    // Returns `Ok(Some(true))` when the explorer should exit (killed),
    // `Ok(None)` to continue running, or `Err` on channel errors.
    fn handle_orchestrator_msg(
        &mut self,
        msg: OrchestratorToExplorer,
    ) -> Result<Option<bool>, String> {
        const ORCH_DISCONNECT_ERR: &str = "Orchestrator disconnected.";
        match msg {
            // Flow 1
            OrchestratorToExplorer::SupportedResourceRequest => {
                println!("[FirstExplorer {}] Resource request from Orchestrator. Calling planet function...", self.name);

                // Call sequence diagram 1
                let resources = self.ask_planet_for_resources();

                // Replay Orchestrator
                let response = ExplorerToOrchestrator::SupportedResourceResult {
                    explorer_id: self.explorer_id,
                    supported_resources: resources,
                };
                let _ = self.tx_to_orchestrator.send(response)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            // Flow 2
            OrchestratorToExplorer::SupportedCombinationRequest => {
                println!("[FirstExplorer {}] 🔍 Combinations request from Orchestrator. Calling planet function...", self.name);

                // Call directly Diagram 2
                let combinations = self.ask_planet_for_combinations();

                // Replay to Orchestrator
                let orchestrator_res = ExplorerToOrchestrator::SupportedCombinationResult {
                    explorer_id: self.explorer_id,
                    combination_list: combinations,
                };
                let _ = self.tx_to_orchestrator.send(orchestrator_res)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            // Flow 3
            OrchestratorToExplorer::GenerateResourceRequest { to_generate } => {
                println!("[FirstExplorer {}] 🛠️ Orchestrator asked to generate a resource. Calling planet...", self.name);

                let generation_result = self.generate_resource_from_planet(to_generate);

                let response = ExplorerToOrchestrator::GenerateResourceResponse {
                    explorer_id: self.explorer_id,
                    generated: generation_result,
                };

                let _ = self.tx_to_orchestrator.send(response)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            // Flow 4
            OrchestratorToExplorer::CombineResourceRequest { to_generate } => {
                println!("[FirstExplorer {}] 🔄 Orchestrator asked to combine a resource. Calling planet...", self.name);

                let combine_result = self.ask_planet_to_combine_resource(to_generate);

                let response = ExplorerToOrchestrator::GenerateResourceResponse {
                    explorer_id: self.explorer_id,
                    generated: combine_result,
                };

                let _ = self.tx_to_orchestrator.send(response)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::BagContentRequest => {
                self.tx_to_orchestrator
                    .send(ExplorerToOrchestrator::BagContentResponse { explorer_id: self.explorer_id, bag_content: self.bag.snapshot() })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::StartExplorerAI => {
                self.on_start();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::StartExplorerAIResult { explorer_id: self.explorer_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::ResetExplorerAI => {
                self.reset_routine();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::ResetExplorerAIResult { explorer_id: self.explorer_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::KillExplorer => {
                self.on_kill();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::StopExplorerAIResult { explorer_id: self.explorer_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(Some(true))
            }

            OrchestratorToExplorer::StopExplorerAI => {
                self.on_stop();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::StopExplorerAIResult { explorer_id: self.explorer_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;

                if self.wait_for_start()? { return Ok(Some(true)); }
                self.on_start();

                Ok(None)
            }

            OrchestratorToExplorer::CurrentPlanetRequest => {
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::CurrentPlanetResult { explorer_id: self.explorer_id, planet_id: self.current_planet_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::MoveToPlanet { sender_to_new_planet, planet_id, } => {
                match sender_to_new_planet {
                    Some(channel) => { self.tx_to_planet = channel; }
                    None => { return Err("Failed to intercept Sender<ExplorerToPlanet> during space travel.".to_string()); }
                }
                self.current_planet_id = planet_id;
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::MovedToPlanetResult { explorer_id: self.explorer_id, planet_id: self.current_planet_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            _ => Err("Unexpected message received.".to_string()),
        }
    }

    // =========================================================================
    // MAIN RUN LOOP
    // =========================================================================
    pub fn run(
        &mut self,
    ) {
        println!("[FirstExplorer {}] Active and waiting...", self.name);

        match self.wait_for_start() {
            Ok(true) => { return }
            Err(e) => { println!("Error: {}", e); return }
            _ => {}
        }

        loop {
            while let Ok(msg) = self.rx_from_orchestrator.try_recv() {
                if let Ok(Some(true)) = self.handle_orchestrator_msg(msg) {
                    println!("[FirstExplorer {}] Terminating by request.", self.name);
                    return; // Exit the loop entirely
                }
            }

            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn wait_for_start(&self) -> Result<bool, String> {
        const ORCH_DISCONNECT_ERR: &str = "Orchestrator disconnected.";
        while let Ok(msg) = self.rx_from_orchestrator.recv() {
            match msg {
                OrchestratorToExplorer::StartExplorerAI => {
                    self.tx_to_orchestrator
                        .send(ExplorerToOrchestrator::StartExplorerAIResult {
                            explorer_id: self.explorer_id,
                        })
                        .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;

                    return Ok(false)
                }

                OrchestratorToExplorer::KillExplorer => {
                    self.tx_to_orchestrator
                        .send(ExplorerToOrchestrator::KillExplorerResult { explorer_id: self.explorer_id })
                        .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;

                    return Ok(true)
                }

                OrchestratorToExplorer::ResetExplorerAI => {}

                _ => {}
            }
        }
        Err(ORCH_DISCONNECT_ERR.to_string())
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
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer_id = 42;
        let mut explorer = FirstExplorer::new("PipelineBot".to_string(), rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id, 0);

        thread::spawn(move || {
            explorer.run();
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
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer_id = 99;
        let mut explorer = FirstExplorer::new("ComboBot".to_string(), rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id, 0);

        thread::spawn(move || {
            explorer.run();
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
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer_id = 15;
        let mut explorer = FirstExplorer::new("GenBot".to_string(), rx_exp_from_orch, tx_exp_to_orch, tx_exp_to_planet, rx_exp_from_planet, explorer_id, 0);

        thread::spawn(move || {
            explorer.run();
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
/*
    #[test]
    fn test_ask_planet_to_combine_resource_success() {
        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<()>>();
        let (tx_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer_id = 123;
        let mut explorer = FirstExplorer::new("CombinerSuccessBot".to_string(), rx_exp_from_orch, tx_exp_to_orch, tx_to_planet, rx_exp_from_planet, explorer_id, 0);

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
        let result = explorer.ask_planet_to_combine_resource(mock_request);

        // Verifichiamo il risultato stampando l'esito
        println!("🚀 [EXPLORER TEST] Risultato ricevuto della funzione: {:?}", result.is_ok());
        assert!(result.is_ok());

        println!("🎉 [EXPLORER TEST] Test completato con successo!");
        planet_handle.join().unwrap();
    }
 */
    #[test]
    fn test_ask_planet_for_available_energy_cells() {
        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();
        let (tx_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer_id = 42;
        let explorer = FirstExplorer::new("EnergyCellBot".to_string(), rx_exp_from_orch, tx_exp_to_orch, tx_to_planet, rx_exp_from_planet, explorer_id, 0);

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
        let result = explorer.ask_planet_for_available_energy_cells();

        // Verifichiamo il risultato stampando l'esito
        println!("🚀 [EXPLORER TEST] Risultato ricevuto della funzione: {}", result);
        assert_eq!(result, 10);

        println!("🎉 [EXPLORER TEST] Test completato con successo!");
        planet_handle.join().unwrap();
    }
}