mod cli;
mod model;

#[global_allocator]
// static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    cli::run(argh::from_env::<cli::CliOptions>());
}
