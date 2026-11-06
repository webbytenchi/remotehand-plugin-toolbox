# RemoteHand Toolbox

RemoteHand Toolbox is a curated collection of practical utilities for RemoteHand.

Toolbox intentionally ships a built-in catalog rather than a second plugin system. Specialized features can live as normal RemoteHand plugins; broadly useful utilities can be added to Toolbox in future releases.

## v0.1.2

Toolbox v0.1.2 builds on the normalized Toolbox launcher from v0.1.1, adds consistent copy actions for reusable text output, streams Traceroute results while a trace is still running, and introduces WHOIS / RDAP domain and public IP registration lookups as the thirteenth built-in utility.

### Generators

- **Password Generator** — cryptographically random passwords with configurable length and character sets, with one-click copy.
- **Lorem Ipsum Generator** — placeholder words, sentences, or paragraphs for layouts and mockups, with one-click copy.

### Encoding and data

- **Base64 Encode / Decode** — UTF-8-aware text conversion with copyable output.
- **URL Encode / Decode** — URL-component escaping and decoding with copyable output.
- **Hash / Checksum** — MD5, SHA-1, SHA-256, and SHA-512 for text and files, with copyable digests.
- **Timestamp / Unix Epoch** — local date/time, Unix seconds, Unix milliseconds, and UTC conversion.
- **JSON Formatter / Validator** — validate, pretty-print, and compact JSON with a copyable syntax-highlighted preview.
- **Regex Tester** — JavaScript-compatible regular-expression matching with indices and capture groups.

### Network

- **WHOIS / RDAP** — domain registration and public IP registry lookup through HTTPS RDAP (rdap.net bootstrap), with structured key fields, nameservers, statuses, raw response and copy. Where RDAP fails, ordinary .ph domains attempt traditional port-43 WHOIS via community-listed whois.dot.ph; this server is not listed by IANA and may be unavailable. .gov.ph is independently administered by DICT, and .edu.ph is also separately administered; neither is sent to dotPH, and an unavailable-registry explanation is shown instead of a misleading 404. Windows 11 supplies curl.exe; other hosts need curl. Traditional WHOIS is unencrypted and requires outbound TCP 43; both methods limit time and output size. If both automated methods are unavailable for an ordinary .ph domain, Toolbox displays a concise explanation, an external link to the official https://whois.dot.ph/ lookup page, and expandable technical diagnostics. It does not scrape the website or imply that missing registry data means the domain is unregistered. The official WHOIS button uses the RemoteHand Plugin API v1 `link.open` host method, requiring the `links.open` permission, to open the site in the user's default browser. Direct HTML external navigation from the sandboxed panel is intentionally unsupported. No universal WHOIS fallback is claimed. Registration information may be redacted or absent.
- **Ping** — operating-system ping with raw, copyable results.
- **Traceroute** — operating-system traceroute/tracert rendered progressively as hops arrive, with live raw output and copy support.
- **Port Scanner** — single hostname/IP or full IPv4 range, combined with a single port, port range, or comma-separated list. Includes common-port presets and service-name legends. Scans are cancellable, concurrency-bounded, and results can be copied. A hostname resolves to one scan target, preferring IPv4 when available.

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

Toolbox v0.1.2 requests:

- `network.connect`
- `network.inspect`
- `files.open`
- `files.save`
- `links.open` (needed for the official dotPH WHOIS website button)

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
- RDAP domain/IP registry lookup via HTTPS

There is intentionally no user-defined Toolbox tool API in v0.1.2.

## Build on Windows

Requirements:

- Rust 1.85 or later. The `image` dependency is pinned to `0.25.8` so the declared Rust 1.85 baseline remains valid.
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
dist\remotehand-toolbox-v0.1.2-windows-x86_64.zip
dist\SHA256SUMS.txt
```

## License

MIT.
