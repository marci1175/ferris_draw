#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use bevy::{
    app::{App, FixedPreUpdate, FixedUpdate, Startup, Update},
    asset::Assets,
    color::Color,
    prelude::{Camera2d, Circle, Commands, Component, DetectChanges, Entity, Mesh, Mesh2d, Query, Res, ResMut, Resource, Transform, With},
    sprite::{ColorMaterial, MeshMaterial2d},
    DefaultPlugins,
};
use bevy_egui::{EguiContexts, EguiPlugin};

#[derive(Resource, Default)]
pub struct UiState {
    x: f32,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin)
        .init_resource::<UiState>()
        .add_systems(Startup, setup)
        .add_systems(Update, main_ui)
        .add_systems(FixedPreUpdate, clear_screen)
        .add_systems(FixedUpdate, draw)
        .run();
}

fn main_ui(mut state: ResMut<UiState>, mut contexts: EguiContexts<'_, '_>) {
    let ctx = contexts.ctx_mut();
    bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {
            if ui.button("Increment").clicked() {
                state.x += 10.;
            }
        });
}

fn setup(
    mut commands: Commands,
) {
    commands.spawn(Camera2d);
}

#[derive(Component)]
pub struct CircleEntity;

fn draw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    state: Res<UiState>,
)
{
    let shape = meshes.add(Circle::new(20.));
    
    commands.spawn((
        Mesh2d(shape),
        MeshMaterial2d(materials.add(Color::linear_rgb(255., 0., 100.))),
        Transform::from_xyz(state.x, 100., 10.),
        CircleEntity,
    ));
}

fn clear_screen(mut commands: Commands, state: Res<UiState>, entities: Query<Entity, With<CircleEntity>>) {
    if state.is_changed() {
        for entity in entities.iter() {
            commands.entity(entity).despawn();
        }
    }
}