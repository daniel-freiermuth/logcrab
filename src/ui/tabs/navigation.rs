use crate::input::PaneDirection;

/// Find a neighboring leaf node in the specified direction
pub fn find_neighbor(
    tree: &egui_dock::Tree<super::tabs::TabContent>,
    current: egui_dock::NodeIndex,
    direction: PaneDirection,
) -> Option<egui_dock::NodeIndex> {
    let current_rect = tree[current].rect()?;

    // Track best candidate: (NodeIndex, distance, overlap)
    let mut best: Option<(egui_dock::NodeIndex, f32, f32)> = None;

    for idx_usize in 0..tree.len() {
        let idx = egui_dock::NodeIndex::from(idx_usize);

        if idx == current {
            continue;
        }

        let node = &tree[idx];
        if !node.is_leaf() {
            continue;
        }

        let candidate_rect = match node.rect() {
            Some(r) => r,
            None => continue,
        };

        // Check if candidate is in the correct direction and calculate distance and overlap
        let (interesting_candidate, distance, overlap) = match direction {
            PaneDirection::Left => {
                // Must be entirely to the left
                if candidate_rect.max.x > current_rect.min.x {
                    (false, 0.0, 0.0)
                } else {
                    // Require vertical overlap
                    let overlap = (candidate_rect.max.y.min(current_rect.max.y))
                        - (candidate_rect.min.y.max(current_rect.min.y));
                    if overlap <= 0.0 {
                        (false, 0.0, 0.0)
                    } else {
                        (true, current_rect.min.x - candidate_rect.max.x, overlap)
                    }
                }
            }
            PaneDirection::Right => {
                // Must be entirely to the right
                if candidate_rect.min.x < current_rect.max.x {
                    (false, 0.0, 0.0)
                } else {
                    // Require vertical overlap
                    let overlap = (candidate_rect.max.y.min(current_rect.max.y))
                        - (candidate_rect.min.y.max(current_rect.min.y));
                    if overlap <= 0.0 {
                        (false, 0.0, 0.0)
                    } else {
                        (true, candidate_rect.min.x - current_rect.max.x, overlap)
                    }
                }
            }
            PaneDirection::Up => {
                // Must be entirely above
                if candidate_rect.max.y > current_rect.min.y {
                    (false, 0.0, 0.0)
                } else {
                    // Require horizontal overlap
                    let overlap = (candidate_rect.max.x.min(current_rect.max.x))
                        - (candidate_rect.min.x.max(current_rect.min.x));
                    if overlap <= 0.0 {
                        (false, 0.0, 0.0)
                    } else {
                        (true, current_rect.min.y - candidate_rect.max.y, overlap)
                    }
                }
            }
            PaneDirection::Down => {
                // Must be entirely below
                if candidate_rect.min.y < current_rect.max.y {
                    (false, 0.0, 0.0)
                } else {
                    // Require horizontal overlap
                    let overlap = (candidate_rect.max.x.min(current_rect.max.x))
                        - (candidate_rect.min.x.max(current_rect.min.x));
                    if overlap <= 0.0 {
                        (false, 0.0, 0.0)
                    } else {
                        (true, candidate_rect.min.y - current_rect.max.y, overlap)
                    }
                }
            }
        };

        if !interesting_candidate {
            continue;
        }

        // Pick the best candidate: closest distance, then most overlap as tiebreaker
        let is_better = best.is_none_or(|(_, best_dist, best_overlap)| {
            const EPSILON: f32 = 1e-6;
            if (distance - best_dist).abs() < EPSILON {
                // Distances are essentially equal, use overlap as tiebreaker
                overlap > best_overlap
            } else {
                distance < best_dist
            }
        });

        if is_better {
            best = Some((idx, distance, overlap));
        }
    }

    best.map(|(idx, _, _)| idx)
}
