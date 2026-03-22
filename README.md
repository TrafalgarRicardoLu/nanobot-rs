# nanobot-rs

`nanobot-rs` is a standalone Rust workspace extracted from the Rust rewrite of nanobot.

It currently includes:

- A Cargo workspace with the core app, CLI, config, provider, session, tools, cron, heartbeat, and generic channel runtime crates
- A runnable CLI binary in `crates/cli`
- A generic channel abstraction in `crates/channels` that host applications can wire to concrete transports
- The existing WhatsApp bridge assets under `bridge/`

## Workspace Layout

```text
.
├── Cargo.toml
├── crates/
│   ├── app
│   ├── bus
│   ├── channels
│   ├── cli
│   ├── config
│   ├── core
│   ├── cron
│   ├── heartbeat
│   ├── provider
│   ├── session
│   └── tools
└── bridge/
```

## Requirements

- Rust stable toolchain
- Cargo
- `curl` available in `PATH` for the current provider/channel HTTP helpers
- Node.js only if you need the optional `bridge/` assets

## Build

```bash
cargo build
```

## Test

```bash
cargo test
```

## Run

Initialize a local config:

```bash
cargo run -p nanobot-cli -- onboard
```

Show status:

```bash
cargo run -p nanobot-cli -- status
```

Send a one-off message:

```bash
cargo run -p nanobot-cli -- agent -m "hello"
```

Start the service loop:

```bash
cargo run -p nanobot-cli -- serve
```

## Notes

- This repository is intentionally Rust-only. The original Python package, Python tests, and packaging files were not carried over.
- The current implementation is the extracted Rust rewrite as it exists today, not a full feature-parity port of the original nanobot project.
- Concrete inbound and outbound channels are no longer bundled in this workspace. Integrators are expected to provide channel implementations against the shared runtime contract.
- A built-in Telegram Bot channel is available via long polling for private text chats.

## Telegram Channel

Configure a Telegram Bot channel in `.nanobot-rs/config.json`:

```json
{
  "channels": [
    {
      "kind": "telegram",
      "enabled": true,
      "allowFrom": ["123456789"],
      "settings": {
        "botToken": "123456:telegram-bot-token",
        "apiBase": "https://api.telegram.org",
        "pollTimeoutSeconds": 20,
        "dropPendingUpdatesOnStart": true
      }
    }
  ]
}
```

Current scope:

- Long polling only
- Private chats only
- Text message receive/send only
- `allowFrom` must contain Telegram numeric `user_id` values
