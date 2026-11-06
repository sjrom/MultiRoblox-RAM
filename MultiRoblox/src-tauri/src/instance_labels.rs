// Persist attribution for log labels only; these records never grant process ownership.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize, Serialize)]
struct Instance {
    pid: u32,
    created: u64,
}

fn load(path: &Path) -> Option<HashMap<String, Instance>> {
    if let Err(e) = std::fs::read(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return None;
        }
    }
    let data = crate::jsonfile::read_object(path);
    if crate::jsonfile::is_unreadable(path) {
        return None;
    }
    serde_json::from_value(serde_json::Value::Object(data)).ok()
}

fn save_instance(path: &Path, id: &str, pid: u32, created: u64) -> Option<()> {
    let mut instances = load(path)?;
    instances.insert(id.to_string(), Instance { pid, created });
    let json = serde_json::to_string_pretty(&instances).ok()?;
    crate::jsonfile::write_atomic(path, &json).ok()
}

pub fn remember(id: &str, pid: u32) {
    if let Some(created) = process_created(pid) {
        let path = crate::paths::app_data_dir().join("instance-labels.json");
        if save_instance(&path, id, pid, created).is_none() {
            eprintln!("[storage] could not save instance log attribution");
        }
    }
}

fn matching_account(
    instances: &HashMap<String, Instance>,
    pid: u32,
    created: u64,
) -> Option<String> {
    instances
        .iter()
        .find(|(_, instance)| instance.pid == pid && instance.created == created)
        .map(|(id, _)| id.clone())
}

pub fn account_for_pid(pid: u32) -> Option<String> {
    let path = crate::paths::app_data_dir().join("instance-labels.json");
    let instances = load(&path)?;
    matching_account(&instances, pid, process_created(pid)?)
}

/// Returns all `(account_id, pid)` pairs from `instance-labels.json` whose
/// OS process is still alive with the exact same creation timestamp.
pub fn alive_instances() -> Vec<(String, u32)> {
    let path = crate::paths::app_data_dir().join("instance-labels.json");
    let Some(instances) = load(&path) else {
        return Vec::new();
    };
    instances
        .into_iter()
        .filter_map(|(id, inst)| {
            if process_created(inst.pid) == Some(inst.created) {
                Some((id, inst.pid))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(windows)]
fn process_created(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        result.ok()?;
        Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

#[cfg(not(windows))]
fn process_created(_: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_survive_restart_and_reject_reused_pids() {
        let path = std::env::temp_dir().join(format!("mr-labels-{}.json", uuid::Uuid::new_v4()));
        save_instance(&path, "account-a", 123, 1000).unwrap();
        save_instance(&path, "account-b", 456, 2000).unwrap();
        let instances = load(&path).unwrap();
        assert_eq!(
            matching_account(&instances, 123, 1000).as_deref(),
            Some("account-a")
        );
        assert_eq!(
            matching_account(&instances, 456, 2000).as_deref(),
            Some("account-b")
        );
        assert_eq!(matching_account(&instances, 123, 3000), None);
        assert_eq!(matching_account(&instances, 999, 1000), None);
        std::fs::write(&path, "{broken").unwrap();
        assert!(save_instance(&path, "account-c", 789, 4000).is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        std::fs::remove_file(path.with_extension("corrupt-backup.json")).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn process_identity_is_stable_and_invalid_pid_is_rejected() {
        let pid = std::process::id();
        let created = process_created(pid).unwrap();
        assert_ne!(created, 0);
        assert_eq!(process_created(pid), Some(created));
        assert_eq!(process_created(0), None);
    }
}
