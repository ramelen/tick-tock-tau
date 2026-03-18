use crate::model::{self, MathError};
use argh::FromArgs;
use std::{
    collections::BinaryHeap,
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    sync::mpsc,
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

    /// skip the specified number of bytes in the output, including the byte before the decimal place. if an output path is supplied, this will default to the size of the file. note that the output bytes will be written to their corresponding index, so skipping a billion bytes will result in a 1 GB file filled with mostly zeros.
    #[argh(option, long = "skip", short = 's')]
    // the number of bytes to skip and the start index happen to be the same because the former is one-indexed while the latter is zero-indexed.
    pub start_index: Option<usize>,

    /// the number of bytes of tau to calculate
    #[argh(option, long = "bytes", short = 'b', default = "usize::MAX")]
    pub num_bytes: usize,

    /// calculate bytes in parallel using the specified number of threads (1 by default)
    #[argh(option, long = "threads", short = 't', default = "1")]
    pub num_threads: usize,

    /// do not print the most recent byte information while the program is running. note: this information will still be logged if a path is specified.
    #[argh(switch, long = "quiet", short = 'q')]
    pub is_quiet: bool,
}

pub fn run(config: Config) {
    let mut output = config.output_path.and_then(|path| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()
    });

    let mut log = config
        .log_path
        .and_then(|path| OpenOptions::new().append(true).create(true).open(path).ok());

    let len = output
        .as_ref()
        .and_then(|file| {
            file.metadata()
                .expect("should have gotten metadata")
                .len()
                .try_into()
                .ok()
        })
        .unwrap_or(0);

    let first_byte_index = config.start_index.unwrap_or(len);

    let program_start_time = Instant::now();
    let (sender, reciever) = mpsc::channel();

    for thread_id in 0..config.num_threads {
        let channel = sender.clone();
        thread::spawn(move || {
            let mut interval_precision = 12;
            let byte_indices = first_byte_index..(first_byte_index + config.num_bytes);
            for byte_index in byte_indices.skip(thread_id).step_by(config.num_threads) {
                let byte_time = Instant::now();
                let precision = (2 * (byte_index + 1).ilog2() + 8).into();
                let byte = model::calculate_byte(byte_index, precision).unwrap();
                let byte_interval = loop {
                    match model::calculate_byte_interval(byte_index, interval_precision) {
                        Ok(byte) => break byte,
                        Err(MathError::InsufficientPrecision(_, _)) => interval_precision += 1,
                        Err(e) => panic!("{e}"),
                    };
                };

                if byte != byte_interval {
                    panic!(
                        "Ordinary byte ({byte:02X}) and interval byte ({byte_interval:02X}) do not agree."
                    );
                }

                channel
                    .send(model::ByteInfo::new(
                        byte_index,
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
    let mut latest = model::ByteInfo::new(first_byte_index.wrapping_sub(1), 0, 0, 0);
    let mut bytes = Vec::with_capacity(config.num_threads * 2);

    if let Some(file) = output.as_mut() {
        file.seek(SeekFrom::Start(first_byte_index.try_into().unwrap()))
            .expect("should have moved to correct position");
    }

    for item in reciever {
        queue.push(std::cmp::Reverse(item));

        while queue
            .peek()
            .is_some_and(|data| data.0.pos == latest.pos.wrapping_add(1))
        {
            latest = queue.pop().unwrap().0;

            bytes.push(latest.byte);

            if let Some(file) = log.as_mut() {
                writeln!(file, "{latest}",).expect("should have written to file");
            }
        }

        if bytes.is_empty() {
            continue;
        }

        if !config.is_quiet {
            print!("\r{latest:?}",);
            std::io::stdout().flush().unwrap();
        }

        if let Some(file) = output.as_mut() {
            file.write_all(&bytes).expect("should have written bytes");
        };

        bytes.clear();
    }
}
