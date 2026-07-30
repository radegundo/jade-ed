use bevy::{
    dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridSettings, InfiniteGridPlugin},
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
};

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
        app.add_systems(Startup, spawn_camera).add_systems(Update, control);
    }
}

#[derive(Component)]
pub struct OrbitCamera {
    pub focus: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 10.0,
            yaw: -0.7,
            pitch: 0.5,
        }
    }
}

const ORBIT_SENSITIVITY: f32 = 0.005;
const PAN_SENSITIVITY: f32 = 0.0015;
const WHEEL_ZOOM_SENSITIVITY: f32 = 0.12;
const TRACKPAD_ZOOM_SENSITIVITY: f32 = 0.01;
const PINCH_ZOOM_SENSITIVITY: f32 = 3.0;
const TRACKPAD_MOTION_SCALE: f32 = 0.4;
const MIN_DISTANCE: f32 = 0.5;
const MAX_DISTANCE: f32 = 500.0;

impl OrbitCamera {
    fn orbit_by(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SENSITIVITY;
        self.pitch -= delta.y * ORBIT_SENSITIVITY;
    }

    fn pan(&mut self, transform: &Transform, delta: Vec2) {
        let right = *transform.right();
        let up = *transform.up();
        let scale = PAN_SENSITIVITY * self.distance;
        self.focus += (-right * delta.x + up * delta.y) * scale;
    }

    fn zoom(&mut self, amount: f32) {
        self.distance = (self.distance * (1.0 - amount)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    fn apply_to(&self, transform: &mut Transform) {
        let rot =
            Quat::from_axis_angle(Vec3::Y, self.yaw) * Quat::from_axis_angle(Vec3::X, self.pitch);
        transform.rotation = rot;
        transform.translation = self.focus + rot * Vec3::new(0.0, 0.0, self.distance);
    }
}

fn spawn_camera(mut commands: Commands) {
    let orbit = OrbitCamera::default();
    let mut transform = Transform::IDENTITY;
    orbit.apply_to(&mut transform);
    commands.spawn((Camera3d::default(), transform, orbit));
}

fn control(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mut pinch_reader: MessageReader<PinchGesture>,
    mut camera: Single<(&mut OrbitCamera, &mut Transform)>,
) {
    let (orbit, transform) = &mut *camera;
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl =
        keys.pressed(KeyCode::ControlLeft) ||
        keys.pressed(KeyCode::ControlRight) ||
        keys.pressed(KeyCode::SuperLeft) ||
        keys.pressed(KeyCode::SuperRight);
    if mouse_buttons.pressed(MouseButton::Middle) {
        let delta = mouse_motion.delta;
        if delta != Vec2::ZERO {
            if shift {
                orbit.pan(transform, delta);
            } else {
                orbit.orbit_by(delta);
            }
        }
    }
    let scroll = mouse_scroll.delta;
    if scroll != Vec2::ZERO {
        match mouse_scroll.unit {
            MouseScrollUnit::Line => orbit.zoom(scroll.y * WHEEL_ZOOM_SENSITIVITY),
            MouseScrollUnit::Pixel => {
                let d = scroll * TRACKPAD_MOTION_SCALE;
                if ctrl {
                    orbit.zoom(d.y * TRACKPAD_ZOOM_SENSITIVITY);
                } else if shift {
                    orbit.pan(transform, d);
                } else {
                    orbit.orbit_by(Vec2::new(-d.x, -d.y));
                }
            }
        }
    }
    let pinch: f32 = pinch_reader.read().map(|g| g.0).sum();
    if pinch != 0.0 {
        orbit.zoom(pinch * PINCH_ZOOM_SENSITIVITY);
    }
    orbit.apply_to(transform);
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
