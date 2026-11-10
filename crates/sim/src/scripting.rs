//! Programmable blocks: snapshot views, parallel execution, command application.

use crate::*;
use rayon::prelude::*;
use script::{BlockInfo, Command, GridInfo, GridView, RunResult, ScriptHost};
use shared::blocks::*;
use shared::protocol::BlockProp;
use shared::orient;
use shared::solar::{BODIES, gravity_at};

impl World {
    /// Build the read-only view a programmable block sees.
    pub fn build_view(&self, pb: BlockId) -> Option<GridView> {
        let (g, me) = self.block(pb)?;
        let body = &self.phys.bodies[g.body];
        let gp = *body.position();
        let inv = gp.rotation.inverse();
        let grav = gravity_at(body.center_of_mass());
        let vel = body.linvel();
        let grid = GridInfo {
            position: gp.translation,
            velocity: vel,
            local_velocity: inv * vel,
            angular_velocity: inv * body.angvel(),
            gravity: inv * grav.accel,
            altitude: grav.altitude,
            planet: BODIES[grav.nearest].name.to_string(),
            mass: body.mass(),
            speed: vel.length(),
            cruise: g.cruise,
        };
        let mut blocks = Vec::new();
        for gid in self.mech_group(g.id) {
            let og = &self.grids[&gid];
            let op = *self.phys.bodies[og.body].position();
            let rel_rot = inv * op.rotation;
            for b in og.blocks.values() {
                let (mut info_thr, mut thr_override, mut thr_out) = (0.0, 0.0, 0.0);
                let (mut pp, mut pv, mut ha, mut hv, mut ht) = (0.0, 0.0, 0.0, 0.0, None);
                match &b.state {
                    BlockState::Thruster(t) => {
                        info_thr = THRUSTER_MAX_FORCE;
                        thr_override = t.override_;
                        thr_out = t.output * THRUSTER_MAX_FORCE;
                    }
                    BlockState::Mech(m) if b.kind == BlockKind::Piston => {
                        pp = m.value;
                        pv = m.velocity;
                    }
                    BlockState::Mech(m) => {
                        ha = m.value;
                        hv = m.velocity;
                        ht = m.target;
                    }
                    _ => {}
                }
                blocks.push(BlockInfo {
                    id: b.id,
                    kind: b.kind,
                    name: b.name.clone(),
                    forward: rel_rot * orient::forward(b.rot).as_dvec3(),
                    up: rel_rot * orient::up(b.rot).as_dvec3(),
                    position: gp.inverse_transform_point(op.transform_point(b.cell.as_dvec3())),
                    same_grid: gid == g.id,
                    thrust_max: info_thr,
                    thrust_override: thr_override,
                    thrust_current: thr_out,
                    piston_position: pp,
                    piston_velocity: pv,
                    hinge_angle: ha,
                    hinge_velocity: hv,
                    hinge_target: ht,
                });
            }
        }
        Some(GridView {
            me: pb,
            me_name: me.name.clone(),
            tick: self.tick,
            dt: TICK_DT,
            grid,
            blocks,
        })
    }

    /// Run every programmable block that is due this tick (in parallel).
    pub(crate) fn run_scripts(&mut self) -> u32 {
        // 1. Collect views (immutable pass).
        let mut views: HashMap<BlockId, GridView> = HashMap::new();
        for g in self.grids.values() {
            for b in g.blocks.values() {
                if let BlockState::Program(p) = &b.state {
                    if p.host.is_none() {
                        continue;
                    }
                    let due = (self.tick + b.id as u64) % p.frequency.max(1) as u64 == 0;
                    if due || !p.pending_messages.is_empty() {
                        if let Some(v) = self.build_view(b.id) {
                            views.insert(b.id, v);
                        }
                    }
                }
            }
        }
        if views.is_empty() {
            return 0;
        }
        // 2. Run in parallel; each host only touches its own state.
        let mut jobs: Vec<(BlockId, &mut Program, GridView)> = Vec::new();
        for g in self.grids.values_mut() {
            for b in g.blocks.values_mut() {
                if let BlockState::Program(p) = &mut b.state {
                    if let Some(v) = views.remove(&b.id) {
                        jobs.push((b.id, p.as_mut(), v));
                    }
                }
            }
        }
        let tick = self.tick;
        let results: Vec<(BlockId, Vec<(Vec<BlockId>, RunResult)>)> = jobs
            .into_par_iter()
            .map(|(id, p, view)| {
                let start = Instant::now();
                let allowed: Vec<BlockId> = view.blocks.iter().map(|b| b.id).collect();
                let mut out = Vec::new();
                let host = p.host.as_ref().unwrap();
                for msg in std::mem::take(&mut p.pending_messages) {
                    out.push((allowed.clone(), host.message(view.clone(), &msg)));
                }
                if (tick + id as u64) % p.frequency.max(1) as u64 == 0 {
                    out.push((allowed, host.tick(view)));
                }
                p.storage = host.storage();
                p.last_run_us = start.elapsed().as_micros() as u64;
                (id, out)
            })
            .collect();
        // 3. Apply commands and output in a deterministic order.
        let mut results = results;
        results.sort_by_key(|r| r.0);
        let mut count = 0;
        for (pb, runs) in results {
            for (allowed, r) in runs {
                count += 1;
                self.apply_run_result(pb, &allowed, r);
            }
        }
        count
    }

    fn apply_run_result(&mut self, pb: BlockId, allowed: &[BlockId], r: RunResult) {
        for c in r.commands {
            let target = match &c {
                Command::ThrusterOverride(id, _)
                | Command::PistonVelocity(id, _)
                | Command::HingeVelocity(id, _)
                | Command::HingeTarget(id, _) => Some(*id),
                _ => None,
            };
            if let Some(t) = target {
                if !allowed.contains(&t) {
                    continue;
                }
            }
            match c {
                Command::ThrusterOverride(id, v) => {
                    let _ = self.set_block_prop(id, BlockProp::ThrusterOverride(v as f32));
                }
                Command::PistonVelocity(id, v) => {
                    let _ = self.set_block_prop(id, BlockProp::PistonVelocity(v as f32));
                }
                Command::HingeVelocity(id, v) => {
                    let _ = self.set_block_prop(id, BlockProp::HingeVelocity(v as f32));
                }
                Command::HingeTarget(id, t) => {
                    let _ = self.set_block_prop(id, BlockProp::HingeTarget(t.map(|t| t as f32)));
                }
                Command::SetFrequency(f) => {
                    let _ = self.set_block_prop(pb, BlockProp::ScriptFrequency(f));
                }
                Command::SetCruise(on) => {
                    if let Some((gid, _)) = self.block_loc.get(&pb).copied() {
                        let g = self.grids.get_mut(&gid).unwrap();
                        g.cruise = on;
                        self.phys.bodies[g.body].enable_ccd(on);
                    }
                }
            }
        }
        if let Some(BlockState::Program(p)) = self.block_mut(pb).map(|b| &mut b.state) {
            push_output(p, r.output);
            if p.error != r.error {
                p.error = r.error;
                p.output_dirty = true;
            }
        }
    }

    /// Replace a programmable block's script and compile it.
    pub fn upload_script(&mut self, pb: BlockId, source: &str) -> Result<(), String> {
        if source.len() > 64 * 1024 {
            return Err("script too large (64 KiB max)".into());
        }
        let view = self.build_view(pb).ok_or("no such block")?;
        let Some(BlockState::Program(p)) = self.block_mut(pb).map(|b| &mut b.state) else {
            return Err("not a programmable block".into());
        };
        p.source = source.to_string();
        p.output.clear();
        p.new_output.clear();
        p.output_dirty = true;
        p.frequency = 1;
        if source.trim().is_empty() {
            p.host = None;
            p.error = None;
            return Ok(());
        }
        match ScriptHost::new(source, &p.storage, &view) {
            Ok((host, init)) => {
                p.storage = host.storage();
                p.host = Some(host);
                p.error = None;
                push_output(p, init.output);
                let cmds = RunResult { commands: init.commands, output: Vec::new(), error: None };
                let allowed: Vec<BlockId> = view.blocks.iter().map(|b| b.id).collect();
                self.apply_run_result(pb, &allowed, cmds);
                Ok(())
            }
            Err(e) => {
                p.host = None;
                p.error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Queue `on_message(arg)` for the next tick.
    pub fn run_script(&mut self, pb: BlockId, arg: &str) -> Result<(), String> {
        let Some(BlockState::Program(p)) = self.block_mut(pb).map(|b| &mut b.state) else {
            return Err("not a programmable block".into());
        };
        if p.host.is_none() {
            return Err("no compiled script".into());
        }
        if p.pending_messages.len() < 16 {
            p.pending_messages.push(arg.chars().take(256).collect());
        }
        Ok(())
    }

    pub fn script_info(&self, pb: BlockId) -> Option<shared::protocol::ScriptInfo> {
        let (_, b) = self.block(pb)?;
        let BlockState::Program(p) = &b.state else { return None };
        Some(shared::protocol::ScriptInfo {
            block: pb,
            source: p.source.clone(),
            output: p.output.iter().cloned().collect(),
            error: p.error.clone(),
            frequency: p.frequency,
        })
    }
}

fn push_output(p: &mut Program, lines: Vec<String>) {
    for l in lines {
        let l: String = l.chars().take(200).collect();
        p.output.push_back(l.clone());
        p.new_output.push(l);
    }
    while p.output.len() > script::MAX_OUTPUT_LINES {
        p.output.pop_front();
    }
    if p.new_output.len() > script::MAX_OUTPUT_LINES {
        let n = p.new_output.len() - script::MAX_OUTPUT_LINES;
        p.new_output.drain(..n);
    }
}
