//! The desktop shell.
//!
//! It owns the server: the same axum router the CLI serves is started inside this
//! process, and the window is pointed at it. Nothing to launch in a terminal, and one
//! session shared by the window and by whatever agent connects over MCP.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// The port the original uses, so an agent's saved configuration keeps working.
const PREFERRED_PORT: u16 = 19789;

/// Take the usual port when it is free, and any port when it is not.
///
/// A second copy of the app must not fail to start, and must not silently attach itself
/// to the first one's project.
fn pick_port() -> u16 {
    for port in [PREFERRED_PORT, 0] {
        if let Ok(listener) = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            && let Ok(address) = listener.local_addr()
        {
            return address.port();
        }
    }
    PREFERRED_PORT
}

fn main() {
    let port = pick_port();

    tauri::Builder::default()
        .setup(move |app| {
            let session = Arc::new(tokio::sync::Mutex::new(
                palmier_mcp::session::Session::default(),
            ));
            let jobs = palmier_mcp::jobs::Jobs::new();
            let router = palmier_ui::router(palmier_ui::Ui {
                session: session.clone(),
                jobs: jobs.clone(),
                port,
            })
            .merge(palmier_mcp::mcp_router(session, jobs));

            // The server runs for the life of the window; there is nothing to shut down
            // separately, because closing the window ends the process that owns it.
            let runtime = tokio::runtime::Runtime::new()?;
            std::thread::spawn(move || {
                runtime.block_on(async move {
                    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                    if let Ok(listener) = tokio::net::TcpListener::bind(address).await {
                        let _ = axum::serve(listener, router).await;
                    }
                });
            });

            let url = format!("http://127.0.0.1:{port}")
                .parse()
                .expect("a loopback URL is always valid");
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Palmier")
                .inner_size(1200.0, 860.0)
                .min_inner_size(880.0, 620.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the desktop shell failed to start");
}
