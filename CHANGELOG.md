# Changelog

## 0.1.0 - Unreleased

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

This candidate is for development and manual acceptance testing. It is not a published release.

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
