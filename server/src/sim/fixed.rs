use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Fixed(i64);

const SCALE: i64 = 1 << 16;

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);

    pub fn from_bits(bits: i64) -> Self {
        Self(bits)
    }

    pub fn to_bits(self) -> i64 {
        self.0
    }

    pub fn from_f64(value: f64) -> Self {
        let scaled = (value * SCALE as f64).round() as i64;
        Self(scaled)
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / SCALE as f64
    }

    pub fn clamp(self, min: Fixed, max: Fixed) -> Fixed {
        Fixed(self.0.clamp(min.0, max.0))
    }

    pub fn mul_f64(self, value: f64) -> Self {
        Self::from_f64(self.to_f64() * value)
    }
}

impl Add for Fixed {
    type Output = Fixed;

    fn add(self, rhs: Fixed) -> Self::Output {
        Fixed(self.0 + rhs.0)
    }
}

impl AddAssign for Fixed {
    fn add_assign(&mut self, rhs: Fixed) {
        self.0 += rhs.0;
    }
}

impl Sub for Fixed {
    type Output = Fixed;

    fn sub(self, rhs: Fixed) -> Self::Output {
        Fixed(self.0 - rhs.0)
    }
}

impl SubAssign for Fixed {
    fn sub_assign(&mut self, rhs: Fixed) {
        self.0 -= rhs.0;
    }
}

impl Mul<Fixed> for Fixed {
    type Output = Fixed;

    fn mul(self, rhs: Fixed) -> Self::Output {
        Fixed(((self.0 as i128 * rhs.0 as i128) / SCALE as i128) as i64)
    }
}

impl From<f64> for Fixed {
    fn from(value: f64) -> Self {
        Self::from_f64(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addition_is_exact() {
        let a = Fixed::from_f64(1.5);
        let b = Fixed::from_f64(2.25);
        assert_eq!((a + b).to_f64(), 3.75);
    }
}
