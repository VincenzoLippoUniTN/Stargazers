mod orchestrator;
mod visualizer;
mod explorers;

fn main() {
    env_logger::init();
    orchestrator::launch();
}