#![warn(unused_crate_dependencies)]

pub const DEMO_FILE_EXTENSION: &str = "demo";
pub const PROJECT_FILE_EXTENSION: &str = "save";

use bevy::{
    asset::RenderAssetUsages,
    color::Color,
    math::{Vec2, Vec3, Vec4},
    prelude::{Component, Mesh, Res, ResMut, Resource},
    render::mesh::PrimitiveTopology,
};

#[cfg(target_family = "wasm")]
use fragile::Fragile;
#[cfg(target_family = "wasm")]
use piccolo::{error::LuaError, Callback, RuntimeError, Value};

use std::{
    collections::VecDeque, fmt::Display, ops::{Deref, DerefMut}, sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    }
};
use strum::{EnumCount, EnumIter};

pub mod ui;
use chrono::{DateTime, Local};
use dashmap::DashMap;
use egui_toast::{Toast, Toasts};
use geo::{coord, point, Contains, ConvexHull, Coord, LineString, Polygon};

#[cfg(not(target_family = "wasm"))]
use mlua::{Error, Function};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use typed_floats::NonNaN;

#[derive(Clone, Serialize, Deserialize)]
pub struct DemoBuffer<T>
{
    pub buffer: Arc<RwLock<T>>,
    pub iter_idx: usize,
    pub state: Arc<RwLock<DemoBufferState>>,
}

#[derive(PartialEq, Default, serde::Serialize, serde::Deserialize, Clone, Copy)]
pub enum DemoBufferState
{
    Playback,
    Record,

    #[default]
    None,
}

impl<T: Default> DemoBuffer<T>
{
    pub fn new(inner: T) -> Self
    {
        Self {
            // Create an `Arc<Mutex<T>>` for the buffer.
            buffer: Arc::new(RwLock::new(inner)),
            // The index indicating which value in the buffer should be accessed.
            iter_idx: 0,
            // Initalize the buffer state with Default (None).
            state: Arc::new(RwLock::new(DemoBufferState::default())),
        }
    }

    pub fn clear(&mut self)
    {
        *self.buffer.write() = T::default();
        self.set_state(DemoBufferState::None);
        self.iter_idx = 0;
    }

    pub fn set_buffer(&self, buffer: T)
    {
        *self.buffer.write() = buffer;
    }

    pub fn get_state_if_eq(&self, buffer_state: DemoBufferState) -> Option<&RwLock<T>>
    {
        if *self.state.read() == buffer_state {
            return Some(&self.buffer);
        }

        None
    }

    pub fn get_state(&self) -> DemoBufferState
    {
        *self.state.read()
    }

    pub fn set_state(&self, state: DemoBufferState)
    {
        *self.state.write() = state;
    }
}

/// The DemoInstance is used to store demos of scripts.
/// These contain a script identifier, so that we can notify the user if their code has changed since the last demo recording.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DemoInstance
{
    pub name: String,
    pub demo_steps: Vec<DemoStep>,
    pub created_at: DateTime<Local>,
}

/// The items of this enum contain the functions a user can call on their turtles.
/// When recording a demo these are stored and can later be playbacked.
/// All of the arguments to the functions are contained in the enum variants.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub enum DemoStep
{
    New(String),
    Center(String),
    Forward(String, NonNaN<f32>),
    Rotate(String, NonNaN<f32>),
    SetAngle(String, NonNaN<f32>),
    Color(String, NonNaN<f32>, NonNaN<f32>, NonNaN<f32>, NonNaN<f32>),
    Wipe,
    Remove(String),
    Disable(String),
    Enable(String),
    Fill(String),
    Rectangle(String, NonNaN<f32>, NonNaN<f32>),
    Print(String),
    Loop(usize, Vec<DemoStep>),
    PointTo(String, f32, f32),
}

impl DemoStep
{
    pub fn execute_lua_function(&self, lua_rt: LuaRuntime) -> anyhow::Result<()>
    {
        lua_rt.execute_code(&self.to_string())
    }
}

impl Display for DemoStep
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.write_str(&match self {
            DemoStep::New(id) => {
                format!(r#"new("{id}")"#)
            },
            DemoStep::Center(id) => {
                format!(r#"center("{id}")"#)
            },
            DemoStep::Forward(id, amount) => {
                format!(r#"forward("{id}", {amount})"#)
            },
            DemoStep::Rotate(id, amount) => {
                format!(r#"rotate("{id}", {amount})"#)
            },
            DemoStep::Color(id, r, g, b, a) => {
                format!(r#"color("{id}", {r}, {g}, {b}, {a})"#)
            },
            DemoStep::Wipe => r#"wipe()"#.to_string(),
            DemoStep::Remove(id) => {
                format!(r#"remove("{id}")"#)
            },
            DemoStep::Disable(id) => {
                format!(r#"disable("{id}")"#)
            },
            DemoStep::Enable(id) => {
                format!(r#"disable("{id}")"#)
            },
            DemoStep::Fill(id) => {
                format!(r#"fill("{id}")"#)
            },
            DemoStep::Rectangle(id, desired_x, desired_y) => {
                format!(r#"rectangle("{id}", {desired_x}, {desired_y})"#)
            },
            DemoStep::Print(string) => {
                format!(r#"print("{string}")"#)
            },
            DemoStep::Loop(count, vec) => {
                format!("for i=1,{count} do {} end", {
                    vec.iter()
                        .map(|step| step.to_string())
                        .collect::<Vec<String>>()
                        .join("\n")
                })
            },
            DemoStep::SetAngle(id, angle) => {
                format!(r#"set_angle("{id}", {angle})"#)
            },
            DemoStep::PointTo(id, dx, dy) => {
                format!(r#"point_to("{id}", {dx}, {dy})"#)
            }
        })
    }
}

#[derive(Resource, Clone)]
pub struct DrawRequester
{
    pub receiver: Arc<Mutex<Receiver<(Vec<Vec3>, Color, String)>>>,
    pub sender: Arc<Sender<(Vec<Vec3>, Color, String)>>,
}

impl Default for DrawRequester
{
    fn default() -> Self
    {
        let (sender, receiver) = channel::<(Vec<Vec3>, Color, String)>();
        Self {
            sender: Arc::new(sender),
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

#[derive(Resource, Clone)]
pub struct LuaRuntime(
    #[cfg(not(target_family = "wasm"))] pub mlua::Lua,
    #[cfg(target_family = "wasm")] pub Arc<Fragile<Mutex<piccolo::Lua>>>,
);

impl Default for LuaRuntime
{
    fn default() -> Self
    {
        #[cfg(not(target_family = "wasm"))]
        return Self(unsafe { mlua::Lua::unsafe_new() });

        #[cfg(target_family = "wasm")]
        {
            let mut piccolo_lua = piccolo::Lua::core();

            piccolo_lua.load_io();

            return Self(Arc::new(Fragile::new(Mutex::new(piccolo_lua))));
        }
    }
}

impl LuaRuntime
{
    pub fn execute_code(&self, code: &str) -> anyhow::Result<()>
    {
        #[cfg(not(target_family = "wasm"))]
        self.load(code).exec()?;

        #[cfg(target_family = "wasm")]
        {
            use piccolo::{Closure, Executor};

            let mut lua = self.get().lock();
            let executor = lua.try_enter(|ctx| {
                let closure = Closure::load(ctx, None, code.as_bytes())?;

                Ok(ctx.stash(Executor::start(
                    ctx,
                    piccolo::Function::Closure(closure),
                    (),
                )))
            })?;

            lua.execute::<()>(&executor)?;
        }

        Ok(())
    }
}

impl Deref for LuaRuntime
{
    #[cfg(not(target_family = "wasm"))]
    type Target = mlua::Lua;

    #[cfg(target_family = "wasm")]
    type Target = Arc<Fragile<Mutex<piccolo::Lua>>>;

    fn deref(&self) -> &Self::Target
    {
        &self.0
    }
}

impl DerefMut for LuaRuntime
{
    fn deref_mut(&mut self) -> &mut Self::Target
    {
        &mut self.0
    }
}

/// This buffer type has a set length. 
/// The buffer is always `n` long, if newer items are pushed to the buffer the older ones will get dropped.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SetLenBuffer<T> {
    inner_buffer: VecDeque<T>,
    buffer_length: usize,
}

impl<T> SetLenBuffer<T> {
    /// Creates a new instance of [`SetLenBuffer<T>`].
    pub fn new(buffer_length: usize) -> Self {
        Self { inner_buffer: VecDeque::with_capacity(buffer_length), buffer_length }
    }

    /// Pushes the item to the back of the inner buffer. 
    /// If the current buffer's length == `self.buffer_length` it will first pop the first item from the list.
    pub fn push(&mut self, item: T) {
        if self.inner_buffer.len() >= self.buffer_length {
            self.inner_buffer.pop_front();
        }

        self.inner_buffer.push_back(item);
    }

    /// Sets the maximum length of the inner buffer.
    pub fn set_len(&mut self, len: usize) {
        self.buffer_length = len;
    }
}

impl<T> Deref for SetLenBuffer<T> {
    type Target = VecDeque<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner_buffer
    }
}

impl<T> DerefMut for SetLenBuffer<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner_buffer
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
pub struct DrawerMesh;

/// A list of points that will have a line drawn between each consecutive points
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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

/// A list of points that will have polygon created from them
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FilledPolygonPoints
{
    /// The points of the polygon.
    pub points: Vec<Vec3>,
    /// The color of the polygon.
    pub color: Color,
}

impl FilledPolygonPoints
{
    pub fn new(points: Vec<Vec3>, color: Color) -> Self
    {
        Self { points, color }
    }
}

impl From<FilledPolygonPoints> for Mesh
{
    fn from(line: FilledPolygonPoints) -> Self
    {
        let mut indices = vec![];

        for i in 1..line.points.len() - 1 {
            indices.push(0);
            indices.push(i as u32);
            indices.push((i + 1) as u32);
        }

        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleStrip,
            RenderAssetUsages::RENDER_WORLD,
        )
        // Add the point positions as an attribute
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, line.points.to_vec())
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vec![[0., 0., 1.]; line.points.len()],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; line.points.len()]);

        mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));

        mesh
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

#[derive(Debug, Clone, PartialEq)]
pub struct Angle(bevy::text::cosmic_text::Angle);

impl Deref for Angle
{
    type Target = bevy::text::cosmic_text::Angle;

    fn deref(&self) -> &Self::Target
    {
        &self.0
    }
}

impl Angle
{
    fn from_degrees(degrees: f32) -> Self
    {
        Self(bevy::text::cosmic_text::Angle::from_degrees(degrees))
    }
}

impl Serialize for Angle
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let angle_radians = self.0.to_radians();

        angle_radians.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Angle
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let angle_value: f32 = Deserialize::deserialize(deserializer)?;
        Ok(Angle(bevy::text::cosmic_text::Angle::from_radians(
            angle_value,
        )))
    }
}

/// The information of the Drawer
#[derive(Resource, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Drawer
{
    /// Whether the Drawer should draw.
    pub enabled: bool,

    /// The position of the Drawer.
    pub pos: Vec2,

    /// The angle of the Drawer.
    pub ang: Angle,

    /// The line drawn by the drawer.
    pub drawings: Drawings,

    /// The color of the Drawer.
    pub color: Color,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Drawings
{
    /// The lines drawn by the drawer.
    pub lines: Vec<LineStrip>,
    /// The polygons drawn by the drawer.
    pub polygons: Vec<FilledPolygonPoints>,
}

impl Default for Drawings
{
    fn default() -> Self
    {
        Self {
            lines: vec![LineStrip::new(vec![(Vec3::default(), Color::WHITE)])],
            polygons: vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub enum DrawingType
{
    Line(LineStrip),
    Polygon(FilledPolygonPoints),
}

impl Default for Drawer
{
    fn default() -> Self
    {
        Self {
            enabled: true,
            pos: Vec2::default(),
            ang: Angle::from_degrees(90.),
            drawings: Drawings::default(),
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
#[cfg(not(target_family = "wasm"))]
pub fn init_lua_functions(
    lua_rt: ResMut<LuaRuntime>,
    draw_requester: Res<DrawRequester>,
    drawers_handle: Drawers,
    output_list: Arc<RwLock<SetLenBuffer<ScriptLinePrompts>>>,
    demo_buffer: DemoBuffer<Vec<DemoStep>>,
    toast_handle: Arc<Mutex<Toasts>>,
)
{
    let lua_vm = lua_rt.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let print = lua_vm
        .create_function(move |_, msg: String| {
            if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                buffer.write().push(DemoStep::Print(msg));

                return Ok(());
            }

            output_list.write().push(ScriptLinePrompts::Standard(msg));

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    // Creates a new drawer with the Drawer handle, from a unique handle.
    let new = lua_vm
        .create_function(move |_, id: String| {
            if !drawers_clone.contains_key(&id) {
                if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                    buffer.write().push(DemoStep::New(id.clone()));
                }

                drawers_clone.insert(id.clone(), Drawer::default());
            }
            else {
                return Err(mlua::Error::RuntimeError(format!(
                    r#"The drawer with handle "{id}" already exists."#
                )));
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    // Sets the drawer's angle.
    let rotate_drawer = lua_vm
        .create_function(move |_, params: (String, f32)| {
            // Get params
            let (id, degrees) = params;

            // Clone the drawers' list handle
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Rotate(
                            id,
                            NonNaN::<f32>::new(degrees).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    // Set the drawer's angle.
                    drawer.ang = Angle::from_degrees(drawer.ang.to_degrees() + degrees);
                },
                None => {
                    // Return the error
                    return Err(mlua::Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();
        // Sets the drawer's angle.
        
        let set_drawer_angle = lua_vm
        .create_function(move |_, params: (String, f32)| {
            // Get params
            let (id, degrees) = params;

            // Clone the drawers' list handle
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::SetAngle(
                            id,
                            NonNaN::<f32>::new(degrees).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    // Set the drawer's angle.
                    drawer.ang = Angle::from_degrees(degrees);
                },
                None => {
                    // Return the error
                    return Err(mlua::Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    // Resets the drawers position and angle.
    let center = lua_vm
        .create_function(move |_, id: String| {
            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Center(id));

                        return Ok(());
                    }

                    //Reset the drawer's position.
                    drawer.pos = Vec2::default();

                    let drawer_color = drawer.color;

                    //Add the reseted pos to the drawer
                    drawer.drawings.lines.push(LineStrip {
                        points: vec![(Vec3::default(), drawer_color)],
                    });

                    //Reset the drawer's angle.
                    drawer.ang = Angle::from_degrees(90.);
                },
                None => {
                    return Err(mlua::Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    // Sets the color of the drawing
    let color = lua_vm
        .create_function(move |_, params: (String, f32, f32, f32, f32)| {
            // Get params
            let (id, red, green, blue, alpha) = params;

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Color(
                            id,
                            NonNaN::<f32>::new(red).unwrap_or_default(),
                            NonNaN::<f32>::new(green).unwrap_or_default(),
                            NonNaN::<f32>::new(blue).unwrap_or_default(),
                            NonNaN::<f32>::new(alpha).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    // Set the drawer's color
                    drawer.color = Color::linear_rgba(red, green, blue, alpha);
                },
                None => {
                    // Return the error
                    return Err(mlua::Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    // Moves the drawer forward by a set amount of units, this makes the drawer draw too.
    let forward = lua_vm
        .create_function(move |_, params: (String, f32)| {
            // Get params
            let (id, amount) = params;

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Forward(
                            id,
                            NonNaN::<f32>::new(amount).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    // Calculate the difference on the y and x coordinate from its angle.
                    // Get origin
                    let origin = drawer.pos;

                    //Clone the color so we can move it into the lines' list.
                    let drawer_color = drawer.color;

                    // Degrees into radians.
                    let angle_rad = drawer.ang.to_radians();

                    // Forward units
                    let amount_forward = amount;

                    // The new x.
                    let x = origin.x
                        + (amount_forward * floating_point_calculation_error(angle_rad.cos()));
                    // The new y.
                    let y = origin.y
                        + (amount_forward * floating_point_calculation_error(angle_rad.sin()));

                    //Store the new position and the drawer's color if it is enabled
                    if drawer.enabled {
                        drawer
                            .drawings
                            .lines
                            .last_mut()
                            .unwrap()
                            .points
                            .push((Vec3::new(x, y, 0.), drawer_color));
                    }

                    //Set the new drawers position.
                    drawer.pos = Vec2::new(x, y);
                },
                None => {
                    //Reset the drawer's position
                    return Err(mlua::Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let wipe = lua_vm
        .create_function(move |_, _: ()| {
            if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                buffer.write().push(DemoStep::Wipe);

                return Ok(());
            }

            for mut drawer in drawers_clone.iter_mut() {
                let drawer = drawer.value_mut();

                let mut default_drawings = Drawings::default();
                default_drawings.lines.push(LineStrip {
                    points: vec![(Vec3::new(drawer.pos.x, drawer.pos.y, 0.), Color::WHITE)],
                });
                drawer.drawings = default_drawings;
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    let exists = lua_vm
        .create_function(move |_, id: String| Ok(drawers_clone.contains_key(&id)))
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let remove = lua_vm
        .create_function(move |_, id: String| {
            if drawers_clone.remove(&id).is_none() {
                return Err(Error::RuntimeError(format!(
                    r#"The drawer with handle "{id}" doesn't exist."#
                )));
            }

            if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                buffer.write().push(DemoStep::Remove(id));

                return Ok(());
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    let drawers = lua_vm
        .create_function(move |_, _: ()| {
            let mut names = Vec::new();

            for drawer in drawers_clone.iter() {
                names.push(drawer.key().clone());
            }

            Ok(names)
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let enable = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Enable(id));

                        return Ok(());
                    }

                    drawer.enabled = true;

                    let tuple = (Vec3::new(drawer.pos.x, drawer.pos.y, 0.), drawer.color);

                    drawer.drawings.lines.push(LineStrip::new(vec![tuple]));
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();
    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let disable = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Disable(id));

                        return Ok(());
                    }

                    drawer.enabled = false;
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let draw_request_sender = draw_requester.sender.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let fill = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get(&id) {
                Some(selected_drawer) => {
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Fill(id));

                        return Ok(());
                    }

                    let drawer_lines: Vec<Vec3> = drawers_clone
                        .iter()
                        .flat_map(|pair| {
                            let drawer = pair.value();

                            drawer.drawings.clone().lines.iter().flat_map(|line_strip| {
                                line_strip.points.iter().map(|points| points.0).collect::<Vec<Vec3>>()
                            }).collect::<Vec<Vec3>>()
                        })
                        .collect();

                    let mut lines: Vec<Line> = vec![];

                    for positions in dbg!(drawer_lines).windows(2) {
                        let (min, max) = (positions[0], positions[1]);

                        lines.push(Line::new(min, max));
                    }

                    let mut checked_lines: Vec<Line> = vec![];

                    for (idx, line) in lines.iter().enumerate() {
                        for (current_checked_idx, checked_line) in checked_lines.iter().enumerate() {
                            if idx as isize - 1 != current_checked_idx as isize && checked_lines.len() > 2 {
                                if let Some(intersection_pos) = line.intersects(checked_line) {
                                    let intersected_line_idx = checked_lines.iter().position(|line| line == checked_line).unwrap();
                                    let mut polygon_points: Vec<Coord> = vec![];

                                    polygon_points.push(coord!{x: intersection_pos.x as f64, y: intersection_pos.y as f64});

                                    for poly_line in &checked_lines[intersected_line_idx + 1..idx] {
                                        polygon_points.push(coord! {x: poly_line.max.x as f64, y: poly_line.max.y as f64});
                                    }

                                    let polygon = Polygon::new(LineString::new(polygon_points.clone()), vec![]);

                                    let poly_convex_hull = polygon.convex_hull();

                                    if poly_convex_hull.contains(&point!(x: selected_drawer.pos.x as f64, y: selected_drawer.pos.y as f64)) {
                                        draw_request_sender.send((polygon_points.iter().map(|coord| Vec3::new(coord.x as f32, coord.y as f32, 0.)).collect::<Vec<Vec3>>(), selected_drawer.color, id.clone())).unwrap();
                                    }

                                    break;
                                }
                            }
                        }

                        checked_lines.push(line.clone());
                    }
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    let toasts_handle = toast_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let notification = lua_vm
        .create_function(move |_, params: (u32, String)| {
            if demo_buffer_handle.get_state() == DemoBufferState::Record {
                return Ok(());
            }

            let (notification_type, text) = params;

            let toast = Toast::new();

            let toast = match notification_type {
                1 => toast.kind(egui_toast::ToastKind::Info),
                2 => toast.kind(egui_toast::ToastKind::Success),
                3 => toast.kind(egui_toast::ToastKind::Error),
                4 => toast.kind(egui_toast::ToastKind::Warning),
                _ => toast.kind(egui_toast::ToastKind::Custom(notification_type)),
            }
            .text(text);

            toasts_handle.lock().add(toast);

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    let position = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get(&id) {
                Some(drawer) => Ok([drawer.pos.x, drawer.pos.y]),
                None => {
                    Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )))
                },
            }
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let rectangle = lua_vm
        .create_function(move |_, params: (String, f32, f32)| {
            let (id, desired_x, desired_y) = params;

            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Rectangle(
                            id,
                            NonNaN::<f32>::new(desired_x).unwrap_or_default(),
                            NonNaN::<f32>::new(desired_y).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    let current_position = drawer.pos;

                    let current_color = drawer.color;
                    drawer.drawings.polygons.push(FilledPolygonPoints {
                        points: vec![
                            Vec3::new(current_position.x, current_position.y, 0.),
                            Vec3::new(current_position.x + (desired_x), current_position.y, 0.),
                            Vec3::new(
                                current_position.x + (desired_x),
                                current_position.y + (desired_y),
                                0.,
                            ),
                            Vec3::new(current_position.x, current_position.y + (desired_y), 0.),
                        ],
                        color: current_color,
                    });
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }
            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();
    let demo_buffer_handle = demo_buffer.clone();

    let point_to = lua_vm
        .create_function(move |_, params: (String, f32, f32)| {
            let (id, point_to_x, point_to_y) = params;

            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Rectangle(
                            id,
                            NonNaN::<f32>::new(point_to_x).unwrap_or_default(),
                            NonNaN::<f32>::new(point_to_y).unwrap_or_default(),
                        ));

                        return Ok(());
                    }

                    let drawer_pos = drawer.pos;
                    
                    let dy = drawer_pos.y - point_to_y;
                    let dx = drawer_pos.x - point_to_x;

                    let atan = dy.atan2(dx);

                    drawer.ang = Angle::from_degrees(atan.to_degrees() - 90.);
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    )));
                },
            }

            Ok(())
        })
        .unwrap();

    //Set all the functions in the global handle of the lua runtime
    lua_vm.globals().set("new", new).unwrap();
    lua_vm.globals().set("remove", remove).unwrap();
    lua_vm.globals().set("drawers", drawers).unwrap();
    lua_vm.globals().set("rotate", rotate_drawer).unwrap();
    lua_vm.globals().set("forward", forward).unwrap();
    lua_vm.globals().set("center", center).unwrap();
    lua_vm.globals().set("color", color).unwrap();
    lua_vm.globals().set("print", print).unwrap();
    lua_vm.globals().set("wipe", wipe).unwrap();
    lua_vm.globals().set("exists", exists).unwrap();
    lua_vm.globals().set("enable", enable).unwrap();
    lua_vm.globals().set("disable", disable).unwrap();
    lua_vm.globals().set("fill", fill).unwrap();
    lua_vm.globals().set("notification", notification).unwrap();
    lua_vm.globals().set("position", position).unwrap();
    lua_vm.globals().set("rectangle", rectangle).unwrap();
    lua_vm.globals().set("set_drawer_angle", set_drawer_angle).unwrap();
    lua_vm.globals().set("point_to", point_to).unwrap();
}

#[cfg(target_family = "wasm")]
pub fn init_lua_functions_wasm(
    mut lua_rt: ResMut<LuaRuntime>,
    draw_requester: Res<DrawRequester>,
    drawers_handle: Drawers,
    output_list: Arc<RwLock<SetLenBuffer<ScriptLinePrompts>>>,
    demo_buffer: DemoBuffer<Vec<DemoStep>>,
    toast_handle: Arc<Mutex<Toasts>>,
)
{
    let lua_rt_locked = lua_rt.get();
    let mut lua_rt: &mut piccolo::Lua = &mut *lua_rt_locked.lock();

    lua_rt.enter(|ctx| {
        let demo_buffer_handle = demo_buffer.clone();
        let print = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let arg_value = stack.pop_front();

            if !arg_value.is_nil() {
                let msg = arg_value.to_string();

                if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                    buffer.write().push(DemoStep::Print(msg));

                    return Ok(piccolo::CallbackReturn::Return);
                }

                output_list.write().push(ScriptLinePrompts::Standard(msg));

                Ok(piccolo::CallbackReturn::Return)
            }
            else {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        // Creates a new drawer with the Drawer handle, from a unique handle.
        let new = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let arg_value = stack.pop_front();

            if !arg_value.is_nil() {
                let id = arg_value.to_string();

                if !drawers_clone.contains_key(&id) {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::New(id.clone()));
                    }

                    drawers_clone.insert(id.clone(), Drawer::default());

                    return Ok(piccolo::CallbackReturn::Return);
                }
                else {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                }
            }
            else {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        // Sets the drawer's angle.
        let rotate_drawer = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let args = (stack.pop_front(), stack.pop_front());

            if args.0.is_nil() || args.1.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            // Get params
            let (id, degrees) = (
                args.0.to_string(),
                args.1.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid degree argument.").into())
                })? as f32,
            );

            // Clone the drawers' list handle
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Rotate(
                            id,
                            NonNaN::<f32>::new(degrees).unwrap_or_default(),
                        ));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    // Set the drawer's angle.
                    drawer.ang = Angle::from_degrees(drawer.ang.to_degrees() + degrees);
                },
                None => {
                    // Return the error
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        // Resets the drawers position and angle.
        let center = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let arg = stack.pop_front();

            if arg.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = arg.to_string();

            // Fetch the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Center(id));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    //Reset the drawer's position.
                    drawer.pos = Vec2::default();

                    let drawer_color = drawer.color;

                    //Add the reseted pos to the drawer
                    drawer.drawings.lines.push(LineStrip {
                        points: vec![(Vec3::default(), drawer_color)],
                    });

                    //Reset the drawer's angle.
                    drawer.ang = Angle::from_degrees(90.);
                },
                None => {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        // Sets the color of the drawing
        let color = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let args = (
                stack.pop_front(),
                stack.pop_front(),
                stack.pop_front(),
                stack.pop_front(),
                stack.pop_front(),
            );

            if args.0.is_nil()
                || args.1.is_nil()
                || args.2.is_nil()
                || args.3.is_nil()
                || args.4.is_nil()
            {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            // Get params
            let (id, red, green, blue, alpha) = (
                args.0.to_string(),
                args.1.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid red color argument.").into())
                })? as f32,
                args.2.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid green color argument.").into())
                })? as f32,
                args.3.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid blue color argument.").into())
                })? as f32,
                args.4.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid alpha argument.").into())
                })? as f32,
            );

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Color(
                            id,
                            NonNaN::<f32>::new(red).unwrap_or_default(),
                            NonNaN::<f32>::new(green).unwrap_or_default(),
                            NonNaN::<f32>::new(blue).unwrap_or_default(),
                            NonNaN::<f32>::new(alpha).unwrap_or_default(),
                        ));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    // Set the drawer's color
                    drawer.color = Color::linear_rgba(red, green, blue, alpha);
                },
                None => {
                    // Return the error
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        // Moves the drawer forward by a set amount of units, this makes the drawer draw too.
        let forward = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let args = (stack.pop_front(), stack.pop_front());

            if args.0.is_nil() || args.1.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            // Get params
            let (id, amount) = (
                args.0.to_string(),
                args.1.to_number().ok_or_else(|| {
                    piccolo::Error::Runtime(anyhow::Error::msg("Invalid forward argument.").into())
                })? as f32,
            );

            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Forward(
                            id,
                            NonNaN::<f32>::new(amount).unwrap_or_default(),
                        ));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    // Calculate the difference on the y and x coordinate from its angle.
                    // Get origin
                    let origin = drawer.pos;

                    //Clone the color so we can move it into the lines' list.
                    let drawer_color = drawer.color;

                    // Degrees into radians.
                    let angle_rad = drawer.ang.to_radians();

                    // Forward units
                    let amount_forward = amount;

                    // The new x.
                    let x = origin.x
                        + (amount_forward * floating_point_calculation_error(angle_rad.cos()));
                    // The new y.
                    let y = origin.y
                        + (amount_forward * floating_point_calculation_error(angle_rad.sin()));

                    //Store the new position and the drawer's color if it is enabled
                    if drawer.enabled {
                        drawer
                            .drawings
                            .lines
                            .last_mut()
                            .unwrap()
                            .points
                            .push((Vec3::new(x, y, 0.), drawer_color));
                    }

                    //Set the new drawers position.
                    drawer.pos = Vec2::new(x, y);
                },
                None => {
                    //Reset the drawer's position
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();
        
        let wipe = Callback::from_fn(&ctx, move |_, _, mut stack| {
            if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                buffer.write().push(DemoStep::Wipe);

                return Ok(piccolo::CallbackReturn::Return);
            }

            for mut drawer in drawers_clone.iter_mut() {
                let drawer = drawer.value_mut();

                let mut default_drawings = Drawings::default();
                default_drawings.lines.push(LineStrip {
                    points: vec![(Vec3::new(drawer.pos.x, drawer.pos.y, 0.), Color::WHITE)],
                });
                drawer.drawings = default_drawings;
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();

        let exists = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            let exists = drawers_clone.contains_key(&id);

            stack.push_back(Value::Boolean(exists));

            return Ok(piccolo::CallbackReturn::Return);
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        let remove = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            if drawers_clone.remove(&id).is_none() {
                return Err(anyhow::Error::msg(format!(
                    r#"The drawer with handle "{id}" already exists."#
                ))
                .into());
            }

            if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                buffer.write().push(DemoStep::Remove(id));

                return Ok(piccolo::CallbackReturn::Return);
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let drawers = Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            for drawer in drawers_clone.iter() {
                stack.push_back(Value::String(piccolo::String::from_buffer(&ctx, drawer.key().as_bytes().into())));
            }

            return Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();
        
        let enable = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Enable(id));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    drawer.enabled = true;

                    let tuple = (Vec3::new(drawer.pos.x, drawer.pos.y, 0.), drawer.color);

                    drawer.drawings.lines.push(LineStrip::new(vec![tuple]));
                },
                None => {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        let disable = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Disable(id));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    drawer.enabled = false;
                },
                None => {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });
    
        let drawers_clone = drawers_handle.clone();
        let draw_request_sender = draw_requester.sender.clone();
        let demo_buffer_handle = demo_buffer.clone();

        let fill = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            match drawers_clone.get(&id) {
                Some(selected_drawer) => {
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Fill(id));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    let drawer_lines: Vec<Vec3> = drawers_clone
                        .iter()
                        .flat_map(|pair| {
                            let drawer = pair.value();

                            drawer.drawings.clone().lines.iter().flat_map(|line_strip| {
                                line_strip.points.iter().map(|points| points.0).collect::<Vec<Vec3>>()
                            }).collect::<Vec<Vec3>>()
                        })
                        .collect();

                    let mut lines: Vec<Line> = vec![];

                    for positions in dbg!(drawer_lines).windows(2) {
                        let (min, max) = (positions[0], positions[1]);

                        lines.push(Line::new(min, max));
                    }

                    let mut checked_lines: Vec<Line> = vec![];

                    for (idx, line) in lines.iter().enumerate() {
                        for (current_checked_idx, checked_line) in checked_lines.iter().enumerate() {
                            if idx as isize - 1 != current_checked_idx as isize && checked_lines.len() > 2 {
                                if let Some(intersection_pos) = line.intersects(checked_line) {
                                    let intersected_line_idx = checked_lines.iter().position(|line| line == checked_line).unwrap();
                                    let mut polygon_points: Vec<Coord> = vec![];

                                    polygon_points.push(coord!{x: intersection_pos.x as f64, y: intersection_pos.y as f64});

                                    for poly_line in &checked_lines[intersected_line_idx + 1..idx] {
                                        polygon_points.push(coord! {x: poly_line.max.x as f64, y: poly_line.max.y as f64});
                                    }

                                    let polygon = Polygon::new(LineString::new(polygon_points.clone()), vec![]);

                                    let poly_convex_hull = polygon.convex_hull();

                                    if poly_convex_hull.contains(&point!(x: selected_drawer.pos.x as f64, y: selected_drawer.pos.y as f64)) {
                                        draw_request_sender.send((polygon_points.iter().map(|coord| Vec3::new(coord.x as f32, coord.y as f32, 0.)).collect::<Vec<Vec3>>(), selected_drawer.color, id.clone())).unwrap();
                                    }

                                    break;
                                }
                            }
                        }

                        checked_lines.push(line.clone());
                    }
                },
                None => {
                    return Err(anyhow::Error::msg(format!(r#"The drawer with handle "{id}" already exists."#)).into());
                },
            }

            Ok(piccolo::CallbackReturn::Return)
        });

        let toasts_handle = toast_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();
    
        let notification = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let params = (stack.pop_front(), stack.pop_front());

            if params.0.is_nil() || params.1.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let (notification_type, text) = (params.0.to_integer().ok_or_else(|| {
                piccolo::Error::Runtime(anyhow::Error::msg("Invalid notification code argument.").into())
            })? as u32, params.1.to_string());

            if demo_buffer_handle.get_state() == DemoBufferState::Record {
                return Ok(piccolo::CallbackReturn::Return);
            }

            let toast = Toast::new();

            let toast = match notification_type {
                1 => toast.kind(egui_toast::ToastKind::Info),
                2 => toast.kind(egui_toast::ToastKind::Success),
                3 => toast.kind(egui_toast::ToastKind::Error),
                4 => toast.kind(egui_toast::ToastKind::Warning),
                _ => toast.kind(egui_toast::ToastKind::Custom(notification_type)),
            }
            .text(text);

            toasts_handle.lock().add(toast);

            Ok(piccolo::CallbackReturn::Return)
        });

        let drawers_clone = drawers_handle.clone();

        let position = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let id = stack.pop_front();

            if id.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let id = id.to_string();

            match drawers_clone.get(&id) {
                Some(drawer) => {
                    stack.push_back(Value::Number(drawer.pos.x as f64));
                    stack.push_back(Value::Number(drawer.pos.y as f64));

                    return Ok(piccolo::CallbackReturn::Return);
                },
                None => {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" doesn't exist."#
                    ))
                    .into());
                },
            }
        });

        let drawers_clone = drawers_handle.clone();
        let demo_buffer_handle = demo_buffer.clone();

        let rectangle = Callback::from_fn(&ctx, move |_, _, mut stack| {
            let params = (stack.pop_front(), stack.pop_front(), stack.pop_front());

            if params.0.is_nil() || params.1.is_nil() || params.2.is_nil() {
                return Err(piccolo::Error::Lua(LuaError::from(Value::Nil)));
            }

            let (id, desired_x, desired_y) = (params.0.to_string(), params.1.to_number().ok_or_else(|| {
                piccolo::Error::Runtime(anyhow::Error::msg("Invalid desired x argument.").into())
            })? as f32, params.2.to_number().ok_or_else(|| {
                piccolo::Error::Runtime(anyhow::Error::msg("Invalid desired y argument.").into())
            })? as f32);

            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    if let Some(buffer) =
                        demo_buffer_handle.get_state_if_eq(DemoBufferState::Record)
                    {
                        buffer.write().push(DemoStep::Rectangle(
                            id,
                            NonNaN::<f32>::new(desired_x).unwrap_or_default(),
                            NonNaN::<f32>::new(desired_y).unwrap_or_default(),
                        ));

                        return Ok(piccolo::CallbackReturn::Return);
                    }

                    let current_position = drawer.pos;

                    let current_color = drawer.color;
                    drawer.drawings.polygons.push(FilledPolygonPoints {
                        points: vec![
                            Vec3::new(current_position.x, current_position.y, 0.),
                            Vec3::new(current_position.x + (desired_x), current_position.y, 0.),
                            Vec3::new(
                                current_position.x + (desired_x),
                                current_position.y + (desired_y),
                                0.,
                            ),
                            Vec3::new(current_position.x, current_position.y + (desired_y), 0.),
                        ],
                        color: current_color,
                    });
                },
                None => {
                    return Err(anyhow::Error::msg(format!(
                        r#"The drawer with handle "{id}" already exists."#
                    ))
                    .into());
                },
            }
            Ok(piccolo::CallbackReturn::Return)
        });

        //Set all the functions in the global handle of the lua runtime
        ctx.globals().set(ctx, "new", new).unwrap();
        ctx.globals().set(ctx, "remove", remove).unwrap();
        ctx.globals().set(ctx, "drawers", drawers).unwrap();
        ctx.globals().set(ctx, "rotate", rotate_drawer).unwrap();
        ctx.globals().set(ctx, "forward", forward).unwrap();
        ctx.globals().set(ctx, "center", center).unwrap();
        ctx.globals().set(ctx, "color", color).unwrap();
        ctx.globals().set(ctx, "print", print).unwrap();
        ctx.globals().set(ctx, "wipe", wipe).unwrap();
        ctx.globals().set(ctx, "exists", exists).unwrap();
        ctx.globals().set(ctx, "enable", enable).unwrap();
        ctx.globals().set(ctx, "disable", disable).unwrap();
        ctx.globals().set(ctx, "fill", fill).unwrap();
        ctx.globals().set(ctx, "notification", notification).unwrap();
        ctx.globals().set(ctx, "position", position).unwrap();
        ctx.globals().set(ctx, "rectangle", rectangle).unwrap();
    });
}

#[derive(EnumIter, EnumCount, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallbackType
{
    OnDraw,
    OnInput,
    OnParameterChange,
}

impl Display for CallbackType
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.write_str(match &self {
            CallbackType::OnDraw => "on_draw",
            CallbackType::OnInput => "on_input",
            CallbackType::OnParameterChange => "on_param_change",
        })
    }
}
#[cfg(not(target_family = "wasm"))]
pub fn function_callback(argument: Option<String>, function: Function) -> anyhow::Result<()>
{
    match argument {
        Some(arg) => {
            function.call::<String>(arg)?;
        },
        None => {
            function.call::<()>(())?;
        },
    }

    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq)]
/// Two points which will have a line drawn between them.
pub struct Line
{
    pub min: Vec3,
    pub max: Vec3,
}

pub enum IntersectType
{
    Connected(Vec3),
    Intersected(Vec3),
}

impl Line
{
    pub fn new(min: Vec3, max: Vec3) -> Self
    {
        Self { min, max }
    }

    pub fn intersects(&self, other_line: &Self) -> Option<Vec3>
    {
        if self == other_line {
            return None;
        }

        // Slope of Line 1 (m1) and Line 2 (m2)
        let mut m1 = (self.max.y - self.min.y) / (self.max.x - self.min.x);

        if (self.max.x - self.min.x) == 0. {
            m1 = 0.;
        }

        let mut m2 = (other_line.max.y - other_line.min.y) / (other_line.max.x - other_line.min.x);

        if (other_line.max.x - other_line.min.x) == 0. {
            m2 = 0.;
        }

        if other_line.max == self.min || self.min == other_line.min {
            return Some(Vec3::new(self.min.x, self.min.y, 0.));
        }

        if self.max == other_line.min || self.max == other_line.max {
            return Some(Vec3::new(self.max.x, self.max.y, 0.));
        }

        // Check if the lines are parallel (i.e., have the same slope)
        if m1 == m2 {
            return None;
        }

        // Calculate the y-intercepts (b1 and b2) of the two lines
        let b1 = floating_point_calculation_error(self.min.y - m1 * self.min.x);
        let b2 = floating_point_calculation_error(other_line.min.y - m2 * other_line.min.x);

        // Calculate the x-coordinate of the intersection point
        let intersection_x = (b2 - b1) / (m1 - m2);

        // Calculate the y-coordinate of the intersection point using either line's equation
        let intersection_y = m1 * intersection_x + b1;
        let intersection_point = Vec3::new(intersection_x, intersection_y, 0.);

        if !self.is_point_on_segment(&intersection_point)
            || !other_line.is_point_on_segment(&intersection_point)
        {
            return None;
        }

        Some(Vec3::new(intersection_x, intersection_y, 0.))
    }

    fn is_point_on_segment(&self, point: &Vec3) -> bool
    {
        let within_x_bounds =
            point.x >= self.min.x.min(self.max.x) && point.x <= self.min.x.max(self.max.x);
        let within_y_bounds =
            point.y >= self.min.y.min(self.max.y) && point.y <= self.min.y.max(self.max.y);
        within_x_bounds && within_y_bounds
    }
}

pub fn floating_point_calculation_error(float: f32) -> f32
{
    if float.abs() < 0.000001 {
        0.0
    }
    else {
        float
    }
}
