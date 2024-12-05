use std::{alloc::{self, Layout}, sync::{Arc, Mutex}};

use bevy::{asset::RenderAssetUsages, math::{Vec2, Vec3}, prelude::{Commands, Component, Mesh, Resource}, render::mesh::PrimitiveTopology};
pub mod ui;
use mlua::Lua;
use once_cell::sync::Lazy;
use std::pin::Pin;

pub const LUA_RUNTIME: Lazy<Arc<Lua>> = Lazy::new(|| {
    let lua_vm = unsafe { Lua::unsafe_new() };

    Arc::new(lua_vm)
});

pub const BEVY_COMMAND_HANDLE_PTR: Lazy<Pin<Arc<Mutex<*mut u8>>>> = unsafe {
    Lazy::new(|| {
        let layout = Layout::new::<Commands>();
    
        let ptr = alloc::alloc(layout);

        Pin::new(Arc::new(Mutex::new(ptr)))
    })
};

#[derive(Component)]
pub struct DrawingEnitity(pub String);

/// A list of points that will have a line drawn between each consecutive points
#[derive(Debug, Clone)]
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

#[derive(Resource)]
pub struct DrawerInfo {
    pub id: String,
    pub pos: Vec2,
}