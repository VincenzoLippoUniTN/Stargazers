use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};

// Protocols
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};

// Resource Types
use common_game::components::resource::{BasicResource, BasicResourceType, ComplexResource, ComplexResourceRequest, ComplexResourceType};
use common_game::components::resource::BasicResourceType::{Carbon, Hydrogen, Oxygen, Silicon};
use common_game::components::resource::ComplexResourceType::{Water, Diamond, Life, Robot, Dolphin, AIPartner};
use common_game::utils::ID;

// Structured logging (logging.rs). Adjust this path if the module isn't `common_game::logging`.
use common_game::logging::{ActorType, Channel, EventType, LogEvent, Participant, Payload};

use super::bag::{Bag, BagSnapshot};

type SharedExplorer = Arc<(Mutex<Explorer>, Condvar)>;
pub type ExplorerBehaviour = fn(AI);

pub struct Explorer {
    pub name: String,
    rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
    tx_to_orchestrator: Sender<ExplorerToOrchestrator<BagSnapshot>>,
    tx_to_planet: Sender<ExplorerToPlanet>,
    rx_from_planet: Receiver<PlanetToExplorer>,
    explorer_id: ID,
    current_planet_id: ID,
    bag: Bag,

    current_generation_rules: HashSet<BasicResourceType>,
    current_combination_cookbook: HashSet<ComplexResourceType>,
    current_neighbors: Vec<ID>,
    awaiting_move: bool,
    awaiting_neighbors: bool,

    /// The autonomous behaviour that drives an explorer once it has started.
    ///
    /// It receives the `AI` handle (a cheap, lock-internally Arc wrapper) and owns
    /// the main loop. `FnOnce` because it runs exactly once on its own thread and
    /// consumes the handle; `Send + 'static` so it can cross the `thread::spawn`
    /// boundary. Inject a different one per explorer to get different behaviours.
    ///
    /// Injected at construction; `take()`n out in `run()` and handed to the AI
    /// thread. `Option` so we can move it out without disturbing the rest of
    /// `self` (which then gets moved whole into the shared `Mutex`).
    behaviour: Option<Box<dyn FnOnce(AI) + Send + 'static>>,
}

impl Explorer {
    pub fn new(
        name: String,
        rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
        tx_to_orchestrator: Sender<ExplorerToOrchestrator<BagSnapshot>>,
        tx_to_planet: Sender<ExplorerToPlanet>,
        rx_from_planet: Receiver<PlanetToExplorer>,
        explorer_id: ID,
        current_planet_id: ID,
        behaviour: impl FnOnce(AI) + Send + 'static,
    ) -> Self {
        Explorer {
            name,
            rx_from_orchestrator,
            tx_to_orchestrator,
            tx_to_planet,
            rx_from_planet,
            explorer_id,
            current_planet_id,
            bag: Bag::new(),
            current_generation_rules: HashSet::new(),
            current_combination_cookbook: HashSet::new(),
            current_neighbors: Vec::new(),
            awaiting_move: false,
            awaiting_neighbors: false,
            behaviour: Some(Box::new(behaviour)),
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 1: E ->> P (SupportedResource)
    // =========================================================================
    /// E ->> P: SupportedResourceRequest(explorer_id)
    /// P ->> E: SupportedResourceResponse(resource_list)
    fn ask_planet_for_resources( &mut self ) -> Result<(), String> {
        // 1. E --> Planet
        let request = ExplorerToPlanet::SupportedResourceRequest { explorer_id: self.explorer_id };
        let _ = self.tx_to_planet.send(request)
            .map_err(|_| "Orchestrator disconnected.".to_string())?;

        // 2. waiting
        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::SupportedResourceResponse { resource_list }) => {
                self.current_generation_rules = resource_list;
                self.log_from_planet(
                    Channel::Debug,
                    kv([
                        ("detail", "supported resources received".to_string()),
                        ("resources", format!("{:?}", self.current_generation_rules)),
                    ]),
                );
                Ok(())
            }
            Ok(other) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([
                        ("detail", "unexpected message from planet".to_string()),
                        ("message", format!("{other:?}")),
                    ]),
                );
                Err("Unexpected msg from planet.".to_string())
            }
            Err(_) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([("detail", "timeout waiting for supported resources".to_string())]),
                );
                Err("Timeout from Planet AI.".to_string())
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM 2: E ->> P (SupportedCombination)
    // =========================================================================
    /// E ->> P: SupportedCombinationRequest(explorer_id)
    /// P ->> E: SupportedCombinationResponse(combination_list)
    fn ask_planet_for_combinations( &mut self ) -> Result<(), String> {
        // 1. E --> P combination
        let request = ExplorerToPlanet::SupportedCombinationRequest { explorer_id: self.explorer_id };
        let _ = self.tx_to_planet.send(request)
            .map_err(|_| "Orchestrator disconnected.".to_string())?;

        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::SupportedCombinationResponse { combination_list }) => {
                self.current_combination_cookbook = combination_list;
                self.log_from_planet(
                    Channel::Debug,
                    kv([
                        ("detail", "supported combinations received".to_string()),
                        ("combinations", format!("{:?}", self.current_combination_cookbook)),
                    ]),
                );
                Ok(())
            }
            Ok(other) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([
                        ("detail", "unexpected message from planet".to_string()),
                        ("message", format!("{other:?}")),
                    ]),
                );
                Err("Unexpected msg from planet.".to_string())
            }
            Err(_) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([("detail", "timeout waiting for supported combinations".to_string())]),
                );
                Err("Timeout from Planet AI.".to_string())
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
            self.log_to_planet(
                Channel::Error,
                kv([
                    ("detail", "failed to send GenerateResource request".to_string()),
                    ("error", e.to_string()),
                ]),
            );
            return Err(e.to_string());
        }

        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::GenerateResourceResponse { resource: opt_res }) => {
                match opt_res {
                    Some(resource) => {
                        self.log_from_planet(
                            Channel::Debug,
                            kv([
                                ("detail", "resource generated".to_string()),
                                ("resource", format!("{resource:?}")),
                            ]),
                        );

                        // Adding basic resource to bag
                        self.bag.add_basic(resource);

                        Ok(())
                    }
                    None => {
                        self.log_from_planet(
                            Channel::Warning,
                            kv([("detail", "planet failed to generate resource".to_string())]),
                        );
                        Err("Resource generation failed or timed out".to_string())
                    }
                }
            }
            Ok(other) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([
                        ("detail", "unexpected message from planet".to_string()),
                        ("message", format!("{other:?}")),
                    ]),
                );
                Err("Unexpected msg from planet.".to_string())
            }
            Err(_) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([("detail", "timeout waiting for generate response".to_string())]),
                );
                Err("Timeout from Planet AI.".to_string())
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
    fn ask_planet_to_combine_resource(
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
                self.log_from_planet(
                    Channel::Warning,
                    kv([
                        ("detail", "unexpected message from planet".to_string()),
                        ("message", format!("{other:?}")),
                    ]),
                );
                Err("Unexpected msg from planet.".to_string())
            }
            Err(_) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([("detail", "timeout waiting for combine response".to_string())]),
                );
                Err("Timeout from Planet AI.".to_string())
            }
        }
    }

    // =========================================================================
    // SEQUENCE DIAGRAM: E ->> P (AvailableEnergyCell)
    // =========================================================================
    /// E ->> P: AvailableEnergyCellRequest(explorer_id)
    /// P ->> E: AvailableEnergyCellResponse(available_cells)
    fn ask_planet_for_available_energy_cells(
        &self,
    ) -> usize {
        // 1. E --> Planet
        let req = ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id: self.explorer_id };
        if let Err(e) = self.tx_to_planet.send(req) {
            self.log_to_planet(
                Channel::Error,
                kv([
                    ("detail", "failed to send energy-cell request".to_string()),
                    ("error", e.to_string()),
                ]),
            );
            return 0;
        }

        // 2. waiting
        match self.rx_from_planet.recv_timeout(Duration::from_millis(500)) {
            Ok(PlanetToExplorer::AvailableEnergyCellResponse { available_cells }) => {
                self.log_from_planet(
                    Channel::Debug,
                    kv([
                        ("detail", "energy cells received".to_string()),
                        ("available_cells", available_cells.to_string()),
                    ]),
                );
                available_cells as usize
            }
            Ok(other) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([
                        ("detail", "unexpected message from planet".to_string()),
                        ("message", format!("{other:?}")),
                    ]),
                );
                0
            }
            Err(_) => {
                self.log_from_planet(
                    Channel::Warning,
                    kv([("detail", "timeout waiting for energy cells".to_string())]),
                );
                0
            }
        }
    }

    // Reset the Explorer on Orchestrator's request
    fn reset_routine(&mut self) {
        self.log_internal(
            Channel::Debug,
            kv([("detail", "reset requested; wiping bag and telemetry".to_string())]),
        );

        self.bag = Bag::new();
        self.current_generation_rules = HashSet::new();
        self.current_combination_cookbook = HashSet::new();
        self.current_neighbors = Vec::new();

        // TODO: Clear out any other simulation baselines if needed
        // (For example, if you track traveled distance, energy spent, or a score, you would zero them out here).

        self.log_internal(
            Channel::Debug,
            kv([("detail", "reset complete".to_string())]),
        );
    }

    // Neighbors Discovery (NeighborsRequest) request method
    fn ask_orchestrator_for_neighbors(&mut self) -> Result<(), String> {
        let request = ExplorerToOrchestrator::NeighborsRequest {
            explorer_id: self.explorer_id,
            current_planet_id: self.current_planet_id,
        };
        self.tx_to_orchestrator.send(request)
            .map_err(|_| "Orchestrator disconnected.".to_string())?;
        self.awaiting_neighbors = true;
        Ok(())
    }

    // TravelToPlanet request method
    fn initiate_travel_to_planet(&mut self, planet_id: u32) -> Result<(), String> {
        self.log_to_orchestrator(
            Channel::Debug,
            kv([
                ("detail", "requesting travel to planet".to_string()),
                ("dst_planet_id", planet_id.to_string()),
            ]),
        );
        let request = ExplorerToOrchestrator::TravelToPlanetRequest {
            explorer_id: self.explorer_id,
            current_planet_id: self.current_planet_id,
            dst_planet_id: planet_id,
        };
        self.tx_to_orchestrator.send(request)
            .map_err(|_| "Orchestrator disconnected.".to_string())?;
        self.awaiting_move = true;
        Ok(())
    }

    // Fire NeighborsRequest, block (lock released) until the listener applies the reply.
    fn request_neighbors_and_wait(slot: &SharedExplorer) -> Result<(), String> {
        let (lock, cvar) = &**slot;
        let mut guard = lock.lock().unwrap();
        guard.ask_orchestrator_for_neighbors()?;
        let _guard  = cvar.wait_while(guard, |e| e.awaiting_neighbors).unwrap();
        Ok(())
    }

    // Fire TravelToPlanet, block until MoveToPlanet is applied.
    // On Ok, tx_to_planet + current_planet_id already point at the new planet.
    fn travel_and_wait(slot: &SharedExplorer, dst_planet_id: u32) -> Result<(), String> {
        let (lock, cvar) = &**slot;
        let mut guard = lock.lock().unwrap();
        guard.initiate_travel_to_planet(dst_planet_id)?;
        let guard = cvar.wait_while(guard, |e| e.awaiting_move).unwrap();
        if guard.current_planet_id == dst_planet_id {
            Ok(())
        } else {
            Err(format!("Travel to planet {} failed.", dst_planet_id))
        }
    }

    // Returns `Ok(Some(true))` when the explorer  should exit (killed),
    // `Ok(None)` to continue running, or `Err` on channel errors.
    fn handle_orchestrator_msg(
        &mut self,
        msg: OrchestratorToExplorer,
    ) -> Result<Option<bool>, String> {
        const ORCH_DISCONNECT_ERR: &str = "Orchestrator disconnected.";
        match msg {
            // Flow 1
            OrchestratorToExplorer::SupportedResourceRequest => {
                self.log_from_orchestrator(
                    Channel::Debug,
                    kv([("detail", "supported-resource request; querying planet".to_string())]),
                );

                // Call sequence diagram 1
                if let Err(e) = self.ask_planet_for_resources() {
                    return Err(e);
                }

                // Replay Orchestrator
                let response = ExplorerToOrchestrator::SupportedResourceResult {
                    explorer_id: self.explorer_id,
                    supported_resources: self.current_generation_rules.clone(),
                };
                let _ = self.tx_to_orchestrator.send(response)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            // Flow 2
            OrchestratorToExplorer::SupportedCombinationRequest => {
                self.log_from_orchestrator(
                    Channel::Debug,
                    kv([("detail", "supported-combination request; querying planet".to_string())]),
                );

                // Call directly Diagram 2
                if let Err(e) = self.ask_planet_for_combinations() {
                    return Err(e);
                }

                // Replay to Orchestrator
                let orchestrator_res = ExplorerToOrchestrator::SupportedCombinationResult {
                    explorer_id: self.explorer_id,
                    combination_list: self.current_combination_cookbook.clone(),
                };
                let _ = self.tx_to_orchestrator.send(orchestrator_res)
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            // Flow 3
            OrchestratorToExplorer::GenerateResourceRequest { to_generate } => {
                self.log_from_orchestrator(
                    Channel::Debug,
                    kv([("detail", "generate-resource request; querying planet".to_string())]),
                );

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
                self.log_from_orchestrator(
                    Channel::Debug,
                    kv([("detail", "combine-resource request; querying planet".to_string())]),
                );

                let combine_result = self.ask_planet_to_combine_resource(to_generate);

                let response = ExplorerToOrchestrator::CombineResourceResponse {
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

            OrchestratorToExplorer::ResetExplorerAI => {
                self.reset_routine();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::ResetExplorerAIResult { explorer_id: self.explorer_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::KillExplorer => {
                self.on_kill();
                self.tx_to_orchestrator.send(ExplorerToOrchestrator::KillExplorerResult { explorer_id: self.explorer_id })
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
                    None => { self.awaiting_move = false; return Err("Failed to intercept Sender<ExplorerToPlanet> during space travel.".to_string()); }
                }
                self.current_planet_id = planet_id;
                self.current_generation_rules = HashSet::new();
                self.current_combination_cookbook = HashSet::new();
                self.current_neighbors = Vec::new();
                self.awaiting_move = false;

                self.tx_to_orchestrator.send(ExplorerToOrchestrator::MovedToPlanetResult { explorer_id: self.explorer_id, planet_id: self.current_planet_id })
                    .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                Ok(None)
            }

            OrchestratorToExplorer::NeighborsResponse { neighbors } => {
                self.log_from_orchestrator(
                    Channel::Debug,
                    kv([
                        ("detail", "neighbors received".to_string()),
                        ("neighbors", format!("{neighbors:?}")),
                    ]),
                );
                self.current_neighbors = neighbors;
                self.awaiting_neighbors = false;
                Ok(None)
            }

            _ => Err("Unexpected message received.".to_string()),

            /*  Messages that aren't handled because unexpected:
                OrchestratorToExplorer::StartExplorerAI => {
                    self.on_start();
                    self.tx_to_orchestrator.send(ExplorerToOrchestrator::StartExplorerAIResult { explorer_id: self.explorer_id })
                        .map_err(|_| ORCH_DISCONNECT_ERR.to_string())?;
                    Ok(None)
                }
            */
        }
    }

    // On START/STOP/KILL methods
    // (Not needed in the current state of the program, but it's good to have them wired for future changes)
    fn on_start (&self) { /* TODO */ }
    fn on_stop (&self) { /* TODO */ }
    fn on_kill (&self) { /* TODO */ }

    // =========================================================================
    // MAIN RUN LOOP
    // =========================================================================
    pub fn run(mut self) {
        self.log_internal(
            Channel::Debug,
            kv([("detail", "active and waiting for start".to_string())]),
        );

        match self.wait_for_start() {
            Ok(true) => { return }
            Err(e) => {
                self.log_internal(
                    Channel::Error,
                    kv([
                        ("detail", "error while waiting for start".to_string()),
                        ("error", e),
                    ]),
                );
                return
            }
            _ => {}
        }

        let rx_from_orchestrator = self.rx_from_orchestrator.clone();
        // Captured before `self` is moved into the Arc so the threads can still log.
        let explorer_id = self.explorer_id;
        let listener_name = self.name.clone();
        let ai_name = self.name.clone();
        // Pull the injected behaviour out (leaves `None` behind) before the move.
        let behaviour = self.behaviour.take();

        // Explorer + Condvar: the AI thread blocks on replies the LISTENER processes.
        let explorer: SharedExplorer = Arc::new((Mutex::new(self), Condvar::new()));
        let listener_explorer = Arc::clone(&explorer);
        let ai_explorer = Arc::clone(&explorer);

        // THREAD 1: LISTENER — the ONLY reader of rx_from_orchestrator.
        thread::spawn(move || {
            let (lock, cvar) = &*listener_explorer;
            while let Ok(msg) = rx_from_orchestrator.recv() {
                let mut guard = lock.lock().unwrap();
                let outcome = guard.handle_orchestrator_msg(msg);
                drop(guard);
                cvar.notify_all(); // wake the AI if it was awaiting neighbors/move

                match outcome {
                    Ok(Some(true)) => {
                        LogEvent::self_directed(
                            Participant::new(ActorType::Explorer, explorer_id),
                            EventType::InternalExplorerAction,
                            Channel::Debug,
                            kv([
                                ("explorer", listener_name.clone()),
                                ("detail", "listener: termination signal processed".to_string()),
                            ]),
                        )
                            .emit();
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        LogEvent::self_directed(
                            Participant::new(ActorType::Explorer, explorer_id),
                            EventType::InternalExplorerAction,
                            Channel::Error,
                            kv([
                                ("explorer", listener_name.clone()),
                                ("detail", "listener: handler error".to_string()),
                                ("error", e),
                            ]),
                        )
                            .emit();
                    }
                }
            }
        });

        // THREAD 2: AUTONOMOUS AI LOOP — never reads rx_from_orchestrator.
        // The behaviour was injected at construction; it owns its own loop and
        // drives the explorer purely through the `AI` handle's public API.
        thread::spawn(move || {
            let ai = AI::new(&ai_explorer);
            match behaviour {
                Some(run_ai) => run_ai(ai),
                None => {
                    // `run()` was called twice, or the explorer was built without
                    // a behaviour. Nothing to drive, so just idle out.
                    LogEvent::self_directed(
                        Participant::new(ActorType::Explorer, explorer_id),
                        EventType::InternalExplorerAction,
                        Channel::Warning,
                        kv([
                            ("explorer", ai_name.clone()),
                            ("detail", "no AI behaviour injected; AI thread idle".to_string()),
                        ]),
                    )
                        .emit();
                }
            }
        });
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

    // =========================================================================
    // LOGGING HELPERS
    // Translate the old terminal prints into structured `LogEvent`s emitted
    // through the `log` crate. Every helper auto-tags the payload with the
    // explorer name. Channel policy:
    //   - Debug   : normal status / received responses
    //   - Warning : recoverable anomalies (timeouts, unexpected msgs, planet refusals)
    //   - Error   : broken communication (disconnected channels / failed sends)
    // =========================================================================
    fn me(&self) -> Participant {
        Participant::new(ActorType::Explorer, self.explorer_id)
    }
    fn planet_participant(&self) -> Participant {
        Participant::new(ActorType::Planet, self.current_planet_id)
    }
    fn orchestrator_participant() -> Participant {
        Participant::new(ActorType::Orchestrator, 0u32)
    }

    fn emit_event(
        &self,
        sender: Option<Participant>,
        receiver: Option<Participant>,
        event_type: EventType,
        channel: Channel,
        mut payload: Payload,
    ) {
        payload
            .entry("explorer".to_string())
            .or_insert_with(|| self.name.clone());
        LogEvent::new(sender, receiver, event_type, channel, payload).emit();
    }

    /// Planet -> this explorer.
    fn log_from_planet(&self, channel: Channel, payload: Payload) {
        self.emit_event(
            Some(self.planet_participant()),
            Some(self.me()),
            EventType::MessagePlanetToExplorer,
            channel,
            payload,
        );
    }
    /// This explorer -> planet.
    fn log_to_planet(&self, channel: Channel, payload: Payload) {
        self.emit_event(
            Some(self.me()),
            Some(self.planet_participant()),
            EventType::MessageExplorerToPlanet,
            channel,
            payload,
        );
    }
    /// Orchestrator -> this explorer.
    fn log_from_orchestrator(&self, channel: Channel, payload: Payload) {
        self.emit_event(
            Some(Self::orchestrator_participant()),
            Some(self.me()),
            EventType::MessageOrchestratorToExplorer,
            channel,
            payload,
        );
    }
    /// This explorer -> orchestrator.
    fn log_to_orchestrator(&self, channel: Channel, payload: Payload) {
        self.emit_event(
            Some(self.me()),
            Some(Self::orchestrator_participant()),
            EventType::MessageExplorerToOrchestrator,
            channel,
            payload,
        );
    }
    /// Internal explorer action (self-directed).
    fn log_internal(&self, channel: Channel, payload: Payload) {
        self.emit_event(
            Some(self.me()),
            Some(self.me()),
            EventType::InternalExplorerAction,
            channel,
            payload,
        );
    }
}

/// Build a `Payload` from a fixed set of `&str -> String` pairs.
fn kv<const N: usize>(entries: [(&str, String); N]) -> Payload {
    let mut map = Payload::new();
    for (k, v) in entries {
        map.insert(k.to_string(), v);
    }
    map
}

/// AI-side handle. Every method locks internally — the AI loop never touches
/// a guard. Cheap to make (just an Arc refcount bump).
///
/// `pub(crate)` so behaviour functions can be authored in their own module
/// (e.g. `crate::behaviours`) and still call this handle's API. The methods
/// below are the *entire* surface a behaviour needs — it never touches
/// `Explorer`'s internals directly.
pub(crate) struct AI {
    slot: SharedExplorer,
}

impl AI {
    pub(crate) fn new(slot: &SharedExplorer) -> Self {
        AI { slot: Arc::clone(slot) }
    }

    /// Emit a structured self-directed log from inside a behaviour. Locks
    /// briefly to read the explorer's identity, then unlocks before emitting.
    /// This is the behaviour-facing equivalent of `Explorer::log_internal`.
    pub(crate) fn log(&self, channel: Channel, detail: &str) {
        let (id, name) = {
            let (lock, _) = &*self.slot;
            let g = lock.lock().unwrap();
            (g.explorer_id, g.name.clone())
        };
        let mut payload = Payload::new();
        payload.insert("explorer".to_string(), name);
        payload.insert("detail".to_string(), detail.to_string());
        LogEvent::self_directed(
            Participant::new(ActorType::Explorer, id),
            EventType::InternalExplorerAction,
            channel,
            payload,
        )
            .emit();
    }

    // ---- orchestrator: fire + wait (delegates to existing helpers) ----
    pub(crate) fn request_neighbors(&self) -> Result<(), String> {
        Explorer::request_neighbors_and_wait(&self.slot)
    }
    pub(crate) fn travel(&self, dst: u32) -> Result<(), String> {
        Explorer::travel_and_wait(&self.slot, dst)
    }

    // ---- reads: lock, copy out, unlock ----
    pub(crate) fn neighbors(&self) -> Vec<ID> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().current_neighbors.clone()
    }
    pub(crate) fn current_planet(&self) -> u32 {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().current_planet_id
    }
    pub(crate) fn bag(&self) -> BagSnapshot {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().bag.snapshot()
    }

    // ---- planet round-trips: lock, &mut self method, unlock ----
    pub(crate) fn discover_resources(&self) -> Result<(), String> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().ask_planet_for_resources()
    }
    pub(crate) fn discover_combinations(&self) -> Result<(), String> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().ask_planet_for_combinations()
    }
    pub(crate) fn generate(&self, r: BasicResourceType) -> Result<(), String> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().generate_resource_from_planet(r)
    }
    pub(crate) fn combine(&self, c: ComplexResourceType) -> Result<(), String> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().ask_planet_to_combine_resource(c)
    }
    pub(crate) fn energy_cells(&self) -> usize {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().ask_planet_for_available_energy_cells()
    }
}

// =========================================================================
// UNIT TESTS — replaces the existing `#[cfg(test)] mod tests { ... }` block.
//
// Strategy: drive `handle_orchestrator_msg` / `wait_for_start` / the condvar
// helpers DIRECTLY. No `run()` except in the single integration test, so every
// test is deterministic (no reliance on the AI loop's timing or lock contention).
//
// Sections A–G need no resource internals. Section H mints REAL resources by
// borrowing a throwaway Planet's recipe-loaded Generator/Combinator.
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::sync::{Arc, Condvar, Mutex, Once};
    use std::thread;
    use std::time::Duration;

    // Extra imports for resource minting (section H).
    use common_game::components::planet::{DummyPlanetState, Planet, PlanetAI, PlanetState, PlanetType};
    use common_game::components::resource::{Combinator, Generator};
    use common_game::components::rocket::Rocket;
    use common_game::components::sunray::Sunray;
    use common_game::components::energy_cell::EnergyCell;
    use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};

    const T: Duration = Duration::from_secs(1);

    // Initialise `env_logger` exactly once for the whole test binary.
    // `is_test(true)` routes log output through libtest's capture, so it only
    // shows for failing tests — or for every test when you pass `--nocapture`.
    // Control verbosity with the RUST_LOG env var. To watch the explorer's
    // structured logs while testing, run:
    //     RUST_LOG=debug cargo test -- --nocapture
    static LOGGER_INIT: Once = Once::new();
    fn init_test_logger() {
        LOGGER_INIT.call_once(|| {
            // is_test(false): write to the real stderr fd, which libtest does NOT
            //   capture -> logs appear on a plain `cargo test` (no --nocapture).
            // default_filter_or("debug"): show Debug/Info/Warn/Error when RUST_LOG
            //   is unset; RUST_LOG still overrides (use "trace" to include Trace).
            let _ = env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or("debug"),
            )
                .is_test(false)
                .try_init();
        });
    }

    // Build the four channels + an explorer in one shot.
    // Returns (explorer, tx_orch_to_exp, rx_orch_from_exp, rx_planet_from_exp, tx_planet_to_exp).
    fn make(
        name: &str,
        explorer_id: ID,
        planet_id: ID,
    ) -> (
        Explorer,
        Sender<OrchestratorToExplorer>,
        Receiver<ExplorerToOrchestrator<BagSnapshot>>,
        Receiver<ExplorerToPlanet>,
        Sender<PlanetToExplorer>,
    ) {
        init_test_logger();

        let (tx_orch_to_exp, rx_exp_from_orch) = unbounded::<OrchestratorToExplorer>();
        let (tx_exp_to_orch, rx_orch_from_exp) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();
        let (tx_exp_to_planet, rx_planet_from_exp) = unbounded::<ExplorerToPlanet>();
        let (tx_planet_to_exp, rx_exp_from_planet) = unbounded::<PlanetToExplorer>();

        let explorer = Explorer::new(
            name.to_string(),
            rx_exp_from_orch,
            tx_exp_to_orch,
            tx_exp_to_planet,
            rx_exp_from_planet,
            explorer_id,
            planet_id,
            // Tests drive the explorer directly via its methods / the AI handle,
            // so they don't need a real behaviour. `run()` is exercised once in
            // the integration test, which is fine with this no-op.
            |_ai| {},
        );
        (explorer, tx_orch_to_exp, rx_orch_from_exp, rx_planet_from_exp, tx_planet_to_exp)
    }

    // =====================================================================
    // A. RELAY FLOWS (Orchestrator -> Explorer -> Planet -> Explorer -> Orchestrator)
    // =====================================================================

    #[test]
    fn test_flow1_supported_resources() {
        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("ResBot", 30, 0);

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::SupportedResourceRequest { explorer_id }) => {
                assert_eq!(explorer_id, 30);
                let mut set = HashSet::new();
                set.insert(Oxygen);
                tx_planet
                    .send(PlanetToExplorer::SupportedResourceResponse { resource_list: set })
                    .unwrap();
            }
            _ => panic!("planet did not receive SupportedResourceRequest"),
        });

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::SupportedResourceRequest);
        assert!(matches!(res, Ok(None)));
        assert!(explorer.current_generation_rules.contains(&Oxygen));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::SupportedResourceResult { explorer_id, supported_resources }) => {
                assert_eq!(explorer_id, 30);
                assert!(supported_resources.contains(&Oxygen));
            }
            _ => panic!("expected SupportedResourceResult"),
        }
        planet.join().unwrap();
    }

    #[test]
    fn test_flow2_supported_combinations() {
        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("ComboBot", 31, 0);

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::SupportedCombinationRequest { explorer_id }) => {
                assert_eq!(explorer_id, 31);
                let mut set = HashSet::new();
                set.insert(Water);
                tx_planet
                    .send(PlanetToExplorer::SupportedCombinationResponse { combination_list: set })
                    .unwrap();
            }
            _ => panic!("planet did not receive SupportedCombinationRequest"),
        });

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::SupportedCombinationRequest);
        assert!(matches!(res, Ok(None)));
        assert!(explorer.current_combination_cookbook.contains(&Water));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::SupportedCombinationResult { explorer_id, combination_list }) => {
                assert_eq!(explorer_id, 31);
                assert!(combination_list.contains(&Water));
            }
            _ => panic!("expected SupportedCombinationResult"),
        }
        planet.join().unwrap();
    }

    #[test]
    fn test_flow3_generate_failure() {
        // Planet replies None -> generation failed.
        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("GenBot", 32, 0);

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::GenerateResourceRequest { explorer_id, resource }) => {
                assert_eq!(explorer_id, 32);
                assert_eq!(resource, Oxygen);
                tx_planet
                    .send(PlanetToExplorer::GenerateResourceResponse { resource: None })
                    .unwrap();
            }
            _ => panic!("planet did not receive GenerateResourceRequest"),
        });

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::GenerateResourceRequest { to_generate: Oxygen });
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::GenerateResourceResponse { explorer_id, generated }) => {
                assert_eq!(explorer_id, 32);
                assert!(generated.is_err());
            }
            _ => panic!("expected GenerateResourceResponse"),
        }
        planet.join().unwrap();
    }

    #[test]
    fn test_flow4_combine_insufficient_resources() {
        // Empty bag -> combine early-returns Err BEFORE touching the planet.
        let (mut explorer, _tx_orch, rx_orch, rx_planet, _tx_planet) = make("CombineBot", 14, 0);

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::CombineResourceRequest { to_generate: Water });
        assert!(matches!(res, Ok(None)));

        assert!(rx_planet.try_recv().is_err(), "planet must NOT receive a request on insufficient resources");

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::CombineResourceResponse { explorer_id, generated }) => {
                assert_eq!(explorer_id, 14);
                assert!(generated.is_err());
            }
            _ => panic!("expected CombineResourceResponse"),
        }
    }

    // =====================================================================
    // B. DIRECT PLANET METHOD (energy cells)
    // =====================================================================

    #[test]
    fn test_energy_cells_success() {
        let (explorer, _tx_orch, _rx_orch, rx_planet, tx_planet) = make("EnergyBot", 42, 0);

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id }) => {
                assert_eq!(explorer_id, 42);
                tx_planet
                    .send(PlanetToExplorer::AvailableEnergyCellResponse { available_cells: 10 })
                    .unwrap();
            }
            _ => panic!("planet did not receive AvailableEnergyCellRequest"),
        });

        assert_eq!(explorer.ask_planet_for_available_energy_cells(), 10);
        planet.join().unwrap();
    }

    #[test]
    fn test_energy_cells_timeout_returns_zero() {
        // Receiver kept alive (send succeeds) but never answers -> timeout -> 0.
        let (explorer, _tx_orch, _rx_orch, _rx_planet, _tx_planet) = make("EnergyTimeoutBot", 43, 0);
        assert_eq!(explorer.ask_planet_for_available_energy_cells(), 0);
    }

    // =====================================================================
    // C. SIMPLE LIFECYCLE / STATE HANDLERS
    // =====================================================================

    #[test]
    fn test_bag_content_request_empty() {
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("BagBot", 6, 0);

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::BagContentRequest);
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::BagContentResponse { explorer_id, bag_content }) => {
                assert_eq!(explorer_id, 6);
                assert!(bag_content.basic_resources.is_empty());
                assert!(bag_content.complex_resources.is_empty());
            }
            _ => panic!("expected BagContentResponse"),
        }
    }

    #[test]
    fn test_reset_routine_clears_state() {
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("ResetBot", 7, 0);

        explorer.current_neighbors = vec![1, 2, 3];
        explorer.current_generation_rules.insert(Oxygen);
        explorer.current_combination_cookbook.insert(Water);

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::ResetExplorerAI);
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::ResetExplorerAIResult { explorer_id }) => assert_eq!(explorer_id, 7),
            _ => panic!("expected ResetExplorerAIResult"),
        }

        assert!(explorer.current_neighbors.is_empty());
        assert!(explorer.current_generation_rules.is_empty());
        assert!(explorer.current_combination_cookbook.is_empty());
    }

    #[test]
    fn test_current_planet_request() {
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("PlanetBot", 8, 42);

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::CurrentPlanetRequest);
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::CurrentPlanetResult { explorer_id, planet_id }) => {
                assert_eq!(explorer_id, 8);
                assert_eq!(planet_id, 42);
            }
            _ => panic!("expected CurrentPlanetResult"),
        }
    }

    #[test]
    fn test_kill_returns_terminate_signal() {
        // Requires the handler's KillExplorer arm to send KillExplorerResult.
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("KillBot", 9, 0);

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::KillExplorer);
        assert!(matches!(res, Ok(Some(true))), "KillExplorer must signal termination");

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::KillExplorerResult { explorer_id }) => assert_eq!(explorer_id, 9),
            _ => panic!("expected KillExplorerResult"),
        }
    }

    #[test]
    fn test_neighbors_response_applies_and_clears_flag() {
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("NeighResp", 12, 0);
        explorer.awaiting_neighbors = true;

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::NeighborsResponse { neighbors: vec![3, 4, 5] });
        assert!(matches!(res, Ok(None)));
        assert_eq!(explorer.current_neighbors, vec![3, 4, 5]);
        assert!(!explorer.awaiting_neighbors);

        assert!(rx_orch.try_recv().is_err(), "NeighborsResponse produces no orchestrator reply");
    }

    #[test]
    fn test_move_to_planet_success() {
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("MoveBot", 10, 0);
        explorer.awaiting_move = true;
        explorer.current_neighbors = vec![1, 2];

        let (tx_new_planet, _rx_new_planet) = unbounded::<ExplorerToPlanet>();
        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::MoveToPlanet {
            sender_to_new_planet: Some(tx_new_planet),
            planet_id: 99,
        });
        assert!(matches!(res, Ok(None)));
        assert_eq!(explorer.current_planet_id, 99);
        assert!(!explorer.awaiting_move);
        assert!(explorer.current_neighbors.is_empty(), "neighbors must be reset after a move");

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::MovedToPlanetResult { explorer_id, planet_id }) => {
                assert_eq!(explorer_id, 10);
                assert_eq!(planet_id, 99);
            }
            _ => panic!("expected MovedToPlanetResult"),
        }
    }

    #[test]
    fn test_move_to_planet_failure_clears_flag() {
        let (mut explorer, _tx_orch, _rx_orch, _rx_planet, _tx_planet) = make("MoveFailBot", 11, 7);
        explorer.awaiting_move = true;

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::MoveToPlanet {
            sender_to_new_planet: None,
            planet_id: 99,
        });
        assert!(res.is_err());
        assert_eq!(explorer.current_planet_id, 7); // unchanged
        assert!(!explorer.awaiting_move); // cleared so a waiting AI can't hang
    }

    #[test]
    fn test_unexpected_message_is_error() {
        let (mut explorer, _tx_orch, _rx_orch, _rx_planet, _tx_planet) = make("OddBot", 13, 0);
        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::StartExplorerAI);
        assert!(res.is_err());
    }

    // =====================================================================
    // D. wait_for_start (pre-send into the buffered channel, then call directly)
    // =====================================================================

    #[test]
    fn test_wait_for_start_returns_on_start() {
        let (explorer, tx_orch, rx_orch, _rx_planet, _tx_planet) = make("StartBot", 5, 0);

        tx_orch.send(OrchestratorToExplorer::StartExplorerAI).unwrap();
        assert_eq!(explorer.wait_for_start(), Ok(false));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::StartExplorerAIResult { explorer_id }) => assert_eq!(explorer_id, 5),
            _ => panic!("expected StartExplorerAIResult"),
        }
    }

    #[test]
    fn test_wait_for_start_returns_on_kill() {
        let (explorer, tx_orch, rx_orch, _rx_planet, _tx_planet) = make("StartKillBot", 5, 0);

        tx_orch.send(OrchestratorToExplorer::KillExplorer).unwrap();
        assert_eq!(explorer.wait_for_start(), Ok(true));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::KillExplorerResult { explorer_id }) => assert_eq!(explorer_id, 5),
            _ => panic!("expected KillExplorerResult"),
        }
    }

    // =====================================================================
    // E. StopExplorerAI (blocks in wait_for_start, then resumes on Start)
    // =====================================================================

    #[test]
    fn test_stop_then_start_resumes() {
        let (mut explorer, tx_orch, rx_orch, _rx_planet, _tx_planet) = make("StopBot", 4, 0);

        let handle = thread::spawn(move || {
            explorer.handle_orchestrator_msg(OrchestratorToExplorer::StopExplorerAI)
        });

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::StopExplorerAIResult { explorer_id }) => assert_eq!(explorer_id, 4),
            _ => panic!("expected StopExplorerAIResult"),
        }

        tx_orch.send(OrchestratorToExplorer::StartExplorerAI).unwrap();

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::StartExplorerAIResult { explorer_id }) => assert_eq!(explorer_id, 4),
            _ => panic!("expected StartExplorerAIResult after resume"),
        }

        assert!(matches!(handle.join().unwrap(), Ok(None)));
    }

    // =====================================================================
    // F. CONDVAR HELPERS — a fake listener applies the reply under the lock
    //    + notifies, exactly as run()'s listener does.
    // =====================================================================

    #[test]
    fn test_request_neighbors_and_wait() {
        let (explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("NeighborWaitBot", 1, 0);
        let slot: SharedExplorer = Arc::new((Mutex::new(explorer), Condvar::new()));
        let slot_listener = Arc::clone(&slot);

        let fake = thread::spawn(move || {
            match rx_orch.recv_timeout(T) {
                Ok(ExplorerToOrchestrator::NeighborsRequest { explorer_id, .. }) => assert_eq!(explorer_id, 1),
                _ => panic!("expected NeighborsRequest"),
            }
            let (lock, cvar) = &*slot_listener;
            let mut g = lock.lock().unwrap();
            let _ = g.handle_orchestrator_msg(OrchestratorToExplorer::NeighborsResponse { neighbors: vec![7, 8] });
            drop(g);
            cvar.notify_all();
        });

        assert!(Explorer::request_neighbors_and_wait(&slot).is_ok());

        let g = slot.0.lock().unwrap();
        assert_eq!(g.current_neighbors, vec![7, 8]);
        assert!(!g.awaiting_neighbors);
        drop(g);
        fake.join().unwrap();
    }

    #[test]
    fn test_travel_and_wait_success() {
        let (explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("TravelBot", 2, 0);
        let slot: SharedExplorer = Arc::new((Mutex::new(explorer), Condvar::new()));
        let slot_listener = Arc::clone(&slot);

        let fake = thread::spawn(move || {
            match rx_orch.recv_timeout(T) {
                Ok(ExplorerToOrchestrator::TravelToPlanetRequest { dst_planet_id, .. }) => assert_eq!(dst_planet_id, 5),
                _ => panic!("expected TravelToPlanetRequest"),
            }
            let (tx_new_planet, _rx_new_planet) = unbounded::<ExplorerToPlanet>();
            let (lock, cvar) = &*slot_listener;
            let mut g = lock.lock().unwrap();
            let _ = g.handle_orchestrator_msg(OrchestratorToExplorer::MoveToPlanet {
                sender_to_new_planet: Some(tx_new_planet),
                planet_id: 5,
            });
            drop(g);
            cvar.notify_all();
            let _ = rx_orch.recv_timeout(T); // drain MovedToPlanetResult
        });

        assert!(Explorer::travel_and_wait(&slot, 5).is_ok());

        let g = slot.0.lock().unwrap();
        assert_eq!(g.current_planet_id, 5);
        assert!(!g.awaiting_move);
        drop(g);
        fake.join().unwrap();
    }

    #[test]
    fn test_travel_and_wait_failure() {
        let (explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("TravelFailBot", 3, 0);
        let slot: SharedExplorer = Arc::new((Mutex::new(explorer), Condvar::new()));
        let slot_listener = Arc::clone(&slot);

        let fake = thread::spawn(move || {
            match rx_orch.recv_timeout(T) {
                Ok(ExplorerToOrchestrator::TravelToPlanetRequest { .. }) => {}
                _ => panic!("expected TravelToPlanetRequest"),
            }
            let (lock, cvar) = &*slot_listener;
            let mut g = lock.lock().unwrap();
            let _ = g.handle_orchestrator_msg(OrchestratorToExplorer::MoveToPlanet {
                sender_to_new_planet: None,
                planet_id: 5,
            });
            drop(g);
            cvar.notify_all();
        });

        assert!(Explorer::travel_and_wait(&slot, 5).is_err());

        let g = slot.0.lock().unwrap();
        assert_eq!(g.current_planet_id, 0); // unchanged
        assert!(!g.awaiting_move);
        drop(g);
        fake.join().unwrap();
    }

    // =====================================================================
    // G. INTEGRATION through run(): start handshake -> listener -> planet round-trip
    // =====================================================================

    #[test]
    fn test_run_listener_integration() {
        let (explorer, tx_orch, rx_orch, rx_planet, tx_planet) = make("RunBot", 20, 0);
        thread::spawn(move || explorer.run());

        tx_orch.send(OrchestratorToExplorer::StartExplorerAI).unwrap();
        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::StartExplorerAIResult { explorer_id }) => assert_eq!(explorer_id, 20),
            _ => panic!("expected StartExplorerAIResult"),
        }

        tx_orch.send(OrchestratorToExplorer::SupportedResourceRequest).unwrap();
        match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::SupportedResourceRequest { explorer_id }) => {
                assert_eq!(explorer_id, 20);
                let mut set = HashSet::new();
                set.insert(Oxygen);
                tx_planet
                    .send(PlanetToExplorer::SupportedResourceResponse { resource_list: set })
                    .unwrap();
            }
            _ => panic!("planet did not receive SupportedResourceRequest"),
        }
        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::SupportedResourceResult { supported_resources, .. }) => {
                assert!(supported_resources.contains(&Oxygen));
            }
            _ => panic!("orchestrator did not receive SupportedResourceResult"),
        }
    }

    // =====================================================================
    // H. RESOURCE-DEPENDENT PATHS — real BasicResource/ComplexResource minted
    //    via a throwaway Planet's recipe-loaded Generator/Combinator.
    //
    //    ASSUMPTION: EnergyCell::new(), EnergyCell::charge(Sunray) and
    //    Sunray::new() are `pub` (common_game's own tests use them this way).
    //    If your crate can't see them, tell me and we mint differently.
    // =====================================================================

    // Minimal AI so Planet::new succeeds; its handlers are never invoked here.

    /*
    struct NoopAI;
    impl PlanetAI for NoopAI {
        fn handle_sunray(&mut self, _: &mut PlanetState, _: &Generator, _: &Combinator, _: Sunray) {}
        fn handle_asteroid(&mut self, _: &mut PlanetState, _: &Generator, _: &Combinator) -> Option<Rocket> {
            None
        }
        fn handle_internal_state_req(&mut self, s: &mut PlanetState, _: &Generator, _: &Combinator) -> DummyPlanetState {
            s.to_dummy()
        }
        fn handle_explorer_msg(&mut self, _: &mut PlanetState, _: &Generator, _: &Combinator, _: ExplorerToPlanet) -> Option<PlanetToExplorer> {
            None
        }
    }

    // Owns a Planet purely to borrow its recipe-loaded Generator/Combinator.
    struct Minter {
        planet: Planet,
    }
    impl Minter {
        fn new() -> Self {
            let (_a, rx_o2p) = unbounded::<OrchestratorToPlanet>();
            let (tx_p2o, _b) = unbounded::<PlanetToOrchestrator>();
            let (_c, rx_e2p) = unbounded::<ExplorerToPlanet>();
            // Type B: unbounded gen rules + 1 combination rule (Water).
            let planet = Planet::new(
                0,
                PlanetType::B,
                Box::new(NoopAI),
                vec![Oxygen, Hydrogen, Carbon, Silicon],
                vec![Water],
                (rx_o2p, tx_p2o),
                rx_e2p,
            )
                .expect("minter planet construction");
            Minter { planet }
        }

        fn charged() -> EnergyCell {
            let mut c = EnergyCell::new();
            c.charge(Sunray::new());
            c
        }

        fn basic(&self, ty: BasicResourceType) -> BasicResource {
            let mut cell = Self::charged();
            match ty {
                Oxygen => self.planet.generator().make_oxygen(&mut cell).unwrap().to_basic(),
                Hydrogen => self.planet.generator().make_hydrogen(&mut cell).unwrap().to_basic(),
                Carbon => self.planet.generator().make_carbon(&mut cell).unwrap().to_basic(),
                Silicon => self.planet.generator().make_silicon(&mut cell).unwrap().to_basic(),
            }
        }

        fn water(&self) -> ComplexResource {
            let h = self.planet.generator().make_hydrogen(&mut Self::charged()).unwrap();
            let o = self.planet.generator().make_oxygen(&mut Self::charged()).unwrap();
            self.planet
                .combinator()
                .make_water(h, o, &mut Self::charged())
                .unwrap()
                .to_complex()
        }
    }

    #[test]
    fn test_flow3_generate_success() {
        let minter = Minter::new();
        let oxygen = minter.basic(Oxygen); // real resource the planet will "produce"

        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("GenOkBot", 33, 0);

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::GenerateResourceRequest { explorer_id, resource }) => {
                assert_eq!(explorer_id, 33);
                assert_eq!(resource, Oxygen);
                tx_planet
                    .send(PlanetToExplorer::GenerateResourceResponse { resource: Some(oxygen) })
                    .unwrap();
            }
            _ => panic!("planet did not receive GenerateResourceRequest"),
        });

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::GenerateResourceRequest { to_generate: Oxygen });
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::GenerateResourceResponse { explorer_id, generated }) => {
                assert_eq!(explorer_id, 33);
                assert!(generated.is_ok());
            }
            _ => panic!("expected GenerateResourceResponse"),
        }

        let snap = explorer.bag.snapshot();
        assert_eq!(snap.basic_resources.get(&Oxygen).copied().unwrap_or(0), 1, "oxygen should be in the bag");

        planet.join().unwrap();
    }

    #[test]
    fn test_flow4_combine_success() {
        let minter = Minter::new();
        let water = minter.water(); // real resource the planet will "produce"

        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("CombineOkBot", 34, 0);

        // Seed the inputs the explorer will take from the bag for a Water combine.
        explorer.bag.add_basic(minter.basic(Hydrogen));
        explorer.bag.add_basic(minter.basic(Oxygen));

        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::CombineResourceRequest { explorer_id, msg: _ }) => {
                assert_eq!(explorer_id, 34);
                tx_planet
                    .send(PlanetToExplorer::CombineResourceResponse { complex_response: Ok(water) })
                    .unwrap();
            }
            _ => panic!("planet did not receive CombineResourceRequest"),
        });

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::CombineResourceRequest { to_generate: Water });
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::CombineResourceResponse { explorer_id, generated }) => {
                assert_eq!(explorer_id, 34);
                assert!(generated.is_ok());
            }
            _ => panic!("expected CombineResourceResponse"),
        }

        let snap = explorer.bag.snapshot();
        assert_eq!(snap.complex_resources.get(&Water).copied().unwrap_or(0), 1, "water produced");
        assert_eq!(snap.basic_resources.get(&Hydrogen).copied().unwrap_or(0), 0, "hydrogen consumed");
        assert_eq!(snap.basic_resources.get(&Oxygen).copied().unwrap_or(0), 0, "oxygen consumed");

        planet.join().unwrap();
    }

    #[test]
    fn test_flow4_combine_planet_error_rolls_back() {
        let minter = Minter::new();
        let (mut explorer, _tx_orch, rx_orch, rx_planet, tx_planet) = make("CombineRollbackBot", 35, 0);

        explorer.bag.add_basic(minter.basic(Hydrogen));
        explorer.bag.add_basic(minter.basic(Oxygen));

        // Planet "fails" and hands the two inputs straight back in the Err tuple.
        let planet = thread::spawn(move || match rx_planet.recv_timeout(T) {
            Ok(ExplorerToPlanet::CombineResourceRequest { msg: ComplexResourceRequest::Water(h, o), .. }) => {
                tx_planet
                    .send(PlanetToExplorer::CombineResourceResponse {
                        complex_response: Err(("planet failed".to_string(), h.to_generic(), o.to_generic())),
                    })
                    .unwrap();
            }
            _ => panic!("planet did not receive a Water CombineResourceRequest"),
        });

        let res = explorer
            .handle_orchestrator_msg(OrchestratorToExplorer::CombineResourceRequest { to_generate: Water });
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::CombineResourceResponse { explorer_id, generated }) => {
                assert_eq!(explorer_id, 35);
                assert!(generated.is_err());
            }
            _ => panic!("expected CombineResourceResponse"),
        }

        // No resource lost: both inputs are back in the bag.
        let snap = explorer.bag.snapshot();
        assert_eq!(snap.basic_resources.get(&Hydrogen).copied().unwrap_or(0), 1, "hydrogen returned");
        assert_eq!(snap.basic_resources.get(&Oxygen).copied().unwrap_or(0), 1, "oxygen returned");

        planet.join().unwrap();
    }

    #[test]
    fn test_bag_content_request_with_contents() {
        let minter = Minter::new();
        let (mut explorer, _tx_orch, rx_orch, _rx_planet, _tx_planet) = make("BagFullBot", 36, 0);

        explorer.bag.add_basic(minter.basic(Oxygen));
        explorer.bag.add_complex(minter.water());

        let res = explorer.handle_orchestrator_msg(OrchestratorToExplorer::BagContentRequest);
        assert!(matches!(res, Ok(None)));

        match rx_orch.recv_timeout(T) {
            Ok(ExplorerToOrchestrator::BagContentResponse { explorer_id, bag_content }) => {
                assert_eq!(explorer_id, 36);
                assert_eq!(bag_content.basic_resources.get(&Oxygen).copied().unwrap_or(0), 1);
                assert_eq!(bag_content.complex_resources.get(&Water).copied().unwrap_or(0), 1);
            }
            _ => panic!("expected BagContentResponse"),
        }
    }
    */
}