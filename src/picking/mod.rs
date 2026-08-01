use bevy::prelude::*;

// Re-export components and systems from submodules
// pub use highlight::*;

use crate::picking::{
    controller::update_picking_state,
    drag::update_drag,
    visuals::{ Hovered, MaterialCache },
};

// pub mod highlight;
pub mod drag;
pub mod visuals;
pub mod state;
pub mod controller;

pub struct OwnPickingPlugin;

// ── Observers (fire when pointer events occur) ────────────────────

pub fn on_hover_enter(trigger: On<Pointer<Over>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).insert(Hovered);
}

pub fn on_hover_exit(trigger: On<Pointer<Out>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).remove::<Hovered>();
}

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin)
            .init_resource::<state::PickingState>()
            .init_resource::<drag::DragState>()
            .init_resource::<MaterialCache>()

            //OBSERVERS
            .add_observer(on_hover_enter)
            .add_observer(on_hover_exit)
            .add_observer(drag::on_press)

            .add_systems(PreUpdate, update_picking_state)
            .add_systems(
                Update,
                (visuals::restore_unmarked_materials, visuals::apply_material_tints).chain()
            )
            .add_systems(Update, update_drag);
    }
}
