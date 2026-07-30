use bevy::prelude::*;

use crate::picking::Selected;

/// Observer for click events - handles selection logic
pub fn on_click(
    trigger: On<Pointer<Click>>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>
) {
    // Deselect all others
    for e in &selected {
        commands.entity(e).remove::<Selected>();
    }
    commands.entity(trigger.event_target()).insert(Selected);
}
