use bevy::prelude::*;
use bevy_egui::{ egui, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass };
use crate::map::{ Map, SideDefTextures };
use crate::mode::{ EditorMode, ModeState };
use crate::save::{ map_path, SaveState };
use crate::textures::{ load_repeat, TextureCatalog, ThumbnailCache };
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

//------------------------------TEXTURE BROWSER STATE------------------

/// One paintable surface on a selected sector / wall / obstacle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SurfaceSlot {
    Floor,
    Ceiling,
    WallUpper,
    WallMiddle,
    WallLower,
    ObstacleSide,
    ObstacleTop,
    ObstacleBottom,
}

fn slot_label(slot: SurfaceSlot) -> &'static str {
    match slot {
        SurfaceSlot::Floor => "Floor",
        SurfaceSlot::Ceiling => "Ceiling",
        SurfaceSlot::WallUpper => "Wall upper",
        SurfaceSlot::WallMiddle => "Wall middle",
        SurfaceSlot::WallLower => "Wall lower",
        SurfaceSlot::ObstacleSide => "Obstacle side",
        SurfaceSlot::ObstacleTop => "Obstacle top",
        SurfaceSlot::ObstacleBottom => "Obstacle bottom",
    }
}

/// Persistent browser state (category, filter, active paint target).
#[derive(Default)]
struct TextureBrowserState {
    category: Option<String>,
    filter: String,
    active_slot: Option<SurfaceSlot>,
}

const THUMB_SIZE: f32 = 72.0;

/// Which slots the current selection offers.
fn available_slots(selection: &Selection, map: &Map) -> Vec<SurfaceSlot> {
    let mut slots = Vec::new();
    if let Some(si) = selection.sector
        && si < map.sectors.len()
    {
        slots.push(SurfaceSlot::Floor);
        slots.push(SurfaceSlot::Ceiling);
    }
    if let Some((sid, wid)) = selection.wall
        && let Some(si) = map.sectors.iter().position(|s| s.id == sid)
        && wid < map.sectors[si].walls.len()
    {
        if map.sectors[si].walls[wid].back_side_def.is_some() {
            slots.push(SurfaceSlot::WallUpper);
            slots.push(SurfaceSlot::WallLower);
        } else {
            slots.push(SurfaceSlot::WallMiddle);
        }
    }
    if let Some((sid, oid)) = selection.obstacle
        && let Some(si) = map.sectors.iter().position(|s| s.id == sid)
        && map.sectors[si].obstacles.iter().any(|o| o.id == oid)
    {
        slots.push(SurfaceSlot::ObstacleSide);
        slots.push(SurfaceSlot::ObstacleTop);
        slots.push(SurfaceSlot::ObstacleBottom);
    }
    slots
}

/// The texture currently assigned to a slot, if its selection is still valid.
fn slot_handle<'a>(
    map: &'a Map,
    selection: &Selection,
    slot: SurfaceSlot,
) -> Option<&'a Handle<Image>> {
    match slot {
        SurfaceSlot::Floor | SurfaceSlot::Ceiling => {
            let s = selection.sector.and_then(|si| map.sectors.get(si))?;
            Some(match slot {
                SurfaceSlot::Floor => &s.floor_texture,
                _ => &s.ceiling_texture,
            })
        }
        SurfaceSlot::WallUpper | SurfaceSlot::WallMiddle | SurfaceSlot::WallLower => {
            let (sid, wid) = selection.wall?;
            let si = map.sectors.iter().position(|s| s.id == sid)?;
            let wall = map.sectors.get(si)?.walls.get(wid)?;
            match slot {
                SurfaceSlot::WallUpper => wall.front_side_def.textures.upper.as_ref(),
                SurfaceSlot::WallMiddle => wall.front_side_def.textures.middle.as_ref(),
                SurfaceSlot::WallLower => wall.front_side_def.textures.lower.as_ref(),
                _ => unreachable!(),
            }
        }
        SurfaceSlot::ObstacleSide | SurfaceSlot::ObstacleTop | SurfaceSlot::ObstacleBottom => {
            let (sid, oid) = selection.obstacle?;
            let si = map.sectors.iter().position(|s| s.id == sid)?;
            let obs = map.sectors.get(si)?.obstacles.iter().find(|o| o.id == oid)?;
            Some(match slot {
                SurfaceSlot::ObstacleSide => &obs.side_texture,
                SurfaceSlot::ObstacleTop => &obs.top_texture,
                SurfaceSlot::ObstacleBottom => &obs.bottom_texture,
                _ => unreachable!(),
            })
        }
    }
}

/// Assign a texture to a slot. Portal walls get both sides mirrored so the
/// preview and the far side stay consistent.
fn apply_slot_texture(
    map: &mut Map,
    selection: &Selection,
    slot: SurfaceSlot,
    handle: Handle<Image>,
) {
    match slot {
        SurfaceSlot::Floor => {
            if let Some(si) = selection.sector
                && si < map.sectors.len()
            {
                map.sectors[si].floor_texture = handle;
            }
        }
        SurfaceSlot::Ceiling => {
            if let Some(si) = selection.sector
                && si < map.sectors.len()
            {
                map.sectors[si].ceiling_texture = handle;
            }
        }
        SurfaceSlot::WallUpper | SurfaceSlot::WallMiddle | SurfaceSlot::WallLower => {
            if let Some((sid, wid)) = selection.wall
                && let Some(si) = map.sectors.iter().position(|s| s.id == sid)
                && wid < map.sectors[si].walls.len()
            {
                let wall = &mut map.sectors[si].walls[wid];
                set_wall_slot(slot, &mut wall.front_side_def.textures, &handle);
                if let Some(back) = &mut wall.back_side_def {
                    set_wall_slot(slot, &mut back.textures, &handle);
                }
            }
        }
        SurfaceSlot::ObstacleSide | SurfaceSlot::ObstacleTop | SurfaceSlot::ObstacleBottom => {
            if let Some((sid, oid)) = selection.obstacle
                && let Some(si) = map.sectors.iter().position(|s| s.id == sid)
                && let Some(oi) = map.sectors[si].obstacles.iter().position(|o| o.id == oid)
            {
                let obs = &mut map.sectors[si].obstacles[oi];
                match slot {
                    SurfaceSlot::ObstacleSide => obs.side_texture = handle,
                    SurfaceSlot::ObstacleTop => obs.top_texture = handle,
                    SurfaceSlot::ObstacleBottom => obs.bottom_texture = handle,
                    _ => unreachable!(),
                }
            }
        }
    }
}

fn set_wall_slot(slot: SurfaceSlot, textures: &mut SideDefTextures, handle: &Handle<Image>) {
    match slot {
        SurfaceSlot::WallUpper => textures.upper = Some(handle.clone()),
        SurfaceSlot::WallMiddle => textures.middle = Some(handle.clone()),
        SurfaceSlot::WallLower => textures.lower = Some(handle.clone()),
        _ => {}
    }
}

fn handle_path(server: &AssetServer, handle: &Handle<Image>) -> String {
    server
        .get_path(handle.id())
        .map(|p| p.to_string())
        .unwrap_or_else(|| "unloaded".to_string())
}

fn filter_matches(path: &str, filter_lower: &str) -> bool {
    if filter_lower.is_empty() {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    name.contains(filter_lower)
}

//------------------------------EDITOR UI-------------------------------

fn editor_ui(
    mut contexts: EguiContexts,
    mode: Res<ModeState>,
    mut tool_state: ResMut<ToolState>,
    mut map: ResMut<Map>,
    selection: Res<Selection>,
    mut save_state: ResMut<SaveState>,
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    catalog: Res<TextureCatalog>,
    mut thumb_cache: ResMut<ThumbnailCache>,
    mut browser: Local<TextureBrowserState>,
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
        if
            let Some((sid, wid)) = selection.wall &&
            let Some(si) = map.sectors.iter().position(|s| s.id == sid) &&
            wid < map.sectors[si].walls.len()
        {
            let is_portal = map.sectors[si].walls[wid].back_side_def.is_some();
            ui.label(format!(
                "Wall {wid} in sector {sid}{}",
                if is_portal { " (portal)" } else { "" }
            ));
            for slot in [
                SurfaceSlot::WallUpper,
                SurfaceSlot::WallMiddle,
                SurfaceSlot::WallLower,
            ] {
                let short = slot_handle(&map, &selection, slot)
                    .map(|h| {
                        let p = handle_path(&asset_server, h);
                        p.rsplit('/').next().unwrap_or(&p).to_string()
                    })
                    .unwrap_or_else(|| "—".to_string());
                ui.label(format!("  {}: {short}", slot_label(slot)));
            }
        }
        if mode.mode == EditorMode::View3D {
            ui.colored_label(
                egui::Color32::GRAY,
                "In 3D, click a sector/obstacle/wall to select it, then drag the green/blue height handles (or the obstacle body) or paint it from the Textures window."
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

    texture_browser_ui(ctx, &mut map, &selection, &mut tool_state, &asset_server, &images, &catalog, &mut thumb_cache, &mut browser);
}

//------------------------------TEXTURE WINDOW--------------------------

fn texture_browser_ui(
    ctx: &egui::Context,
    map: &mut Map,
    selection: &Selection,
    tool_state: &mut ToolState,
    asset_server: &AssetServer,
    images: &Assets<Image>,
    catalog: &TextureCatalog,
    thumb_cache: &mut ThumbnailCache,
    browser: &mut TextureBrowserState,
) {
    egui::Window::new("Textures")
        .default_width(360.0)
        .show(ctx, |ui| {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));

            // ── Paint target slots ──────────────────────────────
            let slots = available_slots(selection, map);
            if slots.is_empty() {
                ui.label("Select a sector, wall or obstacle to paint it.");
            } else {
                if browser
                    .active_slot
                    .map(|s| !slots.contains(&s))
                    .unwrap_or(true)
                {
                    browser.active_slot = slots.first().copied();
                }
                ui.label("Paint target:");
                for slot in slots {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut browser.active_slot, Some(slot), slot_label(slot));
                        if let Some(handle) = slot_handle(map, selection, slot) {
                            let path = handle_path(asset_server, handle);
                            if let Some(id) = thumb_cache.ensure(ui, asset_server, images, &path) {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(22.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().image(id, rect, uv, egui::Color32::WHITE);
                            }
                            let short = path.rsplit('/').next().unwrap_or(&path);
                            ui.label(egui::RichText::new(short).small().monospace());
                        }
                    });
                }
            }
            ui.separator();

            // ── Browser controls ────────────────────────────────
            egui::ComboBox::from_id_salt("tex_category")
                .selected_text(browser.category.clone().unwrap_or_else(|| "All".to_string()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut browser.category, None, "All");
                    for cat in &catalog.categories {
                        ui.selectable_value(&mut browser.category, Some(cat.clone()), cat);
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(
                    egui::TextEdit::singleline(&mut browser.filter)
                        .hint_text("e.g. WAL")
                        .desired_width(140.0),
                );
            });

            let filter_lower = browser.filter.trim().to_lowercase();
            let paths: Vec<&String> = match &browser.category {
                Some(cat) => catalog
                    .category_paths(cat)
                    .map(|ps| ps.iter().filter(|p| filter_matches(p, &filter_lower)).collect())
                    .unwrap_or_default(),
                None => catalog
                    .categories
                    .iter()
                    .flat_map(|c| catalog.category_paths(c).unwrap())
                    .filter(|p| filter_matches(p, &filter_lower))
                    .collect(),
            };
            if paths.is_empty() {
                ui.label("No textures match.");
                return;
            }

            // ── Virtualized thumbnail grid ──────────────────────
            let spacing_x = ui.spacing().item_spacing.x;
            let per_row =
                ((ui.available_width() + spacing_x) / (THUMB_SIZE + spacing_x))
                    .floor()
                    .max(1.0) as usize;
            let total_rows = paths.len().div_ceil(per_row);
            let mut pending: Option<String> = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(
                    ui,
                    THUMB_SIZE,
                    total_rows,
                    |ui, range| {
                        for row in range {
                            let start = row * per_row;
                            let end = (start + per_row).min(paths.len());
                            ui.horizontal(|ui| {
                                for path in &paths[start..end] {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::Vec2::splat(THUMB_SIZE),
                                        egui::Sense::click(),
                                    );
                                    if let Some(id) = thumb_cache.ensure(ui, asset_server, images, path) {
                                        ui.painter().image(id, rect, uv, egui::Color32::WHITE);
                                    } else {
                                        ui.painter()
                                            .rect_filled(rect, 4.0, egui::Color32::from_gray(35));
                                    }
                                    response.clone().on_hover_text(path.as_str());
                                    if response.clicked() {
                                        pending = Some(path.to_string());
                                    }
                                }
                            });
                        }
                    },
                );

            // ── Apply the picked texture to the active slot ──────
            if let Some(path) = pending {
                if let Some(slot) = browser.active_slot {
                    let handle = load_repeat(asset_server, &path);
                    apply_slot_texture(map, selection, slot, handle);
                    tool_state.message =
                        Some(format!("Painted {} on {}", path, slot_label(slot)));
                } else {
                    tool_state.message =
                        Some("Select a sector, wall or obstacle first".to_string());
                }
            }

            // Evict stale thumbnails now that every visible cell has been drawn.
            thumb_cache.end_frame();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapAssets;

    fn assets() -> MapAssets {
        MapAssets {
            wall: Handle::default(),
            floor: Handle::default(),
            ceiling: Handle::default(),
            obstacle_side: Handle::default(),
            obstacle_top: Handle::default(),
            obstacle_bottom: Handle::default(),
        }
    }

    #[test]
    fn apply_wall_texture_updates_the_selected_wall() {
        let mut map = Map::default();
        map.add_sector_from_polygon(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            &assets(),
        )
        .unwrap();

        let new_tex = Handle::default();
        let sid = map.sectors[0].id;
        apply_slot_texture(
            &mut map,
            &Selection { wall: Some((sid, 2)), ..default() },
            SurfaceSlot::WallMiddle,
            new_tex.clone(),
        );

        let wall = &map.sectors[0].walls[2];
        assert_eq!(wall.front_side_def.textures.middle.as_ref(), Some(&new_tex));
    }

    #[test]
    fn apply_wall_texture_mirrors_to_portal_back_side() {
        // Two adjacent sectors share an edge, forming a portal on the lower-id
        // sector's wall. Painting the front must mirror onto the owning wall's
        // back_side_def so both sides of the shared wall stay consistent.
        let mut map = Map::default();
        map.add_sector_from_polygon(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            &assets(),
        )
        .unwrap();
        map.add_sector_from_polygon(
            &[
                Vec2::new(10.0, 0.0),
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 10.0),
                Vec2::new(10.0, 10.0),
            ],
            &assets(),
        )
        .unwrap();

        // The shared edge is a portal on the lower-id sector's wall.
        let mut portal = None;
        for sector in &map.sectors {
            for (wi, wall) in sector.walls.iter().enumerate() {
                if wall.back_side_def.is_some() {
                    portal = Some((sector.id, wi));
                }
            }
        }
        let (sid, wi) = portal.expect("adjacent sectors must share a portal");
        let si = map.sectors.iter().position(|s| s.id == sid).unwrap();

        let new_tex = Handle::default();
        apply_slot_texture(
            &mut map,
            &Selection { wall: Some((sid, wi)), ..default() },
            SurfaceSlot::WallMiddle,
            new_tex.clone(),
        );

        let wall = &map.sectors[si].walls[wi];
        assert_eq!(wall.front_side_def.textures.middle.as_ref(), Some(&new_tex));
        assert_eq!(
            wall.back_side_def.as_ref().unwrap().textures.middle.as_ref(),
            Some(&new_tex)
        );
    }
}
