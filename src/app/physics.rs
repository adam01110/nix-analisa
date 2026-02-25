use eframe::egui::{vec2, Vec2};

use super::{PhysicsConfig, RenderGraph};

const QUADTREE_LEAF_CAPACITY: usize = 12;
const QUADTREE_MAX_DEPTH: usize = 10;
const BARNES_HUT_THETA: f32 = 0.72;

#[derive(Clone, Copy)]
struct QuadBounds {
    center: Vec2,
    half_extent: f32,
}

impl QuadBounds {
    fn from_points(points: &[Vec2]) -> Option<Self> {
        let mut min = vec2(f32::INFINITY, f32::INFINITY);
        let mut max = vec2(f32::NEG_INFINITY, f32::NEG_INFINITY);

        for point in points {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }

        if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
            return None;
        }

        let center = (min + max) * 0.5;
        let span_x = (max.x - min.x).max(1.0);
        let span_y = (max.y - min.y).max(1.0);
        let half_extent = (span_x.max(span_y) * 0.5) + 1.0;

        Some(Self {
            center,
            half_extent,
        })
    }

    fn contains(self, point: Vec2) -> bool {
        let min = self.center - vec2(self.half_extent, self.half_extent);
        let max = self.center + vec2(self.half_extent, self.half_extent);
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }

    fn child(self, quadrant: usize) -> Self {
        let quarter = self.half_extent * 0.5;
        let offset = match quadrant {
            0 => vec2(-quarter, -quarter),
            1 => vec2(quarter, -quarter),
            2 => vec2(-quarter, quarter),
            _ => vec2(quarter, quarter),
        };

        Self {
            center: self.center + offset,
            half_extent: quarter,
        }
    }

    fn quadrant_for(self, point: Vec2) -> usize {
        let right = point.x >= self.center.x;
        let upper = point.y >= self.center.y;
        match (right, upper) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

    fn side_length(self) -> f32 {
        self.half_extent * 2.0
    }

    fn distance_sq_to(self, other: Self) -> f32 {
        let dx = (self.center.x - other.center.x).abs() - (self.half_extent + other.half_extent);
        let dy = (self.center.y - other.center.y).abs() - (self.half_extent + other.half_extent);
        let clamped_dx = dx.max(0.0);
        let clamped_dy = dy.max(0.0);
        (clamped_dx * clamped_dx) + (clamped_dy * clamped_dy)
    }
}

struct QuadNode {
    bounds: QuadBounds,
    center_of_mass: Vec2,
    mass: f32,
    indices: Vec<usize>,
    children: [Option<Box<QuadNode>>; 4],
}

pub(in crate::app) struct QuadtreeCell {
    pub center: Vec2,
    pub half_extent: f32,
    pub depth: usize,
    pub is_leaf: bool,
}

impl QuadNode {
    fn build(positions: &[Vec2]) -> Option<Self> {
        let bounds = QuadBounds::from_points(positions)?;
        let indices = (0..positions.len()).collect::<Vec<_>>();
        Some(Self::build_node(bounds, indices, positions, 0))
    }

    fn build_node(
        bounds: QuadBounds,
        indices: Vec<usize>,
        positions: &[Vec2],
        depth: usize,
    ) -> Self {
        let mut center_of_mass = Vec2::ZERO;
        for &index in &indices {
            center_of_mass += positions[index];
        }

        let mass = indices.len() as f32;
        if mass > 0.0 {
            center_of_mass /= mass;
        }

        let mut node = Self {
            bounds,
            center_of_mass,
            mass,
            indices,
            children: std::array::from_fn(|_| None),
        };

        if depth >= QUADTREE_MAX_DEPTH || node.indices.len() <= QUADTREE_LEAF_CAPACITY {
            return node;
        }

        let mut buckets = std::array::from_fn::<_, 4, _>(|_| Vec::new());
        for &index in &node.indices {
            let quadrant = bounds.quadrant_for(positions[index]);
            buckets[quadrant].push(index);
        }

        let non_empty = buckets.iter().filter(|bucket| !bucket.is_empty()).count();
        if non_empty <= 1 {
            return node;
        }

        for (quadrant, bucket) in buckets.into_iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }

            let child_bounds = bounds.child(quadrant);
            node.children[quadrant] = Some(Box::new(Self::build_node(
                child_bounds,
                bucket,
                positions,
                depth + 1,
            )));
        }
        node.indices.clear();
        node
    }

    fn is_leaf(&self) -> bool {
        self.children.iter().all(|child| child.is_none())
    }
}

fn collect_quadtree_cells(node: &QuadNode, depth: usize, cells: &mut Vec<QuadtreeCell>) {
    cells.push(QuadtreeCell {
        center: node.bounds.center,
        half_extent: node.bounds.half_extent,
        depth,
        is_leaf: node.is_leaf(),
    });

    for child in &node.children {
        if let Some(child) = child.as_ref() {
            collect_quadtree_cells(child, depth + 1, cells);
        }
    }
}

pub(in crate::app) fn quadtree_cells(cache: &RenderGraph) -> Vec<QuadtreeCell> {
    let positions = cache
        .nodes
        .iter()
        .map(|node| node.world_pos)
        .collect::<Vec<_>>();
    let Some(quadtree) = QuadNode::build(&positions) else {
        return Vec::new();
    };

    let mut cells = Vec::new();
    collect_quadtree_cells(&quadtree, 0, &mut cells);
    cells
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

fn accumulate_repulsion_for_node(
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

fn accumulate_collision_pairs(
    node_a: &QuadNode,
    node_b: &QuadNode,
    same_node: bool,
    positions: &[Vec2],
    radii: &[f32],
    collision_strength: f32,
    max_collision_distance_sq: f32,
    forces: &mut [Vec2],
) {
    if node_a.bounds.distance_sq_to(node_b.bounds) > max_collision_distance_sq {
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
                        let overlap_push = (min_distance - distance) * collision_strength;
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
                        let overlap_push = (min_distance - distance) * collision_strength;
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

            accumulate_collision_pairs(
                child_a,
                child_a,
                true,
                positions,
                radii,
                collision_strength,
                max_collision_distance_sq,
                forces,
            );

            for second in (first + 1)..4 {
                let Some(child_b) = node_a.children[second].as_ref() else {
                    continue;
                };
                accumulate_collision_pairs(
                    child_a,
                    child_b,
                    false,
                    positions,
                    radii,
                    collision_strength,
                    max_collision_distance_sq,
                    forces,
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
            accumulate_collision_pairs(
                child,
                node_b,
                false,
                positions,
                radii,
                collision_strength,
                max_collision_distance_sq,
                forces,
            );
        }
    } else {
        for child in &node_b.children {
            let Some(child) = child.as_ref() else {
                continue;
            };
            accumulate_collision_pairs(
                node_a,
                child,
                false,
                positions,
                radii,
                collision_strength,
                max_collision_distance_sq,
                forces,
            );
        }
    }
}

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

    let positions = cache
        .nodes
        .iter()
        .map(|node| node.world_pos)
        .collect::<Vec<_>>();
    let radii = cache
        .nodes
        .iter()
        .map(|node| node.base_radius)
        .collect::<Vec<_>>();

    if let Some(quadtree) = QuadNode::build(&positions) {
        for (index, force) in forces.iter_mut().enumerate() {
            accumulate_repulsion_for_node(
                &quadtree,
                index,
                &positions,
                repulsion_strength,
                softening,
                BARNES_HUT_THETA,
                force,
            );
        }

        let max_radius = radii.iter().copied().fold(0.0_f32, f32::max);
        let max_collision_distance = (max_radius * 2.0) * 4.2;
        if max_collision_distance > 0.0 {
            accumulate_collision_pairs(
                &quadtree,
                &quadtree,
                true,
                &positions,
                &radii,
                collision_strength,
                max_collision_distance * max_collision_distance,
                &mut forces,
            );
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
