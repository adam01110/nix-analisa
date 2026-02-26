mod forces;
mod quadtree;

use eframe::egui::{Vec2, vec2};

use super::{PhysicsConfig, RenderGraph, RenderNode};
use forces::{CollisionParams, accumulate_collision_pairs, accumulate_repulsion_for_node};
pub(in crate::app) use quadtree::QuadtreeCell;
use quadtree::{QuadNode, collect_quadtree_cells};

const BARNES_HUT_THETA: f32 = 0.72;

pub(in crate::app) fn quadtree_cells(
    nodes: &[RenderNode],
    positions: &mut Vec<Vec2>,
    cells: &mut Vec<QuadtreeCell>,
) {
    positions.clear();
    positions.reserve(nodes.len().saturating_sub(positions.capacity()));
    for node in nodes {
        positions.push(node.world_pos);
    }

    cells.clear();
    let Some(quadtree) = QuadNode::build(positions) else {
        return;
    };

    collect_quadtree_cells(&quadtree, 0, cells);
}

pub(super) fn step_physics(cache: &mut RenderGraph, config: PhysicsConfig) -> bool {
    let node_count = cache.nodes.len();
    if node_count < 2 {
        return false;
    }

    let scratch = &mut cache.physics_scratch;
    scratch.forces.resize(node_count, Vec2::ZERO);
    scratch.forces.fill(Vec2::ZERO);
    scratch.positions.clear();
    scratch.radii.clear();
    scratch
        .positions
        .reserve(node_count.saturating_sub(scratch.positions.capacity()));
    scratch
        .radii
        .reserve(node_count.saturating_sub(scratch.radii.capacity()));
    let mut max_radius = 0.0_f32;
    for node in &cache.nodes {
        scratch.positions.push(node.world_pos);
        scratch.radii.push(node.base_radius);
        max_radius = max_radius.max(node.base_radius);
    }

    let forces = &mut scratch.forces;
    let positions = &scratch.positions;
    let radii = &scratch.radii;

    let intensity = config.intensity.clamp(0.2, 2.5);
    let repulsion_strength = 78_000.0 * intensity * config.repulsion_scale.clamp(0.25, 2.6);
    let spring_strength = 0.016 * intensity * config.spring_scale.clamp(0.2, 2.2);
    let spring_damping = 0.22;
    let collision_strength = 1.9 * intensity * config.collision_scale.clamp(0.2, 2.0);
    let center_pull = 0.0011 * intensity;
    let root_pull = 0.036 * intensity;
    let damping = (config.velocity_damping - (intensity * 0.015)).clamp(0.78, 0.97);
    let softening = 620.0;
    let time_step_scale = (config.delta_seconds * 60.0).clamp(0.25, 3.0);
    let damping_factor = damping.powf(time_step_scale);
    let root_index = cache.root_index.filter(|&index| index < node_count);

    if let Some(quadtree) = QuadNode::build(positions) {
        for (index, force) in forces.iter_mut().enumerate() {
            accumulate_repulsion_for_node(
                &quadtree,
                index,
                positions,
                repulsion_strength,
                softening,
                BARNES_HUT_THETA,
                force,
            );
        }

        let max_collision_distance = (max_radius * 2.0) * 4.2;
        if max_collision_distance > 0.0 {
            accumulate_collision_pairs(
                &quadtree,
                &quadtree,
                true,
                positions,
                radii,
                CollisionParams {
                    collision_strength,
                    max_collision_distance_sq: max_collision_distance * max_collision_distance,
                },
                forces,
            );
        }
    }

    for &(from, to) in &cache.edges {
        if from >= node_count || to >= node_count || from == to {
            continue;
        }

        let delta = cache.nodes[from].world_pos - cache.nodes[to].world_pos;
        let distance_sq = delta.length_sq();
        if distance_sq <= 0.0001 * 0.0001 {
            continue;
        }
        let distance = distance_sq.sqrt();
        let direction = delta / distance;

        let preferred = 96.0 + (cache.nodes[from].base_radius + cache.nodes[to].base_radius) * 4.0;
        let spring = (distance - preferred) * spring_strength;
        let relative_velocity = cache.nodes[from].velocity - cache.nodes[to].velocity;
        let damping_force = relative_velocity.dot(direction) * spring_damping;
        let correction = direction * (spring + damping_force);

        forces[from] -= correction;
        forces[to] += correction;
    }

    for (index, force) in forces.iter_mut().enumerate().take(node_count) {
        *force -= cache.nodes[index].world_pos * center_pull;
        if Some(index) == root_index {
            *force -= cache.nodes[index].world_pos * root_pull;
        }
    }

    let target_radius = (node_count as f32).sqrt() * 42.0 * config.target_spread.clamp(0.6, 2.0);
    let spread_force = config.spread_force.clamp(0.0, 0.08) * intensity;
    if spread_force > 0.0 {
        let mut radius_sum = 0.0;
        let mut radius_count = 0usize;
        for (index, node) in cache.nodes.iter().enumerate() {
            if Some(index) == root_index {
                continue;
            }
            radius_sum += node.world_pos.length();
            radius_count += 1;
        }

        if radius_count > 0 {
            let average_radius = radius_sum / radius_count as f32;
            let radius_error = average_radius - target_radius;
            let hard_limit = target_radius * 1.55;
            for (index, force) in forces.iter_mut().enumerate().take(node_count) {
                if Some(index) == root_index {
                    continue;
                }

                let position = cache.nodes[index].world_pos;
                let radius = position.length();
                let direction = if radius > 0.0001 {
                    position / radius
                } else {
                    let angle = ((index as f32) * 0.618_034 + 0.37) * std::f32::consts::TAU;
                    vec2(angle.cos(), angle.sin())
                };

                *force -= direction * radius_error * spread_force;
                if radius > hard_limit {
                    *force -= direction * (radius - hard_limit) * ((spread_force * 2.6) + 0.02);
                }
            }
        }
    }

    let max_force = 165.0 + (intensity * 90.0);
    let max_force_sq = max_force * max_force;
    let max_speed = 11.0 + (intensity * 15.0);
    let max_speed_sq = max_speed * max_speed;
    let min_sleep_speed_sq = 0.02 * 0.02;
    let min_sleep_force_sq = 0.08 * 0.08;
    let mut any_motion = false;
    let mut average_velocity = Vec2::ZERO;
    for (index, force_value) in forces.iter().enumerate().take(node_count) {
        let mut force = *force_value;
        let force_sq = force.length_sq();
        if force_sq > max_force_sq {
            force *= max_force / force_sq.sqrt();
        }

        let mut velocity =
            (cache.nodes[index].velocity + (force * (0.055 * time_step_scale))) * damping_factor;
        let mut speed_sq = velocity.length_sq();
        if speed_sq > max_speed_sq {
            velocity *= max_speed / speed_sq.sqrt();
            speed_sq = max_speed_sq;
        }

        if speed_sq < min_sleep_speed_sq && force_sq < min_sleep_force_sq {
            velocity = Vec2::ZERO;
            speed_sq = 0.0;
        }

        cache.nodes[index].velocity = velocity;
        average_velocity += velocity;
        cache.nodes[index].world_pos += velocity * time_step_scale;
        if speed_sq > 0.000_001 {
            any_motion = true;
        }
    }

    average_velocity /= node_count as f32;
    if average_velocity.length_sq() > 0.000_001 {
        for node in &mut cache.nodes {
            node.velocity -= average_velocity;
        }
    }

    let mut centroid = Vec2::ZERO;
    for node in &cache.nodes {
        centroid += node.world_pos;
    }
    centroid /= node_count as f32;
    if centroid.length_sq() > 0.000_001 {
        for node in &mut cache.nodes {
            node.world_pos -= centroid;
        }
    }

    any_motion
}
