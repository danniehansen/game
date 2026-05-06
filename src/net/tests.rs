use super::*;
use crate::{
    protocol::{ClientMessage, ServerMessage},
    save::WorldSave,
    steam::{AuthenticatedUser, offline_auth_token},
};

#[test]
fn local_session_receives_welcome_and_snapshots() {
    let user = AuthenticatedUser {
        steam_id: 1,
        display_name: "Host".to_owned(),
        token: offline_auth_token(1),
    };
    let mut session = LocalGameSession::start(WorldSave::new("Local", Some(user.steam_id)), &user)
        .expect("local session should start");

    let initial = session.drain();
    assert!(matches!(initial[0], ServerMessage::Welcome { .. }));

    session.tick(0.05);
    let tick = session.drain();
    assert!(
        tick.iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    );
}

#[test]
fn local_session_accumulates_fixed_authoritative_ticks() {
    let user = AuthenticatedUser {
        steam_id: 1,
        display_name: "Host".to_owned(),
        token: offline_auth_token(1),
    };
    let mut session = LocalGameSession::start(WorldSave::new("Local", Some(user.steam_id)), &user)
        .expect("local session should start");
    session.drain();

    session.tick(0.025);
    assert!(session.drain().is_empty());

    session.tick(0.025);
    assert!(
        session
            .drain()
            .iter()
            .any(|message| matches!(message, ServerMessage::Snapshot(_)))
    );
}

#[test]
fn local_session_chat_round_trips_through_server() {
    let user = AuthenticatedUser {
        steam_id: 1,
        display_name: "Host".to_owned(),
        token: offline_auth_token(1),
    };
    let mut session = LocalGameSession::start(WorldSave::new("Local", Some(user.steam_id)), &user)
        .expect("local session should start");
    session.drain();

    session.send(ClientMessage::Chat {
        text: "  hello  ".to_owned(),
    });

    let messages = session.drain();
    assert!(messages.iter().any(|message| {
        matches!(
            message,
            ServerMessage::Chat(chat) if chat.from == "Host" && chat.text == "hello"
        )
    }));
}
