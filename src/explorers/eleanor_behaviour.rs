use rand::Rng;
use std::collections::{HashMap, HashSet};

use common_game::components::planet::PlanetType;
use common_game::components::resource::BasicResourceType::{Carbon, Hydrogen, Oxygen, Silicon};
use common_game::components::resource::ComplexResourceType::{
    AIPartner, Diamond, Dolphin, Life, Robot, Water,
};
use common_game::components::resource::{BasicResourceType, ComplexResourceType};
use common_game::utils::ID;

use crate::explorers::BagSnapshot;
use crate::explorers::explorer::AI;

const FORGET_CHANCE_PER_TICK: f64 = 0.02; // how often an amnesia attempt fires
const FORGET_FIELD_CHANCE: f64 = 0.2; // per-fact drop chance; lower = rarer, and less likely to drop >1

#[derive(Debug, Clone)]
struct PlanetInfo {
    gen_recipes: Option<HashSet<BasicResourceType>>,
    comb_recipes: Option<HashSet<ComplexResourceType>>,
    kind: Option<PlanetType>,
    neighbours: Option<Vec<ID>>,
}

impl PlanetInfo {
    fn new() -> Self {
        Self {
            gen_recipes: None,
            comb_recipes: None,
            kind: None,
            neighbours: None,
        }
    }

    // --- Convenience queries ---
    fn can_generate(&self, r: &BasicResourceType) -> bool {
        self.gen_recipes.as_ref().map_or(false, |s| s.contains(r))
    }
    fn can_combine(&self, r: &ComplexResourceType) -> bool {
        self.comb_recipes.as_ref().map_or(false, |s| s.contains(r))
    }
    fn is_fully_known(&self) -> bool {
        self.gen_recipes.is_some()
            && self.comb_recipes.is_some()
            && self.kind.is_some()
            && self.neighbours.is_some()
    }
    fn is_dangerous(&self) -> bool {
        matches!(self.kind, Some(PlanetType::B) | Some(PlanetType::D))
    }
    fn forget_random(&mut self, rng: &mut impl Rng, id: ID) {
        let mut forgotten = String::new();
        if rng.gen_bool(FORGET_FIELD_CHANCE) {
            self.gen_recipes = None;
            forgotten += "generation recipes, ";
        }
        if rng.gen_bool(FORGET_FIELD_CHANCE) {
            self.comb_recipes = None;
            forgotten += "combination recipes, ";
        }
        if rng.gen_bool(FORGET_FIELD_CHANCE) {
            self.neighbours = None;
            forgotten += "neighbours, ";
        }
        // Kind can't be forgotten by choice. It would be inefficient for the marginal use it has to forget and recompute it,
        // so I leave it alone. Let's say Eleanor's brain doesn't struggle as much to remember useless info

        if forgotten.is_empty() {
            return;
        }

        let forgotten = forgotten.trim_end_matches(", ");
        println!("[Eleanor] amnesia on planet {:?}: forgot {}", id, forgotten);
        // debug!("[Eleanor] amnesia on planet {:?}: forgot {}", id, forgotten);
    }
    // fn forget(&mut self) { *self = Self::new(); }
}

// --- States & Knowledge ------------------

#[derive(Debug, Clone)]
enum KnowledgeState {
    Unknowing,
    DeducingPlanetType,
    NeedGenerationRecipes,
    NeedCombinationRecipes,
    NeedNeighbours,
    Amnesiac,
    Deciding,
}

#[derive(Debug, Clone)]
enum Objective {
    Produce(ComplexResourceType),

    GatherBasic(BasicResourceType),
    CombineInto(ComplexResourceType),

    FindGeneratorFor(BasicResourceType),
    FindCombinerFor(ComplexResourceType),
    Relocate(ID),
}

#[derive(Debug, Default)]
struct ExplorerKnowledge {
    current_planet: ID,
    planets_info: HashMap<ID, PlanetInfo>,
    inventory: BagSnapshot,
}

// --- The explorer: state pointer + accumulated knowledge ---

pub struct Eleanor {
    knowledge: ExplorerKnowledge,
    knowledge_state: KnowledgeState,
    objectives: Vec<Objective>,
    ai: AI,
}

impl Eleanor {
    pub fn new(ai: AI) -> Self {
        Self {
            knowledge: ExplorerKnowledge {
                current_planet: ai.current_planet(),
                inventory: ai.bag(),
                ..Default::default()
            },
            knowledge_state: KnowledgeState::Unknowing,
            objectives: Vec::new(),
            ai,
        }
    }

    pub fn run(&mut self) {
        while !self.ai.is_killed() {
            while self.ai.is_stopped() {
                std::thread::sleep(std::time::Duration::from_millis(1000));
            }
            if self.ai.take_reset() {
                self.reset();
            }

            self.maybe_forget();
            self.knowledge_state = self.decide();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!("[Eleanor] Bweh..."); // TODO
    }

    fn decide(&mut self) -> KnowledgeState {
        // Knowledge always takes priority — act() is only safe when Deciding
        match self.knowledge_state {
            KnowledgeState::Unknowing => {
                self.knowledge
                    .planets_info
                    .insert(self.knowledge.current_planet, PlanetInfo::new());
                KnowledgeState::Amnesiac
            }
            KnowledgeState::DeducingPlanetType => {
                let planet_info = self
                    .knowledge
                    .planets_info
                    .get_mut(&self.knowledge.current_planet)
                    .unwrap();
                if planet_info.comb_recipes.as_ref().unwrap().is_empty() {
                    if planet_info.gen_recipes.as_ref().unwrap().len() > 1 {
                        planet_info.kind = Some(PlanetType::D);
                    } else {
                        planet_info.kind = Some(PlanetType::A);
                    }
                } else if planet_info.comb_recipes.as_ref().unwrap().len() > 1 {
                    planet_info.kind = Some(PlanetType::C);
                } else {
                    planet_info.kind = Some(PlanetType::B);
                }

                KnowledgeState::Amnesiac
            }
            KnowledgeState::NeedGenerationRecipes => {
                if let Ok(()) = self.ai.discover_resources() {
                    self.knowledge
                        .planets_info
                        .get_mut(&self.knowledge.current_planet)
                        .unwrap()
                        .gen_recipes = Some(self.ai.known_resources());
                }
                KnowledgeState::Amnesiac
            }
            KnowledgeState::NeedCombinationRecipes => {
                if let Ok(()) = self.ai.discover_combinations() {
                    self.knowledge
                        .planets_info
                        .get_mut(&self.knowledge.current_planet)
                        .unwrap()
                        .comb_recipes = Some(self.ai.known_combinations());
                }
                KnowledgeState::Amnesiac
            }
            KnowledgeState::NeedNeighbours => {
                if let Ok(()) = self.ai.request_neighbors() {
                    self.knowledge
                        .planets_info
                        .get_mut(&self.knowledge.current_planet)
                        .unwrap()
                        .neighbours = Some(self.ai.neighbors());
                }
                KnowledgeState::Amnesiac
            }
            KnowledgeState::Amnesiac => {
                match self
                    .knowledge
                    .planets_info
                    .get_mut(&self.knowledge.current_planet)
                {
                    None => KnowledgeState::Unknowing,
                    Some(planet_info) => {
                        if planet_info.is_fully_known() {
                            return KnowledgeState::Deciding;
                        }
                        if planet_info.gen_recipes.is_none() {
                            return KnowledgeState::NeedGenerationRecipes;
                        }
                        if planet_info.comb_recipes.is_none() {
                            return KnowledgeState::NeedCombinationRecipes;
                        }
                        if planet_info.neighbours.is_none() {
                            return KnowledgeState::NeedNeighbours;
                        }
                        if planet_info.kind.is_none() {
                            return KnowledgeState::DeducingPlanetType;
                        }
                        KnowledgeState::Amnesiac
                    }
                }
            }
            KnowledgeState::Deciding => {
                self.act();
                KnowledgeState::Amnesiac
            }
        }
    }

    fn act(&mut self) {
        // Empty stack → pick a random new objective
        if self.objectives.is_empty() {
            self.push_random_objective();
            return;
        }

        let objective = self.objectives.last().cloned().unwrap();

        match objective {
            Objective::Produce(target) => {
                self.objectives.pop();
                self.push_recipe_tree(target);
            }

            Objective::GatherBasic(resource) => {
                let can_gen = self
                    .knowledge
                    .planets_info
                    .get(&self.knowledge.current_planet)
                    .map_or(false, |info| info.can_generate(&resource));

                if can_gen {
                    match self.ai.generate(resource) {
                        Ok(()) => {
                            self.knowledge.inventory = self.ai.bag();
                            self.objectives.pop();
                        }
                        Err(_) => { /* retry next tick */ }
                    }
                } else {
                    if self.find_planet_that_generates(&resource).is_none()
                        && !self.has_unexplored_neighbours()
                    {
                        self.abort_current_objective();
                    } else {
                        self.objectives.push(Objective::FindGeneratorFor(resource));
                    }
                }
            }

            Objective::CombineInto(resource) => {
                let can_comb = self
                    .knowledge
                    .planets_info
                    .get(&self.knowledge.current_planet)
                    .map_or(false, |info| info.can_combine(&resource));

                if can_comb {
                    match self.ai.combine(resource) {
                        Ok(()) => {
                            self.knowledge.inventory = self.ai.bag();
                            self.objectives.pop();
                            println!("[Eleanor] Combined {:?}", resource); // TODO
                        }
                        Err(_) => { /* retry next tick */ }
                    }
                } else {
                    if self.find_planet_that_combines(&resource).is_none()
                        && !self.has_unexplored_neighbours()
                    {
                        self.abort_current_objective();
                    } else {
                        self.objectives.push(Objective::FindCombinerFor(resource));
                    }
                }
            }

            Objective::FindGeneratorFor(resource) => {
                match self.find_planet_that_generates(&resource).copied() {
                    Some(dst) => {
                        self.objectives.pop();
                        self.objectives.push(Objective::Relocate(dst));
                    }
                    None => match self.pick_unexplored_neighbour().copied() {
                        Some(next) => self.objectives.push(Objective::Relocate(next)),
                        None => {
                            self.abort_current_objective();
                        }
                    },
                }
            }

            Objective::FindCombinerFor(resource) => {
                match self.find_planet_that_combines(&resource).copied() {
                    Some(dst) => {
                        self.objectives.pop();
                        self.objectives.push(Objective::Relocate(dst));
                    }
                    None => match self.pick_unexplored_neighbour().copied() {
                        Some(next) => self.objectives.push(Objective::Relocate(next)),
                        None => {
                            self.abort_current_objective();
                        }
                    },
                }
            }

            Objective::Relocate(dst) => match self.ai.travel(dst) {
                Ok(()) => {
                    self.knowledge.current_planet = dst;
                    self.knowledge
                        .planets_info
                        .entry(dst)
                        .or_insert_with(PlanetInfo::new);
                    self.knowledge_state = KnowledgeState::Amnesiac;
                    self.objectives.pop();
                }
                Err(_) => {
                    self.on_planet_destroyed(dst);
                }
            },
        }
    }

    // -------------------------------------------------------------------------
    // BAG HELPERS — read from the cached inventory snapshot
    // -------------------------------------------------------------------------

    fn has_basic(&self, r: BasicResourceType) -> bool {
        self.knowledge.inventory.get_basic_count(r) > 0
    }

    fn has_complex(&self, r: ComplexResourceType) -> bool {
        self.knowledge.inventory.get_complex_count(r) > 0
    }

    fn count_basic(&self, r: BasicResourceType) -> usize {
        self.knowledge.inventory.get_basic_count(r)
    }

    // -------------------------------------------------------------------------
    // RECIPE TREE
    // -------------------------------------------------------------------------

    fn push_recipe_tree(&mut self, target: ComplexResourceType) {
        self.objectives.push(Objective::CombineInto(target));

        match target {
            Water => {
                if !self.has_basic(Hydrogen) {
                    self.objectives.push(Objective::GatherBasic(Hydrogen));
                }
                if !self.has_basic(Oxygen) {
                    self.objectives.push(Objective::GatherBasic(Oxygen));
                }
            }
            Diamond => {
                for _ in 0..(2usize.saturating_sub(self.count_basic(Carbon))) {
                    self.objectives.push(Objective::GatherBasic(Carbon));
                }
            }
            Life => {
                if !self.has_complex(Water) {
                    self.objectives.push(Objective::Produce(Water));
                }
                if !self.has_basic(Carbon) {
                    self.objectives.push(Objective::GatherBasic(Carbon));
                }
            }
            Robot => {
                if !self.has_complex(Life) {
                    self.objectives.push(Objective::Produce(Life));
                }
                if !self.has_basic(Silicon) {
                    self.objectives.push(Objective::GatherBasic(Silicon));
                }
            }
            Dolphin => {
                if !self.has_complex(Water) {
                    self.objectives.push(Objective::Produce(Water));
                }
                if !self.has_complex(Life) {
                    self.objectives.push(Objective::Produce(Life));
                }
            }
            AIPartner => {
                if !self.has_complex(Robot) {
                    self.objectives.push(Objective::Produce(Robot));
                }
                if !self.has_complex(Diamond) {
                    self.objectives.push(Objective::Produce(Diamond));
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // RANDOM OBJECTIVE — pick a random ComplexResourceType when stack is empty
    // -------------------------------------------------------------------------

    fn push_random_objective(&mut self) {
        // All possible complex resources
        const ALL_COMPLEX: &[ComplexResourceType] =
            &[Water, Diamond, Life, Robot, Dolphin, AIPartner];

        // Weighted by "difficulty" — favour simpler ones so the explorer
        // does useful work rather than always aiming for AIPartner
        let weights: &[usize] = &[
            3, // Water     — easy, 2 ingredients
            3, // Diamond   — easy, 2 of same
            2, // Life      — medium
            2, // Robot     — medium
            2, // Dolphin   — medium
            1, // AIPartner — hardest
        ];

        // Simple weighted pick using the current planet id as entropy seed
        // (no rand crate needed — deterministic but varied enough for gameplay)
        let total: usize = weights.iter().sum();
        let seed = (self.knowledge.current_planet as usize)
            .wrapping_add(self.knowledge.inventory.get_basic_count(Carbon))
            .wrapping_add(self.knowledge.inventory.get_complex_count(Water))
            % total;

        let mut acc = 0;
        for (resource, weight) in ALL_COMPLEX.iter().zip(weights.iter()) {
            acc += weight;
            if seed < acc {
                self.objectives.push(Objective::Produce(*resource));
                return;
            }
        }

        // Fallback — should never reach here
        self.objectives.push(Objective::Produce(Water));
    }

    // -------------------------------------------------------------------------
    // PLANET DESTROYED — called when travel fails.
    // Removes the planet from our knowledge and purges any Relocate(dst)
    // entries that point to it from the objective stack.
    // -------------------------------------------------------------------------

    fn on_planet_destroyed(&mut self, dead_planet: ID) {
        // Wipe it from our knowledge map
        self.knowledge.planets_info.remove(&dead_planet);

        // Also remove it from any neighbour lists that mention it
        for info in self.knowledge.planets_info.values_mut() {
            if let Some(neighbours) = info.neighbours.as_mut() {
                neighbours.retain(|&id| id != dead_planet);
            }
        }

        // Purge all Relocate(dead_planet) from the objective stack
        self.objectives
            .retain(|obj| !matches!(obj, Objective::Relocate(id) if *id == dead_planet));
    }

    // -------------------------------------------------------------------------
    // ABORT — pop objectives until we reach the nearest Produce boundary,
    // so we abandon the whole sub-tree rather than leaving half-pushed goals
    // -------------------------------------------------------------------------

    fn abort_current_objective(&mut self) {
        // Pop everything up to and including the next Produce or until empty.
        // This clears the sub-goals for the unachievable resource.
        while let Some(obj) = self.objectives.last() {
            match obj {
                Objective::Produce(_) => {
                    self.objectives.pop();
                    break;
                }
                _ => {
                    self.objectives.pop();
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // RESET ELEANOR & FORGET INFO
    // -------------------------------------------------------------------------

    fn reset(&mut self) {
        self.knowledge = ExplorerKnowledge {
            current_planet: self.ai.current_planet(),
            inventory: self.ai.bag(),
            ..Default::default()
        };
        self.knowledge_state = KnowledgeState::Unknowing;
        self.objectives.clear();
    }

    fn maybe_forget(&mut self) {
        let mut rng = rand::thread_rng();
        if !rng.gen_bool(FORGET_CHANCE_PER_TICK) {
            return;
        }

        let ids: Vec<ID> = self.knowledge.planets_info.keys().copied().collect();
        if ids.is_empty() {
            return;
        }
        let victim = ids[rng.gen_range(0..ids.len())];

        if let Some(info) = self.knowledge.planets_info.get_mut(&victim) {
            info.forget_random(&mut rng, victim);
        }

        if victim == self.knowledge.current_planet {
            self.knowledge_state = KnowledgeState::Amnesiac;
        }
    }

    // -------------------------------------------------------------------------
    // HELPERS
    // -------------------------------------------------------------------------

    fn has_unexplored_neighbours(&self) -> bool {
        self.pick_unexplored_neighbour().is_some()
    }

    fn find_planet_that_generates(&self, r: &BasicResourceType) -> Option<&ID> {
        // Prefer a safe generator; fall back to a B/D one only if it's the only
        // place that makes the resource.
        let mut dangerous_fallback = None;
        for (id, info) in self.knowledge.planets_info.iter() {
            if info.can_generate(r) {
                if info.is_dangerous() {
                    dangerous_fallback.get_or_insert(id);
                } else {
                    return Some(id);
                }
            }
        }
        dangerous_fallback
    }

    fn find_planet_that_combines(&self, r: &ComplexResourceType) -> Option<&ID> {
        self.knowledge
            .planets_info
            .iter()
            .find_map(|(id, info)| info.can_combine(r).then_some(id))
    }

    fn pick_unexplored_neighbour(&self) -> Option<&ID> {
        let neighbours = self
            .knowledge
            .planets_info
            .get(&self.knowledge.current_planet)?
            .neighbours
            .as_ref()
            .unwrap();

        neighbours
            .iter()
            .find(|id| {
                self.knowledge
                    .planets_info
                    .get(id)
                    .map_or(true, |info| !info.is_fully_known())
            })
            .or_else(|| neighbours.first())
    }
}
