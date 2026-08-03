use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;
use crate::map::{Map, Sector};
use crate::mode::{EditorMode, ModeState, VisibleIn3D};

#[derive(Component)]
struct MapPreviewMesh;

pub struct MapPreviewPlugin;

impl Plugin for MapPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_3d_preview);
    }
}

/// Regenerate 3D preview meshes whenever the map or mode changes.
/// Strategy A (full regeneration): despawn everything and respawn.
fn update_3d_preview(
    mut commands: Commands,
    map: Res<Map>,
    mode: Res<ModeState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<MapPreviewMesh>>,
    mut material_cache: Local<HashMap<Handle<Image>, Handle<StandardMaterial>>>,
) {
    let visible = mode.mode == EditorMode::View3D;

    if !map.is_changed() && !mode.is_changed() {
        return;
    }

    // Clear old previews
    for e in &existing {
        commands.entity(e).despawn();
    }

    if !visible {
        return;
    }

    for sector in &map.sectors {
        for (image, mesh_data) in build_sector_meshes(sector, &map) {
            if mesh_data.positions.is_empty() {
                continue;
            }
            let mat = match material_cache.get(&image) {
                Some(h) => h.clone(),
                None => {
                    let h = materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: Some(image.clone()),
                        perceptual_roughness: 0.9,
                        cull_mode: None, // double-sided so walls are visible from both sides
                        ..default()
                    });
                    material_cache.insert(image, h.clone());
                    h
                }
            };
            commands.spawn((
                MapPreviewMesh,
                VisibleIn3D,
                // Previews are not interactive; without this they'd be pickable by
                // default and could race with the despawn/respawn cycle below.
                Pickable::IGNORE,
                Mesh3d(meshes.add(mesh_data.into_mesh())),
                MeshMaterial3d(mat),
            ));
        }
    }
}

// ── Mesh building ────────────────────────────────────────────────

#[derive(Default)]
struct MeshData {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshData {
    /// Fan-triangulate a convex polygon at height `y`. `normal` drives lighting.
    fn add_polygon(&mut self, points: &[Vec2], y: f32, normal: Vec3) {
        if points.len() < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        for p in points {
            self.positions.push([p.x, y, p.y]);
            self.normals.push(normal.to_array());
            self.uvs.push([p.x * 0.1, p.y * 0.1]);
        }
        for i in 1..points.len() - 1 {
            self.indices.push(base);
            self.indices.push(base + i as u32);
            self.indices.push(base + (i + 1) as u32);
        }
    }

    /// Quad from (a, y0) to (b, y1), front face toward `normal`.
    fn add_wall_quad(&mut self, a: Vec2, b: Vec2, y0: f32, y1: f32, normal: Vec3) {
        let base = self.positions.len() as u32;
        self.positions.push([a.x, y0, a.y]);
        self.positions.push([b.x, y0, b.y]);
        self.positions.push([b.x, y1, b.y]);
        self.positions.push([a.x, y1, a.y]);
        for _ in 0..4 {
            self.normals.push(normal.to_array());
        }
        self.uvs.push([0.0, 0.0]);
        self.uvs.push([1.0, 0.0]);
        self.uvs.push([1.0, 1.0]);
        self.uvs.push([0.0, 1.0]);
        self.indices.push(base);
        self.indices.push(base + 1);
        self.indices.push(base + 2);
        self.indices.push(base);
        self.indices.push(base + 2);
        self.indices.push(base + 3);
    }

    /// Convert accumulated data into a triangle-list mesh.
    fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

/// Get-or-insert the mesh bucket for a texture.
fn bucket<'a>(
    buckets: &'a mut HashMap<Handle<Image>, MeshData>,
    tex: &Handle<Image>,
) -> &'a mut MeshData {
    buckets.entry(tex.clone()).or_default()
}

/// Interior-facing normal for a wall from `a` to `b` on the XZ plane.
/// Sectors are wound counter-clockwise, so the interior is to the left
/// of the traversal direction.
fn interior_normal(a: Vec2, b: Vec2) -> Vec3 {
    let (dx, dz) = (b.x - a.x, b.y - a.y);
    Vec3::new(-dz, 0.0, dx).normalize_or_zero()
}

/// Build one mesh per texture used by a sector, so each surface can use the
/// image assigned to it in the map data (floor, ceiling, walls, obstacles).
fn build_sector_meshes(sector: &Sector, map: &Map) -> HashMap<Handle<Image>, MeshData> {
    let mut buckets: HashMap<Handle<Image>, MeshData> = HashMap::new();

    // Sector outline (walls in order describe a convex polygon)
    let outline: Vec<Vec2> = sector.walls.iter().map(|w| *w.start(&map.vertices)).collect();

    // Floor (visible from above) and ceiling (visible from below). The outline
    // is wound counter-clockwise seen from above, which makes the floor's front
    // face point up; reversing it for the ceiling makes the front face point
    // down so its texture reads correctly (not mirrored) from inside the room.
    // The ceiling still gets an up-facing normal so it is lit by the top light.
    bucket(&mut buckets, &sector.floor_texture)
        .add_polygon(&outline, sector.floor_height, Vec3::Y);
    let ceiling_outline: Vec<Vec2> = outline.iter().rev().copied().collect();
    bucket(&mut buckets, &sector.ceiling_texture)
        .add_polygon(&ceiling_outline, sector.ceiling_height, Vec3::Y);

    // Wall quads. Portals are only built by the owner sector (the one with the
    // lower id) and only as the floor/ceiling step regions, so the shared wall
    // is rendered exactly once and the doorway stays open.
    for wall in &sector.walls {
        let a = *wall.start(&map.vertices);
        let b = *wall.end(&map.vertices);

        if let Some(back) = &wall.back_side_def {
            if sector.id >= back.facing {
                continue;
            }
            let Some(back_sector) = map.sectors.iter().find(|s| s.id == back.facing) else {
                continue;
            };

            let floor_lo = sector.floor_height.min(back_sector.floor_height);
            let floor_hi = sector.floor_height.max(back_sector.floor_height);
            if floor_hi - floor_lo > 0.001
                && let Some(tex) = wall.front_side_def.textures.lower.as_ref()
            {
                bucket(&mut buckets, tex)
                    .add_wall_quad(a, b, floor_lo, floor_hi, interior_normal(a, b));
            }

            let ceil_lo = sector.ceiling_height.min(back_sector.ceiling_height);
            let ceil_hi = sector.ceiling_height.max(back_sector.ceiling_height);
            if ceil_hi - ceil_lo > 0.001
                && let Some(tex) = wall.front_side_def.textures.upper.as_ref()
            {
                bucket(&mut buckets, tex)
                    .add_wall_quad(a, b, ceil_lo, ceil_hi, interior_normal(a, b));
            }
        } else if let Some(tex) = wall.front_side_def.textures.middle.as_ref() {
            bucket(&mut buckets, tex).add_wall_quad(
                a,
                b,
                sector.floor_height,
                sector.ceiling_height,
                interior_normal(a, b),
            );
        }
    }

    // Obstacles: prisms between bottom..top
    for obs in &sector.obstacles {
        let pts: Vec<Vec2> = obs.edges.iter().map(|e| *e.start(&map.vertices)).collect();
        bucket(&mut buckets, &obs.top_texture).add_polygon(&pts, obs.top, Vec3::Y);
        bucket(&mut buckets, &obs.bottom_texture).add_polygon(&pts, obs.bottom, Vec3::NEG_Y);
        for edge in &obs.edges {
            let a = *edge.start(&map.vertices);
            let b = *edge.end(&map.vertices);
            bucket(&mut buckets, &obs.side_texture)
                .add_wall_quad(a, b, obs.bottom, obs.top, interior_normal(a, b));
        }
    }

    buckets
}
