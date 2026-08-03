use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use crate::mode::{EditorMode, ModeState};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(),))
            .add_systems(EguiPrimaryContextPass, editor_ui);
    }
}

fn editor_ui(mut contexts: EguiContexts, mode: Res<ModeState>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Mode").show(ctx, |ui| {
        let label = match mode.mode {
            EditorMode::View3D => "3D View (Tab for 2D)",
            EditorMode::Edit2D => "2D Edit (Tab for 3D)",
        };
        ui.label(label);
    });
}
