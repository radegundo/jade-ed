use bevy::prelude::*;
use std::collections::HashMap;
use crate::picking::drag::BeingDragged;

// ── Marker Components ─────────────────────────────────────────────

#[derive(Component)]
pub struct Hovered;

#[derive(Component)]
pub struct Selected;

// ── Resource: stores original materials (avoids query conflicts) ──

#[derive(Resource, Default)]
pub struct MaterialCache {
    pub originals: HashMap<Entity, Handle<StandardMaterial>>,
}

// ── System 1: Restore materials for unmarked entities ─────────────

pub fn restore_unmarked_materials(
    mut commands: Commands,
    query: Query<
        Entity,
        (
            With<MeshMaterial3d<StandardMaterial>>,
            Without<Hovered>,
            Without<Selected>,
            Without<BeingDragged>,
        )
    >,
    mut cache: ResMut<MaterialCache>
) {
    for entity in &query {
        if let Some(orig) = cache.originals.remove(&entity)
            && let Ok(mut entity_cmds) = commands.get_entity(entity)
        {
            entity_cmds.insert(MeshMaterial3d(orig));
        }
    }
}

// ── System 2: Apply tints to marked entities ──────────────────────

pub fn apply_material_tints(
    mut commands: Commands,
    marked: Query<
        (Entity, Has<Hovered>, Has<Selected>, Has<BeingDragged>),
        Or<(With<Hovered>, With<Selected>, With<BeingDragged>)>
    >,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<MaterialCache>
) {
    for (entity, has_hover, has_select, has_drag) in &marked {
        let tint = if has_drag {
            Some(Color::srgb(1.0, 1.0, 0.0)) // yellow
        } else if has_select && has_hover {
            Some(Color::srgb(1.0, 0.7, 0.3)) // light orange
        } else if has_select {
            Some(Color::srgb(1.0, 0.5, 0.1)) // orange
        } else if has_hover {
            Some(Color::srgb(0.5, 0.6, 1.0)) // blue
        } else {
            None
        };

        let Some(color) = tint else {
            continue;
        };

        // Get or store original material
        let original = if let Some(h) = cache.originals.get(&entity) {
            h.clone()
        } else if let Ok(mesh_mat) = mesh_materials.get(entity) {
            let h = mesh_mat.0.clone();
            cache.originals.insert(entity, h.clone());
            h
        } else {
            continue;
        };

        // Apply tint
        if let Some(mat) = materials.get(&original)
            && let Ok(mut entity_cmds) = commands.get_entity(entity)
        {
            let mut tinted = mat.clone();
            tinted.base_color = blend(mat.base_color, color, 0.4);
            entity_cmds.insert(MeshMaterial3d(materials.add(tinted)));
        }
    }
}

// ── Helper ────────────────────────────────────────────────────────

fn blend(base: Color, tint: Color, factor: f32) -> Color {
    let a = base.to_linear();
    let b = tint.to_linear();
    Color::LinearRgba(
        LinearRgba::new(
            a.red + (b.red - a.red) * factor,
            a.green + (b.green - a.green) * factor,
            a.blue + (b.blue - a.blue) * factor,
            a.alpha
        )
    )
}
