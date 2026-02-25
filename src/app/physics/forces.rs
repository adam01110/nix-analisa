use eframe::egui::{Vec2, vec2};

use super::quadtree::QuadNode;

#[derive(Clone, Copy)]
pub(super) struct CollisionParams {
    pub(super) collision_strength: f32,
    pub(super) max_collision_distance_sq: f32,
}

fn repulsion_between(
    point_a: Vec2,
    point_b: Vec2,
    repulsion_strength: f32,
    softening: f32,
) -> Vec2 {
    let delta = point_a - point_b;
    let distance_sq = delta.length_sq();
    let distance = distance_sq.sqrt();
    let direction = if distance > 0.0001 {
        delta / distance
    } else {
        vec2(1.0, 0.0)
    };
    direction * (repulsion_strength / (distance_sq + softening))
}

pub(super) fn accumulate_repulsion_for_node(
    node: &QuadNode,
    index: usize,
    positions: &[Vec2],
    repulsion_strength: f32,
    softening: f32,
    theta: f32,
    force: &mut Vec2,
) {
    if node.mass <= 0.0 {
        return;
    }

    let point = positions[index];

    if node.is_leaf() {
        for &other_index in &node.indices {
            if other_index == index {
                continue;
            }
            *force +=
                repulsion_between(point, positions[other_index], repulsion_strength, softening);
        }
        return;
    }

    let delta = point - node.center_of_mass;
    let distance_sq = delta.length_sq().max(0.0001);
    let distance = distance_sq.sqrt();
    let can_approximate = !node.bounds.contains(point)
        && ((node.bounds.side_length() / distance) < theta)
        && node.mass > 1.0;

    if can_approximate {
        let direction = delta / distance;
        let scaled = (repulsion_strength * node.mass) / (distance_sq + softening);
        *force += direction * scaled;
        return;
    }

    for child in &node.children {
        if let Some(child) = child.as_ref() {
            accumulate_repulsion_for_node(
                child,
                index,
                positions,
                repulsion_strength,
                softening,
                theta,
                force,
            );
        }
    }
}

pub(super) fn accumulate_collision_pairs(
    node_a: &QuadNode,
    node_b: &QuadNode,
    same_node: bool,
    positions: &[Vec2],
    radii: &[f32],
    params: CollisionParams,
    forces: &mut [Vec2],
) {
    if node_a.bounds.distance_sq_to(node_b.bounds) > params.max_collision_distance_sq {
        return;
    }

    if node_a.is_leaf() && node_b.is_leaf() {
        if same_node {
            for i in 0..node_a.indices.len() {
                let from = node_a.indices[i];
                for j in (i + 1)..node_a.indices.len() {
                    let to = node_a.indices[j];
                    let delta = positions[from] - positions[to];
                    let distance_sq = delta.length_sq();
                    let distance = distance_sq.sqrt();
                    let direction = if distance > 0.0001 {
                        delta / distance
                    } else {
                        let angle = ((from as f32) * 0.618_034 + (to as f32) * 0.414_214)
                            * std::f32::consts::TAU;
                        vec2(angle.cos(), angle.sin())
                    };

                    let min_distance = (radii[from] + radii[to]) * 4.2;
                    if distance < min_distance {
                        let overlap_push = (min_distance - distance) * params.collision_strength;
                        forces[from] += direction * overlap_push;
                        forces[to] -= direction * overlap_push;
                    }
                }
            }
        } else {
            for &from in &node_a.indices {
                for &to in &node_b.indices {
                    let delta = positions[from] - positions[to];
                    let distance_sq = delta.length_sq();
                    let distance = distance_sq.sqrt();
                    let direction = if distance > 0.0001 {
                        delta / distance
                    } else {
                        let angle = ((from as f32) * 0.618_034 + (to as f32) * 0.414_214)
                            * std::f32::consts::TAU;
                        vec2(angle.cos(), angle.sin())
                    };

                    let min_distance = (radii[from] + radii[to]) * 4.2;
                    if distance < min_distance {
                        let overlap_push = (min_distance - distance) * params.collision_strength;
                        forces[from] += direction * overlap_push;
                        forces[to] -= direction * overlap_push;
                    }
                }
            }
        }
        return;
    }

    if same_node {
        for first in 0..4 {
            let Some(child_a) = node_a.children[first].as_ref() else {
                continue;
            };

            accumulate_collision_pairs(child_a, child_a, true, positions, radii, params, forces);

            for second in (first + 1)..4 {
                let Some(child_b) = node_a.children[second].as_ref() else {
                    continue;
                };
                accumulate_collision_pairs(
                    child_a, child_b, false, positions, radii, params, forces,
                );
            }
        }
        return;
    }

    let split_a = if node_a.is_leaf() {
        false
    } else if node_b.is_leaf() {
        true
    } else {
        node_a.bounds.half_extent >= node_b.bounds.half_extent
    };

    if split_a {
        for child in &node_a.children {
            let Some(child) = child.as_ref() else {
                continue;
            };
            accumulate_collision_pairs(child, node_b, false, positions, radii, params, forces);
        }
    } else {
        for child in &node_b.children {
            let Some(child) = child.as_ref() else {
                continue;
            };
            accumulate_collision_pairs(node_a, child, false, positions, radii, params, forces);
        }
    }
}
