use std::{collections::HashMap, f32::consts::FRAC_PI_2};

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};

use crate::protocol::{ClientMessage, PlayerInput, Vec3Net};

use super::{
    EYE_HEIGHT,
    scene::{
        MainCamera, NetworkPlayer, PlayerVisualAssets, TargetPosition, TargetRotation,
        player_visual_position,
    },
    state::{ClientRuntime, LookState, MenuState, Screen},
};

pub(crate) fn chat_shortcut_system(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<MenuState>) {
    if menu.screen != Screen::InGame || menu.pause_open || menu.chat_open {
        return;
    }

    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::KeyT) {
        menu.chat_open = true;
        menu.chat_focus_pending = true;
        menu.chat_input.clear();
    }
}

pub(crate) fn toggle_pause_system(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<MenuState>) {
    if menu.screen != Screen::InGame {
        return;
    }
    if menu.chat_open {
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        menu.pause_open = !menu.pause_open;
    }
}

pub(crate) fn update_cursor_system(
    mut cursor_options: Single<&mut CursorOptions>,
    menu: Res<MenuState>,
) {
    let should_capture = menu.screen == Screen::InGame && !menu.pause_open && !menu.chat_open;
    cursor_options.visible = !should_capture;
    cursor_options.grab_mode = if should_capture {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
}

pub(crate) fn mouse_look_system(
    accumulated_mouse_motion: Res<AccumulatedMouseMotion>,
    mut look: ResMut<LookState>,
    menu: Res<MenuState>,
) {
    if menu.screen != Screen::InGame || menu.pause_open || menu.chat_open {
        return;
    }

    let delta = accumulated_mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    look.yaw -= delta.x * look.sensitivity.x;
    look.pitch =
        (look.pitch - delta.y * look.sensitivity.y).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
}

pub(crate) fn client_input_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut runtime: ResMut<ClientRuntime>,
    menu: Res<MenuState>,
    look: Res<LookState>,
) {
    if menu.screen != Screen::InGame || menu.pause_open || menu.chat_open {
        return;
    }
    if runtime.client_id.is_none() {
        return;
    }

    let mut direction = Vec3Net::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction.z -= 1.0;
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
        jump: keys.just_pressed(KeyCode::Space),
        yaw: look.yaw,
        pitch: look.pitch,
    };

    if let Some(predicted) = runtime.predicted_local.as_mut() {
        predicted.apply_input(input);
        predicted.simulate(time.delta_secs());
    }

    if let Some(session) = runtime.session.as_mut() {
        let _ = session.send(ClientMessage::Input(input));
    }
}

pub(crate) fn network_tick_system(
    time: Res<Time>,
    mut runtime: ResMut<ClientRuntime>,
    menu: Res<MenuState>,
) {
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
            runtime.push_error_message(format!("network error: {error}"));
            Vec::new()
        }
        None => Vec::new(),
    };

    for message in messages {
        runtime.apply_message(message);
    }
}

pub(crate) fn apply_snapshot_system(
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
        let rotation = Quat::from_rotation_y(player.yaw);
        if let Some(entity) = existing.get(&player.client_id) {
            commands
                .entity(*entity)
                .insert((TargetPosition(target), TargetRotation(rotation)));
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
                TargetRotation(rotation),
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(player_visual_position(target)).with_rotation(rotation),
                if Some(player.client_id) == runtime.client_id {
                    Visibility::Hidden
                } else {
                    Visibility::Visible
                },
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

pub(crate) fn interpolate_players_system(
    time: Res<Time>,
    mut players: Query<(&mut Transform, &TargetPosition, &TargetRotation), With<NetworkPlayer>>,
) {
    let alpha = 1.0 - (-18.0 * time.delta_secs()).exp();
    for (mut transform, target, target_rotation) in &mut players {
        transform.translation = transform
            .translation
            .lerp(player_visual_position(target.0), alpha);
        transform.rotation = transform.rotation.slerp(target_rotation.0, alpha);
    }
}

pub(crate) fn camera_follow_system(
    time: Res<Time>,
    runtime: Res<ClientRuntime>,
    look: Res<LookState>,
    menu: Res<MenuState>,
    mut camera: Query<&mut Transform, (With<MainCamera>, Without<NetworkPlayer>)>,
) {
    if menu.screen != Screen::InGame {
        return;
    }

    let Ok(mut camera_transform) = camera.single_mut() else {
        return;
    };
    let Some(player) = runtime.local_view() else {
        return;
    };

    let feet = Vec3::new(player.position.x, player.position.y, player.position.z);
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    let alpha = 1.0 - (-30.0 * time.delta_secs()).exp();
    camera_transform.translation = camera_transform.translation.lerp(eye, alpha);
    camera_transform.rotation = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0);
}
