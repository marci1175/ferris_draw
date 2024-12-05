#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] 

// hide console window on Windows in release
use bevy::{
    app::{App, FixedPreUpdate, FixedUpdate, Startup, Update}, asset::Assets, color::Color, math::Vec3, prelude::{Camera2d, Commands, Entity, Mesh, Mesh2d, Query, Res, ResMut, With}, sprite::{ColorMaterial, MeshMaterial2d}, DefaultPlugins
};
use bevy_egui::EguiPlugin;
use ferris_draw::{init_lua_functions, ui::{main_ui, UiState}, DrawerEntity, Drawers, LineStrip, LUA_RUNTIME};
use mlua::Lua;

#[tokio::main]
async fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EguiPlugin)
        .init_resource::<UiState>()
        .init_resource::<Drawers>()
        .add_systems(Startup, setup)
        .add_systems(Update, main_ui)
        .add_systems(FixedUpdate, draw)
        .run();
}

fn setup(
    mut commands: Commands,
    drawers: Res<Drawers>,
) {
    commands.spawn(Camera2d);

    let drawers_handle = drawers.0.clone();

    *LUA_RUNTIME.lock().unwrap() = init_lua_functions(unsafe {
        Lua::unsafe_new()
    }, drawers_handle);
}

fn draw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    drawers: Res<Drawers>,
)
{
    for drawer in drawers.0.iter() {
        let (id, drawer_info) = drawer.pair();

        let shape = meshes.add(drawer_info.line.clone());
    
        commands.spawn((
            Mesh2d(shape),
            MeshMaterial2d(materials.add(drawer_info.color.clone())),
            DrawerEntity(id.clone()),
        ));
    }
}

fn clear_screen(mut commands: Commands, entities: Query<Entity, With<DrawerEntity>>) {
    for entity in entities.iter() {
        commands.entity(entity).despawn();
    }
}