use std::{thread, time::Duration};

use super::*;
use crate::{
    protocol::{ClientMessage, ServerMessage},
    save::{WorldSave, WorldStore},
    server::ServerSettings,
    steam::{AuthMode, AuthenticatedUser, offline_auth_token},
};

fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        steam_id: 1,
        display_name: "Host".to_owned(),
        token: offline_auth_token(1),
    }
}

fn temp_store() -> WorldStore {
    WorldStore::new(
        std::env::temp_dir().join(format!("game-network-test-{}", uuid::Uuid::new_v4())),
    )
}

fn start_session() -> ClientSession {
    let user = user();
    ClientSession::start_singleplayer(WorldSave::new("Local", Some(user.steam_id)), &user)
        .expect("network session should start")
}

fn collect_until(
    session: &mut ClientSession,
    accepts: impl Fn(&[ServerMessage]) -> bool,
) -> Vec<ServerMessage> {
    let mut messages = Vec::new();
    for _ in 0..100 {
        messages.extend(session.tick(0.0).expect("session should tick"));
        if accepts(&messages) {
            return messages;
        }
        thread::sleep(Duration::from_millis(10));
    }
    messages
}

#[test]
fn singleplayer_session_connects_through_loopback_server() {
    let mut session = start_session();

    let initial = session.tick(0.0).expect("session should tick");
    assert!(matches!(initial[0], ServerMessage::Welcome { .. }));

    let tick = collect_until(&mut session, |messages| {
        messages
            .iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    });
    assert!(
        tick.iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    );
}

#[test]
fn singleplayer_session_receives_authoritative_snapshots_from_loopback_host() {
    let mut session = start_session();
    let _ = session.tick(0.0);

    let messages = collect_until(&mut session, |messages| {
        messages
            .iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    });

    assert!(
        messages
            .iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    );
}

#[test]
fn singleplayer_chat_round_trips_through_network_server() {
    let mut session = start_session();
    let _ = session.tick(0.0);

    session
        .send(ClientMessage::Chat {
            text: "  hello  ".to_owned(),
        })
        .expect("chat should send");

    let messages = collect_until(&mut session, |messages| {
        messages.iter().any(|message| {
            matches!(
                message,
                ServerMessage::Chat(chat) if chat.from == "Host" && chat.text == "hello"
            )
        })
    });
    assert!(messages.iter().any(|message| {
        matches!(
            message,
            ServerMessage::Chat(chat) if chat.from == "Host" && chat.text == "hello"
        )
    }));
}

#[test]
fn direct_multiplayer_connects_to_shared_lightyear_server_host() {
    let user = user();
    let mut spawned = super::host::spawn_loopback_server(
        WorldSave::new("Remote", Some(user.steam_id)),
        ServerSettings {
            auth_mode: AuthMode::Offline,
            singleplayer_host: None,
        },
    )
    .expect("Lightyear server should start");

    let mut session =
        ClientSession::connect(spawned.addr, &user).expect("direct network session should connect");
    let initial = session.tick(0.0).expect("session should tick");
    assert!(matches!(initial[0], ServerMessage::Welcome { .. }));

    session
        .send(ClientMessage::Chat {
            text: "  remote path  ".to_owned(),
        })
        .expect("chat should send");

    let messages = collect_until(&mut session, |messages| {
        messages.iter().any(|message| {
            matches!(
                message,
                ServerMessage::Chat(chat)
                    if chat.from == "Host" && chat.text == "remote path"
            )
        })
    });
    assert!(messages.iter().any(|message| {
        matches!(
            message,
            ServerMessage::Chat(chat) if chat.from == "Host" && chat.text == "remote path"
        )
    }));

    session
        .send(ClientMessage::Disconnect)
        .expect("disconnect should send");
    spawned.handle.shutdown().expect("server should stop");
}

#[test]
fn singleplayer_shutdown_persists_world_from_network_server() {
    let store = temp_store();
    let user = user();
    let save = store
        .create_world("Persisted", Some(user.steam_id))
        .expect("world should create");
    let world_id = save.id;
    let mut session =
        ClientSession::start_singleplayer(save, &user).expect("network session should start");
    let _ = session.tick(0.0);

    session
        .shutdown(&store)
        .expect("session should persist and shut down");

    let loaded = store.load_world(world_id).expect("world should load");
    assert_eq!(loaded.name, "Persisted");
    let _ = std::fs::remove_dir_all(store.root());
}
