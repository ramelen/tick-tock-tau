pub mod config;

use crate::cli::config::Config;
use crate::model::{self, NICE_CAST};
use malachite::base::num::conversion::string::options::ToSciOptions;
use malachite::base::num::conversion::traits::{OverflowingFrom, ToSci};
use malachite::base::num::logic::traits::SignificantBits;
use malachite::{Natural, Rational};
use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const NZU1: NonZeroUsize = NonZeroUsize::new(1).unwrap();

pub(crate) const SENDABLE: &str = "reciever is still alive";

pub(crate) enum Status {
    InsufficientPrecision(Vec<u8>),
    Calculating { current: Natural, expected: Natural },
    Finished(Vec<u8>),
}

const STATUS_FRAME_TIME: Duration = Duration::from_millis(1);

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

    // try to get the length of the contents of the output file
    let len: u64 = output
        .as_ref()
        .map(|file| file.metadata().map(|metadata| metadata.len()))
        .transpose()? // return if getting the metadata failed
        .unwrap_or(0);

    // the index of the first byte to calculate, with 6 at index 0.
    let first_byte_index = config.start_index.unwrap_or(Natural::from(len));
    let last_byte_index = config.num_bytes.map(|n| &first_byte_index + n);

    if let Some(file) = output.as_mut() {
        let (small_index, overflowed) = u64::overflowing_from(&first_byte_index);
        if !overflowed {
            file.seek(SeekFrom::Start(small_index))?;
        } else {
            eprintln!("Warning: start index is greater than 2^64 - 1, ignoring output file...");
            output = None;
        }
    }

    if let Some(num_threads) = config.num_threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads.get())
            .build_global()
            .expect("this is the only place the thread pool is built")
    }

    let mut current_byte_index = first_byte_index.clone();

    let program_start = Instant::now();

    let mut status_start = Instant::now();
    let mut status_len: usize = 0;
    let mut bytes_per_second = 0.0;
    let mut batch_time = None;

    let mut precision = config
        .initial_precision
        .map(NonZeroU64::get)
        .unwrap_or((first_byte_index.significant_bits() / 4 + 1) * 8);
    // .unwrap_or(u64::try_from(config.min_batch_size.get()).expect(NICE_CAST) * 8);

    'outer: while last_byte_index
        .as_ref()
        .map(|index| index > &current_byte_index)
        .unwrap_or(true)
    {
        let batch_start = Instant::now();

        // last - current + 1
        let bytes_left = last_byte_index
            .as_ref()
            .and_then(|index| usize::try_from(&(index - &current_byte_index)).ok())
            .and_then(|diff| NZU1.checked_add(diff));

        let min_batch_size = config.min_batch_size.get();
        let max_batch_size = match (config.max_batch_size, bytes_left) {
            (None, None) => None,
            (Some(size), None) | (None, Some(size)) => Some(size),
            (Some(config_max), Some(bytes_left)) => Some(config_max.min(bytes_left)),
        };

        if config.is_quiet || config.no_status {
            let bytes = match model::calculate_byte_range_parallel(
                &current_byte_index,
                precision,
                min_batch_size,
                max_batch_size,
            ) {
                Ok(bytes) => bytes,
                Err(partial_bytes) => {
                    let bytes_left =
                        u64::try_from(min_batch_size - partial_bytes.len()).expect(NICE_CAST);
                    precision += bytes_left * 8;
                    continue;
                }
            };

            if let Some(file) = log.as_mut() {
                writeln!(
                    file,
                    "{}, {}, {}, {:.06}, {:.06},",
                    current_byte_index,
                    bytes.len(),
                    precision,
                    batch_start.elapsed().as_secs_f64(),
                    program_start.elapsed().as_secs_f64(),
                )?;
            }

            if let Some(file) = output.as_mut() {
                file.write_all(&bytes)?;
            };

            current_byte_index += Natural::from(bytes.len());

            if !(config.is_quiet || config.no_waterfall) {
                println!(
                    "{current_byte_index:#x}: {}",
                    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
            }
        } else {
            let (sender, reciever) = mpsc::channel::<Status>();

            let handle = std::thread::spawn({
                let current_byte_index = current_byte_index.clone();
                move || {
                    model::send_byte_range_parallel(
                        &current_byte_index,
                        precision,
                        min_batch_size,
                        max_batch_size,
                        sender,
                    );
                }
            });

            for status in reciever {
                match status {
                    Status::Calculating { current, expected } => {
                        const Q100: Rational = Rational::const_from_unsigned(100);
                        let mut options = ToSciOptions::default();
                        options.set_scale(1);
                        options.set_include_trailing_zeros(true);
                        if status_start.elapsed() > STATUS_FRAME_TIME {
                            let status = format!(
                                "calculating batch at index {} (uptime: {}, batch time: {}, speed: {:.1} B/s, precision: {} bits): {:}%",
                                current_byte_index,
                                fmt_time(program_start.elapsed()),
                                fmt_time(batch_time.unwrap_or(batch_start.elapsed())),
                                bytes_per_second,
                                precision,
                                (Rational::from_naturals(current, expected) * Q100)
                                    .to_sci_with_options(options)
                            );
                            let len = status.len();
                            let additional_space =
                                ' '.to_string().repeat(status_len.saturating_sub(len));
                            // the extra space char is so that the cursor isn't right next to the end of the line
                            print!("\r{status}{additional_space} ");
                            std::io::stdout().flush().unwrap();
                            status_len = len;
                            status_start = Instant::now();
                        };
                    }
                    Status::InsufficientPrecision(bytes) => {
                        let bytes_left = min_batch_size - bytes.len();
                        let bytes_left = u64::try_from(bytes_left).expect(NICE_CAST);
                        precision += bytes_left * 8;
                        handle.join().expect("sender didn't panic");
                        continue 'outer;
                    }
                    Status::Finished(bytes) => {
                        if let Some(file) = log.as_mut() {
                            writeln!(
                                file,
                                "{}, {}, {} {:.06}, {:.06},",
                                current_byte_index,
                                bytes.len(),
                                precision,
                                batch_start.elapsed().as_secs_f64(),
                                program_start.elapsed().as_secs_f64(),
                            )?;
                        }

                        if let Some(file) = output.as_mut() {
                            file.write_all(&bytes)?;
                        };

                        if !config.no_waterfall {
                            let waterfall = format!(
                                "{current_byte_index:#x}: {}",
                                bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
                            );

                            let len = waterfall.len();
                            let additional_space =
                                ' '.to_string().repeat(status_len.saturating_sub(len));
                            println!("\r{waterfall}{additional_space}");
                            status_len = 0;
                        }

                        current_byte_index += Natural::from(bytes.len());

                        batch_time = Some(batch_start.elapsed());
                        bytes_per_second = bytes.len() as f64 / batch_start.elapsed().as_secs_f64();

                        let status = format!(
                            "calculating batch at index {} (uptime: {}, batch time: {}, speed: {:.1} B/s, precision: {} bits): 100%",
                            current_byte_index,
                            fmt_time(program_start.elapsed()),
                            fmt_time(batch_time.unwrap()),
                            bytes_per_second,
                            precision
                        );
                        let len = status.len();
                        let additional_space =
                            ' '.to_string().repeat(status_len.saturating_sub(len) + 1);
                        // the extra space char is so that the cursor isn't right next to the end of the line
                        print!("\r{status}{additional_space} ");
                        std::io::stdout().flush().unwrap();
                        status_len = len;
                    }
                }
            }
        }
    }

    Ok(())
}

fn fmt_time(duration: Duration) -> String {
    let seconds = duration.as_secs();

    let days_str = match seconds / 86400 {
        0 => "",
        1 => "1 day, ",
        d => &(d.to_string() + " days, "),
    };

    let hours_str = match (seconds / 3600) % 24 {
        0 => "",
        1 => "1 hour, ",
        h => &(h.to_string() + " hours, "),
    };

    let minutes = (seconds / 60) % 60;
    let minutes_str = match minutes {
        0 => "",
        1 => "1 minute, ",
        m => &(m.to_string() + " minutes, "),
    };

    let seconds_str = match seconds % 60 {
        1 if seconds >= 60 => "1 second",
        s if seconds >= 60 => &(s.to_string() + " seconds"),
        s => &format!("{s}.{:03} seconds", duration.subsec_millis()),
    };

    [days_str, hours_str, minutes_str, seconds_str].concat()
}
