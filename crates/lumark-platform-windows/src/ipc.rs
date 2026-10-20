use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn request_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Lumark")
        .join("requests")
}

/// Persist a second-instance request for the primary process to consume.
/// The request directory inherits the current user's local-app-data ACL.
pub fn send_request(arguments: &[String]) -> std::io::Result<()> {
    let directory = request_dir();
    std::fs::create_dir_all(&directory)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let temporary = directory.join(format!(".request-{}-{stamp}.tmp", std::process::id()));
    let final_path = directory.join(format!("request-{}-{stamp}.txt", std::process::id()));
    let mut payload = arguments.join("\n");
    payload.push('\n');
    std::fs::write(&temporary, payload.as_bytes())?;
    std::fs::rename(temporary, final_path)
}

pub fn take_requests() -> std::io::Result<Vec<Vec<String>>> {
    let directory = request_dir();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };
    let mut requests = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("txt") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let _ = std::fs::remove_file(path);
        let arguments = String::from_utf8_lossy(&bytes)
            .lines()
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect();
        requests.push(arguments);
    }
    Ok(requests)
}
