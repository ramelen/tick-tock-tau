use crate::model;
use argh::FromArgs;
use malachite::base::num::conversion::traits::OverflowingFrom;
use malachite::base::num::logic::traits::SignificantBits;
use malachite::{Natural, base::num::basic::traits::One};

use std::{
    collections::BinaryHeap,
    fs::OpenOptions,
    io::{self, Seek, SeekFrom, Write},
    str::FromStr,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Instant,
};

/// Calculate the binary expansion of tau to an arbitrary number of places. Get started with
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help", "help"))]
pub struct Config {
    /// the file path to write the output to. note: the output is in unformatted binary, which may require specialized software to view (I reccomend `hexyl`), and also means that the file extension may be omitted.
    #[argh(option, long = "output", short = 'o')]
    pub output_path: Option<String>,

    /// write log output to the specified path while the program is running
    #[argh(option, long = "log", short = 'l')]
    pub log_path: Option<String>,

    /// skip the specified number of bytes in the output, including the byte before the decimal place. if an output path is supplied, this will default to the size of the file. note that the output bytes will be written to their corresponding index, so skipping a billion bytes will result in a 1 GB file filled with mostly zeros. if this is not desired, consider specifying a log path instead of an output path.
    #[argh(option, long = "skip", short = 's', from_str_fn(parse_natural))]
    // the number of bytes to skip and the start index happen to be the same because the former is one-indexed while the latter is zero-indexed.
    pub start_index: Option<Natural>,

    /// the number of bytes of tau to calculate
    #[argh(option, long = "bytes", short = 'b', from_str_fn(parse_natural))]
    pub num_bytes: Option<Natural>,

    /// calculate bytes in parallel using the specified number of threads (1 by default)
    #[argh(option, long = "threads", short = 't', default = "1")]
    pub num_threads: u64,

    /// do not print the most recent byte information while the program is running. note: this information will still be logged if a path is specified.
    #[argh(switch, long = "quiet", short = 'q')]
    pub is_quiet: bool,

    /// skip calculation of error bounds to save some extra calculations, at the cost of an extraordinarily small chance that the algorithm produces an incorrect digit every once in a while.
    #[argh(switch, long = "fast")]
    pub is_fast: bool,
}

pub fn run(config: Config) -> Result<(), io::Error> {
    // open the output file if a path was given, returning early if the file couldn't be opened
    let mut output = config
        .output_path
        .map(|path| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
        })
        .transpose()?;

    // similarly, open the log file if a path was given
    let mut log = config
        .log_path
        .map(|path| OpenOptions::new().append(true).create(true).open(path))
        .transpose()?;

    // try to get the length
    let len: u64 = output
        .as_ref()
        .map(|file| file.metadata().map(|metadata| metadata.len()))
        .transpose()? // return if getting the metadata failed
        .unwrap_or(0);

    // the index of the first byte to calculate, with 6 at index 0.
    let first_byte_index = config.start_index.unwrap_or(Natural::from(len));
    let last_byte_index = config.num_bytes.map(|n| &first_byte_index + n);

    let program_start_time = Instant::now();
    let (sender, reciever) = mpsc::channel();

    // shared counter holding the next byte index that has not yet started calculation
    let next_byte = Arc::new(Mutex::new(first_byte_index.clone()));

    for _ in 0..config.num_threads {
        let channel = sender.clone();
        let next_byte = Arc::clone(&next_byte);
        let first_byte_index = first_byte_index.clone();
        let last_byte_index = last_byte_index.clone();

        thread::spawn(move || {
            // initial value, incremented each time there is not enough precision
            // empirically a better bound 2.2 * significant_bits + 9, but this is good enough
            let mut interval_precision = 2 * first_byte_index.significant_bits() + 8;

            loop {
                // atomically get and increment the next byte index
                let byte_index = {
                    let mut guard = next_byte.lock().unwrap();
                    // check if we've reached the end
                    if last_byte_index.as_ref().is_some_and(|last| *guard >= *last) {
                        break;
                    }
                    let current = guard.clone();
                    *guard += Natural::ONE;
                    current
                };

                let byte_time = Instant::now();

                let byte = if config.is_fast {
                    let precision = 2 * byte_index.significant_bits() + 8;
                    model::calculate_byte(&byte_index, precision)
                } else {
                    loop {
                        match model::calculate_byte_interval(&byte_index, interval_precision) {
                            Some(byte) => break byte,
                            None => {
                                eprintln!(
                                    "\rbyte {} needs more precision than {interval_precision}         ",
                                    byte_index.clone()
                                );
                                interval_precision += 1
                            }
                        };
                    }
                };

                channel
                    .send(model::ByteInfo::new(
                        byte_index.clone(),
                        byte,
                        byte_time.elapsed().as_millis() as usize,
                        program_start_time.elapsed().as_millis() as usize,
                    ))
                    .unwrap();
            }
        });
    }
    drop(sender);

    let mut queue = BinaryHeap::new();
    // seperate counter for bytes that are in the middle of being calculated
    let mut next_pos = first_byte_index.clone();
    let mut bytes = Vec::with_capacity(2 * config.num_threads as usize);

    if let Some(file) = output.as_mut() {
        let (small_index, overflowed) = u64::overflowing_from(&first_byte_index);
        if !overflowed {
            file.seek(SeekFrom::Start(small_index))?;
        } else {
            eprintln!("Warning: start index is greater than 2^64 - 1, ignoring output file...");
            output = None;
        }
    }

    for item in reciever {
        queue.push(std::cmp::Reverse(item));

        while queue.peek().is_some_and(|data| data.0.pos == next_pos) {
            let data = queue.pop().unwrap().0;
            next_pos += Natural::ONE;

            bytes.push(data.byte);

            if !config.is_quiet {
                print!("\r{data:?}",);
            }

            if let Some(file) = log.as_mut() {
                writeln!(file, "{data}",)?;
            }
        }

        if bytes.is_empty() {
            continue;
        }

        if !config.is_quiet {
            std::io::stdout().flush().unwrap();
        }

        if let Some(file) = output.as_mut() {
            file.write_all(&bytes).expect("should have written bytes");
        };

        bytes.clear();
    }

    Ok(())
}

fn parse_natural(value: &str) -> Result<Natural, String> {
    // remove underscores from input but not commas or periods (as they may be intended as decimal seperators)
    match Natural::from_str(&value.replace('_', "")) {
        Ok(natural) => Ok(natural),
        Err(()) => Err(String::from(
            "input must consist only of digits and underscores",
        )),
    }
}
