use server::room::{InputEvent, Room, RoomConfig};

#[test]
fn deterministic_simulation() {
    let mut room_a = Room::new("r", 12345, RoomConfig::default());
    let mut room_b = Room::new("r", 12345, RoomConfig::default());
    room_a.register_player("p1");
    room_b.register_player("p1");

    for tick in 1..=1000 {
        let input = InputEvent {
            tick,
            seq: tick,
            axis_x: if tick % 20 < 10 { -0.5 } else { 0.5 },
            jump: tick % 15 == 0,
        };
        room_a.push_input("p1", input.clone()).unwrap();
        room_b.push_input("p1", input).unwrap();
        room_a.step();
        room_b.step();
        let snap_a = room_a.snapshot_for_player("p1", true);
        let snap_b = room_b.snapshot_for_player("p1", true);
        assert!((snap_a.players[0].y - snap_b.players[0].y).abs() < f32::EPSILON);
    }
}
