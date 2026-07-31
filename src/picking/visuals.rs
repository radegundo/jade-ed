use bevy::prelude::*;

use crate::picking::{ Hovered, OriginalMaterial, Selected };

pub fn update_material_highlights(
    mut commands: Commands,
    entities: Query<
        Entity,
        Or<
            (
                With<Hovered>,
                With<Selected>,
                With<super::drag::BeingDragged>,
                Changed<Hovered>,
                Changed<Selected>,
            )
        >
    >,
    hovered: Query<(), With<Hovered>>,
    selected: Query<(), With<Selected>>,
    dragged: Query<(), With<super::drag::BeingDragged>>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut originals: Query<&mut OriginalMaterial>
) {
    for entity in &entities {
        let is_dragged = dragged.get(entity).is_ok();
        let is_selected = selected.get(entity).is_ok();
        let is_hovered = hovered.get(entity).is_ok();

        // Get or store original material
        let original = if let Ok(orig) = originals.get(entity) {
            orig.0.clone()
        } else if let Ok(mesh_mat) = mesh_materials.get(entity) {
            let h = mesh_mat.0.clone();
            commands.entity(entity).insert(OriginalMaterial(h.clone()));
            h
        } else {
            continue;
        };

        // Determine tint by priority: drag > select+hover > select > hover > none
        let tint = if is_dragged {
            Some(Color::srgb(1.0, 1.0, 0.0)) // yellow
        } else if is_selected && is_hovered {
            Some(Color::srgb(1.0, 0.7, 0.3)) // light orange
        } else if is_selected {
            Some(Color::srgb(1.0, 0.5, 0.1)) // orange
        } else if is_hovered {
            Some(Color::srgb(0.5, 0.6, 1.0)) // blue
        } else {
            None
        };

        if let Some(tint_color) = tint {
            if let Some(mat) = materials.get(&original) {
                let mut tinted = mat.clone();
                tinted.base_color = blend(mat.base_color, tint_color, 0.4);
                commands.entity(entity).insert(MeshMaterial3d(materials.add(tinted)));
            }
        } else {
            // Restore original
            commands.entity(entity).insert(MeshMaterial3d(original)).remove::<OriginalMaterial>();
        }
    }
}

fn blend(a: Color, b: Color, f: f32) -> Color {
    let a = a.to_linear();
    let b = b.to_linear();
    Color::LinearRgba(
        LinearRgba::new(
            a.red + (b.red - a.red) * f,
            a.green + (b.green - a.green) * f,
            a.blue + (b.blue - a.blue) * f,
            a.alpha
        )
    )
}
