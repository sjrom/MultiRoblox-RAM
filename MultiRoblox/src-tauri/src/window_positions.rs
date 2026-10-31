use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
struct Position {
    x: i32,
    y: i32,
}

#[derive(Default)]
pub struct WindowPositions {
    saved: Option<HashMap<String, Position>>,
    pending: HashMap<u32, Instant>,
}

impl WindowPositions {
    fn load(&mut self, path: &Path) {
        if self.saved.is_none() {
            // A permissions/sharing error must not turn existing data into an
            // empty store that the next successful write would overwrite.
            if let Err(e) = std::fs::read(path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return;
                }
            }
            self.saved = Some(
                crate::jsonfile::read_object(path)
                    .into_iter()
                    .filter_map(|(id, value)| serde_json::from_value(value).ok().map(|p| (id, p)))
                    .collect(),
            );
        }
    }

    fn save(&self, path: &Path) {
        if crate::jsonfile::is_unreadable(path) {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.saved) {
            if let Err(e) = crate::jsonfile::write_atomic(path, &json) {
                eprintln!("[windows] could not save instance positions: {}", e);
            }
        }
    }
}

pub fn launched(state: &AppState, pid: u32) {
    state
        .window_positions
        .lock()
        .unwrap()
        .pending
        .insert(pid, Instant::now());
}

// Only consult attributed PIDs, while holding their lock so account removal or
// a replacement launch cannot race a move. Save only when coordinates change.
pub fn update(state: &AppState) {
    let pids = state.account_pids.lock().unwrap();
    let mut positions = state.window_positions.lock().unwrap();
    let path = crate::paths::app_data_dir().join("window-positions.json");
    positions.load(&path);
    if positions.saved.is_none() {
        return;
    }
    positions
        .pending
        .retain(|pid, _| pids.values().any(|p| p == pid));
    let mut changed = false;
    for (id, pid) in pids.iter() {
        let Some(current) = platform::position(*pid) else {
            continue;
        };
        if let Some(started) = positions.pending.get(pid) {
            if let Some(saved) = positions.saved.as_ref().unwrap().get(id).copied() {
                if started.elapsed() < Duration::from_secs(60) {
                    if platform::restore(*pid, saved) {
                        positions.pending.remove(pid);
                    }
                    // Never replace the saved coordinates with startup defaults.
                    continue;
                }
            }
            positions.pending.remove(pid);
        }
        let saved = positions.saved.as_mut().unwrap();
        if saved.get(id) != Some(&current) {
            saved.insert(id.clone(), current);
            changed = true;
        }
    }
    if changed {
        positions.save(&path);
    }
}

pub fn forget(state: &AppState, id: &str) {
    let mut positions = state.window_positions.lock().unwrap();
    let path = crate::paths::app_data_dir().join("window-positions.json");
    positions.load(&path);
    if positions.saved.is_none() {
        return;
    }
    if positions.saved.as_mut().unwrap().remove(id).is_some() {
        positions.save(&path);
    }
}

fn fit(value: i32, size: i32, min: i32, max: i32) -> i32 {
    value.clamp(min, max.saturating_sub(size).max(min))
}

#[cfg(windows)]
mod platform {
    use super::{fit, Position};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::*;

    struct Search {
        pid: u32,
        window: Option<HWND>,
    }

    unsafe extern "system" fn find(hwnd: HWND, param: LPARAM) -> BOOL {
        let search = &mut *(param.0 as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == search.pid
            && IsWindowVisible(hwnd).as_bool()
            && GetWindowTextLengthW(hwnd) > 0
            && GetWindow(hwnd, GW_OWNER).unwrap_or_default().0.is_null()
        {
            search.window = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    fn window(pid: u32) -> Option<HWND> {
        let mut search = Search { pid, window: None };
        unsafe {
            let _ = EnumWindows(Some(find), LPARAM(&mut search as *mut Search as isize));
        }
        search.window
    }

    pub(super) fn position(pid: u32) -> Option<Position> {
        let hwnd = window(pid)?;
        unsafe {
            // Minimized coordinates and fullscreen/maximized bounds must not
            // overwrite the user's normal window position.
            if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
                return None;
            }
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).ok()?;
            if rect.right <= rect.left || rect.bottom <= rect.top {
                return None;
            }
            Some(Position {
                x: rect.left,
                y: rect.top,
            })
        }
    }

    pub(super) fn restore(pid: u32, position: Position) -> bool {
        let Some(hwnd) = window(pid) else {
            return false;
        };
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            let width = rect.right.saturating_sub(rect.left);
            let height = rect.bottom.saturating_sub(rect.top);
            let target = RECT {
                left: position.x,
                top: position.y,
                right: position.x.saturating_add(width),
                bottom: position.y.saturating_add(height),
            };
            let monitor = MonitorFromRect(&target, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            SetWindowPos(
                hwnd,
                None,
                fit(position.x, width, info.rcWork.left, info.rcWork.right),
                fit(position.y, height, info.rcWork.top, info.rcWork.bottom),
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .is_ok()
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Position;
    pub(super) fn position(_: u32) -> Option<Position> {
        None
    }
    pub(super) fn restore(_: u32, _: Position) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_fit_changed_and_negative_monitor_layouts() {
        assert_eq!(fit(-1600, 800, -1920, 0), -1600);
        assert_eq!(fit(3000, 800, 0, 1920), 1120);
        assert_eq!(fit(-2000, 800, 0, 1920), 0);
        assert_eq!(fit(100, 3000, 0, 1920), 0);
    }

    #[test]
    fn positions_survive_restart_and_corrupt_files_are_preserved() {
        let path = std::env::temp_dir().join(format!("mr-positions-{}.json", uuid::Uuid::new_v4()));
        let mut positions = WindowPositions::default();
        positions.load(&path);
        positions
            .saved
            .as_mut()
            .unwrap()
            .insert("test-account".into(), Position { x: -1000, y: 200 });
        positions.save(&path);
        let mut loaded = WindowPositions::default();
        loaded.load(&path);
        assert_eq!(loaded.saved, positions.saved);
        std::fs::write(&path, "{broken").unwrap();
        let mut corrupt = WindowPositions::default();
        corrupt.load(&path);
        corrupt.save(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        std::fs::remove_file(path.with_extension("corrupt-backup.json")).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
