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
    let pb = w.grids[&g].blocks[&IVec3::Y].id;
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
    let pb = w.grids[&g].blocks[&IVec3::Y].id;
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
    let cockpit = w.grids[&g].blocks[&IVec3::ZERO].id;
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
    let cases: [(&str, &[(&str, f64, f64)]); 4] = [
        ("elevator", &[("Lift 1", 0.3, 4.7), ("Lift 2", 0.3, 4.7), ("Lift 3", 0.3, 4.7)]),
        ("crane", &[("Boom Hinge", -50.0, -10.0), ("Boom Piston", 1.0, 4.0), ("Hook Hinge", -50.0, -10.0)]),
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
        for (i, (n, want_lo, want_hi)) in mechs.iter().enumerate() {
            eprintln!("{name}/{n}: {:.2} .. {:.2}", lo[i], hi[i]);
            // Nothing may slam into the hinge limits (that means it toppled).
            assert!(lo[i] > -85.0 && hi[i] < 85.0, "{name}/{n} hit its limits: {:.1}..{:.1}", lo[i], hi[i]);
            assert!(lo[i] < *want_lo && hi[i] > *want_hi, "{name}/{n} range {:.2}..{:.2}, wanted below {want_lo} and above {want_hi}", lo[i], hi[i]);
        }
    }
}

