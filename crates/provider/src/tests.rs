use nanobot_config::Config;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, Once, OnceLock};

use super::{
    ChatMessage, ChatRequest, DemoToolCallingProvider, HttpExecutor, HttpRequest, LlmProvider,
    OpenAiCompatibleProvider, ProviderError, ProviderKind, ProviderSelection, ReqwestExecutor,
    StaticProvider, build_provider_from_config,
};

#[derive(Debug, Default)]
struct TestLogger {
    entries: Mutex<Vec<String>>,
}

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.entries.lock().expect("lock").push(format!(
                "{} {}",
                record.level(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger {
    entries: Mutex::new(Vec::new()),
};
static LOGGER_INIT: Once = Once::new();
static LOGGER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn with_test_logger<T>(f: impl FnOnce(&Mutex<Vec<String>>) -> T) -> T {
    LOGGER_INIT.call_once(|| {
        log::set_logger(&LOGGER).expect("logger should initialize once");
        log::set_max_level(log::LevelFilter::Info);
    });

    let guard = LOGGER_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("logger lock");
    LOGGER.entries.lock().expect("lock").clear();
    let result = f(&LOGGER.entries);
    drop(guard);
    result
}

#[derive(Debug, Clone, Default)]
struct RecordingExecutor {
    response: String,
    last_request: std::sync::Arc<std::sync::Mutex<Option<HttpRequest>>>,
}

impl RecordingExecutor {
    fn with_response(response: &str) -> Self {
        Self {
            response: response.to_string(),
            last_request: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl HttpExecutor for RecordingExecutor {
    fn execute(&self, request: &HttpRequest) -> Result<String, ProviderError> {
        *self.last_request.lock().expect("lock") = Some(request.clone());
        Ok(self.response.clone())
    }
}

#[test]
fn provider_selection_always_builds_openai() {
    let provider = ProviderSelection::detect(Some("gpt-4o-mini"), Some("sk-test"), None);
    assert_eq!(provider.model, "gpt-4o-mini");
    assert_eq!(provider.kind, ProviderKind::OpenAI);
}

#[test]
fn static_provider_still_exposes_default_model() {
    let provider = StaticProvider::default();
    assert_eq!(provider.default_model(), "offline/echo");
}

#[test]
fn config_builds_provider_from_agent_defaults() {
    let config = Config::from_json_str(
        r#"{
            "providers": {
                "openai": {
                    "apiKey": "sk-test",
                    "apiBase": "https://example.com/v1"
                }
            },
            "agents": {
                "defaults": {
                    "model": "gpt-4o-mini",
                    "provider": "openai"
                }
            }
        }"#,
    )
    .expect("config should parse");

    let provider = build_provider_from_config(&config);
    assert_eq!(provider.default_model(), "gpt-4o-mini");
}

#[test]
fn openai_compatible_provider_builds_real_http_request() {
    let executor = RecordingExecutor::with_response(
        r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}"#,
    );
    let provider = OpenAiCompatibleProvider::new(
        "sk-test",
        "https://api.openai.com/v1",
        "gpt-4o-mini",
        executor.clone(),
    );

    let response = provider
        .chat(ChatRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hi".to_string(),
            }],
            tools: vec!["filesystem".to_string()],
            model: None,
        })
        .expect("provider call should succeed");

    let request = executor
        .last_request
        .lock()
        .expect("lock")
        .clone()
        .expect("request should be recorded");
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
    assert!(request.body.contains("\"model\":\"gpt-4o-mini\""));
    assert!(request.body.contains("\"filesystem\""));
    assert_eq!(response.content.as_deref(), Some("hello"));
}

#[test]
fn openai_compatible_provider_parses_tool_calls() {
    let executor = RecordingExecutor::with_response(
        r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "message",
                            "arguments": "{\"content\":\"hi\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }"#,
    );
    let provider =
        OpenAiCompatibleProvider::new("sk-test", "https://example.com/v1", "gpt-4o-mini", executor);

    let response = provider
        .chat(ChatRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hi".to_string(),
            }],
            tools: vec!["message".to_string()],
            model: None,
        })
        .expect("provider call should succeed");

    assert_eq!(response.finish_reason, "tool_calls");
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "message");
    assert_eq!(response.tool_calls[0].arguments["content"], "hi");
}

#[test]
fn openai_compatible_provider_logs_full_request_and_response() {
    with_test_logger(|entries| {
        let raw_response =
            r#"{"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}"#;
        let executor = RecordingExecutor::with_response(raw_response);
        let provider = OpenAiCompatibleProvider::new(
            "sk-test",
            "https://api.openai.com/v1",
            "gpt-4o-mini",
            executor,
        );

        provider
            .chat(ChatRequest {
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: "Hi".to_string(),
                }],
                tools: vec!["filesystem".to_string()],
                model: None,
            })
            .expect("provider call should succeed");

        let output = entries.lock().expect("lock").join("\n");
        assert!(
            output.contains("https://api.openai.com/v1/chat/completions"),
            "request url should be logged"
        );
        assert!(
            output.contains("\"model\":\"gpt-4o-mini\""),
            "request body should be logged"
        );
        assert!(
            output.contains("\"content\":\"Hi\""),
            "request message should be logged"
        );
        assert!(
            output.contains(raw_response),
            "raw response body should be logged"
        );
    });
}

#[test]
fn curl_executor_executes_http_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener.local_addr().expect("listener address");

    let server = std::thread::spawn(move || {
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

        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Length: 17\r\n",
            "Content-Type: text/plain\r\n",
            "Connection: close\r\n",
            "\r\n",
            "executor response"
        );
        stream
            .write_all(response.as_bytes())
            .expect("response should be writable");
        request
    });

    let executor = ReqwestExecutor;
    let response = executor
        .execute(&HttpRequest {
            method: "POST".to_string(),
            url: format!("http://{address}"),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: "{\"hello\":\"world\"}".to_string(),
        })
        .expect("executor should succeed");

    let request = server.join().expect("server thread should join");
    assert!(request.contains("POST / HTTP/1.1"));
    assert!(
        request.contains("content-type: application/json")
            || request.contains("Content-Type: application/json")
    );
    assert!(request.contains("{\"hello\":\"world\"}"));
    assert_eq!(response, "executor response");
}

#[test]
fn demo_tool_calling_provider_emits_tool_calls() {
    let provider = DemoToolCallingProvider;
    let response = provider
        .chat(ChatRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "please write a file".to_string(),
            }],
            tools: vec!["filesystem".to_string()],
            model: None,
        })
        .expect("provider should succeed");

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "filesystem");
}