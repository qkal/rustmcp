use std::{env, path::PathBuf};

use rust_analyzer_mcp::server::RaMcpServer;
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

struct StderrWriter;

impl<'a> MakeWriter<'a> for StderrWriter {
    type Writer = std::io::Stderr;

    fn make_writer(&'a self) -> Self::Writer {
        std::io::stderr()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let workspace = match parse_args(env::args().skip(1)) {
        Ok(Command::Serve { workspace }) => workspace,
        Ok(Command::Help) => {
            print_help();
            return Ok(());
        }
        Ok(Command::Version) => {
            eprintln!("rust-analyzer-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Err(message) => {
            eprintln!("error: {message}");
            print_help();
            std::process::exit(2);
        }
    };

    let server = RaMcpServer::new(workspace.unwrap_or(env::current_dir()?))?;
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

enum Command {
    Serve { workspace: Option<PathBuf> },
    Help,
    Version,
}

fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut workspace = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--version" | "-V" => return Ok(Command::Version),
            "--workspace" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--workspace requires a path".to_string())?;
                workspace = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    Ok(Command::Serve { workspace })
}

fn print_help() {
    eprintln!("rust-analyzer-mcp");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  rust-analyzer-mcp [--workspace <path>]");
    eprintln!();
    eprintln!("All MCP protocol messages are written to stdout. Human output is stderr only.");
}
