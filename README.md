# qb-port-sync

`qb-port-sync` keeps qBittorrent's listening port aligned with ProtonVPN's forwarded port. It supports two strategies:

- **File watcher (Linux default):** observe `/run/user/$UID/Proton/VPN/forwarded_port` and push changes immediately.
- **Port-mapping fallback (Linux/macOS):** request ProtonVPN's forwarded port via PCP, falling back to NAT-PMP when PCP is unavailable, then apply it to qBittorrent.

The daemon hardens qBittorrent's Web API usage, disables random ports and UPnP when applying updates, and can optionally bind to a specific VPN interface.

## Highlights

- Async Rust 2021 codebase using `tokio`, `reqwest`, `notify`, and `serde`.
- Configurable strategy selection (`file`, `pcp`, `natpmp`, `auto`).
- `--once` and `--json` flags for automation and scripting.
- Hardened qBittorrent Web API helper with cookie authentication, Referer/Origin headers, preference verification, and interface binding.
- PCP feature gated behind `--features pcp`; NAT-PMP always available (requires router support).
- **Prometheus metrics** and **health endpoints** for monitoring (feature-gated).
- **systemd-journald integration** on Linux for structured logging (feature-gated).
- **Docker support** with multi-stage builds and non-root execution.
- **Packaging**: PKGBUILD for Arch Linux, Homebrew formula for macOS, and installation script.
- systemd unit, user-level systemd `.path` + oneshot service, and launchd plist included.
- GitHub Actions CI matrix builds on Linux and macOS, installing `rustfmt`/`clippy` before fmt/clippy/test stages.

## Requirements

- Rust stable (see `rust-toolchain.toml`).
- qBittorrent Web UI enabled and reachable (HTTP or HTTPS). For HTTPS, ensure certificates are trusted by the host running `qb-port-sync`.
- ProtonVPN account with port forwarding support (Plus/Unlimited with PF-enabled P2P servers).
- For PCP/NAT-PMP fallback:
  - Router or gateway must expose NAT-PMP (or PCP when the `pcp` feature is enabled).
  - WireGuard manual setups must enable NAT-PMP when generating the profile; otherwise the PCP/NAT-PMP fallback cannot discover a forwarded port.

## Installation

### From Source

```bash
cargo install --path .
# or build manually
cargo build --release
cp target/release/qb-port-sync /usr/local/bin/
```

You can build with additional features:

```bash
# With PCP support
cargo build --release --features pcp

# With all features (PCP, journald, metrics)
cargo build --release --all-features
```

### Using the Installation Script

```bash
./install.sh
```

This builds the project and installs the binary, config, and systemd units to standard locations.

### Arch Linux

Build and install from the PKGBUILD:

```bash
makepkg -si
```

### macOS (Homebrew)

Install using the Homebrew formula:

```bash
brew install --build-from-source Formula/qb-port-sync.rb
```

### Docker

Build the Docker image:

```bash
docker build -t qb-port-sync:latest .
```

## Configuration

Copy the example config and edit as needed:

```bash
sudo install -d /etc/qb-port-sync
sudo cp config/config.example.toml /etc/qb-port-sync/config.toml
```

Search order (first match wins):

1. `--config <path>`
2. `$XDG_CONFIG_HOME/qb-port-sync/config.toml` (Linux)
3. `/Library/Application Support/qb-port-sync/config.toml` (macOS)
4. `/etc/qb-port-sync/config.toml` (Linux)

Key sections:

```toml
[qbittorrent]
base_url = "http://127.0.0.1:8080"
username = "admin"
password = ""           # leave blank to use QB_PORT_SYNC_QB_PASSWORD

[protonvpn]
forwarded_port_path = "" # Linux resolves to /run/user/$UID/Proton/VPN/forwarded_port

[portmap]
internal_port = 0         # 0 lets the gateway assign
protocol = "BOTH"         # TCP | UDP | BOTH (BOTH maps once and applies to qBittorrent)
refresh_secs = 300        # used when TTL is missing from the mapping API
autodiscover_gateway = true
gateway = ""             # override default gateway when autodiscovery is disabled

[net]
bind_interface = ""       # Optional qBittorrent interface binding (e.g., "tun0", "utun5")

[metrics]
enabled = false          # Enable Prometheus metrics endpoint at /metrics
port = 0                 # Set to non-zero to enable (e.g., 9000)

[health]
enabled = false          # Enable health check endpoint at /healthz
port = 0                 # Set to non-zero to enable, or 0 to use metrics port
```

If the qBittorrent password is blank, export `QB_PORT_SYNC_QB_PASSWORD` in the environment or `/etc/default/qb-port-sync`.

## Running the daemon

### One-shot update

Useful for cron/systemd timers or manual refreshes:

```bash
qb-port-sync --once --strategy auto --json
```

Example JSON output:

```json
{"strategy":"pcp","detected_port":51820,"applied":true,"verified":true,"note":"ttl=600s"}
```

Exit codes:

| Code | Meaning                                        |
|------|------------------------------------------------|
| 0    | Success (including “no change” idempotent run) |
| 1    | Transient error (network/auth/router)          |
| 2    | Configuration or usage error                   |
| 3    | Unsupported environment (e.g., PCP disabled and NAT-PMP unreachable) |

### Long-running service

```bash
qb-port-sync --strategy auto
```

- On Linux with ProtonVPN's forwarded port file available, the daemon runs a file watcher.
- Otherwise it negotiates a forwarded port using PCP first, then NAT-PMP.
- When PCP is unavailable (`--features pcp` not enabled), NAT-PMP is attempted directly.
- Ports are re-applied on change or refreshed at ~50% of the granted TTL (or `refresh_secs` fallback).

### systemd units

- `systemd/qb-port-sync.service`: continuous daemon (run as dedicated user `qbportsync`).
- `systemd/qb-port-sync.path` + `systemd/qb-port-sync-oneshot.service`: user-level trigger that runs `qb-port-sync --once --strategy file` whenever `%t/Proton/VPN/forwarded_port` changes.

Enable the continuous service:

```bash
sudo systemctl enable --now qb-port-sync.service
```

#### User-level .path + oneshot trigger

This variant follows ProtonVPN's desktop clients: `%t` resolves to `/run/user/$UID`, so the `.path` unit watches your per-user forwarded port file and launches the oneshot service to apply it.

```bash
mkdir -p ~/.config/systemd/user
cp systemd/qb-port-sync.path ~/.config/systemd/user/
cp systemd/qb-port-sync-oneshot.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now qb-port-sync.path
```

`systemctl --user` keeps the trigger scoped to your login session and runs `qb-port-sync --once --strategy file --config /etc/qb-port-sync/config.toml` each time the forwarded port changes.

### launchd (macOS)

Adapt `launchd/com.example.qb-port-sync.plist` as needed, install to `/Library/LaunchDaemons`, and load with:

```bash
sudo launchctl load -w /Library/LaunchDaemons/com.example.qb-port-sync.plist
```

## Monitoring and Observability

### Metrics and Health Endpoints

When built with the `metrics` feature, `qb-port-sync` can expose Prometheus-compatible metrics and a health check endpoint.

#### Configuration

Enable in your `config.toml`:

```toml
[metrics]
enabled = true
port = 9000

[health]
enabled = true
port = 9000  # Use same port as metrics, or specify different port
```

#### Available Metrics

- `qb_port_sync_port_updates_total`: Counter of successful port updates
- `qb_port_sync_current_port`: Current listening port configured in qBittorrent
- `qb_port_sync_last_update_timestamp_seconds`: Unix timestamp of last successful update

#### Health Endpoint

The `/healthz` endpoint returns:
- **200 OK** with "OK" body when the service has successfully updated qBittorrent at least once
- **503 Service Unavailable** with "Unhealthy" body if no successful update has occurred or the last update failed

#### Prometheus Scrape Configuration

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'qb-port-sync'
    static_configs:
      - targets: ['localhost:9000']
```

### Journald Integration

On Linux, when built with the `journald` feature, `qb-port-sync` automatically sends structured logs to systemd-journald in addition to standard output. This enables advanced filtering and querying:

```bash
# View logs
journalctl -u qb-port-sync.service -f

# Filter by log level
journalctl -u qb-port-sync.service -p info

# View logs from the last hour
journalctl -u qb-port-sync.service --since "1 hour ago"
```

The journald integration is automatic when running on Linux with the feature enabled—no additional configuration required.

## Docker

### Running with Docker

Create a configuration file:

```bash
cp config/config.example.toml my-config.toml
# Edit my-config.toml with your settings
```

Run the container:

```bash
docker run -d \
  --name qb-port-sync \
  -v $(pwd)/my-config.toml:/etc/qb-port-sync/config.toml:ro \
  -p 9000:9000 \
  qb-port-sync:latest
```

For one-shot mode:

```bash
docker run --rm \
  -v $(pwd)/my-config.toml:/etc/qb-port-sync/config.toml:ro \
  qb-port-sync:latest --once --strategy auto --json
```

### Docker Compose

```yaml
version: '3.8'
services:
  qb-port-sync:
    image: qb-port-sync:latest
    container_name: qb-port-sync
    volumes:
      - ./config.toml:/etc/qb-port-sync/config.toml:ro
    ports:
      - "9000:9000"
    restart: unless-stopped
```

## Security notes

- Always prefer HTTPS for the qBittorrent Web UI and ensure certificates are trusted.
- `qb-port-sync` logs in via the cookie-based Web API, sets `listen_port`, disables `random_port` and `upnp`, and verifies preferences afterwards.
- `qb-port-sync` enforces `random_port=false` and `upnp=false` via the Web API; **keep qBittorrent's own UPnP/NAT-PMP toggles disabled** to avoid conflicts with ProtonVPN port forwarding.
- Use environment variables or secure secrets managers for the Web UI password.

## Troubleshooting

- **Forwarded port file missing:** ProtonVPN only writes `/run/user/$UID/Proton/VPN/forwarded_port` after connecting to a PF-enabled P2P server.
- **NAT-PMP/PCP blocked:** Ensure your router allows NAT-PMP or PCP. **For WireGuard manual setups, you must enable NAT-PMP when generating the profile** in the ProtonVPN settings; otherwise port forwarding will not work.
- **qBittorrent UPnP/NAT-PMP conflicts:** Disable UPnP and NAT-PMP in qBittorrent's settings (Tools → Options → Connection) to prevent conflicts with ProtonVPN's port forwarding. `qb-port-sync` manages the port automatically.
- **Interface binding warnings:** When `bind_interface` is set but qBittorrent does not report the interface in `/api/v2/app/networkInterfaceList`, the daemon logs a warning and continues without binding.
- **Verification mismatch:** Some routers may remap the requested port. `qb-port-sync` logs a warning if qBittorrent reports a different port after the update.
- **Metrics not appearing:** Ensure you've built with `--features metrics` or `--all-features` and that the `[metrics]` section in config.toml has `enabled = true` and a non-zero `port`.

## Development

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

GitHub Actions (`.github/workflows/ci.yml`) builds on Linux and macOS, installs `rustfmt`/`clippy` explicitly, then runs `cargo fmt --all -- --check`, the full Clippy suite, and workspace tests.

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed contribution guidelines, packaging instructions, and Docker workflows.

## License

MIT – see [LICENSE](LICENSE).
