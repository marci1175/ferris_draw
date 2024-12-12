use bevy::{
    color::palettes::css::WHITE,
    prelude::{Res, ResMut},
};
use bevy_egui::{
    egui::{self, vec2, Color32, Key, Layout, Rect, RichText, ScrollArea, Sense, UiBuilder},
    EguiContexts,
};
use miniz_oxide::deflate::CompressionLevel;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};

use parking_lot::{Mutex, RwLock};

use bevy::prelude::Resource;
use egui_tiles::Tiles;
use egui_toast::{Toast, Toasts};
use indexmap::{map::MutableKeys, IndexMap};

use crate::{Drawers, LuaRuntime, ScriptLinePrompts};

#[derive(Resource, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState
{
    /// Should the manager panel open.
    pub manager_panel: bool,

    /// Egui notifications
    #[serde(skip)]
    toasts: Arc<Mutex<Toasts>>,

    #[serde(skip)]
    /// Should the new viewport open? NOTE: This egui backend doesnt support multiple viewports.
    pub code_manager_window: Arc<AtomicBool>,

    #[serde(skip)]
    rename_buffer: Arc<Mutex<String>>,

    #[serde(skip)]
    name_buffer: Arc<Mutex<String>>,

    #[serde(skip)]
    command_line_buffer: String,

    /// The manager panel's tab state.
    pub item_manager: egui_tiles::Tree<ManagerPane>,

    #[serde(skip)]
    pub command_line_outputs: Arc<RwLock<Vec<ScriptLinePrompts>>>,
}

impl Default for UiState
{
    fn default() -> Self
    {
        Self {
            manager_panel: Default::default(),
            code_manager_window: Default::default(),
            item_manager: {
                let mut tiles = Tiles::default();
                let mut tileids = vec![];

                tileids.push(tiles.insert_pane(ManagerPane::ItemManager));
                tileids.push(tiles.insert_pane(ManagerPane::Scripts(IndexMap::new())));

                egui_tiles::Tree::new("manager_tree", tiles.insert_tab_tile(tileids), tiles)
            },
            toasts: Arc::new(Mutex::new(Toasts::new())),
            rename_buffer: Arc::new(parking_lot::Mutex::new(String::new())),
            name_buffer: Arc::new(parking_lot::Mutex::new(String::new())),
            command_line_outputs: Arc::new(RwLock::new(vec![])),
            command_line_buffer: String::new(),
        }
    }
}

/// The manager panel's tabs.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub enum ManagerPane
{
    Scripts(IndexMap<String, String>),
    #[default]
    ItemManager,
}

/// The manager panel's inner behavior, the data it contains, this can be used to share data over to the tabs from the main ui.
pub struct ManagerBehavior
{
    /// Should the new viewport open? NOTE: This egui backend doesnt support multiple viewports.
    pub code_manager_window: Arc<AtomicBool>,

    /// The [`mlua::Lua`] runtime handle, this can be used to run code on.
    pub lua_runtime: LuaRuntime,

    /// [`Toasts`] are used to display notifications to the user.
    toasts: Arc<Mutex<Toasts>>,

    /// The field is used to display the current number of drawers.
    drawers: Drawers,

    name_buffer: Arc<Mutex<String>>,

    rename_buffer: Arc<Mutex<String>>,
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
            ManagerPane::Scripts(scripts) => {
                ui.allocate_space(vec2(ui.available_width(), 2.));

                ui.allocate_ui(vec2(ui.available_width(), ui.min_size().y), |ui| {
                    ui.horizontal_centered(|ui| {
                        let add_button = ui.button("Add");
                        if add_button.clicked() {
                            let name_buffer = &mut *self.name_buffer.lock();

                            if !scripts.contains_key(&*name_buffer) {
                                scripts.insert(name_buffer.clone(), String::from(""));
                                name_buffer.clear();
                            }
                            else {
                                self.toasts.lock().add(Toast::new().kind(egui_toast::ToastKind::Error).text(format!("The script named: {name_buffer} already exists. Please choose another name or rename an existing script.")));
                            }
                        }

                        ui.text_edit_singleline(&mut *self.name_buffer.lock());
                    });
                });

                ui.separator();

                let scripts_clone = scripts.clone();

                ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    scripts.retain2(|name, script| {
                        let mut should_keep = true;
                        ui.horizontal(|ui| {
                            ui.label(name.clone());
                            if ui.button("Run").clicked() {
                                let script = script.to_string();
                                if let Err(err) = self.lua_runtime.load(script).exec() {
                                    self.toasts.lock().add(
                                        Toast::new()
                                            .kind(egui_toast::ToastKind::Error)
                                            .text(err.to_string()),
                                    );
                                };
                            }
                            ui.push_id(name.clone(), |ui| {
                                ui.collapsing("Settings", |ui| {
                                    ui.menu_button("Edit", |ui| {
                                        ui.code_editor(script);
                                    });
                                    if ui.button("Delete").clicked() {
                                        should_keep = false;
                                    }
                                    let menu_button = ui.menu_button("Rename script", |ui| {
                                        ui.text_edit_singleline(&mut *self.rename_buffer.lock());
                                        if ui.button("Rename").clicked() {
                                            let name_buffer = &*self.rename_buffer.lock();
                                            if !scripts_clone.contains_key(name_buffer) {
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
                                });
                            });
                        });
                        should_keep
                    });
                });
            },
            ManagerPane::ItemManager => {
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
        }

        Default::default()
    }

    fn tab_title_for_pane(&mut self, pane: &ManagerPane) -> bevy_egui::egui::WidgetText
    {
        match pane {
            ManagerPane::Scripts(scripts) => format!("Scripts: {}", scripts.len()),
            ManagerPane::ItemManager => "Items".to_string(),
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

    bevy_egui::egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Manager").clicked() {
                    ui_state.manager_panel = !ui_state.manager_panel;
                }

                ui.menu_button("File", |ui| {
                    if ui.button("Save project").clicked() {
                        if let Some(save_path) = rfd::FileDialog::new()
                            .set_file_name("new_save")
                            .add_filter("Save file", &["dat"])
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
                            .add_filter("Save file", &["dat"])
                            .save_file()
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
            });
        });

    bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {
            let (_id, rect) = ui.allocate_space(vec2(ui.available_width(), 170.));

            ui.painter().rect_filled(rect, 5.0, Color32::BLACK);

            ui.allocate_new_ui(UiBuilder::new().max_rect(rect.shrink(10.)), |ui| {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .max_height(120.)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for output in ui_state.command_line_outputs.read().iter() {
                            match output {
                                ScriptLinePrompts::UserInput(text) => {
                                    ui.label(
                                        RichText::from(format!("> {text}")).color(Color32::GRAY),
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

                        // Create text editor
                        let text_edit = ui.add(
                            egui::TextEdit::singleline(&mut ui_state.command_line_buffer)
                                .frame(false)
                                .code_editor()
                                .desired_width(ui.available_size_before_wrap().x)
                                .hint_text(RichText::from("lua command").italics()),
                        );

                        // If the enter was pressed and the text editor was in focus execute the code written in the terminal.
                        if enter_was_pressed && text_edit.has_focus() {
                            if ui_state.command_line_buffer.clone() == "cls"
                                || ui_state.command_line_buffer.clone() == "clear"
                            {
                                ui_state.command_line_outputs.write().clear();
                            }
                            else {
                                ui_state.command_line_outputs.write().push(
                                    ScriptLinePrompts::UserInput(
                                        ui_state.command_line_buffer.clone(),
                                    ),
                                );
                                match lua_runtime
                                    .load(ui_state.command_line_buffer.clone())
                                    .exec()
                                {
                                    Ok(_output) => (),
                                    Err(_err) => {
                                        ui_state
                                            .command_line_outputs
                                            .write()
                                            .push(ScriptLinePrompts::Error(_err.to_string()));
                                    },
                                }
                            }

                            // Clear out the buffer regardless of the command being used.
                            ui_state.command_line_buffer.clear();
                        }
                    });
                });
            });
        });

    if ui_state.manager_panel {
        bevy_egui::egui::SidePanel::right("right_panel")
            .resizable(true)
            .show(ctx, |ui| {
                let code_manager_window = ui_state.code_manager_window.clone();

                let toasts = ui_state.toasts.clone();

                let rename_buffer = ui_state.rename_buffer.clone();
                let name_buffer = ui_state.name_buffer.clone();

                ui_state.item_manager.ui(
                    &mut ManagerBehavior {
                        code_manager_window,
                        lua_runtime: lua_runtime.clone(),
                        toasts,
                        drawers: drawers.clone(),
                        rename_buffer,
                        name_buffer,
                    },
                    ui,
                );
            });
    }
}
