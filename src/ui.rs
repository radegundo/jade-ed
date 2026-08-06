use bevy::prelude::*;
use bevy_egui::{ egui, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass };
use crate::map::Map;
use crate::mode::{ EditorMode, ModeState };
use crate::save::{ map_path, SaveState };
use crate::tools::{ EditorTool, Selection, ToolState };

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(),));
        // Contexts are moved between cameras by `ModePlugin::toggle_mode`, so
        // don't let bevy_egui auto-attach a primary context to the first camera.
        app.world_mut().resource_mut::<EguiGlobalSettings>().auto_create_primary_context = false;
        app.add_systems(EguiPrimaryContextPass, editor_ui);
    }
}

fn editor_ui(
    mut contexts: EguiContexts,
    mode: Res<ModeState>,
    mut tool_state: ResMut<ToolState>,
    mut map: ResMut<Map>,
    selection: Res<Selection>,
    mut save_state: ResMut<SaveState>
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Editor").show(ctx, |ui| {
        let mode_label = match mode.mode {
            EditorMode::View3D => "3D View (Tab for 2D)",
            EditorMode::Edit2D => "2D Edit (Tab for 3D)",
        };
        ui.label(mode_label);
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Tool:");
            ui.radio_value(&mut tool_state.tool, EditorTool::Select, "Select");
            ui.radio_value(&mut tool_state.tool, EditorTool::DrawSector, "Draw Sector");
            ui.radio_value(&mut tool_state.tool, EditorTool::DrawWall, "Draw Wall");
            ui.radio_value(&mut tool_state.tool, EditorTool::PlaceObstacle, "Obstacle");
        });

        let hint = match tool_state.tool {
            EditorTool::Select =>
                "Click to select · drag to move · Delete to remove · drag height handles / [ ] to adjust heights",
            EditorTool::DrawSector =>
                "Click to place vertices · Right-click/Enter to close · Esc to cancel",
            EditorTool::DrawWall => "Click start then end · Right-click/Esc to cancel",
            EditorTool::PlaceObstacle => "Drag a rectangle inside a sector",
        };
        ui.label(hint);

        if
            mode.mode == EditorMode::View3D &&
            matches!(
                tool_state.tool,
                EditorTool::DrawSector | EditorTool::DrawWall | EditorTool::PlaceObstacle
            )
        {
            ui.colored_label(egui::Color32::YELLOW, "Press Tab to switch to 2D mode to draw");
        }

        ui.separator();
        if let Some(idx) = selection.sector && idx < map.sectors.len() {
            let id = map.sectors[idx].id;
            ui.label(format!("Sector {id} heights"));
            let s = &mut map.sectors[idx];
            ui.horizontal(|ui| {
                ui.label("Floor:");
                ui.add(egui::DragValue::new(&mut s.floor_height).speed(0.25));
            });
            ui.horizontal(|ui| {
                ui.label("Ceiling:");
                ui.add(egui::DragValue::new(&mut s.ceiling_height).speed(0.25));
            });
            // Keep floor <= ceiling after direct numeric edits.
            if s.floor_height > s.ceiling_height {
                s.floor_height = s.ceiling_height;
            }
            if s.ceiling_height < s.floor_height {
                s.ceiling_height = s.floor_height;
            }
        }
        if
            let Some((sid, oid)) = selection.obstacle &&
            let Some(si) = map.sectors.iter().position(|s| s.id == sid) &&
            let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == oid)
        {
            ui.label(format!("Obstacle {oid} heights"));
            let o = &mut map.sectors[si].obstacles[oi];
            ui.horizontal(|ui| {
                ui.label("Bottom:");
                ui.add(egui::DragValue::new(&mut o.bottom).speed(0.25));
            });
            ui.horizontal(|ui| {
                ui.label("Top:");
                ui.add(egui::DragValue::new(&mut o.top).speed(0.25));
            });
            if o.bottom > o.top {
                o.bottom = o.top;
            }
            if o.top < o.bottom {
                o.top = o.bottom;
            }
        }
        if mode.mode == EditorMode::View3D {
            ui.colored_label(
                egui::Color32::GRAY,
                "In 3D, select a sector/obstacle, then drag the green/blue height handles (or the obstacle body)."
            );
        }

        if let Some(message) = &tool_state.message {
            ui.colored_label(egui::Color32::LIGHT_BLUE, message);
        }

        ui.horizontal(|ui| {
            ui.label("Map name:");
            ui.add(
                egui::TextEdit::singleline(&mut save_state.map_name)
                    .hint_text("unnamed")
                    .desired_width(120.0),
            );
            if ui.button("Save").clicked() {
                save_state.save_pending = true;
            }
            if ui.button("Load").clicked() {
                save_state.load_pending = true;
            }
        });
        ui.label(
            egui::RichText::new(map_path(&save_state.map_name))
                .small()
                .color(egui::Color32::GRAY),
        );
    });
}
