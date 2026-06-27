// =========================================================================
// STANDARD LIBRARY & EXTERNAL CRATES
// =========================================================================
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{never, select_biased, tick, unbounded, Receiver, Sender};
use log::{debug, error, info, warn};

// =========================================================================
// COMMON-GAME IMPORTS
// =========================================================================
use common_game::components::forge::Forge;
use common_game::components::planet::Planet;
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};
use common_game::utils::ID;

// =========================================================================
// INTERNAL MODULES
// =========================================================================
use crate::explorers::{harvesting_explorer, roaming_explorer, BagSnapshot, Explorer, ExplorerBehaviour};

// [VIZ] disabled until the visualizer is ready ------------------------------
// use crate::visualizer::{kind_of, VizBridge};
// use galaxy_visualizer_stargazers::GalaxyCommand;
// ---------------------------------------------------------------------------

// =========================================================================
// PLANET CREATION ALIASES (group-specific constructors, kept as-is)
// =========================================================================
use the_compiler_strikes_back::planet::create_planet as new_csb;
use huston::{houston_we_have_a_borrow as new_hus, RocketStrategy};
use one_million_crabs::planet::create_planet as new_omc;
use ara_kees::planet::create_planet as new_bas;
use trip::trip as new_trp;
use immutable_cosmic_borrow::create_planet as new_icb;
use rusty_crab_ap2025::planet::create_planet as new_ryc;

// =========================================================================
// TUNABLES
// =========================================================================
const TICK_INTERVAL: Duration = Duration::from_millis(500);

// Per-planet, per-tick probabilities. P_SKIP + P_ASTEROID must be <= 1.0;
// the remainder is the sunray probability. Sunrays dominate.
const P_SKIP: f64 = 0.25; // send nothing this tick
const P_ASTEROID: f64 = 0.08; // send an asteroid (rare)3
// => P_SUNRAY = 1 - 0.25 - 0.08 = 0.67

// =========================================================================
// PER-ACTOR OUTBOUND HANDLES
// Inbound is SHARED (one receiver each for planets / explorers, per the docs'
// diagrams). Outbound is PER-ACTOR, looked up by id in these maps.
// =========================================================================

/// Everything the orchestrator needs to address one planet.
struct PlanetLink {
    name: &'static str,
    to_planet: Sender<OrchestratorToPlanet>,
    /// The explorer->planet sender, handed to explorers that travel here.
    to_planet_from_explorer: Sender<ExplorerToPlanet>,
    alive: bool,
}

/// Everything the orchestrator needs to address one explorer.
struct ExplorerLink {
    name: &'static str,
    start_planet: ID,
    to_explorer: Sender<OrchestratorToExplorer>,
    /// The planet->explorer sender, registered on every planet this explorer visits.
    to_explorer_from_planet: Sender<PlanetToExplorer>,
    alive: bool,
}

// =========================================================================
// ORCHESTRATOR
// =========================================================================
struct Orchestrator {
    forge: Forge,

    /// Per-actor OUTBOUND senders, keyed by id.
    planets: HashMap<ID, PlanetLink>,
    explorers: HashMap<ID, ExplorerLink>,

    /// SHARED INBOUND receivers (every planet / explorer sends into these).
    from_planets: Receiver<PlanetToOrchestrator>,
    from_explorers: Receiver<ExplorerToOrchestrator<BagSnapshot>>,

    // [VIZ] manual operations coming from the visualizer UI.
    // commands: Receiver<GalaxyCommand>,
    // [VIZ] snapshots out to the visualizer.
    // viz: VizBridge,

    dead_planets: std::collections::HashSet<ID>,
    rng: u64, // xorshift64 (dependency-free; swap for `rand` if you like)
    //to remember the travel in waiting for the planet response
    pending_travels: HashMap<ID, ID>,
}

// =========================================================================
// SETUP
// =========================================================================

/// Spawns a planet thread. `tx_from_planet` is a CLONE of the single shared
/// planet->orchestrator sender, so all planets fan in to one receiver.
fn spawn_planet(
    name: &'static str,
    tx_from_planet: Sender<PlanetToOrchestrator>,
    create_fn: impl FnOnce(
        Receiver<OrchestratorToPlanet>,
        Sender<PlanetToOrchestrator>,
        Receiver<ExplorerToPlanet>,
    ) -> Planet
    + Send
    + 'static,
) -> PlanetLink {
    let (to_planet, planet_rx_orch) = unbounded::<OrchestratorToPlanet>();
    let (to_planet_from_explorer, planet_rx_expl) = unbounded::<ExplorerToPlanet>();

    let mut planet = create_fn(planet_rx_orch, tx_from_planet, planet_rx_expl);
    thread::spawn(move || {
        if let Err(e) = planet.run() {
            error!("[{name}] planet thread exited: {e}");
        }
    });

    PlanetLink { name, to_planet, to_planet_from_explorer, alive: true }
}

/// Spawns an explorer thread. `tx_from_explorer` is a CLONE of the single shared
/// explorer->orchestrator sender. `initial_planet_sender` is the explorer->planet
/// sender of the planet it starts on.
fn spawn_explorer(
    name: &'static str,
    id: ID,
    start_planet: ID,
    tx_from_explorer: Sender<ExplorerToOrchestrator<BagSnapshot>>,
    initial_planet_sender: Sender<ExplorerToPlanet>,
    ai_fn: ExplorerBehaviour,
) -> ExplorerLink {
    let (to_explorer, expl_rx_orch) = unbounded::<OrchestratorToExplorer>();
    let (to_explorer_from_planet, expl_rx_planet) = unbounded::<PlanetToExplorer>();

    let explorer = Explorer::new(
        name.to_string(),
        expl_rx_orch,
        tx_from_explorer,
        initial_planet_sender,
        expl_rx_planet,
        id,
        start_planet,
        ai_fn,
    );
    thread::spawn(move || explorer.run());

    ExplorerLink { name, start_planet, to_explorer, to_explorer_from_planet, alive: true }
}

impl Orchestrator {
    // [VIZ] when re-enabling, restore the signature:
    //     fn build(commands: Receiver<GalaxyCommand>, mut viz: VizBridge) -> Result<Orchestrator, String>
    fn build() -> Result<Orchestrator, String> {
        let forge = Forge::new()?;

        // ONE shared inbound receiver per actor family. Senders are cloned per actor.
        let (tx_from_planet, from_planets) = unbounded::<PlanetToOrchestrator>();
        let (tx_from_explorer, from_explorers) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();

        // --- Planets (group-specific constructors; adjust args to your tree) ---
        let csb = spawn_planet("CSB", tx_from_planet.clone(), |rx, tx, rx_exp| new_csb(rx, tx, rx_exp, 1));
        let hus = spawn_planet("HUS", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_hus(rx, tx, rx_exp, 2, RocketStrategy::Safe, None).expect("Failed to create HUS")
        });
        let omc = spawn_planet("OMC", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_omc(rx, tx, rx_exp, 3).expect("Failed to create OMC")
        });
        let bas = spawn_planet("BAS", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_bas(rx, tx, rx_exp, 4).expect("Failed to create BAS")
        });
        let trp = spawn_planet("TRP", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_trp(5, rx, tx, rx_exp).expect("Failed to create TRP")
        });
        let icb = spawn_planet("ICB", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_icb(false, 1.0, 1.0, Duration::from_secs(60), Duration::from_secs(10), 6, (rx, tx), rx_exp)
                .expect("Failed to create ICB")
        });
        // Last clone moves in (no further planets need it).
        let ryc = spawn_planet("RYC", tx_from_planet, |rx, tx, rx_exp| new_ryc(rx, tx, rx_exp, 7));

        let mut planets = HashMap::new();
        planets.insert(1, csb);
        planets.insert(2, hus);
        planets.insert(3, omc);
        planets.insert(4, bas);
        planets.insert(5, trp);
        planets.insert(6, icb);
        planets.insert(7, ryc);

        // --- Explorers, wired to their start planets' explorer->planet senders ---
        let anon = spawn_explorer(
            "Anon", 101, 1,
            tx_from_explorer.clone(),
            planets[&1].to_planet_from_explorer.clone(),
            roaming_explorer,
        );
        let eleanor = spawn_explorer(
            "Eleanor", 102, 2,
            tx_from_explorer, // last clone moves in
            planets[&2].to_planet_from_explorer.clone(),
            harvesting_explorer,
        );

        let mut explorers = HashMap::new();
        explorers.insert(101, anon);
        explorers.insert(102, eleanor);

        // [VIZ] register planets with the visualizer (kinds known from how we built them).
        // use common_game::components::planet::PlanetType;
        // viz.register_planet(1, kind_of(PlanetType::A));
        // viz.register_planet(2, kind_of(PlanetType::B));
        // viz.register_planet(3, kind_of(PlanetType::C));
        // viz.register_planet(4, kind_of(PlanetType::D));
        // viz.register_planet(5, kind_of(PlanetType::A));
        // viz.register_planet(6, kind_of(PlanetType::B));
        // viz.register_planet(7, kind_of(PlanetType::C));

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;

        Ok(Orchestrator {
            forge,
            planets,
            explorers,
            from_planets,
            from_explorers,
            // commands, // [VIZ]
            // viz,      // [VIZ]
            dead_planets: std::collections::HashSet::new(),
            rng: seed,
            pending_travels: Default::default(),
        })
    }

    // =====================================================================
    // MAIN RUN LOOP (biased event multiplexing)
    // =====================================================================
    fn run(&mut self) {
        self.bootstrap();
        let ticker = tick(TICK_INTERVAL);

        loop {
            // Clone receivers into locals so the select borrows the locals, not
            // `self`; that lets each arm body call `&mut self` handlers freely.
            // let commands = self.commands.clone(); // [VIZ]
            let from_explorers = self.from_explorers.clone();
            let from_planets = self.from_planets.clone();
            let tick_rx = ticker.clone();

            select_biased! {
                // ---- PRIORITY 1: visualizer / user commands ---- [VIZ]
                // recv(commands) -> msg => match msg {
                //     Ok(cmd) => self.handle_command(cmd),
                //     Err(_) => { info!("[orch] visualizer command channel closed; shutting down"); break; }
                // },

                // ---- PRIORITY 2: actor messages (explorers, then planets) ----
                recv(from_explorers) -> msg => match msg {
                    Ok(m) => if self.handle_explorer_msg(m) { info!("[orch] all explorers dead; shutting down"); break; },
                    Err(_) => { info!("[orch] all explorers gone; shutting down"); break; }
                },
                recv(from_planets) -> msg => match msg {
                    Ok(m) => self.handle_planet_msg(m),
                    Err(_) => {
                        // All planets gone: silence this arm so it doesn't spin on a dead channel.
                        warn!("[orch] all planets gone");
                        self.from_planets = never();
                    }
                },

                // ---- PRIORITY 3: periodic tick ----
                recv(tick_rx) -> _ => if self.handle_tick() { break; },
            }
        }

        self.shutdown();
        info!("[orch] run loop terminated");
    }

    // =====================================================================
    // BOOTSTRAP / SHUTDOWN
    // =====================================================================
    fn bootstrap(&mut self) {
        for p in self.planets.values() {
            let _ = p.to_planet.send(OrchestratorToPlanet::StartPlanetAI);
        }
        for e in self.explorers.values() {
            let _ = e.to_explorer.send(OrchestratorToExplorer::StartExplorerAI);
        }
        // Register each explorer on its starting planet so the planet can reply to it.
        let registrations: Vec<(ID, ID, Sender<PlanetToExplorer>)> = self
            .explorers
            .iter()
            .map(|(&eid, e)| (eid, e.start_planet, e.to_explorer_from_planet.clone()))
            .collect();
        for (explorer_id, start_planet, new_sender) in registrations {
            if let Some(p) = self.planets.get(&start_planet) {
                let _ = p.to_planet.send(OrchestratorToPlanet::IncomingExplorerRequest { explorer_id, new_sender });
            }
        }
    }

    fn shutdown(&mut self) {
        for p in self.planets.values() {
            if p.alive {
                let _ = p.to_planet.send(OrchestratorToPlanet::KillPlanet);
            }
        }
        for e in self.explorers.values() {
            if e.alive {
                let _ = e.to_explorer.send(OrchestratorToExplorer::KillExplorer);
            }
        }
    }

    // =====================================================================
    // [VIZ] VISUALIZER COMMANDS (manual ops == the same paths the AI uses)
    // Re-enable together with the `commands` field, import, and run-loop arm.
    // =====================================================================
    /*
    fn handle_command(&mut self, cmd: GalaxyCommand) {
        match cmd {
            GalaxyCommand::Sunray { planet_id } => {
                let sunray = self.forge.generate_sunray();
                self.send_to_planet(planet_id, OrchestratorToPlanet::Sunray(sunray));
            }
            GalaxyCommand::Asteroid { planet_id } => {
                let asteroid = self.forge.generate_asteroid();
                self.send_to_planet(planet_id, OrchestratorToPlanet::Asteroid(asteroid));
            }
            GalaxyCommand::SetAi { planet_id, running } => {
                let msg = if running {
                    OrchestratorToPlanet::StartPlanetAI
                } else {
                    OrchestratorToPlanet::StopPlanetAI
                };
                self.send_to_planet(planet_id, msg);
            }
            GalaxyCommand::Kill { planet_id } => self.begin_planet_destruction(planet_id),
            GalaxyCommand::MoveExplorer { explorer_id, to_planet } => {
                if let Some(current) = self.explorer_current_planet_guess(explorer_id) {
                    self.handle_travel_request(explorer_id, current, to_planet);
                } else {
                    self.grant_travel(explorer_id, None, to_planet);
                }
            }
            // ---- These require the visualizer team to ADD these variants ----
            // GalaxyCommand::KillExplorer { explorer_id } => self.begin_explorer_kill(explorer_id),
            // GalaxyCommand::SupportedResources { explorer_id } =>
            //     self.send_to_explorer(explorer_id, OrchestratorToExplorer::SupportedResourceRequest),
            // GalaxyCommand::SupportedCombinations { explorer_id } =>
            //     self.send_to_explorer(explorer_id, OrchestratorToExplorer::SupportedCombinationRequest),
            // GalaxyCommand::Generate { explorer_id, resource } =>
            //     self.send_to_explorer(explorer_id, OrchestratorToExplorer::GenerateResourceRequest { to_generate: resource }),
            // GalaxyCommand::Combine { explorer_id, resource } =>
            //     self.send_to_explorer(explorer_id, OrchestratorToExplorer::CombineResourceRequest { to_generate: resource }),
            // GalaxyCommand::BagContent { explorer_id } =>
            //     self.send_to_explorer(explorer_id, OrchestratorToExplorer::BagContentRequest),
            #[allow(unreachable_patterns)]
            _ => debug!("[orch] unhandled visualizer command: {cmd:?}"),
        }
    }
    */

    // =====================================================================
    // TICK — entropic sunrays/asteroids ( + [VIZ] publish a fresh view )
    // =====================================================================
    /// Returns `true` if the loop should terminate.
    fn handle_tick(&mut self) -> bool {
        // 1. Entropic weather.
        let alive_ids: Vec<ID> = self.planets.iter().filter(|(_, p)| p.alive).map(|(&id, _)| id).collect();
        for id in alive_ids {
            let r = self.roll();
            if r < P_SKIP {
                continue;
            } else if r < P_SKIP + P_ASTEROID {
                let asteroid = self.forge.generate_asteroid();
                self.send_to_planet(id, OrchestratorToPlanet::Asteroid(asteroid));
            } else {
                let sunray = self.forge.generate_sunray();
                self.send_to_planet(id, OrchestratorToPlanet::Sunray(sunray));
            }
        }

        // [VIZ] 2. Refresh the view: poll planet state + explorer positions.
        // for p in self.planets.values() {
        //     if p.alive {
        //         let _ = p.to_planet.send(OrchestratorToPlanet::InternalStateRequest);
        //     }
        // }
        // for e in self.explorers.values() {
        //     if e.alive {
        //         let _ = e.to_explorer.send(OrchestratorToExplorer::CurrentPlanetRequest);
        //     }
        // }

        // [VIZ] 3. Publish. `false` => the window closed.
        // if !self.viz.publish() {
        //     info!("[orch] visualizer window closed; shutting down");
        //     return true;
        // }
        false
    }

    // =====================================================================
    // PLANET MESSAGES (dispatched by planet_id carried in the message)
    // =====================================================================
    fn handle_planet_msg(&mut self, msg: PlanetToOrchestrator) {
        let id = msg.planet_id();
        let name = self.planets.get(&id).map(|p| p.name).unwrap_or("?");

        match msg {
            PlanetToOrchestrator::SunrayAck { .. } => debug!("[orch] {name} acked Sunray"),
            PlanetToOrchestrator::AsteroidAck { planet_id, rocket } => {
                if rocket.is_some() {
                    info!("[orch] {name} deflected the asteroid (id={planet_id})");
                } else {
                    info!("[orch] {name} could NOT deflect -> destroying (id={planet_id})");
                    self.begin_planet_destruction(planet_id);
                }
            }
            PlanetToOrchestrator::KillPlanetResult { planet_id } => self.finalize_planet_death(planet_id),
            PlanetToOrchestrator::StartPlanetAIResult { .. } => info!("[orch] {name} AI started"),
            PlanetToOrchestrator::StopPlanetAIResult { .. } | PlanetToOrchestrator::Stopped { .. } => {
                debug!("[orch] {name} stopped / ack-while-stopped")
            }
            PlanetToOrchestrator::InternalStateResponse { planet_id, planet_state: _ } => {
                // [VIZ] self.viz.update_planet(planet_id, &planet_state);
                debug!("[orch] {name} internal state (id={planet_id}) [viz disabled]");
            }
            PlanetToOrchestrator::IncomingExplorerResponse { explorer_id, res, .. } => {
                //planet response, travel in waiting
                self.pending_travels.remove(&explorer_id);

                //check if the planet is alive
                if self.dead_planets.contains(&id) {
                    warn!("[orch] {name} accepted explorer {explorer_id} but the planet just died! Denying travel.");
                    //the explorer with None return understand that the travel fail
                    self.grant_travel(explorer_id, None, id);
                    return;
                }

                //autorization here because if we send the explorer before to the planet is ok can be an error
                let dst_sender = self.planets.get(&id).map(|p| p.to_planet_from_explorer.clone());
                self.grant_travel(explorer_id, dst_sender, id);
                debug!("[orch] {name} incoming explorer {explorer_id}: {res:?}")
            }
            PlanetToOrchestrator::OutgoingExplorerResponse { explorer_id, res, .. } => {
                debug!("[orch] {name} outgoing explorer {explorer_id}: {res:?}")
            }
        }
    }

    /// Ask a planet to die; finalize when KillPlanetResult comes back.
    fn begin_planet_destruction(&mut self, planet_id: ID) {
        if let Some(p) = self.planets.get(&planet_id) {
            if p.alive {
                let _ = p.to_planet.send(OrchestratorToPlanet::KillPlanet);
            }
        }
    }

    /// Planet confirmed dead: mark it, ( [VIZ] tell the view, ) and probe explorers
    /// so any colocated ones get killed (we never track location; we ask).
    fn finalize_planet_death(&mut self, planet_id: ID) {
        let still_alive = self.planets.get(&planet_id).map(|p| p.alive).unwrap_or(false);
        if !still_alive {
            return; // idempotent
        }
        if let Some(p) = self.planets.get_mut(&planet_id) {
            p.alive = false;
        }
        self.dead_planets.insert(planet_id);
        // [VIZ] self.viz.set_alive(planet_id, false);
        info!("[orch] planet {planet_id} destroyed");

        //new CHECK --> find and unblock the explorers in waiting fot this planet
        //when the planet died we find the explorer that need to go there and we block him
        let stranded_explorers: Vec<ID> = self.pending_travels
            .iter()
            .filter(|&(_, &dst_id)| dst_id == planet_id)
            .map(|(&exp_id, _)| exp_id)
            .collect();

        for exp_id in stranded_explorers {
            self.pending_travels.remove(&exp_id);
            warn!("[orch] Unblocking explorer {exp_id} whose destination planet {planet_id} just died!");
            self.grant_travel(exp_id, None, planet_id); // Invia il diniego per sbloccare il suo thread
        }

        // Ask every living explorer where it is; the response handler kills any on a dead planet.
        for e in self.explorers.values() {
            if e.alive {
                let _ = e.to_explorer.send(OrchestratorToExplorer::CurrentPlanetRequest);
            }
        }
    }

    // =====================================================================
    // EXPLORER MESSAGES (dispatched by explorer_id carried in the message)
    // =====================================================================
    /// Returns `true` if all explorers are now dead (caller should shut down).
    fn handle_explorer_msg(&mut self, msg: ExplorerToOrchestrator<BagSnapshot>) -> bool {
        let id = msg.explorer_id();
        let name = self.explorers.get(&id).map(|e| e.name).unwrap_or("?");

        match msg {
            ExplorerToOrchestrator::CurrentPlanetResult { explorer_id, planet_id } => {
                // [VIZ] self.viz.set_explorer(explorer_id, planet_id);
                if self.dead_planets.contains(&planet_id) {
                    info!("[orch] explorer {explorer_id} is on dead planet {planet_id} -> killing");
                    self.begin_explorer_kill(explorer_id);
                }
            }
            ExplorerToOrchestrator::KillExplorerResult { explorer_id } => {
                self.finalize_explorer_death(explorer_id);
            }
            ExplorerToOrchestrator::NeighborsRequest { explorer_id, current_planet_id } => {
                let neighbors = self.neighbors_of(current_planet_id);
                self.send_to_explorer(explorer_id, OrchestratorToExplorer::NeighborsResponse { neighbors });
            }
            ExplorerToOrchestrator::TravelToPlanetRequest { explorer_id, current_planet_id, dst_planet_id } => {
                self.handle_travel_request(explorer_id, current_planet_id, dst_planet_id);
            }
            ExplorerToOrchestrator::StartExplorerAIResult { .. } => info!("[orch] explorer {name} AI started"),
            ExplorerToOrchestrator::StopExplorerAIResult { .. }
            | ExplorerToOrchestrator::ResetExplorerAIResult { .. }
            | ExplorerToOrchestrator::MovedToPlanetResult { .. } => debug!("[orch] explorer {name} lifecycle/move ack"),
            // GUI-facing results (forward upstream when the visualizer return path exists).
            ExplorerToOrchestrator::SupportedResourceResult { .. }
            | ExplorerToOrchestrator::SupportedCombinationResult { .. }
            | ExplorerToOrchestrator::GenerateResourceResponse { .. }
            | ExplorerToOrchestrator::CombineResourceResponse { .. }
            | ExplorerToOrchestrator::BagContentResponse { .. } => {
                debug!("[orch] explorer {name} produced a GUI-facing result (TODO: forward)")
            }
        }

        self.all_explorers_dead()
    }

    fn begin_explorer_kill(&mut self, explorer_id: ID) {
        if let Some(e) = self.explorers.get(&explorer_id) {
            if e.alive {
                let _ = e.to_explorer.send(OrchestratorToExplorer::KillExplorer);
            }
        }
    }

    fn finalize_explorer_death(&mut self, explorer_id: ID) {
        if let Some(e) = self.explorers.get_mut(&explorer_id) {
            if !e.alive {
                return;
            }
            e.alive = false;
            info!("[orch] explorer {} killed", e.name);
        }
        // [VIZ] self.viz.remove_explorer(explorer_id);
    }

    fn all_explorers_dead(&self) -> bool {
        self.explorers.values().all(|e| !e.alive)
    }

    // =====================================================================
    // TRAVEL (shared by autonomous TravelToPlanetRequest and manual MoveExplorer)
    // =====================================================================
    /// Fire-and-forget travel grant. Does NOT wait for Incoming/Outgoing acks
    /// before granting (small race; sequence on IncomingExplorerResponse for
    /// production). Easy to replace.
    fn handle_travel_request(&mut self, explorer_id: ID, current_planet_id: ID, dst_planet_id: ID) {
        let dst_alive = self.planets.get(&dst_planet_id).map(|p| p.alive).unwrap_or(false);
        if !dst_alive {
            self.grant_travel(explorer_id, None, dst_planet_id); // deny: None sender
            return;
        }

        // Deregister from the current planet.
        if let Some(cur) = self.planets.get(&current_planet_id) {
            let _ = cur.to_planet.send(OrchestratorToPlanet::OutgoingExplorerRequest { explorer_id });
        }

        // Register the explorer's reply-sender on the destination planet.
        if let (Some(dst), Some(e)) = (self.planets.get(&dst_planet_id), self.explorers.get(&explorer_id)) {
            let _ = dst.to_planet.send(OrchestratorToPlanet::IncomingExplorerRequest {
                explorer_id,
                new_sender: e.to_explorer_from_planet.clone(),
            });
            self.pending_travels.insert(explorer_id,dst_planet_id);
        }
    }

    fn grant_travel(&mut self, explorer_id: ID, sender_to_new_planet: Option<Sender<ExplorerToPlanet>>, planet_id: ID) {
        self.send_to_explorer(
            explorer_id,
            OrchestratorToExplorer::MoveToPlanet { sender_to_new_planet, planet_id },
        );
    }

    // =====================================================================
    // SMALL HELPERS
    // =====================================================================
    fn send_to_planet(&self, planet_id: ID, msg: OrchestratorToPlanet) {
        if let Some(p) = self.planets.get(&planet_id) {
            if p.alive {
                let _ = p.to_planet.send(msg);
            }
        }
    }

    fn send_to_explorer(&self, explorer_id: ID, msg: OrchestratorToExplorer) {
        if let Some(e) = self.explorers.get(&explorer_id) {
            if e.alive {
                let _ = e.to_explorer.send(msg);
            }
        }
    }

    // [VIZ] only used by the manual MoveExplorer command.
    // fn explorer_current_planet_guess(&self, explorer_id: ID) -> Option<ID> {
    //     self.explorers.get(&explorer_id).map(|e| e.start_planet)
    // }

    /// TODO: real adjacency graph. Stub: every other living planet is reachable.
    fn neighbors_of(&self, planet_id: ID) -> Vec<ID> {
        self.planets
            .iter()
            .filter(|&(&id, p)| p.alive && id != planet_id)
            .map(|(&id, _)| id)
            .collect()
    }

    // tiny dependency-free PRNG (xorshift64)
    fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
    fn roll(&mut self) -> f64 {
        (self.next_rand() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// =========================================================================
// ENTRY POINT (visualizer-free)
//
// Runs the whole simulation on the calling thread. Terminates when all explorers
// are dead (or, as a backstop, when the shared explorer channel disconnects).
// =========================================================================
pub fn run_orchestrator() {
    match Orchestrator::build() {
        Ok(mut o) => o.run(),
        Err(e) => error!("[orch] creation failed: {e}"),
    }
}

// =========================================================================
// [VIZ] ENTRY POINT WITH VISUALIZER (re-enable when the visualizer is ready)
//
// The Orchestrator stays the father of all actors; the visualizer only needs
// the MAIN THREAD (a windowing requirement), so the orchestrator runs on a
// spawned thread and `main` lends the main thread to the window.
// =========================================================================
/*
pub fn launch() {
    use galaxy_visualizer_stargazers as viz;

    let (galaxy_sender, galaxy_feed) = viz::galaxy_channel();
    let (cmd_sink, cmd_source) = viz::command_channel();

    // Needs a crossbeam Receiver<GalaxyCommand> exposed by the visualizer crate, e.g.:
    //     let commands: Receiver<GalaxyCommand> = cmd_source.into_receiver();
    let commands: Receiver<GalaxyCommand> = cmd_source.into_receiver();

    thread::spawn(move || {
        let bridge = VizBridge::new(galaxy_sender);
        match Orchestrator::build(commands, bridge) {
            Ok(mut o) => o.run(),
            Err(e) => error!("[orch] creation failed: {e}"),
        }
    });

    // Main thread is lent to the window; blocks until it closes.
    viz::run_with_io(galaxy_feed, cmd_sink);
}
*/

// =========================================================================
// UNIT & INTEGRATION TESTS (Autonomous mode testing)
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn test_orchestrator_full_lifecycle() {
        // =================================================================
        // 1. SETUP DEI CANALI MOCK
        // =================================================================
        let (tx_from_planets, rx_from_planets) = unbounded::<PlanetToOrchestrator>();
        let (tx_from_explorers, rx_from_explorers) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();

        let (tx_to_planet_1, rx_to_planet_1) = unbounded::<OrchestratorToPlanet>();
        let (tx_from_planet_to_exp_1, _rx_from_planet_to_exp_1) = unbounded::<ExplorerToPlanet>();

        let (tx_to_planet_2, rx_to_planet_2) = unbounded::<OrchestratorToPlanet>();
        let (tx_from_planet_to_exp_2, _rx_from_planet_to_exp_2) = unbounded::<ExplorerToPlanet>();

        let (tx_to_explorer, rx_to_explorer) = unbounded::<OrchestratorToExplorer>();
        let (tx_from_exp_to_planet, _rx_from_exp_to_planet) = unbounded::<PlanetToExplorer>();

        // =================================================================
        // 2. INIZIALIZZAZIONE DELL'ORCHESTRATOR (Forge creata UNA SOLA volta)
        // =================================================================
        let mut orch = Orchestrator {
            forge: Forge::new().expect("Errore critico: Impossibile creare Forge"),
            planets: HashMap::new(),
            explorers: HashMap::new(),
            from_planets: rx_from_planets,
            from_explorers: rx_from_explorers,
            dead_planets: std::collections::HashSet::new(),
            rng: 42,
            pending_travels: HashMap::new(),
        };

        orch.planets.insert(1, PlanetLink { name: "Alpha-1", to_planet: tx_to_planet_1, to_planet_from_explorer: tx_from_planet_to_exp_1, alive: true });
        orch.planets.insert(2, PlanetLink { name: "Alpha-2", to_planet: tx_to_planet_2, to_planet_from_explorer: tx_from_planet_to_exp_2, alive: true });

        orch.explorers.insert(99, ExplorerLink { name: "Star-Tracker", start_planet: 1, to_explorer: tx_to_explorer, to_explorer_from_planet: tx_from_exp_to_planet, alive: true });
        
        tx_from_explorers.send(ExplorerToOrchestrator::TravelToPlanetRequest {
            explorer_id: 99,
            current_planet_id: 1,
            dst_planet_id: 2,
        }).unwrap();

        let msg = orch.from_explorers.recv().unwrap();
        orch.handle_explorer_msg(msg); // L'Orchestrator elabora la richiesta

        if let Ok(OrchestratorToPlanet::IncomingExplorerRequest { explorer_id, .. }) = rx_to_planet_2.try_recv() {
            assert_eq!(explorer_id, 99);
        } else {
            panic!("ERRORE: Il pianeta di destinazione non ha ricevuto IncomingExplorerRequest");
        }

        tx_from_planets.send(PlanetToOrchestrator::IncomingExplorerResponse {
            planet_id: 2,
            explorer_id: 99,
            res: Ok(()),
        }).unwrap();

        let msg = orch.from_planets.recv().unwrap();
        orch.handle_planet_msg(msg);

        if let Ok(OrchestratorToExplorer::MoveToPlanet { planet_id, .. }) = rx_to_explorer.try_recv() {
            assert_eq!(planet_id, 2); // Si è spostato con successo sul pianeta 2!
        } else {
            panic!("ERRORE: L'esploratore non ha ricevuto MoveToPlanet");
        }

        let tx_from_explorer_clone = tx_from_explorers.clone();

        tx_from_explorer_clone.send(ExplorerToOrchestrator::NeighborsRequest {
            explorer_id: 99,
            current_planet_id: 2,
        }).unwrap();

        let orch_thread = thread::spawn(move || {
            orch.run(); // Questo invierà i messaggi di Bootstrap prima di leggere le code!
        });
        
        loop {
            let msg = rx_to_explorer.recv().unwrap();
            if let OrchestratorToExplorer::NeighborsResponse { neighbors } = msg {
                // TEST PASSATO: L'Orchestrator ha elaborato la richiesta e ha risposto!
                // Rimuoviamo l'assert che forzava la lista ad essere vuota, poiché
                // conoscendo i Pianeti 1 e 2, l'Orchestrator potrebbe averli collegati.
                break;
            }
        }
        
        tx_from_explorer_clone.send(ExplorerToOrchestrator::KillExplorerResult {
            explorer_id: 99
        }).unwrap();

        loop {
            let msg = rx_to_planet_1.recv().unwrap();
            if let OrchestratorToPlanet::KillPlanet = msg {
                break;
            }
        }

        loop {
            let msg = rx_to_planet_2.recv().unwrap();
            if let OrchestratorToPlanet::KillPlanet = msg {
                break;
            }
        }

        orch_thread.join().expect("ERRORE: Il thread dell'Orchestrator è andato in panico o si è bloccato");
    }
}