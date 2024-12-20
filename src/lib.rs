use std::{
    ops::{Deref, DerefMut},
    sync::{mpsc::{channel, Receiver, Sender}, Arc},
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
use geo::{
    coord, point, Contains, ConvexHull, Coord, Intersects, LineString, Point, Polygon
};
use mlua::{Error, Lua};
use parking_lot::{Mutex, RwLock};
use ui::fill_from_points;

#[derive(Resource, Clone)]
pub struct DrawRequester { 
    pub receiver: Arc<Mutex<Receiver<(Vec<Vec3>, Color, String)>>>,
    pub sender: Arc<Sender<(Vec<Vec3>, Color, String)>>
}

impl Default for DrawRequester
{
    fn default() -> Self
    {
        let (sender, receiver) = channel::<(Vec<Vec3>, Color, String)>();
        Self {
            sender: Arc::new(sender), receiver: Arc::new(Mutex::new(receiver))
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
    pub drawings: Vec<DrawingType>,

    /// The color of the Drawer.
    pub color: Color,
}

#[derive(Clone, Debug)]
pub enum DrawingType {
    Line(LineStrip),
    Polygon((Vec<Vec3>, Color)),
}

impl Default for Drawer
{
    fn default() -> Self
    {
        Self {
            enabled: true,
            pos: Vec2::default(),
            ang: Angle::from_degrees(90.),
            drawings: vec![DrawingType::Line(LineStrip {
                points: vec![(Vec3::default(), Color::WHITE)],
            })],
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
            if !drawers_clone.contains_key(&id) {
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

                    let drawer_color =  drawer.color.clone();

                    //Add the reseted pos to the drawer
                    drawer.drawings.push(DrawingType::Line(LineStrip { points: vec![(Vec3::default(), drawer_color)] }));

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

                    //Clone the color so we can move it into the lines' list.
                    let drawer_color = drawer.color;

                    // Degrees into radians.
                    let angle_rad = drawer.ang.to_radians();

                    // Forward units
                    let transformation_factor = amount;

                    // The new x.
                    let x = origin.x
                        + transformation_factor * floating_point_calculation_error(angle_rad.cos());
                    // The new y.
                    let y = origin.y
                        + transformation_factor * floating_point_calculation_error(angle_rad.sin());

                    //Store the new position and the drawer's color if it is enabled
                    if drawer.enabled {
                        drawer
                            .drawings
                            .push(DrawingType::Line(LineStrip { points: vec![(Vec3::new(x, y, 0.), drawer_color)] }));
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

    let wipe = lua_vm
        .create_function(move |_, _: ()| {
            for mut drawer in drawers_clone.iter_mut() {
                let drawer = drawer.value_mut();

                drawer.drawings = vec![DrawingType::Line(LineStrip::new(vec![(
                    Vec3::new(drawer.pos.x, drawer.pos.y, 0.),
                    Color::WHITE, 
                )]))];
            }

            Ok(())
        })
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    let exists = lua_vm
        .create_function(move |_, id: String| Ok(drawers_clone.contains_key(&id)))
        .unwrap();

    let drawers_clone = drawers_handle.clone();

    let remove = lua_vm
        .create_function(move |_, id: String| {
            if drawers_clone.remove(&id).is_none() {
                return Err(Error::RuntimeError(format!(
                    "The drawer with handle {id} doesn't exist."
                )));
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

    let enable = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
                    drawer.enabled = true;

                    let tuple = (Vec3::new(drawer.pos.x, drawer.pos.y, 0.), drawer.color);

                    drawer.drawings.push(DrawingType::Line(LineStrip::new(vec![tuple])));
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

    let disable = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get_mut(&id) {
                Some(mut drawer) => {
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

    let fill = lua_vm
        .create_function(move |_, id: String| {
            match drawers_clone.get(&id) {
                Some(selected_drawer) => {
                    let drawer_lines: Vec<Vec3> = drawers_clone
                        .iter()
                        .flat_map(|pair| {
                            let drawer = pair.value();
                            
                            drawer.drawings.clone().iter().flat_map(|drawing| {
                                match drawing {
                                    DrawingType::Line(line_strip) => {
                                        line_strip.points.iter().map(|points| points.0).collect::<Vec<Vec3>>()
                                    },
                                    DrawingType::Polygon(points) => {
                                        points.0.clone()
                                    },
                                }
                            }).collect::<Vec<_>>()
                        })
                        .collect();

                    let mut lines: Vec<Line> = vec![];

                    for positions in drawer_lines.windows(2) {
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

                                    for poly_line in &checked_lines[intersected_line_idx..idx] {
                                        polygon_points.push(coord! {x: poly_line.max.x as f64, y: poly_line.max.y as f64});
                                    }

                                    let polygon = Polygon::new(LineString::new(polygon_points.clone()), vec![]);

                                    let poly_convex_hull = polygon.convex_hull();

                                    if poly_convex_hull.contains(&point!(x: selected_drawer.pos.x as f64, y: selected_drawer.pos.y as f64)) {
                                        draw_request_sender.send((polygon_points.iter().map(|coord| Vec3::new(coord.x as f32, coord.y as f32, 0.)).collect::<Vec<Vec3>>(), selected_drawer.color, id.clone())).unwrap();
                                    }

                                    checked_lines.clear();
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

    //Set all the functions in the global handle of the lua runtime
    lua_vm.globals().set("new", new_drawer).unwrap();
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

        Some(Vec3::new(intersection_x, intersection_y, 0.))
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
