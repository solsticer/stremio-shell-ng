use std::{cmp, mem};
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct SavedWindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    maximized: bool,
}

impl SavedWindowGeometry {
    fn config_path() -> Option<PathBuf> {
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("window_state.json")))
    }

    pub fn save(hwnd: HWND) {
        use winapi::um::winuser::{GetWindowPlacement, WINDOWPLACEMENT, IsZoomed};
        let maximized = unsafe { IsZoomed(hwnd) } != 0;
        let mut wp: WINDOWPLACEMENT = unsafe { mem::zeroed() };
        wp.length = mem::size_of::<WINDOWPLACEMENT>() as u32;
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } == 0 {
            return;
        }
        let state = SavedWindowGeometry {
            x: wp.rcNormalPosition.left,
            y: wp.rcNormalPosition.top,
            width: wp.rcNormalPosition.right - wp.rcNormalPosition.left,
            height: wp.rcNormalPosition.bottom - wp.rcNormalPosition.top,
            maximized,
        };
        if let Some(path) = Self::config_path() {
            if let Ok(json) = serde_json::to_string_pretty(&state) {
                std::fs::write(path, json).ok();
            }
        }
    }

    pub fn load() -> Option<SavedWindowGeometry> {
        let path = Self::config_path()?;
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }
}
use winapi::shared::windef::HWND;
use winapi::um::winuser::{
    GetForegroundWindow, GetMonitorInfoA, GetSystemMetrics, GetWindowLongA, GetWindowRect,
    IsIconic, IsZoomed, MonitorFromWindow, SetForegroundWindow, SetWindowLongA, SetWindowPos,
    GWL_EXSTYLE, GWL_STYLE, HWND_NOTOPMOST, HWND_TOPMOST, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    SM_CXSCREEN, SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, WS_CAPTION,
    WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_TOPMOST, WS_EX_WINDOWEDGE,
    WS_THICKFRAME,
};
// https://doc.qt.io/qt-5/qt.html#WindowState-enum
bitflags! {
    struct WindowState: u8 {
        const MINIMIZED = 0x01;
        const MAXIMIZED = 0x02;
        const FULL_SCREEN = 0x04;
        const ACTIVE = 0x08;
    }
}

#[derive(Default, Clone)]
pub struct WindowStyle {
    pub full_screen: bool,
    pub pos: (i32, i32),
    pub size: (i32, i32),
    pub style: i32,
    pub ex_style: i32,
}

impl WindowStyle {
    pub fn get_window_state(self, hwnd: HWND) -> u32 {
        let mut state: WindowState = WindowState::empty();
        if 0 != unsafe { IsIconic(hwnd) } {
            state |= WindowState::MINIMIZED;
        }
        if 0 != unsafe { IsZoomed(hwnd) } {
            state |= WindowState::MAXIMIZED;
        }
        if hwnd == unsafe { GetForegroundWindow() } {
            state |= WindowState::ACTIVE
        }
        if self.full_screen {
            state |= WindowState::FULL_SCREEN;
        }
        state.bits() as u32
    }
    pub fn is_window_minimized(&self, hwnd: HWND) -> bool {
        0 != unsafe { IsIconic(hwnd) }
    }
    pub fn show_window_at(&self, hwnd: HWND, pos: HWND) {
        unsafe {
            SetWindowPos(
                hwnd,
                pos,
                self.pos.0,
                self.pos.1,
                self.size.0,
                self.size.1,
                SWP_FRAMECHANGED,
            );
        }
    }
    pub fn center_window(&mut self, hwnd: HWND, min_width: i32, min_height: i32) {
        let monitor_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let monitor_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let small_side = cmp::min(monitor_w, monitor_h) * 70 / 100;
        self.size = (
            cmp::max(small_side * 16 / 9, min_width),
            cmp::max(small_side, min_height),
        );
        self.pos = ((monitor_w - self.size.0) / 2, (monitor_h - self.size.1) / 2);
        self.show_window_at(hwnd, HWND_NOTOPMOST);
    }
    pub fn toggle_full_screen(&mut self, hwnd: HWND) {
        if self.full_screen {
            let topmost = if self.ex_style as u32 & WS_EX_TOPMOST == WS_EX_TOPMOST {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };
            unsafe {
                SetWindowLongA(hwnd, GWL_STYLE, self.style);
                SetWindowLongA(hwnd, GWL_EXSTYLE, self.ex_style);
            }
            self.show_window_at(hwnd, topmost);
            self.full_screen = false;
        } else {
            unsafe {
                let mut rect = mem::zeroed();
                GetWindowRect(hwnd, &mut rect);
                self.pos = (rect.left, rect.top);
                self.size = ((rect.right - rect.left), (rect.bottom - rect.top));
                self.style = GetWindowLongA(hwnd, GWL_STYLE);
                self.ex_style = GetWindowLongA(hwnd, GWL_EXSTYLE);

                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut monitor_info: MONITORINFO = mem::zeroed();
                monitor_info.cbSize = mem::size_of_val(&monitor_info) as u32;
                if GetMonitorInfoA(monitor, &mut monitor_info) == 0 {
                    println!("GetMonitorInfoA failed");
                    return;
                }
                SetWindowLongA(
                    hwnd,
                    GWL_STYLE,
                    self.style & !(WS_CAPTION as i32 | WS_THICKFRAME as i32),
                );
                SetWindowLongA(
                    hwnd,
                    GWL_EXSTYLE,
                    self.ex_style
                        & !(WS_EX_DLGMODALFRAME as i32
                            | WS_EX_WINDOWEDGE as i32
                            | WS_EX_CLIENTEDGE as i32
                            | WS_EX_STATICEDGE as i32),
                );
                SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    monitor_info.rcMonitor.left,
                    monitor_info.rcMonitor.top,
                    monitor_info.rcMonitor.right - monitor_info.rcMonitor.left,
                    monitor_info.rcMonitor.bottom - monitor_info.rcMonitor.top,
                    SWP_FRAMECHANGED,
                );
            }
            self.full_screen = true;
        }
    }
    pub fn toggle_topmost(&mut self, hwnd: HWND) {
        let topmost = if unsafe { GetWindowLongA(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST
            == WS_EX_TOPMOST
        {
            HWND_NOTOPMOST
        } else {
            HWND_TOPMOST
        };
        unsafe {
            SetWindowPos(
                hwnd,
                topmost,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED,
            );
        }
        self.ex_style = unsafe { GetWindowLongA(hwnd, GWL_EXSTYLE) };
    }
    /// Restores window position from saved state. Returns true if restored.
    pub fn restore_window_state(&mut self, hwnd: HWND) -> bool {
        if let Some(state) = SavedWindowGeometry::load() {
            self.pos = (state.x, state.y);
            self.size = (state.width, state.height);
            self.show_window_at(hwnd, HWND_NOTOPMOST);
            if state.maximized {
                use winapi::um::winuser::{ShowWindow, SW_SHOWMAXIMIZED};
                unsafe { ShowWindow(hwnd, SW_SHOWMAXIMIZED); }
            }
            true
        } else {
            false
        }
    }

    /// Saves current window position and state to disk.
    pub fn save_window_state(&self, hwnd: HWND) {
        SavedWindowGeometry::save(hwnd);
    }

    /// Sets the window title bar color using DWM API (Windows 10 build 22000+).
    /// Color format is COLORREF (0x00BBGGRR).
    pub fn set_title_bar_color(&self, hwnd: HWND, color: u32) {
        use winapi::um::dwmapi::DwmSetWindowAttribute;
        const DWMWA_CAPTION_COLOR: u32 = 35;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &color as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    pub fn set_active(&mut self, hwnd: HWND) {
        unsafe {
            SetForegroundWindow(hwnd);
        }
    }
}
