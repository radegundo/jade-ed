use bevy::prelude::*;
use crate::scene::ScenePlugin;
use crate::ui::UiPlugin;
use crate::viewport::ViewportPlugin;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ViewportPlugin, ScenePlugin, UiPlugin));
    }
}
