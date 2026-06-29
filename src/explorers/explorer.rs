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
    killed: bool,
    stopped: bool,
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
            killed: false,
            stopped: false,
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
    fn on_stop(&mut self) { self.stopped = true; }
    fn on_start(&mut self) { self.stopped = false; }
    fn on_kill (&mut self) {
        self.killed = true;
        self.awaiting_move = false;
        self.awaiting_neighbors = false;
    }

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
                    ).emit();
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
    pub(crate) fn is_killed(&self) -> bool {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().killed
    }
    pub(crate) fn is_stopped(&self) -> bool {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().stopped
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
    pub(crate) fn known_resources(&self) -> HashSet<BasicResourceType> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().current_generation_rules.clone()
    }
    pub(crate) fn known_combinations(&self) -> HashSet<ComplexResourceType> {
        let (lock, _) = &*self.slot;
        lock.lock().unwrap().current_combination_cookbook.clone()
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
// =========================================================================
// UNIT TESTS
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::time::Duration;

    const T: Duration = Duration::from_millis(200);

    fn make(
        name: &str,
        explorer_id: ID,
        current_planet_id: ID,
    ) -> (
        Explorer,
        Receiver<ExplorerToOrchestrator<BagSnapshot>>,
        Sender<OrchestratorToExplorer>,
        Sender<PlanetToExplorer>,
        Receiver<ExplorerToPlanet>,
    ) {
        let (tx_orch, rx_orch) = unbounded();
        let (tx_to_exp, rx_from_orch) = unbounded();
        let (tx_planet, rx_from_planet) = unbounded();
        let (tx_to_planet, rx_planet) = unbounded();

        let exp = Explorer::new(
            name.to_string(),
            rx_from_orch,
            tx_orch,
            tx_to_planet,
            rx_from_planet,
            explorer_id,
            current_planet_id,
            |_ai| {} // Dummy behaviour vuoto per bypassare il loop AI
        );

        (exp, rx_orch, tx_to_exp, tx_planet, rx_planet)
    }

    #[test]
    fn test_ask_planet_for_resources_success() {
        let (mut explorer, _rx_orch, _tx_to_exp, tx_planet, rx_planet) = make("ResourceBot", 10, 1);

        let planet_thread = thread::spawn(move || {
            match rx_planet.recv_timeout(T) {
                Ok(ExplorerToPlanet::SupportedResourceRequest { explorer_id }) => {
                    assert_eq!(explorer_id, 10);
                }
                _ => panic!("Expected SupportedResourceRequest message"),
            }

            let mut resources = HashSet::new();
            resources.insert(Oxygen);
            resources.insert(Carbon);

            tx_planet.send(PlanetToExplorer::SupportedResourceResponse {
                resource_list: resources,
            }).unwrap();
        });

        let res = explorer.ask_planet_for_resources();
        assert!(res.is_ok());

        planet_thread.join().unwrap();

        assert!(explorer.current_generation_rules.contains(&Oxygen));
        assert!(explorer.current_generation_rules.contains(&Carbon));
    }

    #[test]
    fn test_ask_planet_for_combinations_success() {
        let (mut explorer, _rx_orch, _tx_to_exp, tx_planet, rx_planet) = make("ComboBot", 11, 2);

        let planet_thread = thread::spawn(move || {
            match rx_planet.recv_timeout(T) {
                Ok(ExplorerToPlanet::SupportedCombinationRequest { explorer_id }) => {
                    assert_eq!(explorer_id, 11);
                }
                _ => panic!("Expected SupportedCombinationRequest message"),
            }

            let mut combos = HashSet::new();
            combos.insert(Water);

            tx_planet.send(PlanetToExplorer::SupportedCombinationResponse {
                combination_list: combos,
            }).unwrap();
        });

        let res = explorer.ask_planet_for_combinations();
        assert!(res.is_ok());

        planet_thread.join().unwrap();

        assert!(explorer.current_combination_cookbook.contains(&Water));
    }

    #[test]
    fn test_generate_resource_refusal() {
        let (mut explorer, _rx_orch, _tx_to_exp, tx_planet, rx_planet) = make("FailBot", 12, 3);

        let planet_thread = thread::spawn(move || {
            if let Ok(ExplorerToPlanet::GenerateResourceRequest { explorer_id, resource }) = rx_planet.recv_timeout(T) {
                assert_eq!(explorer_id, 12);
                assert_eq!(resource, Oxygen);
            } else {
                panic!("Expected GenerateResourceRequest message");
            }

            tx_planet.send(PlanetToExplorer::GenerateResourceResponse {
                resource: None,
            }).unwrap();
        });

        let res = explorer.generate_resource_from_planet(Oxygen);
        assert!(res.is_err(), "La generazione doveva fallire correttamente");
        assert_eq!(res.unwrap_err(), "Resource generation failed or timed out");

        planet_thread.join().unwrap();
    }

    #[test]
    fn test_energy_cells_request() {
        let (explorer, _rx_orch, _tx_to_exp, tx_planet, rx_planet) = make("EnergyBot", 13, 4);

        let planet_thread = thread::spawn(move || {
            if let Ok(ExplorerToPlanet::AvailableEnergyCellRequest { explorer_id }) = rx_planet.recv_timeout(T) {
                assert_eq!(explorer_id, 13);
            }

            tx_planet.send(PlanetToExplorer::AvailableEnergyCellResponse {
                available_cells: 42,
            }).unwrap();
        });

        let cells = explorer.ask_planet_for_available_energy_cells();
        assert_eq!(cells, 42);

        planet_thread.join().unwrap();
    }

    #[test]
    fn test_handle_orchestrator_msg_neighbors_response() {
        let (mut explorer, _rx_orch, _tx_to_exp, _tx_planet, _rx_planet) = make("NavBot", 14, 5);

        explorer.awaiting_neighbors = true;

        let msg = OrchestratorToExplorer::NeighborsResponse {
            neighbors: vec![99, 100],
        };

        let res = explorer.handle_orchestrator_msg(msg);
        assert!(res.is_ok());
        assert_eq!(explorer.current_neighbors, vec![99, 100]);
        assert!(!explorer.awaiting_neighbors);
    }

    #[test]
    fn test_handle_orchestrator_msg_reset() {
        let (mut explorer, rx_orch, _tx_to_exp, _tx_planet, _rx_planet) = make("ResetBot", 15, 6);

        explorer.current_neighbors = vec![1, 2, 3];
        explorer.current_generation_rules.insert(Oxygen);

        let msg = OrchestratorToExplorer::ResetExplorerAI;
        let res = explorer.handle_orchestrator_msg(msg);
        assert!(res.is_ok());

        assert!(explorer.current_neighbors.is_empty());
        assert!(explorer.current_generation_rules.is_empty());

        if let Ok(ExplorerToOrchestrator::ResetExplorerAIResult { explorer_id }) = rx_orch.recv_timeout(T) {
            assert_eq!(explorer_id, 15);
        } else {
            panic!("L'esploratore non ha inviato ResetExplorerAIResult all'orchestrator");
        }
    }
}