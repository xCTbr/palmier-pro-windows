//! `palmier serve` — the MCP daemon.
//!
//! Binds loopback only, so nothing on the network can reach the user's projects (FR-001).

use std::net::{Ipv4Addr, SocketAddr};

use anyhow::Context;

pub async fn run(port: u16) -> anyhow::Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot bind {address} — is another palmier already running?"))?;

    let bound = listener.local_addr()?;
    println!("palmier: MCP at http://{bound}/mcp");
    // Say it now, not when an export fails twenty minutes into a session.
    let missing = crate::missing_tools();
    if !missing.is_empty() {
        eprintln!(
            "palmier: warning — {} not on PATH. Editing works; rendering and media import do not.",
            missing.join(" and ")
        );
    }
    println!("  claude mcp add --transport http palmier http://{bound}/mcp");

    axum::serve(listener, palmier_mcp::http_router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("server stopped unexpectedly")?;
    Ok(())
}
