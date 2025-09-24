use super::{fixed::Fixed, PlayerSimState, SimulationConfig};
use crate::protocol::PlayerAction;

#[derive(Debug, Default)]
pub struct PhysicsWorld;

impl PhysicsWorld {
    pub fn new() -> Self {
        Self
    }

    pub fn apply_action(
        &self,
        player: &mut PlayerSimState,
        action: &PlayerAction,
        config: &SimulationConfig,
    ) {
        match action {
            PlayerAction::Idle => {}
            PlayerAction::Thrust => {
                player.velocity_y += config.thrust;
            }
        }
    }

    pub fn tick_player(&self, player: &mut PlayerSimState, config: &SimulationConfig) {
        player.velocity_y += config.gravity;
        player.position_y += player.velocity_y;
        if player.position_y.to_bits() < 0 {
            player.position_y = Fixed::ZERO;
            player.velocity_y = Fixed::ZERO;
        }
    }
}

pub trait PhysicsState {
    fn position(&self) -> Fixed;
}

impl PhysicsState for PlayerSimState {
    fn position(&self) -> Fixed {
        self.position_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thrust_increases_position() {
        let physics = PhysicsWorld::new();
        let mut player = PlayerSimState {
            player_id: "a".into(),
            position_y: Fixed::ZERO,
            velocity_y: Fixed::ZERO,
            last_input_seq: 0,
        };
        let config = SimulationConfig::default();
        physics.apply_action(&mut player, &PlayerAction::Thrust, &config);
        physics.tick_player(&mut player, &config);
        assert!(player.position_y.to_f64() > 0.0);
    }
}
