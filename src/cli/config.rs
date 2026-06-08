use argh::FromArgs;
use malachite::Natural;
use std::num::{NonZeroU64, NonZeroUsize};
use std::str::FromStr;

const NZU1: NonZeroUsize = NonZeroUsize::new(1).unwrap();

/// Calculate the binary expansion of tau (2 pi) to an arbitrary number of places.
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help", "help"))]
pub struct Config {
    /// the file path to write the output to. note: the output is in unformatted binary, which may require specialized software to view (e.g. `hexyl`), and also means that the file extension may be omitted.
    #[argh(option, long = "output", short = 'o')]
    pub output_path: Option<String>,

    /// write log output to the specified path while the program is running.
    #[argh(option, long = "log", short = 'l')]
    pub log_path: Option<String>,

    /// skip the specified number of bytes in the output, including the byte before the decimal place. if an output path is supplied, this will default to the size of the file. note that the output bytes will be written to their corresponding index, so skipping a billion bytes will result in a 1 GB file filled with mostly zeros. if this is not desired, consider specifying a log path instead of an output path.
    #[argh(option, long = "skip", short = 's', from_str_fn(parse_natural))]
    // the number of bytes to skip and the start index happen to be the same because the former is one-indexed while the latter is zero-indexed.
    pub start_index: Option<Natural>,

    /// the number of bytes of tau to calculate.
    #[argh(option, long = "bytes", short = 'b', from_str_fn(parse_natural))]
    pub num_bytes: Option<Natural>,

    /// calculate bytes in parallel using the specified number of threads, which is set by default to the number of cpu cores on your system.
    #[argh(option, long = "threads", short = 't')]
    pub num_threads: Option<NonZeroUsize>,

    /// the minimum number of bytes to calculate in each batch (default 1). higher values will speed up calculation (to a certain point) but will also increase memory usage.
    #[argh(option, long = "min-batch-size", default = "NZU1")]
    pub min_batch_size: NonZeroUsize,

    /// a maximum number of bytes to calculate per iteration. takes precedence over `min-batch-size`.
    #[argh(option, long = "max-batch-size")]
    pub max_batch_size: Option<NonZeroUsize>,

    /// an initial precision (in bits) for intermediate calculations. If not specified it is automatically determined based on `min_batch_size` and `skip`, and should only be changed if the default is too much of an under- or over-estimate.
    #[argh(option, long = "initial-precision")]
    pub initial_precision: Option<NonZeroU64>,

    /// hide the line of information about the current batch of bytes to stdout.
    #[argh(switch, long = "no-status")]
    pub no_status: bool,

    /// do not print out batches of bytes as they are calculated.
    #[argh(switch, long = "no-waterfall")]
    pub no_waterfall: bool,

    /// shorthand for `--no-status --no-waterfall`.
    #[argh(switch, long = "quiet", short = 'q')]
    pub is_quiet: bool,
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
