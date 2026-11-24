use std::net::SocketAddr;

use axum::Router;
use clap::{Parser, Subcommand};
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use mockhub_core::config::Config;
use mockhub_web::app::{shell, App};
use tokio::task::JoinSet;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "mockhub", about = "Mock mail / DNS / SMS hub")]
struct Cli {
    /// Đường dẫn file config (mặc định: $MOCKHUB_CONFIG hoặc config/default.toml)
    #[arg(short, long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand, Clone, Copy)]
enum Cmd {
    /// Chạy tất cả service đang `enabled` (mặc định)
    All,
    Web,
    Mail,
    Dns,
    Sms,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(cli.config.as_deref())?;
    let cmd = cli.command.unwrap_or(Cmd::All);
    let all = matches!(cmd, Cmd::All);

    let mut tasks: JoinSet<anyhow::Result<()>> = JoinSet::new();

    if all || matches!(cmd, Cmd::Web) {
        tasks.spawn(web(cfg.clone()));
    }
    if (all && cfg.mail.enabled) || matches!(cmd, Cmd::Mail) {
        let cfg = cfg.clone();
        tasks.spawn(async move { Ok(mockhub_mail::run(&cfg.server.host, &cfg.mail).await?) });
    }
    if (all && cfg.dns.enabled) || matches!(cmd, Cmd::Dns) {
        let cfg = cfg.clone();
        tasks.spawn(async move { Ok(mockhub_dns::run(&cfg.server.host, &cfg.dns).await?) });
    }
    if (all && cfg.sms.enabled) || matches!(cmd, Cmd::Sms) {
        let cfg = cfg.clone();
        tasks.spawn(async move { Ok(mockhub_sms::run(&cfg.server.host, &cfg.sms).await?) });
    }

    // Một service chết thì dừng cả process (để container restart).
    tokio::select! {
        res = tasks.join_next() => {
            if let Some(res) = res { res??; }
        }
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }
    Ok(())
}

async fn web(cfg: Config) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", cfg.server.host, cfg.server.port).parse()?;
    let options = LeptosOptions::builder()
        .output_name("mockhub")
        .site_addr(addr)
        .build();
    let routes = generate_route_list(App);

    let app = Router::new()
        .leptos_routes(&options, routes, {
            let options = options.clone();
            move || shell(options.clone())
        })
        .with_state(options);

    tracing::info!("web listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
