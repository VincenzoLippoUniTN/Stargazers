use std::collections::HashSet;
use std::ptr::null_mut;
use std::sync::mpsc;
use common_game::components::energy_cell::EnergyCell;
use common_game::components::planet::{Planet, PlanetAI, PlanetState, PlanetType};
use common_game::components::resource::{BasicResourceType, ComplexResourceType, Combinator, Generator, BasicResource, ComplexResourceRequest, ComplexResource};
use common_game::components::rocket::Rocket;
use common_game::protocols::messages;

// Group-defined AI struct
struct AI { /* your AI state here */ }

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
        match(msg) {
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
            // Three expected outcomes:
            //  - None => the planet is not able to generate said resource, no charge available
            //  - Some(CombineResourceResponse { complex_response: None }) => the planet could create the resource, but generation failed
            //  - Some(CombineResourceResponse { complex_response: Some(requested_resource) }) => the planet successfully create the resource
            // Explorer is expected to handle said outcomes.
            messages::ExplorerToPlanet::CombineResourceRequest { explorer_id: _ , msg } => {
                // Checking with planet state if charged cell is available
                if let Some((charged_cell, _)) = state.full_cell(){
                    // Matching generation with requested type
                    match msg {
                        ComplexResourceRequest::Robot(silicon, life) => {
                            match combinator.make_robot(silicon, life, charged_cell) {
                                Ok(robot) => {
                                    // TODO: Log successful resource creation
                                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: Some(ComplexResource::Robot(robot)) })
                                }
                                Err(msg) => {
                                    // TODO: Log unsuccessful resource creation & show msg
                                    Some(messages::PlanetToExplorer::CombineResourceResponse { complex_response: None })
                                }
                            }
                        },
                        _ => {
                            // TODO: Log failed request as combination rule unavailable
                            None
                        }
                    }
                } else {
                    // TODO: Log failed request as no charges available
                    None
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
        generator: &Generator,
        combinator: &Combinator,
    ) -> Option<Rocket> {
        // your handler code here...
        None
    }

    fn start(&mut self, state: &PlanetState) { /* startup code */ }
    fn stop(&mut self, state: &PlanetState) { /* stop code */ }
}

// This is the group's "export" function. It will be called by
// the orchestrator to spawn your planet.
pub fn create_planet(
    rx_orchestrator: mpsc::Receiver<messages::OrchestratorToPlanet>,
    tx_orchestrator: mpsc::Sender<messages::PlanetToOrchestrator>,
    rx_explorer: mpsc::Receiver<messages::ExplorerToPlanet>,
    tx_explorer: mpsc::Sender<messages::PlanetToExplorer>,
) -> Planet {
    let id = 1;
    let ai = AI {};
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
    Planet::new(
        id,
        PlanetType::B,
        Box::new(ai),
        gen_rules,
        comb_rules,
        (rx_orchestrator, tx_orchestrator),
        (rx_explorer, tx_explorer),
    ).unwrap() // Don't call .unwrap()! You should do error checking instead.
}

#[cfg(test)]
mod tests {
    use super::*;
}
