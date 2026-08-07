use bevy::prelude::*;
use crate::tools::{EditorTool, ToolState, WallHandle};

/// Attached to entity while being dragged.
#[derive(Component)]
pub struct BeingDragged {
    pub grab_offset: Vec3,
    pub start_position: Vec3,
}

/// Global drag tracking.
#[derive(Resource, Default)]
pub struct DragState {
    pub entity: Option<Entity>,
    pub press_screen_pos: Vec2,
    pub is_dragging: bool,
}

const DRAG_THRESHOLD: f32 = 5.0;

/// Observer: mouse pressed on entity. Start tracking, but DON'T drag yet.
pub fn on_press(
    trigger: On<Pointer<Press>>,
    mut drag_state: ResMut<DragState>,
    state: Res<crate::picking::state::PickingState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    // Wall handles and 3D preview walls are select-only (texture assignment);
    // dragging them would fight the sync/respawn systems that glue them to their
    // walls.
    meshes: Query<(), (With<Transform>, With<Mesh3d>, Without<WallHandle>, Without<crate::map_preview::PickableWall>)>,
    tool: Res<ToolState>,
) {
    if trigger.button != PointerButton::Primary {
        return;
    }

    // A press over egui belongs to the UI, not to the pickable behind it.
    if state.pointer_over_egui {
        return;
    }

    // Dragging handles only happens in Select mode; the draw/stamp tools
    // consume clicks for their own purposes.
    if tool.tool != EditorTool::Select {
        return;
    }

    // Space + left is reserved for camera pan, never vertex drag.
    if keyboard.pressed(KeyCode::Space) {
        return;
    }

    // Only track presses on actual mesh entities
    if meshes.get(trigger.event_target()).is_err() {
        return;
    }

    // Don't overwrite active drag
    if drag_state.entity.is_some() {
        return;
    }

    drag_state.entity = Some(trigger.event_target());
    drag_state.press_screen_pos = state.cursor_pos;
    drag_state.is_dragging = false;
}

/// System: handle drag logic every frame.
pub fn update_drag(
    mut commands: Commands,
    mut drag_state: ResMut<DragState>,
    state: Res<crate::picking::state::PickingState>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut transforms: Query<&mut Transform>,
    dragged_query: Query<&BeingDragged>,
    keyboard: Res<ButtonInput<KeyCode>>,
    tool: Res<ToolState>,
) {
    let Some(entity) = drag_state.entity else {
        return;
    };

    // Drags are only driven in Select mode. A tool switch mid-drag aborts it.
    if tool.tool != EditorTool::Select {
        if let Ok(mut entity_cmds) = commands.get_entity(entity) {
            entity_cmds.remove::<BeingDragged>();
        }
        *drag_state = DragState::default();
        return;
    }

    // Space + left switches to pan: abort any in-flight vertex drag so the
    // camera pan (control_2d_camera) takes over exclusively.
    if keyboard.pressed(KeyCode::Space) {
        if let Ok(mut entity_cmds) = commands.get_entity(entity) {
            entity_cmds.remove::<BeingDragged>();
        }
        *drag_state = DragState::default();
        return;
    }

    // ── Mouse released ───────────────────────────────────────────
    if !mouse_buttons.pressed(MouseButton::Left) {
        if drag_state.is_dragging {
            // Final snap
            if let Ok(mut transform) = transforms.get_mut(entity) {
                if !state.alt_held {
                    transform.translation.x = (transform.translation.x / 1.0).round() * 1.0;
                    transform.translation.z = (transform.translation.z / 1.0).round() * 1.0;
                }
            }
            if let Ok(mut entity_cmds) = commands.get_entity(entity) {
                entity_cmds.remove::<BeingDragged>();
            }
        }
        *drag_state = DragState::default();
        return;
    }

    // ── Check threshold, start drag if past ──────────────────────
    if !drag_state.is_dragging {
        let delta = state.cursor_pos - drag_state.press_screen_pos;
        if delta.length() < DRAG_THRESHOLD {
            return; // still a click, not a drag
        }

        // START DRAG — compute grab offset and insert component
        drag_state.is_dragging = true;

        // Use block scope to avoid double borrow
        let (grab_offset, start_pos) = {
            let Ok(transform) = transforms.get(entity) else {
                return;
            };
            let grab_point = state.mesh_hit_point.unwrap_or(transform.translation);
            let offset = transform.translation - grab_point;
            (offset, transform.translation)
        };

        if let Ok(mut entity_cmds) = commands.get_entity(entity) {
            entity_cmds.insert(BeingDragged {
                grab_offset,
                start_position: start_pos,
            });
        }
    }

    // ── Move entity ──────────────────────────────────────────────
    let Ok(drag) = dragged_query.get(entity) else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(entity) else {
        return;
    };

    let target = if let Some(ground) = state.ground_hit_snapped {
        Vec3::new(ground.x, drag.start_position.y, ground.z) + drag.grab_offset
    } else if let Some(mesh_point) = state.mesh_hit_point {
        mesh_point + drag.grab_offset
    } else {
        return;
    };

    transform.translation = target;
}
