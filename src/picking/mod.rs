use bevy::prelude::*;

// Re-export components and systems from submodules
pub use highlight::*;
pub use click::*;

#[derive(Component)]
pub struct Hovered;
#[derive(Component)]
pub struct Selected;
#[derive(Component)]
pub struct OriginalMaterial(Handle<StandardMaterial>);
#[derive(Component)]
pub struct HasHoverObservers;

pub mod highlight;
pub mod click;
pub mod drag;
pub mod visuals;
pub mod state;

//ATTACH OBSERVERS TO REQUIRED ENTITIES
pub fn attach_observers(
    mut commands: Commands,
    query: Query<Entity, (With<Mesh3d>, Without<HasHoverObservers>)>
) {
    for entity in &query {
        commands
            .entity(entity)
            .observe(on_hover)
            .observe(on_unhover)
            .observe(on_click)
            .insert(HasHoverObservers);
    }
}

// Plugin that combines both highlight and clicking functionality
pub struct OwnPickingPlugin;

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin).add_systems(Update, (
            attach_observers,
            visuals::update_visual_state,
        ));
    }
}
