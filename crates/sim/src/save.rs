//! Saving and loading the world (postcard-encoded).

use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum SavedState {
    Plain,
    Thruster { override_: f64 },
    Mech { top: Option<GridId>, velocity: f64, target: Option<f64> },
    Program { source: String, storage: String },
    Cockpit,
}

#[derive(Serialize, Deserialize)]
pub struct SavedBlock {
    pub id: BlockId,
    pub kind: BlockKind,
    pub cell: IVec3,
    pub rot: u8,
    pub name: String,
    pub state: SavedState,
}

#[derive(Serialize, Deserialize)]
pub struct SavedGrid {
    pub id: GridId,
    pub is_static: bool,
    pub pos: DVec3,
    pub rot: DQuat,
    pub linvel: DVec3,
    pub angvel: DVec3,
    pub cruise: bool,
    pub mount: Option<BlockId>,
    pub blocks: Vec<SavedBlock>,
}

#[derive(Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub tick: u64,
    pub next_grid: GridId,
    pub next_block: BlockId,
    pub grids: Vec<SavedGrid>,
    pub players: Vec<(String, DVec3)>,
}

const SAVE_VERSION: u32 = 1;

impl World {
    pub fn save(&self) -> SaveData {
        let mut players: HashMap<String, DVec3> = self.saved_players.clone();
        for p in self.players.values() {
            players.insert(p.name.clone(), self.phys.bodies[p.body].translation());
        }
        let grids = self
            .grids
            .values()
            .map(|g| {
                let body = &self.phys.bodies[g.body];
                let mut blocks: Vec<SavedBlock> = g
                    .blocks
                    .values()
                    .map(|b| SavedBlock {
                        id: b.id,
                        kind: b.kind,
                        cell: b.cell,
                        rot: b.rot,
                        name: b.name.clone(),
                        state: match &b.state {
                            BlockState::Plain => SavedState::Plain,
                            BlockState::Thruster(t) => SavedState::Thruster { override_: t.override_ },
                            BlockState::Mech(m) => SavedState::Mech { top: m.top, velocity: m.velocity, target: m.target },
                            BlockState::Program(p) => SavedState::Program { source: p.source.clone(), storage: p.storage.clone() },
                            BlockState::Cockpit { .. } => SavedState::Cockpit,
                        },
                    })
                    .collect();
                blocks.sort_by_key(|b| b.id);
                SavedGrid {
                    id: g.id,
                    is_static: g.is_static,
                    pos: body.translation(),
                    rot: *body.rotation(),
                    linvel: body.linvel(),
                    angvel: body.angvel(),
                    cruise: g.cruise,
                    mount: g.mount,
                    blocks,
                }
            })
            .collect();
        SaveData {
            version: SAVE_VERSION,
            tick: self.tick,
            next_grid: self.next_grid,
            next_block: self.next_block,
            grids,
            players: players.into_iter().collect(),
        }
    }

    pub fn save_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(&self.save()).expect("save encodes")
    }

    pub fn load_bytes(bytes: &[u8]) -> Result<World, String> {
        let data: SaveData = postcard::from_bytes(bytes).map_err(|e| e.to_string())?;
        Self::load(data)
    }

    pub fn load(data: SaveData) -> Result<World, String> {
        if data.version != SAVE_VERSION {
            return Err(format!("unsupported save version {}", data.version));
        }
        let mut w = World::new();
        w.tick = data.tick;
        let mut mechs = Vec::new();
        let mut programs = Vec::new();
        for g in &data.grids {
            w.create_grid(g.pos, g.rot, g.is_static, Some(g.id));
            let body = &mut w.phys.bodies[w.grids[&g.id].body];
            body.set_linvel(g.linvel, false);
            body.set_angvel(g.angvel, false);
            w.grids.get_mut(&g.id).unwrap().cruise = g.cruise;
            w.grids.get_mut(&g.id).unwrap().mount = g.mount;
            for b in &g.blocks {
                w.insert_block(g.id, b.cell, b.rot, b.kind, Some(b.id), Some(b.name.clone()), false);
                let blk = w.block_mut(b.id).unwrap();
                match (&b.state, &mut blk.state) {
                    (SavedState::Thruster { override_ }, BlockState::Thruster(t)) => t.override_ = *override_,
                    (SavedState::Mech { top, velocity, target }, BlockState::Mech(m)) => {
                        m.top = *top;
                        m.velocity = *velocity;
                        m.target = *target;
                        mechs.push(b.id);
                    }
                    (SavedState::Program { source, storage }, BlockState::Program(p)) => {
                        p.storage = storage.clone();
                        programs.push((b.id, source.clone()));
                    }
                    _ => {}
                }
            }
        }
        for m in mechs {
            w.attach_joint(m);
        }
        for (pb, src) in programs {
            let _ = w.upload_script(pb, &src);
        }
        w.next_grid = w.next_grid.max(data.next_grid);
        w.next_block = w.next_block.max(data.next_block);
        w.saved_players = data.players.into_iter().collect();
        w.read_mech_values();
        w.events.clear();
        Ok(w)
    }
}
