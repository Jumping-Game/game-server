use super::{fixed::Fixed, rng::SplitMix64};

#[derive(Debug, Clone)]
pub struct WorldGenerator {
    seed: u64,
}

impl WorldGenerator {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub fn platforms_for_tick(&self, tick: u64) -> Vec<(u64, Fixed)> {
        let mut rng = SplitMix64::new(self.seed ^ tick);
        (0..5)
            .map(|i| {
                let base = (i as f64) * 6.0 + (tick as f64 * 0.1);
                let offset = (rng.next_f64() - 0.5) * 2.0;
                let height = base + offset;
                (i as u64, Fixed::from_f64(height))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_platforms() {
        let gen = WorldGenerator::new(42);
        let first = gen.platforms_for_tick(100);
        let second = gen.platforms_for_tick(100);
        assert_eq!(first, second);
    }
}
