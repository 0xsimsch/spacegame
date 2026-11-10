//! The `SpaceGame` Godot node: connection, input, rendering and the UI bridge.

use crate::net::{Embedded, Net};
use crate::state::{ClientState, RayHit, UiEvent};
use crate::visuals::{self, Visuals};
use godot::classes::environment::{AmbientSource, BgMode};
use godot::classes::geometry_instance_3d::ShadowCastingSetting;
use godot::classes::input::MouseMode;
use godot::classes::{
    ArrayMesh, Camera3D, DirectionalLight3D, Environment, INode3D, Input, InputEvent, InputEventKey,
    InputEventMouseButton, InputEventMouseMotion, Label3D, Mesh, MeshInstance3D, Node3D, ProjectSettings, Sky,
    WorldEnvironment,
};
use godot::global::{Key, MouseButton};
use godot::prelude::*;
use shared::blocks::{BUILDABLE, BlockKind};
use shared::orient;
use shared::protocol::{BlockProp, ClientMsg, PlayerInput};
use shared::solar::{BODIES, gravity_at};
use shared::{BlockId, DQuat, DVec3, GridId, IVec3, PlayerId};
use std::collections::HashMap;
use std::time::Duration;

const LOOK_SENS: f64 = 0.0025;
const REACH: f64 = 10.0;
/// Planet centres farther than this are drawn scaled down about the camera
/// ("scaled space"), which keeps their apparent size but fits the depth range.
const SCALED_SPACE_DIST: f64 = 150_000.0;
/// Below this altitude the nearest body gets a high-resolution surface patch.
const PATCH_ALTITUDE: f64 = 40_000.0;
const CAMERA_FAR: f32 = 400_000.0;

fn gs(s: impl AsRef<str>) -> GString {
    GString::from(s.as_ref())
}

fn v3(v: DVec3) -> Vector3 {
    Vector3::new(v.x as f32, v.y as f32, v.z as f32)
}
fn quat(q: DQuat) -> Quaternion {
    Quaternion::new(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}

struct BlockNode {
    flame: Option<Gd<Node3D>>,
    shaft: Option<Gd<Node3D>>,
    door: Option<Gd<Node3D>>,
}

struct GridNode {
    root: Gd<Node3D>,
    blocks: HashMap<BlockId, BlockNode>,
}

#[derive(GodotClass)]
#[class(base=Node3D)]
pub struct SpaceGame {
    base: Base<Node3D>,
    net: Option<Net>,
    embedded: Option<Embedded>,
    st: ClientState,
    vis: Option<Visuals>,
    camera: Option<Gd<Camera3D>>,
    sun_light: Option<Gd<DirectionalLight3D>>,
    grid_nodes: HashMap<GridId, GridNode>,
    player_nodes: HashMap<PlayerId, Gd<Node3D>>,
    planet_nodes: Vec<Gd<MeshInstance3D>>,
    patch: Option<Gd<MeshInstance3D>>,
    patch_mesh: Option<Gd<ArrayMesh>>,
    ghost: Option<Gd<MeshInstance3D>>,
    look: DQuat,
    seq: u32,
    mouse_delta: Vector2,
    jetpack: bool,
    dampeners: bool,
    cruise: bool,
    build_mode: bool,
    build_kind: usize,
    build_rot: u8,
    build_static: bool,
    ui_open: bool,
    last_hit: Option<RayHit>,
    input_accum: f64,
    status: String,
    cam_world: DVec3,
    show_stats: bool,
    fps: f64,
}

#[godot_api]
impl INode3D for SpaceGame {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            base,
            net: None,
            embedded: None,
            st: ClientState::default(),
            vis: None,
            camera: None,
            sun_light: None,
            grid_nodes: HashMap::new(),
            player_nodes: HashMap::new(),
            planet_nodes: Vec::new(),
            patch: None,
            patch_mesh: None,
            ghost: None,
            look: DQuat::IDENTITY,
            seq: 0,
            mouse_delta: Vector2::ZERO,
            jetpack: true,
            dampeners: true,
            cruise: false,
            build_mode: false,
            build_kind: 0,
            build_rot: orient::from_axes(IVec3::NEG_Z, IVec3::Y).unwrap(),
            build_static: false,
            ui_open: true,
            last_hit: None,
            input_accum: 0.0,
            status: "Not connected".into(),
            cam_world: DVec3::ZERO,
            show_stats: false,
            fps: 60.0,
        }
    }

    fn ready(&mut self) {
        let vis = Visuals::new();
        // Camera.
        let mut cam = Camera3D::new_alloc();
        cam.set_near(0.05);
        cam.set_far(CAMERA_FAR);
        cam.set_fov(75.0);
        self.base_mut().add_child(&cam);
        cam.make_current();
        self.camera = Some(cam);
        // Sunlight + environment.
        let mut sun = DirectionalLight3D::new_alloc();
        sun.set_param(godot::classes::light_3d::Param::ENERGY, 1.6);
        sun.set_shadow(true);
        self.base_mut().add_child(&sun);
        self.sun_light = Some(sun);
        let mut env = Environment::new_gd();
        let mut sky = Sky::new_gd();
        sky.set_material(&visuals::sky_material());
        env.set_sky(&sky);
        env.set_background(BgMode::SKY);
        env.set_ambient_source(AmbientSource::COLOR);
        env.set_ambient_light_color(Color::from_rgb(0.25, 0.27, 0.32));
        env.set_ambient_light_energy(1.0);
        let mut we = WorldEnvironment::new_alloc();
        we.set_environment(&env);
        self.base_mut().add_child(&we);
        // Planets.
        for b in BODIES.iter() {
            let mut mi = MeshInstance3D::new_alloc();
            mi.set_mesh(&vis.sphere);
            let mat = visuals::material(b.color, if b.emissive { 3.0 } else { 0.0 }, 1.0);
            if b.emissive {
                let mut m = mat.clone();
                m.set_shading_mode(godot::classes::base_material_3d::ShadingMode::UNSHADED);
            }
            mi.set_material_override(&mat);
            mi.set_cast_shadows_setting(ShadowCastingSetting::OFF);
            mi.set_name(b.name);
            self.base_mut().add_child(&mi);
            self.planet_nodes.push(mi);
        }
        let patch_mesh = ArrayMesh::new_gd();
        let mut patch = MeshInstance3D::new_alloc();
        patch.set_mesh(&patch_mesh);
        patch.set_cast_shadows_setting(ShadowCastingSetting::OFF);
        patch.set_visible(false);
        self.base_mut().add_child(&patch);
        self.patch = Some(patch);
        self.patch_mesh = Some(patch_mesh);
        let mut ghost = MeshInstance3D::new_alloc();
        ghost.set_mesh(&vis.small_cube);
        ghost.set_visible(false);
        self.base_mut().add_child(&ghost);
        self.ghost = Some(ghost);
        self.vis = Some(vis);
    }

    fn process(&mut self, delta: f64) {
        self.fps = self.fps * 0.95 + (1.0 / delta.max(1e-4)) * 0.05;
        self.pump_network(delta);
        if self.st.me.is_none() {
            if let Some(n) = &mut self.net {
                n.flush();
            }
            return;
        }
        self.st.advance(delta);
        self.update_input(delta);
        self.update_camera();
        self.update_world_render();
        self.update_build_ghost();
        if let Some(n) = &mut self.net {
            n.flush();
        }
    }

    fn unhandled_input(&mut self, event: Gd<InputEvent>) {
        if self.st.me.is_none() || self.ui_open {
            return;
        }
        if let Ok(m) = event.clone().try_cast::<InputEventMouseMotion>() {
            if Input::singleton().get_mouse_mode() == MouseMode::CAPTURED {
                self.mouse_delta += m.get_relative();
            }
            return;
        }
        if let Ok(b) = event.clone().try_cast::<InputEventMouseButton>() {
            if b.is_pressed() {
                if Input::singleton().get_mouse_mode() != MouseMode::CAPTURED {
                    Input::singleton().set_mouse_mode(MouseMode::CAPTURED);
                    return;
                }
                match b.get_button_index() {
                    MouseButton::LEFT if self.build_mode => self.place(),
                    MouseButton::RIGHT if self.build_mode => self.remove(),
                    MouseButton::WHEEL_UP if self.build_mode => self.build_kind = (self.build_kind + BUILDABLE.len() - 1) % BUILDABLE.len(),
                    MouseButton::WHEEL_DOWN if self.build_mode => self.build_kind = (self.build_kind + 1) % BUILDABLE.len(),
                    _ => {}
                }
            }
            return;
        }
        if let Ok(k) = event.try_cast::<InputEventKey>() {
            if !k.is_pressed() || k.is_echo() {
                return;
            }
            match k.get_keycode() {
                Key::G => self.build_mode = !self.build_mode,
                Key::J => self.jetpack = !self.jetpack,
                Key::Z => self.dampeners = !self.dampeners,
                Key::X => self.cruise = !self.cruise,
                Key::B => self.build_static = !self.build_static,
                Key::R => self.build_rot = orient::cycle_forward(self.build_rot),
                Key::T => self.build_rot = orient::cycle_up(self.build_rot),
                Key::F => self.interact(),
                Key::K => self.open_panel_for_target(),
                k @ (Key::KEY_1 | Key::KEY_2 | Key::KEY_3 | Key::KEY_4 | Key::KEY_5 | Key::KEY_6 | Key::KEY_7 | Key::KEY_8 | Key::KEY_9) => {
                    self.build_kind = (k.ord() - Key::KEY_1.ord()) as usize;
                    self.build_mode = true;
                }
                _ => {}
            }
        }
    }
}

impl SpaceGame {
    fn send(&mut self, msg: ClientMsg) {
        if let Some(n) = &mut self.net {
            n.send(&msg);
        }
    }

    fn pump_network(&mut self, delta: f64) {
        let Some(net) = &mut self.net else { return };
        match net.update(Duration::from_secs_f64(delta)) {
            Ok(msgs) => {
                for m in msgs {
                    self.st.apply(m);
                }
            }
            Err(e) => {
                self.st.ui_events.push(UiEvent::Disconnected(e));
                self.shutdown();
                return;
            }
        }
        let net = self.net.as_ref().unwrap();
        if let Some(reason) = net.disconnect_reason() {
            self.st.ui_events.push(UiEvent::Disconnected(reason));
            self.shutdown();
        } else if net.is_connected() {
            self.status = format!("Connected ({:.0} ms)", net.rtt_ms());
        }
    }

    fn shutdown(&mut self) {
        if let Some(mut n) = self.net.take() {
            n.disconnect();
            n.flush();
        }
        self.embedded = None;
        let events = std::mem::take(&mut self.st.ui_events);
        self.st = ClientState::default();
        self.st.ui_events = events;
        for (_, mut g) in self.grid_nodes.drain() {
            g.root.queue_free();
        }
        for (_, mut p) in self.player_nodes.drain() {
            p.queue_free();
        }
        self.status = "Not connected".into();
        Input::singleton().set_mouse_mode(MouseMode::VISIBLE);
    }

    fn gravity_up(&self, pos: DVec3) -> Option<DVec3> {
        let g = gravity_at(pos).accel;
        (g.length() > 0.05).then(|| -g.normalize())
    }

    // --------------------------------------------------------------- input

    fn update_input(&mut self, delta: f64) {
        let input = Input::singleton();
        let captured = input.get_mouse_mode() == MouseMode::CAPTURED;
        let active = !self.ui_open && captured;
        let key = |k: Key| if active && input.is_key_pressed(k) { 1.0f32 } else { 0.0 };
        let movement = shared::Vec3::new(
            key(Key::D) - key(Key::A),
            key(Key::SPACE) - key(Key::C).max(key(Key::CTRL)),
            key(Key::S) - key(Key::W),
        );
        let md = std::mem::take(&mut self.mouse_delta);
        let seated = self.st.my_seat();
        let mut rotation = shared::Vec3::ZERO;
        if let Some(seat) = seated {
            rotation = shared::Vec3::new(
                (-md.y as f32 * 0.04).clamp(-1.0, 1.0),
                (-md.x as f32 * 0.04).clamp(-1.0, 1.0),
                key(Key::Q) - key(Key::E),
            );
            if let Some(p) = self.st.block_world_pose(seat) {
                self.look = p.rot;
            }
        } else {
            let pos = self.st.my_render_pos().unwrap_or_default();
            let up = self.gravity_up(pos);
            let local_up = self.look * orient::UP;
            let yaw_axis = up.unwrap_or(local_up);
            let yaw = DQuat::from_axis_angle(yaw_axis, -md.x as f64 * LOOK_SENS);
            let right = self.look * orient::RIGHT;
            let pitch = DQuat::from_axis_angle(right, -md.y as f64 * LOOK_SENS);
            let mut look = (yaw * pitch * self.look).normalize();
            if let Some(up) = up {
                // Keep the horizon level and stop pitch at the poles.
                let fwd = look * orient::FORWARD;
                if fwd.dot(up).abs() > 0.995 {
                    look = (yaw * self.look).normalize();
                }
                let fwd = look * orient::FORWARD;
                let back = -fwd;
                let right = up.cross(back).normalize();
                let up2 = back.cross(right);
                let target = DQuat::from_mat3(&glam::DMat3::from_cols(right, up2, back)).normalize();
                look = look.slerp(target, (delta * 4.0).min(1.0));
            } else {
                let roll = (key(Key::Q) - key(Key::E)) as f64 * delta * 1.5;
                look = (look * DQuat::from_rotation_z(roll)).normalize();
            }
            self.look = look;
        }
        // Send at most 60 inputs per second; predict every frame.
        self.seq += 1;
        let pi = PlayerInput {
            seq: self.seq,
            movement,
            rotation,
            look: self.look,
            dampeners: self.dampeners,
            jetpack: self.jetpack,
            cruise: self.cruise,
        };
        self.st.predict(&pi, delta);
        self.input_accum += delta;
        if self.input_accum >= 1.0 / 60.0 || rotation != shared::Vec3::ZERO {
            self.input_accum = 0.0;
            self.send(ClientMsg::Input(pi));
        }
        if !self.ui_open && !captured && self.st.me.is_some() {
            // Keep mouse captured during play unless the UI asked otherwise.
        }
    }

    fn target(&self) -> Option<RayHit> {
        self.st.raycast(self.cam_world, self.look * orient::FORWARD, REACH)
    }

    fn place(&mut self) {
        let kind = BUILDABLE[self.build_kind];
        match self.target() {
            Some(hit) => {
                let cell = hit.cell + hit.normal;
                self.send(ClientMsg::PlaceBlock { grid: hit.grid, cell, rot: self.build_rot, kind });
            }
            None => {
                let pos = self.cam_world + self.look * orient::FORWARD * 4.0;
                let rot = self.look * orient::quat(self.build_rot).inverse() * orient::quat(orient::from_axes(IVec3::NEG_Z, IVec3::Y).unwrap());
                let rot = self.snap_new_grid_rot(pos, rot);
                self.send(ClientMsg::NewGrid { pos, rot, kind, is_static: self.build_static });
            }
        }
    }

    /// New grids align to the planet's "up" when in gravity.
    fn snap_new_grid_rot(&self, pos: DVec3, rot: DQuat) -> DQuat {
        match self.gravity_up(pos) {
            Some(up) => {
                let fwd = rot * orient::FORWARD;
                let fwd = (fwd - up * fwd.dot(up)).try_normalize().unwrap_or(up.any_orthonormal_vector());
                let back = -fwd;
                let right = up.cross(back).normalize();
                DQuat::from_mat3(&glam::DMat3::from_cols(right, up, back)).normalize()
            }
            None => rot,
        }
    }

    fn remove(&mut self) {
        if let Some(hit) = self.target() {
            self.send(ClientMsg::RemoveBlock { grid: hit.grid, cell: hit.cell });
        }
    }

    fn interact(&mut self) {
        if self.st.my_seat().is_some() {
            self.send(ClientMsg::ExitSeat);
            return;
        }
        let Some(hit) = self.target() else { return };
        match hit.kind {
            BlockKind::Cockpit => self.send(ClientMsg::EnterSeat { block: hit.block }),
            BlockKind::Door => {
                let open = self.st.values.get(&hit.block).copied().unwrap_or(0.0) > 0.5;
                self.send(ClientMsg::SetBlockProp { block: hit.block, prop: BlockProp::DoorOpen(!open) });
            }
            BlockKind::Programmable => {
                self.send(ClientMsg::RequestScript { block: hit.block });
                self.st.ui_events.push(UiEvent::OpenEditor { block: hit.block });
            }
            k if k.is_terminal() => self.st.ui_events.push(UiEvent::OpenPanel { grid: hit.grid, block: Some(hit.block) }),
            _ => self.st.ui_events.push(UiEvent::OpenPanel { grid: hit.grid, block: None }),
        }
    }

    fn open_panel_for_target(&mut self) {
        if let Some(seat) = self.st.my_seat() {
            if let Some((g, _)) = self.st.block(seat) {
                self.st.ui_events.push(UiEvent::OpenPanel { grid: g, block: None });
            }
            return;
        }
        if let Some(hit) = self.target() {
            self.st.ui_events.push(UiEvent::OpenPanel { grid: hit.grid, block: Some(hit.block) });
        }
    }

    // -------------------------------------------------------------- render

    fn update_camera(&mut self) {
        let seat = self.st.my_seat();
        let (pos, rot) = match seat.and_then(|s| self.st.block_world_pose(s)) {
            Some(p) => (p.pos + p.rot * DVec3::new(0.0, 0.25, 0.0), p.rot),
            None => {
                let p = self.st.my_render_pos().unwrap_or_default();
                let up = self.gravity_up(p).unwrap_or(self.look * orient::UP);
                (p + up * 0.6, self.look)
            }
        };
        self.cam_world = pos;
        if let Some(cam) = &mut self.camera {
            cam.set_position(Vector3::ZERO);
            cam.set_quaternion(quat(rot));
        }
    }

    fn update_world_render(&mut self) {
        let cam = self.cam_world;
        // Sun light points from the Sun to the camera.
        if let Some(sun) = &mut self.sun_light {
            let d = (cam - BODIES[0].position).normalize_or(DVec3::NEG_Y);
            let rot = DQuat::from_rotation_arc(orient::FORWARD, d);
            sun.set_quaternion(quat(rot));
        }
        self.update_planets(cam);
        // Removed grids.
        for id in std::mem::take(&mut self.st.removed_grids) {
            if let Some(mut g) = self.grid_nodes.remove(&id) {
                g.root.queue_free();
            }
        }
        // Rebuild grids whose block list changed.
        let dirty: Vec<GridId> = self.st.grids.values().filter(|g| g.dirty).map(|g| g.id).collect();
        for id in dirty {
            self.rebuild_grid(id);
            self.st.grids.get_mut(&id).unwrap().dirty = false;
        }
        // Grid transforms and animated parts.
        let ids: Vec<GridId> = self.grid_nodes.keys().copied().collect();
        for id in ids {
            let Some(pose) = self.st.grid_pose(id) else { continue };
            let node = self.grid_nodes.get_mut(&id).unwrap();
            node.root.set_position(v3(pose.pos - cam));
            node.root.set_quaternion(quat(pose.rot));
            for (bid, bn) in node.blocks.iter_mut() {
                let v = self.st.values.get(bid).copied().unwrap_or(0.0);
                if let Some(f) = &mut bn.flame {
                    f.set_visible(v > 0.01);
                    let len = 0.2 + v * 1.4;
                    f.set_scale(Vector3::new(0.35, len, 0.35));
                    f.set_position(Vector3::new(0.0, 0.0, 0.5 + len * 0.5));
                }
                if let Some(d) = &mut bn.door {
                    // value 1 = open: the panel tucks into the top of the frame.
                    let open = v > 0.5;
                    d.set_position(Vector3::new(0.0, if open { 0.42 } else { 0.0 }, 0.0));
                    d.set_scale(Vector3::new(0.82, if open { 0.08 } else { 0.82 }, 0.2));
                }
                if let Some(s) = &mut bn.shaft {
                    let len = 1.0 + v.max(0.0);
                    s.set_scale(Vector3::new(0.3, len, 0.3));
                    s.set_position(Vector3::new(0.0, 0.0, -len * 0.5));
                }
            }
        }
        self.update_players(cam);
    }

    fn update_planets(&mut self, cam: DVec3) {
        let mut near: Option<(usize, f64)> = None;
        for (i, b) in BODIES.iter().enumerate() {
            let rel = b.position - cam;
            let dist = rel.length();
            let alt = dist - b.radius;
            if near.is_none_or(|(_, a)| alt < a) {
                near = Some((i, alt));
            }
            let node = &mut self.planet_nodes[i];
            let s = (SCALED_SPACE_DIST / dist).min(1.0);
            let patch_on = alt < PATCH_ALTITUDE;
            // Near the surface of a big body the scaled sphere would hide nearby
            // grids; the true-scale patch replaces it there.
            node.set_visible(!(patch_on && s < 1.0));
            node.set_position(v3(rel * s));
            // The coarse sphere sits a little low when the detailed patch is shown.
            let shrink = if patch_on { (b.radius - 25.0) / b.radius } else { 1.0 };
            node.set_scale(Vector3::splat((b.radius * s * shrink) as f32));
        }
        // Detailed surface patch on the nearest body.
        if let (Some((i, alt)), Some(patch), Some(mesh)) = (near, &mut self.patch, &mut self.patch_mesh) {
            let b = &BODIES[i];
            if alt < PATCH_ALTITUDE {
                let rel = b.position - cam;
                let sub = (-rel).normalize();
                let horizon = (2.0 * b.radius * alt.max(1.0)).sqrt();
                let half = (horizon * 1.6 + 2_000.0).min(b.radius * 0.6);
                visuals::build_surface_patch(mesh, rel, b.radius, sub, half);
                patch.set_material_override(&self.planet_nodes[i].get_material_override().unwrap());
                patch.set_visible(true);
            } else {
                patch.set_visible(false);
            }
        }
    }

    fn rebuild_grid(&mut self, id: GridId) {
        let mut root = match self.grid_nodes.remove(&id) {
            Some(old) => {
                for mut c in old.root.get_children().iter_shared() {
                    c.queue_free();
                }
                old.root
            }
            None => {
                let n = Node3D::new_alloc();
                self.base_mut().add_child(&n);
                n
            }
        };
        let vis = self.vis.as_ref().unwrap();
        let mut blocks = HashMap::new();
        let g = &self.st.grids[&id];
        for b in g.blocks.values() {
            let mut bn = Node3D::new_alloc();
            bn.set_position(Vector3::new(b.cell.x as f32, b.cell.y as f32, b.cell.z as f32));
            bn.set_quaternion(quat(orient::quat(b.rot)));
            let add = |parent: &mut Gd<Node3D>, mesh: &Gd<Mesh>, mat: &Gd<godot::classes::StandardMaterial3D>, pos: Vector3, scale: Vector3, rot: Option<Quaternion>| {
                let mut mi = MeshInstance3D::new_alloc();
                mi.set_mesh(mesh);
                mi.set_material_override(mat);
                mi.set_position(pos);
                mi.set_scale(scale);
                if let Some(r) = rot {
                    mi.set_quaternion(r);
                }
                parent.add_child(&mi);
                mi.upcast::<Node3D>()
            };
            let cube: Gd<Mesh> = vis.cube.clone().upcast();
            let cyl: Gd<Mesh> = vis.cylinder.clone().upcast();
            let nozzle: Gd<Mesh> = vis.nozzle.clone().upcast();
            let mat = &vis.block_mats[&b.kind];
            // Cylinders are along Y; this turns them along -Z/+Z.
            let along_z = Quaternion::from_axis_angle(Vector3::RIGHT, std::f32::consts::FRAC_PI_2);
            let along_x = Quaternion::from_axis_angle(Vector3::BACK, std::f32::consts::FRAC_PI_2);
            let mut node = BlockNode { flame: None, shaft: None, door: None };
            let sphere: Gd<Mesh> = vis.sphere.clone().upcast();
            match b.kind {
                BlockKind::Thruster => {
                    add(&mut bn, &cube, mat, Vector3::new(0.0, 0.0, -0.15), Vector3::new(1.0, 1.0, 0.7), None);
                    add(&mut bn, &nozzle, &vis.dark, Vector3::new(0.0, 0.0, 0.3), Vector3::new(1.0, 0.4, 1.0), Some(along_z));
                    let f = add(&mut bn, &nozzle, &vis.flame, Vector3::new(0.0, 0.0, 0.8), Vector3::splat(0.3), Some(along_z));
                    node.flame = Some(f);
                }
                BlockKind::Piston => {
                    add(&mut bn, &cube, mat, Vector3::new(0.0, 0.0, 0.1), Vector3::new(1.0, 1.0, 0.8), None);
                    let s = add(&mut bn, &cyl, &vis.dark, Vector3::new(0.0, 0.0, -0.5), Vector3::new(0.3, 1.0, 0.3), Some(along_z));
                    node.shaft = Some(s);
                }
                BlockKind::PistonTop | BlockKind::HingeTop | BlockKind::RotorTop => {
                    add(&mut bn, &cube, mat, Vector3::new(0.0, 0.0, -0.3), Vector3::new(1.0, 1.0, 0.4), None);
                }
                BlockKind::Hinge => {
                    add(&mut bn, &cube, mat, Vector3::new(0.0, 0.0, 0.15), Vector3::new(1.0, 1.0, 0.7), None);
                    add(&mut bn, &cyl, &vis.dark, Vector3::new(0.0, 0.0, -0.4), Vector3::new(0.5, 1.0, 0.5), Some(along_x));
                }
                BlockKind::Rotor => {
                    // Base plus a turntable disc on the forward (-Z) face.
                    add(&mut bn, &cube, mat, Vector3::new(0.0, 0.0, 0.1), Vector3::new(1.0, 1.0, 0.8), None);
                    add(&mut bn, &cyl, &vis.dark, Vector3::new(0.0, 0.0, -0.4), Vector3::new(0.85, 0.2, 0.85), Some(along_z));
                }
                BlockKind::Programmable => {
                    add(&mut bn, &cube, mat, Vector3::ZERO, Vector3::ONE, None);
                    add(&mut bn, &cube, &vis.screen, Vector3::new(0.0, 0.1, -0.47), Vector3::new(0.7, 0.5, 0.06), None);
                }
                BlockKind::Cockpit => {
                    add(&mut bn, &cube, mat, Vector3::ZERO, Vector3::ONE, None);
                    add(&mut bn, &cube, &vis.window, Vector3::new(0.0, 0.2, -0.47), Vector3::new(0.8, 0.45, 0.08), None);
                }
                BlockKind::Armor => {
                    add(&mut bn, &cube, mat, Vector3::ZERO, Vector3::ONE, None);
                }
                BlockKind::Gyro => {
                    add(&mut bn, &cube, &vis.dark, Vector3::new(0.0, -0.4, 0.0), Vector3::new(1.0, 0.2, 1.0), None);
                    add(&mut bn, &sphere, mat, Vector3::new(0.0, 0.05, 0.0), Vector3::splat(0.36), None);
                    add(&mut bn, &cyl, &vis.dark, Vector3::new(0.0, 0.05, 0.0), Vector3::new(0.9, 0.06, 0.9), None);
                }
                BlockKind::Door => {
                    // Frame around the opening plus a panel that slides up when open.
                    add(&mut bn, &cube, &vis.dark, Vector3::new(0.0, 0.45, 0.0), Vector3::new(1.0, 0.1, 1.0), None);
                    add(&mut bn, &cube, &vis.dark, Vector3::new(0.0, -0.45, 0.0), Vector3::new(1.0, 0.1, 1.0), None);
                    add(&mut bn, &cube, &vis.dark, Vector3::new(0.45, 0.0, 0.0), Vector3::new(0.1, 1.0, 1.0), None);
                    add(&mut bn, &cube, &vis.dark, Vector3::new(-0.45, 0.0, 0.0), Vector3::new(0.1, 1.0, 1.0), None);
                    let panel = add(&mut bn, &cube, mat, Vector3::ZERO, Vector3::new(0.82, 0.82, 0.2), None);
                    node.door = Some(panel);
                }
            }
            root.add_child(&bn);
            blocks.insert(b.id, node);
        }
        self.grid_nodes.insert(id, GridNode { root, blocks });
    }

    fn update_players(&mut self, cam: DVec3) {
        let me = self.st.me;
        let ids: Vec<PlayerId> = self.st.players.keys().copied().filter(|p| Some(*p) != me).collect();
        self.player_nodes.retain(|id, n| {
            let keep = ids.contains(id);
            if !keep {
                n.queue_free();
            }
            keep
        });
        for id in ids {
            let Some((pos, look, seat)) = self.st.player_state(id) else { continue };
            if !self.player_nodes.contains_key(&id) {
                let vis = self.vis.as_ref().unwrap();
                let mut root = Node3D::new_alloc();
                let mut body = MeshInstance3D::new_alloc();
                body.set_mesh(&vis.capsule);
                body.set_material_override(&vis.player);
                body.set_position(Vector3::new(0.0, 0.45, 0.0));
                root.add_child(&body);
                let mut visor = MeshInstance3D::new_alloc();
                visor.set_mesh(&vis.small_cube);
                visor.set_material_override(&vis.visor);
                visor.set_position(Vector3::new(0.0, 1.05, -0.35));
                visor.set_scale(Vector3::new(0.5, 0.2, 0.15));
                root.add_child(&visor);
                let mut label = Label3D::new_alloc();
                label.set_text(self.st.names.get(&id).map(|s| s.as_str()).unwrap_or("?"));
                label.set_billboard_mode(godot::classes::base_material_3d::BillboardMode::ENABLED);
                label.set_position(Vector3::new(0.0, 1.7, 0.0));
                label.set_pixel_size(0.004);
                root.add_child(&label);
                self.base_mut().add_child(&root);
                self.player_nodes.insert(id, root);
            }
            // Stand upright relative to gravity, facing the look direction.
            let rot = match self.gravity_up(pos) {
                Some(up) => {
                    let fwd = look * orient::FORWARD;
                    let fwd = (fwd - up * fwd.dot(up)).try_normalize().unwrap_or(up.any_orthonormal_vector());
                    let back = -fwd;
                    let right = up.cross(back).normalize();
                    DQuat::from_mat3(&glam::DMat3::from_cols(right, up, back)).normalize()
                }
                None => look,
            };
            let n = self.player_nodes.get_mut(&id).unwrap();
            n.set_visible(seat.is_none());
            n.set_position(v3(pos - cam));
            n.set_quaternion(quat(rot));
        }
    }

    fn update_build_ghost(&mut self) {
        let hit = if self.ui_open { None } else { self.target() };
        self.last_hit = hit.clone();
        if !self.build_mode || self.st.my_seat().is_some() {
            if let Some(g) = &mut self.ghost {
                g.set_visible(false);
            }
            return;
        }
        let (pos, rot) = match &hit {
            Some(h) => {
                let Some(gp) = self.st.grid_pose(h.grid) else { return };
                let cell = h.cell + h.normal;
                (gp.transform(cell.as_dvec3()), gp.rot * orient::quat(self.build_rot))
            }
            None => {
                let pos = self.cam_world + self.look * orient::FORWARD * 4.0;
                let rot = self.snap_new_grid_rot(pos, self.look);
                (pos, rot)
            }
        };
        let vis = self.vis.as_ref().unwrap();
        let Some(ghost) = &mut self.ghost else { return };
        ghost.set_visible(true);
        ghost.set_material_override(&vis.ghost_ok);
        ghost.set_position(v3(pos - self.cam_world));
        ghost.set_quaternion(quat(rot));
        ghost.set_scale(Vector3::splat(1.02));
        let _ = vis.ghost_bad.clone();
    }

    fn block_dict(&self, b: &shared::protocol::BlockData) -> VarDictionary {
        let c = self.st.controls.get(&b.id);
        vdict! {
            "id" => b.id as i64,
            "name" => b.name.as_str(),
            "kind" => b.kind.api_name(),
            "kind_name" => b.kind.display_name(),
            "terminal" => b.kind.is_terminal(),
            "override" => c.map(|c| c.thruster_override).unwrap_or(0.0),
            "piston_velocity" => c.map(|c| c.piston_velocity).unwrap_or(0.0),
            "hinge_velocity" => c.map(|c| c.hinge_velocity).unwrap_or(0.0),
            "hinge_target_enabled" => c.and_then(|c| c.hinge_target).is_some(),
            "hinge_target" => c.and_then(|c| c.hinge_target).unwrap_or(0.0),
            "value" => self.st.values.get(&b.id).copied().unwrap_or(0.0),
            "free" => c.map(|c| c.free).unwrap_or(false),
            "gyro_hold" => c.and_then(|c| c.gyro_override).is_some(),
            "door_open" => self.st.values.get(&b.id).copied().unwrap_or(0.0) > 0.5,
        }
    }
}

#[godot_api]
impl SpaceGame {
    /// Start an embedded server and connect to it.
    #[func]
    fn host_and_play(&mut self, name: GString, port: i64) -> GString {
        self.shutdown();
        let save = ProjectSettings::singleton().globalize_path("user://world.save").to_string();
        match Embedded::start(port as u16, save.into()) {
            Ok(e) => {
                let p = e.port;
                self.embedded = Some(e);
                self.join(name, gs(format!("127.0.0.1:{p}")))
            }
            Err(e) => gs(format!("Could not start server: {e}")),
        }
    }

    /// Connect to a server ("host:port"). Returns an error message or "".
    #[func]
    fn join(&mut self, name: GString, addr: GString) -> GString {
        if self.embedded.is_none() {
            self.shutdown();
        }
        let mut addr = addr.to_string();
        if !addr.contains(':') {
            addr = format!("{addr}:{}", shared::DEFAULT_PORT);
        }
        match Net::connect(&addr, &name.to_string()) {
            Ok(n) => {
                self.net = Some(n);
                self.status = format!("Connecting to {addr}...");
                self.ui_open = false;
                "".into()
            }
            Err(e) => {
                self.embedded = None;
                gs(e)
            }
        }
    }

    #[func]
    fn leave(&mut self) {
        self.shutdown();
    }

    #[func]
    fn is_playing(&self) -> bool {
        self.st.me.is_some()
    }

    #[func]
    fn status(&self) -> GString {
        self.status.as_str().into()
    }

    /// Tell the game whether a UI panel has focus (frees the mouse, stops movement).
    #[func]
    fn set_ui_open(&mut self, open: bool) {
        self.ui_open = open;
        let mode = if open || self.st.me.is_none() { MouseMode::VISIBLE } else { MouseMode::CAPTURED };
        Input::singleton().set_mouse_mode(mode);
    }

    #[func]
    fn toggle_stats(&mut self) {
        self.show_stats = !self.show_stats;
    }

    /// Drain pending UI events as dictionaries with a "type" key.
    #[func]
    fn poll_events(&mut self) -> VarArray {
        let mut out = VarArray::new();
        for e in std::mem::take(&mut self.st.ui_events) {
            let d = match e {
                UiEvent::Connected => {
                    Input::singleton().set_mouse_mode(MouseMode::CAPTURED);
                    vdict! { "type" => "connected" }
                }
                UiEvent::Disconnected(r) => vdict! { "type" => "disconnected", "reason" => r.as_str() },
                UiEvent::Chat { from, text } => vdict! { "type" => "chat", "from" => from.as_str(), "text" => text.as_str() },
                UiEvent::ScriptInfo(i) => {
                    let out_lines: PackedStringArray = i.output.iter().map(|s| GString::from(s.as_str())).collect();
                    vdict! {
                        "type" => "script_info",
                        "block" => i.block as i64,
                        "source" => i.source.as_str(),
                        "output" => &out_lines,
                        "error" => i.error.clone().unwrap_or_default().as_str(),
                        "frequency" => i.frequency as i64,
                    }
                }
                UiEvent::ScriptOutput { block, lines, error } => {
                    let l: PackedStringArray = lines.iter().map(|s| GString::from(s.as_str())).collect();
                    vdict! { "type" => "script_output", "block" => block as i64, "lines" => &l, "error" => error.unwrap_or_default().as_str() }
                }
                UiEvent::BlockControl(c) => vdict! { "type" => "block_control", "block" => c.block as i64 },
                UiEvent::GridChanged(g) => vdict! { "type" => "grid_changed", "grid" => g as i64 },
                UiEvent::OpenPanel { grid, block } => {
                    vdict! { "type" => "open_panel", "grid" => grid as i64, "block" => block.map(|b| b as i64).unwrap_or(-1) }
                }
                UiEvent::OpenEditor { block } => vdict! { "type" => "open_editor", "block" => block as i64 },
            };
            out.push(&d.to_variant());
        }
        out
    }

    /// All blocks of a grid and its mechanically connected sub-grids.
    #[func]
    fn grid_blocks(&self, grid: i64) -> VarArray {
        let mut out = VarArray::new();
        let mut ids: Vec<GridId> = vec![grid as GridId];
        // Follow piston/hinge heads by proximity of their mount blocks: the
        // client has no joint info, so include grids whose head blocks touch.
        let heads: Vec<GridId> = self
            .st
            .grids
            .values()
            .filter(|g| g.blocks.values().any(|b| matches!(b.kind, BlockKind::PistonTop | BlockKind::HingeTop | BlockKind::RotorTop)))
            .map(|g| g.id)
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for h in &heads {
                if ids.contains(h) {
                    continue;
                }
                let Some(hp) = self.st.grid_pose(*h) else { continue };
                let connected = ids.iter().any(|g| {
                    let Some(gp) = self.st.grid_pose(*g) else { return false };
                    self.st.grids[g].blocks.values().any(|b| {
                        b.kind.is_mech() && (gp.transform(b.cell.as_dvec3()) - hp.pos).length() < 7.0
                    })
                });
                if connected {
                    ids.push(*h);
                    changed = true;
                }
            }
        }
        let mut blocks: Vec<&shared::protocol::BlockData> =
            ids.iter().filter_map(|g| self.st.grids.get(g)).flat_map(|g| g.blocks.values()).collect();
        blocks.sort_by(|a, b| (!a.kind.is_terminal(), &a.name).cmp(&(!b.kind.is_terminal(), &b.name)));
        for b in blocks {
            out.push(&self.block_dict(b).to_variant());
        }
        out
    }

    #[func]
    fn block_info(&self, block: i64) -> VarDictionary {
        match self.st.block(block as BlockId) {
            Some((_, b)) => self.block_dict(b),
            None => VarDictionary::new(),
        }
    }

    #[func]
    fn rename_block(&mut self, block: i64, name: GString) {
        self.send(ClientMsg::RenameBlock { block: block as BlockId, name: name.to_string() });
    }

    #[func]
    fn set_thruster_override(&mut self, block: i64, v: f64) {
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::ThrusterOverride(v as f32) });
    }

    #[func]
    fn set_piston_velocity(&mut self, block: i64, v: f64) {
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::PistonVelocity(v as f32) });
    }

    #[func]
    fn set_hinge_velocity(&mut self, block: i64, v: f64) {
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::HingeVelocity(v as f32) });
    }

    #[func]
    fn set_hinge_target(&mut self, block: i64, enabled: bool, deg: f64) {
        let t = enabled.then_some(deg as f32);
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::HingeTarget(t) });
    }

    /// Motor off: the piston/hinge/rotor moves only under external forces.
    #[func]
    fn set_free(&mut self, block: i64, free: bool) {
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::Free(free) });
    }

    /// Gyroscope: hold the current orientation (override 0) or switch off.
    #[func]
    fn set_gyro_hold(&mut self, block: i64, on: bool) {
        let v = on.then_some([0.0f32; 3]);
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::GyroOverride(v) });
    }

    #[func]
    fn set_door(&mut self, block: i64, open: bool) {
        self.send(ClientMsg::SetBlockProp { block: block as BlockId, prop: BlockProp::DoorOpen(open) });
    }

    #[func]
    fn request_script(&mut self, block: i64) {
        self.send(ClientMsg::RequestScript { block: block as BlockId });
    }

    #[func]
    fn upload_script(&mut self, block: i64, source: GString) {
        self.send(ClientMsg::UploadScript { block: block as BlockId, source: source.to_string() });
    }

    #[func]
    fn run_script(&mut self, block: i64, arg: GString) {
        self.send(ClientMsg::RunScript { block: block as BlockId, arg: arg.to_string() });
    }

    #[func]
    fn send_chat(&mut self, text: GString) {
        let t = text.to_string();
        if !t.trim().is_empty() {
            self.send(ClientMsg::Chat(t));
        }
    }

    /// Example scripts as [[name, source], ...].
    #[func]
    fn script_examples(&self) -> VarArray {
        let mut out = VarArray::new();
        for (name, src) in shared::examples::all() {
            out.push(&varray![name, src.as_str()].to_variant());
        }
        out
    }

    #[func]
    fn hud_text(&self) -> GString {
        let Some(me) = self.st.me else { return "".into() };
        let mut s = String::new();
        let seat = self.st.my_seat();
        let (vel, pos) = match seat.and_then(|b| self.st.block(b)) {
            Some((g, _)) => {
                let v = self.st.grids.get(&g).and_then(|_| self.st.grid_pose(g)).map(|p| p.pos).unwrap_or_default();
                (self.st.my_state.as_ref().map(|s| s.vel).unwrap_or_default(), v)
            }
            None => (self.st.pred.vel, self.st.my_render_pos().unwrap_or_default()),
        };
        let g = gravity_at(pos);
        let body = BODIES[g.nearest].name;
        s += &format!("Speed {:>6.1} m/s   Altitude {:>8.0} m ({body})   Gravity {:.2} g\n", vel.length(), g.altitude, g.accel.length() / 9.81);
        let onoff = |b: bool| if b { "ON" } else { "off" };
        s += &format!(
            "[J] Jetpack {}  [Z] Dampeners {}  [X] Cruise {}\n",
            onoff(self.jetpack),
            onoff(self.dampeners),
            onoff(self.cruise)
        );
        if seat.is_some() {
            s += "Piloting: WASD/Space/C move, mouse pitch/yaw, Q/E roll, [F] leave seat, [K] control panel\n";
        } else if self.build_mode {
            let kind = BUILDABLE[self.build_kind];
            let f = orient::forward(self.build_rot);
            s += &format!(
                "BUILD: {} (1-9 / wheel)  facing {:?} [R]/[T] rotate  new grids: {} [B]\nLMB place  RMB remove  [G] exit build mode\n",
                kind.display_name(),
                f,
                if self.build_static { "STATION (static)" } else { "SHIP (dynamic)" }
            );
        } else {
            s += "[G] build mode  [F] use  [K] control panel  [Enter] chat  [F1] help\n";
        }
        if let Some(hit) = &self.last_hit {
            if let Some((_, b)) = self.st.block(hit.block) {
                let action = match b.kind {
                    BlockKind::Cockpit => "[F] sit",
                    BlockKind::Programmable => "[F] edit script",
                    BlockKind::Door => {
                        if self.st.values.get(&b.id).copied().unwrap_or(0.0) > 0.5 { "[F] close" } else { "[F] open" }
                    }
                    k if k.is_terminal() => "[F] control",
                    _ => "",
                };
                s += &format!("> {} {}\n", b.name, action);
            }
        }
        let _ = me;
        gs(s)
    }

    #[func]
    fn stats_text(&self) -> GString {
        if !self.show_stats {
            return "".into();
        }
        let s = &self.st.stats;
        let mut out = format!(
            "FPS {:.0}\nServer tick {:.2} ms (scripts {:.2} ms, physics {:.2} ms)\n{} players, {} grids, {} blocks, {} scripts\n",
            self.fps, s.tick_ms, s.script_ms, s.physics_ms, s.players, s.grids, s.blocks, s.scripts
        );
        if let Some(n) = &self.net {
            let i = n.network_info();
            out += &format!(
                "RTT {:.0} ms, loss {:.1}%, in {:.1} KB/s, out {:.1} KB/s\n",
                i.rtt * 1000.0,
                i.packet_loss * 100.0,
                i.bytes_received_per_second / 1024.0,
                i.bytes_sent_per_second / 1024.0
            );
        }
        out += &format!(
            "Known grids {}, snapshots {}, render delay {:.1} ticks, prediction error {:.3} m\n",
            self.st.grids.len(),
            self.st.snapshots_received,
            self.st.latest_tick as f64 - self.st.render_tick,
            self.st.pred.error.length()
        );
        gs(out)
    }

    /// For automated tests: world-space camera position as "x y z".
    #[func]
    fn debug_camera(&self) -> GString {
        gs(format!("{:.2} {:.2} {:.2}", self.cam_world.x, self.cam_world.y, self.cam_world.z))
    }

    /// For automated tests: number of grids/players currently rendered.
    #[func]
    fn debug_counts(&self) -> VarDictionary {
        vdict! {
            "grids" => self.grid_nodes.len() as i64,
            "players" => self.player_nodes.len() as i64,
            "snapshots" => self.st.snapshots_received as i64,
        }
    }

    /// For automated tests: ids of all known grids.
    #[func]
    fn debug_grid_ids(&self) -> VarArray {
        let mut a = VarArray::new();
        for id in self.st.grids.keys() {
            a.push(&(*id as i64).to_variant());
        }
        a
    }

    /// For automated tests: look at the block with this name (if known).
    #[func]
    fn debug_look_at_block(&mut self, name: GString, height_offset: f64) -> bool {
        let name = name.to_string();
        let Some(id) = self.st.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == name).map(|b| b.id) else {
            return false;
        };
        let Some(target) = self.st.block_world_pose(id) else { return false };
        let up = self.gravity_up(self.cam_world).unwrap_or(DVec3::Y);
        let fwd = (target.pos + up * height_offset - self.cam_world).normalize();
        let back = -fwd;
        let right = up.cross(back).normalize();
        let u = back.cross(right);
        self.look = DQuat::from_mat3(&glam::DMat3::from_cols(right, u, back)).normalize();
        true
    }

    /// For automated tests: look at a world direction.
    #[func]
    fn debug_set_look(&mut self, yaw_deg: f64, pitch_deg: f64) {
        let pos = self.st.my_render_pos().unwrap_or_default();
        let up = self.gravity_up(pos).unwrap_or(DVec3::Y);
        // Yaw 0 faces the frame the starter base was built in.
        let fwd0 = DQuat::from_rotation_arc(DVec3::Y, up) * orient::FORWARD;
        let right0 = fwd0.cross(up);
        let yaw = DQuat::from_axis_angle(up, yaw_deg.to_radians());
        let fwd = yaw * fwd0;
        let right = yaw * right0;
        let fwd = DQuat::from_axis_angle(right, pitch_deg.to_radians()) * fwd;
        let back = -fwd;
        let r = up.cross(back).normalize();
        let u = back.cross(r);
        self.look = DQuat::from_mat3(&glam::DMat3::from_cols(r, u, back)).normalize();
    }

    #[func]
    fn debug_key(&mut self, key: GString) {
        let k = match key.to_string().as_str() {
            "F" => Key::F,
            "G" => Key::G,
            "J" => Key::J,
            other => {
                godot_warn!("unknown debug key {other}");
                return;
            }
        };
        let mut ev = InputEventKey::new_gd();
        ev.set_keycode(k);
        ev.set_pressed(true);
        let was = self.ui_open;
        self.ui_open = false;
        self.unhandled_input(ev.upcast());
        self.ui_open = was;
    }

    #[func]
    fn debug_click(&mut self, right: bool) {
        if right {
            self.remove();
        } else {
            self.place();
        }
    }
}

impl Drop for SpaceGame {
    fn drop(&mut self) {
        if let Some(mut n) = self.net.take() {
            n.disconnect();
            n.flush();
        }
        self.embedded = None;
    }
}
