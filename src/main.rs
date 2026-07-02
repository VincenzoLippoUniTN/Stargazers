mod explorers;
mod galaxy_layout;
mod orchestrator;
mod visualizer;

fn main() {
    env_logger::init();
    orchestrator::launch();
}
