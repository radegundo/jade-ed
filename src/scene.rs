use bevy::prelude::*;

use crate::picking::*;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene);
    }
}

pub fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>
) {
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.7, 0.7, 0.7),
                    perceptual_roughness: 0.85,
                    ..default()
                })
            ),
            Transform::from_xyz(0.0, 0.5, 0.0),
            Pickable::default(),
        ))
        .observe(on_hover_enter)
        .observe(on_hover_exit);
    // .observe(on_click);
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::IDENTITY.looking_to(Vec3::new(-1.0, -2.0, -1.0).normalize(), Vec3::Y),
    ));
}
