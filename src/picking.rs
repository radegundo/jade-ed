use bevy::prelude::*;

#[derive(Component)]
pub struct Hovered;
#[derive(Component)]
pub struct Selected;
#[derive(Component)]
struct OriginalMaterial(Handle<StandardMaterial>);
#[derive(Component)]
struct HasHoverObservers;

pub struct OwnPickingPlugin;

impl Plugin for OwnPickingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin).add_systems(Update, (
            attach_hover_observers,
            update_visual_state,
        ));
    }
}

/// Attach observers to any mesh without them.
fn attach_hover_observers(
    mut commands: Commands,
    query: Query<Entity, (With<Mesh3d>, Without<HasHoverObservers>)>
) {
    for entity in &query {
        commands
            .entity(entity)
            .observe(on_hover)
            .observe(on_unhover)
            .observe(on_click)
            .insert(HasHoverObservers);
    }
}

/// Observers just toggle components — no material logic here.
fn on_hover(trigger: On<Pointer<Over>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).insert(Hovered);
}

fn on_unhover(trigger: On<Pointer<Out>>, mut commands: Commands) {
    commands.entity(trigger.event_target()).remove::<Hovered>();
}

fn on_click(
    trigger: On<Pointer<Click>>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>
) {
    // Deselect all others
    for e in &selected {
        commands.entity(e).remove::<Selected>();
    }
    commands.entity(trigger.event_target()).insert(Selected);
}

/// Central system handles all material updates based on component state.
fn update_visual_state(
    mut commands: Commands,
    entities: Query<(Entity, Option<&Hovered>, Option<&Selected>), With<Mesh3d>>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut originals: Query<&mut OriginalMaterial>
) {
    for (entity, hover, select) in &entities {
        let has_hover = hover.is_some();
        let has_select = select.is_some();

        // Skip if state hasn't changed (optimization)
        // ... (can use Changed filters for this)

        let original = if let Ok(orig) = originals.get(entity) {
            orig.0.clone()
        } else if let Ok(mesh_mat) = mesh_materials.get(entity) {
            let h = mesh_mat.0.clone();
            commands.entity(entity).insert(OriginalMaterial(h.clone()));
            h
        } else {
            continue;
        };

        let tint = match (has_select, has_hover) {
            (true, true) => Some(Color::srgb(1.0, 0.7, 0.3)),
            (true, false) => Some(Color::srgb(1.0, 0.5, 0.1)),
            (false, true) => Some(Color::srgb(0.5, 0.6, 1.0)),
            (false, false) => None,
        };

        if let Some(color) = tint {
            if let Some(mat) = materials.get(&original) {
                let mut t = mat.clone();
                t.base_color = blend(mat.base_color, color, 0.4);
                commands.entity(entity).insert(MeshMaterial3d(materials.add(t)));
            }
        } else {
            commands.entity(entity).insert(MeshMaterial3d(original)).remove::<OriginalMaterial>();
        }
    }
}

fn blend(a: Color, b: Color, f: f32) -> Color {
    let a = a.to_linear();
    let b = b.to_linear();
    Color::LinearRgba(
        LinearRgba::new(
            a.red + (b.red - a.red) * f,
            a.green + (b.green - a.green) * f,
            a.blue + (b.blue - a.blue) * f,
            a.alpha
        )
    )
}
