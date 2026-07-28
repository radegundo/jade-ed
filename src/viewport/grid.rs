use bevy::{ dev_tools::infinite_grid::{ InfiniteGrid, InfiniteGridSettings }, prelude::* };
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
