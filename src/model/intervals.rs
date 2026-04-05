use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Add, AddAssign, Div, Mul, Neg, Shl, Sub, SubAssign},
};

use malachite::{
    Integer, Natural,
    base::{
        num::{
            basic::traits::{Infinity, NegativeInfinity, One, Zero},
            conversion::traits::RoundingFrom,
        },
        rounding_modes::RoundingMode,
    },
};
use malachite_float::Float;

/// An interval of real numbers, as in interval arithmetic.
#[derive(Clone)]
pub struct Interval {
    pub lower: Float,
    pub upper: Float,
}

impl Interval {
    pub const ZERO: Self = Self::new_unchecked(Float::ZERO, Float::ZERO);
    pub const ONE: Self = Self::new_unchecked(Float::ONE, Float::ONE);
    pub const EVERYWHERE: Self = Self::new_unchecked(Float::NEGATIVE_INFINITY, Float::INFINITY);

    pub const fn new_unchecked(lower: Float, upper: Float) -> Self {
        Self { lower, upper }
    }

    pub fn new(lhs: Float, rhs: Float) -> Self {
        if lhs <= rhs {
            Self {
                lower: lhs,
                upper: rhs,
            }
        } else {
            Self {
                lower: rhs,
                upper: lhs,
            }
        }
    }

    pub fn union(self, rhs: Interval) -> Interval {
        let new_lower = if self.lower <= rhs.lower {
            self.lower
        } else {
            rhs.lower
        };

        let new_upper = if self.upper >= rhs.upper {
            self.upper
        } else {
            rhs.upper
        };

        Self::new_unchecked(new_lower, new_upper)
    }

    pub fn reciprocal(self) -> Self {
        if !self.contains(Float::ZERO) {
            let new_lower = self.upper.reciprocal_round(RoundingMode::Floor).0;
            let new_upper = self.lower.reciprocal_round(RoundingMode::Ceiling).0;
            Self::new_unchecked(new_lower, new_upper)
        } else {
            Self::EVERYWHERE
        }
    }

    pub fn contains(&self, val: Float) -> bool {
        self.lower <= val && val <= self.upper
    }

    pub fn try_floor(&self) -> Option<Integer> {
        let floored_lower = Integer::rounding_from(&self.lower, RoundingMode::Floor).0;
        let floored_upper = Integer::rounding_from(&self.upper, RoundingMode::Floor).0;
        if floored_lower == floored_upper {
            Some(floored_lower)
        } else {
            None
        }
    }

    pub fn try_ceil(&self) -> Option<Integer> {
        let ceiled_lower = Integer::rounding_from(&self.lower, RoundingMode::Ceiling).0;
        let ceiled_upper = Integer::rounding_from(&self.upper, RoundingMode::Ceiling).0;
        if ceiled_lower == ceiled_upper {
            Some(ceiled_lower)
        } else {
            None
        }
    }
}

impl Display for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.lower, self.upper)
    }
}

impl From<Float> for Interval {
    fn from(value: Float) -> Self {
        Self {
            lower: value.clone(),
            upper: value,
        }
    }
}

impl From<(Float, Ordering)> for Interval {
    fn from((float, ordering): (Float, Ordering)) -> Self {
        match ordering {
            Ordering::Equal => float.into(),
            Ordering::Less => {
                let mut lower = float.clone();
                lower.decrement();
                Self::new(lower, float)
            }
            Ordering::Greater => {
                let mut upper = float.clone();
                upper.increment();
                Self::new(float, upper)
            }
        }
    }
}

impl Neg for Interval {
    type Output = Interval;

    fn neg(self) -> Self::Output {
        Self::new_unchecked(-self.upper, -self.lower)
    }
}

impl Neg for &Interval {
    type Output = Interval;

    fn neg(self) -> Self::Output {
        Interval::new_unchecked(-&self.upper, -&self.lower)
    }
}

impl Add for Interval {
    type Output = Interval;

    fn add(self, rhs: Self) -> Self::Output {
        let new_lower = self.lower.add_round(rhs.lower, RoundingMode::Floor).0;
        let new_upper = self.upper.add_round(rhs.upper, RoundingMode::Ceiling).0;
        Self::new_unchecked(new_lower, new_upper)
    }
}

impl Add<Interval> for &Interval {
    type Output = Interval;

    fn add(self, rhs: Interval) -> Self::Output {
        let new_lower = self
            .lower
            .add_round_ref_val(rhs.lower, RoundingMode::Floor)
            .0;
        let new_upper = self
            .upper
            .add_round_ref_val(rhs.upper, RoundingMode::Ceiling)
            .0;
        Interval::new_unchecked(new_lower, new_upper)
    }
}

impl AddAssign for Interval {
    fn add_assign(&mut self, rhs: Self) {
        self.lower.add_round_assign(rhs.lower, RoundingMode::Floor);
        self.upper
            .add_round_assign(rhs.upper, RoundingMode::Ceiling);
    }
}

impl AddAssign<&Interval> for Interval {
    fn add_assign(&mut self, rhs: &Interval) {
        self.lower
            .add_round_assign_ref(&rhs.lower, RoundingMode::Floor);
        self.upper
            .add_round_assign_ref(&rhs.upper, RoundingMode::Ceiling);
    }
}

impl SubAssign for Interval {
    fn sub_assign(&mut self, rhs: Self) {
        self.lower.sub_round_assign(rhs.upper, RoundingMode::Floor);
        self.upper
            .sub_round_assign(rhs.lower, RoundingMode::Ceiling);
    }
}

impl Sub for Interval {
    type Output = Interval;

    fn sub(self, rhs: Self) -> Self::Output {
        self + -rhs
    }
}

impl Sub<&Interval> for Interval {
    type Output = Interval;

    fn sub(self, rhs: &Interval) -> Self::Output {
        self + -rhs
    }
}

impl Sub<Interval> for &Interval {
    type Output = Interval;

    fn sub(self, rhs: Interval) -> Self::Output {
        self + -rhs
    }
}

impl Shl<u64> for Interval {
    type Output = Interval;

    fn shl(self, rhs: u64) -> Self::Output {
        Interval::new_unchecked(self.lower << rhs, self.upper << rhs)
    }
}

impl Mul<Interval> for Float {
    type Output = Interval;

    fn mul(self, rhs: Interval) -> Self::Output {
        let multiplied_lower: Interval = self
            .mul_round_ref_val(rhs.lower, RoundingMode::Nearest)
            .into();

        let multiplied_upper: Interval = self
            .mul_round_ref_val(rhs.upper, RoundingMode::Nearest)
            .into();

        multiplied_lower.union(multiplied_upper)
    }
}

impl Mul<&Interval> for Float {
    type Output = Interval;

    fn mul(self, rhs: &Interval) -> Self::Output {
        let multiplied_lower: Interval = self
            .mul_round_ref_ref(&rhs.lower, RoundingMode::Nearest)
            .into();

        let multiplied_upper: Interval = self
            .mul_round_ref_ref(&rhs.upper, RoundingMode::Nearest)
            .into();

        multiplied_lower.union(multiplied_upper)
    }
}

impl Mul for Interval {
    type Output = Interval;

    fn mul(self, rhs: Interval) -> Self::Output {
        let multiplied_lower: Interval = self.lower * &rhs;
        let multiplied_upper = self.upper * &rhs;

        multiplied_lower.union(multiplied_upper)
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
impl Div for Interval {
    type Output = Interval;

    fn div(self, rhs: Self) -> Self::Output {
        self * rhs.reciprocal()
    }
}

impl Div<Interval> for Natural {
    type Output = Interval;

    fn div(self, rhs: Interval) -> Self::Output {
        if !rhs.contains(Float::ZERO) {
            // in practice the naturals passed to this function will never be large enough for the conversion to panic
            let numerator: Float = self.try_into().unwrap();
            let new_lower = numerator
                .div_round_ref_val(rhs.upper, RoundingMode::Floor)
                .0;
            let new_upper = numerator.div_round(rhs.lower, RoundingMode::Ceiling).0;
            Interval::new_unchecked(new_lower, new_upper)
        } else {
            Interval::EVERYWHERE
        }
    }
}

impl Div<&Natural> for Interval {
    type Output = Interval;

    fn div(self, rhs: &Natural) -> Self::Output {
        let denomenator: Float = rhs.try_into().unwrap();
        let new_lower = self
            .lower
            .div_round_val_ref(&denomenator, RoundingMode::Floor)
            .0;
        let new_upper = self.upper.div_round(denomenator, RoundingMode::Ceiling).0;
        Interval::new_unchecked(new_lower, new_upper)
    }
}
