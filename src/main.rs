mod orchestrator;
mod visualizer;
mod explorers;
mod galaxy_layout;

fn main() {
    env_logger::init();
    orchestrator::launch();
}