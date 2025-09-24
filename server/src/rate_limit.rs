use std::time::Instant;

#[derive(Debug)]
pub struct LeakyBucket {
    capacity: f64,
    refill_per_sec: f64,
    tokens: f64,
    last: Instant,
}

impl LeakyBucket {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            refill_per_sec,
            tokens: capacity,
            last: Instant::now(),
        }
    }

    pub fn allow(&mut self, cost: f64) -> bool {
        self.refill();
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        self.last = now;
        let refill = self.refill_per_sec * elapsed.as_secs_f64();
        self.tokens = (self.tokens + refill).min(self.capacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn allows_after_refill() {
        let mut bucket = LeakyBucket::new(1.0, 1.0);
        assert!(bucket.allow(1.0));
        assert!(!bucket.allow(1.0));
        sleep(Duration::from_secs(1));
        assert!(bucket.allow(1.0));
    }
}
