use std::any::TypeId;

use bevy::{
    asset::{HandleId, ReflectAsset},
    prelude::*,
    render::camera::{CameraProjection, Viewport},
    window::PrimaryWindow,
};
use bevy_inspector_egui::{
    bevy_egui::{self, EguiContext, EguiSet},
    bevy_inspector::{
        self,
        hierarchy::{hierarchy_ui, SelectedEntities},
        ui_for_entities_shared_components, ui_for_entity_with_children,
    },
    DefaultInspectorConfigPlugin,
};
use bevy_reflect::TypeRegistry;
use egui_dock::{DockArea, NodeIndex, Style, Tree};
use egui_gizmo::{Gizmo, GizmoMode, GizmoOrientation};
use input::EditorInputPlugin;

pub mod input;

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EditorResource>()
            .register_type::<MainCamera>()
            .add_plugins(DefaultInspectorConfigPlugin)
            .add_plugins(bevy_egui::EguiPlugin)
            .add_plugins(EditorInputPlugin)
            .insert_resource(UiState::new())
            .add_systems(PostStartup, setup)
            .add_systems(
                PostUpdate,
                show_ui
                    .before(EguiSet::ProcessOutput)
                    .before(bevy::transform::TransformSystem::TransformPropagate),
            )
            .add_systems(PostUpdate, set_camera_viewport.after(show_ui))
            .add_systems(Update, set_gizmo_mode);
    }
}

#[derive(Debug, Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct EditorResource(pub bool);

#[derive(Debug, Component, Default, Reflect)]
#[reflect(Component)]
pub struct MainCamera;

fn setup(
    mut commands: Commands,
    query: Query<Entity, With<Camera>>,
    mut app_exit_events: EventWriter<bevy::app::AppExit>,
) {
    let Ok(camera) = query.get_single() else {
        error!("No Camera found, change that");
        app_exit_events.send(bevy::app::AppExit);
        return;
    };

    commands.entity(camera).insert(MainCamera);
    commands.insert_resource(EditorResource(false));
}

fn show_ui(world: &mut World) {
    if !world.get_resource::<EditorResource>().unwrap().0 {
        return;
    }

    let Ok(egui_context) = world.query_filtered::<&mut EguiContext, With<PrimaryWindow>>().get_single(world) else {
        return;
    };
    let mut egui_context = egui_context.clone();

    world.resource_scope::<UiState, _>(|world, mut ui_state| {
        ui_state.ui(world, egui_context.get_mut());
    })
}

fn set_camera_viewport(
    ui_state: Res<UiState>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    egui_settings: Res<bevy_egui::EguiSettings>,
    mut cameras: Query<&mut Camera, With<MainCamera>>,
    ed: Res<EditorResource>,
) {
    let Ok(window) = primary_window.get_single() else {
        return;
    };

    let scale_factor = window.scale_factor() * egui_settings.scale_factor;

    let viewport_pos = ui_state.viewport_rect.left_top().to_vec2() * scale_factor as f32;
    let viewport_size = ui_state.viewport_rect.size() * scale_factor as f32;

    if let Ok(mut cam) = cameras.get_single_mut() {
        if ed.0 {
            cam.viewport = Some(Viewport {
                physical_position: UVec2::new(viewport_pos.x as u32, viewport_pos.y as u32),
                physical_size: UVec2::new(viewport_size.x as u32, viewport_size.y as u32),
                depth: 0.0..1.0,
            });
        } else {
            cam.viewport = Some(Viewport {
                physical_position: UVec2 { x: 0, y: 0 },
                physical_size: UVec2::new(window.physical_width(), window.physical_height()),
                depth: 0.0..1.0,
            })
        }
    }
}

fn set_gizmo_mode(input: Res<Input<KeyCode>>, mut ui_state: ResMut<UiState>) {
    for (key, mode) in [
        (KeyCode::R, GizmoMode::Rotate),
        (KeyCode::T, GizmoMode::Translate),
        (KeyCode::S, GizmoMode::Scale),
    ] {
        if input.just_pressed(key) {
            ui_state.gizmo_mode = mode;
        }
    }
}

#[derive(Eq, PartialEq)]
enum InspectorSelection {
    Entities,
    Resource(TypeId, String),
    Asset(TypeId, String, HandleId),
}

#[derive(Resource)]
pub struct UiState {
    tree: Tree<EguiWindow>,
    viewport_rect: egui::Rect,
    selected_entities: SelectedEntities,
    selection: InspectorSelection,
    gizmo_mode: GizmoMode,
}

impl UiState {
    pub fn new() -> Self {
        let mut tree = Tree::new(vec![EguiWindow::GameView]);
        let [game, _inspector] =
            tree.split_right(NodeIndex::root(), 0.75, vec![EguiWindow::Inspector]);
        let [game, _hierarchy] = tree.split_left(game, 0.2, vec![EguiWindow::Hierarchy]);
        let [_game, _bottom] =
            tree.split_below(game, 0.8, vec![EguiWindow::Resources, EguiWindow::Assets]);

        Self {
            tree,
            selected_entities: SelectedEntities::default(),
            selection: InspectorSelection::Entities,
            viewport_rect: egui::Rect::NOTHING,
            gizmo_mode: GizmoMode::Translate,
        }
    }

    fn ui(&mut self, world: &mut World, ctx: &mut egui::Context) {
        let mut tab_viewer = TabViewer {
            world,
            viewport_rect: &mut self.viewport_rect,
            selected_entities: &mut self.selected_entities,
            selection: &mut self.selection,
            gizmo_mode: self.gizmo_mode,
        };
        DockArea::new(&mut self.tree)
            .style(Style::from_egui(ctx.style().as_ref()))
            .show(ctx, &mut tab_viewer);
    }
}

#[derive(Debug)]
enum EguiWindow {
    GameView,
    Hierarchy,
    Resources,
    Assets,
    Inspector,
}

struct TabViewer<'a> {
    world: &'a mut World,
    selected_entities: &'a mut SelectedEntities,
    selection: &'a mut InspectorSelection,
    viewport_rect: &'a mut egui::Rect,
    gizmo_mode: GizmoMode,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = EguiWindow;

    fn ui(&mut self, ui: &mut egui_dock::egui::Ui, window: &mut Self::Tab) {
        let type_registry = self.world.resource::<AppTypeRegistry>().0.clone();
        let type_registry = type_registry.read();

        match window {
            EguiWindow::GameView => {
                *self.viewport_rect = ui.clip_rect();

                draw_gizmo(ui, self.world, self.selected_entities, self.gizmo_mode);
            }
            EguiWindow::Hierarchy => {
                let selected = hierarchy_ui(self.world, ui, self.selected_entities);
                if selected {
                    *self.selection = InspectorSelection::Entities;
                }
            }
            EguiWindow::Resources => select_resource(ui, &type_registry, self.selection),
            EguiWindow::Assets => select_asset(ui, &type_registry, self.world, self.selection),
            EguiWindow::Inspector => match *self.selection {
                InspectorSelection::Entities => match self.selected_entities.as_slice() {
                    &[entity] => ui_for_entity_with_children(self.world, entity, ui),
                    entities => ui_for_entities_shared_components(self.world, entities, ui),
                },
                InspectorSelection::Resource(type_id, ref name) => {
                    ui.label(name);
                    bevy_inspector::by_type_id::ui_for_resource(
                        self.world,
                        type_id,
                        ui,
                        name,
                        &type_registry,
                    )
                }
                InspectorSelection::Asset(type_id, ref name, handle) => {
                    ui.label(name);
                    bevy_inspector::by_type_id::ui_for_asset(
                        self.world,
                        type_id,
                        handle,
                        ui,
                        &type_registry,
                    );
                }
            },
        }
    }

    fn title(&mut self, window: &mut Self::Tab) -> egui_dock::egui::WidgetText {
        format!("{window:?}").into()
    }

    fn clear_background(&self, window: &Self::Tab) -> bool {
        !matches!(window, EguiWindow::GameView)
    }
}

fn draw_gizmo(
    ui: &mut egui::Ui,
    world: &mut World,
    selected_entities: &SelectedEntities,
    gizmo_mode: GizmoMode,
) {
    let Ok((cam_transform, projection)) = world
        .query_filtered::<(&GlobalTransform, &Projection), With<MainCamera>>()
        .get_single(world)
    else {

        let Ok((cam_transform, projection)) = world.query_filtered::<(&GlobalTransform, &OrthographicProjection), With<MainCamera>>().get_single(world) else {
            return;
        };

        if selected_entities.len() != 1 {
            return;
        }
        let view_matrix = Mat4::from(cam_transform.affine().inverse());
        let projection_matrix = projection.get_projection_matrix();

        for selected in selected_entities.iter() {
            let Some(transform) = world.get::<Transform>(selected) else {
                continue;
            };
            let model_matrix = transform.compute_matrix();

            let Some(result) = Gizmo::new(selected)
                .model_matrix(model_matrix.to_cols_array_2d())
                .view_matrix(view_matrix.to_cols_array_2d())
                .projection_matrix(projection_matrix.to_cols_array_2d())
                .orientation(GizmoOrientation::Local)
                .mode(gizmo_mode)
                .interact(ui)
            else {
                continue;
            };

            let mut transform = world.get_mut::<Transform>(selected).unwrap();
            *transform = Transform {
                translation: Vec3::from(<[f32; 3]>::from(result.translation)),
                rotation: Quat::from_array(<[f32; 4]>::from(result.rotation)),
                scale: Vec3::from(<[f32; 3]>::from(result.scale)),
            };
        }
        return;
    };
    let view_matrix = Mat4::from(cam_transform.affine().inverse());
    let projection_matrix = projection.get_projection_matrix();

    if selected_entities.len() != 1 {
        return;
    }

    for selected in selected_entities.iter() {
        let Some(transform) = world.get::<Transform>(selected) else {
            continue;
        };
        let model_matrix = transform.compute_matrix();

        let Some(result) = Gizmo::new(selected)
            .model_matrix(model_matrix.to_cols_array_2d())
            .view_matrix(view_matrix.to_cols_array_2d())
            .projection_matrix(projection_matrix.to_cols_array_2d())
            .orientation(GizmoOrientation::Local)
            .mode(gizmo_mode)
            .interact(ui)
        else {
            continue;
        };

        let mut transform = world.get_mut::<Transform>(selected).unwrap();
        *transform = Transform {
            translation: Vec3::from(<[f32; 3]>::from(result.translation)),
            rotation: Quat::from_array(<[f32; 4]>::from(result.rotation)),
            scale: Vec3::from(<[f32; 3]>::from(result.scale)),
        };
    }
}

fn select_resource(
    ui: &mut egui::Ui,
    type_registry: &TypeRegistry,
    selection: &mut InspectorSelection,
) {
    let mut resources: Vec<_> = type_registry
        .iter()
        .filter(|registration| registration.data::<ReflectResource>().is_some())
        .map(|registration| (registration.short_name().to_owned(), registration.type_id()))
        .collect();
    resources.sort_by(|(name_a, _), (name_b, _)| name_a.cmp(name_b));

    for (resource_name, type_id) in resources {
        let selected = match *selection {
            InspectorSelection::Resource(selected, _) => selected == type_id,
            _ => false,
        };

        if ui.selectable_label(selected, &resource_name).clicked() {
            *selection = InspectorSelection::Resource(type_id, resource_name);
        }
    }
}

fn select_asset(
    ui: &mut egui::Ui,
    type_registry: &TypeRegistry,
    world: &World,
    selection: &mut InspectorSelection,
) {
    let mut assets: Vec<_> = type_registry
        .iter()
        .filter_map(|registration| {
            let reflect_asset = registration.data::<ReflectAsset>()?;
            Some((
                registration.short_name().to_owned(),
                registration.type_id(),
                reflect_asset,
            ))
        })
        .collect();
    assets.sort_by(|(name_a, ..), (name_b, ..)| name_a.cmp(name_b));

    for (asset_name, asset_type_id, reflect_asset) in assets {
        let mut handles: Vec<_> = reflect_asset.ids(world).collect();
        handles.sort();

        ui.collapsing(format!("{asset_name} ({})", handles.len()), |ui| {
            for handle in handles {
                let selected = match *selection {
                    InspectorSelection::Asset(_, _, selected_id) => selected_id == handle,
                    _ => false,
                };

                if ui
                    .selectable_label(selected, format!("{:?}", handle))
                    .clicked()
                {
                    *selection =
                        InspectorSelection::Asset(asset_type_id, asset_name.clone(), handle);
                }
            }
        });
    }
}
