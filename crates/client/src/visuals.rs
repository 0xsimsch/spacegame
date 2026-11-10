//! Meshes, materials and procedural geometry (planet surface patch, star sky).

use godot::classes::base_material_3d::{EmissionOperator, Feature, ShadingMode, Transparency};
use godot::classes::mesh::{ArrayType, PrimitiveType};
use godot::classes::{
    ArrayMesh, BoxMesh, CapsuleMesh, CylinderMesh, Shader, ShaderMaterial, SphereMesh, StandardMaterial3D,
};
use godot::prelude::*;
use shared::DVec3;
use shared::blocks::BlockKind;
use std::collections::HashMap;

pub fn material(color: [f32; 3], emissive: f32, alpha: f32) -> Gd<StandardMaterial3D> {
    let mut m = StandardMaterial3D::new_gd();
    m.set_albedo(Color::from_rgba(color[0], color[1], color[2], alpha));
    m.set_roughness(0.7);
    if alpha < 1.0 {
        m.set_transparency(Transparency::ALPHA);
        m.set_shading_mode(ShadingMode::UNSHADED);
    }
    if emissive > 0.0 {
        m.set_feature(Feature::EMISSION, true);
        m.set_emission(Color::from_rgb(color[0], color[1], color[2]));
        m.set_emission_energy_multiplier(emissive);
        m.set_emission_operator(EmissionOperator::ADD);
    }
    m
}

pub struct Visuals {
    pub cube: Gd<BoxMesh>,
    pub small_cube: Gd<BoxMesh>,
    pub cylinder: Gd<CylinderMesh>,
    pub nozzle: Gd<CylinderMesh>,
    pub sphere: Gd<SphereMesh>,
    pub capsule: Gd<CapsuleMesh>,
    pub block_mats: HashMap<BlockKind, Gd<StandardMaterial3D>>,
    pub dark: Gd<StandardMaterial3D>,
    pub screen: Gd<StandardMaterial3D>,
    pub window: Gd<StandardMaterial3D>,
    pub flame: Gd<StandardMaterial3D>,
    pub ghost_ok: Gd<StandardMaterial3D>,
    pub ghost_bad: Gd<StandardMaterial3D>,
    pub player: Gd<StandardMaterial3D>,
    pub visor: Gd<StandardMaterial3D>,
}

impl Visuals {
    pub fn new() -> Self {
        let mut cube = BoxMesh::new_gd();
        cube.set_size(Vector3::splat(0.98));
        let mut small_cube = BoxMesh::new_gd();
        small_cube.set_size(Vector3::splat(1.0));
        let mut cylinder = CylinderMesh::new_gd();
        cylinder.set_top_radius(0.5);
        cylinder.set_bottom_radius(0.5);
        cylinder.set_height(1.0);
        cylinder.set_radial_segments(16);
        let mut nozzle = CylinderMesh::new_gd();
        nozzle.set_top_radius(0.25);
        nozzle.set_bottom_radius(0.45);
        nozzle.set_height(1.0);
        nozzle.set_radial_segments(16);
        let mut sphere = SphereMesh::new_gd();
        sphere.set_radius(1.0);
        sphere.set_height(2.0);
        sphere.set_radial_segments(192);
        sphere.set_rings(96);
        let mut capsule = CapsuleMesh::new_gd();
        capsule.set_radius(0.4);
        capsule.set_height(1.8);
        let all = [
            BlockKind::Armor,
            BlockKind::Thruster,
            BlockKind::Piston,
            BlockKind::PistonTop,
            BlockKind::Hinge,
            BlockKind::HingeTop,
            BlockKind::Programmable,
            BlockKind::Cockpit,
        ];
        let block_mats = all.iter().map(|k| (*k, material(k.color(), 0.0, 1.0))).collect();
        Self {
            cube,
            small_cube,
            cylinder,
            nozzle,
            sphere,
            capsule,
            block_mats,
            dark: material([0.08, 0.08, 0.1], 0.0, 1.0),
            screen: material([0.1, 0.9, 0.4], 2.0, 1.0),
            window: material([0.2, 0.5, 0.9], 0.6, 1.0),
            flame: material([1.0, 0.55, 0.15], 4.0, 1.0),
            ghost_ok: material([0.3, 0.8, 1.0], 0.0, 0.35),
            ghost_bad: material([1.0, 0.3, 0.2], 0.0, 0.35),
            player: material([0.85, 0.45, 0.15], 0.0, 1.0),
            visor: material([0.1, 0.15, 0.3], 0.5, 1.0),
        }
    }
}

/// Rebuild the high-resolution surface patch of a planet around the camera.
///
/// `body_rel` is the planet centre relative to the camera; `sub` the unit
/// direction from the centre towards the camera; `half` the patch half-size in
/// metres. Vertices are generated directly in camera-relative coordinates, so
/// there is no precision loss no matter how big the planet is.
pub fn build_surface_patch(mesh: &mut Gd<ArrayMesh>, body_rel: DVec3, radius: f64, sub: DVec3, half: f64) {
    const N: usize = 97;
    let e1 = sub.any_orthonormal_vector();
    let e2 = sub.cross(e1);
    let mut verts = PackedVector3Array::new();
    let mut normals = PackedVector3Array::new();
    verts.resize(N * N);
    normals.resize(N * N);
    for j in 0..N {
        for i in 0..N {
            // Quadratic spacing: dense near the camera, coarse at the edge.
            let s = |k: usize| {
                let x = k as f64 / (N - 1) as f64 * 2.0 - 1.0;
                x * x.abs() * half
            };
            let dir = (sub * radius + e1 * s(i) + e2 * s(j)).normalize();
            let p = body_rel + dir * radius;
            verts[j * N + i] = Vector3::new(p.x as f32, p.y as f32, p.z as f32);
            normals[j * N + i] = Vector3::new(dir.x as f32, dir.y as f32, dir.z as f32);
        }
    }
    let mut idx = PackedInt32Array::new();
    for j in 0..N - 1 {
        for i in 0..N - 1 {
            let a = (j * N + i) as i32;
            let b = a + 1;
            let c = a + N as i32;
            let d = c + 1;
            idx.extend([a, c, b, b, c, d]);
        }
    }
    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::NORMAL.ord() as usize, &normals.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &idx.to_variant());
    mesh.clear_surfaces();
    mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
}

const SKY_SHADER: &str = r#"
shader_type sky;

float hash(vec3 p) {
    p = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3));
    p *= 17.0;
    return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

void sky() {
    vec3 d = normalize(EYEDIR);
    vec3 cell = floor(d * 300.0);
    float h = hash(cell);
    float star = step(0.9965, h);
    vec3 local = fract(d * 300.0) - 0.5;
    float falloff = smoothstep(0.5, 0.0, length(local));
    float tint = hash(cell + 7.0);
    vec3 col = mix(vec3(1.0, 0.85, 0.7), vec3(0.7, 0.8, 1.0), tint);
    float band = exp(-pow(d.y * 3.0, 2.0)) * 0.025;
    COLOR = col * star * falloff * (0.6 + 1.4 * hash(cell + 3.0)) + vec3(0.6, 0.6, 0.8) * band;
}
"#;

pub fn sky_material() -> Gd<ShaderMaterial> {
    let mut shader = Shader::new_gd();
    shader.set_code(SKY_SHADER);
    let mut m = ShaderMaterial::new_gd();
    m.set_shader(&shader);
    m
}
