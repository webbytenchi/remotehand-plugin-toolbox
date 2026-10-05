# Changelog

## 0.1.1 - Unreleased

### Improved

- Normalized the Toolbox launcher header to match the visual rhythm of other RemoteHand plugins.
- Removed the redundant `REMOTEHAND PLUGIN` eyebrow and oversized marketing-style launcher heading.
- Added a compact `All Tools` row while keeping the existing `12 built-in tools` badge.
- Kept the launcher intentionally single-page: no placeholder Favorites or Settings tabs were added.
- Added consistent Copy actions for reusable output from Password Generator, Lorem Ipsum, Base64, URL Encode/Decode, Hash/Checksum, JSON preview, Ping, Traceroute, and Port Scanner.
- Traceroute now streams live hop/raw output while the operating-system trace is still running instead of waiting for completion.
- Preserved the declared Rust 1.85 baseline by pinning `image` to `0.25.8` and refreshing the lockfile to Rust-1.85-compatible dependency versions.

## 0.1.0

Initial RemoteHand Toolbox candidate with 12 curated built-in utilities:

- Password Generator
- Lorem Ipsum Generator
- Base64 Encode / Decode
- URL Encode / Decode
- Hash / Checksum
- Timestamp / Unix Epoch
- JSON Formatter / Validator
- Regex Tester
- Ping
- Traceroute
- Port Scanner
- Image Converter


### Acceptance polish

- Expanded JSON Formatter into a full-height editor/preview workspace with syntax highlighting.
- Added Regex quick-reference help and loadable examples.
- Renamed Visual Traceroute to Traceroute.
- Added Port Scanner presets, known-service legends, comma-separated port lists, and single-address hostname resolution.
- Added converted-image thumbnails and reveal-in-file-manager actions.
- Improved spacing between action buttons and result areas.
- Removed Media Downloader from v0.1.0 to keep Toolbox self-contained and dependency-free for end users.

- Fixed JSON Format/Compact so both editor and highlighted preview reset to the top after content is rewritten.
- Added Enter-to-run shortcuts for single-line tools and Ctrl+Enter primary actions for multiline editors.
- Added Image Converter input/output dimensions, format, and file-size details.

- Fixed JSON Formatter pane sizing so editable JSON and syntax-highlighted preview each scroll independently.
- Added Lorem Ipsum Generator with words, sentences, and paragraphs modes.

- Added more spacing between Lorem Ipsum controls and generated output.
- Added a hidden Lorem easter egg that only appears for Paragraphs × 10.
