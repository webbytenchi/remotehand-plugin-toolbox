# RemoteHand Toolbox

RemoteHand Toolbox is a curated collection of practical utilities for RemoteHand.

Toolbox intentionally ships a built-in catalog rather than a second plugin system. Specialized features can live as normal RemoteHand plugins; broadly useful utilities can be added to Toolbox in future releases.

## v0.1.0 scope

The first Toolbox release contains 12 built-in tools:

### Generators

- **Password Generator** — cryptographically random passwords with configurable length and character sets.

### Encoding and data

- **Base64 Encode / Decode** — UTF-8-aware text conversion.
- **URL Encode / Decode** — URL-component escaping and decoding.
- **Hash / Checksum** — MD5, SHA-1, SHA-256, and SHA-512 for text and files.
- **Timestamp / Unix Epoch** — local date/time, Unix seconds, Unix milliseconds, and UTC conversion.
- **JSON Formatter / Validator** — validate, pretty-print, and compact JSON.
- **Regex Tester** — JavaScript-compatible regular-expression matching with indices and capture groups.

### Network

- **Ping** — operating-system ping with raw results.
- **Visual Traceroute** — operating-system traceroute/tracert rendered as a hop list with raw output available.
- **Port Scanner** — single hostname/IP or full IPv4 range, combined with a single port or port range. Scans are cancellable and concurrency-bounded.

Port Scanner v0.1.0 limits a single scan to:

- 256 target IP addresses
- 4,096 ports
- 131,072 target/port attempts
- 64 concurrent workers

A single target may be a hostname, IPv4 address, or IPv6 address. Ranges use full IPv4 addresses on both sides, for example:

`192.168.1.10-192.168.1.50`

Ports may be entered as either:

`443`

or:

`1-1024`

### Image and media

- **Image Converter** — reads PNG, JPG/JPEG, WebP, BMP, GIF, and ICO; writes PNG, JPG, WebP, BMP, or ICO. GIF is treated as a static image in v0.1.0.
- **Media Downloader** — authorized video or MP3 audio downloads through yt-dlp.

Media Downloader intentionally uses the maintained `yt-dlp` extractor rather than implementing site-specific scraping inside Toolbox. `yt-dlp` must be available in `PATH`. FFmpeg is recommended for best-quality video merging and required for MP3 extraction.

## Launcher

Opening Toolbox shows an Arcade-style launcher grouped into:

- Generators
- Encoding & Data
- Network
- Image & Media

Every utility is a compact card. Selecting a card opens the tool in-place; **← Toolbox** returns to the launcher.

## Permissions

Toolbox v0.1.0 requests:

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
- media-download orchestration

There is intentionally no user-defined Toolbox tool API in v0.1.0.

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
dist\remotehand-toolbox-v0.1.0-windows-x86_64.zip
dist\SHA256SUMS.txt
```

For Media Downloader testing, install `yt-dlp` and FFmpeg separately and make them available in `PATH`.

## License

MIT.
