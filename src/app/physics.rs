use eframe::egui::{Vec2, vec2};

use super::{PhysicsConfig, RenderGraph};

pub(super) fn step_physics(cache: &mut RenderGraph, config: PhysicsConfig) {
    let node_count = cache.nodes.len();
    if node_count < 2 {
        return;
    }

    let mut forces = vec![Vec2::ZERO; node_count];
    let intensity = config.intensity.clamp(0.2, 2.5);
    let repulsion_strength = 78_000.0 * intensity * config.repulsion_scale.clamp(0.25, 2.6);
    let spring_strength = 0.016 * intensity * config.spring_scale.clamp(0.2, 2.2);
    let spring_damping = 0.22;
    let collision_strength = 1.9 * intensity * config.collision_scale.clamp(0.2, 2.0);
    let center_pull = 0.00055;
    let root_pull = 0.022;
    let damping = (config.velocity_damping - (intensity * 0.015)).clamp(0.78, 0.97);
    let softening = 620.0;

    for i in 0..node_count {
        for j in (i + 1)..node_count {
            let delta = cache.nodes[i].world_pos - cache.nodes[j].world_pos;
            let distance_sq = delta.length_sq();
            let distance = distance_sq.sqrt();
            let direction = if distance > 0.0001 {
                delta / distance
            } else {
                let angle =
                    ((i as f32) * 0.618_034 + (j as f32) * 0.414_214) * std::f32::consts::TAU;
                vec2(angle.cos(), angle.sin())
            };

            let min_distance = (cache.nodes[i].base_radius + cache.nodes[j].base_radius) * 4.2;
            let repulsion = repulsion_strength / (distance_sq + softening);

            forces[i] += direction * repulsion;
            forces[j] -= direction * repulsion;

            if distance < min_distance {
                let overlap_push = (min_distance - distance) * collision_strength;
                forces[i] += direction * overlap_push;
                forces[j] -= direction * overlap_push;
            }
        }
    }

    for &(from, to) in &cache.edges {
        if from >= node_count || to >= node_count || from == to {
            continue;
        }

        let delta = cache.nodes[from].world_pos - cache.nodes[to].world_pos;
        let distance = delta.length();
        if distance <= 0.0001 {
            continue;
        }
        let direction = delta / distance;

        let preferred = 96.0 + (cache.nodes[from].base_radius + cache.nodes[to].base_radius) * 4.0;
        let spring = (distance - preferred) * spring_strength;
        let relative_velocity = cache.nodes[from].velocity - cache.nodes[to].velocity;
        let damping_force = relative_velocity.dot(direction) * spring_damping;
        let correction = direction * (spring + damping_force);

        forces[from] -= correction;
        forces[to] += correction;
    }

    for index in 0..node_count {
        forces[index] -= cache.nodes[index].world_pos * center_pull;
    }

    let target_radius = (node_count as f32).sqrt() * 42.0 * config.target_spread.clamp(0.6, 2.0);
    let spread_force = config.spread_force.clamp(0.0, 0.08) * intensity;
    if spread_force > 0.0 {
        let root_index = cache.root_index.filter(|index| *index < node_count);
        let mut radius_sum = 0.0;
        let mut radius_count = 0usize;
        for (index, node) in cache.nodes.iter().enumerate() {
            if root_index.is_some_and(|root| root == index) {
                continue;
            }
            radius_sum += node.world_pos.length();
            radius_count += 1;
        }

        if radius_count > 0 {
            let average_radius = radius_sum / radius_count as f32;
            let radius_error = average_radius - target_radius;
            let hard_limit = target_radius * 1.55;
            for index in 0..node_count {
                if root_index.is_some_and(|root| root == index) {
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

                forces[index] -= direction * radius_error * spread_force;
                if radius > hard_limit {
                    forces[index] -=
                        direction * (radius - hard_limit) * ((spread_force * 2.6) + 0.02);
                }
            }
        }
    }

    if let Some(root_index) = cache.root_index {
        if root_index < node_count {
            forces[root_index] -= cache.nodes[root_index].world_pos * root_pull;
        }
    }

    let max_force = 165.0 + (intensity * 90.0);
    let max_speed = 11.0 + (intensity * 15.0);
    for index in 0..node_count {
        let mut force = forces[index];
        let mut force_magnitude = force.length();
        if force_magnitude > max_force {
            force = force / force_magnitude * max_force;
            force_magnitude = max_force;
        }

        let mut velocity = (cache.nodes[index].velocity + (force * 0.055)) * damping;
        let mut speed = velocity.length();
        if speed > max_speed {
            velocity = velocity / speed * max_speed;
            speed = max_speed;
        }

        if speed < 0.02 && force_magnitude < 0.08 {
            velocity = Vec2::ZERO;
        }

        cache.nodes[index].velocity = velocity;
        cache.nodes[index].world_pos += velocity;
    }
}
