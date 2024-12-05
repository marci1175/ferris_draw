#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] 

use std::{alloc::{alloc, Layout}, process::Command, sync::Arc};

// hide console window on Windows in release
use bevy::{
    app::{App, FixedPreUpdate, FixedUpdate, Startup, Update}, asset::Assets, color::Color, math::Vec3, prelude::{Camera2d, Commands, Entity, Mesh, Mesh2d, Query, ResMut, With}, ptr, render::mesh::ConicalFrustumMeshBuilder, sprite::{ColorMaterial, MeshMaterial2d}, tasks::TaskPool, DefaultPlugins
};
use bevy_egui::EguiPlugin;
use ferris_draw::{ui::{main_ui, UiState}, DrawingEnitity, LineStrip, BEVY_COMMAND_HANDLE_PTR};
use mlua::Lua;

#[tokio::main]
async fn main() {
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

fn setup(
    mut commands: Commands,
) {
    
    commands.spawn(Camera2d);

    unsafe {
        if let Ok(ptr_handle) = BEVY_COMMAND_HANDLE_PTR.lock() {
            std::ptr::write(ptr_handle.cast::<Commands>(), commands);
        }
        else {
            panic!("Failed to write `Command` instance to the global handle.")
        }
    };

    let lua_vm = unsafe { Lua::unsafe_new() };
    
    unsafe {
        let lua_fn = lua_vm.create_function(|_, pos2: (f32, f32)| {
            let (x, y) = pos2;

            let command_handle = retrive_commands_handle();
            
            if let Ok(mut command) = command_handle {

            }

            Ok(())
        }).unwrap();
    }
}

unsafe fn retrive_commands_handle<'w, 's>() -> anyhow::Result<Commands<'w, 's>> {
    match BEVY_COMMAND_HANDLE_PTR.lock() {
        Ok(ptr_handle) => {
            let command_handle = std::ptr::read(ptr_handle.cast::<Commands>().cast_const());
            
            Ok(command_handle)
        },
        Err(_err) => {
            Err(anyhow::Error::msg(_err.to_string()))
        },
    }
}

fn draw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
)
{
    let shape = meshes.add(LineStrip::new(vec![
        Vec3::new(100., 200., 0.),
        Vec3::new(200., 100., 0.),
        Vec3::new(7., 000., 0.),
    ]));

    commands.spawn((
        Mesh2d(shape),
        MeshMaterial2d(materials.add(Color::linear_rgb(255., 0., 100.))),
        DrawingEnitity("asd".to_string()),
    ));
}

fn clear_screen(mut commands: Commands, entities: Query<Entity, With<DrawingEnitity>>) {
    for entity in entities.iter() {
        commands.entity(entity).despawn();
    }
}