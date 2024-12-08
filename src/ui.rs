use bevy::prelude::{Res, ResMut};
use bevy_egui::{
    egui::{self, ScrollArea},
    EguiContexts,
};
use miniz_oxide::deflate::CompressionLevel;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use bevy::prelude::Resource;
use egui_tiles::Tiles;
use egui_toast::{Toast, Toasts};
use indexmap::IndexMap;

use crate::{Drawers, LuaRuntime};

#[derive(Resource, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UiState {
    /// Should the manager panel open.
    pub manager_panel: bool,

    /// Egui notifications
    #[serde(skip)]
    toasts: Arc<Mutex<Toasts>>,

    #[serde(skip)]
    /// Should the new viewport open? NOTE: This egui backend doesnt support multiple viewports.
    pub code_manager_window: Arc<AtomicBool>,

    /// The manager panel's tab state.
    pub item_manager: egui_tiles::Tree<ManagerPane>,
}

impl Default for UiState {
    fn default() -> Self {
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
        }
    }
}

/// The manager panel's tabs.
#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub enum ManagerPane {
    Scripts(IndexMap<String, String>),
    #[default]
    ItemManager,
}

/// The manager panel's inner behavior, the data it contains, this can be used to share data over to the tabs from the main ui.
pub struct ManagerBehavior {
    /// Should the new viewport open? NOTE: This egui backend doesnt support multiple viewports.
    pub code_manager_window: Arc<AtomicBool>,

    /// The [`mlua::Lua`] runtime handle, this can be used to run code on.
    pub lua_runtime: LuaRuntime,

    /// [`Toasts`] are used to display notifications to the user.
    toasts: Arc<Mutex<Toasts>>,

    /// The field is used to display the current number of drawers.
    drawers: Drawers,
}

impl egui_tiles::Behavior<ManagerPane> for ManagerBehavior {
    fn pane_ui(
        &mut self,
        ui: &mut bevy_egui::egui::Ui,
        _tile_id: egui_tiles::TileId,
        pane: &mut ManagerPane,
    ) -> egui_tiles::UiResponse {
        match pane {
            ManagerPane::Scripts(scripts) => {
                ui.horizontal(|ui| {
                    if ui.button("Add").clicked() {
                        scripts.insert(format!("script{}", scripts.len()), String::from(""));
                    }
                });

                ui.separator();

                scripts.retain(|name, script| {
                    let mut should_keep = true;
                    ui.horizontal(|ui| {
                        ui.label(name);

                        if ui.button("Run").clicked() {
                            let script = script.to_string();

                            if let Err(err) = self.lua_runtime.load(script).exec() {
                                self.toasts.lock().unwrap().add(
                                    Toast::new()
                                        .kind(egui_toast::ToastKind::Error)
                                        .text(err.to_string()),
                                );
                            };
                        }

                        ui.push_id(name, |ui| {
                            ui.collapsing("Settings", |ui| {
                                ui.menu_button("Edit", |ui| {
                                    ui.code_editor(script);
                                });

                                if ui.button("Delete").clicked() {
                                    should_keep = false;
                                }
                            });
                        });
                    });

                    should_keep
                });
            }
            ManagerPane::ItemManager => {
                ScrollArea::vertical().show(ui, |ui| {
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
            }
        }

        Default::default()
    }

    fn tab_title_for_pane(&mut self, pane: &ManagerPane) -> bevy_egui::egui::WidgetText {
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
) {
    let ctx = contexts.ctx_mut();

    egui_extras::install_image_loaders(ctx);

    ui_state.toasts.lock().unwrap().show(ctx);

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
                            .add_filter("Save file", &[".dat"])
                            .save_file()
                        {
                            let compressed_data = miniz_oxide::deflate::compress_to_vec(
                                &rmp_serde::to_vec(&*ui_state).unwrap(),
                                CompressionLevel::BestCompression as u8,
                            );

                            if let Err(err) = fs::write(save_path, compressed_data) {
                                ui_state.toasts.lock().unwrap().add(
                                    Toast::new()
                                        .kind(egui_toast::ToastKind::Error)
                                        .text(err.to_string()),
                                );
                            };
                        }
                    };

                    if ui.button("Open project").clicked() {};
                });
            });
        });

    bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {});

    if ui_state.manager_panel {
        bevy_egui::egui::SidePanel::right("right_panel")
            .resizable(true)
            .show(ctx, |ui| {
                let code_manager_window = ui_state.code_manager_window.clone();

                let toasts = ui_state.toasts.clone();

                ui_state.item_manager.ui(
                    &mut ManagerBehavior {
                        code_manager_window,
                        lua_runtime: lua_runtime.clone(),
                        toasts,
                        drawers: drawers.clone(),
                    },
                    ui,
                );
            });
    }
}
