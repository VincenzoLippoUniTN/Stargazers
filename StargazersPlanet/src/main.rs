use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use stargazers_planet::create_planet;
use common_game::protocols::messages::{
    OrchestratorToPlanet, PlanetToOrchestrator,
};

fn main() {
    // --- canali orchestrator ↔ pianeta ---
    let (tx_to_planet, rx_from_orch) = mpsc::channel::<OrchestratorToPlanet>();
    let (tx_from_planet, rx_to_orch) = mpsc::channel::<PlanetToOrchestrator>();

    // --- canali explorer ↔ pianeta (vuoti per ora) ---
    let (_tx_to_expl, rx_from_expl) = mpsc::channel();
    let (tx_from_expl, _rx_from_planet_expl) = mpsc::channel();

    // --- crea il pianeta ---
    let mut planet = create_planet(rx_from_orch, tx_from_planet, rx_from_expl, tx_from_expl);

    // --- lancia il pianeta in un thread ---
    let handle = thread::spawn(move || {
        planet.run().unwrap();
    });

    // --- Start AI ---
    tx_to_planet.send(OrchestratorToPlanet::StartPlanetAI).unwrap();
    match rx_to_orch.recv_timeout(Duration::from_secs(2)) {
        Ok(PlanetToOrchestrator::StartPlanetAIResult { planet_id }) => {
            println!("Orchestrator: Planet {} started!", planet_id);
        }
        _ => println!("Orchestrator: Start timeout"),
    }

    // --- Stop AI ---
    tx_to_planet.send(OrchestratorToPlanet::StopPlanetAI).unwrap();
    match rx_to_orch.recv_timeout(Duration::from_secs(2)) {
        Ok(PlanetToOrchestrator::StopPlanetAIResult { planet_id }) => {
            println!("Orchestrator: Planet {} stopped!", planet_id);
        }
        _ => println!("Orchestrator: Stop timeout"),
    }

    // --- attendi il thread del pianeta (loop infinito per ora) ---
    handle.join().unwrap();
}
