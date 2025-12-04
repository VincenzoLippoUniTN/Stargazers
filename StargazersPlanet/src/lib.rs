use std::sync::mpsc;
use common_game::components::planet::{Planet, PlanetAI, PlanetState, PlanetType};
use common_game::components::resource::{Combinator, Generator};
use common_game::components::rocket::Rocket;
use common_game::protocols::messages;

pub struct AI {
    started: bool,
}
impl PlanetAI for AI {
    fn handle_orchestrator_msg(
        &mut self,
        state: &mut PlanetState,
        generator: &Generator,
        combinator: &Combinator,
        msg: messages::OrchestratorToPlanet
    ) -> Option<messages::PlanetToOrchestrator> {
        // your handler code here...
        None
    }

    fn handle_explorer_msg(
        &mut self,
        state: &mut PlanetState,
        generator: &Generator,
        combinator: &Combinator,
        msg: messages::ExplorerToPlanet
    ) -> Option<messages::PlanetToExplorer> {
        // your handler code here...
        None
    }

    fn handle_asteroid(
        &mut self,
        state: &mut PlanetState,
        generator: &Generator,
        combinator: &Combinator,
    ) -> Option<Rocket> {
        // your handler code here...
        None
    }

    fn start(&mut self, state: &PlanetState) {
        println!("Planet {}: AI started!", state.id());
        self.started = true;

    }
    fn stop(&mut self, state: &PlanetState) { /* stop code */ }
}
pub fn create_planet(
    rx_orchestrator: mpsc::Receiver<messages::OrchestratorToPlanet>,
    tx_orchestrator: mpsc::Sender<messages::PlanetToOrchestrator>,
    rx_explorer: mpsc::Receiver<messages::ExplorerToPlanet>,
    tx_explorer: mpsc::Sender<messages::PlanetToExplorer>
) -> Planet {
    let id = 1;
    let ai = AI {started: false};
    let gen_rules = vec![common_game::components::resource::BasicResourceType::Oxygen];
    let comb_rules = vec![/* your recipes */];

    // Construct the planet and return it
    Planet::new(
        id,
        PlanetType::A,
        Box::new(ai),
        gen_rules,
        comb_rules,
        (rx_orchestrator, tx_orchestrator),
        (rx_explorer, tx_explorer)
    ).unwrap() // Don't call .unwrap()! You should do error checking instead.
}

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
