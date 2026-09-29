use md5::Md5;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

const PLUGIN_ID: &str = "remotehand.toolbox";
const PLUGIN_VERSION: &str = "0.1.0";
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
struct MediaJobSnapshot {
    id: u64,
    state: String,
    mode: String,
    url: String,
    output_dir: String,
    message: String,
    error: Option<String>,
}

#[derive(Default)]
struct MediaJobManager {
    next_id: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Arc<Mutex<MediaJobSnapshot>>>>,
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
        return Err("port or port range is required".to_string());
    }
    if let Some((start, end)) = text.split_once('-') {
        let start: u16 = start.trim().parse().map_err(|_| "invalid start port".to_string())?;
        let end: u16 = end.trim().parse().map_err(|_| "invalid end port".to_string())?;
        if start == 0 || end == 0 || start > end {
            return Err("port range must be between 1 and 65535 with start <= end".to_string());
        }
        let count = usize::from(end - start) + 1;
        if count > MAX_SCAN_PORTS {
            return Err(format!("port range is limited to {MAX_SCAN_PORTS} ports per scan"));
        }
        Ok((start..=end).collect())
    } else {
        let port: u16 = text.parse().map_err(|_| "invalid port".to_string())?;
        if port == 0 {
            return Err("port must be between 1 and 65535".to_string());
        }
        Ok(vec![port])
    }
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
        .map_err(|error| format!("cannot resolve {text}: {error}"))?;
    let mut unique = BTreeSet::new();
    for address in resolved {
        unique.insert(address.ip());
        if unique.len() >= 16 {
            break;
        }
    }
    if unique.is_empty() {
        Err(format!("{text} did not resolve to an IP address"))
    } else {
        Ok(unique.into_iter().collect())
    }
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

fn traceroute(target: &str) -> Result<String, String> {
    let target = validate_network_target(target)?;
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("tracert");
        command.args(["-d", "-w", "1500", target]);
        command_text(command)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("traceroute");
        command.args(["-n", "-w", "2", target]);
        command_text(command)
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

fn executable_version(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text.lines().next().unwrap_or_default().to_string()) }
}

fn validate_media_url(url: &str) -> Result<&str, String> {
    let value = url.trim();
    if value.len() > 4096 || !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err("media URL must be an http:// or https:// URL".to_string());
    }
    Ok(value)
}

impl MediaJobManager {
    fn start(&self, url: String, mode: String, output_dir: String) -> Result<MediaJobSnapshot, String> {
        validate_media_url(&url)?;
        if !matches!(mode.as_str(), "video" | "audio") {
            return Err("media mode must be video or audio".to_string());
        }
        let output_path = PathBuf::from(output_dir.trim());
        if !output_path.is_dir() {
            return Err("output directory does not exist".to_string());
        }
        if executable_version("yt-dlp", &["--version"]).is_none() {
            return Err("yt-dlp was not found in PATH".to_string());
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let snapshot = MediaJobSnapshot {
            id,
            state: "running".to_string(),
            mode: mode.clone(),
            url: url.clone(),
            output_dir: output_path.to_string_lossy().into_owned(),
            message: "Download started".to_string(),
            error: None,
        };
        let job = Arc::new(Mutex::new(snapshot.clone()));
        self.jobs
            .lock()
            .map_err(|_| "media job store lock poisoned".to_string())?
            .insert(id, Arc::clone(&job));

        thread::spawn(move || {
            let template = output_path.join("%(title)s.%(ext)s");
            let mut command = Command::new("yt-dlp");
            command.arg("--no-playlist").arg("--newline");
            if mode == "audio" {
                command.args(["-x", "--audio-format", "mp3"]);
            } else {
                command.args(["-f", "bv*+ba/b", "--merge-output-format", "mp4"]);
            }
            command.arg("-o").arg(template).arg(url);
            let outcome = command.output();

            if let Ok(mut state) = job.lock() {
                match outcome {
                    Ok(output) if output.status.success() => {
                        state.state = "completed".to_string();
                        state.message = "Download completed".to_string();
                    }
                    Ok(output) => {
                        state.state = "failed".to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        state.error = Some(if !stderr.is_empty() { stderr } else if !stdout.is_empty() { stdout } else { format!("yt-dlp failed with {}", output.status) });
                        state.message = "Download failed".to_string();
                    }
                    Err(error) => {
                        state.state = "failed".to_string();
                        state.error = Some(error.to_string());
                        state.message = "Download failed".to_string();
                    }
                }
            }
        });

        Ok(snapshot)
    }

    fn get(&self, id: u64) -> Result<MediaJobSnapshot, String> {
        let job = self
            .jobs
            .lock()
            .map_err(|_| "media job store lock poisoned".to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("media job {id} was not found"))?;
        job.lock()
            .map(|state| state.clone())
            .map_err(|_| "media job state lock poisoned".to_string())
    }
}

fn pick_file(
    host_call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
    filters: Value,
) -> Result<Value, String> {
    host_call("file.open", json!({ "filters": filters }))
}

fn dispatch(
    request: &HostRequest,
    host_call: &mut dyn FnMut(&str, Value) -> Result<Value, String>,
    scans: &PortScanManager,
    media: &MediaJobManager,
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
            convert_image(&input, Path::new(output), &params.format)?;
            Ok(json!({ "saved": true, "path": output }))
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
        "media.requirements" => Ok(json!({
            "ytDlp": executable_version("yt-dlp", &["--version"]),
            "ffmpeg": executable_version("ffmpeg", &["-version"])
        })),
        "media.start" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Params { url: String, mode: String, output_dir: String }
            let params: Params = serde_json::from_value(request.params.clone())
                .map_err(|error| format!("invalid media.start request: {error}"))?;
            Ok(serde_json::to_value(media.start(params.url, params.mode, params.output_dir)?)
                .map_err(|error| error.to_string())?)
        }
        "media.status" => {
            let id = request.params.get("id").and_then(Value::as_u64)
                .ok_or_else(|| "media.status requires id".to_string())?;
            Ok(serde_json::to_value(media.get(id)?).map_err(|error| error.to_string())?)
        }
        method => Err(format!("unsupported plugin method: {method}")),
    }
}

fn default_timeout_ms() -> u64 { 350 }

fn run() -> Result<(), String> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let scans = PortScanManager::default();
    let media = MediaJobManager::default();

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
        let (ok, result, error) = match dispatch(&request, &mut host_call, &scans, &media) {
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
    fn media_url_requires_http() {
        assert!(validate_media_url("https://example.com/video").is_ok());
        assert!(validate_media_url("file:///tmp/video").is_err());
    }

    #[test]
    fn network_target_rejects_whitespace() {
        assert!(validate_network_target("example.com").is_ok());
        assert!(validate_network_target("bad target").is_err());
    }
}
