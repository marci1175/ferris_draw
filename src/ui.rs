use bevy::{prelude::{Res, ResMut}, tasks::futures_lite::stream::FlatMap};
use bevy_egui::{
    egui::{self, vec2, Color32, Key, RichText, ScrollArea, TextEdit, UiBuilder},
    EguiContexts,
};
use dashmap::DashMap;
use egui_commonmark::{commonmark_str, CommonMarkCache};
use miniz_oxide::deflate::CompressionLevel;
use std::{collections::VecDeque, fs, sync::Arc, thread::sleep, time::Duration};

use parking_lot::{Mutex, RwLock};

use bevy::prelude::Resource;
use egui_tiles::Tiles;
use egui_toast::{Toast, Toasts};
use indexmap::{map::MutableKeys, IndexMap};

use crate::{DemoBuffer, DemoBufferState, DemoInstance, DemoStep, Drawers, LuaRuntime, ScriptLinePrompts};

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
    pub rubbish_bin: Arc<DashMap<String, String>>,

    /// The CommonMarkCache is a cache which stores data about the documentation displayer widget.
    /// We need this to be able to display the documentation.
    #[serde(skip)]
    pub common_mark_cache: CommonMarkCache,

    pub documentation_window: bool,

    /// This field is used to store demos, which can be playbacked later.
    /// One script can only have one demo.
    pub demos: Arc<DashMap<String, DemoInstance>>,

    /// This demo buffer is used when recording a demo for a script.
    /// If the demo is accessible a recording can't be started as it is only available when there is an ongoing recording.
    /// Being accessible indicates that the lua runtime's scripts can write to the underlying mutex.
    /// If the demo recording is stopped the buffer is cleared and accessibility is set to false.
    #[serde(skip)]
    pub demo_buffer: DemoBuffer<Vec<DemoStep>>,

    pub scripts: Arc<Mutex<IndexMap<String, String>>>,
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
            rubbish_bin: Arc::new(DashMap::new()),
            common_mark_cache: CommonMarkCache::default(),
            documentation_window: false,
            demos: Arc::new(DashMap::new()),
            demo_buffer: DemoBuffer::new(vec![]),
            scripts: Arc::new(Mutex::new(IndexMap::new()))
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

    DemoManager,
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
    rubbish_bin: Arc<DashMap<String, String>>,

    demos: Arc<DashMap<String, DemoInstance>>,

    demo_buffer: DemoBuffer<Vec<DemoStep>>,

    scripts: Arc<Mutex<IndexMap<String, String>>>,
}

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
                ui.allocate_space(vec2(ui.available_width(), 2.));

                ui.allocate_ui(vec2(ui.available_width(), ui.min_size().y), |ui| {
                    let name_buffer = &mut *self.name_buffer.lock();
                    
                    ui.menu_button("Add Script", |ui| {
                        ui.allocate_ui(vec2(ui.available_width(), 70.), |ui| {
                            ui.horizontal_centered(|ui| {
                                    ui.add_enabled_ui(!name_buffer.is_empty(), |ui| {
                                    let add_button = ui.button("Add");
    
                                    if add_button.clicked() {
                                        let mut script_handle = self.scripts.lock();

                                        if !script_handle.contains_key(&*name_buffer) && !self.rubbish_bin.contains_key(&*name_buffer) {
                                            script_handle.insert(name_buffer.clone(), String::from(""));
                                            name_buffer.clear();
                                            ui.close_menu();
                                        }
                                        else {
                                            self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text(format!("The script named: {name_buffer} already exists. Please choose another name or rename an existing script.")));
                                        }
                                    }
                                });

                                ui.add(TextEdit::singleline(name_buffer).hint_text("Name"));
                            });
                        });

                        ui.separator();

                        if ui.button("Import from File").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                match fs::read_to_string(&path) {
                                    Ok(file_content) => {
                                        self.scripts.lock().insert(path.file_name().unwrap_or_default().to_str().unwrap_or_default().to_string(), file_content);
                                    },
                                    Err(_err) => {
                                        self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text(format!("Reading from file ({}) failed: {_err}", path.display())));
                                    },
                                }

                                ui.close_menu();
                            }
                        }

                    });
                });

                ui.separator();

                let scripts_clone = self.scripts.clone();

                ScrollArea::both().max_height(ui.available_height() - 200.).auto_shrink([false, false]).show(ui, |ui| {
                    self.scripts.lock().retain2(|name, script| {
                        let mut should_keep = true;
                        ui.horizontal(|ui| {
                            ui.label(name.clone());

                            ui.add_enabled_ui(self.demo_buffer.get_state() == DemoBufferState::None, |ui| {
                                if ui.button("Run").clicked() {
                                    if let Err(err) = self.lua_runtime.load(script.to_string()).exec() {
                                        self.toasts.lock().add(
                                            Toast::new()
                                                .kind(egui_toast::ToastKind::Error)
                                                .text(err.to_string()),
                                        );
                                    };
                                }
                            });

                            ui.push_id(name.clone(), |ui| {
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
                                                TextEdit::multiline(script)
                                                    .code_editor()
                                                    .layouter(&mut layouter),
                                            );
                                        });
                                    });
                                    if ui.button("Delete").clicked() {
                                        // Flag the script as to be deleted
                                        should_keep = false;

                                        //Insert the script into the rubbish bin
                                        self.rubbish_bin.insert(name.clone(), script.clone());
                                    }
                                    let menu_button = ui.menu_button("Rename script", |ui| {
                                        ui.text_edit_singleline(&mut *self.rename_buffer.lock());
                                        if ui.button("Rename").clicked() {
                                            let name_buffer = &*self.rename_buffer.lock();
                                            if !scripts_clone.lock().contains_key(name_buffer) {
                                                *name = name_buffer.clone();
                                            }
                                            else {
                                                self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text(format!("Script with name {name_buffer} already exists.")));
                                            }
                                        }
                                    });

                                    if menu_button.response.clicked() {
                                        *self.rename_buffer.lock() = name.clone();
                                    }
                                
                                    if ui.button("Export as File").clicked() {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .set_file_name(name.clone())
                                            .add_filter("Lua", &["lua"])
                                            .save_file() {
                                                fs::write(path, script.clone()).unwrap();
                                            }
                                    }
                                
                                    ui.separator();

                                    if ui.button("Create Demo").clicked() {
                                        //Store current drawers and canvas
                                        let current_drawer_canvas = self.drawers.clone();

                                        self.drawers.clear();

                                        //Set Demo buffer state
                                        self.demo_buffer.set_state(DemoBufferState::Record);

                                        //Run lua script
                                        match self.lua_runtime.load(script.clone()).exec() {
                                            Ok(_output) => {
                                                let demo_steps: Vec<DemoStep> = self.demo_buffer.resource.write().drain(..).collect();

                                                let demo_instance = DemoInstance { demo_steps, script_identifier: sha256::digest(script.clone())};
                                                
                                                self.demos.insert(name.clone(), demo_instance);
                                            },
                                            Err(err) => {
                                                self.toasts.lock().add(
                                                    Toast::new()
                                                        .kind(egui_toast::ToastKind::Error)
                                                        .text(format!("Failed to create Demo: {err}")),
                                                );
                                            },
                                        }

                                        //Reset Demo buffer state
                                        self.demo_buffer.set_state(DemoBufferState::None);

                                        //Load back the state
                                        self.drawers = current_drawer_canvas;
                                    }
                                });
                            });
                        });

                        should_keep
                    });
                });

                ui.collapsing(
                    format!("Deleted Scripts: {}", self.rubbish_bin.len()),
                    |ui| {
                        ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.rubbish_bin.retain(|name, script| {
                                    let mut should_be_retained = true;

                                    ui.horizontal(|ui| {
                                        ui.label(name);
                                        if ui
                                            .button(RichText::from("Delete").color(Color32::RED))
                                            .clicked()
                                        {
                                            // Flag it to be deleted finally.
                                            should_be_retained = false;
                                        };

                                        if ui.button("Restore").clicked() {
                                            // Since the HashMap entries are copied over to the `rubbish_bin` the keys and the values all match.
                                            self.scripts.lock().insert(name.clone(), script.clone());

                                            // Flag it to be deleted finally from this hashmap.
                                            should_be_retained = false;
                                        };
                                    });

                                    // Return the final value.
                                    should_be_retained
                                });
                            });
                    },
                );
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
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in self.demos.clone().iter() {
                            let demo_name = entry.key();

                            if ui.button("Import Demo from File").clicked() {

                            }

                            ui.separator();

                            ui.horizontal(|ui| {
                                if let Some(script) = self.scripts.lock().get(demo_name) {
                                    if sha256::digest(script) != entry.script_identifier {
                                        ui.group(|ui| {
                                            ui.label(RichText::from("!").color(Color32::RED)).on_hover_text("The script has been modified since the last demo recording.");
                                        });
                                    }
                                }

                                ui.label(demo_name);
                                ui.add_enabled_ui(self.demo_buffer.get_state() == DemoBufferState::None, |ui| {
                                    if ui.button("Playback").clicked() {
                                        //Set buffer state
                                        self.demo_buffer.set_state(DemoBufferState::Playback);
    
                                        //Set the buffer
                                        self.demo_buffer.set_buffer(entry.demo_steps.clone());
                                    };
                                });
                                
                                ui.menu_button("Settings", |ui| {
                                    if ui.button("Export as File").clicked() {
                                        
                                    }
                                    
                                    ui.separator();

                                    ui.menu_button("View raw Demo", |ui| {
                                        ui.label("Raw demo");
                                        
                                        ui.separator();

                                        ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                                            for command in &entry.demo_steps {
                                                ui.label(command.to_string());
                                            }
                                        });
                                    });
                                });
                            });
                        }
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
                            .add_filter("Save file", &["data"])
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
                            .add_filter("Open file", &["data"])
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

    if ui_state.manager_panel {
        bevy_egui::egui::SidePanel::right("right_panel")
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
                    },
                    ui,
                );
            });
    }

    if ui_state.command_panel {
        bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
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

                            // Create text editor
                            let text_edit = ui.add(
                                egui::TextEdit::singleline(&mut ui_state.command_line_buffer)
                                    .frame(false)
                                    .code_editor()
                                    .layouter(&mut layouter)
                                    .desired_width(ui.available_size_before_wrap().x)
                                    .hint_text(RichText::from("lua command").italics()),
                            );

                            // If the underlying text was changed reset the command line input index to 0.
                            if text_edit.changed() {
                                ui_state.command_line_input_index = 0;
                            }

                            // Only take keyobard inputs, when the text editor has focus.
                            if text_edit.has_focus() {
                                if enter_was_pressed {
                                    let command_line_buffer = ui_state.command_line_buffer.clone();

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
                                        match lua_runtime.load(command_line_buffer.clone()).exec() {
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
                                        ui_state.command_line_buffer = ui_state.command_line_inputs
                                            [ui_state.command_line_input_index]
                                            .clone();
                                        ui_state.command_line_input_index += 1;
                                    }
                                }

                                if down_was_pressed {
                                    if ui_state.command_line_input_index
                                        == ui_state.command_line_inputs.len()
                                    {
                                        ui_state.command_line_buffer = ui_state.command_line_inputs
                                            [ui_state.command_line_input_index - 2]
                                            .clone();
                                        ui_state.command_line_input_index -= 2;
                                    }
                                    else if ui_state.command_line_input_index > 0 {
                                        ui_state.command_line_input_index -= 1;

                                        ui_state.command_line_buffer = ui_state.command_line_inputs
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
    }
}
