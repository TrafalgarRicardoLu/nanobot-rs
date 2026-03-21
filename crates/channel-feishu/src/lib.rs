use std::collections::HashMap;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use futures_util::StreamExt;
use nanobot_bus::{InboundMessage, OutboundMessage};
use nanobot_channels::{Channel, ChannelRuntimeHandle};
use nanobot_config::FeishuConfig;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone)]
pub struct FeishuChannel {
    config: FeishuConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuHttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl FeishuChannel {
    pub fn new(config: FeishuConfig) -> Self {
        Self { config }
    }

    pub fn react_emoji(&self) -> &str {
        &self.config.react_emoji
    }

    pub fn websocket_url(&self) -> Option<&str> {
        if self.config.websocket_url.trim().is_empty() {
            None
        } else {
            Some(self.config.websocket_url.as_str())
        }
    }

    pub fn receive_text(
        &self,
        sender_id: &str,
        chat_id: &str,
        content: &str,
        metadata: HashMap<String, String>,
    ) -> Option<InboundMessage> {
        if !self.is_allowed(sender_id) || content.trim().is_empty() {
            return None;
        }
        Some(InboundMessage {
            channel: self.name().to_string(),
            sender_id: sender_id.to_string(),
            chat_id: chat_id.to_string(),
            content: content.trim().to_string(),
            media: Vec::new(),
            metadata,
            session_key_override: None,
        })
    }

    pub fn parse_inbound_event(&self, raw: &str) -> Option<InboundMessage> {
        let json = serde_json::from_str::<Value>(raw).ok()?;
        let sender_id = json
            .pointer("/event/sender/sender_id/open_id")
            .and_then(Value::as_str)?;
        let chat_id = json
            .pointer("/event/message/chat_id")
            .and_then(Value::as_str)?;
        let content = json
            .pointer("/event/message/content")
            .and_then(Value::as_str)?;
        let content_json = serde_json::from_str::<Value>(content).ok()?;
        let text = content_json.get("text")?.as_str()?;
        let mut metadata = HashMap::new();
        if let Some(message_id) = json
            .pointer("/event/message/message_id")
            .and_then(Value::as_str)
        {
            metadata.insert("message_id".to_string(), message_id.to_string());
        }
        self.receive_text(sender_id, chat_id, text, metadata)
    }

    pub fn format_outbound(&self, msg: &OutboundMessage) -> String {
        msg.content.clone()
    }

    pub fn build_access_token_request(&self) -> FeishuHttpRequest {
        FeishuHttpRequest {
            method: "POST".to_string(),
            url: "https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal".to_string(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret
            })
            .to_string(),
        }
    }

    pub fn build_send_message_request(
        &self,
        access_token: &str,
        receive_id: &str,
        msg: &OutboundMessage,
    ) -> FeishuHttpRequest {
        FeishuHttpRequest {
            method: "POST".to_string(),
            url: "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id"
                .to_string(),
            headers: vec![
                (
                    "Authorization".to_string(),
                    format!("Bearer {access_token}"),
                ),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: serde_json::json!({
                "receive_id": receive_id,
                "msg_type": "text",
                "content": serde_json::json!({"text": self.format_outbound(msg)}).to_string(),
            })
            .to_string(),
        }
    }

    pub fn parse_access_token_response(&self, raw: &str) -> Option<String> {
        serde_json::from_str::<Value>(raw)
            .ok()?
            .get("app_access_token")?
            .as_str()
            .map(ToString::to_string)
    }

    pub fn fetch_access_token_via_curl(&self) -> Result<String, String> {
        let request = self.build_access_token_request();
        let mut command = Command::new("curl");
        command.arg("--silent").arg("--show-error");
        command.arg("-X").arg(&request.method);
        for (name, value) in &request.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command.arg("--data").arg(&request.body).arg(&request.url);
        let output = command.output().map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        self.parse_access_token_response(&raw)
            .ok_or_else(|| "missing app_access_token".to_string())
    }

    pub fn send_via_curl(
        &self,
        access_token: &str,
        receive_id: &str,
        msg: &OutboundMessage,
    ) -> Result<String, String> {
        let request = self.build_send_message_request(access_token, receive_id, msg);
        let mut command = Command::new("curl");
        command.arg("--silent").arg("--show-error");
        command.arg("-X").arg(&request.method);
        for (name, value) in &request.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command.arg("--data").arg(&request.body).arg(&request.url);
        let output = command.output().map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    pub fn spawn_inbound_runtime(
        &self,
        inbound_tx: UnboundedSender<InboundMessage>,
    ) -> Option<ChannelRuntimeHandle> {
        let websocket_url = self.websocket_url()?.to_string();
        let channel = self.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let join = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("feishu runtime");
            while !stop_worker.load(Ordering::SeqCst) {
                let result = runtime.block_on(run_feishu_socket(
                    channel.clone(),
                    websocket_url.clone(),
                    inbound_tx.clone(),
                    stop_worker.clone(),
                ));
                if stop_worker.load(Ordering::SeqCst) {
                    break;
                }
                if result.is_err() {
                    thread::sleep(Duration::from_millis(200));
                }
            }
        });
        Some(ChannelRuntimeHandle::new(self.name(), stop, join))
    }
}

async fn run_feishu_socket(
    channel: FeishuChannel,
    websocket_url: String,
    inbound_tx: UnboundedSender<InboundMessage>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let (stream, _) = connect_async(&websocket_url)
        .await
        .map_err(|error| error.to_string())?;
    let (_, mut read) = stream.split();
    while let Some(frame) = read.next().await {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match frame.map_err(|error| error.to_string())? {
            Message::Text(text) => {
                if let Some(message) = channel.parse_inbound_event(&text) {
                    let _ = inbound_tx.send(message);
                }
            }
            Message::Binary(bytes) => {
                if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                    if let Some(message) = channel.parse_inbound_event(&text) {
                        let _ = inbound_tx.send(message);
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    fn allow_from(&self) -> &[String] {
        &self.config.allow_from
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::FeishuChannel;
    use futures_util::SinkExt;
    use nanobot_bus::{MessageBus, OutboundMessage};
    use nanobot_config::FeishuConfig;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    #[test]
    fn builds_inbound_message_for_allowed_sender() {
        let channel = FeishuChannel::new(FeishuConfig {
            enabled: true,
            app_id: "cli_1".to_string(),
            app_secret: "secret".to_string(),
            encrypt_key: String::new(),
            verification_token: String::new(),
            websocket_url: String::new(),
            allow_from: vec!["ou_1".to_string()],
            react_emoji: "DONE".to_string(),
        });

        let message = channel
            .receive_text("ou_1", "oc_1", " hello ", HashMap::new())
            .expect("message should be accepted");

        assert_eq!(message.channel, "feishu");
        assert_eq!(message.content, "hello");
        assert_eq!(channel.react_emoji(), "DONE");
    }

    #[test]
    fn rejects_sender_not_in_allow_list() {
        let channel = FeishuChannel::new(FeishuConfig {
            allow_from: vec!["ou_1".to_string()],
            ..FeishuConfig::default()
        });

        assert!(
            channel
                .receive_text("ou_2", "oc_1", "hello", HashMap::new())
                .is_none()
        );
    }

    #[test]
    fn builds_real_access_token_request() {
        let channel = FeishuChannel::new(FeishuConfig {
            app_id: "cli_123".to_string(),
            app_secret: "secret_456".to_string(),
            ..FeishuConfig::default()
        });

        let request = channel.build_access_token_request();
        assert_eq!(
            request.url,
            "https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal"
        );
        assert!(request.body.contains("\"app_id\":\"cli_123\""));
        assert!(request.body.contains("\"app_secret\":\"secret_456\""));
    }

    #[test]
    fn builds_real_send_message_request() {
        let channel = FeishuChannel::new(FeishuConfig::default());
        let request = channel.build_send_message_request(
            "token_1",
            "oc_1",
            &OutboundMessage {
                channel: "feishu".to_string(),
                chat_id: "oc_1".to_string(),
                content: "hello".to_string(),
                reply_to: None,
                metadata: HashMap::new(),
            },
        );

        assert!(request.url.contains("/open-apis/im/v1/messages"));
        assert!(
            request
                .headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer token_1")
        );
        assert!(request.body.contains("\"receive_id\":\"oc_1\""));
        assert!(request.body.contains("\\\"text\\\":\\\"hello\\\""));
    }

    #[test]
    fn parses_feishu_websocket_event_into_inbound_message() {
        let channel = FeishuChannel::new(FeishuConfig {
            allow_from: vec!["ou_1".to_string()],
            ..FeishuConfig::default()
        });

        let inbound = channel
            .parse_inbound_event(
                r#"{
                    "event": {
                        "sender": {
                            "sender_id": {
                                "open_id": "ou_1"
                            }
                        },
                        "message": {
                            "message_id": "om_x",
                            "chat_id": "oc_1",
                            "content": "{\"text\":\"ping from ws\"}"
                        }
                    }
                }"#,
            )
            .expect("event should parse");

        assert_eq!(inbound.channel, "feishu");
        assert_eq!(inbound.chat_id, "oc_1");
        assert_eq!(inbound.content, "ping from ws");
        assert_eq!(
            inbound.metadata.get("message_id").map(String::as_str),
            Some("om_x")
        );
    }

    #[test]
    fn inbound_runtime_receives_messages_over_websocket_and_reconnects() {
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
                for index in 0..2 {
                    let (stream, _) = listener.accept().await.expect("accept");
                    let mut ws = accept_async(stream)
                        .await
                        .expect("accept websocket");
                    ws.send(Message::Text(
                        format!(
                            r#"{{"event":{{"sender":{{"sender_id":{{"open_id":"ou_1"}}}},"message":{{"message_id":"om_{index}","chat_id":"oc_1","content":"{{\"text\":\"ping-{index}\"}}"}}}}}}"#
                        )
                        .into(),
                    ))
                    .await
                    .expect("send frame");
                    ws.close(None).await.expect("close");
                }
            });
        });
        let addr = addr_rx.recv().expect("recv addr");
        let channel = FeishuChannel::new(FeishuConfig {
            enabled: true,
            websocket_url: format!("ws://{addr}"),
            allow_from: vec!["ou_1".to_string()],
            ..FeishuConfig::default()
        });
        let mut bus = MessageBus::new();
        let handle = channel
            .spawn_inbound_runtime(bus.inbound_publisher())
            .expect("runtime should start");

        let mut received = Vec::new();
        for _ in 0..50 {
            if let Some(message) = bus.try_consume_inbound() {
                received.push(message.content);
                if received.len() == 2 {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(20));
            }
        }

        handle.stop();
        handle.join().expect("runtime should join");
        server.join().expect("server should join");
        assert_eq!(received, vec!["ping-0".to_string(), "ping-1".to_string()]);
    }
}
