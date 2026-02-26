use eframe::egui::{Vec2, vec2};

const QUADTREE_LEAF_CAPACITY: usize = 12;
const QUADTREE_MAX_DEPTH: usize = 10;

#[derive(Clone, Copy)]
pub(super) struct QuadBounds {
    pub(super) center: Vec2,
    pub(super) half_extent: f32,
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

    pub(super) fn contains(self, point: Vec2) -> bool {
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

    pub(super) fn side_length(self) -> f32 {
        self.half_extent * 2.0
    }

    pub(super) fn distance_sq_to(self, other: Self) -> f32 {
        let dx = (self.center.x - other.center.x).abs() - (self.half_extent + other.half_extent);
        let dy = (self.center.y - other.center.y).abs() - (self.half_extent + other.half_extent);
        let clamped_dx = dx.max(0.0);
        let clamped_dy = dy.max(0.0);
        (clamped_dx * clamped_dx) + (clamped_dy * clamped_dy)
    }
}

pub(super) struct QuadNode {
    pub(super) bounds: QuadBounds,
    pub(super) center_of_mass: Vec2,
    pub(super) mass: f32,
    pub(super) indices: Vec<usize>,
    pub(super) children: [Option<Box<QuadNode>>; 4],
}

pub(in crate::app) struct QuadtreeCell {
    pub center: Vec2,
    pub half_extent: f32,
    pub depth: usize,
    pub is_leaf: bool,
}

impl QuadNode {
    pub(super) fn build(positions: &[Vec2]) -> Option<Self> {
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

    pub(super) fn is_leaf(&self) -> bool {
        self.children.iter().all(|child| child.is_none())
    }
}

pub(super) fn collect_quadtree_cells(node: &QuadNode, depth: usize, cells: &mut Vec<QuadtreeCell>) {
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
