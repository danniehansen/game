use std::{collections::HashMap, net::SocketAddr};

use anyhow::{Context, Result};
use bevy::{app::AppExit, prelude::*};
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui,
    input::{egui_wants_any_keyboard_input, egui_wants_any_pointer_input},
};
use uuid::Uuid;

use crate::{
    net::ClientSession,
    protocol::{
        ChatMessage, ClientId, ClientMessage, PlayerEvent, PlayerInput, ServerMessage, Vec3Net,
        WorldSnapshot,
    },
    save::{WorldStore, WorldSummary},
    steam::{AuthenticatedUser, OfflineSteamBackend, SteamBackend},
};

const LOCAL_PLAYER_COLOR: Color = Color::srgb(0.25, 0.68, 0.95);
const REMOTE_PLAYER_COLOR: Color = Color::srgb(0.95, 0.61, 0.25);
const WORLD_COLOR: Color = Color::srgb(0.18, 0.34, 0.22);

pub fn run_app() -> Result<()> {
    let store = WorldStore::platform_default()?;
    store.ensure_exists()?;

    let steam = OfflineSteamBackend;
    let user = steam.current_user()?;

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.015, 0.018, 0.023)))
        .insert_resource(SaveStore(store))
        .insert_resource(SteamUser(user))
        .insert_resource(MenuState::default())
        .insert_resource(ClientRuntime::default())
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Game".to_owned(),
                resolution: (1280, 720).into(),
                present_mode: bevy::window::PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, setup_scene)
        .add_systems(EguiPrimaryContextPass, ui_system)
        .add_systems(
            Update,
            toggle_pause_system.run_if(not(egui_wants_any_keyboard_input)),
        )
        .add_systems(
            Update,
            client_input_system.run_if(not(egui_wants_any_keyboard_input)),
        )
        .add_systems(Update, network_tick_system)
        .add_systems(Update, apply_snapshot_system)
        .add_systems(Update, interpolate_players_system)
        .add_systems(
            Update,
            camera_follow_system.run_if(not(egui_wants_any_pointer_input)),
        )
        .run();

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    MainMenu,
    Worlds,
    Multiplayer,
    InGame,
}

#[derive(Resource)]
struct SaveStore(WorldStore);

#[derive(Resource)]
struct SteamUser(AuthenticatedUser);

#[derive(Resource)]
struct MenuState {
    screen: Screen,
    worlds: Vec<WorldSummary>,
    new_world_name: String,
    multiplayer_addr: String,
    status: Option<String>,
    pause_open: bool,
    chat_input: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            screen: Screen::MainMenu,
            worlds: Vec::new(),
            new_world_name: "New World".to_owned(),
            multiplayer_addr: "127.0.0.1:7777".to_owned(),
            status: None,
            pause_open: false,
            chat_input: String::new(),
        }
    }
}

#[derive(Resource, Default)]
struct ClientRuntime {
    session: Option<ClientSession>,
    active_world_id: Option<Uuid>,
    client_id: Option<ClientId>,
    is_admin: bool,
    snapshot: Option<WorldSnapshot>,
    messages: Vec<String>,
    input_sequence: u64,
}

impl ClientRuntime {
    fn start_session(&mut self, session: ClientSession, world_id: Option<Uuid>) {
        self.session = Some(session);
        self.active_world_id = world_id;
        self.client_id = None;
        self.is_admin = false;
        self.snapshot = None;
        self.messages.clear();
        self.input_sequence = 0;
    }

    fn shutdown(&mut self, store: &WorldStore) {
        if let Some(session) = self.session.as_mut()
            && let Err(error) = session.shutdown(store)
        {
            self.messages.push(format!("save/shutdown error: {error}"));
        }

        self.session = None;
        self.active_world_id = None;
        self.client_id = None;
        self.snapshot = None;
        self.is_admin = false;
    }

    fn apply_message(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Welcome {
                client_id,
                is_admin,
                snapshot,
                ..
            } => {
                self.client_id = Some(client_id);
                self.is_admin = is_admin;
                self.snapshot = Some(snapshot);
                self.messages
                    .push(format!("connected as player {client_id}"));
            }
            ServerMessage::AuthRejected { reason } => {
                self.messages.push(format!("auth rejected: {reason}"));
            }
            ServerMessage::PlayerEvent(event) => self.messages.push(format_player_event(event)),
            ServerMessage::Snapshot(snapshot) => {
                self.snapshot = Some(snapshot);
            }
            ServerMessage::Chat(ChatMessage { from, text }) => {
                self.messages.push(format!("{from}: {text}"));
            }
        }

        if self.messages.len() > 80 {
            let drain_count = self.messages.len() - 80;
            self.messages.drain(0..drain_count);
        }
    }
}

#[derive(Resource, Clone)]
struct PlayerVisualAssets {
    mesh: Handle<Mesh>,
    local_material: Handle<StandardMaterial>,
    remote_material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct NetworkPlayer {
    client_id: ClientId,
}

#[derive(Component)]
struct TargetPosition(Vec3);

#[derive(Component)]
struct MainCamera;

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Camera"),
        MainCamera,
        Camera3d::default(),
        Transform::from_xyz(6.0, 7.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 16_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(-3.0, 8.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        Name::new("Authoritative Plane"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, 80.0).subdivisions(16))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: WORLD_COLOR,
            perceptual_roughness: 0.9,
            cull_mode: None,
            ..default()
        })),
    ));

    commands.insert_resource(PlayerVisualAssets {
        mesh: meshes.add(Capsule3d::new(0.35, 0.9)),
        local_material: materials.add(LOCAL_PLAYER_COLOR),
        remote_material: materials.add(REMOTE_PLAYER_COLOR),
    });
}

fn ui_system(
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
            chat_ui(ctx, &mut menu, &mut runtime);
            if menu.pause_open {
                pause_ui(ctx, &mut menu, &mut runtime, &store);
            }
        }
    }

    Ok(())
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
                    ui.set_min_width(360.0);
                    ui.set_max_width(420.0);
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
                            .desired_width(f32::INFINITY),
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

fn toggle_pause_system(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<MenuState>) {
    if menu.screen != Screen::InGame {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        menu.pause_open = !menu.pause_open;
    }
}

fn client_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut runtime: ResMut<ClientRuntime>,
    menu: Res<MenuState>,
) {
    if menu.screen != Screen::InGame || menu.pause_open {
        return;
    }

    let mut direction = Vec3Net::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction.x += 1.0;
    }

    runtime.input_sequence += 1;
    let sequence = runtime.input_sequence;
    let input = PlayerInput {
        sequence,
        direction,
        sprint: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    };

    if let Some(session) = runtime.session.as_mut() {
        let _ = session.send(ClientMessage::Input(input));
    }
}

fn network_tick_system(time: Res<Time>, mut runtime: ResMut<ClientRuntime>, menu: Res<MenuState>) {
    if menu.screen != Screen::InGame {
        return;
    }

    let tick_result = runtime
        .session
        .as_mut()
        .map(|session| session.tick(time.delta_secs()));
    let messages = match tick_result {
        Some(Ok(messages)) => messages,
        Some(Err(error)) => {
            runtime.messages.push(format!("network error: {error}"));
            Vec::new()
        }
        None => Vec::new(),
    };

    for message in messages {
        runtime.apply_message(message);
    }
}

fn apply_snapshot_system(
    mut commands: Commands,
    runtime: Res<ClientRuntime>,
    assets: Res<PlayerVisualAssets>,
    players: Query<(Entity, &NetworkPlayer)>,
) {
    let Some(snapshot) = &runtime.snapshot else {
        for (entity, _) in &players {
            commands.entity(entity).despawn();
        }
        return;
    };

    let existing = players
        .iter()
        .map(|(entity, player)| (player.client_id, entity))
        .collect::<HashMap<_, _>>();

    for player in &snapshot.players {
        let target = Vec3::new(player.position.x, player.position.y, player.position.z);
        if let Some(entity) = existing.get(&player.client_id) {
            commands.entity(*entity).insert(TargetPosition(target));
        } else {
            let material = if Some(player.client_id) == runtime.client_id {
                assets.local_material.clone()
            } else {
                assets.remote_material.clone()
            };
            commands.spawn((
                Name::new(format!("Player {}", player.client_id)),
                NetworkPlayer {
                    client_id: player.client_id,
                },
                TargetPosition(target),
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(target),
            ));
        }
    }

    for (entity, network_player) in &players {
        if !snapshot
            .players
            .iter()
            .any(|player| player.client_id == network_player.client_id)
        {
            commands.entity(entity).despawn();
        }
    }
}

fn interpolate_players_system(
    time: Res<Time>,
    mut players: Query<(&mut Transform, &TargetPosition), With<NetworkPlayer>>,
) {
    let alpha = 1.0 - (-18.0 * time.delta_secs()).exp();
    for (mut transform, target) in &mut players {
        transform.translation = transform.translation.lerp(target.0, alpha);
    }
}

fn camera_follow_system(
    runtime: Res<ClientRuntime>,
    mut camera: Query<&mut Transform, (With<MainCamera>, Without<NetworkPlayer>)>,
    players: Query<(&Transform, &NetworkPlayer)>,
) {
    let Some(client_id) = runtime.client_id else {
        return;
    };
    let Some((player_transform, _)) = players
        .iter()
        .find(|(_, player)| player.client_id == client_id)
    else {
        return;
    };

    let Ok(mut camera_transform) = camera.single_mut() else {
        return;
    };

    let target = player_transform.translation;
    let desired = target + Vec3::new(6.0, 7.0, 8.0);
    camera_transform.translation = camera_transform.translation.lerp(desired, 0.08);
    camera_transform.look_at(target, Vec3::Y);
}

fn format_player_event(event: PlayerEvent) -> String {
    match event {
        PlayerEvent::Joined { name, .. } => format!("{name} joined"),
        PlayerEvent::Left { name, .. } => format!("{name} left"),
    }
}
