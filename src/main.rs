use bevy::{ prelude::*, window::{ PresentMode, WindowResolution } };
use bevy::dev_tools::infinite_grid::InfiniteGridPlugin;

use bevy_egui::{ egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass };

pub mod viewport;

use crate::viewport::camera::CameraPlugin;
use viewport::grid::GridPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "My Bevy App".to_string(),
                    resolution: WindowResolution::new(1920, 1080),
                    present_mode: PresentMode::AutoVsync,
                    resizable: false,
                    ..default()
                }),
                ..default()
            })
        )
        .add_plugins(EditorPlugin)
        .add_plugins(EguiPlugin::default())
        .init_resource::<WidgetDemoState>()
        .add_systems(EguiPrimaryContextPass, (ui_example_system, widget_gallery).chain())
        .run();
}

struct EditorPlugin;
impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((InfiniteGridPlugin, CameraPlugin, GridPlugin)).add_systems(
            Startup,
            setup_scene
        );
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh<>>>,
    mut materials: ResMut<Assets<StandardMaterial<>>>
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.7, 0.7),
                perceptual_roughness: 0.85,
                ..default()
            })
        ),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::IDENTITY.looking_to(Vec3::new(-1.0, -2.0, -1.0).normalize(), Vec3::Y),
    ));
}

fn ui_example_system(
    mut contexts: EguiContexts,
    mut slider: Local<f32>,
    mut selected: Local<usize>,
    mut color: Local<egui::Color32>
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("Hello").show(ctx, |ui| {
        ui.label("world");
        ui.add(egui::Slider::new(&mut *slider, 0.0..=100.0).text("Health"));

        if ui.button("AAA").clicked() {
            println!("AAAA");
        }

        ui.label(egui::RichText::new("Important!").color(egui::Color32::RED).size(24.0).strong());
        ui.add_enabled(false, egui::Button::new("Can't click"));

        // Single line
        let mut name = "Player 1".to_string();
        ui.text_edit_singleline(&mut name);

        // Multi-line
        let mut bio = "Once upon a time...".to_string();
        ui.text_edit_multiline(&mut bio);

        // Password
        let mut password = "".to_string();
        ui.add(egui::TextEdit::singleline(&mut password).password(true));

        ui.radio_value(&mut *selected, 0, "Easy");
        ui.radio_value(&mut *selected, 1, "Medium");
        ui.radio_value(&mut *selected, 2, "Hard");

        let health_percent = 0.75;
        ui.add(egui::ProgressBar::new(health_percent).text("Health").fill(egui::Color32::GREEN));

        ui.color_edit_button_srgba(&mut color);

        ui.label("Above");
        ui.separator();
        ui.label("Below");

        let mut advanced = false;

        let mut debug_level = 3;

        ui.collapsing("Advanced Settings", |ui| {
            ui.checkbox(&mut advanced, "Enable debug mode");
            ui.add(egui::Slider::new(&mut debug_level, 0..=5));
        });
    });
    Ok(())
}

#[derive(Resource, Default)]
struct WidgetDemoState {
    name: String,
    bio: String,
    age: u32,
    volume: f32,
    is_subscribed: bool,
    difficulty: u32,
    color: egui::Color32,
    selected_item: usize,
}

fn widget_gallery(mut contexts: EguiContexts, mut state: ResMut<WidgetDemoState>) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Widget Gallery").show(ctx, |ui| {
        // Text inputs
        ui.heading("Text");
        ui.text_edit_singleline(&mut state.name);
        ui.text_edit_multiline(&mut state.bio);

        ui.separator();

        // Numeric inputs
        ui.heading("Numbers");
        ui.add(egui::Slider::new(&mut state.age, 0..=120).text("Age"));
        ui.add(egui::DragValue::new(&mut state.age).prefix("Age: "));
        ui.add(egui::Slider::new(&mut state.volume, 0.0..=1.0).text("Volume"));

        ui.separator();

        // Boolean inputs
        ui.heading("Toggles");
        ui.checkbox(&mut state.is_subscribed, "Subscribe to newsletter");

        ui.separator();

        // Selection
        ui.heading("Selection");
        ui.radio_value(&mut state.difficulty, 0, "Easy");
        ui.radio_value(&mut state.difficulty, 1, "Medium");
        ui.radio_value(&mut state.difficulty, 2, "Hard");

        let items = ["Apple", "Banana", "Cherry"];
        egui::ComboBox
            ::from_label("Fruit")
            .selected_text(items[state.selected_item])
            .show_ui(ui, |ui| {
                for (i, item) in items.iter().enumerate() {
                    ui.selectable_value(&mut state.selected_item, i, *item);
                }
            });

        ui.separator();

        // Color
        ui.heading("Color");
        ui.color_edit_button_srgba(&mut state.color);

        ui.separator();

        // Action
        ui.heading("Actions");
        if ui.button("Submit").clicked() {
            println!("Submitted: {} (age {})", state.name, state.age);
        }
    });

    Ok(())
}
