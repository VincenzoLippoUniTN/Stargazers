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
    Evacuating,
    BunkerCrafting,
    VictoryFlex,
}

#[derive(Debug, Clone, Default)]
pub struct AmonPlanetInfo {
    generates: HashSet<BasicResourceType>,
    combines: HashSet<ComplexResourceType>,
    neighbors: Vec<ID>,
}

impl AmonPlanetInfo {
    pub fn is_bunker(&self) -> bool {
        !self.combines.is_empty() && self.generates.is_empty()
    }
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

        if elapsed >= 24 && state == AmonState::GoldRush {
            ai.log(Channel::Warning, "🚨 ALLARME 24 SECONDI! Evacuazione immediata verso il Bunker!");
            state = AmonState::Evacuating;
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

                // --- CALCOLO ESATTO DELLA RICETTA ---
                let c = bag.get_basic_count(Carbon);
                let si = bag.get_basic_count(Silicon);
                let h = bag.get_basic_count(Hydrogen);
                let o = bag.get_basic_count(Oxygen);

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

                // --- MOVIMENTO CHIRURGICO ---
                if !extracted_this_tick {
                    let needed_resources: Vec<_> = quotas.iter()
                        .filter(|(res, target)| bag.get_basic_count(*res) < *target)
                        .map(|(res, _)| *res)
                        .collect();

                    if needed_resources.is_empty() {
                        ai.log(Channel::Info, "🎒 SET COMPLETATO! Amon cerca il prossimo pianeta...");
                    }

                    let target_planet = galaxy_map.iter().find_map(|(&id, p_info)| {
                        if id != current_planet {
                            if needed_resources.iter().any(|res| p_info.generates.contains(res)) {
                                return Some(id);
                            }
                        }
                        None
                    });

                    if let Some(target_id) = target_planet {
                        if let Some(next_hop) = get_next_hop(current_planet, target_id, &galaxy_map) {
                            ai.log(Channel::Debug, &format!("🚀 Manca roba! GPS per Pianeta {} (Salto su {})", target_id, next_hop));
                            let _ = ai.travel(next_hop);
                        }
                    } else {
                        let unexplored = info.neighbors.iter().find(|&&n| !galaxy_map.contains_key(&n));

                        if let Some(&target) = unexplored {
                            ai.log(Channel::Debug, &format!("🔭 Esploro nuovo settore: salto su {}", target));
                            let _ = ai.travel(target);
                        } else {
                            let fallback = info.neighbors.first().copied().unwrap_or(current_planet);
                            let _ = ai.travel(fallback);
                        }
                    }
                }
            }

            AmonState::Evacuating => {
                let current_planet = ai.current_planet();

                // ISPIRAZIONE ELEANOR: Scopriamo i dettagli del pianeta anche durante la fuga!
                let _ = ai.discover_resources();
                let _ = ai.discover_combinations();
                let _ = ai.request_neighbors();

                let mut info = AmonPlanetInfo::default();
                info.generates = ai.known_resources();
                info.combines = ai.known_combinations();
                info.neighbors = ai.neighbors();
                galaxy_map.insert(current_planet, info.clone());

                if info.is_bunker() {
                    ai.log(Channel::Info, "🛡️ Arrivato al Bunker! Avvio catena di montaggio opportunistica.");
                    state = AmonState::BunkerCrafting;
                    continue; // Inizia subito a craftare in questo tick!
                }

                let bunker_target = galaxy_map.iter().find_map(|(&id, p_info)| {
                    if p_info.is_bunker() { Some(id) } else { None }
                });

                if let Some(bunker_id) = bunker_target {
                    if let Some(next_hop) = get_next_hop(current_planet, bunker_id, &galaxy_map) {
                        ai.log(Channel::Warning, &format!("🏃 Fuga verso Bunker {} via {}!", bunker_id, next_hop));
                        let _ = ai.travel(next_hop);
                    }
                } else {
                    let neighbors = info.neighbors;
                    let next = neighbors.iter()
                        .find(|&&n| !galaxy_map.contains_key(&n))
                        .unwrap_or_else(|| neighbors.first().unwrap_or(&current_planet));
                    ai.log(Channel::Warning, "⚠️ Bunker ignoto! Viaggio alla cieca cercando rifugio!");
                    let _ = ai.travel(*next);
                }
            }

            AmonState::BunkerCrafting => {
                let current_planet = ai.current_planet();

                let _ = ai.discover_combinations();
                let _ = ai.request_neighbors();

                let mut current_info = AmonPlanetInfo::default();
                current_info.generates = ai.known_resources();
                current_info.combines = ai.known_combinations();
                current_info.neighbors = ai.neighbors();
                galaxy_map.insert(current_planet, current_info.clone());

                let bag = ai.bag();

                // --- STRATEGIA ELEANOR: ARTIGIANATO OPPORTUNISTICO LOCALE ---
                // Prima di viaggiare, esauriamo QUALSIASI combinazione fattibile su QUESTO pianeta!
                let mut crafted_local = false;

                if current_info.combines.contains(&AIPartner) && bag.get_complex_count(Robot) > 0 && bag.get_complex_count(Diamond) > 0 {
                    if ai.combine(AIPartner).is_ok() {
                        ai.log(Channel::Info, "✨ SUCCESSORIO: Creato ed evoluto AIPartner! (VITTORIA)");
                        crafted_local = true;
                    }
                } else if current_info.combines.contains(&Robot) && bag.get_complex_count(Life) > 0 && bag.get_basic_count(Silicon) > 0 {
                    if ai.combine(Robot).is_ok() {
                        ai.log(Channel::Debug, "⚙️ Successo Fabbrica: Creato Robot");
                        crafted_local = true;
                    }
                } else if current_info.combines.contains(&Diamond) && bag.get_basic_count(Carbon) >= 2 {
                    if ai.combine(Diamond).is_ok() {
                        ai.log(Channel::Debug, "💎 Successo Fabbrica: Creato Diamond");
                        crafted_local = true;
                    }
                } else if current_info.combines.contains(&Life) && bag.get_complex_count(Water) > 0 && bag.get_basic_count(Carbon) > 0 {
                    if ai.combine(Life).is_ok() {
                        ai.log(Channel::Debug, "🌱 Successo Fabbrica: Creato Life");
                        crafted_local = true;
                    }
                } else if current_info.combines.contains(&Water) && bag.get_basic_count(Hydrogen) > 0 && bag.get_basic_count(Oxygen) > 0 {
                    if ai.combine(Water).is_ok() {
                        ai.log(Channel::Debug, "💧 Successo Fabbrica: Creato Water");
                        crafted_local = true;
                    }
                }

                if crafted_local {
                    continue; // Se abbiamo combinato qualcosa, ricalcoliamo subito il tick con il nuovo zaino!
                }

                // --- NAVIGAZIONE INDUSTRIALE (SE IL POSTO ATTUALE NON PUÒ PIÙ FARE NULLA) ---
                // Se non possiamo fare altro qui, scopriamo qual è la risorsa più avanzata che POSSIAMO fare altrove.
                let next_global_target = if bag.get_complex_count(Robot) > 0 && bag.get_complex_count(Diamond) > 0 {
                    Some(AIPartner)
                } else if bag.get_complex_count(Life) > 0 && bag.get_basic_count(Silicon) > 0 {
                    Some(Robot)
                } else if bag.get_basic_count(Carbon) >= 2 {
                    Some(Diamond)
                } else if bag.get_complex_count(Water) > 0 && bag.get_basic_count(Carbon) > 0 {
                    Some(Life)
                } else if bag.get_basic_count(Hydrogen) > 0 && bag.get_basic_count(Oxygen) > 0 {
                    Some(Water)
                } else {
                    None
                };

                if let Some(target_complex) = next_global_target {
                    // Troviamo nella mappa chi sa craftare questa specifica risorsa
                    let destination_planet = galaxy_map.iter().find_map(|(&id, p_info)| {
                        if p_info.combines.contains(&target_complex) { Some(id) } else { None }
                    });

                    if let Some(target_id) = destination_planet {
                        if let Some(next_hop) = get_next_hop(current_planet, target_id, &galaxy_map) {
                            ai.log(Channel::Info, &format!("🏃 Navigazione: vado su {} per combinare {:?}", target_id, target_complex));
                            let _ = ai.travel(next_hop);
                        }
                    } else {
                        // Se non sappiamo chi la fa, viaggiamo verso un altro bunker noto o cerchiamo nell'ignoto
                        let fallback_bunker = galaxy_map.iter().find_map(|(&id, p_info)| {
                            if id != current_planet && p_info.is_bunker() { Some(id) } else { None }
                        });

                        if let Some(bunker_id) = fallback_bunker {
                            if let Some(next_hop) = get_next_hop(current_planet, bunker_id, &galaxy_map) {
                                ai.log(Channel::Warning, &format!("🔍 Ricetta per {:?} ignota. Cerco nel bunker {}", target_complex, bunker_id));
                                let _ = ai.travel(next_hop);
                            }
                        } else {
                            let unexplored = current_info.neighbors.iter().find(|&&n| !galaxy_map.contains_key(&n));
                            if let Some(&target) = unexplored {
                                ai.log(Channel::Debug, &format!("🔭 Cerco laboratori nell'ignoto: salto su {}", target));
                                let _ = ai.travel(target);
                            } else {
                                state = AmonState::VictoryFlex;
                            }
                        }
                    }
                } else {
                    // Non ci sono più materiali per fare nient'altro in assoluto
                    ai.log(Channel::Info, "🏁 Linea di montaggio completata. Risorse residue esaurite.");
                    state = AmonState::VictoryFlex;
                }
            }

            AmonState::VictoryFlex => {
                let final_bag = ai.bag();
                ai.log(Channel::Info, &format!(
                    "🏆 AMON HA FINITO. Risorse Finali -> AIPartner: {} | Robot avanzi: {} | Diamanti extra: {}.",
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