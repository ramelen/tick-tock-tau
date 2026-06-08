use malachite::base::num::arithmetic::traits::{NegAssign, ShlRoundAssign, ShrRoundAssign};
use malachite::base::num::basic::traits::{Infinity, NegativeInfinity, Zero};
use malachite::base::num::conversion::string::options::ToSciOptions;
use malachite::base::num::conversion::traits::{RoundingFrom, ToSci};
use malachite::base::rounding_modes::RoundingMode::{self, Ceiling, Floor};
use malachite::{Integer, Natural, Rational};
use malachite_float::Float;
use std::cmp::Ordering;
use std::fmt::{Debug, Display};
use std::ops::{Add, AddAssign, Mul, Neg, Shl, ShlAssign, Shr, ShrAssign, Sub, SubAssign};

use crate::model::NICE_CAST;

/// An interval of real numbers, as in interval arithmetic.
#[derive(Clone)]
pub(crate) struct Interval {
    pub inf: Float,
    pub sup: Float,
}

impl Interval {
    pub(crate) const ZERO: Self = Self::new_unchecked(Float::ZERO, Float::ZERO);
    pub(crate) const ENTIRE: Self = Self::new_unchecked(Float::NEGATIVE_INFINITY, Float::INFINITY);

    pub(crate) const fn new_unchecked(inf: Float, sup: Float) -> Self {
        Self { inf, sup }
    }

    pub(crate) fn reciprocal(mut self) -> Self {
        if !self.contains(Float::ZERO) {
            self.sup.reciprocal_round_assign(Floor);
            self.inf.reciprocal_round_assign(Ceiling);
            std::mem::swap(&mut self.inf, &mut self.sup);
            self
        } else {
            Self::ENTIRE
        }
    }

    pub(crate) fn contains(&self, val: Float) -> bool {
        self.inf <= val && val <= self.sup
    }

    pub(crate) fn width(&self) -> Float {
        self.sup.sub_round_ref_ref(&self.inf, Ceiling).0
    }

    pub(crate) fn try_floor(&self) -> Option<Integer> {
        let (floored_lower, _) = Integer::rounding_from(&self.inf, Floor);
        let (floored_upper, _) = Integer::rounding_from(&self.sup, Floor);
        if floored_lower == floored_upper {
            Some(floored_lower)
        } else {
            None
        }
    }

    // returns the unique integer contained in the interval, or the number of integers contained in the interval otherwise.
    pub(crate) fn inner_int(&self) -> Result<Integer, Integer> {
        let (ceiled_lower, _) = Integer::rounding_from(&self.inf, RoundingMode::Ceiling);
        let (floored_upper, _) = Integer::rounding_from(&self.sup, RoundingMode::Floor);
        if ceiled_lower == floored_upper {
            Ok(ceiled_lower)
        } else {
            Err(floored_upper - ceiled_lower + Integer::const_from_unsigned(1))
        }
    }

    pub(crate) fn maybe(self) -> Interval {
        if self.inf > 0 {
            Self::new_unchecked(Float::ZERO, self.sup)
        } else if self.sup < 0 {
            Self::new_unchecked(self.inf, Float::ZERO)
        } else {
            self
        }
    }
}

impl Display for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.inf, self.sup)
    }
}

impl Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}, {}]",
            fmt_debug_round(&self.inf, Floor, f),
            fmt_debug_round(&self.sup, Ceiling, f)
        )
    }
}

// format in hex for now
fn fmt_debug_round(x: &Float, rm: RoundingMode, f: &std::fmt::Formatter<'_>) -> String {
    let mut string = String::new();
    if x.is_nan() {
        return String::from("NaN");
    };

    if f.sign_plus() && x.is_sign_positive() {
        string += "+";
    } else if x.is_sign_negative() {
        string += "-";
    }

    if x.is_infinite() {
        return string + "inf";
    }

    if f.alternate() {
        string += "0x";
    }

    if x == &0 {
        return string + "0";
    };

    // the number of significant figures in the significand should be equal to prec
    let prec = x
        .get_prec()
        .expect("already handled floats with no precision");

    let mut sci_options = ToSciOptions::default();
    sci_options.set_base(16);
    sci_options.set_rounding_mode(rm);
    sci_options.set_size_complete();
    sci_options.set_include_trailing_zeros(true);

    let rational = Rational::try_from(x).expect("infinities already handled");
    let formatted = rational.to_sci_with_options(sci_options).to_string();
    string + formatted.trim_start_matches('-') + "#" + prec.to_string().as_str()
}

impl From<Float> for Interval {
    fn from(value: Float) -> Self {
        Self {
            inf: value.clone(),
            sup: value,
        }
    }
}

impl From<Natural> for Interval {
    fn from(value: Natural) -> Self {
        Self::from(Float::try_from(value).expect(NICE_CAST))
    }
}

impl From<Integer> for Interval {
    fn from(value: Integer) -> Self {
        Self::from(Float::try_from(value).expect(NICE_CAST))
    }
}

impl From<(Float, Ordering)> for Interval {
    fn from((float, ordering): (Float, Ordering)) -> Self {
        match ordering {
            Ordering::Equal => float.into(),
            // the given value is less than the real value, so increment the given value to get the bounds
            Ordering::Less => {
                let mut upper = float.clone();
                upper.increment();
                Self::new_unchecked(float, upper)
            }
            // the given value is greater than the real value, so decrement the given value to get the bounds
            Ordering::Greater => {
                let mut lower = float.clone();
                lower.decrement();
                Self::new_unchecked(lower, float)
            }
        }
    }
}

impl NegAssign for Interval {
    fn neg_assign(&mut self) {
        self.inf.neg_assign();
        self.sup.neg_assign();
        std::mem::swap(&mut self.inf, &mut self.sup);
    }
}

impl Neg for Interval {
    type Output = Interval;

    fn neg(mut self) -> Self::Output {
        self.neg_assign();
        self
    }
}

impl Neg for &Interval {
    type Output = Interval;

    fn neg(self) -> Self::Output {
        Interval::new_unchecked(-&self.sup, -&self.inf)
    }
}

impl<T> ShlAssign<T> for Interval
where
    Float: ShlRoundAssign<T>,
    T: Copy,
{
    fn shl_assign(&mut self, rhs: T) {
        self.inf.shl_round_assign(rhs, Floor);
        self.sup.shl_round_assign(rhs, Ceiling);
    }
}

impl<T> ShrAssign<T> for Interval
where
    Float: ShrRoundAssign<T>,
    T: Copy,
{
    fn shr_assign(&mut self, rhs: T) {
        self.inf.shr_round_assign(rhs, Floor);
        self.sup.shr_round_assign(rhs, Ceiling);
    }
}

impl<T> Shl<T> for Interval
where
    Interval: ShlAssign<T>,
{
    type Output = Interval;

    fn shl(mut self, rhs: T) -> Self::Output {
        self <<= rhs;
        self
    }
}

impl<T> Shr<T> for Interval
where
    Interval: ShrAssign<T>,
{
    type Output = Interval;

    fn shr(mut self, rhs: T) -> Self::Output {
        self >>= rhs;
        self
    }
}

impl Mul<Natural> for Interval {
    type Output = Interval;

    fn mul(mut self, rhs: Natural) -> Self::Output {
        let float = Float::try_from(rhs).expect(NICE_CAST);
        self.inf.mul_round_assign_ref(&float, Floor);
        self.sup.mul_round_assign_ref(&float, Ceiling);
        self
    }
}

impl AddAssign<Interval> for Interval {
    fn add_assign(&mut self, rhs: Interval) {
        self.inf.add_round_assign(rhs.inf, Floor);
        self.sup.add_round_assign(rhs.sup, Ceiling);
    }
}

impl AddAssign<&Interval> for Interval {
    fn add_assign(&mut self, rhs: &Interval) {
        self.inf.add_round_assign_ref(&rhs.inf, Floor);
        self.sup.add_round_assign_ref(&rhs.sup, Ceiling);
    }
}

impl Add<Interval> for Interval {
    type Output = Interval;

    fn add(mut self, rhs: Interval) -> Self::Output {
        self.add_assign(rhs);
        self
    }
}

impl Add<&Interval> for Interval {
    type Output = Interval;

    fn add(mut self, rhs: &Interval) -> Self::Output {
        self += rhs;
        self
    }
}

impl Add<Interval> for &Interval {
    type Output = Interval;

    fn add(self, rhs: Interval) -> Self::Output {
        Interval {
            inf: self.inf.add_round_ref_val(rhs.inf, Floor).0,
            sup: self.sup.add_round_ref_val(rhs.sup, Ceiling).0,
        }
    }
}

impl Add<&Interval> for &Interval {
    type Output = Interval;

    fn add(self, rhs: &Interval) -> Self::Output {
        Interval {
            inf: self.inf.add_round_ref_ref(&rhs.inf, Floor).0,
            sup: self.sup.add_round_ref_ref(&rhs.sup, Ceiling).0,
        }
    }
}

impl SubAssign<Interval> for Interval {
    fn sub_assign(&mut self, rhs: Interval) {
        self.inf.sub_round_assign(rhs.sup, Floor);
        self.sup.sub_round_assign(rhs.inf, Ceiling);
    }
}

impl SubAssign<&Interval> for Interval {
    fn sub_assign(&mut self, rhs: &Interval) {
        self.inf.sub_round_assign_ref(&rhs.sup, Floor);
        self.sup.sub_round_assign_ref(&rhs.inf, Ceiling);
    }
}

impl Sub<Interval> for Interval {
    type Output = Interval;

    fn sub(mut self, rhs: Interval) -> Self::Output {
        self -= rhs;
        self
    }
}

impl Sub<&Interval> for Interval {
    type Output = Interval;

    fn sub(mut self, rhs: &Interval) -> Self::Output {
        self -= rhs;
        self
    }
}

impl Sub<Interval> for &Interval {
    type Output = Interval;

    fn sub(self, rhs: Interval) -> Self::Output {
        Interval {
            inf: self.inf.sub_round_ref_val(rhs.sup, Floor).0,
            sup: self.sup.sub_round_ref_val(rhs.inf, Ceiling).0,
        }
    }
}

impl Sub<&Interval> for &Interval {
    type Output = Interval;

    fn sub(self, rhs: &Interval) -> Self::Output {
        Interval {
            inf: self.inf.sub_round_ref_ref(&rhs.sup, Floor).0,
            sup: self.sup.sub_round_ref_ref(&rhs.inf, Ceiling).0,
        }
    }
}
