//! Serves the benchmark API over HTTP for `benches/load.sh`: linz-rs at `/`, and the same
//! endpoints written with plain axum at `/plain`.
//!
//! Run with `cargo run --release --example bench_server`. Set PORT to change the port
//! (default 3998) and WORKERS to limit the runtime's worker threads (default: one per core).

#[path = "../benches/common/mod.rs"]
mod common;

fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT").map_or(3998, |port| port.parse().expect("PORT is a port number"));
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    if let Ok(workers) = std::env::var("WORKERS") {
        runtime.worker_threads(workers.parse().expect("WORKERS is a number"));
    }

    runtime.enable_all().build()?.block_on(async {
        let app = common::app().nest("/plain", common::plain_axum_app());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        println!("Listening on http://{}", listener.local_addr()?);
        axum::serve(listener, app).await
    })
}
