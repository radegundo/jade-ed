use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
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
    mut material: Local<Option<Handle<StandardMaterial>>>,
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

    let mat = match material.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = materials.add(StandardMaterial {
                base_color: Color::srgb(0.65, 0.65, 0.7),
                perceptual_roughness: 0.9,
                cull_mode: None, // double-sided so walls are visible from both sides
                ..default()
            });
            *material = Some(h.clone());
            h
        }
    };

    for sector in &map.sectors {
        let mesh = build_sector_mesh(sector, &map.vertices);
        commands.spawn((
            MapPreviewMesh,
            VisibleIn3D,
            // Previews are not interactive; without this they'd be pickable by
            // default and could race with the despawn/respawn cycle below.
            Pickable::IGNORE,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
        ));
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
}

/// Interior-facing normal for a wall from `a` to `b` on the XZ plane.
/// Sectors are wound counter-clockwise, so the interior is to the left
/// of the traversal direction.
fn interior_normal(a: Vec2, b: Vec2) -> Vec3 {
    let (dx, dz) = (b.x - a.x, b.y - a.y);
    Vec3::new(-dz, 0.0, dx).normalize_or_zero()
}

fn build_sector_mesh(sector: &Sector, vertices: &[Vec2]) -> Mesh {
    let mut data = MeshData::default();

    // Sector outline (walls in order describe a convex polygon)
    let outline: Vec<Vec2> = sector.walls.iter().map(|w| *w.start(vertices)).collect();

    // Floor (visible from above) and ceiling (visible from below)
    data.add_polygon(&outline, sector.floor_height, Vec3::Y);
    data.add_polygon(&outline, sector.ceiling_height, Vec3::NEG_Y);

    // Wall quads
    for wall in &sector.walls {
        let a = *wall.start(vertices);
        let b = *wall.end(vertices);
        data.add_wall_quad(
            a,
            b,
            sector.floor_height,
            sector.ceiling_height,
            interior_normal(a, b),
        );
    }

    // Obstacles: prisms between bottom..top
    for obs in &sector.obstacles {
        let pts: Vec<Vec2> = obs.edges.iter().map(|e| *e.start(vertices)).collect();
        data.add_polygon(&pts, obs.top, Vec3::Y);
        data.add_polygon(&pts, obs.bottom, Vec3::NEG_Y);
        for edge in &obs.edges {
            let a = *edge.start(vertices);
            let b = *edge.end(vertices);
            data.add_wall_quad(a, b, obs.bottom, obs.top, interior_normal(a, b));
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs);
    mesh.insert_indices(Indices::U32(data.indices));
    mesh
}
