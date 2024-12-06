use std::{ops::{Deref, DerefMut}, sync::Arc};

use bevy::{asset::RenderAssetUsages, color::Color, ecs::query::{QueryData, WorldQuery}, math::{Vec2, Vec3}, prelude::{Component, Mesh, ResMut, Resource}, render::mesh::PrimitiveTopology, text::cosmic_text::Angle};
pub mod ui;
use dashmap::DashMap;
use mlua::Lua;

#[derive(Resource, Clone)]
pub struct LuaRuntime(pub Lua);

impl Default for LuaRuntime {
    fn default() -> Self {
        Self(unsafe {
            Lua::unsafe_new()
        })
    }
}

/// Implement dereferencing for LuaRuntime so that I wouldnt have to call .0 everytime i want to access the inner value.
impl Deref for LuaRuntime {
    type Target = Lua;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Implement dereferencing for LuaRuntime so that I wouldnt have to call .0 everytime i want to access the inner value.
impl DerefMut for LuaRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

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

/// The information of the Drawer
#[derive(Resource, Debug, Default)]
pub struct Drawer {
    /// The position of the Drawer.
    pub pos: Vec2,
    
    /// The angle of the Drawer.
    pub ang: Angle,

    /// The line drawn by the drawer.
    pub line: LineStrip,
    
    /// The color of the Drawer.
    pub color: Color,
}

/// The list of the drawers currently alive.
/// This list is modified through the [`Lua`] runtime.
/// The key is a [`String`] is used to identify each individual [`Drawer`].
#[derive(Resource, Default, Debug)]
pub struct Drawers(pub Arc<DashMap<String, Drawer>>);

impl Deref for Drawers {
    type Target = Arc<DashMap<String, Drawer>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Drawers {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Create a valid* [`Lua`] runtime.
/// This function automaticly adds all the functions to the global variables.
pub fn init_lua_functions(lua_rt: ResMut<LuaRuntime>, drawers_handle: std::sync::Arc<dashmap::DashMap<String, Drawer>>) {
    let lua_vm = lua_rt.clone();

    let drawers_clone = drawers_handle.clone();

    let new_drawer = lua_vm.create_function(move |_, id: String| {
        let insertion = drawers_clone.insert(id.clone(), Drawer::default());

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
}