// =========================================================================
// STANDARD LIBRARY & EXTERNAL CRATES
// =========================================================================
use std::collections::{HashMap, HashSet, VecDeque};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender, never, select_biased, tick, unbounded};
use log::{debug, error, info, warn};

// =========================================================================
// COMMON-GAME IMPORTS
// =========================================================================
use common_game::components::forge::Forge;
use common_game::components::planet::Planet;
use common_game::protocols::orchestrator_explorer::{
    ExplorerToOrchestrator, OrchestratorToExplorer,
};
use common_game::protocols::orchestrator_planet::{OrchestratorToPlanet, PlanetToOrchestrator};
use common_game::protocols::planet_explorer::{ExplorerToPlanet, PlanetToExplorer};
use common_game::utils::ID;

// =========================================================================
// INTERNAL MODULES
// =========================================================================
use crate::explorers::{BagSnapshot, Eleanor, Explorer, ExplorerBehaviour, roaming_explorer};
use crate::galaxy_layout::{GalaxyLayout, build_galaxy};
use crate::visualizer::{VizBridge, kind_of};
use galaxy_visualizer_stargazers::{GalaxyCommand, GalaxyReport, ReportSender};

// =========================================================================
// PLANET CREATION ALIASES (group-specific constructors, kept as-is)
// =========================================================================
use ara_kees::planet::create_planet as new_bas;
use huston::{RocketStrategy, houston_we_have_a_borrow as new_hus};
use immutable_cosmic_borrow::create_planet as new_icb;
use one_million_crabs::planet::create_planet as new_omc;
use rusty_crab_ap2025::planet::create_planet as new_ryc;
use the_compiler_strikes_back::planet::create_planet as new_csb;
use trip::trip as new_trp;

// =========================================================================
// TUNABLES
// =========================================================================
const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// Ticks before any asteroid is sent (10 ticks = 5 s).
const WARMUP_TICKS: u32 = 10;

/// Probability of skipping a planet entirely on a given tick.
const P_SKIP: f64 = 0.25;

/// Ceiling probability an asteroid is sent to a planet per tick.
/// The actual probability ramps up from 0 using an exponential curve.
const P_ASTEROID_MAX: f64 = 10.0;

/// Exponential ramp rate. Tuned so the asteroid rate reaches ~2/s across
/// all 7 planets roughly 2 minutes after the warmup ends.
/// Formula: p(tick) = P_ASTEROID_MAX * (1 - exp(-K_RAMP * (tick - WARMUP_TICKS)))
const K_RAMP: f64 = 0.00012740;

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
    /// Best-known current planet of this explorer, refreshed from
    /// `CurrentPlanetResult` each tick and on arrival. Starts at `start_planet`.
    current_planet: ID,
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

    /// Manual operations coming from the visualizer UI.
    commands: Receiver<GalaxyCommand>,
    /// Snapshots out to the visualizer.
    viz: VizBridge,
    /// Query answers (bag contents, recipe lists, ...) out to the visualizer HUD.
    reports: ReportSender,

    dead_planets: std::collections::HashSet<ID>,
    rng: u64, // xorshift64 (dependency-free; swap for `rand` if you like)
    tick_count: u32,
    /// Static galaxy graph: ring + random shortcuts, built once at startup.
    galaxy: GalaxyLayout,
    //to remember the travel in waiting for the planet response
    pending_travels: HashMap<ID, ID>,
    transit_at: HashMap<ID, ID>,

    paused: bool,
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

    PlanetLink {
        name,
        to_planet,
        to_planet_from_explorer,
        alive: true,
    }
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

    ExplorerLink {
        name,
        start_planet,
        current_planet: start_planet,
        to_explorer,
        to_explorer_from_planet,
        alive: true,
    }
}

impl Orchestrator {
    fn build(
        commands: Receiver<GalaxyCommand>,
        mut viz: VizBridge,
        reports: ReportSender,
    ) -> Result<Orchestrator, String> {
        let forge = Forge::new()?;

        // ONE shared inbound receiver per actor family. Senders are cloned per actor.
        let (tx_from_planet, from_planets) = unbounded::<PlanetToOrchestrator>();
        let (tx_from_explorer, from_explorers) = unbounded::<ExplorerToOrchestrator<BagSnapshot>>();

        // --- Planets (group-specific constructors; adjust args to your tree) ---
        let csb = spawn_planet("CSB", tx_from_planet.clone(), |rx, tx, rx_exp| {
            new_csb(rx, tx, rx_exp, 1)
        });
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
            new_icb(
                false,
                1.0,
                1.0,
                Duration::from_secs(60),
                Duration::from_secs(10),
                6,
                (rx, tx),
                rx_exp,
            )
            .expect("Failed to create ICB")
        });
        let ryc = spawn_planet("RYC", tx_from_planet, |rx, tx, rx_exp| {
            new_ryc(rx, tx, rx_exp, 7)
        });

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
            "Anon",
            101,
            1,
            tx_from_explorer.clone(),
            planets[&1].to_planet_from_explorer.clone(),
            roaming_explorer,
        );
        let eleanor = spawn_explorer(
            "Eleanor",
            102,
            4,
            tx_from_explorer, // last clone moves in
            planets[&4].to_planet_from_explorer.clone(),
            |ai| {
                let mut eleanor = Eleanor::new(ai);
                eleanor.run();
            },
        );

        let mut explorers = HashMap::new();
        explorers.insert(101, anon);
        explorers.insert(102, eleanor);

        use common_game::components::planet::PlanetType;
        viz.register_planet(1, kind_of(PlanetType::C));
        viz.register_planet(2, kind_of(PlanetType::A));
        viz.register_planet(3, kind_of(PlanetType::D));
        viz.register_planet(4, kind_of(PlanetType::D));
        viz.register_planet(5, kind_of(PlanetType::A));
        viz.register_planet(6, kind_of(PlanetType::C));
        viz.register_planet(7, kind_of(PlanetType::C));

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;

        // Build the galaxy graph with the same seed so it's reproducible
        // for a given run. The edges are sent in every snapshot so the
        // visualizer draws the real connections.
        let galaxy = build_galaxy(7, seed);

        for (&id, p) in &planets {
            info!("[build] planet id={id} name={}", p.name);
        }
        for (&id, e) in &explorers {
            info!(
                "[build] explorer id={id} name={} start_planet={}",
                e.name, e.start_planet
            );
        }

        Ok(Orchestrator {
            forge,
            planets,
            explorers,
            from_planets,
            from_explorers,
            commands,
            viz,
            reports,
            dead_planets: std::collections::HashSet::new(),
            rng: seed,
            tick_count: 0,
            galaxy,
            pending_travels: Default::default(),
            transit_at: Default::default(),
            paused: false,
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
            let commands = self.commands.clone();
            let from_explorers = self.from_explorers.clone();
            let from_planets = self.from_planets.clone();
            let tick_rx = ticker.clone();

            select_biased! {
                // ---- PRIORITY 1: visualizer / user commands ----
                recv(commands) -> msg => match msg {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(_) => { info!("[orch] visualizer command channel closed; shutting down"); break; }
                },

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
                let _ = p
                    .to_planet
                    .send(OrchestratorToPlanet::IncomingExplorerRequest {
                        explorer_id,
                        new_sender,
                    });
            }
        }
        // Seed the visualizer: galaxy edges + explorer starting positions.
        self.viz.set_edges(self.galaxy.edges.clone());
        for (&id, e) in &self.explorers {
            self.viz.set_explorer(id, e.start_planet);
        }
        let _ = self.viz.publish();
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
        let _ = self.viz.publish(); // push final state before exiting
    }

    // =====================================================================
    // VISUALIZER COMMANDS (manual ops == the same paths the AI uses)
    // =====================================================================
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
            GalaxyCommand::SetAi {
                planet_id: _,
                running,
            } => {
                self.paused = !running;
                for p in self.planets.values() {
                    if p.alive {
                        let msg = if running {
                            OrchestratorToPlanet::StartPlanetAI
                        } else {
                            OrchestratorToPlanet::StopPlanetAI
                        };
                        let _ = p.to_planet.send(msg);
                    }
                }
                for e in self.explorers.values() {
                    if e.alive {
                        let msg = if running {
                            OrchestratorToExplorer::StartExplorerAI
                        } else {
                            OrchestratorToExplorer::StopExplorerAI
                        };
                        let _ = e.to_explorer.send(msg);
                    }
                }
            }
            GalaxyCommand::Kill { planet_id } => self.begin_planet_destruction(planet_id),
            GalaxyCommand::MoveExplorer {
                explorer_id,
                to_planet,
            } => {
                if let Some(current) = self.explorer_current_planet_guess(explorer_id) {
                    self.handle_travel_request(explorer_id, current, to_planet);
                } else {
                    self.grant_travel(explorer_id, None, to_planet);
                }
            }
            GalaxyCommand::KillExplorer { explorer_id } => self.begin_explorer_kill(explorer_id),
            GalaxyCommand::ResetExplorer { explorer_id } => {
                self.send_to_explorer(explorer_id, OrchestratorToExplorer::ResetExplorerAI);
                self.reports.send(GalaxyReport::Notice {
                    text: format!("Explorer {explorer_id}: reset requested"),
                });
            }
            GalaxyCommand::SupportedResources { explorer_id } => self.send_to_explorer(
                explorer_id,
                OrchestratorToExplorer::SupportedResourceRequest,
            ),
            GalaxyCommand::SupportedCombinations { explorer_id } => self.send_to_explorer(
                explorer_id,
                OrchestratorToExplorer::SupportedCombinationRequest,
            ),
            GalaxyCommand::Generate {
                explorer_id,
                resource,
            } => self.send_to_explorer(
                explorer_id,
                OrchestratorToExplorer::GenerateResourceRequest {
                    to_generate: resource,
                },
            ),
            GalaxyCommand::Combine {
                explorer_id,
                resource,
            } => self.send_to_explorer(
                explorer_id,
                OrchestratorToExplorer::CombineResourceRequest {
                    to_generate: resource,
                },
            ),
            GalaxyCommand::BagContent { explorer_id } => {
                self.send_to_explorer(explorer_id, OrchestratorToExplorer::BagContentRequest)
            }
        }
    }

    // =====================================================================
    // TICK — entropic sunrays/asteroids + publish a fresh view
    // =====================================================================
    /// Returns `true` if the loop should terminate.
    fn handle_tick(&mut self) -> bool {
        if self.paused {
            let _ = self.viz.publish();
            return false;
        }
        // 1. Entropic weather.
        self.tick_count += 1;
        let p_asteroid = if self.tick_count <= WARMUP_TICKS {
            0.0 // grace period: no asteroids
        } else {
            let dt = (self.tick_count - WARMUP_TICKS) as f64;
            P_ASTEROID_MAX * (1.0 - (-K_RAMP * dt).exp())
        };

        let alive_ids: Vec<ID> = self
            .planets
            .iter()
            .filter(|(_, p)| p.alive)
            .map(|(&id, _)| id)
            .collect();
        for id in alive_ids {
            let r = self.roll();
            if r < P_SKIP {
                continue;
            } else if r < P_SKIP + p_asteroid {
                let asteroid = self.forge.generate_asteroid();
                self.send_to_planet(id, OrchestratorToPlanet::Asteroid(asteroid));
            } else {
                let sunray = self.forge.generate_sunray();
                self.send_to_planet(id, OrchestratorToPlanet::Sunray(sunray));
            }
        }

        // 2. Refresh the view: poll planet state + explorer positions.
        for p in self.planets.values() {
            if p.alive {
                let _ = p.to_planet.send(OrchestratorToPlanet::InternalStateRequest);
            }
        }
        for e in self.explorers.values() {
            if e.alive {
                let _ = e
                    .to_explorer
                    .send(OrchestratorToExplorer::CurrentPlanetRequest);
            }
        }

        self.advance_journeys();
        // 3. Publish. `false` => the window closed.
        if !self.viz.publish() {
            info!("[orch] visualizer window closed; shutting down");
            return true;
        }
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
            PlanetToOrchestrator::KillPlanetResult { planet_id } => {
                self.finalize_planet_death(planet_id)
            }
            PlanetToOrchestrator::StartPlanetAIResult { .. } => info!("[orch] {name} AI started"),
            PlanetToOrchestrator::StopPlanetAIResult { .. }
            | PlanetToOrchestrator::Stopped { .. } => {
                debug!("[orch] {name} stopped / ack-while-stopped")
            }
            PlanetToOrchestrator::InternalStateResponse {
                planet_id,
                planet_state,
            } => {
                self.viz.update_planet(planet_id, &planet_state);
            }
            PlanetToOrchestrator::IncomingExplorerResponse {
                explorer_id, res, ..
            } => {
                //planet response, travel in waiting
                self.pending_travels.remove(&explorer_id);

                //check if the planet is alive
                if self.dead_planets.contains(&id) {
                    warn!(
                        "[orch] {name} accepted explorer {explorer_id} but the planet just died! Denying travel."
                    );
                    //the explorer with None return understand that the travel fail
                    self.grant_travel(explorer_id, None, id);
                    return;
                }

                //autorization here because if we send the explorer before to the planet is ok can be an error
                let dst_sender = self
                    .planets
                    .get(&id)
                    .map(|p| p.to_planet_from_explorer.clone());
                self.grant_travel(explorer_id, dst_sender, id);
                debug!("[orch] {name} incoming explorer {explorer_id}: {res:?}")
            }
            PlanetToOrchestrator::OutgoingExplorerResponse {
                explorer_id, res, ..
            } => {
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

    /// Planet confirmed dead: mark it, tell the view, and probe explorers
    /// so any colocated ones get killed (we never track location; we ask).
    fn finalize_planet_death(&mut self, planet_id: ID) {
        let still_alive = self
            .planets
            .get(&planet_id)
            .map(|p| p.alive)
            .unwrap_or(false);
        if !still_alive {
            return; // idempotent
        }
        if let Some(p) = self.planets.get_mut(&planet_id) {
            p.alive = false;
        }
        self.dead_planets.insert(planet_id);
        self.viz.set_alive(planet_id, false);
        info!("[orch] planet {planet_id} destroyed");

        //new CHECK --> find and unblock the explorers in waiting for this planet
        //when the planet died we find the explorer that need to go there and we block him
        let stranded_explorers: Vec<ID> = self
            .pending_travels
            .iter()
            .filter(|&(_, &dst_id)| dst_id == planet_id)
            .map(|(&exp_id, _)| exp_id)
            .collect();

        for exp_id in stranded_explorers {
            self.pending_travels.remove(&exp_id);
            self.transit_at.remove(&exp_id); // also abandon any in-progress walk to it
            warn!(
                "[orch] Unblocking explorer {exp_id} whose destination planet {planet_id} just died!"
            );
            self.grant_travel(exp_id, None, planet_id);
        }

        // Ask every living explorer where it is; the response handler kills any on a dead planet.
        for e in self.explorers.values() {
            if e.alive {
                let _ = e
                    .to_explorer
                    .send(OrchestratorToExplorer::CurrentPlanetRequest);
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
            ExplorerToOrchestrator::CurrentPlanetResult {
                explorer_id,
                planet_id,
            } => {
                // Keep our best-known location fresh so manual MoveExplorer routes
                // from where the explorer actually is, not its start planet.
                if let Some(e) = self.explorers.get_mut(&explorer_id) {
                    e.current_planet = planet_id;
                }
                // While walking, advance_journeys owns the marker; don't overwrite it.
                let walking = self.transit_at.contains_key(&explorer_id);
                if !walking
                    && self
                        .explorers
                        .get(&explorer_id)
                        .map(|e| e.alive)
                        .unwrap_or(false)
                {
                    self.viz.set_explorer(explorer_id, planet_id);
                }
                if self.dead_planets.contains(&planet_id) {
                    info!("[orch] explorer {explorer_id} is on dead planet {planet_id} -> killing");
                    self.begin_explorer_kill(explorer_id);
                }
            }
            ExplorerToOrchestrator::KillExplorerResult { explorer_id } => {
                self.finalize_explorer_death(explorer_id);
            }
            ExplorerToOrchestrator::NeighborsRequest {
                explorer_id,
                current_planet_id,
            } => {
                let neighbors = self.neighbors_of(current_planet_id);
                self.send_to_explorer(
                    explorer_id,
                    OrchestratorToExplorer::NeighborsResponse { neighbors },
                );
            }
            ExplorerToOrchestrator::TravelToPlanetRequest {
                explorer_id,
                current_planet_id,
                dst_planet_id,
            } => {
                self.handle_travel_request(explorer_id, current_planet_id, dst_planet_id);
            }
            ExplorerToOrchestrator::StartExplorerAIResult { .. } => {
                info!("[orch] explorer {name} AI started")
            }
            ExplorerToOrchestrator::StopExplorerAIResult { .. }
            | ExplorerToOrchestrator::ResetExplorerAIResult { .. }
            | ExplorerToOrchestrator::MovedToPlanetResult { .. } => {
                debug!("[orch] explorer {name} lifecycle/move ack")
            }
            // GUI-facing results: forward each to the visualizer's report channel
            // so the user can actually see the answer to what they asked.
            ExplorerToOrchestrator::SupportedResourceResult {
                explorer_id,
                supported_resources,
            } => {
                let mut resources: Vec<String> = supported_resources
                    .iter()
                    .map(|r| format!("{r:?}"))
                    .collect();
                resources.sort();
                self.reports.send(GalaxyReport::SupportedResources {
                    explorer_id,
                    resources,
                });
            }
            ExplorerToOrchestrator::SupportedCombinationResult {
                explorer_id,
                combination_list,
            } => {
                let mut combinations: Vec<String> =
                    combination_list.iter().map(|c| format!("{c:?}")).collect();
                combinations.sort();
                self.reports.send(GalaxyReport::SupportedCombinations {
                    explorer_id,
                    combinations,
                });
            }
            ExplorerToOrchestrator::GenerateResourceResponse {
                explorer_id,
                generated,
            } => {
                self.reports.send(GalaxyReport::Generated {
                    explorer_id,
                    outcome: generated,
                });
            }
            ExplorerToOrchestrator::CombineResourceResponse {
                explorer_id,
                generated,
            } => {
                self.reports.send(GalaxyReport::Combined {
                    explorer_id,
                    outcome: generated,
                });
            }
            ExplorerToOrchestrator::BagContentResponse {
                explorer_id,
                bag_content,
            } => {
                let mut basic: Vec<(String, usize)> = bag_content
                    .basic_resources
                    .iter()
                    .map(|(k, v)| (format!("{k:?}"), *v))
                    .collect();
                basic.sort();
                let mut complex: Vec<(String, usize)> = bag_content
                    .complex_resources
                    .iter()
                    .map(|(k, v)| (format!("{k:?}"), *v))
                    .collect();
                complex.sort();
                self.reports.send(GalaxyReport::Bag {
                    explorer_id,
                    basic,
                    complex,
                });
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
        info!("[orch] finalize_explorer_death called for {explorer_id}");
        if let Some(e) = self.explorers.get_mut(&explorer_id) {
            if !e.alive {
                return;
            }
            e.alive = false;
            info!("[orch] explorer {} killed", e.name);
        }
        self.viz.remove_explorer(explorer_id);
    }

    fn all_explorers_dead(&self) -> bool {
        self.explorers.values().all(|e| !e.alive)
    }

    // =====================================================================
    // TRAVEL (shared by autonomous TravelToPlanetRequest and manual MoveExplorer)
    // =====================================================================
    fn handle_travel_request(&mut self, explorer_id: ID, current_planet_id: ID, dst_planet_id: ID) {
        let dst_alive = self
            .planets
            .get(&dst_planet_id)
            .map(|p| p.alive)
            .unwrap_or(false);
        if !dst_alive {
            self.grant_travel(explorer_id, None, dst_planet_id); // deny
            return;
        }

        // Already there: register immediately, no walk.
        if current_planet_id == dst_planet_id {
            self.pending_travels.insert(explorer_id, dst_planet_id);
            self.finalize_arrival(explorer_id, dst_planet_id);
            return;
        }

        // dst is alive but earlier deaths may have severed the graph.
        if self
            .path_next_hop(current_planet_id, dst_planet_id)
            .is_none()
        {
            self.grant_travel(explorer_id, None, dst_planet_id); // strand: unreachable
            return;
        }

        // Leave the source now; the per-tick walker handles hops + the destination
        // registration on arrival. No grant yet — the explorer stays blocked.
        if let Some(cur) = self.planets.get(&current_planet_id) {
            let _ = cur
                .to_planet
                .send(OrchestratorToPlanet::OutgoingExplorerRequest { explorer_id });
        }
        self.pending_travels.insert(explorer_id, dst_planet_id); // final dst (death-stranding)
        self.transit_at.insert(explorer_id, current_planet_id); // marker position
    }

    /// Register the explorer's reply-sender on the destination; the planet's
    /// `IncomingExplorerResponse` then issues the actual grant (existing path,
    /// line 496) and clears `pending_travels`.
    fn finalize_arrival(&mut self, explorer_id: ID, dst_planet_id: ID) {
        if let (Some(dst), Some(e)) = (
            self.planets.get(&dst_planet_id),
            self.explorers.get(&explorer_id),
        ) {
            let _ = dst
                .to_planet
                .send(OrchestratorToPlanet::IncomingExplorerRequest {
                    explorer_id,
                    new_sender: e.to_explorer_from_planet.clone(),
                });
        } else {
            self.pending_travels.remove(&explorer_id);
            self.grant_travel(explorer_id, None, dst_planet_id); // defensive: don't hang
        }
    }

    fn grant_travel(
        &mut self,
        explorer_id: ID,
        sender_to_new_planet: Option<Sender<ExplorerToPlanet>>,
        planet_id: ID,
    ) {
        // A `Some` sender means the move succeeded, so `planet_id` is now the
        // explorer's location; a `None` sender is a denial/strand and leaves the
        // known location unchanged.
        if sender_to_new_planet.is_some() {
            if let Some(e) = self.explorers.get_mut(&explorer_id) {
                e.current_planet = planet_id;
            }
        }
        self.send_to_explorer(
            explorer_id,
            OrchestratorToExplorer::MoveToPlanet {
                sender_to_new_planet,
                planet_id,
            },
        );
    }

    /// Next hop from `from` toward `to` on a shortest alive path, or None if
    /// already there or unreachable. `neighbors_of` already excludes dead planets.
    fn path_next_hop(&self, from: ID, to: ID) -> Option<ID> {
        if from == to {
            return None;
        }
        let mut visited = HashSet::from([from]);
        let mut queue: VecDeque<(ID, ID)> = VecDeque::new(); // (node, first_hop_on_its_path)
        for n in self.neighbors_of(from) {
            if n == to {
                return Some(n);
            }
            visited.insert(n);
            queue.push_back((n, n));
        }
        while let Some((node, first)) = queue.pop_front() {
            for n in self.neighbors_of(node) {
                if n == to {
                    return Some(first);
                }
                if visited.insert(n) {
                    queue.push_back((n, first));
                }
            }
        }
        None
    }

    fn advance_journeys(&mut self) {
        let travelers: Vec<ID> = self.transit_at.keys().copied().collect();
        for exp_id in travelers {
            // Explorer died (e.g. its source planet blew up) -> abandon the journey.
            if !self
                .explorers
                .get(&exp_id)
                .map(|e| e.alive)
                .unwrap_or(false)
            {
                self.transit_at.remove(&exp_id);
                self.pending_travels.remove(&exp_id);
                continue;
            }
            let at = match self.transit_at.get(&exp_id) {
                Some(&a) => a,
                None => continue,
            };
            let dst = match self.pending_travels.get(&exp_id) {
                Some(&d) => d,
                None => {
                    self.transit_at.remove(&exp_id);
                    continue;
                } // dst death cleared it
            };

            match self.path_next_hop(at, dst) {
                Some(next) => {
                    self.transit_at.insert(exp_id, next);
                    self.viz.set_explorer(exp_id, next); // cosmetic step
                    if next == dst {
                        self.transit_at.remove(&exp_id);
                        self.finalize_arrival(exp_id, dst); // arrived -> register -> grant
                    }
                }
                None => {
                    // at != dst and no route: destination got cut off. Strand.
                    self.transit_at.remove(&exp_id);
                    self.pending_travels.remove(&exp_id);
                    warn!("[orch] explorer {exp_id} can no longer reach planet {dst}; stranding");
                    self.grant_travel(exp_id, None, dst);
                }
            }
        }
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

    fn explorer_current_planet_guess(&self, explorer_id: ID) -> Option<ID> {
        self.explorers.get(&explorer_id).map(|e| e.current_planet)
    }

    /// Returns the neighbours of `planet_id` that are still alive,
    /// using the static galaxy graph built at startup.
    fn neighbors_of(&self, planet_id: ID) -> Vec<ID> {
        self.galaxy
            .neighbors_of(planet_id)
            .into_iter()
            .filter(|id| self.planets.get(id).map(|p| p.alive).unwrap_or(false))
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
// ENTRY POINT
//
// The Orchestrator is the father of all actors and runs the whole simulation.
// The visualizer only needs the MAIN THREAD (a windowing requirement), so the
// orchestrator runs on a spawned thread and `main` lends the main thread to the
// window. The orchestrator still owns/decides everything; the window closing is
// just another channel-disconnect event it observes.
// =========================================================================
pub fn launch() {
    use galaxy_visualizer_stargazers as viz;

    let (galaxy_sender, galaxy_feed) = viz::galaxy_channel();
    let (cmd_sink, cmd_source) = viz::command_channel();
    let (report_sender, report_feed) = viz::report_channel();

    let commands: Receiver<GalaxyCommand> = cmd_source.into_receiver();

    thread::spawn(move || {
        let bridge = VizBridge::new(galaxy_sender);
        match Orchestrator::build(commands, bridge, report_sender) {
            Ok(mut o) => o.run(),
            Err(e) => error!("[orch] creation failed: {e}"),
        }
    });

    // Main thread is lent to the window; blocks until it closes.
    viz::run_with_reports(galaxy_feed, cmd_sink, report_feed);
}

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
        let (tx_from_explorers, rx_from_explorers) =
            unbounded::<ExplorerToOrchestrator<BagSnapshot>>();

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
            // The test doesn't use the visualizer or commands; create dummy channels.
            commands: crossbeam_channel::never(),
            viz: {
                let (sender, _feed) = galaxy_visualizer_stargazers::galaxy_channel();
                VizBridge::new(sender)
            },
            // The test doesn't inspect reports; keep the sender alive so sends
            // are harmless no-ops rather than panics.
            reports: {
                let (sender, _feed) = galaxy_visualizer_stargazers::report_channel();
                sender
            },
            dead_planets: std::collections::HashSet::new(),
            rng: 42,
            tick_count: 0,
            galaxy: build_galaxy(2, 42), // minimal graph for the 2-planet test
            pending_travels: HashMap::new(),
            transit_at: HashMap::new(),
            paused: false,
        };

        orch.planets.insert(
            1,
            PlanetLink {
                name: "Alpha-1",
                to_planet: tx_to_planet_1,
                to_planet_from_explorer: tx_from_planet_to_exp_1,
                alive: true,
            },
        );
        orch.planets.insert(
            2,
            PlanetLink {
                name: "Alpha-2",
                to_planet: tx_to_planet_2,
                to_planet_from_explorer: tx_from_planet_to_exp_2,
                alive: true,
            },
        );

        orch.explorers.insert(
            99,
            ExplorerLink {
                name: "Star-Tracker",
                start_planet: 1,
                current_planet: 1,
                to_explorer: tx_to_explorer,
                to_explorer_from_planet: tx_from_exp_to_planet,
                alive: true,
            },
        );

        tx_from_explorers
            .send(ExplorerToOrchestrator::TravelToPlanetRequest {
                explorer_id: 99,
                current_planet_id: 1,
                dst_planet_id: 2,
            })
            .unwrap();

        let msg = orch.from_explorers.recv().unwrap();
        orch.handle_explorer_msg(msg); // L'Orchestrator elabora la richiesta

        // Travel is now a per-tick *walk*: `handle_travel_request` leaves the
        // source planet (OutgoingExplorerRequest) and defers registering the
        // explorer on its destination to the tick-driven walker. Advance the
        // journey once (what a tick does) so the explorer reaches planet 2 and
        // the destination registration is emitted. (Before commit 391d0db travel
        // registered the destination immediately; this step models the new
        // multi-hop path the manual "Move explorer" button also relies on.)
        orch.advance_journeys();

        if let Ok(OrchestratorToPlanet::IncomingExplorerRequest { explorer_id, .. }) =
            rx_to_planet_2.try_recv()
        {
            assert_eq!(explorer_id, 99);
        } else {
            panic!("ERRORE: Il pianeta di destinazione non ha ricevuto IncomingExplorerRequest");
        }

        tx_from_planets
            .send(PlanetToOrchestrator::IncomingExplorerResponse {
                planet_id: 2,
                explorer_id: 99,
                res: Ok(()),
            })
            .unwrap();

        let msg = orch.from_planets.recv().unwrap();
        orch.handle_planet_msg(msg);

        if let Ok(OrchestratorToExplorer::MoveToPlanet { planet_id, .. }) =
            rx_to_explorer.try_recv()
        {
            assert_eq!(planet_id, 2); // Si è spostato con successo sul pianeta 2!
        } else {
            panic!("ERRORE: L'esploratore non ha ricevuto MoveToPlanet");
        }

        let tx_from_explorer_clone = tx_from_explorers.clone();

        tx_from_explorer_clone
            .send(ExplorerToOrchestrator::NeighborsRequest {
                explorer_id: 99,
                current_planet_id: 2,
            })
            .unwrap();

        let orch_thread = thread::spawn(move || {
            orch.run(); // Questo invierà i messaggi di Bootstrap prima di leggere le code!
        });

        loop {
            let msg = rx_to_explorer.recv().unwrap();
            if let OrchestratorToExplorer::NeighborsResponse { neighbors: _ } = msg {
                break;
            }
        }

        tx_from_explorer_clone
            .send(ExplorerToOrchestrator::KillExplorerResult { explorer_id: 99 })
            .unwrap();

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

        orch_thread
            .join()
            .expect("ERRORE: Il thread dell'Orchestrator è andato in panico o si è bloccato");
    }
}
