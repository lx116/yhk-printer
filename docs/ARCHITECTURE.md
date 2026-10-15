# Architecture

`yhkp` is a small adapter stack: CLI, TUI, and HTTP entry points share one asynchronous printer service, which owns transport and invokes the pure imaging and protocol layers.

## Module map

| Area | Location | Responsibility |
|---|---|---|
| CLI | `src/main.rs`, `src/cli/` | Clap arguments, command dispatch, file-only encoding, and BLE command setup |
| TUI | `src/tui/` | Ratatui application state, input, previews, queue, settings, and service actions |
| Web | `src/web/` | Axum router, editor, document/card routes, and request security |
| Service | `src/service/` | Async actor, FIFO queue, settings persistence, events, jobs, and card hand-off |
| Protocol | `src/protocol.rs` | Pure wire encoding, reply parsing, gaps, and cut marks |
| Imaging | `src/imaging.rs` | Resize, classify, process, dither, and pack images |
| Typeset | `src/typeset.rs` | Pure layout and rasterization using embedded fonts |
| Document | `src/document.rs` | Plain-text and Markdown parsing into document blocks |
| Card | `src/card.rs` | Deterministic photo-and-text card composition |
| Transport | `src/transport.rs` | Transport traits and paced writer |
| BLE | `src/ble.rs` | CoreBluetooth-facing transport and connector through `btleplug` |

## Dependency direction

```text
main -> cli -> ble / service / protocol / imaging / document / typeset
tui  -> service / web / protocol / imaging / document / typeset / card
web  -> service / document / typeset / card / imaging
service -> protocol / transport
ble -> service::Connector / transport::Transport
card -> document / typeset / imaging / service::Settings
typeset -> document / imaging
protocol -> imaging
```

The pure layers do not depend on BLE, the terminal, or HTTP. `PrinterService` is the boundary that serializes printer work; adapters submit work and observe state rather than writing the transport directly.

## SOLID and KISS choices

- `Transport` and `Connector` are the only hardware seams, so fakes replace BLE in tests.
- The service is one actor with one FIFO queue rather than a shared mutable connection across three front ends.
- Protocol, imaging, parsing, typesetting, and card composition are direct functions over values.
- The web server is a thin router over the same service, not a second printer implementation.
- Embedded fonts and the editor asset avoid a runtime asset lookup or external web dependency.

## Tests

Tests are organized by boundary:

| Tests | Coverage |
|---|---|
| Unit tests beside modules | Pure protocol bytes, parsing, imaging, documents, typesetting, cards, service state, web routes, and TUI updates |
| `tests/protocol.rs` | Exact public wire layouts and reply parsing |
| `tests/pacing.rs` | Chunk and row-boundary pacing through a recording sleeper and transport |
| `tests/imaging.rs` | Image resize and packed raster invariants |
| `tests/cli.rs` | File-only `encode` and `doc --out` behaviour |

Service and web tests use fake transports/connectors. Settings tests use hermetic temporary files and remove them. No automated test connects to real Bluetooth hardware, launches the interactive TUI, or starts the HTTP server on a real listener.

Run the checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
