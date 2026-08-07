use bevy::prelude::*;
use bevy::picking::PickingSystems;

// Re-export components and systems from submodules
// pub use highlight::*;

use crate::mode::{in_mode, EditorMode};
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

pub fn on_hover_enter(
    trigger: On<Pointer<Over>>,
    mut commands: Commands,
    state: Res<crate::picking::state::PickingState>,
) {
    if state.pointer_over_egui {
        return;
    }
    if let Ok(mut entity) = commands.get_entity(trigger.event_target()) {
        entity.insert(Hovered);
    }
}

pub fn on_hover_exit(trigger: On<Pointer<Out>>, mut commands: Commands) {
    if let Ok(mut entity) = commands.get_entity(trigger.event_target()) {
        entity.remove::<Hovered>();
    }
}

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        // Ray-cast everything whose `InheritedVisibility` is true instead of the
        // default `VisibleInView` (which additionally requires `ViewVisibility`
        // to be computed by the render frustum systems). Preview walls must be
        // pickable from both sides and independent of per-frame frustum state.
        app.insert_resource(MeshPickingSettings {
            ray_cast_visibility: RayCastVisibility::Visible,
            ..default()
        });
        app.add_plugins(MeshPickingPlugin)
            .init_resource::<state::PickingState>()
            .init_resource::<drag::DragState>()
            .init_resource::<MaterialCache>()

            //OBSERVERS
            .add_observer(on_hover_enter)
            .add_observer(on_hover_exit)
            .add_observer(drag::on_press)

            // Read the picking pipeline's current-frame hover state: run after
            // `PickingSystems::Hover` (generate_hovermap -> update_interactions
            // -> PointerInteraction) so `state.hovered_entity` is never a frame
            // stale.
            .add_systems(PreUpdate, update_picking_state.after(PickingSystems::Hover))
            .add_systems(
                Update,
                (visuals::restore_unmarked_materials, visuals::apply_material_tints).chain()
            )
            .add_systems(Update, update_drag.run_if(in_mode(EditorMode::Edit2D)));
    }
}
