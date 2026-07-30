use bevy::prelude::*;
use bevy::window::{ PresentMode, WindowResolution };

mod editor;
mod scene;
mod ui;
mod viewport;
mod picking;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "My Bevy App".to_string(),
                    resolution: WindowResolution::new(1920, 1080),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
        )
        .add_plugins(editor::EditorPlugin)
        .run();
}
