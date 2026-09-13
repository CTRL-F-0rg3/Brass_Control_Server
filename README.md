
# Brass Control Server (`brass_control_server`)

`brass_control_server` is a dedicated backend service designed to support the `brass_control` version control ecosystem. It combines a low-level **SSH transport layer** (powered by `russh`) for object pack transfer with a public **REST API** (powered by `axum`) for repository administration, monitoring, and integration with third-party tools.

---

## Release Versioning Scheme

The server uses a deterministic release versioning convention to ensure protocol and structural parity with client binaries.

### Version String Format
```text
w-2026.09.a122-base
│   │    │   │    └─ Build Tag / Stage (e.g., base, stable, dev)
│   │    │   └────── Compatibility Identification ID (e.g., a122)
│   │    └────────── Release Month (09 = September)
│   └─────────────── Release Year (2026)
└─────────────────── Version Edition Indicator ('w' = Work/Edition)

```

### Breakdown of Version Components

| Component | Example | Description |
| --- | --- | --- |
| **Prefix** | `w` | Version edition year indicator. |
| **Year** | `2026` | Calendar year of the release. |
| **Month** | `09` | Two-digit calendar month of the release. |
| **Compat ID** | `a122` | **Crucial:** Compatibility identifier for network protocols and pack formats. |
| **Tag** | `base` | Release classification tag (`base` marks the core baseline edition). |

### Client-Server Compatibility Matching

To guarantee operational stability, **`brass_control_server`** and **`brass_control`** must share the exact same **Compatibility Identification ID**.

* **Matching Example:** Server running `w-2026.09.a122-base` expects clients with `a122` in their version string.
* **Mismatch Handling:** If client and server IDs differ (e.g., `a122` vs `a123`), push/pull streaming or serialization schemas may fail due to protocol drift.

---

## Architecture & Services

The server runs two primary services concurrently on startup:

1. **SSH Service (Port `2222`):** Handles remote packfile transfers (`brass-receive-pack`) and remote administration commands (`brass-admin`).
2. **REST API (Port `8080`):** Exposes HTTP endpoints for Web UI dashboards, CI/CD webhooks, and programmatic repository creation/deletion.

---

## Public REST API Reference

The server exposes a public JSON HTTP API running on port `8080` by default.

### Endpoints

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/api/v1/repos` | Fetches a list of all repositories, object counts, and disk usage. |
| `POST` | `/api/v1/repos` | Creates a new bare repository on the server. |
| `DELETE` | `/api/v1/repos/:name` | Deletes a bare repository and all its associated objects. |

### cURL Usage Examples

**List Repositories:**

```bash
curl http://localhost:8080/api/v1/repos

```

**Create a Bare Repository:**

```bash
curl -X POST http://localhost:8080/api/v1/repos \
  -H "Content-Type: application/json" \
  -d '{"name": "system_core"}'

```

**Delete a Repository:**

```bash
curl -X DELETE http://localhost:8080/api/v1/repos/system_core

```

---

## Remote SSH Management

Administrators can execute management routines directly over SSH without accessing the server's local console:

```bash
# Query active repositories via SSH command request
ssh -p 2222 user@localhost "brass-admin repo list"

```

---

## Building and Running

### Prerequisites

* Rust 1.75+ (2024 edition target)

### Execution

```bash
# Compile and run the server
cargo run --release

```

Upon startup, the server automatically initializes the `./server_repositories` root directory and generates host keys for the SSH service.

```

```
