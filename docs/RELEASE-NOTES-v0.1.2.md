# RemoteHand Toolbox v0.1.2 — Release notes

## Changes since v0.1.1

- **WHOIS / RDAP** is now the thirteenth Toolbox utility, under Network.
- Structured HTTPS RDAP domain and public-IP registration lookups with a copyable raw response, registrar/details, nameservers, events, and status codes when registries provide them.
- For ordinary `.ph` domains, an optional traditional WHOIS attempt to a community-listed server (`whois.dot.ph:43`) when RDAP is unavailable. This WHOIS service is not IANA-listed; it can be unreachable and no universal fallback is claimed.
- Graceful unavailable-registry reporting for `.gov.ph` (administered by DICT) and `.edu.ph` (administered separately) rather than treating RDAP 404 as proof a domain is unregistered.
- When both automated `.ph` methods fail, offer the official [dotPH WHOIS website](https://whois.dot.ph/) via the RemoteHand host's permission-gated `link.open` API, plus expandable diagnostics. Does not scrape registry websites.
- Existing v0.1.2 developments also include consistent copy actions and progressive Traceroute results.

## Security and compatibility

- Windows x86_64; RemoteHand Plugin API v1.
- Additional plugin permission: `links.open` to launch the official HTTPS site in the system browser. Existing `network.connect`, `network.inspect`, `files.open`, and `files.save` remain.
- Domain/IP inputs are validated. RDAP and WHOIS calls have bounded time and response size. No administrator privileges or new background service are required.
- WHOIS on TCP 43 is unencrypted. Registration metadata may be privacy-redacted or absent. Registry lookup is not a DNS availability check.
- Runtime uses Windows' bundled `curl.exe` for RDAP.

## Validation

- GitHub Actions Windows Rust tests and release build verified on the WHOIS/RDAP PR development head prior to final documentation-only changes.
- Windows manual testing by Papa: `google.com` registration summary and raw RDAP JSON; `dti.gov.ph` graceful separate-registry explanation; `dot.ph` unavailable message with diagnostics; official dotPH WHOIS site successfully launched through the default browser.
- Final tagged build and ZIP SHA256 should be established from the exact final `main` commit. Do not reuse a checksum from an earlier development package.

## Packaging

Run `cargo test` and `powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1` from the repository root on Windows. This produces `dist\remotehand-toolbox-v0.1.2-windows-x86_64.zip` and `dist\SHA256SUMS.txt`.

A version tag is not a published GitHub Release. Distribution assets require a separate explicit publication step.
