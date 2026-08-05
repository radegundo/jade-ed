//! Indexed-vertex map data model.
//!
//! The editor currently consumes only the geometry (vertices + wall indices).
//! Texture/side-def/height fields and the query helpers are part of the full
//! data model for the raycaster renderer and future editor tools, so they are
//! kept intact but may be unused for now.
#![allow(dead_code)]

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;

// Default heights used by every structure created through the 2D tools.
// Height editing is deferred; everything is created at these values for now.
pub const DEFAULT_FLOOR_HEIGHT: f32 = 0.0;
pub const DEFAULT_CEILING_HEIGHT: f32 = 20.0;
pub const DEFAULT_OBSTACLE_BOTTOM: f32 = 0.0;
pub const DEFAULT_OBSTACLE_TOP: f32 = 8.0;

//------------------------------MAP PLUGIN-------------------------

pub struct MapPlugin;
impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_map_assets, setup_map).chain());
    }
}

/// Shared texture handles for geometry created at runtime. Loaded once at
/// startup with a Repeat sampler so tiled UVs work, then reused by both the
/// test map and the 2D editing tools.
#[derive(Resource, Clone)]
pub struct MapAssets {
    pub wall: Handle<Image>,
    pub floor: Handle<Image>,
    pub ceiling: Handle<Image>,
    pub obstacle_side: Handle<Image>,
    pub obstacle_top: Handle<Image>,
    pub obstacle_bottom: Handle<Image>,
}

fn setup_map_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let repeat = |s: &mut ImageLoaderSettings| {
        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..default()
        });
    };
    let wall: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("texture.png");
    let floor: Handle<Image> = asset_server.load_builder().with_settings(repeat).load("floor_texture.png");
    commands.insert_resource(MapAssets {
        wall: wall.clone(),
        floor: floor.clone(),
        ceiling: floor.clone(),
        obstacle_side: wall.clone(),
        obstacle_top: floor.clone(),
        obstacle_bottom: floor.clone(),
    });
}

//------------------------------MAP DATA STRUCTURES-----------------

#[derive(Resource, Default, Clone)]
pub struct Map {
    pub vertices: Vec<Vec2>,
    pub sectors: Vec<Sector>,
}

//------------------------------RUNTIME EDITING------------------------

impl Map {
    /// Create a sector from a closed polygon. Points are normalized to
    /// counter-clockwise winding. Any edge that coincides with an existing
    /// wall — exactly or by partially overlapping it — becomes a portal
    /// between the two sectors; overlapped walls are split at the shared
    /// boundaries so both sectors always share exact vertices.
    pub fn add_sector_from_polygon(
        &mut self,
        points: &[Vec2],
        assets: &MapAssets,
    ) -> Result<usize, String> {
        if points.len() < 3 {
            return Err("A sector needs at least 3 vertices".to_string());
        }
        let ccw = ensure_ccw(points);
        if signed_area(&ccw).abs() < 1e-3 {
            return Err("Sector has zero area".to_string());
        }
        let id = self.next_sector_id();

        // Plan every edge's overlaps up front so a bad polygon never mutates
        // the map. Overlapping an existing portal is a 3-way junction and is
        // rejected before anything changes.
        let n = ccw.len();
        let mut overlaps: Vec<EdgeOverlap> = Vec::new();
        for i in 0..n {
            let a = ccw[i];
            let b = ccw[(i + 1) % n];
            overlaps.extend(self.collect_edge_overlaps(a, b)?);
        }

        // Split every overlapped wall at the shared boundaries so each portal
        // piece becomes its own exact-matching sub-wall.
        self.split_overlapped_walls(&overlaps);

        // Build the new sector's walls. Each edge is divided at every overlap
        // boundary that lies on it; pieces that exactly match a (now split)
        // existing wall become portals, everything else stays solid.
        let mut new_walls: Vec<LineDef> = Vec::new();
        let mut portal_pairs: Vec<((usize, usize), usize)> = Vec::new();
        for i in 0..n {
            let a = ccw[i];
            let b = ccw[(i + 1) % n];
            let ab = b - a;
            let ab_len_sq = ab.length_squared();
            if ab_len_sq < 1e-12 {
                return Err("Sector has a zero-length edge".to_string());
            }

            let mut ts: Vec<f32> = Vec::new();
            for ov in &overlaps {
                for p in [ov.start, ov.end] {
                    if (p - a).perp_dot(ab).abs() > 1e-3 {
                        continue;
                    }
                    let t = (p - a).dot(ab) / ab_len_sq;
                    if t > 1e-4 && t < 1.0 - 1e-4 {
                        ts.push(t);
                    }
                }
            }
            ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-4);

            let mut params = vec![0.0];
            params.extend(ts);
            params.push(1.0);
            for w in params.windows(2) {
                let p = a + ab * w[0];
                let q = a + ab * w[1];
                if (q - p).length() < 1e-4 {
                    continue;
                }
                let wall_id = WallId::new(id, new_walls.len());
                let start_idx = add_vertex(&mut self.vertices, p);
                let end_idx = add_vertex(&mut self.vertices, q);
                if let Some((other_sector, other_wall)) = self.find_wall_at_edge(p, q) {
                    let existing_id = self.sectors[other_sector].id;
                    portal_pairs.push(((other_sector, other_wall), new_walls.len()));
                    new_walls.push(portal_wall(
                        start_idx,
                        end_idx,
                        id,
                        existing_id,
                        &assets.wall,
                        wall_id,
                    ));
                } else {
                    new_walls.push(solid_wall(start_idx, end_idx, id, &assets.wall, wall_id));
                }
            }
        }

        self.sectors.push(Sector {
            walls: new_walls,
            obstacles: Vec::new(),
            floor_height: DEFAULT_FLOOR_HEIGHT,
            ceiling_height: DEFAULT_CEILING_HEIGHT,
            floor_texture: assets.floor.clone(),
            ceiling_texture: assets.ceiling.clone(),
            id,
        });

        for ((other_sector, other_wall), _) in portal_pairs {
            let existing_id = self.sectors[other_sector].id;
            to_portal_wall(
                &mut self.sectors[other_sector].walls[other_wall],
                existing_id,
                id,
                &assets.wall,
            );
        }

        Ok(id)
    }

    /// Add a single wall to an existing sector. If the wall coincides with a
    /// wall of another sector — exactly or by partially overlapping it — both
    /// become a portal and the overlapped wall is split at the shared boundary.
    pub fn add_wall(
        &mut self,
        sector_id: usize,
        start: Vec2,
        end: Vec2,
        assets: &MapAssets,
    ) -> Result<(), String> {
        let Some(sector_index) = self.sectors.iter().position(|s| s.id == sector_id) else {
            return Err("Target sector not found".to_string());
        };
        if start.distance(end) < 1e-3 {
            return Err("Wall is too short".to_string());
        }

        // Orient the wall so the sector interior lies to its left (CCW winding).
        let mut a = start;
        let mut b = end;
        if let Some(centroid) = self.sector_centroid(sector_index) {
            if (b - a).perp_dot(centroid - a) < 0.0 {
                std::mem::swap(&mut a, &mut b);
            }
        }

        let overlaps = self.collect_edge_overlaps(a, b)?;
        if overlaps.iter().any(|ov| ov.sector == sector_index) {
            return Err("Wall already exists in this sector".to_string());
        }

        // Split overlapped walls so the portal pieces share exact vertices.
        self.split_overlapped_walls(&overlaps);

        // Build the (possibly split) wall pieces.
        let ab = b - a;
        let ab_len_sq = ab.length_squared();
        let mut ts: Vec<f32> = Vec::new();
        for ov in &overlaps {
            for p in [ov.start, ov.end] {
                let t = (p - a).dot(ab) / ab_len_sq;
                if t > 1e-4 && t < 1.0 - 1e-4 {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-4);

        let mut params = vec![0.0];
        params.extend(ts);
        params.push(1.0);
        let mut portal_pairs: Vec<(usize, usize)> = Vec::new();
        for w in params.windows(2) {
            let p = a + ab * w[0];
            let q = a + ab * w[1];
            if (q - p).length() < 1e-4 {
                continue;
            }
            let wall_index = self.sectors[sector_index].walls.len();
            let wall_id = WallId::new(sector_id, wall_index);
            if let Some((other_sector, other_wall)) = self.find_wall_at_edge(p, q) {
                let existing_id = self.sectors[other_sector].id;
                portal_pairs.push((other_sector, other_wall));
                self.sectors[sector_index].walls.push(portal_wall(
                    add_vertex(&mut self.vertices, p),
                    add_vertex(&mut self.vertices, q),
                    sector_id,
                    existing_id,
                    &assets.wall,
                    wall_id,
                ));
            } else {
                self.sectors[sector_index].walls.push(solid_wall(
                    add_vertex(&mut self.vertices, p),
                    add_vertex(&mut self.vertices, q),
                    sector_id,
                    &assets.wall,
                    wall_id,
                ));
            }
        }

        for (other_sector, other_wall) in portal_pairs {
            let existing_id = self.sectors[other_sector].id;
            to_portal_wall(
                &mut self.sectors[other_sector].walls[other_wall],
                existing_id,
                sector_id,
                &assets.wall,
            );
        }

        Ok(())
    }

    /// Place a rectangular obstacle inside a sector with default heights.
    /// Returns the obstacle's id within that sector.
    pub fn add_obstacle(
        &mut self,
        sector_id: usize,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        assets: &MapAssets,
    ) -> Result<usize, String> {
        let Some(sector_index) = self.sectors.iter().position(|s| s.id == sector_id) else {
            return Err("Target sector not found".to_string());
        };
        let (minx, maxx) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        let (miny, maxy) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        if maxx - minx < 1e-3 || maxy - miny < 1e-3 {
            return Err("Obstacle has zero area".to_string());
        }
        let obs_id = self.sectors[sector_index]
            .obstacles
            .iter()
            .map(|o| o.id)
            .max()
            .map_or(0, |m| m + 1);
        let obstacle = rect_obstacle(
            &mut self.vertices,
            obs_id,
            sector_id,
            minx,
            miny,
            maxx,
            maxy,
            DEFAULT_OBSTACLE_BOTTOM,
            DEFAULT_OBSTACLE_TOP,
            assets.obstacle_side.clone(),
            assets.obstacle_top.clone(),
            assets.obstacle_bottom.clone(),
        );
        self.sectors[sector_index].obstacles.push(obstacle);
        Ok(obs_id)
    }

    /// Delete a sector and every wall/obstacle inside it. Portals shared with
    /// surviving sectors are stripped so their walls become solid again.
    pub fn remove_sector(&mut self, id: usize) {
        let Some(sector_index) = self.sectors.iter().position(|s| s.id == id) else {
            return;
        };
        for (si, sector) in self.sectors.iter_mut().enumerate() {
            if si == sector_index {
                continue;
            }
            for wall in &mut sector.walls {
                if let Some(back) = &wall.back_side_def
                    && back.facing == id
                {
                    to_solid_wall(wall);
                }
            }
        }
        self.sectors.remove(sector_index);
        self.rebuild_vertices();
    }

    /// Delete an obstacle (by id within its sector).
    pub fn remove_obstacle(&mut self, sector_id: usize, obstacle_id: usize) {
        let Some(sector_index) = self.sectors.iter().position(|s| s.id == sector_id) else {
            return;
        };
        let Some(obs_index) = self
            .sectors[sector_index]
            .obstacles
            .iter()
            .position(|o| o.id == obstacle_id)
        else {
            return;
        };
        self.sectors[sector_index].obstacles.remove(obs_index);
        self.rebuild_vertices();
    }

    /// Delete a vertex and every wall/obstacle edge that uses it. Sectors left
    /// with fewer than 3 walls (and obstacles with fewer than 3 edges) are
    /// dropped along the way.
    pub fn remove_vertex(&mut self, vertex_idx: usize) {
        if vertex_idx >= self.vertices.len() {
            return;
        }
        for sector in self.sectors.iter_mut() {
            sector
                .walls
                .retain(|w| w.start_idx != vertex_idx && w.end_idx != vertex_idx);
            for obs in &mut sector.obstacles {
                obs.edges
                    .retain(|e| e.start_idx != vertex_idx && e.end_idx != vertex_idx);
            }
            sector.obstacles.retain(|o| o.edges.len() >= 3);
        }
        self.sectors.retain(|s| s.walls.len() >= 3);
        self.rebuild_vertices();
    }

    /// Re-dedup the vertex pool from all remaining walls/obstacle edges and
    /// remap every index. Drops now-unused vertices; shared vertices collapse.
    pub fn rebuild_vertices(&mut self) {
        let old_vertices = std::mem::take(&mut self.vertices);
        let mut new_pool: Vec<Vec2> = Vec::new();
        for sector in self.sectors.iter_mut() {
            for wall in sector.walls.iter_mut() {
                let s = *wall.start(&old_vertices);
                let e = *wall.end(&old_vertices);
                wall.start_idx = add_vertex(&mut new_pool, s);
                wall.end_idx = add_vertex(&mut new_pool, e);
            }
            for obs in sector.obstacles.iter_mut() {
                for edge in obs.edges.iter_mut() {
                    let s = *edge.start(&old_vertices);
                    let e = *edge.end(&old_vertices);
                    edge.start_idx = add_vertex(&mut new_pool, s);
                    edge.end_idx = add_vertex(&mut new_pool, e);
                }
            }
        }
        self.vertices = new_pool;
    }

    /// Innermost sector whose polygon contains `pos` (last matching in draw
    /// order), used for obstacle placement and sector selection.
    pub fn find_sector_at(&self, pos: Vec2) -> Option<usize> {
        let mut found = None;
        for (i, sector) in self.sectors.iter().enumerate() {
            if point_in_sector(pos, sector, &self.vertices) {
                found = Some(i);
            }
        }
        found
    }

    /// Target-sector resolution for the Draw Wall tool: the sector containing
    /// `pos`, falling back to the sector whose boundary is within `tolerance`
    /// of `pos` (so clicking on a shared edge still resolves a sector). Portal
    /// edges are ambiguous, so the closest interior wins.
    pub fn find_sector_containing_or_on_edge(&self, pos: Vec2, tolerance: f32) -> Option<usize> {
        if let Some(idx) = self.find_sector_at(pos) {
            return Some(idx);
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, sector) in self.sectors.iter().enumerate() {
            let near = sector.walls.iter().any(|w| {
                let s = *w.start(&self.vertices);
                let e = *w.end(&self.vertices);
                s.distance(pos) <= tolerance || e.distance(pos) <= tolerance
            });
            if !near {
                continue;
            }
            let d = self
                .sector_centroid(i)
                .map(|c| c.distance(pos))
                .unwrap_or(f32::INFINITY);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    /// Exact vertex match: the edge (a,b) or (b,a) already exists as a wall.
    fn find_wall_at_edge(&self, a: Vec2, b: Vec2) -> Option<(usize, usize)> {
        for (si, sector) in self.sectors.iter().enumerate() {
            for (wi, wall) in sector.walls.iter().enumerate() {
                let s = *wall.start(&self.vertices);
                let e = *wall.end(&self.vertices);
                if (s == a && e == b) || (s == b && e == a) {
                    return Some((si, wi));
                }
            }
        }
        None
    }

    /// All existing solid walls that `(a, b)` overlaps — exactly or partially —
    /// with the overlap interval ordered along `(a, b)`. Overlapping an
    /// existing portal is a 3-way junction and is rejected.
    fn collect_edge_overlaps(&self, a: Vec2, b: Vec2) -> Result<Vec<EdgeOverlap>, String> {
        let mut out = Vec::new();
        for (si, sector) in self.sectors.iter().enumerate() {
            for (wi, wall) in sector.walls.iter().enumerate() {
                let c = *wall.start(&self.vertices);
                let d = *wall.end(&self.vertices);
                let Some((s, e)) = collinear_overlap(a, b, c, d) else {
                    continue;
                };
                if wall.back_side_def.is_some() {
                    return Err(
                        "An edge overlaps an existing portal (3-way junction is not supported)"
                            .to_string(),
                    );
                }
                out.push(EdgeOverlap { start: s, end: e, sector: si, wall: wi });
            }
        }
        Ok(out)
    }

    /// Split every solid wall touched by an overlap at the overlap interval
    /// boundaries, so each portal piece becomes its own exact-matching
    /// sub-wall. Cut positions are reused verbatim from the overlaps so both
    /// sectors resolve to the same pooled vertices.
    fn split_overlapped_walls(&mut self, overlaps: &[EdgeOverlap]) {
        let mut cuts: Vec<(usize, usize, Vec<(f32, Vec2)>)> = Vec::new();
        for ov in overlaps {
            let wall = &self.sectors[ov.sector].walls[ov.wall];
            let c = *wall.start(&self.vertices);
            let d = *wall.end(&self.vertices);
            let cd = d - c;
            let len_sq = cd.length_squared();
            if len_sq < 1e-12 {
                continue;
            }
            for p in [ov.start, ov.end] {
                let t = (p - c).dot(cd) / len_sq;
                if t > 1e-4 && t < 1.0 - 1e-4 {
                    match cuts.iter_mut().find(|(s, w, _)| *s == ov.sector && *w == ov.wall) {
                        Some((_, _, ts)) => ts.push((t, p)),
                        None => cuts.push((ov.sector, ov.wall, vec![(t, p)])),
                    }
                }
            }
        }
        if cuts.is_empty() {
            return;
        }
        for (_, _, ts) in &mut cuts {
            ts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
            ts.dedup_by(|x, y| (x.0 - y.0).abs() < 1e-4);
        }

        // Rebuild each affected sector's wall list so later wall indices don't
        // shift under earlier splits in the same sector.
        for (si, wi, ts) in cuts {
            let old_walls = std::mem::take(&mut self.sectors[si].walls);
            let mut new_walls: Vec<LineDef> = Vec::new();
            for (i, wall) in old_walls.into_iter().enumerate() {
                if i != wi {
                    new_walls.push(wall);
                    continue;
                }
                let c = *wall.start(&self.vertices);
                let d = *wall.end(&self.vertices);
                let tex = wall.front_side_def.textures.middle.clone().unwrap_or_default();
                let mut params = vec![(0.0, c)];
                params.extend(ts.iter().copied());
                params.push((1.0, d));
                for w in params.windows(2) {
                    let (_, p) = w[0];
                    let (_, q) = w[1];
                    if (q - p).length() < 1e-4 {
                        continue;
                    }
                    let wall_id = WallId::new(self.sectors[si].id, new_walls.len());
                    new_walls.push(solid_wall(
                        add_vertex(&mut self.vertices, p),
                        add_vertex(&mut self.vertices, q),
                        wall.front_side_def.facing,
                        &tex,
                        wall_id,
                    ));
                }
            }
            self.sectors[si].walls = new_walls;
        }
    }

    pub fn sector_centroid(&self, sector_index: usize) -> Option<Vec2> {
        let sector = &self.sectors[sector_index];
        let pts: Vec<Vec2> = sector
            .walls
            .iter()
            .map(|w| *w.start(&self.vertices))
            .collect();
        if pts.is_empty() {
            return None;
        }
        let mut sum = Vec2::ZERO;
        for p in &pts {
            sum += *p;
        }
        Some(sum / pts.len() as f32)
    }

    fn next_sector_id(&self) -> usize {
        self.sectors
            .iter()
            .map(|s| s.id)
            .max()
            .map_or(0, |m| m + 1)
    }
}

/// Normalize polygon winding to counter-clockwise (interior to the left of the
/// traversal, which is what the sector/portal code assumes).
pub fn ensure_ccw(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    if signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    pts
}

fn signed_area(points: &[Vec2]) -> f32 {
    let mut sum = 0.0;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        sum += a.x * b.y - b.x * a.y;
    }
    sum * 0.5
}

/// Snap `pos` to the nearest vertex within `radius` (used by the draw tools so
/// shared corners/edges line up exactly and portals can form).
pub fn snap_to_vertex(vertices: &[Vec2], pos: Vec2, radius: f32) -> Option<Vec2> {
    vertices
        .iter()
        .copied()
        .filter(|v| v.distance(pos) <= radius)
        .min_by(|a, b| a.distance(pos).partial_cmp(&b.distance(pos)).unwrap())
}

/// Intersection of segments (a,b) and (c,d) when they are collinear, returned
/// ordered along (a,b). `None` if they don't overlap in a positive length.
fn collinear_overlap(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(Vec2, Vec2)> {
    let ab = b - a;
    let ab_len_sq = ab.length_squared();
    if ab_len_sq < 1e-12 {
        return None;
    }
    let cd = d - c;
    if ab.perp_dot(cd).abs() > 1e-3 {
        return None;
    }
    if ab.perp_dot(c - a).abs() > 1e-3 {
        return None;
    }
    let t1 = (c - a).dot(ab) / ab_len_sq;
    let t2 = (d - a).dot(ab) / ab_len_sq;
    let lo = t1.min(t2).max(0.0);
    let hi = t1.max(t2).min(1.0);
    if hi - lo < 1e-4 {
        return None;
    }
    Some((a + ab * lo, a + ab * hi))
}

/// A collinear overlap between a newly drawn edge and one existing solid wall.
#[derive(Clone, Copy, Debug)]
struct EdgeOverlap {
    /// Overlap interval endpoints, ordered along the new edge's direction.
    start: Vec2,
    end: Vec2,
    /// Index of the existing wall's sector.
    sector: usize,
    /// Index of the existing wall within that sector.
    wall: usize,
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

impl Sector {
    /// Set the floor height, clamped so it never exceeds the ceiling height.
    pub fn set_floor_height(&mut self, h: f32) {
        self.floor_height = h.min(self.ceiling_height);
    }

    /// Set the ceiling height, clamped so it never drops below the floor height.
    pub fn set_ceiling_height(&mut self, h: f32) {
        self.ceiling_height = h.max(self.floor_height);
    }
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

impl Obstacle {
    /// Set the bottom height, clamped so it never exceeds the top height.
    pub fn set_bottom(&mut self, h: f32) {
        self.bottom = h.min(self.top);
    }

    /// Set the top height, clamped so it never drops below the bottom height.
    pub fn set_top(&mut self, h: f32) {
        self.top = h.max(self.bottom);
    }
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

//------------- RUNTIME WALL / PORTAL CONSTRUCTORS ----------------

/// A plain one-sided wall with the middle texture slot set.
fn solid_wall(
    start_idx: usize,
    end_idx: usize,
    sector: usize,
    texture: &Handle<Image>,
    id: WallId,
) -> LineDef {
    LineDef {
        start_idx,
        end_idx,
        front_side_def: SideDef::new(
            SideDefTextures { upper: None, middle: Some(texture.clone()), lower: None },
            sector,
        ),
        back_side_def: None,
        id,
    }
}

/// A two-sided portal wall: front faces `sector`, back faces `other_sector`.
fn portal_wall(
    start_idx: usize,
    end_idx: usize,
    sector: usize,
    other_sector: usize,
    texture: &Handle<Image>,
    id: WallId,
) -> LineDef {
    LineDef {
        start_idx,
        end_idx,
        front_side_def: SideDef::new(
            SideDefTextures {
                upper: Some(texture.clone()),
                middle: None,
                lower: Some(texture.clone()),
            },
            sector,
        ),
        back_side_def: Some(SideDef::new(
            SideDefTextures {
                upper: Some(texture.clone()),
                middle: None,
                lower: Some(texture.clone()),
            },
            other_sector,
        )),
        id,
    }
}

/// Convert a solid wall into a portal wall sharing the edge with `other_sector`.
fn to_portal_wall(
    wall: &mut LineDef,
    my_sector: usize,
    other_sector: usize,
    texture: &Handle<Image>,
) {
    wall.front_side_def = SideDef::new(
        SideDefTextures {
            upper: Some(texture.clone()),
            middle: None,
            lower: Some(texture.clone()),
        },
        my_sector,
    );
    wall.back_side_def = Some(SideDef::new(
        SideDefTextures {
            upper: Some(texture.clone()),
            middle: None,
            lower: Some(texture.clone()),
        },
        other_sector,
    ));
}

/// Strip the portal back-side so the wall becomes a plain solid wall again.
/// Reuses whatever texture the side already had.
fn to_solid_wall(wall: &mut LineDef) {
    let tex = wall
        .front_side_def
        .textures
        .upper
        .clone()
        .or_else(|| wall.front_side_def.textures.lower.clone())
        .or_else(|| wall.front_side_def.textures.middle.clone());
    let facing = wall.front_side_def.facing;
    wall.front_side_def = SideDef::new(
        SideDefTextures { upper: None, middle: tex, lower: None },
        facing,
    );
    wall.back_side_def = None;
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

/// Ray-cast point-in-polygon test on the XZ plane.
pub fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let n = polygon.len();
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + 1) % n];
        let (x1, y1) = (a.x, a.y);
        let (x2, y2) = (b.x, b.y);
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

pub fn point_in_sector(point: Vec2, sector: &Sector, vertices: &[Vec2]) -> bool {
    let outline: Vec<Vec2> = sector.walls.iter().map(|w| *w.start(vertices)).collect();
    point_in_polygon(point, &outline)
}

/// Centroid of an obstacle's polygon (average of its edge start points).
pub fn obstacle_center(obs: &Obstacle, vertices: &[Vec2]) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut count = 0;
    for e in &obs.edges {
        sum += *e.start(vertices);
        count += 1;
    }
    if count == 0 {
        Vec2::ZERO
    } else {
        sum / count as f32
    }
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

pub fn test_map(assets: &MapAssets) -> Map {
    let wall_tex = assets.wall.clone();
    let floor_tex = assets.floor.clone();
    let ceil_tex = assets.ceiling.clone();
    let obstacle_top_tex = assets.obstacle_top.clone();
    let obstacle_bottom_tex = assets.obstacle_bottom.clone();

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
                    assets.obstacle_side.clone(),
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
                    assets.obstacle_side.clone(),
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

fn setup_map(mut commands: Commands, assets: Res<MapAssets>) {
    commands.insert_resource(test_map(&assets));
}

//------------------------------TESTS-------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> MapAssets {
        MapAssets {
            wall: Handle::default(),
            floor: Handle::default(),
            ceiling: Handle::default(),
            obstacle_side: Handle::default(),
            obstacle_top: Handle::default(),
            obstacle_bottom: Handle::default(),
        }
    }

    /// Draw a counter-clockwise rectangle sector and return its id.
    fn rect(map: &mut Map, assets: &MapAssets, x0: f32, y0: f32, x1: f32, y1: f32) -> usize {
        let pts = vec![
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ];
        map.add_sector_from_polygon(&pts, assets).unwrap()
    }

    #[test]
    fn sector_from_polygon_builds_defaults() {
        let assets = assets();
        let mut map = Map::default();
        let id = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        assert_eq!(map.sectors.len(), 1);
        assert_eq!(id, 0);
        let s = &map.sectors[0];
        assert_eq!(s.walls.len(), 4);
        assert_eq!(s.floor_height, DEFAULT_FLOOR_HEIGHT);
        assert_eq!(s.ceiling_height, DEFAULT_CEILING_HEIGHT);
        assert_eq!(map.vertices.len(), 4);
        for (i, w) in s.walls.iter().enumerate() {
            assert_eq!(w.id, WallId::new(id, i));
            assert!(w.back_side_def.is_none());
        }
    }

    #[test]
    fn sector_from_polygon_rejects_degenerate() {
        let assets = assets();
        let mut map = Map::default();
        assert!(map
            .add_sector_from_polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)], &assets)
            .is_err());
        // Three collinear points = zero area
        assert!(map
            .add_sector_from_polygon(
                &[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)],
                &assets
            )
            .is_err());
        assert!(map.sectors.is_empty());
    }

    #[test]
    fn sector_from_polygon_normalizes_winding() {
        let assets = assets();
        let mut map = Map::default();
        let id = map
            .add_sector_from_polygon(
                &[
                    Vec2::new(0.0, 0.0),
                    Vec2::new(0.0, 10.0),
                    Vec2::new(10.0, 10.0),
                    Vec2::new(10.0, 0.0),
                ],
                &assets,
            )
            .unwrap();
        let outline: Vec<Vec2> = map.sectors[id]
            .walls
            .iter()
            .map(|w| *w.start(&map.vertices))
            .collect();
        assert!(signed_area(&outline) > 0.0);
    }

    #[test]
    fn shared_edge_dedup_vertices() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        // Two shared vertices (10,0) and (10,10): 8 - 2 = 6 pooled vertices.
        assert_eq!(map.vertices.len(), 6);
    }

    #[test]
    fn adjacent_sectors_auto_portal() {
        let assets = assets();
        let mut map = Map::default();
        let a = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let b = rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);

        let pa = map.sectors[a]
            .walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector A should have a portal");
        assert_eq!(pa.front_side_def.facing, a);
        assert_eq!(pa.back_side_def.as_ref().unwrap().facing, b);

        let pb = map.sectors[b]
            .walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector B should have a portal");
        assert_eq!(pb.front_side_def.facing, b);
        assert_eq!(pb.back_side_def.as_ref().unwrap().facing, a);
    }

    #[test]
    fn portal_shares_vertices_opposite_winding() {
        let assets = assets();
        let mut map = Map::default();
        let a = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let b = rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        let wa = map.sectors[a]
            .walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .unwrap();
        let wb = map.sectors[b]
            .walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .unwrap();
        // The two portal walls traverse the shared edge in opposite directions
        // and reference the exact same pooled vertices.
        assert_eq!((wa.start_idx, wa.end_idx), (wb.end_idx, wb.start_idx));
    }

    #[test]
    fn partial_overlap_splits_wall_into_portal() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        // A small sector to the right whose left edge only overlaps the middle
        // of A's right wall (10,0)-(10,10): it must split that wall and form a
        // portal on the shared sub-segment (10,4)-(10,6).
        let b = map
            .add_sector_from_polygon(
                &[
                    Vec2::new(10.0, 4.0),
                    Vec2::new(20.0, 4.0),
                    Vec2::new(20.0, 6.0),
                    Vec2::new(10.0, 6.0),
                ],
                &assets,
            )
            .unwrap();
        assert_eq!(map.sectors.len(), 2);

        let a_walls = &map.sectors[0].walls;
        assert_eq!(a_walls.len(), 6, "right wall split into three pieces");
        let portal = a_walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector A should have a portal");
        assert_eq!(*portal.start(&map.vertices), Vec2::new(10.0, 4.0));
        assert_eq!(*portal.end(&map.vertices), Vec2::new(10.0, 6.0));
        assert_eq!(portal.back_side_def.as_ref().unwrap().facing, b);

        let b_walls = &map.sectors[1].walls;
        assert_eq!(b_walls.len(), 4);
        let b_portal = b_walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector B should have a portal");
        assert_eq!(b_portal.back_side_def.as_ref().unwrap().facing, 0);
        // Both sides traverse the edge in opposite directions over the same
        // pooled vertices.
        assert_eq!(portal.start_idx, b_portal.end_idx);
        assert_eq!(portal.end_idx, b_portal.start_idx);
    }

    #[test]
    fn edge_longer_than_wall_splits_new_edge() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        // A sector below A whose top edge is longer than A's bottom wall: only
        // the shared part (5,0)-(10,0) becomes a portal; the overhang
        // (10,0)-(15,0) stays solid.
        let b = map
            .add_sector_from_polygon(
                &[
                    Vec2::new(5.0, -5.0),
                    Vec2::new(15.0, -5.0),
                    Vec2::new(15.0, 0.0),
                    Vec2::new(5.0, 0.0),
                ],
                &assets,
            )
            .unwrap();
        assert_eq!(map.sectors.len(), 2);

        let a_walls = &map.sectors[0].walls;
        assert_eq!(a_walls.len(), 5, "bottom wall split at (5,0)");
        let a_portal = a_walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector A should have a portal");
        assert_eq!(*a_portal.start(&map.vertices), Vec2::new(5.0, 0.0));
        assert_eq!(*a_portal.end(&map.vertices), Vec2::new(10.0, 0.0));
        assert_eq!(a_portal.back_side_def.as_ref().unwrap().facing, b);

        let b_walls = &map.sectors[1].walls;
        assert_eq!(b_walls.len(), 5, "top edge split at (10,0)");
        let b_portal = b_walls
            .iter()
            .find(|w| w.back_side_def.is_some())
            .expect("sector B should have a portal");
        assert_eq!(*b_portal.start(&map.vertices), Vec2::new(10.0, 0.0));
        assert_eq!(*b_portal.end(&map.vertices), Vec2::new(5.0, 0.0));
        assert_eq!(b_portal.back_side_def.as_ref().unwrap().facing, 0);
        assert_eq!(b_walls.iter().filter(|w| w.back_side_def.is_none()).count(), 4);
    }

    #[test]
    fn edge_spanning_two_walls_splits_both() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        // A sector below both whose top edge runs across the shared corner: it
        // overlaps A's bottom wall and B's bottom wall, so it splits into two
        // portal pieces (one per wall) and splits both walls.
        let c = map
            .add_sector_from_polygon(
                &[
                    Vec2::new(5.0, -5.0),
                    Vec2::new(15.0, -5.0),
                    Vec2::new(15.0, 0.0),
                    Vec2::new(5.0, 0.0),
                ],
                &assets,
            )
            .unwrap();
        assert_eq!(map.sectors.len(), 3);

        assert_eq!(map.sectors[0].walls.len(), 5);
        assert_eq!(map.sectors[1].walls.len(), 5);

        let c_walls = &map.sectors[c].walls;
        assert_eq!(c_walls.len(), 5);
        assert_eq!(c_walls.iter().filter(|w| w.back_side_def.is_some()).count(), 2);
        let faces: Vec<usize> = c_walls
            .iter()
            .filter(|w| w.back_side_def.is_some())
            .map(|w| w.back_side_def.as_ref().unwrap().facing)
            .collect();
        assert_eq!(faces, vec![1, 0]);
    }

    #[test]
    fn three_way_junction_rejected() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        let before = map.clone();
        // Drawing a duplicate of sector B hits B's already-portal edge.
        let err = map.add_sector_from_polygon(
            &[
                Vec2::new(10.0, 0.0),
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 10.0),
                Vec2::new(10.0, 10.0),
            ],
            &assets,
        );
        assert!(err.is_err());
        assert_eq!(map.sectors.len(), 2);
        assert_eq!(map.sectors[0].walls.len(), before.sectors[0].walls.len());
        assert_eq!(map.sectors[1].walls.len(), before.sectors[1].walls.len());
        // The existing portal is untouched.
        assert!(map.sectors[0].walls.iter().any(|w| w.back_side_def.is_some()));
        assert!(map.sectors[1].walls.iter().any(|w| w.back_side_def.is_some()));
    }

    #[test]
    fn remove_sector_scrubs_neighbor_portal() {
        let assets = assets();
        let mut map = Map::default();
        let a = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let b = rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        map.remove_sector(b);
        assert_eq!(map.sectors.len(), 1);
        assert_eq!(map.sectors[0].id, a);
        let walls = &map.sectors[0].walls;
        assert_eq!(walls.len(), 4);
        assert!(walls.iter().all(|w| w.back_side_def.is_none()));
        // Shared-edge vertices reclaimed: only A's 4 corners remain.
        assert_eq!(map.vertices.len(), 4);
    }

    #[test]
    fn remove_vertex_cleans_walls_and_sectors() {
        let assets = assets();
        let mut map = Map::default();
        rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        let idx = map
            .vertices
            .iter()
            .position(|v| *v == Vec2::new(10.0, 0.0))
            .unwrap();
        map.remove_vertex(idx);
        // Both sectors drop below 3 walls (the corner feeds two walls in each)
        // and are removed along with their now-unused vertices.
        assert!(map.sectors.is_empty());
        assert!(map.vertices.is_empty());
    }

    #[test]
    fn remove_obstacle() {
        let assets = assets();
        let mut map = Map::default();
        let id = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let obs = map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets).unwrap();
        assert_eq!(map.sectors[id].obstacles.len(), 1);
        assert_eq!(map.vertices.len(), 8);
        map.remove_obstacle(id, obs);
        assert!(map.sectors[id].obstacles.is_empty());
        assert_eq!(map.vertices.len(), 4);
    }

    #[test]
    fn point_in_sector_and_find_sector_at() {
        let assets = assets();
        let mut map = Map::default();
        let a = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let b = rect(&mut map, &assets, 10.0, 0.0, 20.0, 10.0);
        assert_eq!(map.find_sector_at(Vec2::new(5.0, 5.0)), Some(a));
        assert_eq!(map.find_sector_at(Vec2::new(15.0, 5.0)), Some(b));
        assert_eq!(map.find_sector_at(Vec2::new(-5.0, 5.0)), None);

        // Nesting: an inner sector is returned (innermost match wins).
        let c = rect(&mut map, &assets, 2.0, 2.0, 4.0, 4.0);
        assert_eq!(map.find_sector_at(Vec2::new(3.0, 3.0)), Some(c));
        assert_eq!(map.find_sector_at(Vec2::new(5.0, 5.0)), Some(a));
    }

    #[test]
    fn snap_to_vertex_helper() {
        let vertices = vec![Vec2::new(0.0, 0.0), Vec2::new(5.0, 0.0), Vec2::new(3.0, 3.0)];
        assert_eq!(
            snap_to_vertex(&vertices, Vec2::new(4.9, 0.1), 1.0),
            Some(Vec2::new(5.0, 0.0))
        );
        assert_eq!(
            snap_to_vertex(&vertices, Vec2::new(3.4, 3.4), 1.0),
            Some(Vec2::new(3.0, 3.0))
        );
        assert_eq!(snap_to_vertex(&vertices, Vec2::new(10.0, 10.0), 1.0), None);
    }

    #[test]
    fn sector_height_setters_clamp() {
        let assets = assets();
        let mut map = Map::default();
        let id = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let s = &mut map.sectors[id];

        s.set_floor_height(5.0);
        assert_eq!(s.floor_height, 5.0);
        assert_eq!(s.ceiling_height, DEFAULT_CEILING_HEIGHT);

        s.set_ceiling_height(2.0);
        assert_eq!(s.ceiling_height, 5.0); // clamped up to the floor

        s.set_floor_height(50.0);
        assert_eq!(s.floor_height, 5.0); // clamped down to the ceiling
        assert_eq!(s.ceiling_height, 5.0);
    }

    #[test]
    fn obstacle_height_setters_clamp() {
        let assets = assets();
        let mut map = Map::default();
        let id = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let obs = map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets).unwrap();

        let o = &mut map.sectors[id].obstacles[obs];
        o.set_bottom(4.0);
        assert_eq!(o.bottom, 4.0);
        o.set_top(2.0);
        assert_eq!(o.top, 4.0); // clamped up to the bottom
        o.set_bottom(10.0);
        assert_eq!(o.bottom, 4.0); // clamped down to the top
    }

    #[test]
    fn point_in_polygon_and_obstacle_center() {
        let square = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        assert!(point_in_polygon(Vec2::new(5.0, 5.0), &square));
        assert!(!point_in_polygon(Vec2::new(-1.0, 5.0), &square));
        assert!(!point_in_polygon(Vec2::new(11.0, 5.0), &square));

        let assets = assets();
        let mut map = Map::default();
        let id = rect(&mut map, &assets, 0.0, 0.0, 10.0, 10.0);
        let obs = map.add_obstacle(id, 2.0, 2.0, 4.0, 4.0, &assets).unwrap();
        let center = obstacle_center(&map.sectors[id].obstacles[obs], &map.vertices);
        assert!((center - Vec2::new(3.0, 3.0)).length() < 1e-3);
    }
}
