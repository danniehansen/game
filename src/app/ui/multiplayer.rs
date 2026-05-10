use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

use anyhow::{Result, bail};
use bevy_egui::egui;

use crate::{
    app::state::{ClientRuntime, DirectConnectDialog, MenuState, Screen, SteamUser},
    net::ClientSession,
    steam::{OfflineSteamBackend, SteamBackend},
};

use super::{
    modal,
    theme::{self, ButtonKind},
};

const DIRECT_CONNECT_HOST_INPUT_ID: &str = "direct_connect_host_input";
const DIRECT_CONNECT_PORT_INPUT_ID: &str = "direct_connect_port_input";
const DIRECT_CONNECT_FIELD_HEIGHT: f32 = 34.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectConnectChoice {
    Connect,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
struct DirectConnectModalOutput {
    choice: Option<DirectConnectChoice>,
    finished_closing: bool,
}

pub(super) fn multiplayer_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    user: &SteamUser,
) {
    theme::screen_scrim(ctx, "multiplayer_scrim", 145);
    handle_multiplayer_escape(ctx, menu);
    theme::anchored_panel(
        ctx,
        "multiplayer_panel",
        560.0,
        egui::Align2::CENTER_CENTER,
        [0.0, -10.0],
        |ui| {
            draw_multiplayer_header(ui, menu);

            ui.add_space(16.0);
            theme::inset_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.label(theme::field_label("Steam"));
                    if theme::game_button(
                        ui,
                        "Open Server Browser",
                        ButtonKind::Primary,
                        ui.available_width(),
                    )
                    .clicked()
                    {
                        let steam = OfflineSteamBackend;
                        menu.status = match steam.open_server_browser() {
                            Ok(()) => Some("opened Steam server browser".to_owned()),
                            Err(error) => Some(format!("Steam browser unavailable: {error}")),
                        };
                    }
                });
            });

            if let Some(status) = &menu.status {
                ui.add_space(10.0);
                ui.label(theme::status_text(status));
            }
        },
    );
    direct_connect_dialog_ui(ctx, menu, runtime, user);
}

fn draw_multiplayer_header(ui: &mut egui::Ui, menu: &mut MenuState) {
    if ui.available_width() < 340.0 {
        ui.label(theme::section("Multiplayer"));
        ui.add_space(4.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_header_buttons(ui, menu);
        });
        return;
    }

    ui.horizontal(|ui| {
        ui.label(theme::section("Multiplayer"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_header_buttons(ui, menu);
        });
    });
}

fn draw_header_buttons(ui: &mut egui::Ui, menu: &mut MenuState) {
    if theme::compact_button(ui, "Back", ButtonKind::Secondary, 78.0).clicked() {
        menu.screen = Screen::MainMenu;
    }
    if theme::compact_button(ui, "Direct Connect", ButtonKind::Primary, 128.0).clicked() {
        menu.direct_connect = Some(DirectConnectDialog::new(&menu.multiplayer_addr));
    }
}

fn handle_multiplayer_escape(ctx: &egui::Context, menu: &mut MenuState) {
    if !ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        return;
    }

    if let Some(dialog) = menu.direct_connect.as_mut() {
        dialog.closing = true;
        ctx.request_repaint();
        return;
    }

    menu.screen = Screen::MainMenu;
}

fn direct_connect_dialog_ui(
    ctx: &egui::Context,
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    user: &SteamUser,
) {
    let mut connect_addr = None;
    let finished_closing;
    {
        let Some(dialog) = menu.direct_connect.as_mut() else {
            return;
        };

        let output = direct_connect_modal(ctx, dialog, !dialog.closing);
        if let Some(choice) = output.choice {
            match choice {
                DirectConnectChoice::Connect => match direct_connect_addr(dialog) {
                    Ok(addr) => {
                        dialog.error = None;
                        connect_addr = Some(addr);
                    }
                    Err(error) => {
                        dialog.error = Some(error.to_string());
                        ctx.request_repaint();
                    }
                },
                DirectConnectChoice::Cancel => {
                    dialog.closing = true;
                    ctx.request_repaint();
                }
            }
        }
        finished_closing = output.finished_closing;
    }

    if finished_closing {
        menu.direct_connect = None;
    }

    if let Some(addr) = connect_addr {
        connect_to_addr(menu, runtime, user, addr);
    }
}

fn direct_connect_modal(
    ctx: &egui::Context,
    dialog: &mut DirectConnectDialog,
    open: bool,
) -> DirectConnectModalOutput {
    let id = egui::Id::new("direct_connect_modal");
    let animation = ctx.animate_bool_with_time(id.with("animation"), open, 0.16);
    if animation > 0.0 && animation < 1.0 {
        ctx.request_repaint();
    }

    if !open && animation <= 0.01 {
        return DirectConnectModalOutput {
            choice: None,
            finished_closing: true,
        };
    }

    let screen_rect = ctx.content_rect();
    let backdrop_response = egui::Area::new(id.with("backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen_rect.min)
        .show(ctx, |ui| {
            let local_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, screen_rect.size());
            let response = ui.allocate_rect(local_rect, egui::Sense::click());
            ui.painter().rect_filled(
                local_rect,
                0,
                egui::Color32::from_rgba_unmultiplied(1, 3, 8, (190.0 * animation) as u8),
            );
            response
        })
        .inner;

    let panel_width = screen_rect.width().clamp(340.0, 440.0);
    let mut choice = None;
    let panel_response = egui::Area::new(id.with("panel"))
        .order(egui::Order::Tooltip)
        .anchor(
            egui::Align2::CENTER_CENTER,
            [0.0, 18.0 * (1.0 - animation.clamp(0.0, 1.0))],
        )
        .show(ctx, |ui| {
            ui.set_width(panel_width);
            ui.multiply_opacity(animation);
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(12, 17, 23, 246))
                .stroke(egui::Stroke::new(1.0, theme::panel_stroke()))
                .corner_radius(7)
                .inner_margin(egui::Margin::symmetric(24, 22))
                .show(ui, |ui| {
                    ui.set_width(panel_width - 48.0);
                    draw_direct_connect_form(ui, dialog, &mut choice);
                });
        })
        .response;

    if open && choice.is_none() && modal::confirm_shortcut_pressed(ctx) {
        choice = Some(DirectConnectChoice::Connect);
    }

    if open && choice.is_none() && backdrop_response.clicked() {
        let clicked_outside_panel = ctx.input(|input| {
            input
                .pointer
                .interact_pos()
                .is_some_and(|position| !panel_response.rect.contains(position))
        });
        if clicked_outside_panel {
            choice = Some(DirectConnectChoice::Cancel);
        }
    }

    DirectConnectModalOutput {
        choice,
        finished_closing: false,
    }
}

fn draw_direct_connect_form(
    ui: &mut egui::Ui,
    dialog: &mut DirectConnectDialog,
    choice: &mut Option<DirectConnectChoice>,
) {
    ui.label(theme::section("Direct Connect"));
    ui.add_space(12.0);

    ui.label(theme::field_label("Server Address"));
    ui.add_sized(
        [ui.available_width(), DIRECT_CONNECT_FIELD_HEIGHT],
        theme::text_input(&mut dialog.host).id(egui::Id::new(DIRECT_CONNECT_HOST_INPUT_ID)),
    );

    ui.add_space(6.0);
    ui.label(theme::field_label("Port"));
    ui.add_sized(
        [ui.available_width(), DIRECT_CONNECT_FIELD_HEIGHT],
        theme::text_input(&mut dialog.port).id(egui::Id::new(DIRECT_CONNECT_PORT_INPUT_ID)),
    );

    if let Some(error) = &dialog.error {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(error)
                .size(13.0)
                .color(egui::Color32::from_rgb(255, 154, 130)),
        );
    }

    ui.add_space(18.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if theme::compact_button(ui, "Connect", ButtonKind::Primary, 92.0).clicked() {
            *choice = Some(DirectConnectChoice::Connect);
        }
        if theme::compact_button(ui, "Cancel", ButtonKind::Secondary, 92.0).clicked() {
            *choice = Some(DirectConnectChoice::Cancel);
        }
    });
}

fn direct_connect_addr(dialog: &DirectConnectDialog) -> Result<SocketAddr> {
    let host_input = dialog.host.trim();
    if let Ok(addr) = host_input.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let (host, port_input) =
        split_inline_host_port(host_input).unwrap_or((host_input, dialog.port.trim()));
    if host.is_empty() {
        bail!("Server address is required.");
    }

    let Ok(port) = port_input.parse::<u16>() else {
        bail!("Port must be a number between 1 and 65535.");
    };
    if port == 0 {
        bail!("Port must be a number between 1 and 65535.");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }

    (host, port)
        .to_socket_addrs()
        .map_err(|_| anyhow::anyhow!("Could not resolve server address."))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve server address."))
}

fn split_inline_host_port(host_input: &str) -> Option<(&str, &str)> {
    if let Some(bracketed) = host_input.strip_prefix('[') {
        let (host, port) = bracketed.rsplit_once("]:")?;
        return Some((host, port));
    }

    if host_input.matches(':').count() == 1 {
        return host_input.rsplit_once(':');
    }

    None
}

fn connect_to_addr(
    menu: &mut MenuState,
    runtime: &mut ClientRuntime,
    user: &SteamUser,
    addr: SocketAddr,
) {
    match ClientSession::connect(addr, &user.0) {
        Ok(session) => {
            runtime.start_session(session, None);
            menu.multiplayer_addr = addr.to_string();
            menu.direct_connect = None;
            menu.screen = Screen::InGame;
            menu.pause_open = false;
            menu.pause_options_open = false;
            menu.chat_open = false;
            menu.chat_focus_pending = false;
            menu.status = None;
        }
        Err(error) => {
            if let Some(dialog) = menu.direct_connect.as_mut() {
                dialog.error = Some(format!("Connection failed: {error}"));
            } else {
                menu.status = Some(format!("Connection failed: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_connect_addr_parses_ip_host_and_port() {
        let dialog = DirectConnectDialog {
            host: "127.0.0.1".to_owned(),
            port: "7777".to_owned(),
            error: None,
            closing: false,
        };

        assert_eq!(
            direct_connect_addr(&dialog).expect("address should parse"),
            SocketAddr::from(([127, 0, 0, 1], 7777))
        );
    }

    #[test]
    fn direct_connect_addr_accepts_pasted_host_and_port() {
        let dialog = DirectConnectDialog {
            host: "127.0.0.1:8888".to_owned(),
            port: "7777".to_owned(),
            error: None,
            closing: false,
        };

        assert_eq!(
            direct_connect_addr(&dialog).expect("address should parse"),
            SocketAddr::from(([127, 0, 0, 1], 8888))
        );
    }

    #[test]
    fn direct_connect_addr_rejects_empty_host_and_invalid_port() {
        let empty_host = DirectConnectDialog {
            host: " ".to_owned(),
            port: "7777".to_owned(),
            error: None,
            closing: false,
        };
        assert!(direct_connect_addr(&empty_host).is_err());

        let invalid_port = DirectConnectDialog {
            host: "127.0.0.1".to_owned(),
            port: "0".to_owned(),
            error: None,
            closing: false,
        };
        assert!(direct_connect_addr(&invalid_port).is_err());
    }
}
