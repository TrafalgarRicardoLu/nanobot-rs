# nanobot-rs

`nanobot-rs` is a standalone Rust workspace extracted from the Rust rewrite of nanobot.

It currently includes:

- A Cargo workspace with the core app, CLI, config, provider, session, tools, cron, heartbeat, and channel crates
- A runnable CLI binary in `crates/nanobot-cli`
- Feishu and QQ channel implementations
- The existing WhatsApp bridge assets under `bridge/`

## Workspace Layout

```text
.
├── Cargo.toml
├── crates/
│   ├── nanobot-app
│   ├── nanobot-bus
│   ├── nanobot-channel-feishu
│   ├── nanobot-channel-qq
│   ├── nanobot-channels
│   ├── nanobot-cli
│   ├── nanobot-config
│   ├── nanobot-core
│   ├── nanobot-cron
│   ├── nanobot-heartbeat
│   ├── nanobot-provider
│   ├── nanobot-session
│   └── nanobot-tools
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
