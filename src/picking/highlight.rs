use bevy::prelude::*;

use crate::picking::*;

pub fn on_hover(trigger: On<Pointer<Over>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).insert(Hovered);
}

pub fn on_unhover(trigger: On<Pointer<Out>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).remove::<Hovered>();
}
