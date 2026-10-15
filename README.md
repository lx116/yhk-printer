# yhk-printer

Open-source CLI, terminal UI, and local API for YHK-A32B BLE thermal printers.

The YHK-A32B is a thermal printer locked to its vendor app. This project reverse engineers observed protocol behaviour so anyone can use it from a terminal.

## Features

- Print images, text, and Markdown at the printer's 384-dot width.
- Encode print jobs to files without Bluetooth.
- Manage a queue in a terminal UI with drag-and-drop paths and batch modes.
- Use an offline browser editor for documents and photo-and-text cards.
- Integrate through a loopback-only local HTTP API.

## Status

### Verified on real hardware

- BLE connection and status queries on a YHK-A32B.
- Row-by-row photo printing, text printing, and Markdown letter printing.
- A printed QR code that scanned correctly with a phone (one test, one size).
- Both sides of a card printed manually: front first, then a reinserted back on double-sided paper.

### Not verified yet

- Pause and roll batch modes on paper.
- Whether density changes darkness, how HD looks on paper, and QR scan reliability across sizes and densities.
- Whether faster BLE pacing works, very long jobs, and zero-gap firmware trailer behaviour.
- Reinsert alignment accuracy, single-sided paper behaviour, and guided TUI card printing.
- Browser editor rendering in a real browser or phone, and real-terminal drag-and-drop behaviour.

## Supported hardware

Only the YHK-A32B has been tested. Only macOS has been tested. Other `YHK-*` models and other operating systems are untested.

## Install

```sh
cargo install --locked --git https://github.com/lx116/yhk-printer
```

Or build from source:

```sh
git clone https://github.com/lx116/yhk-printer.git
cd yhk-printer
cargo build --release
```

## Quick start

These commands do not need a printer or Bluetooth:

```sh
yhkp --help
yhkp encode --doc docs/BRIEF.md --out job.bin
yhkp doc docs/BRIEF.md --out preview.png
```

`encode` writes a printer job file. `doc --out` writes a 384-pixel-wide PNG preview; without `--out`, `doc` connects to a printer.

## Terminal UI

Run `yhkp tui` to open the interactive interface. It supports keyboard, mouse, and pasted terminal drop paths.

| Keys | Action |
|---|---|
| `c` | Connect or disconnect |
| `p` or Enter | Print the selected item |
| `a` | Print all ready items |
| `b` | Cycle Advance, Pause, and Roll batch modes |
| `e` | Open the selected document in the browser editor |
| `f` | Feed paper |
| `x` | Cancel the active job or paused sequence |
| `+` / `-` | Change copies |
| `m` | Cycle the print format: Rows, Raster, Gray |
| `v` | Toggle Gray and Printed preview |
| `g` | Toggle grid and selected preview |
| `q` or Esc | Quit (while a Pause sequence is waiting, Esc or `x` aborts it instead) |
| Tab | Switch queue and settings focus |
| Arrows | Select queue items or edit settings |
| `[` / `]` | Move the selected queue item |
| Delete or Backspace | Remove the selected item |
| Ctrl+L | Clear the queue |

Press `?` in the TUI for its key list.

## Documents and cards

`yhkp doc FILE.md --out preview.png` typesets plain text or Markdown with embedded open fonts. Documents support headings, paragraphs, lists, quotes, rules, and code blocks; Word, PDF, and inline images are not supported.

The offline browser editor previews the bitmap that will be sent to the printer. Its Cards tab combines a photo and Markdown text:

- **Below** prints the photo, a gap with an optional dotted cut mark, then the text.
- **Behind** prints the photo, asks you to cut, flip, and reinsert the paper, then prints a text canvas the height of the photo.
- For **Book**, flip left to right and insert the top edge first. For **Calendar**, flip top to bottom and insert the bottom edge first.

Behind cards require double-sided paper. The guided TUI flow is not verified on real hardware.

## Print options

| Option | Values | Notes |
|---|---|---|
| Quality | `low` (20), `medium` (25), `high` (30), or `--density N` | The vendor levels are known; their darkness effect is unverified. |
| Content | `auto`, `photo`, `text`, `qr` | Auto classifies the input; Photo dithers, Text thresholds, and QR preserves module geometry. |
| Definition | `standard`, `hd` | HD sharpens Photo/Text and uses local thresholding for Text; paper results are unverified. |
| Batch | `advance`, `pause`, `roll` | Advance is the default; Pause and Roll are not verified on paper. |
| Gap | `--gap-mm` | Adds up to 30 mm of trailing gap; cut marks default on. |

## Local HTTP API

Start the server with `yhkp serve`. It listens on `127.0.0.1:7731` by default. Do not run it while another program owns the printer connection.

| Route | Method | Purpose |
|---|---|---|
| `/editor` | GET | Offline browser editor |
| `/v1/health` | GET | Health check |
| `/v1/status` | GET | Service state |
| `/v1/connect`, `/v1/disconnect` | POST | Manage the printer connection |
| `/v1/print` | POST | Queue image data |
| `/v1/jobs`, `/v1/jobs/{id}` | GET | List or inspect jobs |
| `/v1/jobs/{id}` | DELETE | Cancel a job |
| `/v1/feed` | POST | Feed paper |
| `/v1/settings` | GET, PUT | Read or update settings |
| `/v1/events` | GET | Server-sent service events |
| `/v1/openapi.json` | GET | API description |
| `/v1/fonts`, `/v1/documents/templates` | GET | Editor resources |
| `/v1/documents/preview`, `/v1/documents/print`, `/v1/documents/draft` | POST | Document operations |
| `/v1/documents/draft/{id}` | GET | Read a draft |
| `/v1/cards/preview`, `/v1/cards/commit` | POST | Card operations |

Safe inspection examples:

```sh
curl http://127.0.0.1:7731/v1/health
curl http://127.0.0.1:7731/v1/settings
curl http://127.0.0.1:7731/v1/openapi.json
```

The server accepts loopback listeners by default, checks `Host` and browser `Origin` headers, and can require an `Authorization: Bearer TOKEN` header through `--token`. A non-loopback listener requires a token.

## Configuration

On macOS, settings are stored in `~/Library/Application Support/org.yhkp.yhkp/config.toml`.

## Troubleshooting

- Error `02` means the cover is open. The printer ignores jobs until it is closed.
- Only one program can hold the Bluetooth connection. Close the TUI before using printer-connected CLI commands.
- Allow Bluetooth access for the terminal application in macOS privacy settings.
- Slow sending is normal: the default is about 380 bytes per second.

## How it was made

Read the project briefs: [English](docs/BRIEF.md) and [Español](docs/BRIEF.es.md).

## Known limitations

- Rows is the working print mode. Raster and Gray formats are available but unverified on real hardware.
- This is not a general ESC/POS printer driver.
- Hardware verification is limited to one YHK-A32B and macOS.

## Credits and licenses

The code is licensed under the [MIT License](LICENSE). The bundled Atkinson Hyperlegible, Patrick Hand, and IBM Plex Mono fonts are licensed under the SIL Open Font License; their license files are in `assets/fonts`.

## Disclaimer

This project is independent and is not affiliated with or endorsed by the printer vendor or the WalkPrint app. It includes no vendor code, keys, or assets: only protocol behaviour observed from the author's own device for interoperability.
