//! The output contract for *query results* flowing back to the viewer.
//!
//! [`crate::command`] carries *questions* from the UI to the simulation
//! (e.g. "what's in this explorer's bag?"). Those questions can't be answered by
//! the next [`crate::GalaxySnapshot`] because a snapshot only describes the
//! galaxy's physical state, not an explorer's inventory or a planet's recipe
//! list. This module is the answer path: the simulation formats a reply as a
//! [`GalaxyReport`] and pushes it here; the visualizer drains it and shows it in
//! the HUD.
//!
//! Reports flow the same direction as snapshots (simulation -> viewer), so this
//! mirrors [`crate::feed`] rather than [`crate::command`].

use bevy::prelude::Resource;
use crossbeam_channel::{Receiver, Sender};

/// An answer to a query [`crate::GalaxyCommand`], ready to show to the user.
///
/// The strings are pre-formatted by the producer so the visualizer never needs
/// to know about the simulation's internal resource/bag types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GalaxyReport {
    /// The contents of an explorer's bag (answer to `BagContent`).
    Bag {
        explorer_id: u32,
        /// `(resource name, count)` for each basic resource held.
        basic: Vec<(String, usize)>,
        /// `(resource name, count)` for each complex resource held.
        complex: Vec<(String, usize)>,
    },
    /// Basic resources the explorer's current planet can generate
    /// (answer to `SupportedResources`).
    SupportedResources {
        explorer_id: u32,
        resources: Vec<String>,
    },
    /// Combination recipes the explorer's current planet supports
    /// (answer to `SupportedCombinations`).
    SupportedCombinations {
        explorer_id: u32,
        combinations: Vec<String>,
    },
    /// Outcome of a `Generate` request.
    Generated {
        explorer_id: u32,
        outcome: Result<(), String>,
    },
    /// Outcome of a `Combine` request.
    Combined {
        explorer_id: u32,
        outcome: Result<(), String>,
    },
    /// A free-form status line (e.g. "explorer killed").
    Notice { text: String },
}

impl GalaxyReport {
    /// Renders the report as one or more HUD lines.
    pub fn describe(&self) -> Vec<String> {
        match self {
            GalaxyReport::Bag {
                explorer_id,
                basic,
                complex,
            } => {
                let mut lines = vec![format!("Explorer {explorer_id} bag:")];
                if basic.is_empty() && complex.is_empty() {
                    lines.push("  (empty)".to_string());
                } else {
                    if !basic.is_empty() {
                        lines.push(format!("  basic:   {}", join_counts(basic)));
                    }
                    if !complex.is_empty() {
                        lines.push(format!("  complex: {}", join_counts(complex)));
                    }
                }
                lines
            }
            GalaxyReport::SupportedResources {
                explorer_id,
                resources,
            } => {
                vec![format!(
                    "Explorer {explorer_id} can generate: {}",
                    join_or_none(resources)
                )]
            }
            GalaxyReport::SupportedCombinations {
                explorer_id,
                combinations,
            } => {
                vec![format!(
                    "Explorer {explorer_id} can combine: {}",
                    join_or_none(combinations)
                )]
            }
            GalaxyReport::Generated {
                explorer_id,
                outcome,
            } => match outcome {
                Ok(()) => vec![format!("Explorer {explorer_id}: generated a resource")],
                Err(e) => vec![format!("Explorer {explorer_id}: generate failed ({e})")],
            },
            GalaxyReport::Combined {
                explorer_id,
                outcome,
            } => match outcome {
                Ok(()) => vec![format!("Explorer {explorer_id}: combined a resource")],
                Err(e) => vec![format!("Explorer {explorer_id}: combine failed ({e})")],
            },
            GalaxyReport::Notice { text } => vec![text.clone()],
        }
    }
}

fn join_counts(items: &[(String, usize)]) -> String {
    items
        .iter()
        .map(|(name, count)| format!("{name} x{count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.join(", ")
    }
}

/// Creates a connected sender/feed pair for reports. Keep the [`ReportSender`] on
/// the producer (orchestrator) side and hand the [`ReportFeed`] to
/// [`crate::run_with_reports`].
pub fn report_channel() -> (ReportSender, ReportFeed) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (ReportSender(tx), ReportFeed(rx))
}

/// Producer side: emits query answers. Cheap to clone.
#[derive(Clone)]
pub struct ReportSender(Sender<GalaxyReport>);

impl ReportSender {
    /// Emits a report, best-effort. Silently does nothing if the viewer has gone.
    pub fn send(&self, report: GalaxyReport) {
        let _ = self.0.send(report);
    }
}

/// Consumer handle held by the visualizer.
#[derive(Resource)]
pub struct ReportFeed(Receiver<GalaxyReport>);

impl ReportFeed {
    /// Drains every pending report, oldest first.
    pub(crate) fn drain(&self) -> Vec<GalaxyReport> {
        let mut out = Vec::new();
        while let Ok(report) = self.0.try_recv() {
            out.push(report);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bag_reads_empty() {
        let r = GalaxyReport::Bag {
            explorer_id: 1,
            basic: vec![],
            complex: vec![],
        };
        assert_eq!(
            r.describe(),
            vec!["Explorer 1 bag:".to_string(), "  (empty)".to_string()]
        );
    }

    #[test]
    fn bag_lists_only_nonempty_sections() {
        let r = GalaxyReport::Bag {
            explorer_id: 7,
            basic: vec![("Oxygen".into(), 2)],
            complex: vec![],
        };
        let lines = r.describe();
        assert_eq!(lines[0], "Explorer 7 bag:");
        assert!(lines.iter().any(|l| l.contains("Oxygen x2")));
        assert!(!lines.iter().any(|l| l.contains("complex")));
    }

    #[test]
    fn supported_resources_none_is_explicit() {
        let r = GalaxyReport::SupportedResources {
            explorer_id: 3,
            resources: vec![],
        };
        assert_eq!(
            r.describe(),
            vec!["Explorer 3 can generate: (none)".to_string()]
        );
    }

    #[test]
    fn channel_round_trips_in_order() {
        let (tx, feed) = report_channel();
        tx.send(GalaxyReport::Notice { text: "a".into() });
        tx.send(GalaxyReport::Notice { text: "b".into() });
        let drained = feed.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], GalaxyReport::Notice { text: "a".into() });
        assert!(feed.drain().is_empty());
    }
}
