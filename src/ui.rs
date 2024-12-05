use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use bevy::prelude::ResMut;
use bevy_egui::{egui, EguiContexts};

use bevy::prelude::Resource;
use egui_tiles::Tiles;
use indexmap::IndexMap;

use crate::LUA_RUNTIME;

#[derive(Resource, serde::Serialize, serde::Deserialize)]
pub struct UiState {
    pub manager_panel: bool,

    #[serde(skip)]
    pub code_manager_window: Arc<AtomicBool>,

    pub item_manager: egui_tiles::Tree<ManagerPane>,
}

impl Default for UiState {
    fn default() -> Self {
        Self { manager_panel: Default::default(), code_manager_window: Default::default(), item_manager: {
            let mut tiles = Tiles::default();
            let mut tileids = vec![];

            tileids.push(tiles.insert_pane(ManagerPane::ItemManager));
            tileids.push(tiles.insert_pane(ManagerPane::Scripts(IndexMap::new())));

            egui_tiles::Tree::new("manager_tree", tiles.insert_tab_tile(tileids), tiles)
        } }
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
pub enum ManagerPane {
    Scripts(IndexMap<String, String>),
    #[default]
    ItemManager,
}

pub struct ManagerBehavior {
    pub code_manager_window: Arc<AtomicBool>,
}

impl egui_tiles::Behavior<ManagerPane> for ManagerBehavior {
    fn pane_ui(&mut self, ui: &mut bevy_egui::egui::Ui, _tile_id: egui_tiles::TileId, pane: &mut ManagerPane) -> egui_tiles::UiResponse {
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
                            
                            if let Err(err) = LUA_RUNTIME.lock().unwrap().load(script).exec() {
                                panic!("{err}");
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
            },
            ManagerPane::ItemManager => {
                
            },
        }

        Default::default()
    }

    fn tab_title_for_pane(&mut self, pane: &ManagerPane) -> bevy_egui::egui::WidgetText {
        match pane {
            ManagerPane::Scripts(scripts) => format!("Scripts: {}", scripts.len()),
            ManagerPane::ItemManager => "Items".to_string(),
        }.into()
    }
}

pub fn main_ui(mut ui_state: ResMut<UiState>, mut contexts: EguiContexts<'_, '_>) {
    let ctx = contexts.ctx_mut();

    if ui_state.code_manager_window.load(Ordering::Relaxed) {
        let code_manager_window = ui_state.code_manager_window.clone();
        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("deferred_viewport"),
            egui::ViewportBuilder::default()
                .with_title("Deferred Viewport")
                .with_inner_size([200.0, 100.0]),
            move |ctx, class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Hello from deferred viewport");
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    // Tell parent to close us.
                    code_manager_window.store(false, Ordering::Relaxed);
                }
            },
        );
    }

    bevy_egui::egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Manager").clicked() {
                    ui_state.manager_panel = !ui_state.manager_panel;
                }
    
                ui.menu_button("File", |ui| {
                    if ui.button("Save project").clicked() {
    
                    };
    
                    if ui.button("Open project").clicked() {
    
                    };
                });
            });
        });
    
    bevy_egui::egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {
            
        });

    if ui_state.manager_panel {
        bevy_egui::egui::SidePanel::right("right_panel")
        .resizable(true)
        .show(ctx, |ui| {
            let code_manager_window =  ui_state.code_manager_window.clone();

            ui_state.item_manager.ui(&mut ManagerBehavior {
                code_manager_window 
            }, ui);
        });
    }
}
