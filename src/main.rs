use tick_tock_tau::cli;

#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let result = cli::run(argh::from_env::<cli::config::Config>());
    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
}
