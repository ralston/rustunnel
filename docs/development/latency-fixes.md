# Latency Fixes — Progress Tracker

Source analysis: [latency-investigation.md](./latency-investigation.md)

---

## Status Key

| Symbol | Meaning |
|--------|---------|
| ⬜ | Not started |
| 🔄 | In progress |
| ✅ | Complete |
| 🚀 | Deployed |

---

## Phase 1 — Quick Wins

**Goal**: Remove easy fixed-overhead latency from every request.
**Commit**: ✅

### Fix 1a — TCP_NODELAY on control/data WebSocket connections ✅

- **File**: `crates/rustunnel-server/src/control/server.rs`
- **Line**: after `listener.accept()` in `run_control_plane`
- **Change**: Add `let _ = tcp_stream.set_nodelay(true);`
- **Impact**: Removes up to 40 ms Nagle buffering per control frame (NewConnection, etc.)

### Fix 1b — TCP_NODELAY on local proxy connection ✅

- **File**: `crates/rustunnel-client/src/proxy.rs`
- **Line**: after `TcpStream::connect` in `proxy_connection`
- **Change**: Add `let _ = local.set_nodelay(true);`
- **Impact**: Ensures small response headers from local service aren't buffered

### Fix 1c — Stream response body instead of buffering ✅

- **File**: `crates/rustunnel-server/src/edge/http.rs`
- **Function**: `forward_http`
- **Change**: Replaced `resp_body.collect().await?.to_bytes()` with `futures_util::stream::unfold`-based `StreamBody`. The `sender` is moved into the unfold state to keep the upstream connection alive.
- **Impact**: Browser receives first bytes as soon as the local service starts responding (TTFB improvement). Particularly significant for HTML pages, JSON APIs, any response > a few KB.

---

## Phase 2 — Core Protocol Improvements

**Goal**: Make concurrent requests truly parallel; reduce yamux window stalls.
**Commit**: ⬜

### Fix 2a — Concurrent stream processing in drive_client_mux ⬜

- **File**: `crates/rustunnel-client/src/control.rs`
- **Function**: `drive_client_mux`
- **Change**: Spawn a task per accepted stream instead of awaiting `read_exact` inline. This unblocks `poll_next_inbound` immediately so the next stream can be accepted in parallel.
- **Impact**: When a browser loads a page with N subresources (CSS, JS, images), all N yamux streams are accepted and processed concurrently instead of one at a time. For a typical page with 10 assets, reduces stream-setup latency from `10 × T` to `1 × T`.

### Fix 2b — Increase yamux window size ⬜

- **Files**:
  - `crates/rustunnel-server/src/control/mux.rs` — server-side Connection
  - `crates/rustunnel-client/src/control.rs` — client-side Connection
- **Change**: Set `receive_window_size` from default 256 KB to 1 MB via `yamux::Config`
- **Impact**: Responses larger than 256 KB no longer stall waiting for WINDOW_UPDATE round trips. Significant for pages with images, large JS bundles, downloads.

---

## Phase 3 — Data Path Cleanup

**Goal**: Remove unnecessary indirection in the data plane; decouple driver serialisation.
**Commit**: ⬜

### Fix 3a — Decouple server yamux driver: spawn per-stream write tasks ⬜

- **File**: `crates/rustunnel-server/src/control/session.rs`
- **Function**: yamux driver task (`tokio::spawn` block)
- **Change**: After `poll_new_outbound`, spawn a separate task for `write_all(conn_id)` + `flush`. The driver loop returns immediately to `poll_next_inbound`, so yamux window updates and ACKs from the client are not blocked.
- **Impact**: Under concurrent requests, the driver no longer serialises stream-open operations. The main driver loop stays responsive for yamux flow control.

### Fix 3b — Eliminate duplex pipe (future) ⬜

- **Files**: `crates/rustunnel-server/src/control/mux.rs`, `crates/rustunnel-server/src/control/session.rs`, `crates/rustunnel-server/src/core/router.rs`, `crates/rustunnel-server/src/core/tunnel.rs`
- **Change**: Remove the `tokio::io::duplex` pair. Create the yamux `Connection` directly from `WsCompat(data WebSocket)` inside `handle_data_connection`. Pass a stream-open channel from `run_session` to `handle_data_connection` via `TunnelCore`.
- **Impact**: Removes two extra async copy hops per byte in the data plane. Removes the 64 KB buffer bottleneck that stalls large responses.
- **Note**: Larger refactor — tracked for a future session.

---

## Deployment Checklist

After implementing any phase, the following must be done before the fix is live:

### Server (Hetzner)

```bash
# On your local machine
cd ~/rustunnel
sudo make update-server
# This runs: git pull → cargo build --release → install → systemctl restart → systemctl status
```

### Client

```bash
# On each machine running the rustunnel client
make deploy-client
# Or manually:
cargo build --release -p rustunnel-client
sudo install -Dm755 target/release/rustunnel /usr/local/bin/rustunnel
```

### Verification

After deployment, compare before/after TTFB:

```bash
# Through tunnel (run several times to warm up)
curl -o /dev/null -s -w "TTFB: %{time_starttransfer}s  Total: %{time_total}s\n" \
  https://yoursubdomain.tunnel.example.com/

# Direct localhost baseline
curl -o /dev/null -s -w "TTFB: %{time_starttransfer}s  Total: %{time_total}s\n" \
  http://localhost:3000/
```

---

## Change Scope by Binary

| Fix | Server needs rebuild | Client needs rebuild |
|-----|---------------------|---------------------|
| 1a — TCP_NODELAY control WS | ✅ yes | ❌ no |
| 1b — TCP_NODELAY local proxy | ❌ no | ✅ yes |
| 1c — Stream response body | ✅ yes | ❌ no |
| 2a — Concurrent drive_client_mux | ❌ no | ✅ yes |
| 2b — Yamux window size | ✅ yes | ✅ yes |
| 3a — Decouple driver tasks | ✅ yes | ❌ no |
| 3b — Eliminate duplex pipe | ✅ yes | ❌ no |
