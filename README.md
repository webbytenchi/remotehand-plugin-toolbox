# RemoteHand Toolbox

RemoteHand Toolbox is a curated collection of practical utilities for RemoteHand.

Toolbox intentionally ships a built-in catalog rather than a second plugin system. Specialized features can live as normal RemoteHand plugins; broadly useful utilities can be added to Toolbox in future releases.

## v0.1.1

Toolbox v0.1.1 keeps the same 12 built-in tools as v0.1.0 and normalizes the launcher header to match the rest of RemoteHand.

### Generators

- **Password Generator** — cryptographically random passwords with configurable length and character sets.
- **Lorem Ipsum Generator** — placeholder words, sentences, or paragraphs for layouts and mockups.

### Encoding and data

- **Base64 Encode / Decode** — UTF-8-aware text conversion.
- **URL Encode / Decode** — URL-component escaping and decoding.
- **Hash / Checksum** — MD5, SHA-1, SHA-256, and SHA-512 for text and files.
- **Timestamp / Unix Epoch** — local date/time, Unix seconds, Unix milliseconds, and UTC conversion.
- **JSON Formatter / Validator** — validate, pretty-print, and compact JSON.
- **Regex Tester** — JavaScript-compatible regular-expression matching with indices and capture groups.

### Network

- **Ping** — operating-system ping with raw results.
- **Traceroute** — operating-system traceroute/tracert rendered as a hop list with raw output available.
- **Port Scanner** — single hostname/IP or full IPv4 range, combined with a single port, port range, or comma-separated list. Includes common-port presets and service-name legends. Scans are cancellable and concurrency-bounded. A hostname resolves to one scan target, preferring IPv4 when available.

Port Scanner v0.1.0 limits a single scan to:

- 256 target IP addresses
- 4,096 ports
- 131,072 target/port attempts
- 64 concurrent workers

A single target may be a hostname, IPv4 address, or IPv6 address. Ranges use full IPv4 addresses on both sides, for example:

`192.168.1.10-192.168.1.50`

Ports may be entered as a single port, range, or list, for example:

`443`

`1-1024`

`22,80,443,8000-8002`

### Image and media

- **Image Converter** — reads PNG, JPG/JPEG, WebP, BMP, GIF, and ICO; writes PNG, JPG, WebP, BMP, or ICO. GIF is treated as a static image in v0.1.0. Converted images show a thumbnail and can be revealed directly in the system file manager.

## Launcher

Opening Toolbox shows an Arcade-style launcher grouped into:

- Generators
- Encoding & Data
- Network
- Image

Every utility is a compact card. Selecting a card opens the tool in-place; **← Toolbox** returns to the launcher.

## Permissions

Toolbox v0.1.1 requests:

- `network.connect`
- `network.inspect`
- `files.open`
- `files.save`

Password generation, Base64, URL conversion, timestamp conversion, JSON formatting, and regex testing execute entirely in the panel and require no plugin permission.

## Architecture

RemoteHand core continues to own generic infrastructure:

- Plugin API v1 runtime
- panel hosting
- native file dialogs
- permission enforcement
- appearance and host interaction behavior
- plugin lifecycle

Toolbox owns:

- the built-in tool catalog
- launcher/navigation
- tool validation and presentation
- hashing
- image conversion
- network commands
- bounded port scanning

There is intentionally no user-defined Toolbox tool API in v0.1.1.

## Build on Windows

Requirements:

- Rust toolchain compatible with Rust 1.85 or later
- PowerShell
- a compatible RemoteHand Plugin API v1 host

Run:

```powershell
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1
```

The build script creates:

```text
dist\remotehand-toolbox-windows-x86_64\
dist\remotehand-toolbox-v0.1.1-windows-x86_64.zip
dist\SHA256SUMS.txt
```

## License

MIT.
