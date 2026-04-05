use malachite::base::num::arithmetic::traits::Parity;
use malachite::base::num::arithmetic::traits::UnsignedAbs;
use malachite::base::num::conversion::traits::SaturatingFrom;
mod intervals;

use malachite::{
    Integer, Natural,
    base::{
        num::{
            arithmetic::traits::{ModPow, ModPowerOf2, Reciprocal},
            basic::traits::{NegativeOne, One},
            conversion::traits::{RoundingFrom, WrappingFrom},
        },
        rounding_modes::RoundingMode::Floor,
    },
};
use malachite_float::Float;
use std::fmt::{self, Debug, Display};

use crate::model::intervals::Interval;

const N0: Natural = Natural::const_from(0);
const N1: Natural = Natural::const_from(1);
const N2: Natural = Natural::const_from(2);
const N3: Natural = Natural::const_from(3);
const N4: Natural = Natural::const_from(4);
const N5: Natural = Natural::const_from(5);
//const N6: Natural = Natural::const_from(6);
const N7: Natural = Natural::const_from(7);
//const N8: Natural = Natural::const_from(8);
const N9: Natural = Natural::const_from(9);
const N10: Natural = Natural::const_from(10);

//const Z0: Integer = Integer::const_from_unsigned(0);
//const Z1: Integer = Integer::const_from_unsigned(1);
const Z2: Integer = Integer::const_from_unsigned(2);
//const Z3: Integer = Integer::const_from_unsigned(3);
const Z4: Integer = Integer::const_from_unsigned(4);
const Z5: Integer = Integer::const_from_unsigned(5);
const Z6: Integer = Integer::const_from_unsigned(6);
//const Z7: Integer = Integer::const_from_unsigned(7);
const Z8: Integer = Integer::const_from_unsigned(8);
const Z9: Integer = Integer::const_from_unsigned(9);
const Z10: Integer = Integer::const_from_unsigned(10);

const F0: Float = Float::const_from_unsigned(0);
const F1: Float = Float::const_from_unsigned(1);
const F16: Float = Float::const_from_unsigned(16);

const I0: Interval = Interval::ZERO;
const I1: Interval = Interval::ONE;

#[derive(Eq, Ord, PartialEq, PartialOrd)]
pub struct ByteInfo {
    pub pos: Natural,
    pub byte: u8,
    pub byte_time: usize,
    pub total_time: usize,
}

impl ByteInfo {
    pub fn new(pos: Natural, byte: u8, byte_time: usize, total_time: usize) -> Self {
        Self {
            pos,
            byte,
            byte_time,
            total_time,
        }
    }
}

impl Display for ByteInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, {:02X}, {:.3}, {:.3},",
            self.pos,
            self.byte,
            self.byte_time as f64 / 1000.0,
            self.total_time as f64 / 1000.0,
        )
    }
}

impl Debug for ByteInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "byte {} = {:02X} (took {:.3}s, {:.3}s total)",
            self.pos,
            self.byte,
            self.byte_time as f64 / 1000.0,
            self.total_time as f64 / 1000.0,
        )
    }
}

/// Calculates the nth byte of the binary expansion of tau, where byte zero is the integer part (six).
pub fn calculate_byte(pos: &Natural, fp: u64) -> u8 {
    if pos == &0 {
        return 6;
    }

    let lower_nibble_index = pos << 1;
    let upper_nibble_index = &lower_nibble_index - N1;

    16 * calc_nibble(upper_nibble_index, fp) + calc_nibble(lower_nibble_index, fp)
}

/// Calculates the nth byte of the binary expansion of tau, where byte zero is the integer part (six). Uses interval arithmetic to guarantee that the result is correct, returning None if there was not enough precision to obtain a correct result.
pub fn calculate_byte_interval(pos: &Natural, fp: u64) -> Option<u8> {
    if pos == &0 {
        return Some(6);
    }

    let lower_nibble_index = pos << 1;
    let upper_nibble_index = &lower_nibble_index - N1;

    Some(
        16 * calc_nibble_interval(upper_nibble_index, fp)?
            + calc_nibble_interval(lower_nibble_index, fp)?,
    )
}

fn calc_nibble(p: Natural, fp: u64) -> u8 {
    let mut r = F0;
    let mut i = N0;
    let mut four_i = N0;
    let mut ten_i = N0;
    let mut exp = (Integer::from(p) << 2) - Z9; // 4i - 9

    while exp >= -10 {
        let sign = if (&i).even() {
            Float::ONE
        } else {
            Float::NEGATIVE_ONE
        };

        r += sign
            * F16
            * (-sigma(&(&exp + Z5), &four_i + N1, fp) - sigma(&exp, &four_i + N3, fp)
                + sigma(&(&exp + Z8), &ten_i + N1, fp)
                - sigma(&(&exp + Z6), &ten_i + N3, fp)
                - sigma(&(&exp + Z2), &ten_i + N5, fp)
                - sigma(&(&exp + Z2), &ten_i + N7, fp)
                + sigma(&exp, &ten_i + N9, fp));

        i += N1;
        four_i += N4;
        ten_i += N10;
        exp -= Z10;
    }
    u8::wrapping_from(&Integer::rounding_from(&r, Floor).0.mod_power_of_2(4))
}

fn calc_nibble_interval(p: Natural, fp: u64) -> Option<u8> {
    let mut r = I0;
    let mut i = N0;
    let mut exp = (Integer::from(&p) << 2) - Z9; // 4i - 9

    loop {
        let sign = if (&i).even() {
            Float::ONE
        } else {
            Float::NEGATIVE_ONE
        };

        r += sign
            * F16
            * (-sigma_interval(&(&exp + Z5), N4 * &i + N1, fp)
                - sigma_interval(&exp, N4 * &i + N3, fp)
                + sigma_interval(&(&exp + Z8), N10 * &i + N1, fp)
                - sigma_interval(&(&exp + Z6), N10 * &i + N3, fp)
                - sigma_interval(&(&exp + Z2), N10 * &i + N5, fp)
                - sigma_interval(&(&exp + Z2), N10 * &i + N7, fp)
                + sigma_interval(&exp, N10 * &i + N9, fp));

        i += N1;
        exp -= Z10;

        if exp > -18 {
            continue;
        }

        let floor = r.try_floor()?;
        let ceiling = r.try_ceil()?;

        // multiply by 64 > 44.22...
        let (next_odd_exp, next_even_exp) = if i.even() {
            (&exp - Z4, &exp + Z6) // next odd is smaller than next even
        } else {
            (&exp + Z6, &exp - Z4) // next odd is larger than next odd
        };

        let dist_to_floor = &r - Interval::from(Float::from_integer_prec_ref(&floor, fp));
        if dist_to_floor.lower >> i64::saturating_from(&next_odd_exp) <= 1 {
            // eprintln!("\rnibble {} needs another term from below", p.clone());
            continue;
        }

        let dist_to_ceil = Interval::from(Float::from_integer_prec(ceiling, fp)) - &r;
        if dist_to_ceil.lower >> i64::saturating_from(&next_even_exp) <= 1 {
            // eprintln!("\rnibble {} needs another term from above", p.clone());
            continue;
        }

        return Some(u8::wrapping_from(&floor.mod_power_of_2(4)));
    }
}

fn sigma(exp: &Integer, denom: Natural, fp: u64) -> Float {
    if denom == 1 {
        return F1;
    }

    let fdenom: Float = Float::from_natural_prec_ref(&denom, fp).0;
    let pow: Natural = exp.unsigned_abs();

    if exp > &0 {
        // (2^exp / denom) mod 1
        // note: denom is in practice never going to be large enough for the conversion to fail
        Float::try_from(N2.mod_pow(pow, denom)).unwrap() / fdenom
    } else {
        // (2^exp / denom) mod 1 = 1/(denom * 2^(-pow))
        // note: exp is never negative enough for saturating from to give an incorrect result
        (fdenom << u64::saturating_from(&pow)).reciprocal()
    }
}

fn sigma_interval(exp: &Integer, denom: Natural, fp: u64) -> Interval {
    if denom == 1 {
        return I1;
    }
    let denom_int: Interval = Float::from_natural_prec_ref(&denom, fp).into();
    let pow: Natural = exp.unsigned_abs();

    if exp > &0 {
        // (2^exp / denom) mod 1
        N2.mod_pow(pow, denom) / denom_int
    } else {
        // (2^exp / denom) mod 1 = 1/(denom * 2^(-pow))
        // note: exp is never negative enough for saturating from to give an incorrect result
        (denom_int << u64::saturating_from(&pow)).reciprocal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_256() {
        for (i, &expected) in TAU.iter().enumerate().take(256) {
            let precision = (2 * (i + 1).ilog2() + 8).into();
            assert_eq!(calculate_byte(&i.into(), precision), expected);
        }
    }

    #[test]
    fn first_4096() {
        for (i, &expected) in TAU.iter().enumerate().take(4096) {
            let precision = (2 * (i + 1).ilog2() + 8).into();
            assert_eq!(calculate_byte(&i.into(), precision), expected);
        }
    }

    #[test]
    fn first_256_interval() {
        for (i, &expected) in TAU.iter().enumerate().take(256) {
            let precision = 38;
            assert_eq!(
                calculate_byte_interval(&i.into(), precision).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn first_4096_interval() {
        for (i, &expected) in TAU.iter().enumerate().take(4096) {
            let precision = 38;
            assert_eq!(
                calculate_byte_interval(&i.into(), precision).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn last_131072() {
        for (i, &expected) in TAU.iter().enumerate().skip(131_056) {
            let precision = (2 * (i + 1).ilog2() + 8).into();
            assert_eq!(calculate_byte(&i.into(), precision), expected);
        }
    }

    #[test]
    fn last_131072_interval() {
        for (i, &expected) in TAU.iter().enumerate().skip(131_056) {
            let precision = 50;
            assert_eq!(
                calculate_byte_interval(&i.into(), precision).unwrap(),
                expected
            );
        }
    }

    const TAU: &[u8; 131073] = include_bytes!("tau-test");
}
