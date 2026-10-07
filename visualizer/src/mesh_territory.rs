//! Stable, progressively dispersed surface territories for observed member nodes.
use crate::mesh_model::MAX_NODES;
use glam::Vec3;
use std::sync::OnceLock;

const CANDIDATES: usize = 1024;
static ANCHORS: OnceLock<[Vec3; MAX_NODES]> = OnceLock::new();

pub fn anchor(worker: usize) -> Vec3 {
    ANCHORS.get_or_init(distribute)[worker]
}

fn distribute() -> [Vec3; MAX_NODES] {
    // Equal-area Fibonacci candidates. Greedily occupy the largest remaining
    // angular gap, so each prefix is well dispersed without moving earlier sites.
    let golden_angle = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
    let candidates: [Vec3; CANDIDATES] = std::array::from_fn(|i| {
        let y = 1.0 - 2.0 * (i as f32 + 0.5) / CANDIDATES as f32;
        let radius = (1.0 - y * y).sqrt();
        let angle = i as f32 * golden_angle;
        Vec3::new(radius * angle.cos(), y, radius * angle.sin())
    });
    let mut anchors = [Vec3::ZERO; MAX_NODES];
    let preferred = Vec3::new(0.42, 0.25, 0.87).normalize();
    for i in 0..MAX_NODES {
        let mut best = f32::INFINITY;
        for &candidate in &candidates {
            let score = if i == 0 {
                -candidate.dot(preferred)
            } else {
                anchors[..i]
                    .iter()
                    .map(|a| a.dot(candidate))
                    .fold(f32::NEG_INFINITY, f32::max)
            };
            if score < best {
                best = score;
                anchors[i] = candidate;
            }
        }
    }
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefixes_cover_both_sides_and_stay_separated() {
        for count in [2, 6, 12, MAX_NODES] {
            let points = (0..count).map(anchor).collect::<Vec<_>>();
            let centroid = points.iter().copied().sum::<Vec3>() / count as f32;
            assert!(centroid.length() < 0.25, "count {count}: {centroid}");
            assert!(points.iter().any(|p| p.z > 0.5));
            assert!(points.iter().any(|p| p.z < -0.5));
            for (i, a) in points.iter().enumerate() {
                assert!((a.length() - 1.0).abs() < 1e-5);
                for b in &points[i + 1..] {
                    assert!(
                        a.dot(*b) < if count > 24 { 0.99 } else { 0.87 },
                        "territories too close: {a} {b}"
                    );
                }
            }
        }
    }
    #[test]
    fn adding_or_removing_workers_does_not_repack_existing_sites() {
        let original = (0..6).map(anchor).collect::<Vec<_>>();
        for i in 6..MAX_NODES {
            let _ = anchor(i);
        }
        assert_eq!(original, (0..6).map(anchor).collect::<Vec<_>>());
    }
}
