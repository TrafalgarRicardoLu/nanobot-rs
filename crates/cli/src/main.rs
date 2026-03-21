use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use nanobot_app::NanobotApp;
use nanobot_config::Config;

#[derive(Debug, Parser)]
#[clap(name = "nanobot", version)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Onboard {
        #[clap(long, default_value = ".nanobot-rs/config.json")]
        config: PathBuf,
    },
    Status {
        #[clap(long, default_value = ".nanobot-rs/config.json")]
        config: PathBuf,
        #[clap(long, default_value = ".nanobot-rs/workspace")]
        workspace: PathBuf,
    },
    Agent {
        #[clap(short = 'm', long)]
        message: Option<String>,
        #[clap(long, default_value = ".nanobot-rs/config.json")]
        config: PathBuf,
        #[clap(long, default_value = ".nanobot-rs/workspace")]
        workspace: PathBuf,
    },
    Serve {
        #[clap(long, default_value = ".nanobot-rs/config.json")]
        config: PathBuf,
        #[clap(long, default_value = ".nanobot-rs/workspace")]
        workspace: PathBuf,
        #[clap(long, default_value_t = 5)]
        max_iterations: usize,
        #[clap(long, default_value_t = 10)]
        interval_ms: u64,
    },
    Gateway {
        #[clap(long, short = 'p', default_value_t = 18790)]
        port: u16,
        #[clap(long, short = 'v')]
        verbose: bool,
        #[clap(long, default_value = ".nanobot-rs/config.json")]
        config: PathBuf,
        #[clap(long, default_value = ".nanobot-rs/workspace")]
        workspace: PathBuf,
        #[clap(long, default_value_t = 5)]
        max_iterations: usize,
        #[clap(long, default_value_t = 10)]
        interval_ms: u64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Onboard { config } => {
            if let Some(parent) = config.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(config, serde_json::to_string_pretty(&Config::default())?)?;
        }
        Command::Status { config, workspace } => {
            fs::create_dir_all(&workspace)?;
            let config = load_or_default_config(&config)?;
            let app = NanobotApp::from_config(config, &workspace)?;
            println!("{}", app.status_summary());
        }
        Command::Agent {
            message,
            config,
            workspace,
        } => {
            fs::create_dir_all(&workspace)?;
            let config = load_or_default_config(&config)?;
            let mut app = NanobotApp::from_config(config, &workspace)?;
            if let Some(message) = message {
                if let Some(response) = app.handle_cli_message("cli:local", &message)? {
                    println!("{response}");
                }
                for dispatch in app.dispatch_outbound_once()? {
                    println!(
                        "[dispatch:{}:{}] {}",
                        dispatch.channel, dispatch.chat_id, dispatch.rendered
                    );
                }
            } else {
                let stdin = std::io::stdin();
                let stdout = std::io::stdout();
                run_interactive_session(&mut app, stdin.lock(), stdout.lock())?;
            }
        }
        Command::Serve {
            config,
            workspace,
            max_iterations,
            interval_ms,
        } => {
            fs::create_dir_all(&workspace)?;
            let config = load_or_default_config(&config)?;
            run_service_mode(config, workspace, max_iterations, interval_ms)?;
        }
        Command::Gateway {
            port,
            verbose,
            config,
            workspace,
            max_iterations,
            interval_ms,
        } => {
            fs::create_dir_all(&workspace)?;
            let config = load_or_default_config(&config)?;
            run_gateway_mode(
                config,
                workspace,
                port,
                verbose,
                max_iterations,
                interval_ms,
            )?;
        }
    }
    Ok(())
}

fn load_or_default_config(path: &PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
    if path.exists() {
        Ok(Config::from_json_file(path)?)
    } else {
        Ok(Config::default())
    }
}

fn run_interactive_session(
    app: &mut NanobotApp,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new();
    loop {
        output.write_all(b"You> ")?;
        output.flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            output.write_all(b"Bye\n")?;
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, "exit" | "quit" | "/exit" | "/quit") {
            output.write_all(b"Bye\n")?;
            break;
        }
        if let Some(response) = app.handle_cli_message("cli:interactive", trimmed)? {
            writeln!(output, "Bot> {response}")?;
        }
        for dispatch in app.dispatch_outbound_once()? {
            writeln!(
                output,
                "Dispatch> [{}:{}] {}",
                dispatch.channel, dispatch.chat_id, dispatch.rendered
            )?;
        }
    }
    Ok(())
}

fn run_service_mode(
    config: Config,
    workspace: PathBuf,
    max_iterations: usize,
    interval_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    run_gateway_loop(config, workspace, max_iterations, interval_ms, None)
}

fn run_gateway_mode(
    config: Config,
    workspace: PathBuf,
    port: u16,
    verbose: bool,
    max_iterations: usize,
    interval_ms: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode = if verbose {
        format!("gateway:{port}:verbose")
    } else {
        format!("gateway:{port}")
    };
    run_gateway_loop(config, workspace, max_iterations, interval_ms, Some(mode))
}

fn run_gateway_loop(
    config: Config,
    workspace: PathBuf,
    max_iterations: usize,
    interval_ms: u64,
    session_seed: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&workspace)?;
    let app = NanobotApp::from_config(config, &workspace)?;
    let shared = app.into_shared();
    if let Some(seed) = session_seed {
        let mut app = shared.lock().expect("service app lock");
        let _ = app.handle_cli_message("system:gateway", &seed)?;
    }
    let channel_handles = {
        let app = shared.lock().expect("service app lock");
        app.start_channel_runtimes()
    };
    let background =
        NanobotApp::spawn_background_worker(shared.clone(), 0, 1, interval_ms, max_iterations);
    for _ in 0..max_iterations {
        {
            let mut app = shared.lock().expect("service app lock");
            let _ = app.process_inbound_once()?;
        }
        if interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
    }
    background.stop();
    let _ = background.join();
    for handle in channel_handles {
        handle.stop();
        let _ = handle.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::thread;

    use futures_util::SinkExt;
    use nanobot_app::NanobotApp;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    use super::*;

    #[test]
    fn interactive_session_stops_on_exit_command() {
        let dir = std::env::temp_dir().join(format!("nanobot-cli-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should exist");
        let config = Config::from_json_str(
            r#"{
                "agents": {
                    "defaults": {
                        "model": "offline/tool-calling-demo",
                        "provider": "demo_tool_calling"
                    }
                }
            }"#,
        )
        .expect("config should parse");
        let mut app = NanobotApp::from_config(config, &dir).expect("app should build");
        let input = Cursor::new("write a file\nexit\n");
        let mut output = Vec::new();

        run_interactive_session(&mut app, input, &mut output)
            .expect("interactive session should run");

        let rendered = String::from_utf8(output).expect("utf8");
        assert!(rendered.contains("demo complete"));
        assert!(rendered.contains("Bye"));
    }

    #[test]
    fn serve_mode_runs_with_finite_iterations() {
        let dir = std::env::temp_dir().join(format!("nanobot-cli-serve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should exist");

        run_service_mode(Config::default(), dir, 2, 0).expect("serve mode should run");
    }

    #[test]
    fn serve_mode_starts_feishu_runtime_and_persists_inbound_session() {
        let dir =
            std::env::temp_dir().join(format!("nanobot-cli-serve-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should exist");
        let (addr_tx, addr_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("listener");
                addr_tx
                    .send(listener.local_addr().expect("local addr"))
                    .expect("send addr");
                let (stream, _) = listener.accept().await.expect("accept");
                let mut ws = accept_async(stream).await.expect("accept websocket");
                ws.send(Message::Text(
                    r#"{"event":{"sender":{"sender_id":{"open_id":"ou_1"}},"message":{"message_id":"om_cli","chat_id":"oc_cli","content":"{\"text\":\"hello from serve\"}"}}}"#
                        .to_string()
                        .into(),
                ))
                .await
                .expect("send frame");
                ws.close(None).await.expect("close");
            });
        });
        let addr = addr_rx.recv().expect("recv addr");
        let config = Config::from_json_str(&format!(
            r#"{{
                "channels": {{
                    "feishu": {{
                        "enabled": true,
                        "appId": "cli_a",
                        "appSecret": "secret_a",
                        "websocketUrl": "ws://{addr}",
                        "allowFrom": ["ou_1"]
                    }}
                }}
            }}"#
        ))
        .expect("config should parse");

        run_service_mode(config, dir.clone(), 20, 10).expect("serve mode should run");

        server.join().expect("server should join");
        let session = std::fs::read_to_string(dir.join("sessions").join("feishu_oc_cli.jsonl"))
            .expect("session file should exist");
        assert!(session.contains("hello from serve"));
        assert!(session.contains("echo: hello from serve"));
    }

    #[test]
    fn gateway_mode_runs_with_original_command_shape() {
        let dir = std::env::temp_dir().join(format!("nanobot-cli-gateway-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir should exist");

        run_gateway_mode(Config::default(), dir.clone(), 18790, true, 2, 0)
            .expect("gateway mode should run");

        let session = std::fs::read_to_string(dir.join("sessions").join("system_gateway.jsonl"))
            .expect("gateway session should exist");
        assert!(session.contains("gateway:18790:verbose"));
    }
}
