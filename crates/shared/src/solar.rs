//! The scaled solar system.
//!
//! Scale factors (see the plan): planet radii 1:100, the Sun's radius 1:1000,
//! planet orbits 1:10,000, moon orbits 1:1000. Positions are frozen for the
//! prototype (no orbital motion); each body sits on a different angle around
//! the Sun so they are not all lined up.

use glam::DVec3;
use std::sync::LazyLock;

#[derive(Clone, Debug)]
pub struct Body {
    pub name: &'static str,
    /// Index of the body this one orbits (`None` for the Sun).
    pub parent: Option<usize>,
    pub radius: f64,
    /// Surface gravity in m/s².
    pub surface_gravity: f64,
    /// Gravity stops at `radius * GRAVITY_RANGE_FACTOR` from the centre.
    pub gravity_range: f64,
    pub position: DVec3,
    pub color: [f32; 3],
    pub emissive: bool,
}

const GRAVITY_RANGE_FACTOR: f64 = 6.0;

struct Def {
    name: &'static str,
    parent: Option<&'static str>,
    real_radius_km: f64,
    real_orbit_km: f64,
    angle_deg: f64,
    gravity: f64,
    color: [f32; 3],
}

const DEFS: &[Def] = &[
    Def { name: "Sun", parent: None, real_radius_km: 696_000.0, real_orbit_km: 0.0, angle_deg: 0.0, gravity: 27.4, color: [1.0, 0.85, 0.5] },
    Def { name: "Mercury", parent: Some("Sun"), real_radius_km: 2_440.0, real_orbit_km: 57.9e6, angle_deg: 40.0, gravity: 3.7, color: [0.55, 0.5, 0.45] },
    Def { name: "Venus", parent: Some("Sun"), real_radius_km: 6_052.0, real_orbit_km: 108.2e6, angle_deg: 110.0, gravity: 8.87, color: [0.9, 0.8, 0.55] },
    Def { name: "Earth", parent: Some("Sun"), real_radius_km: 6_371.0, real_orbit_km: 149.6e6, angle_deg: 0.0, gravity: 9.81, color: [0.3, 0.5, 0.35] },
    Def { name: "Moon", parent: Some("Earth"), real_radius_km: 1_737.0, real_orbit_km: 384_400.0, angle_deg: 30.0, gravity: 1.62, color: [0.6, 0.6, 0.6] },
    Def { name: "Mars", parent: Some("Sun"), real_radius_km: 3_390.0, real_orbit_km: 227.9e6, angle_deg: -35.0, gravity: 3.72, color: [0.75, 0.35, 0.2] },
    Def { name: "Jupiter", parent: Some("Sun"), real_radius_km: 69_911.0, real_orbit_km: 778.5e6, angle_deg: 200.0, gravity: 24.79, color: [0.8, 0.7, 0.55] },
    Def { name: "Saturn", parent: Some("Sun"), real_radius_km: 58_232.0, real_orbit_km: 1_432.0e6, angle_deg: 260.0, gravity: 10.44, color: [0.85, 0.78, 0.6] },
    Def { name: "Uranus", parent: Some("Sun"), real_radius_km: 25_362.0, real_orbit_km: 2_867.0e6, angle_deg: 300.0, gravity: 8.69, color: [0.6, 0.85, 0.9] },
    Def { name: "Neptune", parent: Some("Sun"), real_radius_km: 24_622.0, real_orbit_km: 4_515.0e6, angle_deg: 150.0, gravity: 11.15, color: [0.3, 0.45, 0.9] },
];

pub static BODIES: LazyLock<Vec<Body>> = LazyLock::new(|| {
    let mut out: Vec<Body> = Vec::new();
    for d in DEFS {
        let parent = d.parent.map(|p| out.iter().position(|b| b.name == p).unwrap());
        let radius = if parent.is_none() {
            d.real_radius_km * 1000.0 / 1000.0
        } else {
            d.real_radius_km * 1000.0 / 100.0
        };
        let orbit_scale = match parent {
            Some(p) if out[p].parent.is_some() => 1.0 / 1000.0, // moon
            _ => 1.0 / 10_000.0,
        };
        let orbit = d.real_orbit_km * 1000.0 * orbit_scale;
        let a = d.angle_deg.to_radians();
        // Orbits lie in the XZ plane, with a small tilt per body for variety.
        let local = DVec3::new(a.cos() * orbit, orbit * 0.01 * (a * 3.0).sin(), a.sin() * orbit);
        let position = parent.map(|p| out[p].position).unwrap_or(DVec3::ZERO) + local;
        out.push(Body {
            name: d.name,
            parent,
            radius,
            surface_gravity: d.gravity,
            gravity_range: radius * GRAVITY_RANGE_FACTOR,
            position,
            color: d.color,
            emissive: parent.is_none(),
        });
    }
    out
});

pub fn body_by_name(name: &str) -> Option<(usize, &'static Body)> {
    BODIES
        .iter()
        .enumerate()
        .find(|(_, b)| b.name.eq_ignore_ascii_case(name))
}

/// Gravity at a point: only the dominant body pulls.
#[derive(Clone, Copy, Debug)]
pub struct GravityInfo {
    pub accel: DVec3,
    /// Index of the dominant body, if any.
    pub body: Option<usize>,
    /// Height above the surface of the nearest body.
    pub altitude: f64,
    pub nearest: usize,
}

pub fn gravity_at(p: DVec3) -> GravityInfo {
    let mut best: Option<(usize, DVec3, f64)> = None;
    let mut nearest = (0usize, f64::INFINITY);
    for (i, b) in BODIES.iter().enumerate() {
        let d = b.position - p;
        let r = d.length();
        let alt = r - b.radius;
        if alt < nearest.1 {
            nearest = (i, alt);
        }
        if r > b.gravity_range || r < 1e-6 {
            continue;
        }
        let rr = r.max(b.radius);
        let g = b.surface_gravity * (b.radius / rr).powi(2);
        if best.map(|(_, _, bg)| g > bg).unwrap_or(true) {
            best = Some((i, d / r * g, g));
        }
    }
    GravityInfo {
        accel: best.map(|b| b.1).unwrap_or(DVec3::ZERO),
        body: best.map(|b| b.0),
        altitude: nearest.1,
        nearest: nearest.0,
    }
}

/// Unit direction from a body's centre to a sunlit point at mid latitude
/// (sun about 45 degrees above the horizon).
pub fn sunny_up(body: &Body) -> DVec3 {
    let to_sun = (BODIES[0].position - body.position).normalize_or(DVec3::X);
    (to_sun + DVec3::Y).normalize()
}

/// A spawn point just above Earth's surface, on the day side.
pub fn default_spawn() -> DVec3 {
    let (_, earth) = body_by_name("Earth").unwrap();
    earth.position + sunny_up(earth) * (earth.radius + 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_sane() {
        let sun = &BODIES[0];
        for b in BODIES.iter().skip(1) {
            let (_, parent) = (b.parent.unwrap(), &BODIES[b.parent.unwrap()]);
            let dist = (b.position - parent.position).length();
            assert!(dist > parent.radius + b.radius, "{} overlaps its parent", b.name);
            assert!((b.position - sun.position).length() > sun.radius * 2.0);
        }
        let g = gravity_at(default_spawn());
        assert_eq!(BODIES[g.body.unwrap()].name, "Earth");
        assert!((g.accel.length() - 9.81).abs() < 0.01);
        assert!(g.altitude > 2.0 && g.altitude < 4.0);
    }
}
