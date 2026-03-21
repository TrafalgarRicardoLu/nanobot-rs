use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use futures_util::SinkExt;
use nanobot_app::NanobotApp;
use nanobot_provider::DemoToolCallingProvider;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::*;

fn spawn_openai_test_server(response_content: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener.local_addr().expect("listener addr");
    let response_content = response_content.to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request should connect");
        let mut buffer = Vec::new();
        loop {
            let mut chunk = [0_u8; 1024];
            let bytes_read = stream.read(&mut chunk).expect("request should be readable");
            if bytes_read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..bytes_read]);

            let request = String::from_utf8_lossy(&buffer);
            if let Some(headers_end) = request.find("\r\n\r\n") {
                let content_length = request[..headers_end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        if name.eq_ignore_ascii_case("content-length") {
                            value.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                let body_len = buffer.len() - (headers_end + 4);
                if body_len >= content_length {
                    break;
                }
            }
        }

        let request = String::from_utf8_lossy(&buffer).to_string();
        let response_body = format!(
            r#"{{"choices":[{{"message":{{"content":"{}"}},"finish_reason":"stop"}}]}}"#,
            response_content
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .expect("response should be writable");
        request
    });

    (format!("http://{addr}/v1"), server)
}

#[test]
fn interactive_session_stops_on_exit_command() {
    let dir = std::env::temp_dir().join(format!("nanobot-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir should exist");
    let mut app = NanobotApp::new(Config::default(), Box::new(DemoToolCallingProvider), &dir)
        .expect("app should build");
    let input = Cursor::new("write a file\nexit\n");
    let mut output = Vec::new();

    run_interactive_session(&mut app, input, &mut output).expect("interactive session should run");

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
    let dir = std::env::temp_dir().join(format!("nanobot-cli-serve-live-{}", std::process::id()));
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
    let (api_base, provider_server) = spawn_openai_test_server("echo: hello from serve");
    let config = Config::from_json_str(&format!(
        r#"{{
            "providers": {{
                "openai": {{
                    "apiKey": "sk-test",
                    "apiBase": "{api_base}"
                }}
            }},
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
    provider_server.join().expect("provider server should join");
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

    let (api_base, provider_server) = spawn_openai_test_server("echo: gateway:18790:verbose");
    let config = Config::from_json_str(&format!(
        r#"{{
            "providers": {{
                "openai": {{
                    "apiKey": "sk-test",
                    "apiBase": "{api_base}"
                }}
            }},
            "agents": {{
                "defaults": {{
                    "model": "gpt-4o-mini",
                    "provider": "openai"
                }}
            }}
        }}"#
    ))
    .expect("config should parse");

    run_gateway_mode(config, dir.clone(), 18790, true, 2, 0).expect("gateway mode should run");
    provider_server.join().expect("provider server should join");

    let session = std::fs::read_to_string(dir.join("sessions").join("system_gateway.jsonl"))
        .expect("gateway session should exist");
    assert!(session.contains("gateway:18790:verbose"));
}
