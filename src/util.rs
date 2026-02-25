use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn short_name(id: &str) -> &str {
    id.split_once('-').map(|(_, rest)| rest).unwrap_or(id)
}

pub fn stable_pair(id: &str) -> (f32, f32) {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let hash = hasher.finish();

    let x = ((hash & 0xffff_ffff) as f64 / u32::MAX as f64) as f32;
    let y = (((hash >> 32) & 0xffff_ffff) as f64 / u32::MAX as f64) as f32;
    ((x * 2.0) - 1.0, (y * 2.0) - 1.0)
}
