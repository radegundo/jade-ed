use bevy::prelude::*;

pub struct OwnPickingPlugin;

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin).add_observer(on_hover_enter).add_observer(on_hover_exit);
    }
}

/// Component to store original material handle.
#[derive(Component)]
pub struct OriginalMaterial(pub Handle<StandardMaterial>);

pub fn on_hover_enter(
    trigger: On<Pointer<Over>>,
    mut commands: Commands,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>
) {
    let entity = trigger.event_target();

    let Ok(mesh_mat) = mesh_materials.get(entity) else {
        return;
    };

    commands.entity(entity).insert(OriginalMaterial(mesh_mat.0.clone()));

    if let Some(mat) = materials.get(&mesh_mat.0) {
        let mut tinted = mat.clone();
        tinted.base_color = blend_to_blue(mat.base_color, 0.3);

        commands.entity(entity).insert(MeshMaterial3d(materials.add(tinted)));
    }
}

pub fn on_hover_exit(
    trigger: On<Pointer<Out>>,
    mut commands: Commands,
    originals: Query<&OriginalMaterial>
) {
    let entity = trigger.event_target();

    let Ok(orig) = originals.get(entity) else {
        return;
    };

    // Restore original material
    commands.entity(entity).insert(MeshMaterial3d(orig.0.clone())).remove::<OriginalMaterial>();
}

//---------------------------HELPERS--------------------------------

fn blend_to_blue(color: Color, factor: f32) -> Color {
    let c = color.to_linear();
    Color::LinearRgba(
        LinearRgba::new(
            c.red * (1.0 - factor),
            c.green * (1.0 - factor),
            c.blue + (1.0 - c.blue) * factor,
            c.alpha
        )
    )
}
