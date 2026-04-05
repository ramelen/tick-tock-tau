mod cli;
mod model;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let result = cli::run(argh::from_env::<cli::Config>());
    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
}
