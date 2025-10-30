# qb-port-sync

qBittorrent listens on a single port for incoming peers. ProtonVPN assigns a dynamic forwarded port when you connect to a P2P server. `qb-port-sync` keeps the two in sync so that qBittorrent stays reachable whether you are using ProtonVPN's port forwarding file on Linux or PCP/NAT-PMP discovery on macOS and other environments.

## Features
- Monitors ProtonVPN's forwarded port file on Linux and updates qBittorrent immediately when it changes.
- Falls back to PCP, then NAT-PMP, to negotiate a forwarded port when file watching is unavailable (macOS or ProtonVPN CLI fallback).
- Hardened qBittorrent Web API client with cookie authentication and preference verification.
- Configurable logging, environment overrides, and retry/backoff handling.
- Ready-to-use `systemd` unit and `launchd` service definitions.
- GitHub Actions workflow, sample config, and smoke tests included.

## Requirements
- Rust 1.74+ (stable toolchain pinned via `rust-toolchain.toml`).
- qBittorrent with Web UI enabled (`Preferences ▸ Web UI`).
- ProtonVPN CLI with port forwarding support (Plus/Unlimited with P2P servers).
- PCP or NAT-PMP capable router for fallback mode.

## Quick start
```bash
cargo build --release
# optional: run once to verify credentials and strategy
./target/release/qb-port-sync --once
```

### Configuration
Copy the example config into the appropriate location and edit it:

```bash
install -d /etc/qb-port-sync
cp config/config.example.toml /etc/qb-port-sync/config.toml
```

Key settings:
- `[qbittorrent]` – `base_url`, `username`, and `password`. Leave `password` blank to use the `QB_PORT_SYNC_QB_PASSWORD` environment variable.
- `[protonvpn]` – override the forwarded port file path. On Linux the daemon defaults to `/run/user/$UID/Proton/VPN/forwarded_port`.
- `[portmap]` – controls PCP/NAT-PMP behaviour. Set `internal_port = 0` to allow the gateway to choose; otherwise the daemon requests the port you specify.

When you don't pass `--config`, the daemon searches:
1. `--config <path>` (if provided)
2. `$XDG_CONFIG_HOME/qb-port-sync/config.toml` (Linux)
3. `/Library/Application Support/qb-port-sync/config.toml` (macOS)
4. `/etc/qb-port-sync/config.toml` (Linux fallback)

### Credentials
Add secrets to `/etc/default/qb-port-sync` (systemd) or to the launchd property list, or export them before launching manually. The only required secret is `QB_PORT_SYNC_QB_PASSWORD` when the config leaves `password = ""`.

Example `.env` snippet (`.env.example` provided):
```
QB_PORT_SYNC_QB_PASSWORD=supersecret
RUST_LOG=info
```

### Running as a service

#### systemd (Linux)
1. Create a dedicated user: `sudo useradd --system --create-home qbportsync`
2. Install the binary (default assumes `/usr/local/bin/qb-port-sync`)
3. Copy `systemd/qb-port-sync.service` to `/etc/systemd/system/`
4. Optional: place environment overrides in `/etc/default/qb-port-sync`
5. Enable and start: `sudo systemctl enable --now qb-port-sync`

The unit runs without elevated capabilities (`CapabilityBoundingSet=`) and uses a read-only root filesystem (`ProtectSystem=full`). Logs are available through `journalctl -u qb-port-sync`.

#### launchd (macOS)
1. Copy the binary to `/usr/local/bin`
2. Copy `launchd/com.example.qb-port-sync.plist` to `/Library/LaunchDaemons/`
3. Adjust the `ProgramArguments` path if needed
4. Load the service: `sudo launchctl load -w /Library/LaunchDaemons/com.example.qb-port-sync.plist`

Logs are written to `/var/log/qb-port-sync.log` (stdout/stderr).

## CLI reference
```
USAGE:
    qb-port-sync [FLAGS] [OPTIONS]

FLAGS:
    -v, --verbose    Increase log verbosity (-vv = DEBUG, -vvv = TRACE)
        --once       Perform a single port sync and exit

OPTIONS:
        --config <path>      Override configuration path
        --strategy <mode>    file | pcp | natpmp | auto (default)
```

`auto` uses the ProtonVPN forwarded port file on Linux when available and falls back to PCP/NAT-PMP otherwise.

## Security notes
- Always prefer HTTPS for the qBittorrent Web UI (`base_url = "https://..."`).
- Store the Web UI password in the environment rather than on disk. The daemon only keeps the cookie-backed session in memory.
- The provided service files drop capabilities and isolate the binary from user home directories.

## Troubleshooting
- **Authentication fails** – ensure the Web UI password is correct, verify the Referer header matches the qBittorrent base URL, and check for CSRF protection prompts.
- **Forwarded port file missing** – ProtonVPN only populates `/run/user/$UID/Proton/VPN/forwarded_port` after connecting to a P2P server with port forwarding enabled.
- **PCP/NAT-PMP errors** – confirm your router exposes PCP or NAT-PMP. Disable `autodiscover_gateway` and set `gateway = "192.168.1.1"` if discovery fails.
- **Port mismatch warnings** – some gateways allocate a different external port than requested. The daemon automatically applies the external port that is granted.

## Development
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`

CI enforces formatting, clippy, and tests via GitHub Actions.

## License
MIT – see [LICENSE](LICENSE).
