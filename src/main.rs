use std::{env, path::PathBuf};

use clap::Parser;
use rust_analyzer_mcp::server::{RaMcpServer, ServerConfig};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

struct StderrWriter;

impl<'a> MakeWriter<'a> for StderrWriter {
    type Writer = std::io::Stderr;

    fn make_writer(&'a self) -> Self::Writer {
        std::io::stderr()
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "rust-analyzer-mcp",
    version,
    about = "A stdio MCP server that exposes rust-analyzer intelligence to coding agents.",
    long_about = "Provides read-only Rust IDE features (via rust-analyzer) and controlled Cargo tools to MCP-compatible AI coding agents.\n\nAll MCP protocol messages go to stdout. Human-readable output goes to stderr."
)]
struct Cli {
    /// Set the Rust workspace root (defaults to current working directory)
    #[arg(long, value_name = "PATH")]
    workspace: Option<PathBuf>,

    /// Disable all cargo_* tools (they will return structured \"disabled\" responses)
    #[arg(long)]
    disable_cargo_tools: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let cli = Cli::parse();

    let config = ServerConfig {
        cargo_tools_enabled: !cli.disable_cargo_tools,
    };

    let workspace = cli
        .workspace
        .unwrap_or_else(|| env::current_dir().expect("failed to get current directory"));

    let server = RaMcpServer::with_config(workspace, config)?;
    let running = rmcp::serve_server(server, rmcp::transport::stdio()).await?;
    let _reason = running.waiting().await?;
    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(StderrWriter)
        .with_ansi(false)
        .try_init();
}
