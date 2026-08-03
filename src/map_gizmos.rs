use bevy::prelude::*;
use crate::map::Map;
use crate::mode::{in_mode, EditorMode};

pub struct MapGizmosPlugin;

impl Plugin for MapGizmosPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, draw_map_gizmos.run_if(in_mode(EditorMode::Edit2D)));
    }
}

/// Draw wall lines and vertex markers in 2D mode. These are pure gizmos drawn
/// from Map data every frame — no entities involved.
fn draw_map_gizmos(mut gizmos: Gizmos, map: Res<Map>) {
    // Walls: solid = gray, portal (has back side) = blue
    for sector in &map.sectors {
        for wall in &sector.walls {
            let start = *wall.start(&map.vertices);
            let end = *wall.end(&map.vertices);
            let color = if wall.back_side_def.is_some() {
                Color::srgb(0.2, 0.6, 1.0)
            } else {
                Color::srgb(0.5, 0.5, 0.5)
            };
            // Slightly above ground to avoid z-fighting with the grid
            gizmos.line(
                Vec3::new(start.x, 0.01, start.y),
                Vec3::new(end.x, 0.01, end.y),
                color,
            );
        }
    }

    // Vertex crosses (backup markers under the handle spheres)
    for pos in &map.vertices {
        let color = Color::srgb(0.2, 1.0, 0.3);
        let s = 0.3;
        let p = Vec3::new(pos.x, 0.02, pos.y);
        gizmos.line(p + Vec3::new(-s, 0.0, -s), p + Vec3::new(s, 0.0, s), color);
        gizmos.line(p + Vec3::new(-s, 0.0, s), p + Vec3::new(s, 0.0, -s), color);
    }
}
