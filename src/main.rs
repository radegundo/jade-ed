use bevy::{ prelude::*, window::{ PresentMode, WindowResolution } };
use bevy::dev_tools::infinite_grid::InfiniteGridPlugin;

use bevy_egui::{ egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass };

pub mod viewport;

use crate::viewport::camera::CameraPlugin;
use viewport::grid::GridPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "My Bevy App".to_string(),
                    resolution: WindowResolution::new(1920, 1080),
                    present_mode: PresentMode::AutoVsync,
                    resizable: false,
                    ..default()
                }),
                ..default()
            })
        )
        .add_plugins(EditorPlugin)
        .add_systems(Startup, setup_scene)
        .add_plugins(EguiPlugin::default())
        .add_systems(EguiPrimaryContextPass, ui_example_system)
        .run();
}

struct EditorPlugin;
impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((InfiniteGridPlugin, CameraPlugin, GridPlugin)).add_systems(
            Startup,
            setup_scene
        );
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh<>>>,
    mut materials: ResMut<Assets<StandardMaterial<>>>
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.7, 0.7),
                perceptual_roughness: 0.85,
                ..default()
            })
        ),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::IDENTITY.looking_to(Vec3::new(-1.0, -2.0, -1.0).normalize(), Vec3::Y),
    ));
}

fn ui_example_system(mut contexts: EguiContexts) -> Result {
    egui::Window::new("Hello").show(contexts.ctx_mut()?, |ui| {
        ui.label("world");
    });
    Ok(())
}
