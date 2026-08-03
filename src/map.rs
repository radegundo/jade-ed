//! Indexed-vertex map data model.
//!
//! The editor currently consumes only the geometry (vertices + wall indices).
//! Texture/side-def/height fields and the query helpers are part of the full
//! data model for the raycaster renderer and future editor tools, so they are
//! kept intact but may be unused for now.
#![allow(dead_code)]

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;

//------------------------------MAP PLUGIN-------------------------

pub struct MapPlugin;
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_map);
    }
}

//------------------------------MAP DATA STRUCTURES-----------------

#[derive(Resource, Default, Clone)]
pub struct Map {
    pub vertices: Vec<Vec2>,
    pub sectors: Vec<Sector>,
}

#[derive(Clone, Default)]
pub struct Sector {
    pub walls: Vec<LineDef>,
    pub obstacles: Vec<Obstacle>,
    pub floor_height: f32,
    pub ceiling_height: f32,
    pub floor_texture: Handle<Image>,
    pub ceiling_texture: Handle<Image>,
    pub id: usize,
}

#[derive(Clone, Default)]
pub struct Obstacle {
    pub id: usize,
    pub edges: Vec<LineDef>,
    pub bottom: f32,
    pub top: f32,
    pub side_texture: Handle<Image>,
    pub top_texture: Handle<Image>,
    pub bottom_texture: Handle<Image>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WallId {
    pub sector: usize,
    pub index: usize,
}

impl WallId {
    pub fn new(sector: usize, index: usize) -> Self {
        Self { sector, index }
    }
}

#[derive(Clone, Default)]
pub struct LineDef {
    pub start_idx: usize,
    pub end_idx: usize,
    pub front_side_def: SideDef,
    pub back_side_def: Option<SideDef>,
    pub id: WallId,
}

impl LineDef {
    /// Resolve start position from vertex pool
    pub fn start<'a>(&self, vertices: &'a [Vec2]) -> &'a Vec2 {
        &vertices[self.start_idx]
    }
    /// Resolve end position from vertex pool
    pub fn end<'a>(&self, vertices: &'a [Vec2]) -> &'a Vec2 {
        &vertices[self.end_idx]
    }
}

/// Adds a vertex to the pool. Returns the index of an existing vertex
/// if the exact Vec2 already exists (required so portals share vertices).
fn add_vertex(pool: &mut Vec<Vec2>, pos: Vec2) -> usize {
    if let Some(idx) = pool.iter().position(|&v| v == pos) {
        idx
    } else {
        pool.push(pos);
        pool.len() - 1
    }
}

#[derive(Clone, Default)]
pub struct SideDef {
    pub textures: SideDefTextures,
    pub facing: usize,
}

impl SideDef {
    pub fn new(textures: SideDefTextures, facing: usize) -> Self {
        Self { textures, facing }
    }
}

#[derive(Clone, Default)]
pub struct SideDefTextures {
    pub upper: Option<Handle<Image>>,
    pub middle: Option<Handle<Image>>,
    pub lower: Option<Handle<Image>>,
}

//-------------HELPER FUNCTIONS FOR SECTOR BUILDING----------------

pub fn wall(
    vertex_pool: &mut Vec<Vec2>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    texture: Handle<Image>,
    id: WallId
) -> LineDef {
    let start_idx = add_vertex(vertex_pool, Vec2::new(x0, y0));
    let end_idx = add_vertex(vertex_pool, Vec2::new(x1, y1));
    LineDef {
        start_idx,
        end_idx,
        front_side_def: SideDef::new(
            SideDefTextures { upper: None, middle: Some(texture), lower: None },
            id.sector
        ),
        back_side_def: None,
        id,
    }
}

pub fn portal(
    vertex_pool: &mut Vec<Vec2>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    upper_texture: Handle<Image>,
    lower_texture: Handle<Image>,
    id: WallId,
    front_sector: usize,
    back_sector: usize
) -> LineDef {
    let start_idx = add_vertex(vertex_pool, Vec2::new(x0, y0));
    let end_idx = add_vertex(vertex_pool, Vec2::new(x1, y1));
    LineDef {
        start_idx,
        end_idx,
        front_side_def: SideDef::new(
            SideDefTextures {
                upper: Some(upper_texture.clone()),
                middle: None,
                lower: Some(lower_texture.clone()),
            },
            front_sector
        ),
        back_side_def: Some(
            SideDef::new(
                SideDefTextures {
                    upper: Some(upper_texture.clone()),
                    middle: None,
                    lower: Some(lower_texture.clone()),
                },
                back_sector
            )
        ),
        id,
    }
}

//------------- OBSTACLE BUILDER ---------------

pub struct ObstacleBuilder<'a> {
    vertex_pool: &'a mut Vec<Vec2>,
    edges: Vec<LineDef>,
    bottom: f32,
    top: f32,
    id: usize,
    sector_id: usize,
    wall_counter: usize,
    side_texture: Handle<Image>,
    top_texture: Handle<Image>,
    bottom_texture: Handle<Image>,
}

impl<'a> ObstacleBuilder<'a> {
    pub fn new(
        vertex_pool: &'a mut Vec<Vec2>,
        id: usize,
        sector_id: usize,
        bottom: f32,
        top: f32,
        side_texture: Handle<Image>,
        top_texture: Handle<Image>,
        bottom_texture: Handle<Image>
    ) -> Self {
        Self {
            vertex_pool,
            edges: Vec::new(),
            bottom,
            top,
            id,
            sector_id,
            wall_counter: 0,
            side_texture,
            top_texture,
            bottom_texture,
        }
    }

    pub fn edge(mut self, x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        let wall_id = WallId::new(self.sector_id, 1000 + self.id * 100 + self.wall_counter);
        let edge = wall(self.vertex_pool, x0, y0, x1, y1, self.side_texture.clone(), wall_id);
        self.edges.push(edge);
        self.wall_counter += 1;
        self
    }

    pub fn build(self) -> Obstacle {
        Obstacle {
            id: self.id,
            edges: self.edges,
            bottom: self.bottom,
            top: self.top,
            side_texture: self.side_texture,
            top_texture: self.top_texture,
            bottom_texture: self.bottom_texture,
        }
    }
}

/// Builds a rectangular box obstacle.
/// Edges wound CLOCKWISE so normals face OUTWARD.
/// side_texture: the four vertical faces
/// top_texture:  the top horizontal cap
/// bottom_texture: the bottom horizontal cap
pub fn rect_obstacle(
    vertex_pool: &mut Vec<Vec2>,
    id: usize,
    sector_id: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    bottom: f32,
    top: f32,
    side_texture: Handle<Image>,
    top_texture: Handle<Image>,
    bottom_texture: Handle<Image>
) -> Obstacle {
    ObstacleBuilder::new(vertex_pool, id, sector_id, bottom, top, side_texture, top_texture, bottom_texture)
        .edge(x1, y0, x0, y0) // bottom face (reversed)
        .edge(x0, y0, x0, y1) // left face   (reversed)
        .edge(x0, y1, x1, y1) // top face    (reversed)
        .edge(x1, y1, x1, y0) // right face  (reversed)
        .build()
}

//------------- SECTOR BUILDER ---------------

pub struct SectorBuilder<'a> {
    vertex_pool: &'a mut Vec<Vec2>,
    walls: Vec<LineDef>,
    obstacles: Vec<Obstacle>,
    floor_height: f32,
    ceiling_height: f32,
    id: usize,
    floor_texture: Handle<Image>,
    ceiling_texture: Handle<Image>,
}

impl<'a> SectorBuilder<'a> {
    pub fn new(
        vertex_pool: &'a mut Vec<Vec2>,
        id: usize,
        floor_height: f32,
        ceiling_height: f32,
        floor_texture: Handle<Image>,
        ceiling_texture: Handle<Image>
    ) -> Self {
        SectorBuilder {
            vertex_pool,
            walls: Vec::new(),
            obstacles: Vec::new(),
            floor_height,
            ceiling_height,
            id,
            floor_texture,
            ceiling_texture,
        }
    }

    pub fn wall(
        mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        wall_index: usize,
        texture: Handle<Image>
    ) -> Self {
        let wall_id = WallId::new(self.id, wall_index);
        let w = wall(self.vertex_pool, x0, y0, x1, y1, texture, wall_id);
        self.walls.push(w);
        self
    }

    pub fn portal(
        mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        wall_index: usize,
        upper_texture: Handle<Image>,
        lower_texture: Handle<Image>,
        front_sector: usize,
        back_sector: usize
    ) -> Self {
        let wall_id = WallId::new(self.id, wall_index);
        let p = portal(
            self.vertex_pool,
            x0,
            y0,
            x1,
            y1,
            upper_texture,
            lower_texture,
            wall_id,
            front_sector,
            back_sector
        );
        self.walls.push(p);
        self
    }

    pub fn obstacle(mut self, obstacle: Obstacle) -> Self {
        self.obstacles.push(obstacle);
        self
    }

    /// Convenience builder for a rectangular box obstacle with separate
    /// textures for sides, top cap, and bottom cap.
    pub fn rect_obstacle(
        self,
        id: usize,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        bottom: f32,
        top: f32,
        side_texture: Handle<Image>,
        top_texture: Handle<Image>,
        bottom_texture: Handle<Image>
    ) -> Self {
        let obs = rect_obstacle(
            self.vertex_pool,
            id,
            self.id,
            x0,
            y0,
            x1,
            y1,
            bottom,
            top,
            side_texture,
            top_texture,
            bottom_texture
        );
        self.obstacle(obs)
    }

    pub fn build(self) -> Sector {
        Sector {
            walls: self.walls,
            obstacles: self.obstacles,
            floor_height: self.floor_height,
            ceiling_height: self.ceiling_height,
            floor_texture: self.floor_texture,
            ceiling_texture: self.ceiling_texture,
            id: self.id,
        }
    }
}

#[allow(dead_code)]
pub fn rect_sector(
    vertex_pool: &mut Vec<Vec2>,
    id: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    floor_height: f32,
    ceiling_height: f32,
    floor_texture: Handle<Image>,
    wall_texture: Handle<Image>,
    ceiling_texture: Handle<Image>
) -> Sector {
    SectorBuilder::new(vertex_pool, id, floor_height, ceiling_height, floor_texture, ceiling_texture)
        .wall(x0, y0, x1, y0, 0, wall_texture.clone())
        .wall(x1, y0, x1, y1, 1, wall_texture.clone())
        .wall(x1, y1, x0, y1, 2, wall_texture.clone())
        .wall(x0, y1, x0, y0, 3, wall_texture.clone())
        .build()
}

//---------------HELPER FUNCTIONS----------------------

pub fn point_in_sector(point: Vec2, sector: &Sector, vertices: &[Vec2]) -> bool {
    let mut inside = false;
    for wall in &sector.walls {
        let start = wall.start(vertices);
        let end = wall.end(vertices);
        let (x1, y1) = (start.x, start.y);
        let (x2, y2) = (end.x, end.y);
        let crosses = (y1 > point.y) != (y2 > point.y);
        if crosses {
            let x_intersect = x1 + ((point.y - y1) / (y2 - y1)) * (x2 - x1);
            if point.x < x_intersect {
                inside = !inside;
            }
        }
    }
    inside
}

pub fn find_player_sector(player_pos: Vec2, map: &Map) -> Option<usize> {
    for (i, sector) in map.sectors.iter().enumerate() {
        if point_in_sector(player_pos, sector, &map.vertices) {
            return Some(i);
        }
    }
    None
}

//-------------- MAP DATA ------------------------

pub fn test_map(asset_server: Res<AssetServer>) -> Map {
    // Floor/ceiling/obstacle faces use UVs scaled to world units (0.1 per
    // metre), so their textures must tile. The image loader defaults to
    // ClampToEdge, which would only paint the first tile and show the
    // texture's border colour everywhere else.
    let repeat = |s: &mut ImageLoaderSettings| {
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..default()
        });
    };
    let wall_tex: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("texture.png");
    let floor_tex: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("floor_texture.png");
    let ceil_tex: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("floor_texture.png");
    let obstacle_top_tex: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("floor_texture.png");
    let obstacle_bottom_tex: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("floor_texture.png");

    let mut vertices = Vec::new();

    let sectors = vec![
        SectorBuilder::new(&mut vertices, 0, 0.0, 20.0, floor_tex.clone(), ceil_tex.clone())
                .wall(0.0, 0.0, 100.0, 0.0, 0, wall_tex.clone())
                .wall(100.0, 0.0, 100.0, 40.0, 1, wall_tex.clone())
                .portal(100.0, 40.0, 100.0, 60.0, 2, wall_tex.clone(), wall_tex.clone(), 0, 1)
                .wall(100.0, 60.0, 100.0, 100.0, 3, wall_tex.clone())
                .wall(100.0, 100.0, 0.0, 100.0, 4, wall_tex.clone())
                .wall(0.0, 100.0, 0.0, 0.0, 5, wall_tex.clone())
                // Box sitting on the floor — sides, top cap, bottom cap
                .rect_obstacle(
                    0,
                    40.0,
                    40.0,
                    50.0,
                    50.0,
                    0.0,
                    8.0,
                    wall_tex.clone(),
                    obstacle_top_tex.clone(),
                    obstacle_bottom_tex.clone()
                )
                // Floating platform — sides, top cap, bottom cap
                .rect_obstacle(
                    1,
                    60.0,
                    70.0,
                    80.0,
                    90.0,
                    5.0,
                    7.0,
                    wall_tex.clone(),
                    obstacle_top_tex.clone(),
                    obstacle_bottom_tex.clone()
                )
                .build(),

            SectorBuilder::new(&mut vertices, 1, 10.0, 20.0, floor_tex.clone(), ceil_tex.clone())
                .wall(100.0, 40.0, 140.0, 40.0, 0, wall_tex.clone())
                .wall(140.0, 40.0, 140.0, 60.0, 1, wall_tex.clone())
                .wall(140.0, 60.0, 100.0, 60.0, 2, wall_tex.clone())
                .portal(100.0, 60.0, 100.0, 40.0, 3, wall_tex.clone(), wall_tex.clone(), 1, 0)
                .build()
    ];

    Map { vertices, sectors }
}

fn setup_map(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(test_map(asset_server));
}
