use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use crate::map::Map;
use crate::mode::{in_mode, EditorMode, VisibleIn2D};
use crate::picking::drag::BeingDragged;

/// Attached to the pickable sphere entity representing a map vertex.
#[derive(Component)]
pub struct VertexHandle {
    pub index: usize,
}

pub struct MapHandlesPlugin;

impl Plugin for MapHandlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_handles, sync_dragged_to_map)
                .chain()
                .run_if(in_mode(EditorMode::Edit2D)),
        );
    }
}

/// Spawn/update vertex handle entities. Remove handles for deleted vertices.
/// Dragged entities are skipped (their transform is mid-drag) but still
/// counted as existing so we don't spawn duplicates.
fn sync_handles(
    mut commands: Commands,
    map: Res<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut handles: Query<(Entity, &VertexHandle, Option<&BeingDragged>, &mut Transform)>,
    mut sphere_mesh: Local<Option<Handle<Mesh>>>,
    mut sphere_material: Local<Option<Handle<StandardMaterial>>>,
) {
    let mesh = match sphere_mesh.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = meshes.add(Sphere::new(0.4));
            *sphere_mesh = Some(h.clone());
            h
        }
    };
    let material = match sphere_material.as_ref() {
        Some(h) => h.clone(),
        None => {
            let h = materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.9, 0.3),
                unlit: true,
                ..default()
            });
            *sphere_material = Some(h.clone());
            h
        }
    };

    let mut by_index: HashMap<usize, Entity> = HashMap::new();
    for (entity, handle, _, _) in &handles {
        by_index.insert(handle.index, entity);
    }

    let mut seen: HashSet<usize> = HashSet::new();
    for (index, pos) in map.vertices.iter().enumerate() {
        seen.insert(index);
        let target = Vec3::new(pos.x, 0.0, pos.y);
        if let Some(&entity) = by_index.get(&index)
            && let Ok((_, _, dragged, mut transform)) = handles.get_mut(entity)
            && dragged.is_none()
            && transform.translation != target
        {
            transform.translation = target;
        }
    }

    for (index, pos) in map.vertices.iter().enumerate() {
        if !by_index.contains_key(&index) {
            commands.spawn((
                VertexHandle { index },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(Vec3::new(pos.x, 0.0, pos.y)),
                Pickable::default(),
                VisibleIn2D,
            ));
        }
    }

    for (&index, &entity) in by_index.iter() {
        if !seen.contains(&index) {
            commands.entity(entity).despawn();
        }
    }
}

/// While dragging, write the entity position back to Map.vertices.
/// Only X and Z are synced; Y stays 0 for ground-plane vertices.
fn sync_dragged_to_map(
    mut map: ResMut<Map>,
    dragged: Query<(&Transform, &VertexHandle), With<BeingDragged>>,
) {
    for (transform, handle) in &dragged {
        if handle.index < map.vertices.len() {
            map.vertices[handle.index] =
                Vec2::new(transform.translation.x, transform.translation.z);
        }
    }
}
