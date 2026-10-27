// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! `sorobo-server` – Sorobo without a window. Serves API and frontend on localhost, e.g. for the
//! browser or as backend for the Vite dev server (`npm run dev`).

use std::io::IsTerminal;
use std::time::Duration;

use sorobo::Launch;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();
    if let Err(e) = run().await {
        eprintln!("\n  Sorobo konnte nicht gestartet werden:\n  {e:#}\n");
        // Started by double-click: keep the console window open so the message can be read.
        if std::io::stdin().is_terminal() {
            eprintln!("  Zum Beenden Enter drücken …");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let open_browser = args.iter().any(|a| a == "--open") && sorobo::env_var("NO_BROWSER").is_none();
    let dir = sorobo::data_dir();
    match sorobo::launch(sorobo::default_port(), &dir).await? {
        Launch::AlreadyRunning { url, .. } => {
            println!("\n  Sorobo läuft bereits auf {url}.\n");
            if open_browser {
                let _ = open::that(&url);
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(())
        }
        Launch::Started(server) => {
            let url = server.url.clone();
            println!("\n  Sorobo läuft auf {url}\n  Daten: {}\n", server.data_dir.display());
            if open_browser && let Err(e) = open::that(&url) {
                println!("  Browser konnte nicht geöffnet werden ({e}). Bitte {url} manuell aufrufen.\n");
            }
            server.run().await
        }
    }
}
