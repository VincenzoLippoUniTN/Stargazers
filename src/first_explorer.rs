use std::collections::HashSet;
use common_game::protocols::orchestrator_explorer::{ExplorerToOrchestrator, OrchestratorToExplorer};
use common_game::components::resource::BasicResourceType;
use crossbeam_channel::{Receiver, Sender};

pub struct FirstExplorer {
    pub name: String,
}

impl FirstExplorer {
    pub fn new(name: String) -> Self {
        FirstExplorer { name }
    }

    pub fn run(
        &self,
        rx_from_orchestrator: Receiver<OrchestratorToExplorer>,
        tx_to_orchestrator: Sender<ExplorerToOrchestrator<()>>,
        explorer_id: u32,
    ) {
        println!("[FirstExplorer {}] Active and In Waiting...", self.name);

        while let Ok(msg) = rx_from_orchestrator.recv() {
            match msg {
                OrchestratorToExplorer::SupportedResourceRequest => {
                    println!("[FirstExplorer {}] Received: SupportedResourceRequest", self.name);

                    let mut mock_supported_resources = HashSet::new();

                    if self.name == "Anon" {
                        mock_supported_resources.insert(BasicResourceType::Oxygen);
                        mock_supported_resources.insert(BasicResourceType::Carbon);
                    } else {
                        mock_supported_resources.insert(BasicResourceType::Hydrogen);
                    }

                    let response = ExplorerToOrchestrator::SupportedResourceResult {
                        explorer_id,
                        supported_resources: mock_supported_resources,
                    };

                    let _ = tx_to_orchestrator.send(response);
                }
                _ => {}
            }
        }

        println!("[FirstExplorer {}] Thread end.", self.name);
    }
}