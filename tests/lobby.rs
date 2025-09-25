use server::lobby::{Lobby, Player, Role};
use server::sim::SimHandle;

fn test_sim() -> SimHandle {
    SimHandle::spawn(format!("room-{}", ulid::Ulid::new()))
}

#[tokio::test]
async fn character_selection_updates_lobby_state() {
    let lobby = Lobby::new();
    let room_id = "room-select".to_string();
    let sim = test_sim();
    let master = Player::new("p1".into(), "host".into(), Role::Master);
    lobby
        .create_room(
            room_id.clone(),
            "seed".into(),
            "region".into(),
            4,
            master,
            sim.clone(),
        )
        .await;

    lobby
        .join_room(
            &room_id,
            Player::new("p2".into(), "guest".into(), Role::Member),
        )
        .await
        .unwrap();

    let state = lobby
        .set_character(&room_id, "p2", Some("warrior".into()))
        .await
        .expect("character should update");
    let guest = state
        .players
        .into_iter()
        .find(|p| p.id == "p2")
        .expect("guest present");
    assert_eq!(guest.character_id.as_deref(), Some("warrior"));
}

#[tokio::test]
async fn master_transfer_promotes_earliest_member() {
    let lobby = Lobby::new();
    let room_id = "room-a".to_string();
    let sim = test_sim();
    let master = Player::new("p1".into(), "host".into(), Role::Master);
    lobby
        .create_room(
            room_id.clone(),
            "seed".into(),
            "region".into(),
            4,
            master,
            sim.clone(),
        )
        .await;

    lobby
        .join_room(&room_id, Player::new("p2".into(), "a".into(), Role::Member))
        .await
        .unwrap();
    lobby
        .join_room(&room_id, Player::new("p3".into(), "b".into(), Role::Member))
        .await
        .unwrap();

    lobby.leave_room(&room_id, "p1").await;

    let room = lobby.room(&room_id).await.unwrap();
    let room = room.read().await;
    assert_eq!(room.master_id().as_deref(), Some("p2"));
}

#[tokio::test]
async fn start_requires_ready_when_enabled() {
    let lobby = Lobby::new();
    let room_id = "room-b".to_string();
    let sim = test_sim();
    let master = Player::new("p1".into(), "host".into(), Role::Master);
    lobby
        .create_room(
            room_id.clone(),
            "seed".into(),
            "region".into(),
            4,
            master,
            sim.clone(),
        )
        .await;
    lobby
        .join_room(&room_id, Player::new("p2".into(), "a".into(), Role::Member))
        .await
        .unwrap();

    let err = lobby
        .start_room(&room_id, "p1", 3, true)
        .await
        .expect_err("should require ready");
    if let server::errors::AppError::Http { code, .. } = err {
        assert_eq!(
            code.as_str(),
            server::errors::ErrorCode::RoomNotReady.as_str()
        );
    } else {
        panic!("unexpected error");
    }
}
