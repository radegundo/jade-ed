use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;
use crate::map::{Map, Sector};
use crate::mode::{EditorMode, ModeState, VisibleIn3D};

#[derive(Component)]
struct MapPreviewMesh;

/// Pickable marker on a 3D preview wall quad, for wall selection in View3D.
#[derive(Component)]
pub struct PickableWall {
    pub sector_id: usize,
    pub wall_index: usize,
}

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
        let (buckets, walls) = build_sector_meshes(sector, &map);
        for (image, mesh_data) in buckets {
            if mesh_data.positions.is_empty() {
                continue;
            }
            let mat = material_for_image(&mut materials, &mut material_cache, &image);
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
        // Walls are individually pickable so they can be selected in 3D; each
        // wall (and each texture region of a portal step) is its own entity.
        for wall in walls {
            if wall.mesh.positions.is_empty() {
                continue;
            }
            let mat = material_for_image(&mut materials, &mut material_cache, &wall.image);
            commands.spawn((
                MapPreviewMesh,
                PickableWall { sector_id: wall.sector_id, wall_index: wall.wall_index },
                VisibleIn3D,
                Pickable::default(),
                // Raycasts cull backfaces for 3D meshes by default; walls must be
                // clickable from both sides, so include backfaces for picking only.
                RayCastBackfaces,
                Mesh3d(meshes.add(wall.mesh.into_mesh())),
                MeshMaterial3d(mat),
            ));
        }
    }
}

/// Get-or-add a double-sided StandardMaterial for a texture image.
fn material_for_image(
    materials: &mut Assets<StandardMaterial>,
    cache: &mut HashMap<Handle<Image>, Handle<StandardMaterial>>,
    image: &Handle<Image>,
) -> Handle<StandardMaterial> {
    if let Some(h) = cache.get(image) {
        return h.clone();
    }
    let h = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image.clone()),
        perceptual_roughness: 0.9,
        cull_mode: None, // double-sided so walls are visible from both sides
        ..default()
    });
    cache.insert(image.clone(), h.clone());
    h
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

/// A pickable wall quad for 3D selection: one mesh per texture region.
#[derive(Default)]
struct WallPreview {
    sector_id: usize,
    wall_index: usize,
    image: Handle<Image>,
    mesh: MeshData,
}

/// Build one mesh per texture used by a sector, so each surface can use the
/// image assigned to it in the map data (floor, ceiling, walls, obstacles).
/// Wall quads are also emitted as individual [`WallPreview`]s so they can be
/// pickable entities in 3D.
fn build_sector_meshes(
    sector: &Sector,
    map: &Map,
) -> (HashMap<Handle<Image>, MeshData>, Vec<WallPreview>) {
    let mut buckets: HashMap<Handle<Image>, MeshData> = HashMap::new();
    let mut walls: Vec<WallPreview> = Vec::new();

    let add_wall = |buckets: &mut HashMap<Handle<Image>, MeshData>,
                    walls: &mut Vec<WallPreview>,
                    windex: usize,
                    a: Vec2,
                    b: Vec2,
                    y0: f32,
                    y1: f32,
                    tex: &Handle<Image>| {
        let normal = interior_normal(a, b);
        bucket(buckets, tex).add_wall_quad(a, b, y0, y1, normal);
        let mut mesh = MeshData::default();
        mesh.add_wall_quad(a, b, y0, y1, normal);
        walls.push(WallPreview {
            sector_id: sector.id,
            wall_index: windex,
            image: tex.clone(),
            mesh,
        });
    };

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
    for (windex, wall) in sector.walls.iter().enumerate() {
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
                add_wall(&mut buckets, &mut walls, windex, a, b, floor_lo, floor_hi, tex);
            }

            let ceil_lo = sector.ceiling_height.min(back_sector.ceiling_height);
            let ceil_hi = sector.ceiling_height.max(back_sector.ceiling_height);
            if ceil_hi - ceil_lo > 0.001
                && let Some(tex) = wall.front_side_def.textures.upper.as_ref()
            {
                add_wall(&mut buckets, &mut walls, windex, a, b, ceil_lo, ceil_hi, tex);
            }
        } else if let Some(tex) = wall.front_side_def.textures.middle.as_ref() {
            add_wall(
                &mut buckets,
                &mut walls,
                windex,
                a,
                b,
                sector.floor_height,
                sector.ceiling_height,
                tex,
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

    (buckets, walls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapAssets;
    use crate::mode::ModeState;
    use bevy::app::{App, Update};

    fn map_assets() -> MapAssets {
        MapAssets {
            wall: Handle::default(),
            floor: Handle::default(),
            ceiling: Handle::default(),
            obstacle_side: Handle::default(),
            obstacle_top: Handle::default(),
            obstacle_bottom: Handle::default(),
        }
    }

    fn preview_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&MapPreviewMesh>().iter(world).count()
    }

    #[test]
    fn preview_rebuilds_when_texture_changes() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.insert_resource(Map::default());
        app.insert_resource(ModeState::default());
        app.add_systems(Update, update_3d_preview);

        // One sector in 3D mode.
        {
            let assets = map_assets();
            let mut map = app.world_mut().resource_mut::<Map>();
            map.add_sector_from_polygon(
                &[
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Vec2::new(0.0, 10.0),
                ],
                &assets,
            )
            .unwrap();
            app.world_mut().resource_mut::<ModeState>().mode = EditorMode::View3D;
        }

        app.update();
        let first = preview_count(&mut app);
        assert!(first > 0, "preview should spawn in 3D mode");

        // Repaint the floor with a brand-new texture handle.
        let new_tex = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            images.add(Image::default())
        };
        app.world_mut().resource_mut::<Map>().sectors[0].floor_texture = new_tex.clone();
        app.update();

        // A fresh texture creates one extra bucket (floor vs. default walls),
        // but the rebuild must REPLACE entities instead of piling them up.
        let after = preview_count(&mut app);
        assert!(after > first, "new floor bucket should be added");
        app.update();
        app.update();
        assert_eq!(
            preview_count(&mut app),
            after,
            "no-op frames must not rebuild/accumulate previews"
        );

        // Some spawned material must now reference the new floor texture.
        let world = app.world_mut();
        let mut q = world.query::<&MeshMaterial3d<StandardMaterial>>();
        let handles: Vec<Handle<StandardMaterial>> = q.iter(world).map(|h| h.0.clone()).collect();
        let materials = world.resource::<Assets<StandardMaterial>>();
        let found = handles.iter().any(|h| {
            materials
                .get(h)
                .map_or(false, |m| m.base_color_texture.as_ref() == Some(&new_tex))
        });
        assert!(found, "a preview material must use the new texture");
    }

    #[test]
    fn preview_refreshes_after_2d_paint_switch_to_3d() {
        // The now-supported workflow: paint in 2D (preview is absent), then Tab
        // to 3D — the respawned preview must use the newly painted texture.
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<StandardMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.insert_resource(Map::default());
        app.insert_resource(ModeState::default());
        app.add_systems(Update, update_3d_preview);

        let new_tex = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
        {
            let mut map = app.world_mut().resource_mut::<Map>();
            let assets = map_assets();
            map.add_sector_from_polygon(
                &[
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Vec2::new(0.0, 10.0),
                ],
                &assets,
            )
            .unwrap();
            map.sectors[0].floor_texture = new_tex.clone();
        }

        app.world_mut().resource_mut::<ModeState>().mode = EditorMode::Edit2D;
        app.update();
        assert_eq!(preview_count(&mut app), 0, "no preview in 2D mode");

        app.world_mut().resource_mut::<ModeState>().mode = EditorMode::View3D;
        app.update();
        assert!(preview_count(&mut app) > 0, "preview respawns in 3D mode");

        let world = app.world_mut();
        let mut q = world.query::<&MeshMaterial3d<StandardMaterial>>();
        let handles: Vec<Handle<StandardMaterial>> = q.iter(world).map(|h| h.0.clone()).collect();
        let materials = world.resource::<Assets<StandardMaterial>>();
        assert!(
            handles.iter().any(|h| {
                materials
                    .get(h)
                    .map_or(false, |m| m.base_color_texture.as_ref() == Some(&new_tex))
            }),
            "respawned preview must use the texture painted in 2D"
        );
    }
}
