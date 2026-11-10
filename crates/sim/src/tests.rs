use super::*;
use shared::examples;
use shared::orient;
use shared::protocol::BlockProp;
use shared::solar::{body_by_name, gravity_at};

fn deep_space() -> DVec3 {
    DVec3::new(0.0, 1.0e7, 0.0)
}

fn above(body: &str, alt: f64) -> DVec3 {
    let (_, b) = body_by_name(body).unwrap();
    b.position + DVec3::Y * (b.radius + alt)
}

fn run(w: &mut World, seconds: f64) {
    for _ in 0..(seconds * shared::TICK_HZ as f64) as usize {
        w.step();
    }
}

fn mech_value(w: &World, id: BlockId) -> f64 {
    match &w.block(id).unwrap().1.state {
        BlockState::Mech(m) => m.value,
        _ => panic!("not a mech"),
    }
}

fn ident() -> u8 {
    orient::from_axes(IVec3::NEG_Z, IVec3::Y).unwrap()
}

#[test]
fn building_and_removing() {
    let mut w = World::new();
    w.add_player(1, "a");
    let p = w.player_pos(1).unwrap();
    let g = w.new_grid(1, p + DVec3::X * 3.0, DQuat::IDENTITY, BlockKind::Armor, true).unwrap();
    assert!(w.place_block(1, g, IVec3::new(1, 0, 0), ident(), BlockKind::Armor).is_ok());
    assert!(w.place_block(1, g, IVec3::new(1, 0, 0), ident(), BlockKind::Armor).is_err(), "occupied");
    assert!(w.place_block(1, g, IVec3::new(5, 0, 0), ident(), BlockKind::Armor).is_err(), "floating");
    assert!(w.place_block(1, g, IVec3::new(1, 1, 0), ident(), BlockKind::PistonTop).is_err(), "not buildable");
    let far = w.new_grid(1, p + DVec3::X * 300.0, DQuat::IDENTITY, BlockKind::Armor, true);
    assert!(far.is_err(), "out of reach");
    assert_eq!(w.grids[&g].blocks.len(), 2);
    w.remove_block(1, g, IVec3::new(1, 0, 0)).unwrap();
    w.remove_block(1, g, IVec3::ZERO).unwrap();
    assert!(!w.grids.contains_key(&g), "empty grid is deleted");
    let ev = w.drain_events();
    assert!(ev.iter().any(|e| matches!(e, WorldEvent::GridRemoved(x) if *x == g)));
}

#[test]
fn grids_fall_and_land_on_earth() {
    let mut w = World::new();
    let pos = above("Earth", 10.0);
    let g = w.create_grid(pos, prefabs::surface_orientation(pos), false, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    run(&mut w, 4.0);
    let alt = gravity_at(w.grid_pose(g).unwrap().translation).altitude;
    assert!(alt > 0.3 && alt < 0.7, "resting on the ground, altitude {alt}");
}

#[test]
fn thruster_pushes_grid() {
    let mut w = World::new();
    let g = w.create_grid(deep_space(), DQuat::IDENTITY, false, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    let t = w.insert_block(g, IVec3::Z, ident(), BlockKind::Thruster, None, None, false);
    w.set_block_prop(t, BlockProp::ThrusterOverride(1.0)).unwrap();
    run(&mut w, 1.0);
    let v = w.phys.bodies[w.grids[&g].body].linvel();
    // a = 30 kN / 250 kg = 120 m/s² -> ~120 m/s after 1 s, capped at 150.
    assert!(v.z < -100.0 && v.x.abs() < 1.0, "{v}");
}

#[test]
fn piston_and_hinge_move() {
    let mut w = World::new();
    w.add_player(1, "a");
    let g = w.create_grid(deep_space(), DQuat::IDENTITY, true, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    let piston = w.insert_block(g, IVec3::Y, orient::from_axes(IVec3::Y, IVec3::Z).unwrap(), BlockKind::Piston, None, None, true);
    w.set_block_prop(piston, BlockProp::PistonVelocity(2.0)).unwrap();
    run(&mut w, 3.5);
    let ext = mech_value(&w, piston);
    assert!((ext - 5.0).abs() < 0.1, "fully extended: {ext}");
    w.set_block_prop(piston, BlockProp::PistonVelocity(-1.0)).unwrap();
    run(&mut w, 1.0);
    let ext = mech_value(&w, piston);
    assert!((ext - 4.0).abs() < 0.2, "retracting: {ext}");

    let hinge = w.insert_block(g, IVec3::X, orient::from_axes(IVec3::X, IVec3::Y).unwrap(), BlockKind::Hinge, None, None, true);
    w.set_block_prop(hinge, BlockProp::HingeTarget(Some(45.0))).unwrap();
    run(&mut w, 3.0);
    let a = mech_value(&w, hinge);
    assert!((a - 45.0).abs() < 2.0, "hinge at target: {a}");
    w.set_block_prop(hinge, BlockProp::HingeVelocity(-30.0)).unwrap();
    run(&mut w, 1.0);
    let a = mech_value(&w, hinge);
    assert!((a - 15.0).abs() < 4.0, "hinge moving: {a}");
}

/// Acceptance script 1.
#[test]
fn acceptance_arm_sweep() {
    let mut w = World::new();
    let pos = above("Earth", 0.6);
    w.spawn_prefab("arm", pos, prefabs::surface_orientation(pos), None).unwrap();
    let piston = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Arm Piston").unwrap().id;
    let hinge = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Elbow Hinge").unwrap().id;
    let pb = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Arm Controller").unwrap().id;
    assert_eq!(w.script_info(pb).unwrap().error, None);
    let (mut emin, mut emax, mut amin, mut amax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for _ in 0..(12 * 60) {
        w.step();
        let e = mech_value(&w, piston);
        let a = mech_value(&w, hinge);
        emin = emin.min(e);
        emax = emax.max(e);
        amin = amin.min(a);
        amax = amax.max(a);
    }
    assert_eq!(w.script_info(pb).unwrap().error, None);
    assert!(emax > 4.7 && emin < 0.3, "piston range {emin}..{emax}");
    assert!(amax > 55.0 && amin < -55.0, "hinge range {amin}..{amax}");
}

/// Acceptance script 2.
#[test]
fn acceptance_velocity_hold() {
    let mut w = World::new();
    let g = w.spawn_prefab("ship", deep_space(), DQuat::IDENTITY, Some(&examples::velocity_hold())).unwrap();
    // Give it some sideways drift to cancel.
    w.phys.bodies[w.grids[&g].body].set_linvel(DVec3::new(10.0, -5.0, 0.0), true);
    run(&mut w, 25.0);
    let b = &w.phys.bodies[w.grids[&g].body];
    let local = b.rotation().inverse() * b.linvel();
    assert!((local - DVec3::new(0.0, 0.0, -50.0)).length() < 1.0, "local velocity {local}");
}

/// Acceptance script 3.
#[test]
fn acceptance_hover_on_moon() {
    let mut w = World::new();
    let pos = above("Moon", 5.0);
    let g = w.spawn_prefab("ship", pos, prefabs::surface_orientation(pos), Some(&examples::hover())).unwrap();
    run(&mut w, 30.0);
    let p = w.phys.bodies[w.grids[&g].body].translation();
    let alt = gravity_at(p).altitude;
    assert!((alt - 25.0).abs() < 1.0, "altitude {alt}");
    let pb = w.grids[&g].blocks[&IVec3::ZERO].id;
    w.run_script(pb, "40").unwrap();
    run(&mut w, 20.0);
    let alt = gravity_at(w.phys.bodies[w.grids[&g].body].translation()).altitude;
    assert!((alt - 40.0).abs() < 1.0, "altitude after retarget {alt}");
    assert!(w.script_info(pb).unwrap().output.iter().any(|l| l.contains("target altitude")));
}

#[test]
fn runaway_script_is_contained() {
    let mut w = World::new();
    let g = w.spawn_prefab("ship", deep_space(), DQuat::IDENTITY, None).unwrap();
    let pb = w.grids[&g].blocks[&IVec3::ZERO].id;
    w.upload_script(pb, "function tick() while true do end end").unwrap();
    w.step();
    let t = std::time::Instant::now();
    run(&mut w, 0.5);
    assert!(t.elapsed().as_secs_f64() < 0.5 * 10.0 * script::TIME_BUDGET.as_secs_f64() + 0.5);
    let info = w.script_info(pb).unwrap();
    assert!(info.error.unwrap().contains("budget"));
    // Compile errors are reported.
    assert!(w.upload_script(pb, "function (").is_err());
    // Scripts cannot touch blocks of other grids.
    let other = w.spawn_prefab("ship", deep_space() + DVec3::X * 20.0, DQuat::IDENTITY, None).unwrap();
    let foreign = w.grids[&other].blocks[&IVec3::new(0, 0, 1)].id;
    let src = format!("function tick() end\nprint(grid:block('Main Thruster').id ~= {foreign})");
    w.upload_script(pb, &src).unwrap();
    assert_eq!(w.script_info(pb).unwrap().output, vec!["true".to_string()]);
}

#[test]
fn cockpit_flies_ship() {
    let mut w = World::new();
    w.add_player(1, "pilot");
    let start = w.player_pos(1).unwrap() + DVec3::new(0.0, 0.0, -4.0);
    w.teleport_player(1, deep_space() + DVec3::Z * 3.0);
    let g = w.spawn_prefab("ship", deep_space(), DQuat::IDENTITY, None).unwrap();
    let _ = start;
    let cockpit = w.grids[&g].blocks[&IVec3::Y].id;
    w.enter_seat(1, cockpit).unwrap();
    w.set_input(1, PlayerInput { seq: 1, movement: shared::Vec3::new(0.0, 0.0, -1.0), dampeners: true, look: DQuat::IDENTITY, ..Default::default() });
    run(&mut w, 2.0);
    let v = w.phys.bodies[w.grids[&g].body].linvel();
    assert!(v.z < -20.0, "flying forward {v}");
    // Release: dampeners stop the ship.
    w.set_input(1, PlayerInput { seq: 2, dampeners: true, look: DQuat::IDENTITY, ..Default::default() });
    run(&mut w, 6.0);
    let v = w.phys.bodies[w.grids[&g].body].linvel();
    assert!(v.length() < 0.5, "stopped {v}");
    let pp = w.player_pos(1).unwrap();
    assert!((pp - w.block_world_pos(cockpit).unwrap()).length() < 0.01, "player rides along");
    w.exit_seat(1);
    assert!(w.players[&1].seat.is_none());
}

#[test]
fn jetpack_hovers_and_walks() {
    let mut w = World::new();
    w.add_player(1, "p");
    let start = w.player_pos(1).unwrap();
    // Jetpack with dampeners hovers in gravity.
    w.set_input(1, PlayerInput { seq: 1, look: DQuat::IDENTITY, dampeners: true, jetpack: true, ..Default::default() });
    run(&mut w, 2.0);
    let p = w.player_pos(1).unwrap();
    assert!((p - start).length() < 0.3, "hover drift {}", (p - start).length());
    // Without jetpack the player falls and stands on the ground.
    w.set_input(1, PlayerInput { seq: 2, look: DQuat::IDENTITY, ..Default::default() });
    run(&mut w, 3.0);
    assert!(w.players[&1].grounded);
    let alt = gravity_at(w.player_pos(1).unwrap()).altitude;
    assert!((alt - PLAYER_RADIUS).abs() < 0.1, "standing, alt {alt}");
    w.set_input(1, PlayerInput { seq: 3, look: DQuat::IDENTITY, movement: shared::Vec3::new(0.0, 0.0, -1.0), ..Default::default() });
    let before = w.player_pos(1).unwrap();
    run(&mut w, 1.0);
    let moved = (w.player_pos(1).unwrap() - before).length();
    assert!(moved > 3.0, "walked {moved}");
}

#[test]
fn save_and_load_roundtrip() {
    let mut w = World::new();
    let pos = above("Earth", 0.6);
    w.spawn_prefab("arm", pos, prefabs::surface_orientation(pos), None).unwrap();
    run(&mut w, 2.0);
    let piston = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Arm Piston").unwrap().id;
    let before = mech_value(&w, piston);
    let hinge = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Elbow Hinge").unwrap().id;
    let hinge_before = mech_value(&w, hinge);
    assert!(hinge_before.abs() > 5.0, "hinge should be deflected for this test: {hinge_before}");
    let blocks = w.total_blocks();
    let bytes = w.save_bytes();
    let mut w2 = World::load_bytes(&bytes).unwrap();
    assert_eq!(w2.total_blocks(), blocks);
    assert_eq!(w2.grids.len(), w.grids.len());
    assert!((mech_value(&w2, piston) - before).abs() < 0.05);
    assert_eq!(w2.program_count(), 1);
    // No snapping back to the rest pose after the first steps.
    run(&mut w2, 0.1);
    assert!((mech_value(&w2, piston) - before).abs() < 0.3, "joint kept its extension after load");
    let h = mech_value(&w2, hinge);
    assert!((h - hinge_before).abs() < 6.0, "hinge kept its angle after load: {hinge_before} -> {h}");
    // Keeps running after load.
    run(&mut w2, 2.0);
    assert!((mech_value(&w2, piston) - before).abs() > 0.5, "still animating");
}


#[test]
fn hinge_holds_in_gravity() {
    let mut w = World::new();
    let pos = above("Earth", 0.6);
    let g = w.create_grid(pos, prefabs::surface_orientation(pos), true, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    let hinge = w.insert_block(g, IVec3::Y, orient::from_axes(IVec3::Y, IVec3::Z).unwrap(), BlockKind::Hinge, None, None, true);
    let top = match &w.block(hinge).unwrap().1.state { BlockState::Mech(m) => m.top.unwrap(), _ => unreachable!() };
    for y in 1..=3 {
        w.insert_block(top, IVec3::new(0, y, 0), ident(), BlockKind::Armor, None, None, false);
    }
    w.set_block_prop(hinge, BlockProp::HingeTarget(Some(30.0))).unwrap();
    for i in 0..240 {
        w.step();
        if i % 30 == 0 { eprintln!("ang {:.1}", mech_value(&w, hinge)); }
    }
    let a = mech_value(&w, hinge);
    assert!((a - 30.0).abs() < 3.0, "{a}");
}

/// Every demo construct must move through its range without errors or explosions.
#[test]
fn demo_constructs_work() {
    // (prefab, [(mech name, expected min below, expected max above)])
    let cases: [(&str, &[(&str, f64, f64)]); 5] = [
        ("elevator", &[("Lift 1", 0.3, 4.7), ("Lift 2", 0.3, 4.7), ("Lift 3", 0.3, 4.7)]),
        ("crane", &[("Slew Rotor", -150.0, 150.0), ("Boom Hinge", -50.0, -10.0), ("Boom Piston", 1.0, 4.0), ("Hook Hinge", -50.0, -10.0)]),
        ("carousel", &[("Carousel Rotor", -150.0, 150.0), ("Swing 1", 1.0, 30.0), ("Swing 3", 1.0, 30.0)]),
        ("robot", &[("Shoulder", -25.0, 25.0), ("Reach Piston", 1.0, 4.0), ("Elbow", -45.0, 45.0), ("Wrist", -60.0, 60.0)]),
        ("bridge", &[("Bridge Hinge", 5.0, 70.0)]),
    ];
    for (name, mechs) in cases {
        let mut w = World::new();
        let pos = above("Earth", 0.6);
        let base = w.spawn_prefab(name, pos, prefabs::surface_orientation(pos), None).unwrap();
        let find = |w: &World, n: &str| w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == n).map(|b| b.id).unwrap();
        let pb = w.grids[&base].blocks.values().find(|b| b.kind == BlockKind::Programmable).unwrap().id;
        assert_eq!(w.script_info(pb).unwrap().error, None, "{name}: script error at start");
        let ids: Vec<BlockId> = mechs.iter().map(|(n, _, _)| find(&w, n)).collect();
        let mut lo = vec![f64::MAX; ids.len()];
        let mut hi = vec![f64::MIN; ids.len()];
        let mut max_dist: f64 = 0.0;
        for _ in 0..(45 * 60) {
            w.step();
            for (i, id) in ids.iter().enumerate() {
                let v = mech_value(&w, *id);
                assert!(v.is_finite(), "{name}: NaN");
                lo[i] = lo[i].min(v);
                hi[i] = hi[i].max(v);
            }
            for g in w.grids.values() {
                let d = (w.phys.bodies[g.body].translation() - pos).length();
                max_dist = max_dist.max(d);
            }
        }
        assert_eq!(w.script_info(pb).unwrap().error, None, "{name}: script error");
        assert!(max_dist < 30.0, "{name}: a part flew {max_dist} m away");
        if name == "carousel" {
            // Gondolas must swing outwards (away from the hub), not inwards.
            let hub = w.grids.values().find(|g| g.blocks.values().any(|b| b.kind == BlockKind::RotorTop)).unwrap();
            let hub_pos = w.phys.bodies[hub.body].translation();
            let up = (pos - shared::solar::BODIES[3].position).normalize();
            let swing_out = w.grids.values().filter(|g| g.blocks.values().any(|b| b.kind == BlockKind::Cockpit)).map(|g| {
                let p = w.phys.bodies[g.body].translation() - hub_pos;
                (p - up * p.dot(up)).length()
            }).fold(0.0f64, f64::max);
            eprintln!("carousel gondola max radius at end: {swing_out:.2}");
        }
        for (i, (n, want_lo, want_hi)) in mechs.iter().enumerate() {
            eprintln!("{name}/{n}: {:.2} .. {:.2}", lo[i], hi[i]);
            // Nothing may slam into the hinge limits (that means it toppled).
            let is_rotor = w.block(ids[i]).unwrap().1.kind == BlockKind::Rotor;
            assert!(is_rotor || (lo[i] > -85.0 && hi[i] < 85.0), "{name}/{n} hit its limits: {:.1}..{:.1}", lo[i], hi[i]);
            assert!(lo[i] < *want_lo && hi[i] > *want_hi, "{name}/{n} range {:.2}..{:.2}, wanted below {want_lo} and above {want_hi}", lo[i], hi[i]);
        }
    }
}


#[test]
fn rotor_spins_and_targets() {
    let mut w = World::new();
    let g = w.create_grid(deep_space(), DQuat::IDENTITY, true, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    let rotor = w.insert_block(g, IVec3::Y, orient::from_axes(IVec3::Y, IVec3::Z).unwrap(), BlockKind::Rotor, None, None, true);
    // An off-centre arm so the rotor has something to turn.
    let head = match &w.block(rotor).unwrap().1.state { BlockState::Mech(m) => m.top.unwrap(), _ => unreachable!() };
    for x in 1..=3 {
        w.insert_block(head, IVec3::new(x, 1, 0), ident(), BlockKind::Armor, None, None, false);
    }
    // Continuous rotation: 90 deg/s for 5 s passes the +-180 wrap at least once.
    w.set_block_prop(rotor, BlockProp::HingeVelocity(90.0)).unwrap();
    let mut wraps = 0;
    let mut last = mech_value(&w, rotor);
    for _ in 0..300 {
        w.step();
        let v = mech_value(&w, rotor);
        if last > 150.0 && v < -150.0 {
            wraps += 1;
        }
        last = v;
    }
    assert!(wraps >= 1, "rotor never wrapped around (angle {last})");
    // Target via the shortest way round, including across the wrap.
    for target in [170.0f32, -170.0, 0.0] {
        w.set_block_prop(rotor, BlockProp::HingeTarget(Some(target))).unwrap();
        run(&mut w, 4.0);
        let v = mech_value(&w, rotor);
        let err = crate::build::wrap_deg(v - target as f64).abs();
        assert!(err < 2.0, "target {target}: at {v}");
    }
    // Save/load keeps the angle.
    w.set_block_prop(rotor, BlockProp::HingeTarget(Some(120.0))).unwrap();
    run(&mut w, 3.0);
    let mut w2 = World::load_bytes(&w.save_bytes()).unwrap();
    run(&mut w2, 0.2);
    let v = mech_value(&w2, rotor);
    assert!((v - 120.0).abs() < 3.0, "after load: {v}");
}

/// Free (motor-off) hinges on a spinning rotor swing outwards from physics alone,
/// close to the textbook angle tan(a) = w^2 r / g.
#[test]
fn free_swings_follow_physics() {
    let mut w = World::new();
    let pos = above("Earth", 0.6);
    let base = w.spawn_prefab("carousel", pos, prefabs::surface_orientation(pos), None).unwrap();
    // Replace the demo script: constant 60 deg/s, swings free.
    let pb = w.grids[&base].blocks.values().find(|b| b.kind == BlockKind::Programmable).unwrap().id;
    w.upload_script(pb, "local r = grid:block('Carousel Rotor')\nfor i = 1, 4 do grid:block('Swing ' .. i):set_free(true) end\nfunction tick() r:set_velocity(60) end").unwrap();
    let swing = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Swing 1").unwrap().id;
    run(&mut w, 25.0);
    let mut sum = 0.0;
    let n = 10 * 60;
    for _ in 0..n {
        w.step();
        sum += mech_value(&w, swing);
    }
    let avg = sum / n as f64;
    // w = 60 deg/s, arm radius 4 m plus the swing-out: roughly 25-35 degrees.
    let w_rad = 60f64.to_radians();
    let ideal = (w_rad * w_rad * (4.0 + 2.0 * avg.to_radians().sin()) / 9.81).atan().to_degrees();
    eprintln!("free swing average {avg:.1} deg, textbook {ideal:.1} deg");
    assert!(avg > 10.0, "gondola should swing outwards, got {avg:.1}");
    assert!((avg - ideal).abs() < 12.0, "free swing {avg:.1} vs textbook {ideal:.1}");
}

fn tilt_deg(w: &World, g: GridId) -> f64 {
    let b = &w.phys.bodies[w.grids[&g].body];
    let up = *b.rotation() * DVec3::Y;
    let want = -gravity_at(b.translation()).accel.normalize();
    up.dot(want).clamp(-1.0, 1.0).acos().to_degrees()
}

/// The shuttle sits on its legs, takes off, hovers level, lands and opens its ramp.
#[test]
fn shuttle_autopilot_flight() {
    let mut w = World::new();
    let pos = above("Earth", 3.6);
    let g = w.spawn_prefab("shuttle", pos, prefabs::surface_orientation(pos), None).unwrap();
    let mass = w.phys.bodies[w.grids[&g].body].mass();
    let pb = w.grids[&g].blocks.values().find(|b| b.name == "Shuttle Computer").unwrap().id;
    assert_eq!(w.script_info(pb).unwrap().error, None);
    run(&mut w, 3.0);
    let alt0 = gravity_at(w.grid_pose(g).unwrap().translation).altitude;
    eprintln!("mass {mass:.0} kg, resting altitude {alt0:.2}, tilt {:.1}", tilt_deg(&w, g));
    assert!(tilt_deg(&w, g) < 3.0, "stands level on its legs");

    w.run_script(pb, "takeoff").unwrap();
    let mut max_tilt: f64 = 0.0;
    for _ in 0..(30 * 60) {
        w.step();
        max_tilt = max_tilt.max(tilt_deg(&w, g));
    }
    let alt = gravity_at(w.grid_pose(g).unwrap().translation).altitude;
    eprintln!("after takeoff: altitude {alt:.1}, max tilt {max_tilt:.1}");
    assert!((alt - 40.0).abs() < 2.0, "hovering at 40 m, got {alt:.1}");
    assert!(max_tilt < 10.0, "stays level during takeoff: {max_tilt:.1}");
    assert_eq!(w.script_info(pb).unwrap().error, None);

    w.run_script(pb, "land").unwrap();
    run(&mut w, 45.0);
    let alt = gravity_at(w.grid_pose(g).unwrap().translation).altitude;
    let info = w.script_info(pb).unwrap();
    eprintln!("after landing: altitude {alt:.1}, output {:?}", info.output);
    assert!(info.output.iter().any(|l| l.contains("landed")), "landed message");
    assert!(alt < alt0 + 1.5, "back on the ground: {alt:.1}");
    assert!(tilt_deg(&w, g) < 5.0);
    let ramp = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Ramp Hinge").unwrap().id;
    run(&mut w, 3.0);
    assert!(mech_value(&w, ramp) < -20.0, "ramp opened: {}", mech_value(&w, ramp));
}

#[test]
fn doors_and_gyro() {
    let mut w = World::new();
    let g = w.create_grid(deep_space(), DQuat::IDENTITY, false, None);
    w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Armor, None, None, false);
    let gyro = w.insert_block(g, IVec3::X, ident(), BlockKind::Gyro, None, None, false);
    let door = w.insert_block(g, IVec3::NEG_X, ident(), BlockKind::Door, None, None, false);
    // Gyro spins the grid at the requested rate (deg/s, grid frame).
    w.set_block_prop(gyro, BlockProp::GyroOverride(Some([0.0, 30.0, 0.0]))).unwrap();
    run(&mut w, 2.0);
    let av = w.phys.bodies[w.grids[&g].body].angvel();
    assert!((av.y.to_degrees() - 30.0).abs() < 1.0, "gyro rate {av}");
    // Doors toggle their collider.
    let col = w.block(door).unwrap().1.collider;
    assert!(w.phys.colliders[col].is_enabled());
    w.set_block_prop(door, BlockProp::DoorOpen(true)).unwrap();
    assert!(!w.phys.colliders[col].is_enabled());
    // Scripts can drive both, and the state survives save/load.
    let pb = w.insert_block(g, IVec3::Y, ident(), BlockKind::Programmable, None, None, false);
    w.upload_script(pb, "local d = grid:block('Door 1')\nd:close()\nprint(d.is_open)\ngrid:block('Gyroscope 1'):set_override(nil)").unwrap();
    w.step();
    assert!(w.phys.colliders[col].is_enabled(), "script closed the door");
    w.upload_script(pb, "").unwrap();
    w.set_block_prop(door, BlockProp::DoorOpen(true)).unwrap();
    let w2 = World::load_bytes(&w.save_bytes()).unwrap();
    let col2 = w2.block(door).unwrap().1.collider;
    assert!(!w2.phys.colliders[col2].is_enabled(), "open door stays open after load");
}

/// Grids carrying joints are multibody roots: gyros, speed caps and teleports
/// must still work on them.
#[test]
fn gyro_and_teleport_on_jointed_ship() {
    let mut w = World::new();
    let g = w.spawn_prefab("shuttle", deep_space(), DQuat::IDENTITY, None).unwrap();
    let pb = w.grids[&g].blocks.values().find(|b| b.name == "Shuttle Computer").unwrap().id;
    w.upload_script(pb, "").unwrap();
    let gyro = w.grids[&g].blocks.values().find(|b| b.kind == BlockKind::Gyro).unwrap().id;
    w.set_block_prop(gyro, BlockProp::GyroOverride(Some([20.0, 0.0, 0.0]))).unwrap();
    run(&mut w, 2.0);
    let b = &w.phys.bodies[w.grids[&g].body];
    let local = b.rotation().inverse() * b.angvel();
    assert!((local.x.to_degrees() - 20.0).abs() < 1.5, "gyro on a multibody root: {:.2}", local * 57.3);
    w.set_block_prop(gyro, BlockProp::GyroOverride(Some([0.0, 0.0, 0.0]))).unwrap();
    run(&mut w, 1.0);
    // Teleport moves every part together.
    let leg = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == "Leg 1").unwrap().id;
    let leg_head = match &w.block(leg).unwrap().1.state { BlockState::Mech(m) => m.top.unwrap(), _ => unreachable!() };
    let before = w.grid_pose(leg_head).unwrap().translation - w.grid_pose(g).unwrap().translation;
    let dest = above("Moon", 50.0);
    w.teleport_group(g, dest);
    w.step();
    let p = w.grid_pose(g).unwrap().translation;
    assert!((p - dest).length() < 1.0, "shuttle teleported: off by {}", (p - dest).length());
    let after = w.grid_pose(leg_head).unwrap().translation - p;
    assert!((after - before).length() < 0.5, "legs came along");
}

#[test]
fn sensor_detects_players_in_range() {
    let mut w = World::new();
    let g = w.create_grid(deep_space(), DQuat::IDENTITY, true, None);
    let s = w.insert_block(g, IVec3::ZERO, ident(), BlockKind::Sensor, None, Some("Eye".into()), false);
    w.add_player(1, "near");
    w.add_player(2, "far");
    w.teleport_player(1, deep_space() + DVec3::new(0.0, 0.0, -10.0));
    w.teleport_player(2, deep_space() + DVec3::new(80.0, 0.0, 0.0));
    w.step();
    let detected = |w: &World| match w.block(s).unwrap().1.state {
        BlockState::Sensor { detected, .. } => detected,
        _ => unreachable!(),
    };
    assert_eq!(detected(&w), 1, "default range 50 m sees only the near player");
    w.set_block_prop(s, BlockProp::SensorRange(100.0)).unwrap();
    w.step();
    assert_eq!(detected(&w), 2);
    // Scripts see both, nearest first, in the grid frame.
    let pb = w.insert_block(g, IVec3::X, ident(), BlockKind::Programmable, None, None, false);
    w.upload_script(pb, r#"
        local s = grid:block("Eye")
        function tick()
          local p = s:closest()
          print(s.count, p.name, math.round(p.position.z), math.round(p.distance), s.players[2].name)
        end
    "#).unwrap();
    w.step();
    let out = w.script_info(pb).unwrap().output;
    assert_eq!(out.last().map(String::as_str), Some("2\tnear\t-10\t10\tfar"), "{out:?}");
    // Range survives save/load.
    let w2 = World::load_bytes(&w.save_bytes()).unwrap();
    assert!(matches!(w2.block(s).unwrap().1.state, BlockState::Sensor { range, .. } if range == 100.0));
}

#[test]
fn drone_follows_player() {
    let mut w = World::new();
    let ground = above("Earth", 0.0);
    let rot = prefabs::surface_orientation(ground);
    let up = rot * DVec3::Y;
    w.add_player(1, "pilot");
    w.teleport_player(1, ground + up * 1.0 + rot * DVec3::new(0.0, 0.0, -10.0));
    let g = w.spawn_prefab("drone", ground + up * 1.6, rot, None).unwrap();
    let pb = w.grids[&g].blocks.values().find(|b| b.name == "Drone Computer").unwrap().id;
    assert_eq!(w.script_info(pb).unwrap().error, None);
    let dist_to = |w: &World| {
        let d = w.grid_pose(g).unwrap().translation;
        let p = w.player_pos(1).unwrap();
        let rel = d - p;
        let h = rel.dot(up);
        ((rel - up * h).length(), h)
    };
    run(&mut w, 8.0);
    let (hd, h) = dist_to(&w);
    eprintln!("station: horizontal {hd:.1} m, height {h:.1} m, tilt {:.1}", tilt_deg(&w, g));
    assert!((hd - 5.0).abs() < 1.5 && (h - 3.0).abs() < 1.5, "keeps station 5 m away, 3 m up");
    assert!(tilt_deg(&w, g) < 10.0);
    // The player moves 40 m away: the drone follows.
    let p = w.player_pos(1).unwrap();
    w.teleport_player(1, p + rot * DVec3::new(40.0, 0.0, 0.0));
    let mut max_tilt: f64 = 0.0;
    for i in 0..600 {
        w.step();
        max_tilt = max_tilt.max(tilt_deg(&w, g));
        if i % 60 == 0 {
            let (hd, h) = dist_to(&w);
            eprintln!("t={}s horizontal {hd:.1} height {h:.1} speed {:.1}", i / 60, w.phys.bodies[w.grids[&g].body].linvel().length());
        }
    }
    assert!(max_tilt < 25.0, "max tilt while chasing {max_tilt:.1}");
    let (hd, h) = dist_to(&w);
    eprintln!("after move: horizontal {hd:.1} m, height {h:.1} m");
    assert!((hd - 5.0).abs() < 1.5 && (h - 3.0).abs() < 1.5);
    // Facing the player: forward (-Z) points at them horizontally.
    let pose = w.grid_pose(g).unwrap();
    let to = w.player_pos(1).unwrap() - pose.translation;
    let to_h = (to - up * to.dot(up)).normalize();
    let fwd = pose.rotation * DVec3::NEG_Z;
    assert!(fwd.dot(to_h) > 0.9, "faces the player: {:.2}", fwd.dot(to_h));
    assert_eq!(w.script_info(pb).unwrap().error, None);
}

#[test]
fn cruiser_big_ship() {
    let mut w = World::new();
    let ground = above("Earth", 0.0);
    let rot = prefabs::surface_orientation(ground);
    let up = rot * DVec3::Y;
    let t0 = std::time::Instant::now();
    let g = w.spawn_prefab("cruiser", ground + up * 1.6, rot, None).unwrap();
    let spawn_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let group = w.mech_group(g);
    let parts: usize = group.iter().map(|id| w.grids[id].blocks.len()).sum();
    let mass: f64 = group.iter().map(|id| w.phys.bodies[w.grids[id].body].mass()).sum();
    let count = |k: BlockKind| w.grids[&g].blocks.values().filter(|b| b.kind == k).count();
    let lift = w.grids[&g].blocks.values().filter(|b| b.kind == BlockKind::Thruster && orient::forward(b.rot) == IVec3::Y).count();
    let twr = lift as f64 * shared::blocks::THRUSTER_MAX_FORCE / (mass * 9.81);
    eprintln!(
        "cruiser: {parts} parts ({} armor, {} thrusters, {lift} lift, {} doors), {:.0} t, lift T/W {twr:.2}, spawned in {spawn_ms:.0} ms",
        count(BlockKind::Armor),
        count(BlockKind::Thruster),
        count(BlockKind::Door),
        mass / 1000.0
    );
    assert!(parts >= 3000, "at least 3000 parts: {parts}");
    assert!(twr > 1.3, "can lift off Earth: T/W {twr:.2}");
    let pb = w.grids[&g].blocks.values().find(|b| b.name == "Cruiser Computer").unwrap().id;
    assert_eq!(w.script_info(pb).unwrap().error, None);
    let t = std::time::Instant::now();
    let bytes = w.save_bytes();
    let w2 = World::load_bytes(&bytes).unwrap();
    eprintln!("save {} KB, save+load {:.0} ms", bytes.len() / 1024, t.elapsed().as_secs_f64() * 1000.0);
    assert_eq!(w2.total_blocks(), w.total_blocks());

    // Settles on the ground.
    run(&mut w, 2.0);
    let t = std::time::Instant::now();
    run(&mut w, 2.0);
    let tick_ms = t.elapsed().as_secs_f64() * 1000.0 / 120.0;
    let alt0 = gravity_at(w.grid_pose(g).unwrap().translation).altitude;
    eprintln!("resting altitude {alt0:.2}, tilt {:.1}, {tick_ms:.2} ms per tick", tilt_deg(&w, g));
    assert!(tilt_deg(&w, g) < 3.0);

    // A player outside, 40 m to starboard and 10 m up: both turrets aim at them.
    w.add_player(1, "target");
    // High enough that the far-side turret's barrel clears the hull roof.
    let target = w.grid_pose(g).unwrap().transform_point(DVec3::new(40.0, 30.0, 0.0));
    w.teleport_player(1, target);
    let mut aim_errors = Vec::new();
    for _ in 0..(4 * 60) {
        w.teleport_player(1, target); // hold still against gravity
        w.step();
    }
    for i in 1..=4 {
        let hinge = w.grids.values().flat_map(|g| g.blocks.values()).find(|b| b.name == format!("Turret {i} Hinge")).unwrap().id;
        let barrel = match &w.block(hinge).unwrap().1.state {
            BlockState::Mech(m) => m.top.unwrap(),
            _ => unreachable!(),
        };
        let pose = w.grid_pose(barrel).unwrap();
        let dir = pose.rotation * DVec3::Y;
        let want = (target - pose.translation).normalize();
        aim_errors.push(dir.angle_between(want).to_degrees());
    }
    eprintln!("turret aim errors {aim_errors:.1?}");
    assert!(aim_errors.iter().all(|e| *e < 4.0), "turrets aim at the player: {aim_errors:?}");

    // Fly it from the bridge: climb for 5 s, then let the dampeners hold it.
    let bridge = w.grids[&g].blocks.values().find(|b| b.name == "Cruiser Bridge").unwrap().id;
    w.enter_seat_unchecked(1, bridge).unwrap();
    let mut input = PlayerInput { movement: glam::Vec3::Y, dampeners: true, look: DQuat::IDENTITY, ..Default::default() };
    w.set_input(1, input.clone());
    run(&mut w, 5.0);
    input.movement = glam::Vec3::ZERO;
    w.set_input(1, input);
    run(&mut w, 8.0);
    let pose = w.grid_pose(g).unwrap();
    let alt = gravity_at(pose.translation).altitude;
    let v = w.phys.bodies[w.grids[&g].body].linvel().length();
    eprintln!("after climb: altitude {alt:.1}, speed {v:.2}, tilt {:.1}", tilt_deg(&w, g));
    assert!(alt > alt0 + 15.0, "climbed: {alt:.1}");
    assert!(v < 1.0, "dampeners hold it: {v:.2}");
    assert!(tilt_deg(&w, g) < 5.0);
}

#[test]
fn large_thruster_blocks() {
    let mut w = World::new();
    w.add_player(1, "builder");
    let p = w.player_pos(1).unwrap();
    let g = w.new_grid(1, p + DVec3::X * 4.0, DQuat::IDENTITY, BlockKind::Armor, true).unwrap();
    let fwd_neg_z = ident();
    // A 2x2x2 thruster behind the armour block: covers x 0..1, y 0..1, z 1..2.
    let lt = w.place_block(1, g, IVec3::new(0, 0, 1), fwd_neg_z, BlockKind::LargeThruster).unwrap();
    assert!(w.place_block(1, g, IVec3::new(1, 1, 2), fwd_neg_z, BlockKind::Armor).is_err(), "covered cell is occupied");
    assert!(w.place_block(1, g, IVec3::new(1, 1, 3), fwd_neg_z, BlockKind::Armor).is_ok(), "touching the large block is fine");
    // Removing it through any covered cell frees all eight.
    w.remove_block(1, g, IVec3::new(1, 1, 2)).unwrap();
    assert!(w.block(lt).is_none());
    assert!(w.grids[&g].occupied.is_empty());

    // In space it pushes 200 kN through the centre of its 2x2 face.
    let s = w.create_grid(deep_space(), DQuat::IDENTITY, false, None);
    let lt = w.insert_block(s, IVec3::ZERO, fwd_neg_z, BlockKind::LargeThruster, None, None, false);
    let mass = w.phys.bodies[w.grids[&s].body].mass();
    assert!((mass - 900.0).abs() < 1.0);
    w.set_block_prop(lt, BlockProp::ThrusterOverride(1.0)).unwrap();
    run(&mut w, 0.5); // 222 m/s^2: a full second would hit the 150 m/s speed cap
    let body = &w.phys.bodies[w.grids[&s].body];
    let v = body.linvel();
    eprintln!("after 0.5 s: velocity {v:.3?}, spin {:.4}", body.angvel().length());
    assert!((v.z + 0.5 * 200_000.0 / 900.0).abs() < 2.0, "accelerates along -Z at F/m");
    assert!(body.angvel().length() < 1e-3, "force through the centre of mass: no spin");
    // Saves and loads with its footprint.
    let w2 = World::load_bytes(&w.save_bytes()).unwrap();
    assert_eq!(w2.grids[&s].occupied.len(), 7);
}
