#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;
    use tauri::WebviewWindow;
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW,
        GetMessageW, GetParent, GetWindowLongPtrW, GetWindowRect, IsWindow, RegisterClassW,
        RegisterWindowMessageW, SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent,
        SetWindowLongPtrW, SetWindowPos, TranslateMessage, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM,
        HWND_TOP, LWA_ALPHA, MSG, SMTO_NORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOWPOS, WM_NCDESTROY, WM_WINDOWPOSCHANGING,
        WNDCLASSW, WS_CHILD, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TRANSPARENT, WS_OVERLAPPED, WS_POPUP,
    };

    const SUBCLASS_ID_BOTTOM: usize = 0x4444_0001;

    unsafe extern "system" fn bottom_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        uid_subclass: usize,
        _dw_ref_data: usize,
    ) -> LRESULT {
        if msg == WM_WINDOWPOSCHANGING && lparam.0 != 0 {
            let wp = &mut *(lparam.0 as *mut WINDOWPOS);
            if (wp.flags & SWP_NOZORDER)
                == windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS(0)
            {
                wp.hwndInsertAfter = HWND_BOTTOM;
            }
        } else if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(bottom_subclass_proc), uid_subclass);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    unsafe extern "system" fn enum_windows_find_defview(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        if let Ok(defview) = FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None) {
            if !defview.0.is_null() {
                let target_ptr = lparam.0 as *mut HWND;
                *target_ptr = hwnd;
                return false.into();
            }
        }
        true.into()
    }

    /// `Progman` へ `0x052C` を送信し、壁紙レイヤーの `WorkerW` ウィンドウハンドルを取得する。
    pub fn find_workerw_hwnd() -> Option<HWND> {
        unsafe {
            let progman = FindWindowW(w!("Progman"), None).ok()?;
            if progman.0.is_null() {
                return None;
            }

            let mut result: usize = 0;
            let _ = SendMessageTimeoutW(
                progman,
                0x052C,
                WPARAM(0x0D),
                LPARAM(1),
                SMTO_NORMAL,
                1000,
                Some(&mut result),
            );

            let mut shell_parent = HWND::default();
            let _ = EnumWindows(
                Some(enum_windows_find_defview),
                LPARAM(&mut shell_parent as *mut HWND as isize),
            );

            if !shell_parent.0.is_null() {
                // Windows 10 / 11 (23H2以前): SHELLDLL_DefView を持つウィンドウの直後の兄弟 WorkerW
                if let Ok(workerw) = FindWindowExW(None, Some(shell_parent), w!("WorkerW"), None) {
                    if !workerw.0.is_null() {
                        return Some(workerw);
                    }
                }
            }

            // Windows 11 24H2 以降等: Progman 直下の WorkerW
            if let Ok(workerw) = FindWindowExW(Some(progman), None, w!("WorkerW"), None) {
                if !workerw.0.is_null() {
                    return Some(workerw);
                }
            }

            None
        }
    }

    /// 親子階層や `WS_CHILD` / `WS_POPUP` 切替前に `WS_EX_LAYERED` を解除する。
    unsafe fn clear_layered_ex_style(hwnd: HWND) {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if (ex_style & (WS_EX_LAYERED.0 as isize)) != 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !(WS_EX_LAYERED.0 as isize));
        }
    }

    /// モード A（最背面固定）を適用し、仮想スクリーン座標へ配置する。
    pub fn apply_bottom_mode_with_pos(
        window: &WebviewWindow,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let hwnd = window
            .hwnd()
            .map_err(|e| format!("failed to get HWND: {e}"))?;
        let hwnd = HWND(hwnd.0);

        unsafe {
            let current_parent = GetParent(hwnd).unwrap_or_default();
            if !current_parent.0.is_null() {
                clear_layered_ex_style(hwnd);
                let _ = SetParent(hwnd, None);
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let new_style = (style & !(WS_CHILD.0 as isize)) | (WS_POPUP.0 as isize);
                SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
            }

            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let new_ex_style =
                (ex_style & !(WS_EX_APPWINDOW.0 as isize)) | (WS_EX_TOOLWINDOW.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);

            let _ = SetWindowSubclass(hwnd, Some(bottom_subclass_proc), SUBCLASS_ID_BOTTOM, 0);

            SetWindowPos(
                hwnd,
                Some(HWND_BOTTOM),
                screen_x,
                screen_y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            )
            .map_err(|e| format!("SetWindowPos bottom failed: {e}"))?;
        }

        Ok(())
    }

    /// モード B（WorkerW 壁紙レイヤー埋め込み）を適用し、スクリーン座標→WorkerW クライアント座標へ変換配置する。
    pub fn apply_workerw_mode(
        window: &WebviewWindow,
        screen_x: i32,
        screen_y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let hwnd = window
            .hwnd()
            .map_err(|e| format!("failed to get HWND: {e}"))?;
        let hwnd = HWND(hwnd.0);

        let workerw = find_workerw_hwnd()
            .ok_or_else(|| "failed to locate WorkerW desktop layer".to_string())?;

        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(bottom_subclass_proc), SUBCLASS_ID_BOTTOM);

            let current_parent = GetParent(hwnd).unwrap_or_default();
            if current_parent != workerw {
                clear_layered_ex_style(hwnd);
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let new_style = (style & !(WS_POPUP.0 as isize)) | (WS_CHILD.0 as isize);
                SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);

                SetParent(hwnd, Some(workerw))
                    .map_err(|e| format!("SetParent(WorkerW) failed: {e}"))?;
            }

            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let new_ex_style =
                (ex_style & !(WS_EX_APPWINDOW.0 as isize)) | (WS_EX_TOOLWINDOW.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);

            let mut pt = POINT {
                x: screen_x,
                y: screen_y,
            };
            let _ = ScreenToClient(workerw, &mut pt);

            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                pt.x,
                pt.y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            )
            .map_err(|e| format!("SetWindowPos(WorkerW) failed: {e}"))?;
        }

        Ok(())
    }

    /// ウィンドウの現在の仮想スクリーン座標（物理ピクセル）を取得する。
    pub fn get_window_screen_rect(window: &WebviewWindow) -> Option<(i32, i32, u32, u32)> {
        let hwnd = window.hwnd().ok()?;
        let hwnd = HWND(hwnd.0);
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut rect).ok()?;
        }
        let w = (rect.right - rect.left).max(0) as u32;
        let h = (rect.bottom - rect.top).max(0) as u32;
        Some((rect.left, rect.top, w, h))
    }

    /// 指定ウィンドウが現在の有効な `WorkerW` にアタッチされているかを確認する。
    pub fn is_attached_to_current_workerw(window: &WebviewWindow) -> bool {
        let Ok(hwnd_raw) = window.hwnd() else {
            return false;
        };
        let hwnd = HWND(hwnd_raw.0);
        unsafe {
            let parent = GetParent(hwnd).unwrap_or_default();
            if parent.0.is_null() || !IsWindow(Some(parent)).as_bool() {
                return false;
            }
            if let Some(current_workerw) = find_workerw_hwnd() {
                parent == current_workerw
            } else {
                false
            }
        }
    }

    /// 透明度 (`WS_EX_LAYERED` + `LWA_ALPHA`)、クリック透過 (`WS_EX_TRANSPARENT`)、`WS_EX_NOACTIVATE` を適用する。
    pub fn apply_widget_styles(
        window: &WebviewWindow,
        opacity: f64,
        click_through: bool,
        no_activate: bool,
    ) -> Result<(), String> {
        let hwnd = window
            .hwnd()
            .map_err(|e| format!("failed to get HWND: {e}"))?;
        let hwnd = HWND(hwnd.0);
        let alpha = (opacity.clamp(0.2, 1.0) * 255.0).round() as u8;

        unsafe {
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            ex_style &= !(WS_EX_APPWINDOW.0 as isize);
            ex_style |= (WS_EX_TOOLWINDOW.0 as isize) | (WS_EX_LAYERED.0 as isize);

            if click_through {
                ex_style |= WS_EX_TRANSPARENT.0 as isize;
            } else {
                ex_style &= !(WS_EX_TRANSPARENT.0 as isize);
            }

            if no_activate {
                ex_style |= WS_EX_NOACTIVATE.0 as isize;
            } else {
                ex_style &= !(WS_EX_NOACTIVATE.0 as isize);
            }

            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);

            if SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).is_err() {
                clear_layered_ex_style(hwnd);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
                SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
                    .map_err(|e| format!("SetLayeredWindowAttributes failed: {e}"))?;
            }

            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
            )
            .map_err(|e| format!("SetWindowPos style refresh failed: {e}"))?;
        }

        Ok(())
    }

    static TASKBAR_CREATED_MSG: OnceLock<u32> = OnceLock::new();
    static REATTACH_CALLBACK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

    unsafe extern "system" fn listener_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if let Some(&tb_msg) = TASKBAR_CREATED_MSG.get() {
            if tb_msg != 0 && msg == tb_msg {
                if let Some(cb) = REATTACH_CALLBACK.get() {
                    cb();
                }
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// `TaskbarCreated` ブロードキャストを受信してコールバックを呼ぶ隠しトップレベルウィンドウを起動する。
    pub fn start_taskbar_created_listener<F>(on_reattach: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let _ = REATTACH_CALLBACK.set(Box::new(on_reattach));

        std::thread::spawn(|| unsafe {
            let tb_msg = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = TASKBAR_CREATED_MSG.set(tb_msg);

            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = w!("DesktopDashboardsTaskbarListener");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(listener_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);

            let _hwnd = CreateWindowExW(
                Default::default(),
                class_name,
                w!("DesktopDashboardsListener"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance.into()),
                None,
            );

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use tauri::WebviewWindow;

    pub fn apply_bottom_mode_with_pos(
        _window: &WebviewWindow,
        _screen_x: i32,
        _screen_y: i32,
        _width: u32,
        _height: u32,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn apply_workerw_mode(
        _window: &WebviewWindow,
        _screen_x: i32,
        _screen_y: i32,
        _width: u32,
        _height: u32,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn get_window_screen_rect(_window: &WebviewWindow) -> Option<(i32, i32, u32, u32)> {
        None
    }

    pub fn is_attached_to_current_workerw(_window: &WebviewWindow) -> bool {
        true
    }

    pub fn apply_widget_styles(
        _window: &WebviewWindow,
        _opacity: f64,
        _click_through: bool,
        _no_activate: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn start_taskbar_created_listener<F>(_on_reattach: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
    }
}

pub use imp::{
    apply_bottom_mode_with_pos, apply_widget_styles, apply_workerw_mode, get_window_screen_rect,
    is_attached_to_current_workerw, start_taskbar_created_listener,
};
