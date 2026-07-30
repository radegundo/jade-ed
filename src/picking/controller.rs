use bevy::{ picking::pointer::PointerInteraction, prelude::* };
use bevy::picking::prelude::*;
use super::state::{ PickingState, snap_to_grid };

/// Compute all picking data once per frame.
pub fn update_picking_state(
    mut state: ResMut<PickingState>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    interactions: Query<&PointerInteraction>,
    mut last_cursor: Local<Vec2>
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };

    // Cursor position
    let cursor_pos = window.cursor_position().unwrap_or(*last_cursor);
    state.cursor_pos_prev = *last_cursor;
    state.cursor_pos = cursor_pos;
    *last_cursor = cursor_pos;

    // Mouse button states
    state.just_pressed = mouse_buttons.just_pressed(MouseButton::Left);
    state.just_released = mouse_buttons.just_released(MouseButton::Left);
    state.is_pressed = mouse_buttons.pressed(MouseButton::Left);

    // Modifier keys
    state.shift_held =
        keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    state.ctrl_held =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    state.alt_held = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);

    // Camera ray
    let ray = match camera.viewport_to_world(camera_transform, cursor_pos) {
        Ok(r) => r,
        Err(_) => {
            return;
        }
    };
    state.camera_ray = Some(ray);

    // Ground plane intersection (y = 0)
    state.ground_hit = if ray.direction.y.abs() > 0.001 {
        let t = -ray.origin.y / ray.direction.y;
        if t > 0.0 {
            let hit = ray.origin + ray.direction * t;
            Some(hit)
        } else {
            None
        }
    } else {
        None
    };

    // Snapped ground hit
    state.ground_hit_snapped = state.ground_hit.map(|h| snap_to_grid(h, 1.0));

    // Mesh picking results from MeshPickingPlugin
    state.hovered_entity = None;
    state.mesh_hit_point = None;
    state.mesh_hit_normal = None;

    for interaction in &interactions {
        if let Some((entity, hit)) = interaction.get_nearest_hit() {
            state.hovered_entity = Some(*entity);
            state.mesh_hit_point = hit.position;
            state.mesh_hit_normal = hit.normal;
        }
    }
}
