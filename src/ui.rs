use bevy::prelude::{Res, ResMut};
use bevy_egui::{
    egui::{self, vec2, Color32, Key, Pos2, RichText, ScrollArea, TextEdit, UiBuilder, Window},
    EguiContexts,
};
use chrono::Local;
use dashmap::DashMap;
use egui_commonmark::{commonmark_str, CommonMarkCache};
use miniz_oxide::{deflate::CompressionLevel, inflate::decompress_to_vec};
#[cfg(not(target_family = "wasm"))]
use mlua::{Function, IntoLua};
use serde::Deserialize;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Arc,
};
use strum::IntoEnumIterator;

use parking_lot::{Mutex, RwLock};

#[cfg(not(target_family = "wasm"))]
use crate::LuaRuntime;

#[cfg(target_family = "wasm")]
use crate::{Angle, FilledPolygonPoints, LineStrip, Drawer};
#[cfg(target_family = "wasm")]
use bevy::{color::Color, math::{Vec3, Vec2}};

use crate::{
    CallbackType, DemoBuffer, DemoBufferState, DemoInstance, DemoStep, Drawers, ScriptLinePrompts, DEMO_FILE_EXTENSION, PROJECT_FILE_EXTENSION
};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use bevy::prelude::Resource;
use egui_tiles::Tiles;
use egui_toast::{Toast, Toasts};

/// This struct stores the UI's state, and is initalized from a save file.
#[derive(Resource, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState
{
    /// Should the manager panel open.
    pub manager_panel: bool,

    /// Should the command panel open.
    pub command_panel: bool,

    /// Egui notifications
    #[serde(skip)]
    pub toasts: Arc<Mutex<Toasts>>,

    /// This text buffer is used when renaming an existing script.
    #[serde(skip)]
    pub rename_buffer: Arc<Mutex<String>>,

    /// This text buffer is used when creating a new script.
    #[serde(skip)]
    pub name_buffer: Arc<Mutex<String>>,

    /// The buffer which is used to store the text entered into the command line.
    #[serde(skip)]
    pub command_line_buffer: String,

    /// The manager panel's tab state.
    #[serde(skip)]
    pub item_manager: egui_tiles::Tree<ManagerPane>,

    /// The command line outputs / inputs.
    /// These are displayed to the user.
    #[serde(skip)]
    pub command_line_outputs: Arc<RwLock<Vec<ScriptLinePrompts>>>,

    /// The commands entered in the command line.
    #[serde(skip)]
    pub command_line_inputs: VecDeque<String>,

    /// This is used to track the command line history's index.
    #[serde(skip)]
    pub command_line_input_index: usize,

    /// This DashMap contains the deleted scripts.
    /// Scripts deleted from there pernament.
    pub rubbish_bin: Arc<Mutex<Vec<RubbishBinItem>>>,

    /// The CommonMarkCache is a cache which stores data about the documentation displayer widget.
    /// We need this to be able to display the documentation.
    #[serde(skip)]
    pub common_mark_cache: CommonMarkCache,

    /// If the Documentation window is open.
    pub documentation_window: bool,

    /// This field is used to store demos, which can be playbacked later.
    pub demos: Arc<Mutex<Vec<DemoInstance>>>,

    /// This demo buffer is used when recording a demo for a script.
    /// If the demo is accessible a recording can't be started as it is only available when there is an ongoing recording.
    /// Being accessible indicates that the lua runtime's scripts can write to the underlying mutex.
    /// If the demo recording is stopped the buffer is cleared and accessibility is set to false.
    #[serde(skip)]
    pub demo_buffer: DemoBuffer<Vec<DemoStep>>,

    /// The list of scripts the project contains.
    pub scripts: Arc<Mutex<Vec<ScriptInstance>>>,

    /// The demos' rename text buffer.
    pub demo_rename_text_buffer: Arc<Mutex<String>>,
}

impl Default for UiState
{
    fn default() -> Self
    {
        Self {
            command_panel: {
                // If we are targetting the wasm platform these panels should be automaticly enabled
                cfg!(target_family = "wasm")
            },
            manager_panel: {
                // If we are targetting the wasm platform these panels should be automaticly enabled
                cfg!(target_family = "wasm")
            },
            item_manager: {
                let mut tiles = Tiles::default();
                let tileids = vec![
                    tiles.insert_pane(ManagerPane::EntityManager),
                    tiles.insert_pane(ManagerPane::ScriptManager),
                    tiles.insert_pane(ManagerPane::DemoManager),
                    tiles.insert_pane(ManagerPane::RubbishBin),
                ];

                egui_tiles::Tree::new("manager_tree", tiles.insert_tab_tile(tileids), tiles)
            },
            toasts: Arc::new(Mutex::new(Toasts::new())),
            rename_buffer: Arc::new(parking_lot::Mutex::new(String::new())),
            name_buffer: Arc::new(parking_lot::Mutex::new(String::new())),
            command_line_outputs: Arc::new(RwLock::new(vec![])),
            command_line_buffer: String::new(),
            command_line_inputs: VecDeque::new(),
            command_line_input_index: 0,
            rubbish_bin: Arc::new(Mutex::new(vec![])),
            common_mark_cache: CommonMarkCache::default(),
            documentation_window: false,
            demos: Arc::new(Mutex::new(vec![])),
            demo_buffer: DemoBuffer::new(vec![]),
            scripts: {
                let mut script_list = vec![];

                // In the wasm-environment we should automaticly be adding pre-set demos
                if cfg!(target_family = "wasm") {
                    //Rectangle script
                    script_list.push(ScriptInstance::new(
                        String::from("rectangle"),
                        String::from(
                            r#"if not exists("drawer1") then
    new("drawer1")
end

center("drawer1")

rectangle("drawer1", 100.0, 100.0)
                    "#,
                        ),
                    ));

                    //Circle
                    script_list.push(ScriptInstance::new(
                        String::from("circle"),
                        String::from(
                            r#"if not exists("drawer1") then
    new("drawer1")
end
                    
for i=0, 360, 1 do
    forward("drawer1", 1)
    rotate("drawer1", 1)
end

rotate("drawer1", 10)
"#,
                        ),
                    ));
                    //Line
                    script_list.push(ScriptInstance::new(String::from("line"), String::from(
                        r#"if not exists("drawer1") then
    new("drawer1")
end

forward("drawer1", 100)
                        "#
                    )));
                }

                Arc::new(Mutex::new(script_list))
            },
            demo_rename_text_buffer: Arc::new(Mutex::new(String::new())),
        }
    }
}

/// The manager panel's tabs.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub enum ManagerPane
{
    /// The scripts tab.
    ScriptManager,
    /// The entity manager tab.
    #[default]
    EntityManager,
    /// Demo manager tab.
    DemoManager,
    /// Rubbish bin tab.
    RubbishBin,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum RubbishBinItem
{
    Script(ScriptInstance),
    Demo(DemoInstance),
}

/// The manager panel's inner behavior, the data it contains, this can be used to share data over to the tabs from the main ui.
pub struct ManagerBehavior
{
    /// The [`mlua::Lua`] runtime handle, this can be used to run code on.
    /// This field is only enabled in non-wasm enviroments.
    #[cfg(not(target_family = "wasm"))]
    lua_runtime: LuaRuntime,

    /// [`Toasts`] are used to display notifications to the user.
    toasts: Arc<Mutex<Toasts>>,

    /// The field is used to display the current number of drawers.
    drawers: Drawers,

    /// This text buffer is used when creating a new script.
    name_buffer: Arc<Mutex<String>>,

    /// This text buffer is used when renaming an existing script.
    rename_buffer: Arc<Mutex<String>>,

    /// This DashMap contains the deleted scripts.
    /// Scripts deleted from there pernament.
    rubbish_bin: Arc<Mutex<Vec<RubbishBinItem>>>,

    /// This field is used to store demos, which can be playbacked later.
    demos: Arc<Mutex<Vec<DemoInstance>>>,

    /// This buffer is used when recording a demo, or when playbacking one.
    /// The [`DemoBuffer`] can have multiple "states" depending on what its used for to create custom behavior.
    demo_buffer: DemoBuffer<Vec<DemoStep>>,

    /// The list of scripts the project contains.
    scripts: Arc<Mutex<Vec<ScriptInstance>>>,

    /// The demos' rename text buffer.
    demo_rename_text_buffer: Arc<Mutex<String>>,
}

/// A [`ScriptInstance`] holds information about one script.
/// It contains the script's name, and the script itself.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct ScriptInstance
{
    /// Is the script running.
    #[serde(skip)]
    pub is_running: bool,
    /// The name of the script.
    pub name: String,
    /// The script itself.
    pub script: String,

    /// The list of callback this script has.
    /// This field gets updated every script start
    /// Callbacks are disabled in a wasm environment as the lua virtual machine is not available when compiled to WebAssembly.
    #[serde(skip)]
    #[cfg(not(target_family = "wasm"))]
    pub callbacks: HashMap<CallbackType, Function>,
}

impl ScriptInstance
{
    /// Creates a new [`ScriptInstance`].
    pub fn new(name: String, script: String) -> Self
    {
        Self {
            is_running: false,
            name,
            script,
            #[cfg(not(target_family = "wasm"))]
            callbacks: HashMap::new(),
        }
    }
}

/// Implement tiles for the ManagerBehavior so that it can be dsiplayed.
impl egui_tiles::Behavior<ManagerPane> for ManagerBehavior
{
    fn pane_ui(
        &mut self,
        ui: &mut bevy_egui::egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut ManagerPane,
    ) -> egui_tiles::UiResponse
    {
        match pane {
            ManagerPane::ScriptManager => {
                // Create inner margin to make it look nicer
                ui.allocate_space(vec2(ui.available_width(), 2.));

                // Allocate ui for the script adding menu button
                ui.allocate_ui(vec2(ui.available_width(), ui.min_size().y), |ui| {
                    // Disable the ui if its in a wasm environment as all demos are pre programmed.
                    #[cfg(target_family = "wasm")]
                    ui.disable();

                    // Lock the name buffer so that it can be accessed later, without a deadlock
                    let name_buffer = &mut *self.name_buffer.lock();

                    // Add script menu button widget
                    ui.menu_button("Add Script", |ui| {
                        // Allocate ui for the script creating by name
                        // We allocate 0. for the height so that it only takes how much it needs and nothing more.
                        ui.allocate_ui(vec2(ui.available_width(), 0.), |ui| {
                            // Horizontally center the inner widgets of the menu button
                            ui.horizontal_centered(|ui| {
                                // Only enable the add button if the name buffer isnt empty
                                ui.add_enabled_ui(!name_buffer.is_empty(), |ui| {
                                    // Create the add button
                                    let add_button = ui.button("Add");

                                    if add_button.clicked() {
                                        // Lock the script's list handle
                                        let mut script_handle = self.scripts.lock();

                                        // Insert new ScriptInstance
                                        script_handle.push(ScriptInstance::new(
                                            name_buffer.clone(),
                                            String::new(),
                                        ));

                                        // Clear the name buffer so that wehn creating a new script the text wont be there anymore
                                        name_buffer.clear();

                                        // Close menu after adding a new scirpt
                                        ui.close_menu();
                                    }
                                });

                                // Create the text editor widget so that the buffer can be edited
                                ui.add(TextEdit::singleline(name_buffer).hint_text("Name"));
                            });
                        });

                        // Draw separator line
                        ui.separator();

                        // Create import from file button
                        #[cfg(not(target_family = "wasm"))]
                        if ui.button("Import from File").clicked() {
                            // If the user has selected a file patter match the path
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                // Pattern math reading a String out from it
                                match fs::read_to_string(&path) {
                                    Ok(file_content) => {
                                        // If we could read the file content load the read file into a script instance which we insert into the list
                                        self.scripts.lock().push(ScriptInstance::new(
                                            path.file_name()
                                                .unwrap_or_default()
                                                .to_str()
                                                .unwrap_or_default()
                                                .to_string(),
                                            file_content,
                                        ));

                                        // Close menu if we could read successfully
                                        ui.close_menu();
                                    },
                                    Err(_err) => {
                                        // Display any kind of error to a notification
                                        self.toasts.lock().add(
                                            Toast::new().kind(egui_toast::ToastKind::Error).text(
                                                format!(
                                                    "Reading from file ({}) failed: {_err}",
                                                    path.display()
                                                ),
                                            ),
                                        );
                                    },
                                }
                            }
                        }
                    });
                });

                //Draw separator line
                ui.separator();

                //Create scroll area for listing the scripts.
                ScrollArea::both()
                    // Set max height
                    .max_height(ui.available_height() - 200.)
                    // Turn off auto shrinking
                    .auto_shrink([false, false])
                    // Show the Ui inside the Scroll Area
                    .show(ui, |ui| {
                        // This indicates the current script's position in the list.
                        let mut script_idx = 0;

                        // Iter over the scripts, and move them to the rubbish bin if the closure return false.
                        self.scripts.lock().retain_mut(|script_instance| {
                            // Define what we should do with this entry.
                            let mut should_keep = true;

                            // Create a horizonal part of the ui
                            ui.horizontal(|ui| {
                                // Dispaly the script's name
                                ui.label(script_instance.name.clone());

                                // Only enable the playing of the script if there arent any demo's being replayed or recorded.
                                ui.add_enabled_ui(
                                    self.demo_buffer.get_state() == DemoBufferState::None,
                                    |ui| {
                                        // Create the run button
                                        match script_instance.is_running {
                                            false => {
                                                if ui.button("Run").clicked() {
                                                    script_instance.is_running = true;

                                                    // Check if this is not the wasm build, as we can only enable the lua runtime in non-wasm environments
                                                    #[cfg(not(target_family = "wasm"))]
                                                    {
                                                        // Run the script
                                                        // Pattern match an error and display it as a notification
                                                        if let Err(err) = self
                                                        .lua_runtime
                                                        // Load the script as a string into the lua runtime
                                                        .load(script_instance.script.to_string())
                                                        // Execute the loaded string
                                                        .exec()
                                                        {
                                                            // Add the error into the toasts if it returned an error
                                                            self.toasts.lock().add(
                                                                Toast::new()
                                                                    .kind(egui_toast::ToastKind::Error)
                                                                    .text(err.to_string()),
                                                            );

                                                            script_instance.is_running = false;
                                                            return;
                                                        };

                                                        for callback_type in CallbackType::iter() {
                                                            if let Ok(function) = self.lua_runtime.globals().get::<Function>(callback_type.to_string()) {
                                                                script_instance.callbacks.insert(callback_type, function);
                                                            }
                                                        }

                                                        // If there were no callbacks we can reset the state since nothing is getting called by the app at runtime
                                                        if script_instance.callbacks.is_empty() {
                                                            script_instance.is_running = false;
                                                        }
                                                    }

                                                    //Check if this is a wasm build, so that when the button is pressed it will play the written demo
                                                    #[cfg(target_family = "wasm")]
                                                    {
                                                        // The first script will draw a rectangle.
                                                        if script_idx == 0 {
                                                            if self.drawers.get("drawer1").is_none() {
                                                                self.drawers.insert(String::from("drawer1"), Drawer::default());
                                                            }
    
                                                            if let Some(mut drawer) = self.drawers.get_mut("drawer1") {
                                                                drawer.ang = Angle::from_degrees(90.);
                                                                drawer.pos = Vec2::default();

                                                                drawer.drawings.polygons.push(FilledPolygonPoints {
                                                                    points: vec![
                                                                        Vec3::new(0., 0., 0.),
                                                                        Vec3::new(
                                                                            100.,
                                                                            0.,
                                                                            0.,
                                                                        ),
                                                                        Vec3::new(
                                                                            100.,
                                                                            100.,
                                                                            0.,
                                                                        ),
                                                                        Vec3::new(
                                                                            0.,
                                                                            100.,
                                                                            0.,
                                                                        ),
                                                                    ],
                                                                    color: Color::WHITE,
                                                                });
                                                            }
                                                            
                                                        }
                                                        // The second script will draw a circle and rotate the drawer with 10 degrees.
                                                        else if script_idx == 1 {
                                                            if self.drawers.get("drawer1").is_none() {
                                                                self.drawers.insert(String::from("drawer1"), Drawer::default());
                                                            }
    
                                                            if let Some(mut drawer) = self.drawers.get_mut("drawer1") {
                                                                let mut circle_positions = vec![(Vec3::new(drawer.pos.x, drawer.pos.y, 0.), Color::WHITE)];
    
                                                                // `i` counts as the current angle
                                                                for i in drawer.ang.to_degrees() as i32..drawer.ang.to_degrees() as i32 + 360 {
                                                                    let radians = (i as f32).to_radians();
    
                                                                    circle_positions.push((Vec3::new(circle_positions.last().unwrap().0.x + (1. * radians.cos()), circle_positions.last().unwrap().0.y + (1. * radians.sin()), 0.), Color::WHITE));
                                                                }
    
                                                                drawer.drawings.lines.push(LineStrip::new(circle_positions));
    
                                                                drawer.ang = Angle::from_degrees(drawer.ang.to_degrees() + 10.);
                                                            }
                                                        }
                                                        else if script_idx == 2 {
                                                            if self.drawers.get("drawer1").is_none() {
                                                                self.drawers.insert(String::from("drawer1"), Drawer::default());
                                                            }
    
                                                            if let Some(mut drawer) = self.drawers.get_mut("drawer1") {
                                                                let angle_rad = drawer.ang.to_radians();
                                                                let origin = drawer.pos;

                                                                // Forward units
                                                                let amount_forward = 100.;

                                                                // The new x.
                                                                let x = origin.x
                                                                    + (amount_forward * angle_rad.cos());
                                                                // The new y.
                                                                let y = origin.y
                                                                    + (amount_forward * angle_rad.sin());

                                                                drawer.pos = Vec2::new(x, y);
                                                                drawer.drawings.lines.push(LineStrip::new(vec![(Vec3::new(origin.x, origin.y, 0.), Color::WHITE), (Vec3::new(x, y, 0.), Color::WHITE)]))
                                                            }
                                                        }
                                                        script_instance.is_running = false;
                                                    }
                                                }
                                            },
                                            true => {
                                                if ui.button("Stop").clicked() {
                                                    script_instance.is_running = false;
                                                }
                                            },
                                        }
                                    },
                                );
                                
                                // Create a new ui part with a different id to avoid id collisions
                                ui.push_id(script_instance.name.clone(), |ui| {
                                    // Create the settings collapsing button
                                    ui.collapsing("Settings", |ui| {
                                        // Display the Edit button, and if clicked display the code editor.
                                        // Only enable it if it isnt running yet.
                                        ui.add_enabled_ui(!script_instance.is_running, |ui| {
                                            ui.menu_button("Edit", |ui| {
                                                // Fetch the code theme from context
                                                let theme =
                                            egui_extras::syntax_highlighting::CodeTheme::from_memory(
                                                ui.ctx(),
                                                ui.style(),
                                            );
    
                                            // Create a layouther to display or text correctly
                                                let mut layouter =
                                                    |ui: &egui::Ui, string: &str, wrap_width: f32| {
                                                        let mut layout_job =
                                                            egui_extras::syntax_highlighting::highlight(
                                                                ui.ctx(),
                                                                ui.style(),
                                                                &theme,
                                                                string,
                                                                "lua",
                                                            );
                                                        layout_job.wrap.max_width = wrap_width;
                                                        ui.fonts(|f| f.layout_job(layout_job))
                                                    };
    
                                                // Create a ScrollArea to be able to display / edit more text
                                                ScrollArea::both().show(ui, |ui| {
                                                    // If this is open in a wasm environment we should allow the modification of demos as they're pre programmed
                                                    ui.disable();

                                                    // Add the text editor with the custom layouter to the ui
                                                    ui.add(
                                                        TextEdit::multiline(
                                                            // Mutable script reference
                                                            &mut script_instance.script,
                                                        )
                                                        // Code editor
                                                        .code_editor()
                                                        // Add the custom layouter
                                                        .layouter(&mut layouter),
                                                    );
                                                });
                                            });
                                        });

                                        
                                        // Add the delete button so that the script can be deleted
                                        ui.add_enabled_ui(!cfg!(target_family = "wasm"), |ui| {
                                            if ui.button("Delete").clicked() {
                                                // Flag the script as to be deleted
                                                should_keep = false;
    
                                                //Insert the script into the rubbish bin
                                                self.rubbish_bin.lock().push(RubbishBinItem::Script(
                                                    script_instance.clone(),
                                                ));
                                            }
                                        });

                                        // Display the rename menu button
                                        let rename_menu = ui.menu_button("Rename Script", |ui| {
                                            // This text editor uses the rename_buffer to store the currently entered text in the buffer
                                            ui.text_edit_singleline(
                                                &mut *self.rename_buffer.lock(),
                                            );

                                            // Add the rename button to the ui
                                            // If clicked the buffer's contains will be loaded into the script's name buffer.
                                            if ui.button("Rename").clicked() {
                                                // Lock the rename buffer
                                                let name_buffer = &*self.rename_buffer.lock();

                                                // Modify the script instance's name
                                                script_instance.name = name_buffer.clone();
                                            }
                                        });

                                        // If the rename menu is clicked clear the buffer
                                        if rename_menu.response.clicked() {
                                            // Clear the buffer
                                            *self.rename_buffer.lock() =
                                                script_instance.name.clone();
                                        }
                                        
                                        // Check if the target is not wasm as File exporting is not available in wasm 
                                        #[cfg(not(target_family = "wasm"))] {
                                            // Add the Export as File button
                                            if ui.button("Export as File").clicked() {
                                                // If the user has selected a place to save the file pattern match that path, otherwise dont do anything
                                                if let Some(path) = rfd::FileDialog::new()
                                                    // Set the file's name in the file dialog
                                                    .set_file_name(script_instance.name.clone())
                                                    // Add a filter to the file extiension
                                                    .add_filter("Lua", &["lua"])
                                                    //Select the type of FileDialog
                                                    .save_file()
                                                {
                                                    // Write the text to the path
                                                    fs::write(path, script_instance.script.clone())
                                                        .unwrap();
                                                }
                                            }
                                        }

                                        #[cfg(target_family = "wasm")] {
                                            ui.add_enabled_ui(false, |ui| {
                                                let _ = ui.button("Export as File").on_disabled_hover_text(RichText::from("File handling is not supported in WASM.").color(Color32::RED));
                                            });
                                        }

                                        // Draw a separator
                                        ui.separator();

                                        // Add the Create demo button
                                        #[cfg(not(target_family = "wasm"))]
                                        if ui.button("Create Demo").clicked() {
                                            //Store current drawers and canvas
                                            let current_drawer_canvas =
                                                Drawers(Arc::new(DashMap::clone(&self.drawers.0)));

                                            //Clear the canvas, so that the demo creator has a clear canvas
                                            self.drawers.clear();

                                            //Set Demo buffer state
                                            self.demo_buffer.set_state(DemoBufferState::Record);

                                            //Run lua script
                                            // If the DemoBuffer is in the [`Record`] state the lua runtime will automaticly load the called functions (created by the applications) into the demo buffer with their arguments
                                            match self
                                                .lua_runtime
                                                // Load the script as a string
                                                .load(script_instance.script.clone())
                                                // Execute the String
                                                .exec()
                                            {
                                                // The script has finished executing
                                                Ok(_output) => {
                                                    // Get the demo's steps, while draining it from the original buffer
                                                    let demo_steps: Vec<DemoStep> = self
                                                        .demo_buffer
                                                        .buffer
                                                        .write()
                                                        .drain(..)
                                                        .collect();

                                                    // Get current local date time
                                                    let current_date_time = Local::now();
                                                    
                                                    // Create a new demo instance
                                                    let demo_instance = DemoInstance {
                                                        // Add the steps
                                                        demo_steps,
                                                        // The name of the demo should be the scripts name
                                                        name: script_instance.name.clone(),
                                                        // Load the date
                                                        created_at: current_date_time,
                                                    };

                                                    // Load the demo into the list
                                                    self.demos.lock().push(demo_instance);
                                                },
                                                Err(err) => {
                                                    // Display the error if there were any
                                                    self.toasts.lock().add(
                                                        Toast::new()
                                                            .kind(egui_toast::ToastKind::Error)
                                                            .text(format!(
                                                                "Failed to create Demo: {err}"
                                                            )),
                                                    );

                                                    //Reset script state
                                                    script_instance.is_running = false;
                                                },
                                            }

                                            //Reset Demo buffer state
                                            self.demo_buffer.set_state(DemoBufferState::None);

                                            // Clear up anything created by the demo
                                            self.drawers.clear();

                                            //Load back the state
                                            self.drawers.clone_from(&current_drawer_canvas);
                                        }
                                        
                                        #[cfg(target_family = "wasm")] {
                                            ui.add_enabled_ui(false, |ui| {
                                                let _ = ui.button("Create Demo").on_disabled_hover_text(RichText::from("File handling is not supported in WASM.").color(Color32::RED));
                                            });
                                        }
                                    });
                                });
                            });
                            
                            // Increment script_idx
                            script_idx += 1;

                            // Return if we should keep the script
                            should_keep
                        });
                    });
            },
            ManagerPane::EntityManager => {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for drawer in self.drawers.iter() {
                            let (id, drawer) = drawer.pair();

                            ui.horizontal(|ui| {
                                ui.image(egui::include_image!("../assets/ferris.png"));
                                ui.label(id);
                                ui.menu_button("Info", |ui| {
                                    ui.label(format!("Angle: {}°", drawer.ang.to_degrees() - 90.));
                                    ui.label(format!(
                                        "Position: x: {} y: {}",
                                        drawer.pos.x, drawer.pos.y
                                    ));

                                    let color = drawer.color.to_linear();

                                    ui.label(format!(
                                        "Color: Red: {} Green: {} Blue: {} Alpha: {}",
                                        color.red, color.green, color.blue, color.alpha
                                    ));
                                });
                            });
                        }
                    });
            },
            ManagerPane::DemoManager => {
                ui.allocate_space(vec2(ui.available_width(), 2.));

                ui.menu_button("Import Demo", |ui| {
                    // Check for the target family as file handling is not supported in WASM.
                    #[cfg(not(target_family = "wasm"))] {
                        //Create import demo button, this is not available in wasm.
                        if ui.button("Import from File").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Demo File", &[DEMO_FILE_EXTENSION])
                                .pick_file()
                            {
                                match read_compressed_file_into::<DemoInstance>(path) {
                                    Ok(save_file) => {
                                        self.demos.lock().push(save_file);
                                    },
                                    Err(err) => {
                                        self.toasts.lock().add(
                                            Toast::new()
                                                .kind(egui_toast::ToastKind::Error)
                                                .text(format!("Demo runtime error: {err}")),
                                        );
                                    },
                                };
                            }
                        }
                    }
                    #[cfg(target_family = "wasm")] {
                        // Create placeholder button is wasm
                        ui.add_enabled_ui(false, |ui| {
                            let _ = ui.button("Import from File").on_disabled_hover_text(RichText::from("File handling is not supported in WASM.").color(Color32::RED));
                        });
                    }

                    ui.separator();

                    ui.menu_button("Import from Text", |ui| {
                        ui.horizontal(|ui| {
                            // We only enabled importing from text in the desktop mode.
                            ui.add_enabled_ui(!cfg!(target_family = "wasm"), |ui| {
                                if ui.button("Import").clicked() {
                                    match BASE64_STANDARD.decode(self.demo_rename_text_buffer.lock().to_string()) {
                                        Ok(bytes) => {
                                            let decompressed_bytes = decompress_to_vec(&bytes).unwrap();
                                            
                                            let demo_instance = deserialize_bytes_into::<DemoInstance>(decompressed_bytes).unwrap();
        
                                            self.demos.lock().push(demo_instance);
    
                                            ui.close_menu();
                                        },
                                        Err(_err) => {
                                            self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text("Text copied from clipboard does not contain any DemoInstances."));
                                        },
                                    }
    
                                    self.demo_rename_text_buffer.lock().clear();
                                }
                            });

                            ui.text_edit_singleline(&mut *self.demo_rename_text_buffer.lock());
                        });
                    }); 
                });

                ui.separator();

                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut idx = 0;
                        self.demos.lock().retain_mut(|demo| {
                            let mut should_retain = true;
                            let demo_name = demo.name.clone();

                            ui.horizontal(|ui| {
                                ui.label(demo_name);
                                ui.add_enabled_ui(
                                    self.demo_buffer.get_state() == DemoBufferState::None,
                                    |ui| {
                                        // Disable the UI when compiled to wasm as playbacks don't work in wasm.
                                        #[cfg(target_family = "wasm")]
                                        ui.disable();
                                    
                                        if ui.button("Playback").clicked() {
                                            //Clear environment
                                            self.drawers.clear();
                                            
                                            // Check if this is a wasm environent as playback are not enabled in the browser.
                                            #[cfg(not(target_family = "wasm"))]
                                            // Check if the demo is empty
                                            if let Some(first_step) = demo.demo_steps.first() {
                                                // Check if the first step is a valid function
                                                // If it returns an error the demo wont even start
                                                match first_step
                                                    .execute_lua_function(self.lua_runtime.clone())
                                                {
                                                    Ok(_) => {
                                                        //Set buffer state
                                                        self.demo_buffer
                                                            .set_state(DemoBufferState::Playback);

                                                        //Set the buffer
                                                        self.demo_buffer
                                                            .set_buffer(demo.demo_steps.clone());

                                                        ui.close_menu();
                                                    },
                                                    Err(err) => {
                                                        self.toasts.lock().add(
                                                            Toast::new()
                                                                .kind(egui_toast::ToastKind::Error)
                                                                .text(format!(
                                                                    "Demo runtime error: {err}"
                                                                )),
                                                        );
                                                    },
                                                };
                                            }
                                            else {
                                                // Reset state
                                                self.demo_buffer.set_state(DemoBufferState::None);
                                            }
                                        };
                                    },
                                );

                                ui.push_id(idx, |ui| {
                                    ui.collapsing("Settings", |ui| {
                                        if ui.button("Delete").clicked() {
                                            //Indicate that we would like to remove this entry
                                            should_retain = false;

                                            //Insert the script into the rubbish bin
                                            self.rubbish_bin
                                                .lock()
                                                .push(RubbishBinItem::Demo(demo.clone()));
                                        }

                                        let rename_menu = ui.menu_button("Rename Demo", |ui| {
                                            let rename_buffer = &mut *self.rename_buffer.lock();
                                            ui.text_edit_singleline(rename_buffer);

                                            if ui.button("Rename").clicked() {
                                                //Set the variable so that we will know which entry to modify and re-insert
                                                demo.name = rename_buffer.clone()
                                            }
                                        });

                                        if rename_menu.response.clicked() {
                                            *self.rename_buffer.lock() = demo.name.clone();
                                        }

                                        ui.menu_button("Export", |ui| {
                                            // Check for target family as File hadnling is not supported on wasm yet.
                                            #[cfg(not(target_family = "wasm"))] {
                                                // Create import as file button
                                                if ui.button("As File").clicked() {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .add_filter("Demo File", &[DEMO_FILE_EXTENSION])
                                                        .save_file()
                                                    {
                                                        let compressed_data =
                                                            miniz_oxide::deflate::compress_to_vec(
                                                                &rmp_serde::to_vec(&demo).unwrap(),
                                                                CompressionLevel::BestCompression as u8,
                                                            );
    
                                                        let _ = fs::write(path, compressed_data);
                                                        ui.close_menu();
                                                    }
                                                }
                                            }
                                            #[cfg(target_family = "wasm")] {
                                                ui.add_enabled_ui(false, |ui| {
                                                    let _ = ui.button("Export as File").on_disabled_hover_text(RichText::from("File handling is not supported in WASM.").color(Color32::RED));
                                                });
                                            }

                                            if ui.button("To Clipboard").clicked() {
                                                let compressed_data =
                                                    miniz_oxide::deflate::compress_to_vec(
                                                        &rmp_serde::to_vec(&demo).unwrap(),
                                                        CompressionLevel::BestCompression as u8,
                                                    );

                                                let base64_string =
                                                    BASE64_STANDARD.encode(compressed_data);

                                                ui.output_mut(|output| {
                                                    output.copied_text = base64_string
                                                });

                                                ui.close_menu();
                                            }
                                        });

                                        ui.separator();

                                        ui.menu_button("View raw Demo", |ui| {
                                            ui.label("Raw demo");

                                            ui.separator();

                                            ScrollArea::both().auto_shrink([false, false]).show(
                                                ui,
                                                |ui| {
                                                    for command in &demo.demo_steps {
                                                        ui.label(command.to_string());
                                                    }
                                                },
                                            );
                                        });

                                        ui.menu_button("Information", |ui| {
                                            ui.label(format!(
                                                "Created: {}",
                                                demo.created_at.to_rfc3339()
                                            ));
                                            ui.label(format!(
                                                "Total steps: {}",
                                                demo.demo_steps.len()
                                            ));
                                        });
                                    });
                                });
                            });

                            //Increment idx
                            idx += 1;

                            should_retain
                        });
                    });
            },
            ManagerPane::RubbishBin => {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.rubbish_bin.lock().retain(|item| {
                            let mut should_be_retained = true;
                            match item {
                                RubbishBinItem::Script(script_instance) => {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::from("Script").weak());
                                        ui.label(script_instance.name.clone());

                                        if ui.button("Restore").clicked() {
                                            // Since the HashMap entries are copied over to the `rubbish_bin` the keys and the values all match.
                                            self.scripts.lock().push(script_instance.clone());

                                            // Flag it to be deleted finally from this hashmap.
                                            should_be_retained = false;
                                        };

                                        if ui
                                            .button(RichText::from("Delete").color(Color32::RED))
                                            .clicked()
                                        {
                                            // Flag it to be deleted finally.
                                            should_be_retained = false;
                                        };
                                    });
                                },
                                RubbishBinItem::Demo(demo_instance) => {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::from("Demo").weak());
                                        ui.label(demo_instance.name.clone());
                                        if ui.button("Restore").clicked() {
                                            // Since the HashMap entries are copied over to the `rubbish_bin` the keys and the values all match.
                                            self.demos.lock().push(demo_instance.clone());

                                            // Flag it to be deleted finally from this hashmap.
                                            should_be_retained = false;
                                        };

                                        if ui
                                            .button(RichText::from("Delete").color(Color32::RED))
                                            .clicked()
                                        {
                                            // Flag it to be deleted finally.
                                            should_be_retained = false;
                                        };
                                    });
                                },
                            }

                            // Return the final value.
                            should_be_retained
                        });
                    });
            },
        }

        Default::default()
    }

    fn tab_title_for_pane(&mut self, pane: &ManagerPane) -> bevy_egui::egui::WidgetText
    {
        match pane {
            ManagerPane::ScriptManager => format!("Scripts: {}", self.scripts.lock().len()),
            ManagerPane::EntityManager => format!("Entities: {}", self.drawers.len()),
            ManagerPane::DemoManager => format!("Demos: {}", self.demos.lock().len()),
            ManagerPane::RubbishBin => format!("Deleted: {}", self.rubbish_bin.lock().len()),
        }
        .into()
    }
}

pub fn main_ui(
    mut ui_state: ResMut<UiState>,
    mut contexts: EguiContexts<'_, '_>,
    #[cfg(not(target_family = "wasm"))] lua_runtime: ResMut<LuaRuntime>,
    drawers: Res<Drawers>,
)
{
    let ctx = contexts.ctx_mut();

    // Call scripts with the `on_draw` callback

    // Call scripts with the `on_input` callback
    #[cfg(not(target_family = "wasm"))]
    ctx.input(|reader| {
        if reader.focused {
            let keys_down = reader.keys_down.clone();
            let callback_type = CallbackType::OnInput;
            let data = keys_down
                .iter()
                .enumerate()
                .map(|(idx, key)| (idx, key.name().to_string()));

            invoke_callback_from_scripts(&ui_state, &lua_runtime, callback_type, Some(data));
        }
    });

    egui_extras::install_image_loaders(ctx);

    ui_state.toasts.lock().show(ctx);

    let mut documentation_window_is_open = ui_state.documentation_window;

    egui::Window::new("Application Documentation")
        .open(&mut documentation_window_is_open)
        .show(ctx, |ui| {
            ScrollArea::both().show(ui, |ui| {
                commonmark_str!(ui, &mut ui_state.common_mark_cache, "DOCUMENTATION.md");
            });
        });

    ui_state.documentation_window = documentation_window_is_open;

    bevy_egui::egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    #[cfg(not(target_family = "wasm"))]
                    if ui.button("Save project").clicked() {
                        if let Some(save_path) = rfd::FileDialog::new()
                            .set_file_name("new_save")
                            .add_filter("Save file", &[PROJECT_FILE_EXTENSION])
                            .save_file()
                        {
                            let compressed_data = miniz_oxide::deflate::compress_to_vec(
                                &rmp_serde::to_vec(&*ui_state).unwrap(),
                                CompressionLevel::BestCompression as u8,
                            );

                            if let Err(err) = fs::write(save_path, compressed_data) {
                                ui_state.toasts.lock().add(
                                    Toast::new()
                                        .kind(egui_toast::ToastKind::Error)
                                        .text(err.to_string()),
                                );
                            };
                        }
                    };

                    #[cfg(not(target_family = "wasm"))]
                    if ui.button("Open project").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Open file", &[PROJECT_FILE_EXTENSION])
                            .pick_file()
                        {
                            match fs::read(path) {
                                Ok(read_bytes) => {
                                    let decompressed_data =
                                        miniz_oxide::inflate::decompress_to_vec(&read_bytes)
                                            .unwrap();

                                    let data: UiState =
                                        rmp_serde::from_slice(&decompressed_data).unwrap();

                                    *ui_state = data;
                                },
                                Err(_err) => {
                                    ui_state.toasts.lock().add(
                                        Toast::new()
                                            .kind(egui_toast::ToastKind::Error)
                                            .text(_err.to_string()),
                                    );
                                },
                            }
                        }
                    };

                    #[cfg(target_family = "wasm")]
                    ui.add_enabled_ui(false, |ui| {
                        ui.button("File").on_disabled_hover_text(
                            RichText::from("File handling is not supported in WASM.")
                                .color(Color32::RED),
                        );
                        ui.button("Open project").on_disabled_hover_text(
                            RichText::from("File handling is not supported in WASM.")
                                .color(Color32::RED),
                        );
                    });
                });

                ui.menu_button("Toolbox", |ui| {
                    ui.checkbox(&mut ui_state.manager_panel, "Item Manager");
                    ui.checkbox(&mut ui_state.command_panel, "Command Panel");
                });

                if ui.button("Documentation").clicked() {
                    ui_state.documentation_window = !ui_state.documentation_window;
                }

                #[cfg(target_family = "wasm")]
                ui.hyperlink_to(
                    "Get the full Desktop Version!",
                    "https://github.com/marci1175/ferris_draw/releases",
                )
            });
        });

    let mut entity_manager_width = 0.;

    if ui_state.manager_panel {
        let entity_manager = bevy_egui::egui::SidePanel::right("right_panel")
            .resizable(true)
            .min_width(240.)
            .show(ctx, |ui| {
                let toasts = ui_state.toasts.clone();
                let rubbish_bin = ui_state.rubbish_bin.clone();
                let rename_buffer = ui_state.rename_buffer.clone();
                let name_buffer = ui_state.name_buffer.clone();
                let demos = ui_state.demos.clone();
                let demo_buffer = ui_state.demo_buffer.clone();
                let scripts = ui_state.scripts.clone();
                let demo_text_buffer = ui_state.demo_rename_text_buffer.clone();

                ui_state.item_manager.ui(
                    &mut ManagerBehavior {
                        #[cfg(not(target_family = "wasm"))]
                        lua_runtime: lua_runtime.clone(),
                        toasts,
                        drawers: drawers.clone(),
                        rename_buffer,
                        name_buffer,
                        rubbish_bin,
                        demos,
                        demo_buffer,
                        scripts,
                        demo_rename_text_buffer: demo_text_buffer,
                    },
                    ui,
                );
            });

        entity_manager_width = entity_manager.response.rect.width();
    }

    let mut command_panel_height = 0.;

    if ui_state.command_panel {
        let command_panel = bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
            .default_height(100.)
            .min_height(150.)
            .resizable(true)
            .show(ctx, |ui| {
                let (_id, rect) =
                    ui.allocate_space(vec2(ui.available_width(), ui.available_height()));

                ui.painter().rect_filled(rect, 5.0, Color32::BLACK);

                ui.allocate_new_ui(UiBuilder::new().max_rect(rect.shrink(10.)), |ui| {
                    ScrollArea::both()
                        .auto_shrink([false, false])
                        .max_height(ui.available_height() - 30.)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for output in ui_state.command_line_outputs.read().iter() {
                                match output {
                                    ScriptLinePrompts::UserInput(text) => {
                                        ui.label(
                                            RichText::from(format!("> {text}"))
                                                .color(Color32::GRAY),
                                        );
                                    },
                                    ScriptLinePrompts::Standard(text) => {
                                        ui.label(RichText::from(text).color(Color32::WHITE));
                                    },
                                    ScriptLinePrompts::Error(text) => {
                                        ui.label(RichText::from(text).color(Color32::RED));
                                    },
                                }
                            }
                        });

                    ui.horizontal_centered(|ui| {
                        ui.group(|ui| {
                            // Indicate the terminal input.
                            ui.label(RichText::from("$>").color(Color32::WHITE));

                            // Get key input before spawning the text editor, because that consumes the enter key.
                            let enter_was_pressed = ctx.input_mut(|reader| {
                                reader.consume_key(egui::Modifiers::NONE, Key::Enter)
                            });

                            let up_was_pressed = ctx.input_mut(|reader| {
                                reader.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
                            });

                            let down_was_pressed = ctx.input_mut(|reader| {
                                reader.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                            });

                            let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(
                                ui.ctx(),
                                ui.style(),
                            );

                            let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                                let mut layout_job = egui_extras::syntax_highlighting::highlight(
                                    ui.ctx(),
                                    ui.style(),
                                    &theme,
                                    string,
                                    "lua",
                                );
                                layout_job.wrap.max_width = wrap_width;
                                ui.fonts(|f| f.layout_job(layout_job))
                            };

                            //Only show the text editor as enabled if there isnt a demo going on
                            let is_buffer_state_none =
                                ui_state.demo_buffer.get_state() == DemoBufferState::None;
                            ui.add_enabled_ui(is_buffer_state_none, |ui| {
                                // Create text editor
                                let text_edit = ui.add(
                                egui::TextEdit::singleline(&mut ui_state.command_line_buffer)
                                    .frame(false)
                                    .code_editor()
                                    .layouter(&mut layouter)
                                    .desired_width(ui.available_size_before_wrap().x)
                                    .hint_text(RichText::from({
                                        if is_buffer_state_none {
                                            "Enter a command"
                                        }
                                        else {
                                            "Command line is disabled while replaying a Demo."
                                        }
                                    }).italics()),
                                );

                                // If the underlying text was changed reset the command line input index to 0.
                                if text_edit.changed() {
                                    ui_state.command_line_input_index = 0;
                                }
                                // Only take keyobard inputs, when the text editor has focus.
                                if text_edit.has_focus() {
                                    if enter_was_pressed {
                                        let command_line_buffer =
                                            ui_state.command_line_buffer.clone();

                                        // If the wipe() function is called in wasm we should do the cleaning up here.
                                        #[cfg(target_family = "wasm")]
                                        {
                                            use crate::Drawings;

                                            if command_line_buffer == "wipe()" {
                                                for mut drawer in drawers.iter_mut() {
                                                    drawer.drawings = Drawings::default();
                                                }
                                                
                                                return;
                                            }
                                        }

                                        if command_line_buffer == "cls"
                                            || command_line_buffer == "clear"
                                        {
                                            ui_state.command_line_outputs.write().clear();
                                        }
                                        else if command_line_buffer == "?" || command_line_buffer == "help" {
                                            ui_state.documentation_window = true;
                                        }
                                        else {
                                            ui_state.command_line_outputs.write().push(
                                                ScriptLinePrompts::UserInput(
                                                    command_line_buffer.clone(),
                                                ),
                                            );

                                            // Check if it has the "wasm" target family, as the lua runtime is not supported in wasm
                                            #[cfg(not(target_family = "wasm"))]
                                            match lua_runtime
                                                .load(command_line_buffer.clone())
                                                .exec()
                                            {
                                                Ok(_output) => (),
                                                Err(_err) => {
                                                    ui_state.command_line_outputs.write().push(
                                                        ScriptLinePrompts::Error(_err.to_string()),
                                                    );
                                                },
                                            }
                                            
                                            ui_state.command_line_outputs.write().push(
                                                ScriptLinePrompts::Error(String::from("Demo only has a few functionalites available: `wipe()`, `clear`, `cls`, `?`, `help`."))
                                            );
                                        }

                                        if !command_line_buffer.is_empty() {
                                            //Store the command used
                                            ui_state
                                                .command_line_inputs
                                                .push_front(command_line_buffer.clone());
                                        }

                                        // Clear out the buffer regardless of the command being used.
                                        ui_state.command_line_buffer.clear();
                                    }

                                    if up_was_pressed {
                                        if ui_state.command_line_inputs.is_empty() {
                                            return;
                                        }

                                        if ui_state.command_line_input_index == 0 {
                                            ui_state.command_line_buffer =
                                                ui_state.command_line_inputs[0].clone();

                                            ui_state.command_line_input_index += 1;
                                        }
                                        else if (ui_state.command_line_input_index as i32)
                                            < ui_state.command_line_inputs.len() as i32
                                        {
                                            ui_state.command_line_buffer = ui_state
                                                .command_line_inputs
                                                [ui_state.command_line_input_index]
                                                .clone();
                                            ui_state.command_line_input_index += 1;
                                        }
                                    }

                                    if down_was_pressed {
                                            if ui_state.command_line_input_index
                                                == ui_state.command_line_inputs.len()
                                            {
                                                ui_state.command_line_buffer = ui_state
                                                    .command_line_inputs
                                                    [ui_state.command_line_input_index - 2]
                                                    .clone();
                                                ui_state.command_line_input_index -= 2;
                                            }
                                            else if ui_state.command_line_input_index > 0 {
                                                ui_state.command_line_input_index -= 1;

                                                ui_state.command_line_buffer = ui_state
                                                    .command_line_inputs
                                                    [ui_state.command_line_input_index]
                                                    .clone();
                                            }
                                            else {
                                                ui_state.command_line_buffer.clear();
                                            }
                                        }
                                }
                                text_edit.on_hover_text("Enter ? or help to get more information.");
                            });
                        });
                    });
                });
            });

        command_panel_height = command_panel.response.rect.height();
    }

    let mut is_playbacker_open = true;

    // Demo playbacks are not enabled in the wasm-environment
    #[cfg(not(target_family = "wasm"))]
    if let Some(buffer) = ui_state
        .demo_buffer
        .clone()
        .get_state_if_eq(DemoBufferState::Playback)
    {
        Window::new("Playback Manager")
            .collapsible(false)
            .fixed_pos(Pos2::new(
                (ctx.screen_rect().width() - (entity_manager_width + 225.)) / 2.,
                ctx.used_rect().height() - (command_panel_height + 140.),
            ))
            .open(&mut is_playbacker_open)
            .fixed_size(vec2(300., 50.))
            .show(ctx, |ui| {
                ui.allocate_ui(vec2(200., 70.), |ui| {
                    ui.horizontal(|ui| {
                        ui.centered_and_justified(|ui| {
                            //Lock playback buffer and pray we dont deadlock
                            let locked_buffer = buffer.read();

                            ui.add_enabled_ui(ui_state.demo_buffer.iter_idx != 0, |ui| {
                                if ui.button("◀").clicked() {
                                    drawers.clear();

                                    let desired_idx = ui_state.demo_buffer.iter_idx - 1;

                                    if desired_idx == 0 {
                                        // This cannot panic as we would have paniced already
                                        locked_buffer
                                            .first()
                                            .unwrap()
                                            .execute_lua_function(lua_runtime.clone())
                                            .unwrap();
                                    }

                                    for idx in 0..desired_idx {
                                        let step = locked_buffer[idx]
                                            .execute_lua_function(lua_runtime.clone());

                                        match step {
                                            Ok(_) => (),
                                            Err(err) => {
                                                ui_state.toasts.lock().add(
                                                    Toast::new()
                                                        .kind(egui_toast::ToastKind::Error)
                                                        .text(format!("Demo runtime error: {err}")),
                                                );

                                                return;
                                            },
                                        }
                                    }

                                    // If the action was successful decrement the internal index
                                    ui_state.demo_buffer.iter_idx -= 1;
                                }
                            });

                            ui.vertical(|ui| {
                                ui.label(format!(
                                    "Current step ({}/{}):",
                                    ui_state.demo_buffer.iter_idx + 1,
                                    locked_buffer.len()
                                ));
                                ui.label(locked_buffer[ui_state.demo_buffer.iter_idx].to_string());
                            });

                            ui.add_enabled_ui(
                                ui_state.demo_buffer.iter_idx + 1 != locked_buffer.len(),
                                |ui| {
                                    if ui.button("▶").clicked() {
                                        let next_step = locked_buffer
                                            [ui_state.demo_buffer.iter_idx + 1]
                                            .clone()
                                            .execute_lua_function(lua_runtime.clone());

                                        match next_step {
                                            Ok(_) => {
                                                // If the action was successful increment the internal index
                                                ui_state.demo_buffer.iter_idx += 1;
                                            },
                                            Err(err) => {
                                                ui_state.toasts.lock().add(
                                                    Toast::new()
                                                        .kind(egui_toast::ToastKind::Error)
                                                        .text(format!("Demo error: {err}")),
                                                );
                                            },
                                        }
                                    }
                                },
                            );
                        });
                    });
                });
            });
    }

    // If the playbacker menu was closed
    if !is_playbacker_open {
        drawers.clear();

        //Reset the demo buffer thus existing the demo mode
        ui_state.demo_buffer.clear();
    }
}

#[cfg(not(target_family = "wasm"))]
fn invoke_callback_from_scripts<K, V>(
    ui_state: &ResMut<'_, UiState>,
    lua_runtime: &ResMut<'_, LuaRuntime>,
    callback_type: CallbackType,
    data: Option<impl IntoIterator<Item = (K, V)> + Clone>,
) where
    K: IntoLua,
    V: IntoLua,
{
    for script in ui_state.scripts.lock().iter_mut() {
        // If the script is not running dont call its callbacks
        if !script.is_running {
            continue;
        }

        // If the data is a Some that means that we want to invoke the callback with an argument passed in.
        if let Some(ref data) = data {
            if let Some(function) = script.callbacks.get(&callback_type) {
                if let Err(err) = function.call::<()>(lua_runtime.create_table_from(data.clone())) {
                    // Add the error into the toasts if it returned an error
                    ui_state.toasts.lock().add(
                        Toast::new()
                            .kind(egui_toast::ToastKind::Error)
                            .text(err.to_string()),
                    );

                    script.is_running = false;
                };
            };
        }
        // If the data is a None it means that the callback is to be invoked without arguments.
        else if let Some(function) = script.callbacks.get(&callback_type) {
            if let Err(err) = function.call::<()>(()) {
                // Add the error into the toasts if it returned an error
                ui_state.toasts.lock().add(
                    Toast::new()
                        .kind(egui_toast::ToastKind::Error)
                        .text(err.to_string()),
                );

                script.is_running = false;
            };
        }
    }
}

/// This function reads the bytes available at the specific [`PathBuf`] path, and then Deserializes into type [`T`] from the bytes.
/// This function uses [`rmp_serde`] to Deserialize.
fn read_compressed_file_into<T: for<'a> Deserialize<'a>>(path: PathBuf) -> anyhow::Result<T>
{
    let bytes = fs::read(path)?;

    let decompressed_bytes = decompress_to_vec(&bytes).unwrap();

    let deserialized_data = deserialize_bytes_into(decompressed_bytes)?;

    Ok(deserialized_data)
}

fn deserialize_bytes_into<T: for<'a> Deserialize<'a>>(
    decompressed_bytes: Vec<u8>,
) -> Result<T, anyhow::Error>
{
    let deserialized_data = rmp_serde::from_slice::<T>(&decompressed_bytes)?;
    Ok(deserialized_data)
}
