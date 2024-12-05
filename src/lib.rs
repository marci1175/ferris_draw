use std::sync::{Arc, Mutex};

use bevy::{asset::RenderAssetUsages, color::Color, math::{Vec2, Vec3}, prelude::{Component, Mesh, Resource}, render::mesh::PrimitiveTopology, text::cosmic_text::Angle};
pub mod ui;
use dashmap::DashMap;
use mlua::Lua;
use once_cell::sync::Lazy;

pub const LUA_RUNTIME: Lazy<Arc<Mutex<Lua>>> = Lazy::new(|| Arc::new(Mutex::new(Lua::new())));

#[derive(Component)]
pub struct DrawerEntity(pub String);

/// A list of points that will have a line drawn between each consecutive points
#[derive(Debug, Clone, Default)]
pub struct LineStrip {
    pub points: Vec<Vec3>,
}

impl LineStrip {
    pub fn new(points: Vec<Vec3>) -> Self {
        Self {
            points
        }
    }
}

impl From<LineStrip> for Mesh {
        fn from(line: LineStrip) -> Self {
        Mesh::new(
            // This tells wgpu that the positions are a list of points
            // where a line will be drawn between each consecutive point
            PrimitiveTopology::LineStrip,
            RenderAssetUsages::RENDER_WORLD,
        )
        // Add the point positions as an attribute
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, line.points)
    }
}

#[derive(Resource, Debug, Default)]
pub struct DrawerInfo {
    pub pos: Vec2,
    pub ang: Angle,
    pub line: LineStrip,
    pub color: Color,
}

#[derive(Resource, Default, Debug)]
pub struct Drawers(pub Arc<DashMap<String, DrawerInfo>>);

pub fn init_lua_functions(lua_vm: Lua, drawers_handle: std::sync::Arc<dashmap::DashMap<String, DrawerInfo>>) -> Lua {
    let drawers_clone = drawers_handle.clone();

    let new_drawer = lua_vm.create_function(move |_, id: String| {
        let insertion = drawers_clone.insert(id.clone(), DrawerInfo::default());

        if insertion.is_some() {
            return Err(mlua::Error::RuntimeError(format!("The drawer with handle {id} already exists.")));
        }

        Ok(())
    }).unwrap();

    let drawers_clone = drawers_handle.clone();

    let rotate_drawer = lua_vm.create_function(move |_, params: (String, f32)| {
        let (id, degrees) = params;

        let drawer_handle = drawers_clone.get_mut(&id);

        match drawer_handle {
            Some(mut drawer) => {
                drawer.ang = Angle::from_degrees(drawer.ang.to_degrees() + degrees);
            },
            None => {
                return Err(mlua::Error::RuntimeError(format!("The drawer with handle {id} doesn't exists.")));
            },
        }

        Ok(())
    }).unwrap();

    let drawers_clone = drawers_handle.clone();

    let forward = lua_vm.create_function(move |_, params: (String, f32)| {
        let (id, amount) = params;

        let drawer_handle = drawers_clone.get_mut(&id);

        match drawer_handle {
            Some(mut drawer) => {
                let origin = drawer.pos;
                let angle_rad = drawer.ang.to_radians();
                let transformation_factor = amount;

                let x = origin.x + transformation_factor * angle_rad.cos();
                let y = origin.y + transformation_factor * angle_rad.sin();

                drawer.line.points.push(Vec3 { x, y, z: 0. });

                drawer.pos = Vec2::new(x, y);
            },
            None => {
                return Err(mlua::Error::RuntimeError(format!("The drawer with handle {id} doesn't exists.")));
            },
        }

        Ok(())
    }).unwrap();

    lua_vm.globals().set("new_drawer", new_drawer).unwrap();
    lua_vm.globals().set("rotate_drawer", rotate_drawer).unwrap();
    lua_vm.globals().set("forward", forward).unwrap();

    lua_vm
}