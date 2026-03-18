use crate::model;
use argh::FromArgs;
use std::{
    collections::BinaryHeap,
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    sync::mpsc,
    thread,
    time::Instant,
};

#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help"))]
/// Calculate the binary expansion of tau (or pi i guess) to arbitrary precision
pub struct CliOptions {
    /// write the bytes to a binary file
    #[argh(positional)]
    pub output: Option<String>,
    /// write a <byte number>, <byte>, <byte time>, <total time>, log to a file
    #[argh(option, short = 'L')]
    pub log: Option<String>,
    /// start the calculation at a certain byte number
    #[argh(option, short = 'S')]
    pub start: Option<isize>,
    /// end the calculation at a certain byte number
    #[argh(option, short = 'E', default = "isize::MAX")]
    pub end: isize,
    /// run using the specified number of threads
    #[argh(option, short = 'T', default = "1")]
    pub threads: isize,
    /// calculate pi instead of tau (only losers pick this)
    #[argh(switch, short = 'p')]
    pub pi: bool,
    /// print the latest byte while running
    #[argh(switch, short = 'l')]
    pub live: bool,
    /// don't write bytes at their corresponding position. useful if the starting byte is large
    #[argh(switch, short = 'j')]
    pub no_jump: bool,
}

pub fn run(options: CliOptions) {
    let mut output = options.output.and_then(|path| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()
    });

    let mut log = options
        .log
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

    let start: isize = options.start.unwrap_or(len);
    let thread_count = options.threads.try_into().unwrap();

    let time = Instant::now();
    let (sender, reciever) = mpsc::channel();

    for thread_id in 0..options.threads {
        let channel = sender.clone();
        thread::spawn(move || {
            for byte_number in (start + thread_id..=options.end).step_by(thread_count) {
                let digit_time = Instant::now();
                let byte = model::calc_byte(byte_number, options.pi);

                channel
                    .send(model::ByteInfo::new(
                        byte_number,
                        byte,
                        digit_time.elapsed().as_millis() as usize,
                        time.elapsed().as_millis() as usize,
                    ))
                    .unwrap();
            }
        });
    }
    drop(sender);
    let mut queue = BinaryHeap::new();
    let mut latest = model::ByteInfo::new(start - 1, 0, 0, 0);
    let mut bytes = Vec::with_capacity(thread_count * 2);

    if let Some(file) = output.as_mut() {
        file.seek(SeekFrom::Start(if options.no_jump {
            0
        } else {
            start.try_into().unwrap()
        }))
        .expect("should have moved to correct position");
    }

    for item in reciever {
        queue.push(std::cmp::Reverse(item));

        while queue
            .peek()
            .is_some_and(|data| data.0.pos == latest.pos + 1)
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

        if options.live {
            print!("\r{latest:?}",);
            std::io::stdout().flush().unwrap();
        }

        if let Some(file) = output.as_mut() {
            file.write_all(&bytes).expect("should have written bytes");
        };

        bytes.clear();
    }
}
