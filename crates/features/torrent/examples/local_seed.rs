//! Dev tool: seed one local file on 127.0.0.1 (no DHT, no trackers) so the app
//! can stream it with `CINMA_DEV_PEERS=<addr>`. Optional upload cap simulates a
//! slow swarm.
//!
//!   cargo run --release -p cinma-torrent --example local_seed -- <file> [port] [upload_bytes_per_sec]

use std::num::NonZeroU32;
use std::path::PathBuf;

use librqbit::limits::LimitsConfig;
use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions, Session,
    SessionOptions, create_torrent,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let file = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("usage: local_seed <file> [port] [upload_bps]"))?);
    let port: u16 = args.next().map(|p| p.parse()).transpose()?.unwrap_or(0);
    let upload_bps: Option<NonZeroU32> = args.next().map(|b| b.parse()).transpose()?;
    let dir = file.parent().ok_or_else(|| anyhow::anyhow!("file has no parent dir"))?.to_path_buf();

    let torrent = create_torrent(
        &file,
        CreateTorrentOptions { name: None, trackers: vec![], piece_length: Some(1024 * 1024) },
        &BlockingSpawner::new(2),
    )
    .await?;
    let session = Session::new_with_opts(
        dir.clone(),
        SessionOptions {
            dht: None,
            fastresume: false,
            persistence: None,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: format!("127.0.0.1:{port}").parse()?,
                ..Default::default()
            }),
            ratelimits: LimitsConfig { upload_bps, download_bps: None },
            ..Default::default()
        },
    )
    .await?;
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(torrent.as_bytes()?),
            Some(AddTorrentOptions {
                output_folder: Some(dir.to_string_lossy().into_owned()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .ok_or_else(|| anyhow::anyhow!("no handle"))?;
    handle.wait_until_initialized().await?;
    println!("INFO_HASH={}", torrent.info_hash().as_string());
    println!("ADDR={}", session.listen_addr().map(|a| a.to_string()).unwrap_or_default());
    println!("FINISHED={}", handle.stats().finished);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
