pub mod intervals;

use crate::cli::{SENDABLE, Status};
use crate::model::intervals::Interval;
use malachite::base::num::arithmetic::traits::{ModPow, NegAssign, Parity, UnsignedAbs};
use malachite::base::num::conversion::traits::WrappingFrom;
use malachite::base::num::logic::traits::SignificantBits;
use malachite::base::rounding_modes::RoundingMode;
use malachite::{Integer, Natural};
use malachite_float::Float;
use rayon::iter::{IntoParallelIterator, ParallelBridge, ParallelIterator};
use std::num::NonZeroUsize;
use std::sync::mpsc::Sender;

const N0: Natural = Natural::const_from(0);
const N1: Natural = Natural::const_from(1);
const N2: Natural = Natural::const_from(2);
const N3: Natural = Natural::const_from(3);
const N4: Natural = Natural::const_from(4);
const N5: Natural = Natural::const_from(5);
const N7: Natural = Natural::const_from(7);
const N8: Natural = Natural::const_from(8);
const N9: Natural = Natural::const_from(9);
const N10: Natural = Natural::const_from(10);

const Z0: Integer = Integer::const_from_unsigned(0);
const Z2: Integer = Integer::const_from_unsigned(2);
const Z5: Integer = Integer::const_from_unsigned(5);
const Z6: Integer = Integer::const_from_unsigned(6);
const Z8: Integer = Integer::const_from_unsigned(8);
const Z10: Integer = Integer::const_from_unsigned(10);

const I0: Interval = Interval::ZERO;

pub(crate) const NICE_CAST: &str = "value is too small to overflow";

pub fn calculate_byte_range_parallel(
    start: &Natural,
    precision: u64,
    min_batch_size: usize,
    max_batch_size: Option<NonZeroUsize>,
) -> Result<Vec<u8>, Vec<u8>> {
    // the number of bits to calculate at a time, which is 8 in this case because we are calculating bytes.
    const BITS: u64 = 8;
    const Z_BITS: Integer = Integer::const_from_unsigned(BITS);

    // return early if precision isn't large enough to avoid a few edge cases
    if precision < 2 {
        return Err(Vec::new());
    }

    // the number of bits to shift the calculation over by, as we don't need to calculate any of the bytes before `start`. also shift by lb(base) bits in order to put the calculated value from the [0, 2^bits] range to the [0, 1] range.
    let mut offset = Z8 * Integer::from(start) - Z_BITS;

    // the bytes of tau calculated so far, starting at `start`.
    let mut bytes = Vec::new();

    // n: the iteration number, because ranges of `malachite::Natural` can't be iterated over directly.
    // acc: the accumulator that holds the current partial sum from Bellard's formula.
    let (mut n, mut acc) = (0u64..)
        .map(Natural::from)
        .take_while(|n| {
            // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
            &offset - Z5 - Z10 * Integer::from(n) + Z8 >= (N10 * n + N1).significant_bits() - 1
        })
        .par_bridge()
        .into_par_iter()
        // we shift left by the number of bits to bring the result from the [0, 1] back into the [0, 2^bits] range.
        .map(|n| (N1, bellard_term(&n, &offset, precision) << BITS))
        // add the terms and their count together.
        .reduce(|| (N0, I0), |(n1, d1), (n2, d2)| (n1 + n2, d1 + d2));

    loop {
        // Bellard's formula uses -6, but this uses -5 because we are calculating tau, not pi.
        let exp = &offset - Z5 - Z10 * Integer::from(&n);

        // this is the nth term of Bellard's formula, using `sigma_interval` for each of the individual fractions. we multiply by 256 so that the mod one in `bellard_delta` becomes a mod 256, allowing us to extract a byte at a time.
        let change = bellard_term(&n, &offset, precision) << BITS;

        // add the current term to the accumulator.
        acc += &change;

        // an interval that definitely contains the result of the entire infinite series, used for testing convergence. Follows from the bounds for an alternating series.
        let mut bounds: Interval = &acc - (change.maybe() >> 10);

        // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
        if exp + Z8 >= (N10 * &n + N1).significant_bits() - 1 {
            n += N1;
            continue;
        }

        // everything after this line only tests for convergence, so we can increment the loop index right now.
        n += N1;

        // if `try_floor` is Some, then the bounds have converged enough for us to add another byte to the list of known bytes.
        while let Some(floor) = bounds.try_floor() {
            let byte = u8::wrapping_from(&floor);
            bytes.push(byte);

            if let Some(max_bytes) = max_batch_size
                && bytes.len() >= max_bytes.get()
            {
                bytes.truncate(max_bytes.get());
                return Ok(bytes);
            }

            // subtract the whole part of the bounds and multiply by 256 to get the next byte.
            bounds -= Interval::from(floor);
            bounds <<= BITS;

            // do the same for the accumulator.
            acc -= Interval::from(acc.try_floor().unwrap());
            acc <<= BITS;

            // adjust the offset accordingly.
            offset += Z_BITS;
        }

        match acc.inner_int() {
            // if the accumulator interval contains no integers, it is worth it to keep adding terms in the hope that the bounds for later iterations will shrink enough to get another byte.
            Err(Z0) => continue,
            // if the interval instead contains several integers, adding more terms definitely won't make it converge to a single integer, so we are just done.
            Err(_) => break,
            // if the interval contains exactly one integer, we have to do more sophisticated logic to determine if the interval can still converge or not.
            Ok(inner_int) => {
                // note that because we are only ever adding intervals to the accumulator, its width (the distance between the upper and lower bounds) can stay the same or grow, but never decrease.
                let width = &acc.width();

                // the highest that the accumulator's lower bound can be is the upper outer bound minus the accumulator's width, which would occur if the accumulator is at the highest end of the outer bound.
                let (max_inf, _) = &bounds.sup.sub_round_ref_ref(width, RoundingMode::Floor);

                // the lowest that the accumulator's upper bound can be is the lower outer bound plus the accumulator's width, which would occur if the accumulator is at the lowest end of the outer bound.
                let (min_sup, _) = &bounds.inf.add_round_ref_ref(width, RoundingMode::Ceiling);

                // if the highest lower bound is less than an integer, and the lowest higher bound is greater than that integer, then we can never know on which side the value of this byte lies, so we are done.
                if max_inf <= &inner_int && min_sup >= &inner_int {
                    break;
                }
            }
        }
    }

    if bytes.len() >= min_batch_size {
        Ok(bytes)
    } else {
        Err(bytes)
    }
}

pub fn calculate_byte_range_non_parallel(
    start: &Natural,
    precision: u64,
    min_batch_size: usize,
    max_batch_size: Option<NonZeroUsize>,
) -> Result<Vec<u8>, Vec<u8>> {
    // the number of bits to calculate at a time, which is 8 in this case because we are calculating bytes.
    const BITS: u64 = 8;
    const Z_BITS: Integer = Integer::const_from_unsigned(BITS);

    // return early if precision isn't large enough to avoid a few edge cases
    if precision < 2 {
        return Err(Vec::new());
    }

    // the number of bits to shift the calculation over by, as we don't need to calculate any of the bytes before `start`. also shift by lb(base) bits in order to put the calculated value from the [0, 2^bits] range to the [0, 1] range.
    let mut offset = Z8 * Integer::from(start) - Z_BITS;

    // the bytes of tau calculated so far, starting at `start`.
    let mut bytes = Vec::new();

    // the iteration number, because ranges of `malachite::Natural` can't be iterated over directly.
    let mut n = N0;

    // the accumulator that holds the current partial sum from Bellard's formula.
    let mut acc = I0;

    loop {
        // Bellard's formula uses -6, but this uses -5 because we are calculating tau, not pi.
        let exp = &offset - Z5 - Z10 * Integer::from(&n);

        // this is the nth term of Bellard's formula, using `sigma_interval` for each of the individual fractions. we multiply by 256 so that the mod one in `bellard_delta` becomes a mod 256, allowing us to extract a byte at a time.
        let change = bellard_term(&n, &offset, precision) << BITS;

        // add the current term to the accumulator.
        acc += &change;

        // an interval that definitely contains the result of the entire infinite series, used for testing convergence. Follows from the bounds for an alternating series.
        let mut bounds: Interval = &acc - (change.maybe() >> 10);

        // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
        if exp + Z8 >= (N10 * &n + N1).significant_bits() - 1 {
            n += N1;
            continue;
        }

        // everything after this line only tests for convergence, so we can increment the loop index right now.
        n += N1;

        // if `try_floor` is Some, then the bounds have converged enough for us to add another byte to the list of known bytes.
        while let Some(floor) = bounds.try_floor() {
            let byte = u8::wrapping_from(&floor);
            bytes.push(byte);

            if let Some(max_bytes) = max_batch_size
                && bytes.len() >= max_bytes.get()
            {
                bytes.truncate(max_bytes.get());
                return Ok(bytes);
            }

            // subtract the whole part of the bounds and multiply by 256 to get the next byte.
            bounds -= Interval::from(floor);
            bounds <<= BITS;

            // do the same for the accumulator.
            acc -= Interval::from(acc.try_floor().unwrap());
            acc <<= BITS;

            // adjust the offset accordingly.
            offset += Z_BITS;
        }

        match acc.inner_int() {
            // if the accumulator interval contains no integers, it is worth it to keep adding terms in the hope that the bounds for later iterations will shrink enough to get another byte.
            Err(Z0) => continue,
            // if the interval instead contains several integers, adding more terms definitely won't make it converge to a single integer, so we are just done.
            Err(_) => break,
            // if the interval contains exactly one integer, we have to do more sophisticated logic to determine if the interval can still converge or not.
            Ok(inner_int) => {
                // note that because we are only ever adding intervals to the accumulator, its width (the distance between the upper and lower bounds) can stay the same or grow, but never decrease.
                let width = &acc.width();

                // the highest that the accumulator's lower bound can be is the upper outer bound minus the accumulator's width, which would occur if the accumulator is at the highest end of the outer bound.
                let (max_inf, _) = &bounds.sup.sub_round_ref_ref(width, RoundingMode::Floor);

                // the lowest that the accumulator's upper bound can be is the lower outer bound plus the accumulator's width, which would occur if the accumulator is at the lowest end of the outer bound.
                let (min_sup, _) = &bounds.inf.add_round_ref_ref(width, RoundingMode::Ceiling);

                // if the highest lower bound is less than an integer, and the lowest higher bound is greater than that integer, then we can never know on which side the value of this byte lies, so we are done.
                if max_inf <= &inner_int && min_sup >= &inner_int {
                    break;
                }
            }
        }
    }

    if bytes.len() >= min_batch_size {
        Ok(bytes)
    } else {
        Err(bytes)
    }
}

pub(crate) fn send_byte_range_parallel(
    start: &Natural,
    precision: u64,
    min_batch_size: usize,
    max_batch_size: Option<NonZeroUsize>,
    sender: Sender<Status>,
) {
    // the number of bits to calculate at a time, which is 8 in this case because we are calculating bytes.
    const BITS: u64 = 8;
    const Z_BITS: Integer = Integer::const_from_unsigned(BITS);

    // return early if precision isn't large enough to avoid a few edge cases
    if precision < 2 {
        sender
            .send(Status::InsufficientPrecision(Vec::new()))
            .expect("");
        return;
    }

    // the number of bits to shift the calculation over by, as we don't need to calculate any of the bytes before `start`. also shift by lb(base) bits in order to put the calculated value from the [0, 2^bits] range to the [0, 1] range.
    let mut offset = Z8 * Integer::from(start) - Z_BITS;

    // the bytes of tau calculated so far, starting at `start`.
    let mut bytes = Vec::new();

    // terms ~= bytes * 8 (bits/byte) / 10 (bits/term)
    let expected_iters = ((start + Natural::from(min_batch_size)) * N8) / N10;

    // n: the iteration number, because ranges of `malachite::Natural` can't be iterated over directly.
    // acc: the accumulator that holds the current partial sum from Bellard's formula.
    let (mut n, mut acc) = (0u64..)
        .map(Natural::from)
        .take_while(|n| {
            // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
            &offset - Z5 - Z10 * Integer::from(n) + Z8 >= (N10 * n + N1).significant_bits() - 1
        })
        .par_bridge()
        .into_par_iter()
        // we shift left by the number of bits to bring the result from the [0, 1] back into the [0, 2^bits] range.
        .map(|n| {
            sender
                .send(Status::Calculating {
                    current: n.clone(),
                    expected: expected_iters.clone(),
                })
                .expect(SENDABLE);
            (N1, bellard_term(&n, &offset, precision) << BITS)
        })
        // add the terms and their count together.
        .reduce(|| (N0, I0), |(n1, d1), (n2, d2)| (n1 + n2, d1 + d2));

    loop {
        // Bellard's formula uses -6, but this uses -5 because we are calculating tau, not pi.
        let exp = &offset - Z5 - Z10 * Integer::from(&n);

        // this is the nth term of Bellard's formula, using `sigma_interval` for each of the individual fractions. we multiply by 256 so that the mod one in `bellard_delta` becomes a mod 256, allowing us to extract a byte at a time.
        let change = bellard_term(&n, &offset, precision) << BITS;

        // add the current term to the accumulator.
        acc += &change;

        // an interval that definitely contains the result of the entire infinite series, used for testing convergence. Follows from the bounds for an alternating series.
        let mut bounds: Interval = &acc - (change.maybe() >> 10);

        sender
            .send(Status::Calculating {
                current: n.clone(),
                expected: expected_iters.clone(),
            })
            .expect(SENDABLE);

        // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
        if exp + Z8 >= (N10 * &n + N1).significant_bits() - 1 {
            n += N1;
            continue;
        }

        // everything after this line only tests for convergence, so we can increment the loop index right now.
        n += N1;

        // if `try_floor` is Some, then the bounds have converged enough for us to add another byte to the list of known bytes.
        while let Some(floor) = bounds.try_floor() {
            let byte = u8::wrapping_from(&floor);
            bytes.push(byte);

            if let Some(max_bytes) = max_batch_size
                && bytes.len() >= max_bytes.get()
            {
                bytes.truncate(max_bytes.get());
                sender.send(Status::Finished(bytes)).expect(SENDABLE);
                return;
            }

            // subtract the whole part of the bounds and multiply by 256 to get the next byte.
            bounds -= Interval::from(floor);
            bounds <<= BITS;

            // do the same for the accumulator.
            acc -= Interval::from(acc.try_floor().unwrap());
            acc <<= BITS;

            // adjust the offset accordingly.
            offset += Z_BITS;
        }

        match acc.inner_int() {
            // if the accumulator interval contains no integers, it is worth it to keep adding terms in the hope that the bounds for later iterations will shrink enough to get another byte.
            Err(Z0) => continue,
            // if the interval instead contains several integers, adding more terms definitely won't make it converge to a single integer, so we are just done.
            Err(_) => break,
            // if the interval contains exactly one integer, we have to do more sophisticated logic to determine if the interval can still converge or not.
            Ok(inner_int) => {
                // note that because we are only ever adding intervals to the accumulator, its width (the distance between the upper and lower bounds) can stay the same or grow, but never decrease.
                let width = &acc.width();

                // the highest that the accumulator's lower bound can be is the upper outer bound minus the accumulator's width, which would occur if the accumulator is at the highest end of the outer bound.
                let (max_inf, _) = &bounds.sup.sub_round_ref_ref(width, RoundingMode::Floor);

                // the lowest that the accumulator's upper bound can be is the lower outer bound plus the accumulator's width, which would occur if the accumulator is at the lowest end of the outer bound.
                let (min_sup, _) = &bounds.inf.add_round_ref_ref(width, RoundingMode::Ceiling);

                // if the highest lower bound is less than an integer, and the lowest higher bound is greater than that integer, then we can never know on which side the value of this byte lies, so we are done.
                if max_inf <= &inner_int && min_sup >= &inner_int {
                    break;
                }
            }
        }
    }

    if bytes.len() >= min_batch_size {
        sender.send(Status::Finished(bytes)).expect(SENDABLE);
    } else {
        sender
            .send(Status::InsufficientPrecision(bytes))
            .expect(SENDABLE);
    }
}

/*

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
    u8::wrapping_from(
        &Integer::rounding_from(&r, RoundingMode::Floor)
            .0
            .mod_power_of_2(4),
    )
}

pub fn calc_nibble_interval(p: Natural, fp: u64) -> Option<u8> {
    let mut acc = I0;
    let mut i = N0;
    let mut exp = (Integer::from(&p) << 2) - Z9; // 4i - 9

    loop {
        let sign = if (&i).even() { F1 } else { FN1 };

        let change = sign
            * F16
            * (-sigma_interval(&(&exp + Z5), N4 * &i + N1, fp)
                - sigma_interval(&exp, N4 * &i + N3, fp)
                + sigma_interval(&(&exp + Z8), N10 * &i + N1, fp)
                - sigma_interval(&(&exp + Z6), N10 * &i + N3, fp)
                - sigma_interval(&(&exp + Z2), N10 * &i + N5, fp)
                - sigma_interval(&(&exp + Z2), N10 * &i + N7, fp)
                + sigma_interval(&exp, N10 * &i + N9, fp));

        // add the current term to the accumulator.
        acc += &change;

        i += N1;
        exp -= Z10;

        // if this is true then at least one of the `sigma_interval` calls will have wrapped, which means that we can't properly check for convergence, but also that we don't have to, because the change is too large for the byte to have converged.
        // if &exp + Z8 >= (N10 * &i + N1).significant_bits() - 1 {
        //     continue;
        // }

        // note that because we are only ever adding intervals to the accumulator, its width (the distance between the upper and lower bounds) can stay the same or grow, but never decrease, which is important for several of the statements below.
        let acc_width = &acc.width();

        // eprintln!("p: {p}, fp: {fp}, bounds: {bounds}, acc: {acc}");

        // let inner_int = match inner_int(&acc) {
        //     // if the accumulator interval contains no integers, it is worth it to keep adding terms in the hope that the bounds for later iterations will shrink enough to get another byte.
        //     Err(Z0) => {
        //         eprintln!("no ints");
        //         if let Some(nibble) = bounds.try_floor() {
        //             eprintln!("got nibble: {}", u8::wrapping_from(&nibble) % 16);
        //             return Some(u8::wrapping_from(&nibble) % 16);
        //         } else {
        //             eprintln!("no nibble :(");
        //             continue;
        //         }
        //     }
        //     // if the interval contains exactly one integer, we have to do more sophisticated logic to determine if the interval can still converge or not.
        //     Ok(inner_int) => {
        //         eprintln!("an int");
        //         inner_int
        //     }
        //     // if the interval instead contains several integers, adding more terms definitely won't make it converge to a single integer, so we are just done.
        //     Err(_) => {
        //         eprintln!("many ints");
        //         // eprintln!("oh no, acc be so damn fat");
        //         // dbg!(&start, &n, &precision, &bounds, &acc, &bytes);
        //         return None;
        //     }
        // };

        // // the highest that the accumulator's lower bound can be is the upper outer bound minus the accumulator's width, which would occur if the accumulator is at the highest end of the outer bound.
        // let highest_lower_bound = &bounds
        //     .upper
        //     .sub_round_ref_ref(acc_width, RoundingMode::Floor)
        //     .0;

        // // the lowest that the accumulator's upper bound can be is the lower outer bound plus the accumulator's width, which would occur if the accumulator is at the lowest end of the outer bound.
        // let lowest_upper_bound = &bounds
        //     .lower
        //     .add_round_ref_ref(acc_width, RoundingMode::Ceiling)
        //     .0;

        // // if the highest lower bound is less than an integer, and the lowest higher bound is greater than that integer, then we can never know on which side the value of this byte lies, so we are done.
        // if highest_lower_bound <= &inner_int && lowest_upper_bound >= &inner_int {
        //     // if bytes.is_empty() {
        //     //     eprintln!("oh no, the precision wasn't enough");
        //     //     dbg!(&start, &n, &precision, &bounds, &acc, &bytes);
        //     // }
        //     eprintln!("cant converge");
        //     return None;
        // }

        let floor = acc.try_floor()?;
        let ceiling = acc.try_ceil()?;

        // multiply by 64 > 44.22...
        let (next_odd_exp, next_even_exp) = if i.even() {
            (&exp - Z4, &exp + Z6) // next odd is smaller than next even
        } else {
            (&exp + Z6, &exp - Z4) // next odd is larger than next odd
        };

        let dist_to_floor = &acc - Interval::from(Float::from_integer_prec_ref(&floor, fp));
        if dist_to_floor.inf >> i64::saturating_from(&next_odd_exp) <= 1 {
            // eprintln!("\rnibble {} needs another term from below", p.clone());
            continue;
        }

        let dist_to_ceil = Interval::from(Float::from_integer_prec(ceiling, fp)) - &acc;
        if dist_to_ceil.inf >> i64::saturating_from(&next_even_exp) <= 1 {
            // eprintln!("\rnibble {} needs another term from above", p.clone());
            continue;
        }

        return Some(u8::wrapping_from(&floor) % 16);
    }
}

*/

// fn sigma(exp: &Integer, denom: Natural, fp: u64) -> Float {
//     if denom == 1 {
//         return F1;
//     }

//     let fdenom: Float = Float::from_natural_prec_ref(&denom, fp).0;
//     let pow: Natural = exp.unsigned_abs();

//     if exp > &0 {
//         // (2^exp / denom) mod 1
//         // note: denom is in practice never going to be large enough for the conversion to fail
//         Float::try_from(N2.mod_pow(pow, denom)).unwrap() / fdenom
//     } else {
//         // (2^exp / denom) mod 1 = 1/(denom * 2^(-pow))
//         // note: exp is never negative enough for saturating from to give an incorrect result
//         (fdenom << u64::saturating_from(&pow)).reciprocal()
//     }
// }

// #[cfg(not(feature = "alternate"))]
// fn sigma_interval(exp: &Integer, denom: Natural, fp: u64) -> Interval {
//     if denom == 1 {
//         return I1;
//     }
//     let denom_int: Interval = Float::from_natural_prec_ref(&denom, fp).into();
//     let pow: Natural = exp.unsigned_abs();

//     if exp > &0 {
//         // (2^exp / denom) mod 1
//         N2.mod_pow(pow, denom) / denom_int
//     } else {
//         // (2^exp / denom) mod 1 = 1/(denom * 2^(-pow))
//         // note: exp is never negative enough for saturating from to give an incorrect result
//         (denom_int << u64::saturating_from(&pow)).reciprocal()
//     }
// }

/// One term of [Bellard's formula](https://en.wikipedia.org/wiki/Bellard's_formula) for tau.
fn bellard_term(n: &Natural, offset: &Integer, precision: u64) -> Interval {
    // Bellard's formula uses -6, but this uses -5 because we are calculating tau, not pi.
    let exp = offset - Z5 - Z10 * Integer::from(n);
    let mut term = -mod_pow_div(&(&exp + Z5), N4 * n + N1, precision)
        - mod_pow_div(&exp, N4 * n + N3, precision)
        + mod_pow_div(&(&exp + Z8), N10 * n + N1, precision)
        - mod_pow_div(&(&exp + Z6), N10 * n + N3, precision)
        - mod_pow_div(&(&exp + Z2), N10 * n + N5, precision)
        - mod_pow_div(&(&exp + Z2), N10 * n + N7, precision)
        + mod_pow_div(&exp, N10 * n + N9, precision);

    if n.odd() {
        term.neg_assign()
    }

    term
}

/// Calculate (2^exp / denom) mod 1 at the given precision.
fn mod_pow_div(exp: &Integer, denom: Natural, fp: u64) -> Interval {
    // this is the only place we need to explicitly set the precision since it is automatically propagated everywhere else.
    let denom_int = Interval::from(Float::from_natural_prec_ref(&denom, fp)).reciprocal();

    // to calculate (2^exp / denom) mod 1 quickly, we instead calculate (2^exp mod denom) / denom.
    // this lets us use modular exponentiation to save time, because 2^exp takes O(exp) time, but (2^exp mod denom) takes only O(log exp) time.

    // modular arithmetic is only needed when 2^exp >= denom,
    // taking the log (base 2) of both sides give exp >= lb(denom),
    // since exp is always a whole number, this is equivalent to exp >= floor(lb(denom)),
    // and since significant_bits(x) = floor(lb(x)) + 1, we get exp >= significant_bits(denom) - 1.
    // NOTE: benchmarks suggest that this is just as fast as comparing with zero
    if exp >= &(denom.significant_bits() - 1) {
        if denom == 1 {
            return I0;
        }
        denom_int * N2.mod_pow(&exp.unsigned_abs(), &denom) // (1 / denom) * (2^exp mod denom)
    } else {
        denom_int << i64::try_from(exp).expect(NICE_CAST) // (1 / denom) * (2^exp)
    }
}

#[cfg(test)]
mod tests {
    // use super::*;

    // #[test]
    // fn first_256() {
    //     for (i, &expected) in TAU.iter().enumerate().take(256) {
    //         let precision = (2 * (i + 1).ilog2() + 8).into();
    //         assert_eq!(calculate_byte(&i.into(), precision), expected);
    //     }
    // }

    // #[test]
    // fn first_4096() {
    //     for (i, &expected) in TAU.iter().enumerate().take(4096) {
    //         let precision = (2 * (i + 1).ilog2() + 8).into();
    //         assert_eq!(calculate_byte(&i.into(), precision), expected);
    //     }
    // }

    // #[test]
    // fn first_256_interval() {
    //     for (i, &expected) in TAU.iter().enumerate().take(256) {
    //         let precision = 38;
    //         assert_eq!(
    //             calculate_byte_interval(&i.into(), precision).unwrap(),
    //             expected
    //         );
    //     }
    // }

    // #[test]
    // fn first_4096_interval() {
    //     for (i, &expected) in TAU.iter().enumerate().take(4096) {
    //         let precision = 38;
    //         assert_eq!(
    //             calculate_byte_interval(&i.into(), precision).unwrap(),
    //             expected
    //         );
    //     }
    // }

    // #[test]
    // fn last_131072() {
    //     for (i, &expected) in TAU.iter().enumerate().skip(131_056) {
    //         let precision = (2 * (i + 1).ilog2() + 8).into();
    //         assert_eq!(calculate_byte(&i.into(), precision), expected);
    //     }
    // }

    // #[test]
    // fn last_131072_interval() {
    //     for (i, &expected) in TAU.iter().enumerate().skip(131_056) {
    //         let precision = 50;
    //         assert_eq!(
    //             calculate_byte_interval(&i.into(), precision).unwrap(),
    //             expected
    //         );
    //     }
    // }

    // const TAU: &[u8; 131073] = include_bytes!("tau-test");
}
