use bevy::prelude::*;
use crate::map::MapPlugin;
use crate::map_gizmos::MapGizmosPlugin;
use crate::map_handles::MapHandlesPlugin;
use crate::map_preview::MapPreviewPlugin;
use crate::mode::ModePlugin;
use crate::picking::OwnPickingPlugin;
use crate::scene::ScenePlugin;
use crate::tools::ToolsPlugin;
use crate::ui::UiPlugin;
use crate::viewport::ViewportPlugin;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ViewportPlugin,
            ScenePlugin,
            UiPlugin,
            OwnPickingPlugin,
            ModePlugin,
            MapPlugin,
            MapHandlesPlugin,
            MapGizmosPlugin,
            MapPreviewPlugin,
            ToolsPlugin,
        ));
    }
}
