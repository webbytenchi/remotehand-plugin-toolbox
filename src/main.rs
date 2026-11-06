use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use md5::Md5;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Cursor, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

const PLUGIN_ID: &str = "remotehand.toolbox";
const PLUGIN_VERSION: &str = "0.1.2";
const API_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_HASH_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCAN_TARGETS: usize = 256;
const MAX_SCAN_PORTS: usize = 4096;
const MAX_SCAN_ATTEMPTS: usize = 131_072;
const MAX_SCAN_WORKERS: usize = 64;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginHello<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    plugin_id: &'a str,
    api_version: u32,
    plugin_version: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostReady {
    #[serde(rename = "type")]
    message_type: String,
    plugin_id: String,
    api_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest {
    #[serde(rename = "type")]
    message_type: String,
    request_id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: u64,
    ok: bool,
    result: Value,
    error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginHostRequest<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostResponse {
    #[serde(rename = "type")]
    message_type: String,
    request_id: u64,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenPort {
    target: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PortScanSnapshot {
    id: u64,
    state: String,
    target_input: String,
    port_input: String,
    targets: usize,
    ports: usize,
    total: usize,
    completed: usize,
    open: Vec<OpenPort>,
    cancellable: bool,
    error: Option<String>,
}

struct PortScanControl {
    snapshot: Mutex<PortScanSnapshot>,
    cancel: AtomicBool,
}

#[derive(Default)]
struct PortScanManager {
    next_id: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Arc<PortScanControl>>>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceSnapshot {
    id: u64,
    state: String,
    output: String,
    error: Option<String>,
}

struct TraceControl {
    snapshot: Mutex<TraceSnapshot>,
}

#[derive(Default)]
struct TraceManager {
    next_id: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Arc<TraceControl>>>,
}

fn append_trace_output(control: &TraceControl, text: &str) {
    if let Ok(mut snapshot) = control.snapshot.lock() {
        if !snapshot.output.is_empty() {
            snapshot.output.push('\n');
        }
        snapshot.output.push_str(text.trim_end_matches(['\r', '\n']));
    }
}


fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), String> {
    let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err("frame exceeds Toolbox Plugin API limit".to_string());
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .map_err(|error| error.to_string())?;
    writer.write_all(&payload).map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(format!("invalid frame length: {length}"));
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| error.to_string())?;
    Ok(Some(payload))
}

fn call_host<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    next_request_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    *next_request_id = next_request_id.wrapping_add(1).max(1);
    let request_id = *next_request_id;
    write_frame(
        writer,
        &PluginHostRequest {
            message_type: "plugin.request",
            request_id,
            method,
            params,
        },
    )?;
    let payload = read_frame(reader)?
        .ok_or_else(|| "host closed stream while awaiting host.response".to_string())?;
    let response: HostResponse =
        serde_json::from_slice(&payload).map_err(|error| format!("invalid host.response: {error}"))?;
    if response.message_type != "host.response" {
        return Err(format!(
            "unexpected message while awaiting host.response: {}",
            response.message_type
        ));
    }
    if response.request_id != request_id {
        return Err(format!(
            "host.response requestId mismatch: expected {request_id}, got {}",
            response.request_id
        ));
    }
    if response.ok {
        Ok(response.result)
    } else if response.error.trim().is_empty() {
        Err(format!("host API method {method} failed"))
    } else {
        Err(response.error)
    }
}

fn digest_reader<R: Read>(mut reader: R, algorithm: &str) -> Result<String, String> {
    let algorithm = algorithm.trim().to_ascii_lowercase();
    let mut buffer = [0u8; 64 * 1024];
    match algorithm.as_str() {
        "md5" => {
            let mut hasher = Md5::new();
            loop {
                let n = reader.read(&mut buffer).map_err(|error| error.to_string())?;
                if n == 0 { break; }
                hasher.update(&buffer[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        "sha1" | "sha-1" => {
            let mut hasher = Sha1::new();
            loop {
                let n = reader.read(&mut buffer).map_err(|error| error.to_string())?;
                if n == 0 { break; }
                hasher.update(&buffer[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        "sha256" | "sha-256" => {
            let mut hasher = Sha256::new();
            loop {
                let n = reader.read(&mut buffer).map_err(|error| error.to_string())?;
                if n == 0 { break; }
                hasher.update(&buffer[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        "sha512" | "sha-512" => {
            let mut hasher = Sha512::new();
            loop {
                let n = reader.read(&mut buffer).map_err(|error| error.to_string())?;
                if n == 0 { break; }
                hasher.update(&buffer[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        _ => Err("algorithm must be MD5, SHA-1, SHA-256, or SHA-512".to_string()),
    }
}

fn hash_text(text: &str, algorithm: &str) -> Result<String, String> {
    if text.len() > MAX_HASH_TEXT_BYTES {
        return Err("text exceeds the 8 MiB hash input limit".to_string());
    }
    digest_reader(text.as_bytes(), algorithm)
}

fn hash_file(path: &Path, algorithm: &str) -> Result<String, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.to_string_lossy()))?;
    digest_reader(file, algorithm)
}

fn parse_ports(input: &str) -> Result<Vec<u16>, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("port, port range, or comma-separated port list is required".to_string());
    }

    let mut ports = BTreeSet::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("port list contains an empty item".to_string());
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: u16 = start.trim().parse().map_err(|_| format!("invalid start port: {part}"))?;
            let end: u16 = end.trim().parse().map_err(|_| format!("invalid end port: {part}"))?;
            if start == 0 || end == 0 || start > end {
                return Err("port ranges must be between 1 and 65535 with start <= end".to_string());
            }
            for port in start..=end {
                ports.insert(port);
                if ports.len() > MAX_SCAN_PORTS {
                    return Err(format!("scan is limited to {MAX_SCAN_PORTS} unique ports"));
                }
            }
        } else {
            let port: u16 = part.parse().map_err(|_| format!("invalid port: {part}"))?;
            if port == 0 {
                return Err("ports must be between 1 and 65535".to_string());
            }
            ports.insert(port);
            if ports.len() > MAX_SCAN_PORTS {
                return Err(format!("scan is limited to {MAX_SCAN_PORTS} unique ports"));
            }
        }
    }
    Ok(ports.into_iter().collect())
}
fn ipv4_to_u32(ip: Ipv4Addr) -> u32 {
    u32::from_be_bytes(ip.octets())
}

fn u32_to_ipv4(value: u32) -> Ipv4Addr {
    Ipv4Addr::from(value.to_be_bytes())
}

fn parse_target_range(input: &str) -> Result<Option<Vec<IpAddr>>, String> {
    let Some((left, right)) = input.split_once('-') else {
        return Ok(None);
    };
    let Ok(start) = left.trim().parse::<Ipv4Addr>() else {
        return Ok(None);
    };
    let Ok(end) = right.trim().parse::<Ipv4Addr>() else {
        return Err("IP ranges must use full IPv4 addresses on both sides".to_string());
    };
    let start_num = ipv4_to_u32(start);
    let end_num = ipv4_to_u32(end);
    if start_num > end_num {
        return Err("IP range start must be less than or equal to the end".to_string());
    }
    let count = (end_num as u64 - start_num as u64 + 1) as usize;
    if count > MAX_SCAN_TARGETS {
        return Err(format!("IP range is limited to {MAX_SCAN_TARGETS} targets per scan"));
    }
    Ok(Some(
        (start_num..=end_num)
            .map(|value| IpAddr::V4(u32_to_ipv4(value)))
            .collect(),
    ))
}

fn resolve_targets(input: &str) -> Result<Vec<IpAddr>, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("target host, IP address, or IP range is required".to_string());
    }
    if let Some(range) = parse_target_range(text)? {
        return Ok(range);
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let resolved = (text, 0)
        .to_socket_addrs()
        .map_err(|error| format!("cannot resolve {text}: {error}"))?
        .map(|address| address.ip())
        .collect::<Vec<_>>();

    let selected = resolved
        .iter()
        .copied()
        .find(IpAddr::is_ipv4)
        .or_else(|| resolved.first().copied())
        .ok_or_else(|| format!("{text} did not resolve to an IP address"))?;

    Ok(vec![selected])
}
impl PortScanManager {
    fn start(&self, target_input: String, port_input: String, timeout_ms: u64) -> Result<PortScanSnapshot, String> {
        let targets = resolve_targets(&target_input)?;
        let ports = parse_ports(&port_input)?;
        let total = targets
            .len()
            .checked_mul(ports.len())
            .ok_or_else(|| "scan size overflow".to_string())?;
        if total > MAX_SCAN_ATTEMPTS {
            return Err(format!("scan is limited to {MAX_SCAN_ATTEMPTS} target/port attempts"));
        }
        let timeout_ms = timeout_ms.clamp(50, 5_000);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let snapshot = PortScanSnapshot {
            id,
            state: "running".to_string(),
            target_input: target_input.clone(),
            port_input: port_input.clone(),
            targets: targets.len(),
            ports: ports.len(),
            total,
            completed: 0,
            open: Vec::new(),
            cancellable: true,
            error: None,
        };
        let control = Arc::new(PortScanControl {
            snapshot: Mutex::new(snapshot.clone()),
            cancel: AtomicBool::new(false),
        });
        self.jobs
            .lock()
            .map_err(|_| "port scan store lock poisoned".to_string())?
            .insert(id, Arc::clone(&control));

        thread::spawn(move || {
            let attempts = Arc::new(
                targets
                    .iter()
                    .flat_map(|ip| ports.iter().map(move |port| (*ip, *port)))
                    .collect::<Vec<_>>(),
            );
            let next = Arc::new(AtomicUsize::new(0));
            let worker_count = attempts.len().min(MAX_SCAN_WORKERS).max(1);
            let mut workers = Vec::with_capacity(worker_count);

            for _ in 0..worker_count {
                let attempts = Arc::clone(&attempts);
                let next = Arc::clone(&next);
                let control = Arc::clone(&control);
                workers.push(thread::spawn(move || {
                    loop {
                        if control.cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((ip, port)) = attempts.get(index).copied() else {
                            break;
                        };
                        let address = SocketAddr::new(ip, port);
                        let open = TcpStream::connect_timeout(&address, Duration::from_millis(timeout_ms)).is_ok();
                        if let Ok(mut snapshot) = control.snapshot.lock() {
                            snapshot.completed = snapshot.completed.saturating_add(1);
                            if open {
                                snapshot.open.push(OpenPort {
                                    target: ip.to_string(),
                                    port,
                                });
                            }
                        }
                    }
                }));
            }

            for worker in workers {
                let _ = worker.join();
            }
            if let Ok(mut snapshot) = control.snapshot.lock() {
                snapshot.cancellable = false;
                snapshot.open.sort_by(|a, b| a.target.cmp(&b.target).then(a.port.cmp(&b.port)));
                snapshot.state = if control.cancel.load(Ordering::Relaxed) {
                    "cancelled".to_string()
                } else {
                    "completed".to_string()
                };
            }
        });

        Ok(snapshot)
    }

    fn get(&self, id: u64) -> Result<PortScanSnapshot, String> {
        let control = self
            .jobs
            .lock()
            .map_err(|_| "port scan store lock poisoned".to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("port scan {id} was not found"))?;
        control
            .snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| "port scan state lock poisoned".to_string())
    }

    fn cancel(&self, id: u64) -> Result<PortScanSnapshot, String> {
        let control = self
            .jobs
            .lock()
            .map_err(|_| "port scan store lock poisoned".to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("port scan {id} was not found"))?;
        control.cancel.store(true, Ordering::Relaxed);
        self.get(id)
    }
}

fn validate_network_target(target: &str) -> Result<&str, String> {
    let target = target.trim();
    if target.is_empty() || target.len() > 253 {
        return Err("target must be between 1 and 253 characters".to_string());
    }
    if target.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("target must not contain whitespace or control characters".to_string());
    }
    Ok(target)
}

fn command_text(mut command: Command) -> Result<String, String> {
    let output = command.output().map_err(|error| error.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            Err(format!("command failed with {}", output.status))
        } else {
            Err(trimmed.to_string())
        }
    }
}

fn ping(target: &str) -> Result<String, String> {
    let target = validate_network_target(target)?;
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("ping");
        command.args(["-n", "4", "-w", "1500", target]);
        command_text(command)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("ping");
        command.args(["-c", "4", "-W", "2", target]);
        command_text(command)
    }
}

fn traceroute_command(target: &str) -> Result<Command, String> {
    let target = validate_network_target(target)?;
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("tracert");
        command.args(["-d", "-w", "1500", target]);
        Ok(command)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("traceroute");
        command.args(["-n", "-w", "2", target]);
        Ok(command)
    }
}

fn traceroute(target: &str) -> Result<String, String> {
    command_text(traceroute_command(target)?)
}

impl TraceManager {
    fn start(&self, target: String) -> Result<TraceSnapshot, String> {
        let command = traceroute_command(&target)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let snapshot = TraceSnapshot {
            id,
            state: "running".to_string(),
            output: String::new(),
            error: None,
        };
        let control = Arc::new(TraceControl {
            snapshot: Mutex::new(snapshot.clone()),
        });
        self.jobs
            .lock()
            .map_err(|_| "traceroute store lock poisoned".to_string())?
            .insert(id, Arc::clone(&control));

        thread::spawn(move || {
            let mut command = command;
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => {
                    if let Ok(mut snapshot) = control.snapshot.lock() {
                        snapshot.state = "failed".to_string();
                        snapshot.error = Some(error.to_string());
                    }
                    return;
                }
            };

            let stderr_control = Arc::clone(&control);
            let stderr_thread = child.stderr.take().map(|stderr| {
                thread::spawn(move || {
                    for line in BufReader::new(stderr).lines() {
                        match line {
                            Ok(line) => append_trace_output(&stderr_control, &line),
                            Err(error) => {
                                if let Ok(mut snapshot) = stderr_control.snapshot.lock() {
                                    snapshot.error = Some(error.to_string());
                                }
                                break;
                            }
                        }
                    }
                })
            });

            if let Some(stdout) = child.stdout.take() {
                for line in BufReader::new(stdout).lines() {
                    match line {
                        Ok(line) => append_trace_output(&control, &line),
                        Err(error) => {
                            if let Ok(mut snapshot) = control.snapshot.lock() {
                                snapshot.error = Some(error.to_string());
                            }
                            break;
                        }
                    }
                }
            }

            let status = child.wait();
            if let Some(stderr_thread) = stderr_thread {
                let _ = stderr_thread.join();
            }

            if let Ok(mut snapshot) = control.snapshot.lock() {
                match status {
                    Ok(status) if status.success() => {
                        snapshot.state = "completed".to_string();
                    }
                    Ok(status) => {
                        snapshot.state = "failed".to_string();
                        if snapshot.error.is_none() {
                            snapshot.error = Some(format!("traceroute exited with {status}"));
                        }
                    }
                    Err(error) => {
                        snapshot.state = "failed".to_string();
                        snapshot.error = Some(error.to_string());
                    }
                }
            }
        });

        Ok(snapshot)
    }

    fn get(&self, id: u64) -> Result<TraceSnapshot, String> {
        let control = self
            .jobs
            .lock()
            .map_err(|_| "traceroute store lock poisoned".to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("traceroute {id} was not found"))?;
        control
            .snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| "traceroute state lock poisoned".to_string())
    }
}

fn image_format(name: &str) -> Result<(image::ImageFormat, &'static str), String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "png" => Ok((image::ImageFormat::Png, "png")),
        "jpg" | "jpeg" => Ok((image::ImageFormat::Jpeg, "jpg")),
        "webp" => Ok((image::ImageFormat::WebP, "webp")),
        "bmp" => Ok((image::ImageFormat::Bmp, "bmp")),
        "ico" => Ok((image::ImageFormat::Ico, "ico")),
        _ => Err("output format must be PNG, JPG, WebP, BMP, or ICO".to_string()),
    }
}

fn convert_image(input: &Path, output: &Path, format: &str) -> Result<(), String> {
    let (format, _) = image_format(format)?;
    let image = image::open(input)
        .map_err(|error| format!("cannot decode {}: {error}", input.to_string_lossy()))?;
    image
        .save_with_format(output, format)
        .map_err(|error| format!("cannot save {}: {error}", output.to_string_lossy()))
}

fn image_details(path: &Path, format_label: &str) -> Result<Value, String> {
    let image = image::open(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.to_string_lossy()))?;
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.to_string_lossy()))?;
    Ok(json!({
        "width": image.width(),
        "height": image.height(),
        "bytes": metadata.len(),
        "format": format_label.to_ascii_uppercase()
    }))
}

fn image_preview_data(path: &Path) -> Result<String, String> {
    let image = image::open(path)
        .map_err(|error| format!("cannot preview {}: {error}", path.to_string_lossy()))?
        .thumbnail(360, 360);
    let mut cursor = Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|error| format!("cannot encode preview: {error}"))?;
    Ok(format!("data:image/png;base64,{}", BASE64.encode(cursor.into_inner())))
}

fn reveal_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("generated file no longer exists".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer.exe")
            .arg("/select,")
            .arg(path)
            .spawn()
            .map_err(|error| format!("cannot open File Explorer: {error}"))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|error| format!("cannot open file manager: {error}"))?;
        Ok(())
    }
}

fn pick_file(
    host_call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
    filters: Value,
) -> Result<Value, String> {
    host_call("file.open", json!({ "filters": filters }))
}


fn validate_rdap_target(target: &str) -> Result<(String, &'static str), String> {
    let target = target.trim();
    if let Ok(ip) = target.parse::<IpAddr>() {
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
            return Err("enter a public IP address, not a loopback, multicast or unspecified address".to_string());
        }
        return Ok((ip.to_string(), "ip"));
    }
    if target.len() > 253 || target.len() < 4 || !target.is_ascii() || !target.contains('.') {
        return Err("enter a valid domain name (internationalized domains must use punycode) or IP address".to_string());
    }
    if !target.split('.').all(|part| {
        !part.is_empty() && part.len() <= 63 &&
        !part.starts_with('-') && !part.ends_with('-') &&
        part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    }) {
        return Err("domain labels may contain letters, digits and internal hyphens only".to_string());
    }
    Ok((target.to_ascii_lowercase(), "domain"))
}

fn rdap_event(record: &Value, action: &str) -> Option<String> {
    record.get("events")?.as_array()?.iter().find_map(|event| {
        (event.get("eventAction")?.as_str()? == action)
            .then(|| event.get("eventDate")?.as_str().map(str::to_string)).flatten()
    })
}

fn rdap_entity_name(record: &Value, role: &str) -> Option<String> {
    record.get("entities")?.as_array()?.iter().find_map(|entity| {
        let matches = entity.get("roles")?.as_array()?.iter().any(|r| r.as_str() == Some(role));
        if !matches { return None; }
        let array = entity.get("vcardArray")?.get(1)?.as_array()?;
        array.iter().find_map(|field| {
            let components = field.as_array()?;
            (components.first()?.as_str() == Some("fn"))
                .then(|| components.get(3)?.as_str().map(str::to_string)).flatten()
        }).or_else(|| entity.get("handle")?.as_str().map(str::to_string))
    })
}

fn rdap_lookup(target: &str, kind: &str) -> Result<Value, String> {
    // The bootstrap resolver directs requests to registries. Never accept a user-supplied URL.
    let url = format!("https://www.rdap.net/{kind}/{target}");
    #[cfg(target_os = "windows")]
    let executable = "curl.exe";
    #[cfg(not(target_os = "windows"))]
    let executable = "curl";
    let output = Command::new(executable)
        .args([
            "--silent", "--show-error", "--fail", "--location",
            "--max-redirs", "4", "--connect-timeout", "4", "--max-time", "12",
            "--max-filesize", "1048576",
            "--proto", "=https", "--proto-redir", "=https",
            "--header", "Accept: application/rdap+json, application/json",
            &url,
        ])
        .output()
        .map_err(|error| format!("cannot launch built-in curl for RDAP lookup: {error}"))?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        return Err(format!("RDAP lookup unavailable for {target}: {}", details.trim()));
    }
    if output.stdout.len() > 1_048_576 {
        return Err("RDAP response exceeded 1 MiB".to_string());
    }
    let record: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("registry did not return valid RDAP JSON: {error}"))?;
    let nameservers: Vec<String> = record.get("nameservers").and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item|
            item.get("ldhName").and_then(Value::as_str).map(str::to_string)
        ).collect()).unwrap_or_default();
    let statuses: Vec<String> = record.get("status").and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let mut fields = serde_json::Map::new();
    for (name, value) in [
        ("Name", record.get("ldhName").or(record.get("name")).and_then(Value::as_str)),
        ("Handle", record.get("handle").and_then(Value::as_str)),
    ] {
        if let Some(value) = value { fields.insert(name.to_string(), json!(value)); }
    }
    if let Some(value) = rdap_event(&record, "registration") { fields.insert("Registered".to_string(), json!(value)); }
    if let Some(value) = rdap_event(&record, "expiration") { fields.insert("Expires".to_string(), json!(value)); }
    if let Some(value) = rdap_event(&record, "last changed") { fields.insert("Updated".to_string(), json!(value)); }
    if let Some(value) = rdap_entity_name(&record, "registrar") { fields.insert("Registrar".to_string(), json!(value)); }
    if let Some(value) = record.get("startAddress").and_then(Value::as_str) { fields.insert("Start IP".to_string(), json!(value)); }
    if let Some(value) = record.get("endAddress").and_then(Value::as_str) { fields.insert("End IP".to_string(), json!(value)); }
    Ok(json!({
        "target": target, "kind": kind, "fields": fields,
        "statuses": statuses, "nameservers": nameservers,
        "raw": serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?,
        "source": "Registry RDAP (via rdap.net bootstrap)"
    }))
}


const MAX_WHOIS_BYTES: usize = 256 * 1024;

fn whois_fields(raw: &str) -> (serde_json::Map<String, Value>, Vec<String>, Vec<String>) {
    let mut fields = serde_json::Map::new();
    let mut statuses = Vec::new();
    let mut nameservers = Vec::new();
    for line in raw.lines().take(5000) {
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || value.len() > 2048 { continue; }
        match key.as_str() {
            "domain name" | "domain" => { fields.insert("Name".into(), json!(value)); }
            "registrar" => { fields.insert("Registrar".into(), json!(value)); }
            "creation date" | "created" | "created on" | "registration date" =>
                { fields.insert("Registered".into(), json!(value)); }
            "expiration date" | "expiry date" | "registry expiry date" | "expires" =>
                { fields.insert("Expires".into(), json!(value)); }
            "updated date" | "last updated" | "last modified" =>
                { fields.insert("Updated".into(), json!(value)); }
            "registrant organization" => { fields.insert("Organization".into(), json!(value)); }
            "domain status" | "status" => {
                for entry in value.split(',') {
                    let entry = entry.trim();
                    if !entry.is_empty() && statuses.len() < 40 { statuses.push(entry.to_string()); }
                }
            }
            "name server" | "nameserver" | "nserver" => {
                if nameservers.len() < 40 { nameservers.push(value.to_string()); }
            }
            _ => {}
        }
    }
    (fields, statuses, nameservers)
}

fn whois_ph_lookup(target: &str) -> Result<Value, String> {
    let server = "whois.dot.ph:43";
    let addresses = server.to_socket_addrs()
        .map_err(|error| format!("cannot resolve .ph WHOIS service: {error}"))?;
    let mut stream = None;
    for address in addresses {
        if let Ok(socket) = TcpStream::connect_timeout(&address, Duration::from_secs(4)) {
            stream = Some(socket);
            break;
        }
    }
    let mut stream = stream.ok_or_else(|| "could not connect to the community-listed .ph WHOIS service on TCP port 43 (network or service may block it)".to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    stream.write_all(format!("{target}\r\n").as_bytes())
        .map_err(|error| format!("WHOIS query failed: {error}"))?;
    let mut buffer = [0u8; 4096];
    let mut bytes = Vec::new();
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                if bytes.len() + n > MAX_WHOIS_BYTES {
                    return Err("WHOIS response exceeded the 256 KiB limit".to_string());
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            Err(error) => return Err(format!("WHOIS read failed: {error}")),
        }
    }
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let (fields, statuses, nameservers) = whois_fields(&raw);
    if fields.is_empty() && statuses.is_empty() && nameservers.is_empty() {
        return Err("the .ph WHOIS service returned no recognizable registration fields; registration status could not be determined".to_string());
    }
    Ok(json!({
        "target": target, "kind": "domain", "fields": fields,
        "statuses": statuses, "nameservers": nameservers,
        "raw": raw, "format": "text", "source": "Traditional WHOIS (whois.dot.ph:43; community-listed)",
        "note": "WHOIS uses unencrypted TCP port 43. This endpoint is community-listed, not published in IANA's .ph delegation. Returned fields may be incomplete."
    }))
}

fn ph_lookup_unavailable(target: &str, rdap_error: &str, whois_error: &str) -> Value {
    json!({
        "target": target, "kind": "domain", "fields": {"Name": target},
        "statuses": [], "nameservers": [], "raw": "", "format": "none",
        "source": "Registry information unavailable",
        "lookupStatus": "unavailable",
        "note": "Automated registration lookup is unavailable for this .ph domain. You can try the official dotPH WHOIS website. This does not mean the domain is unregistered.",
        "officialWhoisUrl": "https://whois.dot.ph/",
        "diagnostics": format!("RDAP: {rdap_error}\nTraditional WHOIS (whois.dot.ph:43): {whois_error}")
    })
}

fn lookup_registry(target: &str) -> Result<Value, String> {
    let (target, kind) = validate_rdap_target(target)?;
    match rdap_lookup(&target, kind) {
        Ok(result) => Ok(result),
        Err(rdap_error) => {
            if kind == "domain" && target.ends_with(".gov.ph") {
                return Ok(json!({
                    "target": target, "kind": "domain", "fields": {"Name": target, "Registration administrator": "Philippine DICT"},
                    "statuses": [], "nameservers": [], "raw": "", "format": "none",
                    "source": "Registry information unavailable",
                    "note": "RDAP was unavailable. gov.ph subdomains are administered by the Philippine Department of Information and Communications Technology (DICT), not dotPH. No supported public registration lookup was confirmed. See https://dns.gov.ph/ or contact DICT. This does not imply that the domain is unregistered.",
                    "lookupStatus": "unavailable"
                }));
            }
            if kind == "domain" && target.ends_with(".edu.ph") {
                return Ok(json!({
                    "target": target, "kind": "domain", "fields": {"Name": target},
                    "statuses": [], "nameservers": [], "raw": "", "format": "none",
                    "source": "Registry information unavailable",
                    "note": "RDAP was unavailable. edu.ph subdomains are administered separately from dotPH; a supported public registration-data endpoint has not been confirmed. This does not mean the domain is unregistered.",
                    "lookupStatus": "unavailable"
                }));
            }
            if kind == "domain" && target.ends_with(".ph") {
                return match whois_ph_lookup(&target) {
                    Ok(result) => Ok(result),
                    Err(whois_error) => Ok(ph_lookup_unavailable(&target, &rdap_error, &whois_error)),
                };
            }
            Err(format!("Registry RDAP lookup unavailable for {target}: {rdap_error}. No verified traditional WHOIS fallback is configured for this registry; this does not mean the domain is unregistered."))
        }
    }
}
fn dispatch(
    request: &HostRequest,
    host_call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
    scans: &PortScanManager,
    traces: &TraceManager,
) -> Result<Value, String> {
    match request.method.as_str() {
        "runtime.ping" => Ok(json!({
            "pluginId": PLUGIN_ID,
            "pluginVersion": PLUGIN_VERSION,
            "apiVersion": API_VERSION,
            "status": "ok"
        })),
        "hash.text" => {
            #[derive(Deserialize)]
            struct Params { text: String, algorithm: String }
            let params: Params = serde_json::from_value(request.params.clone())
                .map_err(|error| format!("invalid hash.text request: {error}"))?;
            let digest = hash_text(&params.text, &params.algorithm)?;
            Ok(json!({ "digest": digest }))
        }
        "file.pick" => pick_file(host_call, json!([])),
        "image.pick" => pick_file(host_call, json!([
            { "name": "Images", "extensions": ["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico"] }
        ])),
        "hash.file" => {
            #[derive(Deserialize)]
            struct Params { path: String, algorithm: String }
            let params: Params = serde_json::from_value(request.params.clone())
                .map_err(|error| format!("invalid hash.file request: {error}"))?;
            let digest = hash_file(Path::new(&params.path), &params.algorithm)?;
            Ok(json!({ "digest": digest }))
        }
        "image.convert" => {
            #[derive(Deserialize)]
            struct Params { path: String, format: String }
            let params: Params = serde_json::from_value(request.params.clone())
                .map_err(|error| format!("invalid image.convert request: {error}"))?;
            let input = PathBuf::from(&params.path);
            if !input.is_file() {
                return Err("input image does not exist".to_string());
            }
            let (_, extension) = image_format(&params.format)?;
            let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("converted");
            let suggested = format!("{stem}.{extension}");
            let save = host_call("file.save", json!({
                "suggestedName": suggested,
                "content": "",
                "filters": [
                    { "name": format!("{} image", extension.to_ascii_uppercase()), "extensions": [extension] }
                ]
            }))?;
            if save.get("saved") == Some(&Value::Bool(false)) {
                return Ok(json!({ "saved": false }));
            }
            let output = save
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "host file.save did not return a path".to_string())?;
            let output_path = Path::new(output);
            convert_image(&input, output_path, &params.format)?;
            let preview = image_preview_data(output_path)?;
            let input_format = input
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("image");
            let input_details = image_details(&input, input_format)?;
            let output_details = image_details(output_path, extension)?;
            Ok(json!({
                "saved": true,
                "path": output,
                "preview": preview,
                "input": input_details,
                "output": output_details
            }))
        }
        "network.rdap" => {
            let target = request.params.get("target").and_then(Value::as_str)
                .ok_or_else(|| "network.rdap requires target".to_string())?;
            lookup_registry(target)
        }
        "network.ping" => {
            let target = request.params.get("target").and_then(Value::as_str)
                .ok_or_else(|| "network.ping requires target".to_string())?;
            Ok(json!({ "output": ping(target)? }))
        }
        "network.traceroute" => {
            let target = request.params.get("target").and_then(Value::as_str)
                .ok_or_else(|| "network.traceroute requires target".to_string())?;
            Ok(json!({ "output": traceroute(target)? }))
        }
        "trace.start" => {
            let target = request.params.get("target").and_then(Value::as_str)
                .ok_or_else(|| "trace.start requires target".to_string())?;
            Ok(serde_json::to_value(traces.start(target.to_string())?)
                .map_err(|error| error.to_string())?)
        }
        "trace.status" => {
            let id = request.params.get("id").and_then(Value::as_u64)
                .ok_or_else(|| "trace.status requires id".to_string())?;
            Ok(serde_json::to_value(traces.get(id)?).map_err(|error| error.to_string())?)
        }
        "port.start" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Params { target: String, ports: String, #[serde(default = "default_timeout_ms")] timeout_ms: u64 }
            let params: Params = serde_json::from_value(request.params.clone())
                .map_err(|error| format!("invalid port.start request: {error}"))?;
            Ok(serde_json::to_value(scans.start(params.target, params.ports, params.timeout_ms)?)
                .map_err(|error| error.to_string())?)
        }
        "port.status" => {
            let id = request.params.get("id").and_then(Value::as_u64)
                .ok_or_else(|| "port.status requires id".to_string())?;
            Ok(serde_json::to_value(scans.get(id)?).map_err(|error| error.to_string())?)
        }
        "port.cancel" => {
            let id = request.params.get("id").and_then(Value::as_u64)
                .ok_or_else(|| "port.cancel requires id".to_string())?;
            Ok(serde_json::to_value(scans.cancel(id)?).map_err(|error| error.to_string())?)
        }
        "image.reveal" => {
            let path = request.params.get("path").and_then(Value::as_str)
                .ok_or_else(|| "image.reveal requires path".to_string())?;
            reveal_file(Path::new(path))?;
            Ok(json!({ "opened": true }))
        }
        method => Err(format!("unsupported plugin method: {method}")),
    }
}

fn default_timeout_ms() -> u64 { 350 }

fn run() -> Result<(), String> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let scans = PortScanManager::default();
    let traces = TraceManager::default();

    write_frame(
        &mut stdout,
        &PluginHello {
            message_type: "plugin.hello",
            plugin_id: PLUGIN_ID,
            api_version: API_VERSION,
            plugin_version: PLUGIN_VERSION,
        },
    )?;

    let payload = read_frame(&mut stdin)?
        .ok_or_else(|| "host closed stream before host.ready".to_string())?;
    let ready: HostReady =
        serde_json::from_slice(&payload).map_err(|error| format!("invalid host.ready: {error}"))?;
    if ready.message_type != "host.ready"
        || ready.plugin_id != PLUGIN_ID
        || ready.api_version != API_VERSION
    {
        return Err("RemoteHand rejected Plugin API v1 startup contract".to_string());
    }

    let mut next_host_request_id = 0u64;
    while let Some(payload) = read_frame(&mut stdin)? {
        let request: HostRequest =
            serde_json::from_slice(&payload).map_err(|error| format!("invalid host request: {error}"))?;
        if request.message_type != "host.request" {
            return Err(format!("unexpected Plugin API v1 message type: {}", request.message_type));
        }
        let mut host_call = |method: &str, params: Value| {
            call_host(
                &mut stdin,
                &mut stdout,
                &mut next_host_request_id,
                method,
                params,
            )
        };
        let (ok, result, error) = match dispatch(&request, &mut host_call, &scans, &traces) {
            Ok(result) => (true, result, String::new()),
            Err(error) => (false, Value::Null, error),
        };
        write_frame(
            &mut stdout,
            &PluginResponse {
                message_type: "plugin.response",
                request_id: request.request_id,
                ok,
                result,
                error,
            },
        )?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("RemoteHand Toolbox plugin failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_version_matches_manifest() {
        let manifest: Value =
            serde_json::from_str(include_str!("../plugin.json")).expect("plugin.json must parse");
        assert_eq!(manifest["version"], PLUGIN_VERSION);
        assert_eq!(manifest["id"], PLUGIN_ID);
    }

    #[test]
    fn parses_single_and_range_ports() {
        assert_eq!(parse_ports("443").unwrap(), vec![443]);
        assert_eq!(parse_ports("80-82").unwrap(), vec![80, 81, 82]);
        assert_eq!(parse_ports("22,80,443").unwrap(), vec![22, 80, 443]);
        assert_eq!(parse_ports("80,443,8000-8002").unwrap(), vec![80, 443, 8000, 8001, 8002]);
        assert!(parse_ports("0").is_err());
        assert!(parse_ports("90-80").is_err());
    }

    #[test]
    fn parses_full_ipv4_range() {
        let values = parse_target_range("192.168.1.10-192.168.1.12").unwrap().unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0].to_string(), "192.168.1.10");
        assert_eq!(values[2].to_string(), "192.168.1.12");
    }

    #[test]
    fn rejects_oversized_ranges() {
        assert!(parse_target_range("10.0.0.1-10.0.2.1").is_err());
        assert!(parse_ports("1-5000").is_err());
    }

    #[test]
    fn hashes_known_text() {
        assert_eq!(
            hash_text("abc", "sha256").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_text("abc", "md5").unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn validates_image_formats() {
        assert_eq!(image_format("jpeg").unwrap().1, "jpg");
        assert_eq!(image_format("webp").unwrap().1, "webp");
        assert!(image_format("tiff").is_err());
    }

    #[test]
    fn rdap_rejects_invalid_targets() {
        assert_eq!(validate_rdap_target("Example.COM").unwrap(), ("example.com".to_string(), "domain"));
        assert_eq!(validate_rdap_target("8.8.8.8").unwrap(), ("8.8.8.8".to_string(), "ip"));
        for invalid in ["localhost", "example.com/evil", "bad domain.com", "https://example.com", "127.0.0.1", "::1"] {
            assert!(validate_rdap_target(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn parses_whois_registration_fields() {
        let raw = "Domain Name: example.ph\nRegistrar: Example Registrar\nCreation Date: 2020-01-01\nExpiration Date: 2030-01-01\nName Server: ns1.example.ph\nName Server: ns2.example.ph\nStatus: clientTransferProhibited\n";
        let (fields, statuses, nameservers) = whois_fields(raw);
        assert_eq!(fields["Name"], "example.ph");
        assert_eq!(fields["Registrar"], "Example Registrar");
        assert_eq!(nameservers.len(), 2);
        assert_eq!(statuses, vec!["clientTransferProhibited"]);
    }

    #[test]
    fn ph_registry_unavailable_has_safe_official_link_and_diagnostics() {
        let result = ph_lookup_unavailable("dot.ph", "RDAP 404", "connection refused");
        assert_eq!(result["lookupStatus"], "unavailable");
        assert_eq!(result["officialWhoisUrl"], "https://whois.dot.ph/");
        assert_eq!(result["fields"]["Name"], "dot.ph");
        assert!(result["diagnostics"].as_str().unwrap().contains("connection refused"));
        assert_eq!(result["raw"], "");
    }

    #[test]
    fn network_target_rejects_whitespace() {
        assert!(validate_network_target("example.com").is_ok());
        assert!(validate_network_target("bad target").is_err());
    }
}
