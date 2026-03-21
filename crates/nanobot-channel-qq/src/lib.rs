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
use nanobot_config::QQConfig;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone)]
pub struct QQChannel {
    config: QQConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QQHttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl QQChannel {
    pub fn new(config: QQConfig) -> Self {
        Self { config }
    }

    pub fn websocket_url(&self) -> Option<&str> {
        if self.config.websocket_url.trim().is_empty() {
            None
        } else {
            Some(self.config.websocket_url.as_str())
        }
    }

    pub fn receive_direct_message(
        &self,
        sender_id: &str,
        message_id: &str,
        content: &str,
    ) -> Option<InboundMessage> {
        if !self.is_allowed(sender_id) || content.trim().is_empty() {
            return None;
        }
        let mut metadata = HashMap::new();
        metadata.insert("message_id".to_string(), message_id.to_string());
        Some(InboundMessage {
            channel: self.name().to_string(),
            sender_id: sender_id.to_string(),
            chat_id: sender_id.to_string(),
            content: content.trim().to_string(),
            media: Vec::new(),
            metadata,
            session_key_override: None,
        })
    }

    pub fn parse_inbound_event(&self, raw: &str) -> Option<InboundMessage> {
        let json = serde_json::from_str::<Value>(raw).ok()?;
        let sender_id = json
            .get("author")
            .and_then(|value| value.get("id"))?
            .as_str()?;
        let message_id = json.get("id").and_then(Value::as_str)?;
        let content = json.get("content").and_then(Value::as_str)?;
        self.receive_direct_message(sender_id, message_id, content)
    }

    pub fn format_outbound(&self, msg: &OutboundMessage) -> String {
        msg.content.clone()
    }

    pub fn build_access_token_request(&self) -> QQHttpRequest {
        QQHttpRequest {
            method: "POST".to_string(),
            url: "https://bots.qq.com/app/getAppAccessToken".to_string(),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: serde_json::json!({
                "appId": self.config.app_id,
                "clientSecret": self.config.secret
            })
            .to_string(),
        }
    }

    pub fn build_send_message_request(
        &self,
        access_token: &str,
        openid: &str,
        msg: &OutboundMessage,
    ) -> QQHttpRequest {
        QQHttpRequest {
            method: "POST".to_string(),
            url: format!("https://api.sgroup.qq.com/v2/users/{openid}/messages"),
            headers: vec![
                ("Authorization".to_string(), format!("QQBot {access_token}")),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: serde_json::json!({
                "msg_type": 0,
                "content": self.format_outbound(msg),
                "msg_id": msg.metadata.get("message_id").cloned().unwrap_or_default(),
            })
            .to_string(),
        }
    }

    pub fn parse_access_token_response(&self, raw: &str) -> Option<String> {
        serde_json::from_str::<Value>(raw)
            .ok()?
            .get("access_token")?
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
            .ok_or_else(|| "missing access_token".to_string())
    }

    pub fn send_via_curl(
        &self,
        access_token: &str,
        openid: &str,
        msg: &OutboundMessage,
    ) -> Result<String, String> {
        let request = self.build_send_message_request(access_token, openid, msg);
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
                .expect("qq runtime");
            while !stop_worker.load(Ordering::SeqCst) {
                let result = runtime.block_on(run_qq_socket(
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

async fn run_qq_socket(
    channel: QQChannel,
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

impl Channel for QQChannel {
    fn name(&self) -> &'static str {
        "qq"
    }

    fn allow_from(&self) -> &[String] {
        &self.config.allow_from
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::QQChannel;
    use futures_util::SinkExt;
    use nanobot_bus::{MessageBus, OutboundMessage};
    use nanobot_config::QQConfig;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    #[test]
    fn maps_sender_to_chat_id_for_private_messages() {
        let channel = QQChannel::new(QQConfig {
            enabled: true,
            app_id: "10001".to_string(),
            secret: "secret".to_string(),
            websocket_url: String::new(),
            allow_from: vec!["user-1".to_string()],
        });

        let inbound = channel
            .receive_direct_message("user-1", "msg-1", " ping ")
            .expect("message should be accepted");

        assert_eq!(inbound.channel, "qq");
        assert_eq!(inbound.chat_id, "user-1");
        assert_eq!(
            inbound.metadata.get("message_id").map(String::as_str),
            Some("msg-1")
        );
    }

    #[test]
    fn rejects_sender_not_in_allow_list() {
        let channel = QQChannel::new(QQConfig {
            allow_from: vec!["user-1".to_string()],
            ..QQConfig::default()
        });

        assert!(
            channel
                .receive_direct_message("user-2", "msg-1", "ping")
                .is_none()
        );
    }

    #[test]
    fn builds_real_access_token_request() {
        let channel = QQChannel::new(QQConfig {
            app_id: "10001".to_string(),
            secret: "sec_1".to_string(),
            ..QQConfig::default()
        });

        let request = channel.build_access_token_request();
        assert_eq!(request.url, "https://bots.qq.com/app/getAppAccessToken");
        assert!(request.body.contains("\"appId\":\"10001\""));
        assert!(request.body.contains("\"clientSecret\":\"sec_1\""));
    }

    #[test]
    fn builds_real_send_message_request() {
        let channel = QQChannel::new(QQConfig::default());
        let request = channel.build_send_message_request(
            "token_1",
            "user-9",
            &OutboundMessage {
                channel: "qq".to_string(),
                chat_id: "user-9".to_string(),
                content: "hello".to_string(),
                reply_to: None,
                metadata: std::collections::HashMap::new(),
            },
        );

        assert_eq!(
            request.url,
            "https://api.sgroup.qq.com/v2/users/user-9/messages"
        );
        assert!(
            request
                .headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "QQBot token_1")
        );
        assert!(request.body.contains("\"content\":\"hello\""));
    }

    #[test]
    fn parses_qq_websocket_event_into_inbound_message() {
        let channel = QQChannel::new(QQConfig {
            allow_from: vec!["user-1".to_string()],
            ..QQConfig::default()
        });

        let inbound = channel
            .parse_inbound_event(
                r#"{
                    "id": "msg-1",
                    "content": "ping from qq ws",
                    "author": {
                        "id": "user-1"
                    }
                }"#,
            )
            .expect("event should parse");

        assert_eq!(inbound.channel, "qq");
        assert_eq!(inbound.chat_id, "user-1");
        assert_eq!(inbound.content, "ping from qq ws");
        assert_eq!(
            inbound.metadata.get("message_id").map(String::as_str),
            Some("msg-1")
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
                            r#"{{"id":"msg-{index}","content":"ping-{index}","author":{{"id":"user-1"}}}}"#
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
        let channel = QQChannel::new(QQConfig {
            enabled: true,
            websocket_url: format!("ws://{addr}"),
            allow_from: vec!["user-1".to_string()],
            ..QQConfig::default()
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
