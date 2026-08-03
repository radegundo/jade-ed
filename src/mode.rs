use bevy::camera::ScalingMode;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::map::Map;
use crate::picking::state::PickingState;

/// Single source of truth for which mode is active.
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy, Debug)]
pub struct ModeState {
    pub mode: EditorMode,
}

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum EditorMode {
    #[default]
    View3D,
    Edit2D,
}

/// Tag for the 2D orthographic camera.
#[derive(Component)]
pub struct Camera2D;

/// Tag for the 3D perspective camera.
#[derive(Component)]
pub struct Camera3D;

/// Tag for entities that should only be visible in 3D mode.
#[derive(Component)]
pub struct VisibleIn3D;

/// Tag for entities that should only be visible in 2D mode.
#[derive(Component)]
pub struct VisibleIn2D;

/// Target zoom scale for smooth 2D camera zoom (interpolated each frame).
#[derive(Component)]
pub struct Camera2DZoom {
    pub target_scale: f32,
}

/// Tracks 2D camera edge-pan state (cursor near a window edge auto-pans).
#[derive(Component, Default)]
pub struct Camera2DEdgePan {
    pub velocity: Vec2,
}

pub struct ModePlugin;

impl Plugin for ModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModeState>()
            .add_systems(Startup, spawn_cameras)
            .add_systems(Update, toggle_mode)
            .add_systems(Update, control_2d_camera.run_if(in_mode(EditorMode::Edit2D)))
            .add_systems(Update, center_2d_camera_on_map.run_if(in_mode(EditorMode::Edit2D)));
    }
}

/// Run condition: only run when in the specified mode.
pub fn in_mode(mode: EditorMode) -> impl FnMut(Res<ModeState>) -> bool + Clone {
    move |state: Res<ModeState>| state.mode == mode
}

// ── Startup: spawn both cameras, 3D visible, 2D hidden ──────────

fn spawn_cameras(mut commands: Commands) {
    // 3D fly camera: transform derived from the FlyCamera state
    let fly = crate::viewport::FlyCamera::default();
    let mut transform = Transform::IDENTITY;
    transform.translation = fly.position;
    transform.rotation =
        Quat::from_axis_angle(Vec3::Y, fly.yaw) * Quat::from_axis_angle(Vec3::X, fly.pitch);
    commands.spawn((
        Camera3d::default(),
        Camera3D,
        Camera { is_active: true, ..default() },
        transform,
        fly,
    ));

    commands.spawn((
        Camera3d::default(),
        Camera2D,
        Camera { is_active: false, ..default() },
        Transform::from_xyz(5.0, 50.0, 5.0)
            .looking_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Z),
        Projection::Orthographic(OrthographicProjection {
            scale: 0.15,
            near: 0.1,
            far: 1000.0,
            viewport_origin: Vec2::new(0.5, 0.5),
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_3d()
        }),
        Camera2DZoom { target_scale: 0.15 },
        Camera2DEdgePan::default(),
        Visibility::Hidden,
    ));
}

// ── Toggle system: Tab switches mode ─────────────────────────────

fn toggle_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut mode_state: ResMut<ModeState>,
    mut drag_state: ResMut<crate::picking::drag::DragState>,
    mut cam2d: Query<(&mut Camera, &mut Visibility), With<Camera2D>>,
    mut cam3d: Query<(&mut Camera, &mut Visibility), (With<Camera3D>, Without<Camera2D>)>,
    mut vis2d: Query<&mut Visibility, (With<VisibleIn2D>, Without<VisibleIn3D>, Without<Camera2D>, Without<Camera3D>)>,
    mut vis3d: Query<&mut Visibility, (With<VisibleIn3D>, Without<VisibleIn2D>, Without<Camera2D>, Without<Camera3D>)>,
    dragged: Query<Entity, With<crate::picking::drag::BeingDragged>>,
    mut cursor: Query<&mut CursorOptions>,
) {
    if !keyboard.just_pressed(KeyCode::Tab) {
        return;
    }

    // Always restore the cursor on mode switch so it can't get stuck
    // hidden/locked from a 3D look-drag when entering 2D mode.
    if let Ok(mut cursor) = cursor.single_mut() {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }

    mode_state.mode = match mode_state.mode {
        EditorMode::View3D => EditorMode::Edit2D,
        EditorMode::Edit2D => EditorMode::View3D,
    };

    let (show_2d, show_3d) = match mode_state.mode {
        EditorMode::Edit2D => (true, false),
        EditorMode::View3D => (false, true),
    };

    if let Ok((mut camera, mut visibility)) = cam2d.single_mut() {
        camera.is_active = show_2d;
        *visibility = if show_2d { Visibility::Visible } else { Visibility::Hidden };
    }
    if let Ok((mut camera, mut visibility)) = cam3d.single_mut() {
        camera.is_active = show_3d;
        *visibility = if show_3d { Visibility::Visible } else { Visibility::Hidden };
    }

    for mut v in &mut vis2d {
        *v = if show_2d { Visibility::Visible } else { Visibility::Hidden };
    }
    for mut v in &mut vis3d {
        *v = if show_3d { Visibility::Visible } else { Visibility::Hidden };
    }

    // CRITICAL: Clear drag state to prevent cross-mode ghost dragging
    *drag_state = crate::picking::drag::DragState::default();

    // Abort any in-flight drag (e.g. Tab pressed mid-drag): drop the marker
    // so the handle stops being written to the map / stays tinted.
    for entity in &dragged {
        if let Ok(mut entity_cmds) = commands.get_entity(entity) {
            entity_cmds.remove::<crate::picking::drag::BeingDragged>();
        }
    }
}

// ── 2D camera controls (orthographic pan + smooth zoom) ──────────

fn control_2d_camera(
    mut camera: Query<(&mut Transform, &mut Projection, &mut Camera2DZoom, &mut Camera2DEdgePan), With<Camera2D>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    windows: Query<&Window>,
    state: Res<PickingState>,
) {
    let Ok((mut transform, mut projection, mut zoom, mut edge_pan)) = camera.single_mut() else {
        return;
    };
    let Projection::Orthographic(ref mut ortho) = *projection else {
        return;
    };

    // Zoom: scroll adjusts the target, actual scale eases toward it
    if scroll.delta != Vec2::ZERO {
        let factor = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y * 0.1,
            MouseScrollUnit::Pixel => scroll.delta.y * 0.002,
        };
        zoom.target_scale = (zoom.target_scale * (1.0 - factor)).clamp(0.02, 5.0);
    }
    let ease = 1.0 - (-12.0 * time.delta_secs()).exp();
    ortho.scale += (zoom.target_scale - ortho.scale) * ease;

    // Pan: middle, right, or space + left drag (Figma-style)
    let is_panning = mouse.pressed(MouseButton::Middle)
        || mouse.pressed(MouseButton::Right)
        || (keys.pressed(KeyCode::Space) && mouse.pressed(MouseButton::Left));
    if is_panning && motion.delta != Vec2::ZERO {
        // 1 screen pixel = `ortho.scale` world units (ScalingMode::WindowSize),
        // so use scale directly for 1:1 grab feel (content follows the cursor).
        transform.translation.x += motion.delta.x * ortho.scale;
        transform.translation.z += motion.delta.y * ortho.scale;
    }

    // Edge panning: cursor near a window edge auto-pans that way. Sign-convention
    // is grab-style, consistent with the drag pan above: it continues the pan
    // gesture (camera moves opposite the content, content follows the cursor).
    const EDGE_MARGIN: f32 = 60.0;
    let Ok(window) = windows.single() else {
        return;
    };
    let window_size = window.size();
    let cursor = state.cursor_pos;

    let mut edge_delta = Vec2::ZERO;
    if cursor.x < EDGE_MARGIN {
        edge_delta.x -= 1.0;
    } else if cursor.x > window_size.x - EDGE_MARGIN {
        edge_delta.x += 1.0;
    }
    if cursor.y < EDGE_MARGIN {
        edge_delta.y -= 1.0;
    } else if cursor.y > window_size.y - EDGE_MARGIN {
        edge_delta.y += 1.0;
    }

    if edge_delta != Vec2::ZERO {
        edge_pan.velocity = edge_delta;
        let dt = time.delta_secs();
        let edge_speed = 400.0 * ortho.scale;
        transform.translation.x += edge_pan.velocity.x * edge_speed * dt;
        transform.translation.z += edge_pan.velocity.y * edge_speed * dt;
    } else {
        edge_pan.velocity = Vec2::ZERO;
    }
}

// ── One-shot 2D camera centering on the map's bounding box ───────

fn center_2d_camera_on_map(
    mut camera: Query<(&mut Transform, &mut Projection, &mut Camera2DZoom), With<Camera2D>>,
    windows: Query<&Window>,
    map: Res<Map>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    if map.vertices.is_empty() {
        return;
    }

    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for &v in &map.vertices {
        min = min.min(v);
        max = max.max(v);
    }

    let center = (min + max) * 0.5;
    let size = (max - min).max(Vec2::splat(1.0));

    let Ok((mut transform, mut projection, mut zoom)) = camera.single_mut() else {
        return;
    };
    transform.translation.x = center.x;
    transform.translation.z = center.y;

    let Projection::Orthographic(ref mut ortho) = *projection else {
        return;
    };
    // With ScalingMode::WindowSize the visible world height is scale * window_height.
    let viewport_height = windows.single().map(|w| w.height()).unwrap_or(1080.0);
    let padding = 1.2;
    let target = (size.x.max(size.y) * padding / viewport_height).clamp(0.02, 5.0);
    ortho.scale = target;
    zoom.target_scale = target;

    *done = true;
}