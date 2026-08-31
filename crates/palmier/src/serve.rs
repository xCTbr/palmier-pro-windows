//! `palmier serve` — the MCP daemon.
//!
//! Binds loopback only, so nothing on the network can reach the user's projects (FR-001).

use std::net::{Ipv4Addr, SocketAddr};

use anyhow::Context;

pub async fn run(port: u16) -> anyhow::Result<()> {
    // One session, shared by every MCP client and by the interface, so the agent and
    // the screen are always looking at the same film.
    let session = std::sync::Arc::new(tokio::sync::Mutex::new(
        palmier_mcp::session::Session::default(),
    ));
    let jobs = palmier_mcp::jobs::Jobs::new();
    let router = palmier_ui::router(palmier_ui::Ui {
        session: session.clone(),
        jobs: jobs.clone(),
        port,
    })
    .merge(palmier_mcp::mcp_router(session, jobs));

    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot bind {address} — is another palmier already running?"))?;

    let bound = listener.local_addr()?;
    println!("palmier: interface at http://{bound}");
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

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("server stopped unexpectedly")?;
    Ok(())
}

/// Serve over stdin/stdout for a client that spawns this process.
///
/// Every diagnostic goes to stderr: stdout carries protocol frames, and one stray line
/// there breaks the session.
pub async fn run_stdio() -> anyhow::Result<()> {
    let missing = crate::missing_tools();
    if !missing.is_empty() {
        eprintln!(
            "palmier: warning — {} not on PATH. Editing works; rendering and media import do not.",
            missing.join(" and ")
        );
    }
    palmier_mcp::serve_stdio()
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
}
