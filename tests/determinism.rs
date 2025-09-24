use server::room::{InputEvent, Room, RoomConfig};
use server::protocol::PlayerAction;

#[test]
fn deterministic_simulation() {
    let mut room_a = Room::new("r", 12345, RoomConfig::default());
    let mut room_b = Room::new("r", 12345, RoomConfig::default());
    room_a.register_player("p1");
    room_b.register_player("p1");

    for tick in 1..=1000 {
        let action = if tick % 10 == 0 {
            PlayerAction::Thrust
        } else {
            PlayerAction::Idle
        };
        let input = InputEvent {
            tick,
            seq: tick,
            action,
        };
        room_a.push_input("p1", input.clone()).unwrap();
        room_b.push_input("p1", input).unwrap();
        room_a.step();
        room_b.step();
        let snap_a = room_a.snapshot(false);
        let snap_b = room_b.snapshot(false);
        assert_eq!(snap_a.players[0].position_y.0, snap_b.players[0].position_y.0);
    }
}
