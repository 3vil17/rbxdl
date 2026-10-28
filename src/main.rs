#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod download;
mod error;
mod mesh;
mod migrate;
mod pool;
mod resolver;
mod roblox;
mod scan;
mod server;
mod store;
mod studio;

fn main() {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(server::run(std::env::args().any(|a| a == "--updated"))) {
        eprintln!("rbxyoinker exited with an error: {e}");
        std::process::exit(1);
    }
}
