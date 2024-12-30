use bevy::prelude::{Res, ResMut};
use bevy_egui::{
    egui::{self, vec2, Color32, Key, Pos2, RichText, ScrollArea, TextEdit, UiBuilder, Window},
    EguiContexts,
};
use chrono::Local;
use dashmap::{DashMap, DashSet};
use egui_commonmark::{commonmark_str, CommonMarkCache};
use miniz_oxide::{deflate::CompressionLevel, inflate::decompress_to_vec};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};

use crate::{
    DemoBuffer, DemoBufferState, DemoInstance, DemoStep, Drawers, LuaRuntime,
    ScriptLinePrompts, DEMO_FILE_EXTENSION, PROJECT_FILE_EXTENSION,
};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use bevy::prelude::Resource;
use egui_tiles::Tiles;
use egui_toast::{Toast, Toasts};
use indexmap::{set::MutableValues, IndexSet};

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
    pub rubbish_bin: Arc<DashSet<RubbishBinItem>>,

    /// The CommonMarkCache is a cache which stores data about the documentation displayer widget.
    /// We need this to be able to display the documentation.
    #[serde(skip)]
    pub common_mark_cache: CommonMarkCache,

    pub documentation_window: bool,

    /// This field is used to store demos, which can be playbacked later.
    pub demos: Arc<DashSet<DemoInstance>>,

    /// This demo buffer is used when recording a demo for a script.
    /// If the demo is accessible a recording can't be started as it is only available when there is an ongoing recording.
    /// Being accessible indicates that the lua runtime's scripts can write to the underlying mutex.
    /// If the demo recording is stopped the buffer is cleared and accessibility is set to false.
    #[serde(skip)]
    pub demo_buffer: DemoBuffer<Vec<DemoStep>>,

    pub scripts: Arc<Mutex<IndexSet<ScriptInstance>>>,

    pub demo_text_buffer: Arc<Mutex<String>>,
}

impl Default for UiState
{
    fn default() -> Self
    {
        Self {
            command_panel: false,
            manager_panel: false,
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
            rubbish_bin: Arc::new(DashSet::new()),
            common_mark_cache: CommonMarkCache::default(),
            documentation_window: false,
            demos: Arc::new(DashSet::new()),
            demo_buffer: DemoBuffer::new(vec![]),
            scripts: Arc::new(Mutex::new(IndexSet::new())),
            demo_text_buffer: Arc::new(Mutex::new(String::new())),
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

#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RubbishBinItem
{
    Script(ScriptInstance),
    Demo(DemoInstance),
    // Drawer(crate::Drawer),
}

/// The manager panel's inner behavior, the data it contains, this can be used to share data over to the tabs from the main ui.
pub struct ManagerBehavior
{
    /// The [`mlua::Lua`] runtime handle, this can be used to run code on.
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
    rubbish_bin: Arc<DashSet<RubbishBinItem>>,

    /// This field is used to store demos, which can be playbacked later.
    demos: Arc<DashSet<DemoInstance>>,

    /// This buffer is used when recording a demo, or when playbacking one.
    /// The [`DemoBuffer`] can have multiple "states" depending on what its used for to create custom behavior.
    demo_buffer: DemoBuffer<Vec<DemoStep>>,

    /// The list of scripts the project contains.
    scripts: Arc<Mutex<IndexSet<ScriptInstance>>>,

    /// The demos' rename text buffer.
    demo_rename_text_buffer: Arc<Mutex<String>>,
}

/// A [`ScriptInstance`] holds information about one script.
/// It contains the script's name, and the script itself.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct ScriptInstance
{
    /// The name of the script.
    name: String,
    /// The script itself.
    script: String,
}

impl ScriptInstance
{   
    /// Creates a new [`ScriptInstance`].
    pub fn new(name: String, script: String) -> Self
    {
        Self { name, script }
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
                                        script_handle.insert(ScriptInstance { name: name_buffer.clone(), script: String::new() });

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
                        if ui.button("Import from File").clicked() {
                            // If the user has selected a file patter match the path
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                // Pattern math reading a String out from it
                                match fs::read_to_string(&path) {
                                    Ok(file_content) => {
                                        // If we could read the file content load the read file into a script instance which we insert into the list 
                                        self.scripts.lock().insert(ScriptInstance::new(path.file_name().unwrap_or_default().to_str().unwrap_or_default().to_string(), file_content));
                                        
                                        // Close menu if we could read successfully
                                        ui.close_menu();
                                    },
                                    Err(_err) => {
                                        // Display any kind of error to a notification
                                        self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text(format!("Reading from file ({}) failed: {_err}", path.display())));
                                    },
                                }
                            }
                        }

                    });
                });

                //Draw separator line
                ui.separator();

                ScrollArea::both()
                    .max_height(ui.available_height() - 200.)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.scripts.lock().retain2(|script_instance| {
                            let mut should_keep = true;

                            ui.horizontal(|ui| {
                                ui.label(script_instance.name.clone());

                                ui.add_enabled_ui(
                                    self.demo_buffer.get_state() == DemoBufferState::None,
                                    |ui| {
                                        if ui.button("Run").clicked() {
                                            if let Err(err) = self
                                                .lua_runtime
                                                .load(script_instance.script.to_string())
                                                .exec()
                                            {
                                                self.toasts.lock().add(
                                                    Toast::new()
                                                        .kind(egui_toast::ToastKind::Error)
                                                        .text(err.to_string()),
                                                );
                                            };
                                        }
                                    },
                                );

                                ui.push_id(script_instance.name.clone(), |ui| {
                                    ui.collapsing("Settings", |ui| {
                                        ui.menu_button("Edit", |ui| {
                                            let theme =
                                        egui_extras::syntax_highlighting::CodeTheme::from_memory(
                                            ui.ctx(),
                                            ui.style(),
                                        );

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

                                            ScrollArea::both().show(ui, |ui| {
                                                ui.add(
                                                    TextEdit::multiline(
                                                        &mut script_instance.script,
                                                    )
                                                    .code_editor()
                                                    .layouter(&mut layouter),
                                                );
                                            });
                                        });
                                        if ui.button("Delete").clicked() {
                                            // Flag the script as to be deleted
                                            should_keep = false;

                                            //Insert the script into the rubbish bin
                                            self.rubbish_bin.insert(RubbishBinItem::Script(
                                                script_instance.clone(),
                                            ));
                                        }

                                        let rename_menu = ui.menu_button("Rename Script", |ui| {
                                            ui.text_edit_singleline(
                                                &mut *self.rename_buffer.lock(),
                                            );

                                            if ui.button("Rename").clicked() {
                                                let name_buffer = &*self.rename_buffer.lock();

                                                script_instance.name = name_buffer.clone();
                                            }
                                        });

                                        if rename_menu.response.clicked() {
                                            *self.rename_buffer.lock() =
                                                script_instance.name.clone();
                                        }

                                        if ui.button("Export as File").clicked() {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_file_name(script_instance.name.clone())
                                                .add_filter("Lua", &["lua"])
                                                .save_file()
                                            {
                                                fs::write(path, script_instance.script.clone())
                                                    .unwrap();
                                            }
                                        }

                                        ui.separator();

                                        if ui.button("Create Demo").clicked() {
                                            //Store current drawers and canvas
                                            let current_drawer_canvas =
                                                Drawers(Arc::new(DashMap::clone(&self.drawers.0)));

                                            self.drawers.clear();

                                            //Set Demo buffer state
                                            self.demo_buffer.set_state(DemoBufferState::Record);

                                            //Run lua script
                                            match self
                                                .lua_runtime
                                                .load(script_instance.script.clone())
                                                .exec()
                                            {
                                                Ok(_output) => {
                                                    let demo_steps: Vec<DemoStep> = self
                                                        .demo_buffer
                                                        .buffer
                                                        .write()
                                                        .drain(..)
                                                        .collect();

                                                    let current_date_time = Local::now();

                                                    let demo_instance = DemoInstance {
                                                        demo_steps,
                                                        script_identifier: sha256::digest(
                                                            script_instance.script.clone(),
                                                        ),
                                                        name: script_instance.name.clone(),
                                                        created_at: current_date_time,
                                                    };

                                                    self.demos.insert(demo_instance);
                                                },
                                                Err(err) => {
                                                    self.toasts.lock().add(
                                                        Toast::new()
                                                            .kind(egui_toast::ToastKind::Error)
                                                            .text(format!(
                                                                "Failed to create Demo: {err}"
                                                            )),
                                                    );
                                                },
                                            }

                                            //Reset Demo buffer state
                                            self.demo_buffer.set_state(DemoBufferState::None);

                                            self.drawers.clear();

                                            //Load back the state
                                            self.drawers.clone_from(&current_drawer_canvas);
                                        }
                                    });
                                });
                            });

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
                    if ui.button("Import Demo from File").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Demo File", &[DEMO_FILE_EXTENSION])
                            .pick_file()
                        {
                            match read_compressed_file_into::<DemoInstance>(path) {
                                Ok(save_file) => {
                                    self.demos.insert(save_file);
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

                    ui.separator();

                    ui.menu_button("Import from Text", |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Import").clicked() {
                                match BASE64_STANDARD.decode(self.demo_rename_text_buffer.lock().to_string()) {
                                    Ok(bytes) => {
                                        let decompressed_bytes = decompress_to_vec(&bytes).unwrap();
                                        
                                        let demo_instance = deserialize_bytes_into::<DemoInstance>(decompressed_bytes).unwrap();
    
                                        self.demos.insert(demo_instance);

                                        ui.close_menu();
                                    },
                                    Err(_err) => {
                                        self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text("Text copied from clipboard does not contain any DemoInstances."));
                                    },
                                }

                                self.demo_rename_text_buffer.lock().clear();
                            }

                            ui.text_edit_singleline(&mut *self.demo_rename_text_buffer.lock());
                        });
                    }); 
                });

                ui.separator();

                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut modified_demo: Option<DemoInstance> = None;

                        let mut should_remove = false;
                        for (idx, demo) in self.demos.iter().enumerate() {
                            let demo_name = demo.name.clone();

                            ui.horizontal(|ui| {
                                ui.label(demo_name);
                                ui.add_enabled_ui(
                                    self.demo_buffer.get_state() == DemoBufferState::None,
                                    |ui| {
                                        if ui.button("Playback").clicked() {
                                            //Clear environment
                                            self.drawers.clear();

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
                                                //n1GYpHpfb&_E:an$nR&Ej
                                                self.demo_buffer.set_state(DemoBufferState::None);
                                            }
                                        };
                                    },
                                );

                                ui.push_id(idx, |ui| {
                                    ui.collapsing("Settings", |ui| {
                                        if ui.button("Delete").clicked() {
                                            //Indicate that we would like to remove this entry
                                            should_remove = true;
                                            modified_demo = Some(demo.clone());

                                            //Insert the script into the rubbish bin
                                            self.rubbish_bin
                                                .insert(RubbishBinItem::Demo(demo.clone()));
                                        }

                                        let rename_menu = ui.menu_button("Rename Demo", |ui| {
                                            ui.text_edit_singleline(
                                                &mut *self.rename_buffer.lock(),
                                            );

                                            if ui.button("Rename").clicked() {
                                                //Set the variable so that we will know which entry to modify and re-insert
                                                modified_demo = Some(demo.clone());
                                            }
                                        });

                                        if rename_menu.response.clicked() {
                                            *self.rename_buffer.lock() = demo.name.clone();
                                        }

                                        ui.menu_button("Export", |ui| {
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
                        }

                        if let Some(mut demo) = modified_demo {
                            self.demos.remove(&demo);

                            //If we should be removing this entry we should return from this point
                            if should_remove {
                                return;
                            }

                            let name_buffer = &*self.rename_buffer.lock();

                            demo.name = name_buffer.clone();

                            self.demos.insert(demo);
                        }
                    });
            },
            ManagerPane::RubbishBin => {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.rubbish_bin.retain(|item| {
                            let mut should_be_retained = true;
                            match item {
                                RubbishBinItem::Script(script_instance) => {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::from("Script").weak());
                                        ui.label(script_instance.name.clone());
                                        if ui.button("Restore").clicked() {
                                            // Since the HashMap entries are copied over to the `rubbish_bin` the keys and the values all match.
                                            self.scripts.lock().insert(script_instance.clone());

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
                                            self.demos.insert(demo_instance.clone());

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
            ManagerPane::DemoManager => format!("Demos: {}", self.demos.len()),
            ManagerPane::RubbishBin => format!("Deleted: {}", self.rubbish_bin.len()),
        }
        .into()
    }
}

pub fn main_ui(
    mut ui_state: ResMut<UiState>,
    mut contexts: EguiContexts<'_, '_>,
    lua_runtime: ResMut<LuaRuntime>,
    drawers: Res<Drawers>,
)
{
    let ctx = contexts.ctx_mut();

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
                });

                ui.menu_button("Toolbox", |ui| {
                    ui.checkbox(&mut ui_state.manager_panel, "Item Manager");
                    ui.checkbox(&mut ui_state.command_panel, "Command Panel");
                });

                if ui.button("Documentation").clicked() {
                    ui_state.documentation_window = !ui_state.documentation_window;
                }
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
                let demo_text_buffer = ui_state.demo_text_buffer.clone();

                ui_state.item_manager.ui(
                    &mut ManagerBehavior {
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
                                            "Enter a command..."
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

                                        if command_line_buffer == "cls"
                                            || command_line_buffer == "clear"
                                        {
                                            ui_state.command_line_outputs.write().clear();
                                        }
                                        else {
                                            ui_state.command_line_outputs.write().push(
                                                ScriptLinePrompts::UserInput(
                                                    command_line_buffer.clone(),
                                                ),
                                            );
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
                            });
                        });
                    });
                });
            });

        command_panel_height = command_panel.response.rect.height();
    }

    let mut is_playbacker_open = true;

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
