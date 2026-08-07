use bevy::prelude::*;
use crate::height_handles::HeightHandlesPlugin;
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
use crate::save::SavePlugin;
use crate::textures::TexturePlugin;

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
            HeightHandlesPlugin,
            ToolsPlugin,
            SavePlugin,
            TexturePlugin,
        ));
    }
}
