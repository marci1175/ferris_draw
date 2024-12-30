#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use bevy::{
    asset::embedded_asset,
    math::Quat,
    prelude::PluginGroup,
    text::cosmic_text::Angle,
    window::{Window, WindowPlugin},
};
use std::{fs, path::PathBuf};
// hide console window on Windows in release
use bevy::{
    app::{App, AppExit, PreUpdate, Startup, Update},
    asset::{AssetServer, Assets},
    math::vec3,
    prelude::{
        Camera2d, Commands, Entity, EventReader, Mesh, Mesh2d, Query, Res, ResMut, Transform, With,
    },
    sprite::{ColorMaterial, MeshMaterial2d, Sprite},
    DefaultPlugins,
};
use bevy_egui::EguiPlugin;
use ferris_draw::{
    init_lua_functions,
    ui::{main_ui, UiState},
    DrawRequester, DrawerMesh, Drawers, FilledPolygonPoints, LuaRuntime,
};
use miniz_oxide::deflate::CompressionLevel;
 
fn main()
{
    let mut app = App::new();

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Ferris Draw".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .add_plugins(EguiPlugin)
    .init_resource::<UiState>()
    .init_resource::<Drawers>()
    .init_resource::<LuaRuntime>()
    .init_resource::<DrawRequester>()
    .add_systems(Startup, setup)
    .add_systems(PreUpdate, clear_screen)
    .add_systems(Update, main_ui)
    .add_systems(Update, draw)
    .add_systems(Update, exit_handler);

    embedded_asset!(app, "../assets/ferris.png");

    app.run();
}

fn setup(
    mut commands: Commands,
    drawers: Res<Drawers>,
    mut ui_state: ResMut<UiState>,
    draw_requested: Res<DrawRequester>,
    lua_runtime: ResMut<LuaRuntime>,
)
{
    //Load in save
    let mut app_data_path = PathBuf::from(env!("APPDATA"));

    app_data_path.push("ferris_draw");
    app_data_path.push("serde.data");

    match fs::read(app_data_path) {
        Ok(read_bytes) => {
            let decompressed_data = miniz_oxide::inflate::decompress_to_vec(&read_bytes).unwrap();

            let data: UiState = rmp_serde::from_slice(&decompressed_data).unwrap_or_default();

            *ui_state = data;
        },
        Err(_err) => {
            //The save didnt exist
        },
    }

    commands.spawn(Camera2d);

    let toast_handle = ui_state.toasts.clone();
    let demo_buffer_handle = ui_state.demo_buffer.clone();

    init_lua_functions(
        lua_runtime,
        draw_requested,
        drawers.clone(),
        ui_state.command_line_outputs.clone(),
        demo_buffer_handle,
        toast_handle,
    );
}

fn exit_handler(exit_events: EventReader<AppExit>, ui_state: Res<UiState>)
{
    // This indicated that the app has been closed
    if !exit_events.is_empty() {
        let mut app_data_path = PathBuf::from(env!("APPDATA"));

        app_data_path.push("ferris_draw");

        fs::create_dir_all(app_data_path.clone()).unwrap();

        app_data_path.push("serde.data");

        let compressed_data = miniz_oxide::deflate::compress_to_vec(
            &rmp_serde::to_vec(&*ui_state).unwrap(),
            CompressionLevel::BestCompression as u8,
        );

        fs::write(app_data_path, compressed_data).unwrap();
    }
}

fn draw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    drawers: Res<Drawers>,
    draw_requester: Res<DrawRequester>,
    asset_server: Res<AssetServer>,
)
{
    // Try to receive draw requests from the lua runtime
    if let Ok((points, color, id)) = draw_requester.receiver.lock().try_recv() {
        if let Some(mut drawer) = drawers.get_mut(&id) {
            drawer
                .drawings
                .polygons
                .push(FilledPolygonPoints::new(points, color));
        }
    }

    for drawer in drawers.iter() {
        let (_id, drawer_info) = drawer.pair();

        for line_strip in &drawer_info.drawings.lines {
            let mesh = Mesh::from(line_strip.clone());

            let shape = meshes.add(mesh);

            commands.spawn((
                Mesh2d(shape),
                MeshMaterial2d(materials.add(drawer_info.color)),
                DrawerMesh,
            ));
        }

        for polygon in &drawer_info.drawings.polygons {
            let mesh = Mesh::from(polygon.clone());

            let shape = meshes.add(mesh);

            commands.spawn((
                Mesh2d(shape),
                MeshMaterial2d(materials.add(drawer_info.color)),
                DrawerMesh,
            ));
        }

        let icon: bevy::prelude::Handle<bevy::prelude::Image> =
            asset_server.load("embedded://ferris_draw/../assets/ferris.png");

        commands.spawn((
            Sprite::from_image(icon),
            Transform::from_xyz(drawer_info.pos.x, drawer_info.pos.y, 0.)
                .with_rotation(Quat::from_rotation_z(
                    Angle::from_degrees(drawer.ang.to_degrees() - 90.).to_radians(),
                ))
                .with_scale(vec3(0.1, 0.1, 1.)),
            DrawerMesh,
        ));
    }
}

fn clear_screen(mut commands: Commands, entities: Query<Entity, With<DrawerMesh>>)
{
    for entity in entities.iter() {
        commands.entity(entity).despawn();
    }
}
