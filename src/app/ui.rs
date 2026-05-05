use std::net::SocketAddr;

use anyhow::Context;
use bevy::{app::AppExit, prelude::*};
use bevy_egui::{EguiContexts, egui};
use uuid::Uuid;

use crate::{
    net::ClientSession,
    protocol::{ClientMessage, MAX_HEALTH, MAX_STAMINA},
    steam::{OfflineSteamBackend, SteamBackend},
};

use super::state::{ClientRuntime, MenuState, SaveStore, Screen, SteamUser};

const HUD_WIDTH: f32 = 240.0;
const CHAT_WIDTH: f32 = 420.0;

pub(crate) fn ui_system(
    mut contexts: EguiContexts,
    mut menu: ResMut<MenuState>,
    mut runtime: ResMut<ClientRuntime>,
    store: Res<SaveStore>,
    user: Res<SteamUser>,
    mut app_exit: MessageWriter<AppExit>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;

    match menu.screen {
        Screen::MainMenu => main_menu_ui(ctx, &mut menu, &store, &user, &mut app_exit),
        Screen::Worlds => worlds_ui(ctx, &mut menu, &mut runtime, &store, &user),
        Screen::Multiplayer => multiplayer_ui(ctx, &mut menu, &mut runtime, &user),
        Screen::InGame => {
            hud_ui(ctx, &runtime);
            chat_ui(ctx, &mut menu, &mut runtime);
            if menu.pause_open {
                pause_ui(ctx, &mut menu, &mut runtime, &store);
            }
        }
    }

    Ok(())
}

fn hud_ui(ctx: &egui::Context, runtime: &ClientRuntime) {
    let Some(player) = runtime.local_view() else {
        return;
    };

    egui::Area::new("hud_bars".into())
        .anchor(egui::Align2::LEFT_TOP, [16.0, 16.0])
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 145))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(HUD_WIDTH);
                    status_bar(
                        ui,
                        "Health",
                        player.health,
                        MAX_HEALTH,
                        egui::Color32::from_rgb(190, 55, 58),
                    );
                    ui.add_space(6.0);
                    status_bar(
                        ui,
                        "Stamina",
                        player.stamina,
                        MAX_STAMINA,
                        egui::Color32::from_rgb(61, 159, 104),
                    );
                });
        });
}

fn status_bar(ui: &mut egui::Ui, label: &str, value: f32, max: f32, color: egui::Color32) {
    let fraction = (value / max).clamp(0.0, 1.0);
    ui.label(label);
    ui.add(
        egui::ProgressBar::new(fraction)
            .fill(color)
            .text(format!("{value:.0}/{max:.0}"))
            .desired_width(HUD_WIDTH - 20.0),
    );
}

fn main_menu_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    store: &SaveStore,
    user: &SteamUser,
    app_exit: &mut MessageWriter<AppExit>,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgba_unmultiplied(4, 5, 7, 235)))
        .show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.vertical_centered(|ui| {
                        ui.heading(egui::RichText::new("Game").size(72.0));
                        ui.add_space(32.0);
                        if menu_button(ui, "Singleplayer").clicked() {
                            refresh_worlds(menu, store);
                            menu.screen = Screen::Worlds;
                        }
                        if menu_button(ui, "Multiplayer").clicked() {
                            let steam = OfflineSteamBackend;
                            menu.status = match steam.open_server_browser() {
                                Ok(()) => Some("opened Steam server browser".to_owned()),
                                Err(error) => Some(format!("Steam browser unavailable: {error}")),
                            };
                            menu.screen = Screen::Multiplayer;
                        }
                        if menu_button(ui, "Quit").clicked() {
                            app_exit.write(AppExit::Success);
                        }

                        ui.add_space(18.0);
                        ui.label(format!("Signed in as {}", user.0.display_name));
                        if let Some(status) = &menu.status {
                            ui.label(status);
                        }
                    });
                },
            );
        });
}

fn worlds_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    store: &SaveStore,
    user: &SteamUser,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgba_unmultiplied(8, 10, 13, 238)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Singleplayer Worlds");
                ui.add_space(12.0);
            });

            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut menu.new_world_name);
                if ui.button("Create").clicked() {
                    match store
                        .0
                        .create_world(&menu.new_world_name, Some(user.0.steam_id))
                    {
                        Ok(_) => {
                            menu.new_world_name = "New World".to_owned();
                            refresh_worlds(menu, store);
                        }
                        Err(error) => menu.status = Some(format!("create failed: {error}")),
                    }
                }
                if ui.button("Refresh").clicked() {
                    refresh_worlds(menu, store);
                }
                if ui.button("Back").clicked() {
                    menu.screen = Screen::MainMenu;
                }
            });

            ui.add_space(12.0);
            egui::Grid::new("world_table")
                .striped(true)
                .num_columns(5)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Seed");
                    ui.strong("Admins");
                    ui.strong("Start");
                    ui.strong("Delete");
                    ui.end_row();

                    let worlds = menu.worlds.clone();
                    for world in worlds {
                        ui.label(&world.name);
                        ui.monospace(world.seed.to_string());
                        ui.label(world.admin_count.to_string());
                        if ui.button("Start").clicked() {
                            start_singleplayer(menu, runtime, store, user, world.id);
                        }
                        if ui.button("Delete").clicked() {
                            match store.0.delete_world(world.id) {
                                Ok(()) => refresh_worlds(menu, store),
                                Err(error) => menu.status = Some(format!("delete failed: {error}")),
                            }
                        }
                        ui.end_row();
                    }
                });

            if menu.worlds.is_empty() {
                ui.add_space(12.0);
                ui.label("No worlds yet.");
            }

            if let Some(status) = &menu.status {
                ui.add_space(12.0);
                ui.label(status);
            }
        });
}

fn multiplayer_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    user: &SteamUser,
) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgba_unmultiplied(8, 10, 13, 238)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Multiplayer");
                ui.add_space(12.0);
            });

            ui.horizontal(|ui| {
                if ui.button("Steam Server Browser").clicked() {
                    let steam = OfflineSteamBackend;
                    menu.status = match steam.open_server_browser() {
                        Ok(()) => Some("opened Steam server browser".to_owned()),
                        Err(error) => Some(format!("Steam browser unavailable: {error}")),
                    };
                }
                if ui.button("Back").clicked() {
                    menu.screen = Screen::MainMenu;
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Direct UDP");
                ui.text_edit_singleline(&mut menu.multiplayer_addr);
                if ui.button("Connect").clicked() {
                    match menu.multiplayer_addr.parse::<SocketAddr>() {
                        Ok(addr) => match ClientSession::connect_udp(addr, &user.0) {
                            Ok(session) => {
                                runtime.start_session(session, None);
                                menu.screen = Screen::InGame;
                                menu.pause_open = false;
                                menu.status = None;
                            }
                            Err(error) => {
                                menu.status = Some(format!("connect failed: {error}"));
                            }
                        },
                        Err(error) => menu.status = Some(format!("invalid address: {error}")),
                    }
                }
            });

            if let Some(status) = &menu.status {
                ui.add_space(12.0);
                ui.label(status);
            }
        });
}

fn chat_ui(ctx: &egui::Context, menu: &mut MenuState, runtime: &mut ClientRuntime) {
    egui::Area::new("chat".into())
        .anchor(egui::Align2::LEFT_BOTTOM, [16.0, -16.0])
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 135))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(CHAT_WIDTH);
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for message in &runtime.messages {
                                ui.label(message);
                            }
                        });

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut menu.chat_input)
                            .hint_text("Chat")
                            .desired_width(CHAT_WIDTH - 20.0),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        send_chat(menu, runtime);
                    }
                });
        });
}

fn pause_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    store: &SaveStore,
) {
    let screen_rect = ctx.content_rect();
    let backdrop_response = egui::Area::new("pause_backdrop".into())
        .order(egui::Order::Middle)
        .fixed_pos(screen_rect.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(screen_rect, egui::Sense::click());
            ui.painter().rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 185),
            );
            response
        })
        .inner;

    if backdrop_response.clicked() {
        menu.pause_open = false;
    }

    egui::Window::new("Paused")
        .order(egui::Order::Foreground)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(egui::Color32::from_rgba_unmultiplied(18, 20, 24, 245)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(220.0);
            ui.vertical_centered(|ui| {
                ui.heading("Paused");
                ui.add_space(12.0);
                if menu_button(ui, "Resume").clicked() {
                    menu.pause_open = false;
                }
                if menu_button(ui, "Quit").clicked() {
                    runtime.shutdown(&store.0);
                    menu.screen = Screen::MainMenu;
                    menu.pause_open = false;
                }
            });
        });
}

fn menu_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized([260.0, 44.0], egui::Button::new(text))
}

fn refresh_worlds(menu: &mut MenuState, store: &SaveStore) {
    match store.0.list_worlds() {
        Ok(worlds) => {
            menu.worlds = worlds;
            menu.status = None;
        }
        Err(error) => {
            menu.worlds.clear();
            menu.status = Some(format!("world list failed: {error}"));
        }
    }
}

fn start_singleplayer(
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    store: &SaveStore,
    user: &SteamUser,
    world_id: Uuid,
) {
    let result = store
        .0
        .load_world(world_id)
        .context("could not load selected world")
        .and_then(|save| ClientSession::start_singleplayer(save, &user.0));

    match result {
        Ok(session) => {
            runtime.start_session(session, Some(world_id));
            menu.screen = Screen::InGame;
            menu.pause_open = false;
            menu.status = None;
        }
        Err(error) => menu.status = Some(format!("start failed: {error}")),
    }
}

fn send_chat(menu: &mut MenuState, runtime: &mut ClientRuntime) {
    let text = std::mem::take(&mut menu.chat_input);
    if text.trim().is_empty() {
        return;
    }

    if let Some(session) = runtime.session.as_mut()
        && let Err(error) = session.send(ClientMessage::Chat { text })
    {
        runtime.messages.push(format!("chat send failed: {error}"));
    }
}
