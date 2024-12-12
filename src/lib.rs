use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use bevy::{
    asset::RenderAssetUsages,
    color::Color,
    math::{Vec2, Vec3, Vec4},
    prelude::{Component, Mesh, ResMut, Resource},
    render::mesh::PrimitiveTopology,
    text::cosmic_text::Angle,
};
pub mod ui;
use dashmap::DashMap;
use mlua::Lua;
use parking_lot::RwLock;

#[derive(Resource, Clone)]
pub struct LuaRuntime(pub Lua);

impl Default for LuaRuntime
{
    fn default() -> Self
    {
        Self(unsafe { Lua::unsafe_new() })
    }
}

/// Implement dereferencing for LuaRuntime so that I wouldnt have to call .0 everytime i want to access the inner value.
impl Deref for LuaRuntime
{
    type Target = Lua;

    fn deref(&self) -> &Self::Target
    {
        &self.0
    }
}

/// Implement dereferencing for LuaRuntime so that I wouldnt have to call .0 everytime i want to access the inner value.
impl DerefMut for LuaRuntime
{
    fn deref_mut(&mut self) -> &mut Self::Target
    {
        &mut self.0
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum ScriptLinePrompts
{
    UserInput(String),
    Standard(String),
    Error(String),
}

#[derive(Component)]
pub struct DrawerEntity(pub String);

/// A list of points that will have a line drawn between each consecutive points
#[derive(Debug, Clone, Default)]
pub struct LineStrip
{
    pub points: Vec<(Vec3, Color)>,
}

impl LineStrip
{
    pub fn new(points: Vec<(Vec3, Color)>) -> Self
    {
        Self { points }
    }
}

impl From<LineStrip> for Mesh
{
    fn from(line: LineStrip) -> Self
    {
        Mesh::new(
            // This tells wgpu that the positions are a list of points
            // where a line will be drawn between each consecutive point
            PrimitiveTopology::LineStrip,
            RenderAssetUsages::RENDER_WORLD,
        )
        // Add the point positions as an attribute
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            line.points
                .iter()
                .map(|point| point.0)
                .collect::<Vec<Vec3>>(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_COLOR,
            line.points
                .iter()
                .map(|point| color_into_vec4(point.1))
                .collect::<Vec<Vec4>>(),
        )
    }
}

pub fn color_into_vec4(color: Color) -> Vec4
{
    Vec4::new(
        color.to_linear().red,
        color.to_linear().green,
        color.to_linear().blue,
        color.to_linear().alpha,
    )
}

/// The information of the Drawer
#[derive(Resource, Debug)]
pub struct Drawer
{
    /// The position of the Drawer.
    pub pos: Vec2,

    /// The angle of the Drawer.
    pub ang: Angle,

    /// The line drawn by the drawer.
    pub line: LineStrip,

    /// The color of the Drawer.
    pub color: Color,
}

impl Default for Drawer
{
    fn default() -> Self
    {
        Self {
            pos: Vec2::default(),
            ang: Angle::from_degrees(90.),
            line: LineStrip {
                points: vec![(Vec3::default(), Color::WHITE)],
            },
            color: Color::WHITE,
        }
    }
}

/// The list of the drawers currently alive.
/// This list is modified through the [`Lua`] runtime.
/// The key is a [`String`] is used to identify each individual [`Drawer`].
#[derive(Resource, Default, Debug, Clone)]
pub struct Drawers(pub Arc<DashMap<String, Drawer>>);

/// Implement dereferencing for the [`Drawers`] type.
impl Deref for Drawers
{
    type Target = Arc<DashMap<String, Drawer>>;

    fn deref(&self) -> &Self::Target
    {
        &self.0
    }
}

/// Implement mutable dereferencing for the [`Drawers`] type.
impl DerefMut for Drawers
{
    fn deref_mut(&mut self) -> &mut Self::Target
    {
        &mut self.0
    }
}

/// Create a valid* [`Lua`] runtime.
/// This function automaticly adds all the functions to the global variables.
pub fn init_lua_functions(
    lua_rt: ResMut<LuaRuntime>,
    drawers_handle: Drawers,
    output_list: Arc<RwLock<Vec<ScriptLinePrompts>>>,
)
{
    let lua_vm = lua_rt.clone();

    let print = lua_vm
        .create_function(move |_, msg: String| {
            output_list.write().push(ScriptLinePrompts::Standard(msg));

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    // Creates a new drawer with the Drawer handle, from a unique handle.
    let new_drawer = lua_vm
        .create_function(move |_, id: String| {
            let insertion = drawers_clone.insert(id.clone(), Drawer::default());

            if insertion.is_some() {
                return Err(mlua::Error::RuntimeError(format!(
                    "The drawer with handle {id} already exists."
                )));
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    // Sets the drawer's angle.
    let rotate_drawer = lua_vm
        .create_function(move |_, params: (String, f32)| {
            // Get params
            let (id, degrees) = params;

            // Clone the drawers' list handle
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    // Set the drawer's angle.
                    drawer.ang = Angle::from_degrees(drawer.ang.to_degrees() + degrees);
                },
                None => {
                    // Return the error
                    return Err(mlua::Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    // Resets the drawers position and angle.
    let center = lua_vm
        .create_function(move |_, id: String| {
            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    //Reset the drawer's position.
                    drawer.pos = Vec2::default();
                    //Reset the drawer's angle.
                    drawer.ang = Angle::from_degrees(90.);
                },
                None => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    // Sets the color of the drawing
    let color = lua_vm
        .create_function(move |_, params: (String, f32, f32, f32, f32)| {
            // Get params
            let (id, red, green, blue, alpha) = params;

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    // Set the drawer's color
                    drawer.color = Color::linear_rgba(red, green, blue, alpha);
                },
                None => {
                    // Return the error
                    return Err(mlua::Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    // Moves the drawer forward by a set amount of units, this makes the drawer draw too.
    let forward = lua_vm
        .create_function(move |_, params: (String, f32)| {
            // Get params
            let (id, amount) = params;

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    // Calculate the difference on the y and x coordinate from its angle.
                    // Get origin
                    let origin = drawer.pos;
                    // Degrees into radians.
                    let angle_rad = drawer.ang.to_radians();

                    // Forward units
                    let transformation_factor = amount;

                    // The new x.
                    let x = origin.x + transformation_factor * angle_rad.cos();
                    // The new y.
                    let y = origin.y + transformation_factor * angle_rad.sin();

                    //Clone the color so we can move it into the lines' list.
                    let drawer_color = drawer.color;

                    //Store the new position and the drawer's color.
                    drawer
                        .line
                        .points
                        .push((Vec3 { x, y, z: 0. }, drawer_color));

                    //Set the new drawers position.
                    drawer.pos = Vec2::new(x, y);
                },
                None => {
                    //Reset the drawer's position
                    return Err(mlua::Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    //Set all the functions in the global handle of the lua runtime
    lua_vm.globals().set("new", new_drawer).unwrap();
    lua_vm.globals().set("rotate", rotate_drawer).unwrap();
    lua_vm.globals().set("forward", forward).unwrap();
    lua_vm.globals().set("center", center).unwrap();
    lua_vm.globals().set("color", color).unwrap();
    lua_vm.globals().set("print", print).unwrap();
}
