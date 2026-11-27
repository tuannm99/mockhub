use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::middleware::from_fn_with_state;
use clap::{Parser, Subcommand};
use mockhub_core::config::Config;
use mockhub_dns::Engine;
use mockhub_web::api::Ports;
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
    /// REST `/v1/...` và DoH JSON `/resolve`
    Api,
}

fn main() -> anyhow::Result<()> {
    // Phải đặt env trước khi runtime tạo thread (set_var không an toàn khi đa luồng).
    let public_dir = resolve_public_dir();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(public_dir))
}

/// Thư mục client đã build (output `dx build`) cho SSR/hydrate. Thứ tự: `DIOXUS_PUBLIC_PATH`,
/// `<thư mục exe>/public` (Docker/bản dx), rồi output `dx build` trong `target/dx` (chạy từ repo).
/// Tìm được thì export vào `DIOXUS_PUBLIC_PATH` để dioxus dùng.
fn resolve_public_dir() -> Option<PathBuf> {
    let exe_public = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("public")));
    let candidates = std::env::var_os("DIOXUS_PUBLIC_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain(exe_public)
        .chain(
            ["release", "debug"]
                .map(|p| PathBuf::from(format!("target/dx/mockhub-web/{p}/web/public"))),
        );
    let dir = candidates.into_iter().find(|d| d.is_dir())?;
    let dir = dir.canonicalize().unwrap_or(dir);
    // SAFETY: gọi từ `main` trước khi tạo bất kỳ thread nào.
    unsafe { std::env::set_var("DIOXUS_PUBLIC_PATH", &dir) };
    Some(dir)
}

async fn run(public_dir: Option<PathBuf>) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(cli.config.as_deref())?;
    let cmd = cli.command.unwrap_or(Cmd::All);
    let all = matches!(cmd, Cmd::All);

    // Mở storage một lần, mọi service dùng chung. Backend chọn theo scheme của `database.url`;
    // phần còn lại của app chỉ thấy `dyn Storage`.
    let storage = mockhub_storage::connect(&cfg.database.url).await?;

    // DNS, DoH và trang "tra thử" dùng chung engine nên trả cùng kết quả.
    let engine = Engine::new(storage.clone()).with_upstream(cfg.dns.upstream.as_deref());

    let mut tasks: JoinSet<anyhow::Result<()>> = JoinSet::new();

    if all || matches!(cmd, Cmd::Web) {
        match public_dir {
            Some(dir) => {
                tracing::info!("web assets: {}", dir.display());
                tasks.spawn(web(cfg.clone(), engine.clone()));
            }
            // `all` vẫn chạy mail/dns/sms khi chưa build client; `web` thì báo lỗi rõ ràng.
            None if all => tracing::warn!("bỏ qua web: không thấy client build. {WEB_HINT}"),
            None => anyhow::bail!("không thấy client build. {WEB_HINT}"),
        }
    }
    if (all && cfg.mail.enabled) || matches!(cmd, Cmd::Mail) {
        let (cfg, storage) = (cfg.clone(), storage.clone());
        tasks.spawn(async move {
            let token = cfg.auth.token();
            Ok(mockhub_mail::run(&cfg.server.host, &cfg.mail, storage, token).await?)
        });
    }
    if (all && cfg.dns.enabled) || matches!(cmd, Cmd::Dns) {
        let (cfg, storage) = (cfg.clone(), storage.clone());
        tasks
            .spawn(async move { Ok(mockhub_dns::run(&cfg.server.host, &cfg.dns, storage).await?) });
    }
    if (all && cfg.api.enabled) || matches!(cmd, Cmd::Api) {
        let (cfg, engine) = (cfg.clone(), engine.clone());
        tasks.spawn(async move {
            let token = cfg.auth.token();
            Ok(mockhub_api::run(&cfg.server.host, &cfg.api, &cfg.mail, engine, token).await?)
        });
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

const WEB_HINT: &str = "Chạy `cd crates/mockhub-web && dx build --release --platform web` \
    hoặc đặt DIOXUS_PUBLIC_PATH tới thư mục public đã build.";

async fn web(cfg: Config, engine: Engine) -> anyhow::Result<()> {
    mockhub_web::server::init(engine, ports(&cfg));

    let addr: SocketAddr = format!("{}:{}", cfg.server.host, cfg.server.port).parse()?;
    // SSR + hydrate: dioxus tự mount static assets (đọc `DIOXUS_PUBLIC_PATH`, mặc định
    // `<thư mục exe>/public`), render HTML phía server và đăng ký các server function (`/api/...`).
    let mut app: axum::Router = dioxus::server::router(mockhub_web::app::App);
    if let Some(token) = cfg.auth.token() {
        // Bọc cả router: trang, server function lẫn file tĩnh.
        app = app.layer(from_fn_with_state(
            Arc::<str>::from(token),
            mockhub_api::auth::require_basic,
        ));
    }

    tracing::info!("web listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Cổng đang cấu hình, để UI hướng dẫn nối client. `0` = tắt.
fn ports(cfg: &Config) -> Ports {
    let on = |enabled: bool, port: u16| if enabled { port } else { 0 };
    Ports {
        smtp: on(cfg.mail.enabled, cfg.mail.smtp_port),
        imap: on(cfg.mail.enabled, cfg.mail.imap_port),
        pop3: on(cfg.mail.enabled, cfg.mail.pop3_port),
        dns: on(cfg.dns.enabled, cfg.dns.port),
        api: on(cfg.api.enabled, cfg.api.port),
        sms: on(cfg.sms.enabled, cfg.sms.port),
    }
}
