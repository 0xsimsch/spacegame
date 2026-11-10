//! The 24 axis-aligned orientations a block can have.
//!
//! Convention (same as Godot): local `-Z` is forward, `+Y` is up, `+X` is right.
//! Orientation index = `forward_index * 4 + up_variant`.

use glam::{DMat3, DQuat, DVec3, IVec3};
use std::sync::LazyLock;

pub const DIRS: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

pub const FORWARD: DVec3 = DVec3::NEG_Z;
pub const UP: DVec3 = DVec3::Y;
pub const RIGHT: DVec3 = DVec3::X;

pub const ORIENTATION_COUNT: u8 = 24;

struct Orientation {
    quat: DQuat,
    forward: IVec3,
    up: IVec3,
}

static TABLE: LazyLock<Vec<Orientation>> = LazyLock::new(|| {
    let mut out = Vec::with_capacity(24);
    for f in DIRS {
        let ups: Vec<IVec3> = DIRS.iter().copied().filter(|u| u.dot(f) == 0).collect();
        debug_assert_eq!(ups.len(), 4);
        for u in ups {
            let fwd = f.as_dvec3();
            let up = u.as_dvec3();
            let back = -fwd;
            let right = up.cross(back);
            let m = DMat3::from_cols(right, up, back);
            out.push(Orientation {
                quat: DQuat::from_mat3(&m).normalize(),
                forward: f,
                up: u,
            });
        }
    }
    out
});

/// Rotation of orientation `rot` relative to its grid.
pub fn quat(rot: u8) -> DQuat {
    TABLE[(rot % ORIENTATION_COUNT) as usize].quat
}

/// Grid-local forward axis (`-Z` of the block) of orientation `rot`.
pub fn forward(rot: u8) -> IVec3 {
    TABLE[(rot % ORIENTATION_COUNT) as usize].forward
}

/// Grid-local up axis of orientation `rot`.
pub fn up(rot: u8) -> IVec3 {
    TABLE[(rot % ORIENTATION_COUNT) as usize].up
}

/// Grid-local right axis of orientation `rot`.
pub fn right(rot: u8) -> IVec3 {
    up(rot).cross(-forward(rot))
}

/// Find the orientation whose forward/up match, if they are perpendicular axes.
pub fn from_axes(forward_axis: IVec3, up_axis: IVec3) -> Option<u8> {
    TABLE
        .iter()
        .position(|o| o.forward == forward_axis && o.up == up_axis)
        .map(|i| i as u8)
}

/// Snap an arbitrary rotation to the nearest of the 24 orientations.
pub fn nearest(q: DQuat) -> u8 {
    let f = q * FORWARD;
    let u = q * UP;
    let fa = snap_axis(f);
    let mut ua = snap_axis(u - f * f.dot(u));
    if ua.dot(fa) != 0 {
        ua = DIRS.iter().copied().find(|d| d.dot(fa) == 0).unwrap();
    }
    from_axes(fa, ua).unwrap_or(0)
}

/// The axis direction closest to `v`.
pub fn snap_axis(v: DVec3) -> IVec3 {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z {
        IVec3::new(v.x.signum() as i32, 0, 0)
    } else if a.y >= a.z {
        IVec3::new(0, v.y.signum() as i32, 0)
    } else {
        IVec3::new(0, 0, v.z.signum() as i32)
    }
}

/// Rotate an orientation so its forward moves to the next axis (keeps a valid up).
pub fn cycle_forward(rot: u8) -> u8 {
    let fi = (rot / 4 + 1) % 6;
    fi * 4
}

/// Rotate an orientation around its forward axis (next up variant).
pub fn cycle_up(rot: u8) -> u8 {
    (rot / 4) * 4 + (rot % 4 + 1) % 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        for r in 0..ORIENTATION_COUNT {
            let q = quat(r);
            assert!((q * FORWARD - forward(r).as_dvec3()).length() < 1e-9);
            assert!((q * UP - up(r).as_dvec3()).length() < 1e-9);
            assert!((q * RIGHT - right(r).as_dvec3()).length() < 1e-9);
            assert_eq!(nearest(q), r);
        }
        assert_eq!(quat(from_axes(IVec3::NEG_Z, IVec3::Y).unwrap()), DQuat::IDENTITY);
    }
}
