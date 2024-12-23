use std::{
    fmt::Display, ops::{Deref, DerefMut}, sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    }
};

use bevy::{
    asset::RenderAssetUsages,
    color::Color,
    math::{Vec2, Vec3, Vec4},
    prelude::{Component, Mesh, Res, ResMut, Resource},
    render::mesh::PrimitiveTopology,
    text::cosmic_text::Angle,
};
pub mod ui;
use dashmap::DashMap;
use egui_toast::{Toast, Toasts};
use geo::{coord, point, Contains, ConvexHull, Coord, LineString, Polygon};
use mlua::{Error, Lua};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct DemoBuffer<T>
{
    pub resource: Arc<RwLock<T>>,
    pub buffer_state: Arc<RwLock<DemoBufferState>>,
}

#[derive(PartialEq, Default, serde::Serialize, serde::Deserialize, Clone, Copy)]
pub enum DemoBufferState {
    Playback,
    Record,

    #[default]
    None,
}

impl<T> DemoBuffer<T>
{
    pub fn new(inner: T) -> Self
    {
        Self {
            // Create an `Arc<Mutex<T>>` for the buffer.
            resource: Arc::new(RwLock::new(inner)),
            // Initalize the buffer state with Default (None).
            buffer_state: Arc::new(RwLock::new(DemoBufferState::default())),
        }
    }

    pub fn set_buffer(&self, buffer: T) {
        *self.resource.write() = buffer;
    }

    pub fn get_state_if_eq(&self, buffer_state: DemoBufferState) -> Option<&RwLock<T>>
    {
        if *self.buffer_state.read() == buffer_state {
            return Some(&self.resource);
        }

        None
    }

    pub fn get_state(&self) -> DemoBufferState {
        *self.buffer_state.read()
    }

    pub fn set_state(&self, state: DemoBufferState) {
        *self.buffer_state.write() = state;
    }
}

/// The DemoInstance is used to store demos of scripts.
/// These contain a script identifier, so that we can notify the user if their code has changed since the last demo recording.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DemoInstance
{
    pub demo_steps: Vec<DemoStep>,
    pub script_identifier: String,
}

/// The items of this enum contain the functions a user can call on their turtles.
/// When recording a demo these are stored and can later be playbacked.
/// All of the arguments to the functions are contained in the enum variants.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum DemoStep
{
    New(String),
    Center(String),
    Forward(String, f32),
    Rotate(String, f32),
    Color(String, f32, f32, f32, f32),
    Wipe,
    Remove(String),
    Disable(String),
    Enable(String),
    Fill(String),
    Rectangle(String, f32, f32),
}

impl DemoStep {
    pub fn execute_lua_function(&self, lua_rt: ResMut<LuaRuntime>) -> Result<(), Error> {
        match self {
            DemoStep::New(id) => {
                lua_rt.load(format!(r#"new("{id}")"#)).exec()
            },
            DemoStep::Center(id) => {
                lua_rt.load(format!(r#"center("{id}")"#)).exec()

            },
            DemoStep::Forward(id, amnt) => {
                lua_rt.load(format!(r#"forward("{id}", {amnt})"#)).exec()

            },
            DemoStep::Rotate(id, amnt) => {
                lua_rt.load(format!(r#"rotate("{id}", {amnt})"#)).exec()

            },
            DemoStep::Color(id, r, g, b, a) => {
                lua_rt.load(format!(r#"new("{id}", {r}, {g}, {b}, {a})"#)).exec()

            },
            DemoStep::Wipe => {
                lua_rt.load(format!(r#"wipe()"#)).exec()

            },
            DemoStep::Remove(id) => {
                lua_rt.load(format!(r#"remove("{id}")"#)).exec()

            },
            DemoStep::Disable(id) => {
                lua_rt.load(format!(r#"disable("{id}")"#)).exec()

            },
            DemoStep::Enable(id) => {
                lua_rt.load(format!(r#"enable("{id}")"#)).exec()

            },
            DemoStep::Fill(id) => {
                lua_rt.load(format!(r#"fill("{id}")"#)).exec()

            },
            DemoStep::Rectangle(id, desired_x, desired_y) => {
                lua_rt.load(format!(r#"rectangle("{id}", {desired_x}, {desired_y})"#)).exec()
            },
        }
    }
}

impl Display for DemoStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            &match self {
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
                DemoStep::Wipe => {
                    format!(r#"wipe()"#)
                },
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
            }
        )
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
pub struct DrawerMesh;

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

/// A list of points that will have polygon created from them
#[derive(Debug, Clone, Default)]
pub struct FilledPolygonPoints
{
    pub points: Vec<Vec3>,
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

/// The information of the Drawer
#[derive(Resource, Debug)]
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

#[derive(Clone, Debug)]
pub struct Drawings
{
    pub lines: Vec<LineStrip>,
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
pub fn init_lua_functions(
    lua_rt: ResMut<LuaRuntime>,
    draw_requester: Res<DrawRequester>,
    drawers_handle: Drawers,
    output_list: Arc<RwLock<Vec<ScriptLinePrompts>>>,
    demo_buffer: DemoBuffer<Vec<DemoStep>>,
    toast_handle: Arc<Mutex<Toasts>>,
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
                    "The drawer with handle {id} already exists."
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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Rotate(id, degrees));

                        return Ok(());
                    }

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
    let demo_buffer_handle = demo_buffer.clone();

    // Resets the drawers position and angle.
    let center = lua_vm
        .create_function(move |_, id: String| {
            // Fetich the drawer's handle.
            let drawer_handle = drawers_clone.get_mut(&id);

            match drawer_handle {
                Some(mut drawer) => {
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
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
                        "The drawer with handle {id} doesn't exist."
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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer
                            .write()
                            .push(DemoStep::Color(id, red, green, blue, alpha));

                        return Ok(());
                    }

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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Forward(id, amount));

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
                        "The drawer with handle {id} doesn't exist."
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
                    "The drawer with handle {id} doesn't exist."
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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Enable(id));

                        return Ok(());
                    }

                    drawer.enabled = true;

                    let tuple = (Vec3::new(drawer.pos.x, drawer.pos.y, 0.), drawer.color);

                    drawer.drawings.lines.push(LineStrip::new(vec![tuple]));
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer.write().push(DemoStep::Disable(id));

                        return Ok(());
                    }

                    drawer.enabled = false;
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
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
                        "The drawer with handle {id} doesn't exist."
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
                Some(drawer) => {
                    return Ok((drawer.pos.x, drawer.pos.y));
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
                    )));
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
                    if let Some(buffer) = demo_buffer_handle.get_state_if_eq(DemoBufferState::Record) {
                        buffer
                            .write()
                            .push(DemoStep::Rectangle(id, desired_x, desired_y));

                        return Ok(());
                    }

                    let current_position = drawer.pos.clone();

                    let current_color = drawer.color;
                    drawer.drawings.polygons.push(FilledPolygonPoints {
                        points: vec![
                            Vec3::new(current_position.x, current_position.y, 0.),
                            Vec3::new(
                                current_position.x + (desired_x - current_position.x),
                                current_position.y,
                                0.,
                            ),
                            Vec3::new(
                                current_position.x + (desired_x - current_position.x),
                                current_position.y + (desired_y - current_position.y),
                                0.,
                            ),
                            Vec3::new(
                                current_position.x,
                                current_position.y + (desired_y - current_position.y),
                                0.,
                            ),
                        ],
                        color: current_color,
                    });
                },
                None => {
                    return Err(Error::RuntimeError(format!(
                        "The drawer with handle {id} doesn't exist."
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
