//! Save / load maps as JSON files under `assets/maps/<name>.json`.
//!
//! The editor writes the map with `serde_json`; the renderer (jade) reads the
//! exact same format from its own copy of these `Save*` structs. The two files
//! must stay in sync — the JSON is the shared contract between the projects.
//!
//! A map is addressed by **name**, which becomes the file name. The current
//! name lives in [`SaveState`]; saving/loading is triggered from the egui
//! toolbar or the keyboard shortcuts (Alt+S / Ctrl+L).

use bevy::math::Vec2;
use bevy::prelude::*;
use serde::{ Deserialize, Serialize };

use crate::map::*;

//------------------------------MAP SAVE PLUGIN----------------------

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveState>();
        app.add_systems(Update, (save_map_system, load_map_system));
    }
}

//-------------------------------SAVE STATE---------------------------

/// The map being worked on and any pending save/load request. The egui toolbar
/// and the keyboard shortcuts only *flag* an operation here; the actual file
/// I/O happens in the `Update` systems so loading can swap the `Map` resource
/// through deferred `Commands` instead of mid-frame.
#[derive(Resource, Default)]
pub struct SaveState {
    /// Name of the current map; becomes `assets/maps/<name>.json`.
    pub map_name: String,
    /// Set by the UI/keyboard to request a save on the next frame.
    pub save_pending: bool,
    /// Set by the UI/keyboard to request a load on the next frame.
    pub load_pending: bool,
}

//------------------------------FILES / PATHS-------------------------

/// Folder (relative to the crate root) that saved maps live in.
pub const MAPS_DIR: &str = "assets/maps";

/// Sanitize a map name into a safe file name and build its `.json` path.
/// Only `[A-Za-z0-9_-]` survives; an empty name falls back to `"unnamed"`.
pub fn map_path(name: &str) -> String {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = if clean.is_empty() { "unnamed" } else { &clean };
    format!("{MAPS_DIR}/{file}.json")
}

//------------------------------DISK MODEL---------------------------

/// Current save-format version. Raise it when the `Save*` structs change and
/// bump `SAVE_VERSION` so old/new files fail loudly instead of misreading.
pub const SAVE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveMap {
    /// `#[serde(default)]` lets files saved before versioning load as v0.
    #[serde(default)]
    pub version: u32,
    pub vertices: Vec<Vec2>,
    pub sectors: Vec<SaveSector>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveSector {
    pub walls: Vec<SaveLine>,
    pub obstacles: Vec<SaveObstacle>,
    pub floor_height: f32,
    pub ceiling_height: f32,
    pub floor_texture: String, // e.g. "floor_texture.png"
    pub ceiling_texture: String,
    pub id: usize,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveLine {
    pub start_idx: usize,
    pub end_idx: usize,
    pub front: SaveSide,
    pub back: Option<SaveSide>, // Some => this line is a portal
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveSide {
    pub upper: Option<String>,
    pub middle: Option<String>,
    pub lower: Option<String>,
    pub facing: usize, // the sector id this side belongs to
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SaveObstacle {
    pub id: usize,
    pub edges: Vec<SaveLine>,
    pub bottom: f32,
    pub top: f32,
    pub side_texture: String,
    pub top_texture: String,
    pub bottom_texture: String,
}

//----------------------RUNTIME MAP <-> DISK MODEL--------------------

/// Resolve a texture handle back to the path it was loaded from. Falls back to
/// the default wall texture if the handle isn't backed by a loaded file.
fn texture_path(server: &AssetServer, handle: &Handle<Image>) -> String {
    server
        .get_path(handle.id())
        .map(|p| p.to_string())
        .unwrap_or_else(|| "texture.png".to_string())
}

/// Editor `Map` → disk model (handles become path strings).
fn to_save(map: &Map, server: &AssetServer) -> SaveMap {
    SaveMap {
        version: SAVE_VERSION,
        vertices: map.vertices.clone(),
        sectors: map.sectors
            .iter()
            .map(|s| SaveSector {
                walls: s.walls
                    .iter()
                    .map(|w| line_to_save(w, server))
                    .collect(),
                obstacles: s.obstacles
                    .iter()
                    .map(|o| obstacle_to_save(o, server))
                    .collect(),
                floor_height: s.floor_height,
                ceiling_height: s.ceiling_height,
                floor_texture: texture_path(server, &s.floor_texture),
                ceiling_texture: texture_path(server, &s.ceiling_texture),
                id: s.id,
            })
            .collect(),
    }
}

fn line_to_save(w: &LineDef, server: &AssetServer) -> SaveLine {
    SaveLine {
        start_idx: w.start_idx,
        end_idx: w.end_idx,
        front: side_to_save(&w.front_side_def, server),
        back: w.back_side_def.as_ref().map(|s| side_to_save(s, server)),
    }
}

fn side_to_save(s: &SideDef, server: &AssetServer) -> SaveSide {
    SaveSide {
        upper: s.textures.upper.as_ref().map(|h| texture_path(server, h)),
        middle: s.textures.middle.as_ref().map(|h| texture_path(server, h)),
        lower: s.textures.lower.as_ref().map(|h| texture_path(server, h)),
        facing: s.facing,
    }
}

fn obstacle_to_save(o: &Obstacle, server: &AssetServer) -> SaveObstacle {
    SaveObstacle {
        id: o.id,
        edges: o.edges
            .iter()
            .map(|w| line_to_save(w, server))
            .collect(),
        bottom: o.bottom,
        top: o.top,
        side_texture: texture_path(server, &o.side_texture),
        top_texture: texture_path(server, &o.top_texture),
        bottom_texture: texture_path(server, &o.bottom_texture),
    }
}

/// Disk model → editor `Map` (path strings become handles). `server.load`
/// deduplicates by path, so shared textures stay shared after loading.
fn from_save(save: SaveMap, server: &AssetServer) -> Map {
    let vertices = save.vertices;
    let sectors = save.sectors
        .into_iter()
        .map(|s| Sector {
            walls: s.walls
                .into_iter()
                .enumerate()
                .map(|(i, w)| line_from_save(w, WallId::new(s.id, i), server))
                .collect(),
            obstacles: s.obstacles
                .into_iter()
                .map(|o| obstacle_from_save(o, s.id, server))
                .collect(),
            floor_height: s.floor_height,
            ceiling_height: s.ceiling_height,
            floor_texture: server.load(&s.floor_texture),
            ceiling_texture: server.load(&s.ceiling_texture),
            id: s.id,
        })
        .collect();

    Map { vertices, sectors }
}

fn line_from_save(w: SaveLine, id: WallId, server: &AssetServer) -> LineDef {
    let side = |s: SaveSide|
        SideDef::new(
            SideDefTextures {
                upper: s.upper.map(|t| server.load(&t)),
                middle: s.middle.map(|t| server.load(&t)),
                lower: s.lower.map(|t| server.load(&t)),
            },
            s.facing
        );

    LineDef {
        start_idx: w.start_idx,
        end_idx: w.end_idx,
        front_side_def: side(w.front),
        back_side_def: w.back.map(side),
        id,
    }
}

fn obstacle_from_save(o: SaveObstacle, sector_id: usize, server: &AssetServer) -> Obstacle {
    let edges = o.edges
        .into_iter()
        .enumerate()
        .map(|(i, w)| line_from_save(w, WallId::new(sector_id, i), server))
        .collect();

    Obstacle {
        id: o.id,
        edges,
        bottom: o.bottom,
        top: o.top,
        side_texture: server.load(&o.side_texture),
        top_texture: server.load(&o.top_texture),
        bottom_texture: server.load(&o.bottom_texture),
    }
}

//------------------------------FILE I/O------------------------------

/// Serialize `map` and write it to `path`, creating `assets/maps/` if needed.
pub fn save_map_to_file(map: &Map, server: &AssetServer, path: &str) -> Result<(), String> {
    std::fs::create_dir_all(MAPS_DIR).map_err(|e| e.to_string())?;
    let save = to_save(map, server);
    let json = serde_json::to_string_pretty(&save).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Read `path`, parse it, and build a `Map`. Errors (missing file, bad JSON,
/// unsupported version) are returned as `Err(String)`.
pub fn load_map_from_file(path: &str, server: &AssetServer) -> Result<Map, String> {
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let save: SaveMap = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    if save.version > SAVE_VERSION {
        return Err(format!(
            "map format v{} is newer than this editor supports (v{})",
            save.version, SAVE_VERSION
        ));
    }
    Ok(from_save(save, server))
}

//------------------------------SYSTEMS-------------------------------

/// Save on Alt+S or a toolbar "Save" click. The name is taken from
/// [`SaveState::map_name`] at the moment the operation runs.
fn save_map_system(
    map: Res<Map>,
    asset_server: Res<AssetServer>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SaveState>
) {
    if keyboard.just_pressed(KeyCode::KeyS) && keyboard.pressed(KeyCode::AltLeft) {
        state.save_pending = true;
    }
    if !std::mem::take(&mut state.save_pending) {
        return;
    }
    let path = map_path(&state.map_name);
    match save_map_to_file(&map, &asset_server, &path) {
        Ok(()) => info!("Saved map to {path}"),
        Err(e) => error!("Failed to save map: {e}"),
    }
}

/// Load on Ctrl+L or a toolbar "Load" click. Replaces the `Map` resource via
/// deferred `Commands`, so other systems never observe a half-swapped map.
fn load_map_system(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SaveState>
) {
    if keyboard.just_pressed(KeyCode::KeyL) && keyboard.pressed(KeyCode::ControlLeft) {
        state.load_pending = true;
    }
    if !std::mem::take(&mut state.load_pending) {
        return;
    }
    let path = map_path(&state.map_name);
    match load_map_from_file(&path, &asset_server) {
        Ok(map) => {
            info!("Loaded map from {path}");
            commands.insert_resource(map);
        }
        Err(e) => error!("Failed to load map: {e}"),
    }
}
