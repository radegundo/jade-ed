use bevy::prelude::*;

use crate::picking::{ Hovered, OriginalMaterial, Selected, drag::DragState };

pub fn update_visual_state(
    mut commands: Commands,
    entities: Query<(Entity, Option<&Hovered>, Option<&Selected>), With<Mesh3d>>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Query<&OriginalMaterial>
) {
    for (entity, hover, select) in &entities {
        let has_hover = hover.is_some();
        let has_select = select.is_some();

        let original = if let Ok(orig) = originals.get(entity) {
            orig.0.clone()
        } else if let Ok(mesh_mat) = mesh_materials.get(entity) {
            let h = mesh_mat.0.clone();
            commands.entity(entity).insert(OriginalMaterial(h.clone()));
            h
        } else {
            continue;
        };

        let tint = match (has_select, has_hover) {
            (true, true) => Some(Color::srgb(1.0, 0.7, 0.3)),
            (true, false) => Some(Color::srgb(1.0, 0.5, 0.1)),
            (false, true) => Some(Color::srgb(0.5, 0.6, 1.0)),
            (false, false) => None,
        };

        if let Some(color) = tint {
            if let Some(mat) = materials.get(&original) {
                let mut t = mat.clone();
                t.base_color = blend(mat.base_color, color, 0.4);
                commands.entity(entity).insert(MeshMaterial3d(materials.add(t)));
            }
        } else {
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
