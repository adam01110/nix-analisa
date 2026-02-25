use std::f32::consts::TAU;

use eframe::egui::{Vec2, vec2};

use crate::util::stable_pair;

pub fn force_layout(
    node_ids: &[String],
    edges: &[(usize, usize)],
    node_radii: &[f32],
    iterations: usize,
) -> Vec<Vec2> {
    let n = node_ids.len();
    if n == 0 {
        return Vec::new();
    }

    let base_radius = (n as f32).sqrt() * 360.0;
    let mut positions = node_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let angle = (index as f32 / n as f32) * TAU;
            let (jx, jy) = stable_pair(id);
            let jitter = vec2(jx * 160.0, jy * 160.0);
            let radial = vec2(angle.cos(), angle.sin()) * base_radius;
            radial + jitter
        })
        .collect::<Vec<_>>();

    if n == 1 {
        return positions;
    }

    let area = (base_radius * 2.4).powi(2);
    let k = (area / n as f32).sqrt().max(24.0);
    let mut temperature = (k * 5.5).max(140.0);

    for _ in 0..iterations {
        let mut disp = vec![Vec2::ZERO; n];

        for i in 0..n {
            for j in (i + 1)..n {
                let delta = positions[i] - positions[j];
                let distance = delta.length().max(0.5);
                let direction = delta / distance;

                let ri = node_radii.get(i).copied().unwrap_or(6.0);
                let rj = node_radii.get(j).copied().unwrap_or(6.0);
                let min_distance = (ri + rj) * 4.2;

                let force = (k * k * (1.0 + (ri + rj) * 0.015)) / distance;
                disp[i] += direction * force;
                disp[j] -= direction * force;

                if distance < min_distance {
                    let overlap_push = (min_distance - distance) * 2.4;
                    disp[i] += direction * overlap_push;
                    disp[j] -= direction * overlap_push;
                }
            }
        }

        for &(from, to) in edges {
            if from >= n || to >= n || from == to {
                continue;
            }

            let delta = positions[from] - positions[to];
            let distance = delta.length().max(0.5);
            let direction = delta / distance;

            let rf = node_radii.get(from).copied().unwrap_or(6.0);
            let rt = node_radii.get(to).copied().unwrap_or(6.0);
            let ideal_length = k + (rf + rt) * 3.5;
            let force = (distance - ideal_length) * 0.18;

            disp[from] -= direction * force;
            disp[to] += direction * force;
        }

        for i in 0..n {
            disp[i] -= positions[i] * 0.0012;
        }

        for i in 0..n {
            let d = disp[i];
            let length = d.length();
            if length > 0.0 {
                positions[i] += d / length * length.min(temperature) * 0.92;
            }
        }

        temperature *= 0.965;
        if temperature < 0.55 {
            break;
        }
    }

    positions
}
