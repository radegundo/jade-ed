use bevy::{
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridSettings, InfiniteGridPlugin},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};

use crate::mode::{in_mode, Camera3D, EditorMode};

pub struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            InfiniteGridPlugin,
            CameraPlugin,
            GridPlugin,
        ));
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, control_fly_camera.run_if(in_mode(EditorMode::View3D)));
    }
}

/// Free-movement fly camera: free position + look angles.
/// Movement is camera-relative (WASD) with world-space vertical (Q/E).
#[derive(Component)]
pub struct FlyCamera {
    pub position: Vec3,
    pub yaw: f32,    // rotation around Y axis (horizontal look)
    pub pitch: f32,  // rotation around X axis (vertical look)
    pub speed: f32,
    pub fast_speed: f32,
    pub sensitivity: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            position: Vec3::new(50.0, 40.0, 50.0),
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: -0.4,
            speed: 20.0,
            fast_speed: 60.0,
            sensitivity: 0.003,
        }
    }
}

const SCROLL_IMPULSE: f32 = 3.0;
const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

fn control_fly_camera(
    mut camera: Query<(&mut FlyCamera, &mut Transform), With<Camera3D>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut cursor: Query<&mut CursorOptions>,
) {
    let Ok((mut fly, mut transform)) = camera.single_mut() else {
        return;
    };

    let is_looking = mouse_buttons.pressed(MouseButton::Right);

    // Lock/hide cursor while looking, restore it otherwise
    if let Ok(mut cursor) = cursor.single_mut() {
        if is_looking {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    // Look around
    if is_looking && mouse_motion.delta != Vec2::ZERO {
        fly.yaw -= mouse_motion.delta.x * fly.sensitivity;
        fly.pitch -= mouse_motion.delta.y * fly.sensitivity;
        fly.pitch = fly.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    // Orientation vectors from yaw/pitch. Forward is horizontal-only (FPS style):
    // W always moves along the ground plane, independent of pitch.
    let forward = Vec3::new(-fly.yaw.sin(), 0.0, -fly.yaw.cos()).normalize();
    let right = forward.cross(Vec3::Y).normalize();

    let speed = if keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight)
        || keys.pressed(KeyCode::Space)
    {
        fly.fast_speed
    } else {
        fly.speed
    };

    let mut movement = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        movement += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        movement -= forward;
    }
    if keys.pressed(KeyCode::KeyA) {
        movement -= right;
    }
    if keys.pressed(KeyCode::KeyD) {
        movement += right;
    }
    if keys.pressed(KeyCode::KeyQ) {
        movement -= Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyE) {
        movement += Vec3::Y;
    }

    // Scroll wheel nudges forward/back (one-shot impulse per click)
    if mouse_scroll.delta.y != 0.0 {
        movement += forward * mouse_scroll.delta.y * SCROLL_IMPULSE;
    }

    if movement != Vec3::ZERO {
        fly.position += movement.normalize() * speed * time.delta_secs();
    }

    // Apply derived transform
    transform.translation = fly.position;
    transform.rotation = Quat::from_axis_angle(Vec3::Y, fly.yaw)
        * Quat::from_axis_angle(Vec3::X, fly.pitch);
}

pub struct GridPlugin;

impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_grid);
    }
}

fn spawn_grid(mut commands: Commands) {
    commands.spawn((
        InfiniteGrid,
        InfiniteGridSettings {
            x_axis_color: Color::srgb(0.8, 0.24, 0.24),
            z_axis_color: Color::srgb(0.33, 0.66, 0.33),
            minor_line_color: Color::srgb(0.28, 0.28, 0.28),
            major_line_color: Color::srgb(0.4, 0.4, 0.4),
            fadeout_distance: 150.0,
            ..default()
        },
    ));
}
