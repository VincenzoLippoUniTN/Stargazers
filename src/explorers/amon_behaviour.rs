use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Instant, Duration};
use std::thread;

use common_game::components::resource::BasicResourceType::{self, Carbon, Hydrogen, Oxygen, Silicon};
use common_game::components::resource::ComplexResourceType::{self, Water, Diamond, Life, Robot, AIPartner};
use common_game::logging::Channel;
use common_game::utils::ID;

use crate::explorers::explorer::AI;

#[derive(Debug, PartialEq, Clone, Copy)]
enum AmonState {
    GoldRush,
    AssemblyLine, // Sostituisce Evacuating e BunkerCrafting, unendoli in modo logico
    VictoryFlex,
}

#[derive(Debug, Clone, Default)]
pub struct AmonPlanetInfo {
    generates: HashSet<BasicResourceType>,
    combines: HashSet<ComplexResourceType>,
    neighbors: Vec<ID>,
}

fn get_next_hop(start: ID, target: ID, galaxy_map: &HashMap<ID, AmonPlanetInfo>) -> Option<ID> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut came_from: HashMap<ID, ID> = HashMap::new();

    queue.push_back(start);
    visited.insert(start);

    while let Some(current) = queue.pop_front() {
        if current == target {
            let mut path_node = target;
            while let Some(&prev) = came_from.get(&path_node) {
                if prev == start {
                    return Some(path_node);
                }
                path_node = prev;
            }
        }

        if let Some(info) = galaxy_map.get(&current) {
            for &neighbor in &info.neighbors {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    came_from.insert(neighbor, current);
                    queue.push_back(neighbor);
                }
            }
        }
    }
    None
}

pub fn amon_behaviour(ai: AI) {
    let start_time = Instant::now();
    let mut state = AmonState::GoldRush;
    let mut galaxy_map: HashMap<ID, AmonPlanetInfo> = HashMap::new();

    ai.log(Channel::Info, "🤖 Amon online. Ricetta AIPartner caricata (3C, 1Si, 1H, 1O).");

    while !ai.is_killed() {
        let elapsed = start_time.elapsed().as_secs();

        // Limite di tempo per rompere gli indugi
        if elapsed >= 24 && state == AmonState::GoldRush {
            ai.log(Channel::Warning, "🚨 ALLARME 24 SECONDI! Fine estrazione, avvio catena di montaggio!");
            state = AmonState::AssemblyLine;
        }

        match state {
            AmonState::GoldRush => {
                let current_planet = ai.current_planet();
                let bag = ai.bag();

                let _ = ai.discover_resources();
                let _ = ai.discover_combinations();
                let _ = ai.request_neighbors();

                let mut info = AmonPlanetInfo::default();
                info.generates = ai.known_resources();
                info.combines = ai.known_combinations();
                info.neighbors = ai.neighbors();

                galaxy_map.insert(current_planet, info.clone());

                let c = bag.get_basic_count(Carbon);
                let si = bag.get_basic_count(Silicon);
                let h = bag.get_basic_count(Hydrogen);
                let o = bag.get_basic_count(Oxygen);

                // Appena ha gli ingredienti esatti per 1 AIPartner, va in catena di montaggio.
                if c >= 3 && si >= 1 && h >= 1 && o >= 1 {
                    ai.log(Channel::Info, "🎒 Risorse perfette per AIPartner raccolte! Avvio assemblaggio.");
                    state = AmonState::AssemblyLine;
                    continue;
                }

                let completed_sets = (c / 3).min(si).min(h).min(o);
                let target_sets = completed_sets + 1;

                let quotas = [
                    (Carbon, target_sets * 3),
                    (Silicon, target_sets * 1),
                    (Hydrogen, target_sets * 1),
                    (Oxygen, target_sets * 1),
                ];

                let mut extracted_this_tick = false;

                for (resource, target_amount) in quotas.iter() {
                    if info.generates.contains(resource) {
                        let current_amount = bag.get_basic_count(*resource);

                        if current_amount < *target_amount {
                            if ai.generate(resource.clone()).is_ok() {
                                ai.log(Channel::Debug, &format!("⛏️ Preso {:?} (Zaino: {}/{})", resource, current_amount + 1, target_amount));
                                extracted_this_tick = true;
                                break;
                            }
                        }
                    }
                }

                if !extracted_this_tick {
                    let needed_resources: Vec<_> = quotas.iter()
                        .filter(|(res, target)| bag.get_basic_count(*res) < *target)
                        .map(|(res, _)| *res)
                        .collect();

                    let target_planet = galaxy_map.iter().find_map(|(&id, p_info)| {
                        if id != current_planet && needed_resources.iter().any(|res| p_info.generates.contains(res)) {
                            return Some(id);
                        }
                        None
                    });

                    if let Some(target_id) = target_planet {
                        if let Some(next_hop) = get_next_hop(current_planet, target_id, &galaxy_map) {
                            if ai.travel(next_hop).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                        }
                    } else {
                        let unexplored = info.neighbors.iter().find(|&&n| !galaxy_map.contains_key(&n));
                        if let Some(&target) = unexplored {
                            if ai.travel(target).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                        } else {
                            // MECCANISMO ANTI-RIMBALZO
                            let fallback = info.neighbors[(elapsed as usize) % info.neighbors.len().max(1)];
                            if ai.travel(fallback).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                        }
                    }
                }
            }

            AmonState::AssemblyLine => {
                let current_planet = ai.current_planet();
                let bag = ai.bag();

                if bag.get_complex_count(AIPartner) > 0 {
                    state = AmonState::VictoryFlex;
                    continue;
                }

                let _ = ai.discover_combinations();
                let _ = ai.request_neighbors();

                let mut current_info = galaxy_map.get(&current_planet).cloned().unwrap_or_default();
                current_info.combines = ai.known_combinations();
                current_info.neighbors = ai.neighbors();
                galaxy_map.insert(current_planet, current_info.clone());

                // Come Eleanor, decide un bersaglio specifico dal basso verso l'alto
                let mut target_complex = None;
                if bag.get_complex_count(Robot) > 0 && bag.get_complex_count(Diamond) > 0 {
                    target_complex = Some(AIPartner);
                } else if bag.get_complex_count(Life) > 0 && bag.get_basic_count(Silicon) > 0 {
                    target_complex = Some(Robot);
                } else if bag.get_basic_count(Carbon) >= 2 && bag.get_complex_count(Diamond) == 0 {
                    target_complex = Some(Diamond);
                } else if bag.get_complex_count(Water) > 0 && bag.get_basic_count(Carbon) > 0 {
                    target_complex = Some(Life);
                } else if bag.get_basic_count(Hydrogen) > 0 && bag.get_basic_count(Oxygen) > 0 {
                    target_complex = Some(Water);
                }

                if let Some(target) = target_complex {
                    // Controlla se può fabbricarlo QUI
                    if current_info.combines.contains(&target) {
                        if ai.combine(target).is_ok() {
                            ai.log(Channel::Info, &format!("🛠️ Successo Fabbrica: Creato {:?}", target));
                            thread::sleep(Duration::from_millis(200));
                        }
                    } else {
                        // Cerca un pianeta che sa fare questa specifica risorsa
                        let dest = galaxy_map.iter().find_map(|(&id, p_info)| {
                            if p_info.combines.contains(&target) { Some(id) } else { None }
                        });

                        if let Some(dest_id) = dest {
                            if let Some(next_hop) = get_next_hop(current_planet, dest_id, &galaxy_map) {
                                ai.log(Channel::Debug, &format!("🏃 Viaggio verso {} per combinare {:?}", dest_id, target));
                                if ai.travel(next_hop).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                            }
                        } else {
                            // Se non lo conosce, esplora
                            let unexplored = current_info.neighbors.iter().find(|&&n| !galaxy_map.contains_key(&n));
                            if let Some(&next) = unexplored {
                                ai.log(Channel::Debug, &format!("🔭 Cerco laboratorio per {:?}: salto su ignoto {}", target, next));
                                if ai.travel(next).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                            } else {
                                // ANTI-RIMBALZO
                                let fallback = current_info.neighbors[(elapsed as usize) % current_info.neighbors.len().max(1)];
                                ai.log(Channel::Warning, &format!("⚠️ Giro a vuoto cercando laboratorio per {:?}, salto su {}", target, fallback));
                                if ai.travel(fallback).is_ok() { thread::sleep(Duration::from_millis(1500)); }
                            }
                        }
                    }
                } else {
                    // Se non sa cosa costruire, ha finito le risorse base troppo presto!
                    ai.log(Channel::Warning, "⚠️ Risorse terminate prima di AIPartner! Ritorno a scavare.");
                    state = AmonState::GoldRush;
                }
            }

            AmonState::VictoryFlex => {
                let final_bag = ai.bag();
                ai.log(Channel::Info, &format!(
                    "🏆 AMON HA FINITO. Risorse Finali -> AIPartner: {} | Robot avanzi: {} | Diamanti extra: {}",
                    final_bag.get_complex_count(AIPartner),
                    final_bag.get_complex_count(Robot),
                    final_bag.get_complex_count(Diamond)
                ));

                loop {
                    if ai.is_killed() { break; }
                    thread::sleep(Duration::from_millis(500));
                }
            }
        }
        thread::sleep(Duration::from_millis(40));
    }
}