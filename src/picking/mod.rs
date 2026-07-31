use bevy::prelude::*;

// Re-export components and systems from submodules
// pub use highlight::*;

use crate::picking::{ controller::update_picking_state, visuals::{ update_material_highlights } };

#[derive(Component)]
pub struct Hovered;
#[derive(Component)]
pub struct Selected;
#[derive(Component)]
pub struct OriginalMaterial(Handle<StandardMaterial>);
#[derive(Component)]
pub struct HasHoverObservers;

// pub mod highlight;
pub mod click;
pub mod drag;
pub mod visuals;
pub mod state;
pub mod controller;

/// Observer: mouse entered entity.
pub fn on_hover_enter(trigger: On<Pointer<Over>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).insert(Hovered);
}

/// Observer: mouse left entity.
pub fn on_hover_exit(trigger: On<Pointer<Out>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).remove::<Hovered>();
}

// pub fn on_click(
//     trigger: On<Pointer<Click>>,
//     mut commands: Commands,
//     selected: Query<Entity, With<Selected>>
// ) {
//     if trigger.button != PointerButton::Primary {
//         return;
//     }

//     // Deselect all others
//     for e in &selected {
//         commands.entity(e).remove::<Selected>();
//     }

//     // Select clicked entity
//     commands.entity(trigger.event_target()).insert(Selected);
// }

// Plugin that combines both highlight and clicking functionality
pub struct OwnPickingPlugin;

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin)
            .init_resource::<state::PickingState>()
            .init_resource::<drag::DragState>()

            .add_observer(on_hover_enter)
            .add_observer(on_hover_exit)
            .add_observer(drag::on_press)

            .add_systems(PreUpdate, update_picking_state)
            .add_systems(Update, update_material_highlights)
            .add_systems(
                Update,
                drag::update_drag
                    .after(controller::update_picking_state) // PreUpdate system
                    .before(visuals::update_material_highlights)
            );
    }
}
