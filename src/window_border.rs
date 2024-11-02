use crate::logger::Logger;
use crate::utils::*;
use crate::*;
use std::ffi::c_ulong;
use std::fmt;
use std::os::raw::c_void;
use std::ptr::{null_mut, NonNull};
use std::sync::LazyLock;
use std::sync::OnceLock;
use windows::{
    core::*, Foundation::Numerics::*, Win32::Foundation::*, Win32::Graphics::Direct2D::Common::*,
    Win32::Graphics::Direct2D::*, Win32::Graphics::Dwm::*, Win32::Graphics::Dxgi::Common::*,
    Win32::Graphics::Gdi::*, Win32::UI::HiDpi::*, Win32::UI::WindowsAndMessaging::*,
};

pub static RENDER_FACTORY: LazyLock<ID2D1Factory> = unsafe {
    LazyLock::new(|| {
        D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_MULTI_THREADED, None)
            .expect("creating RENDER_FACTORY failed")
    })
};

#[derive(Debug, Default)]
pub struct WindowBorder {
    pub border_window: HWND,
    pub tracking_window: HWND,
    pub window_rect: RECT,
    pub border_size: i32,
    pub border_offset: i32,
    pub border_radius: f32,
    pub render_target_properties: D2D1_RENDER_TARGET_PROPERTIES,
    pub hwnd_render_target_properties: D2D1_HWND_RENDER_TARGET_PROPERTIES,
    pub brush_properties: D2D1_BRUSH_PROPERTIES,
    pub render_target: OnceLock<ID2D1HwndRenderTarget>,
    pub rounded_rect: D2D1_ROUNDED_RECT,
    pub active_color: Color,
    pub inactive_color: Color,
    pub current_color: Color,
    pub pause: bool,
    pub active_gradient_angle: f32,
    pub inactive_gradient_angle: f32,
    pub last_render_time_active: Option<std::time::Instant>,
    pub last_render_time_inactive: Option<std::time::Instant>,
    pub use_active_animation: bool,
    pub use_inactive_animation: bool,
}

impl WindowBorder {
    pub fn create_border_window(&mut self, hinstance: HINSTANCE) -> Result<()> {
        unsafe {
            self.border_window = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                w!("tacky-border"),
                w!("tacky-border"),
                WS_POPUP | WS_DISABLED,
                0,
                0,
                0,
                0,
                None,
                None,
                hinstance,
                Some(std::ptr::addr_of!(*self) as *const _),
            )?;
        }

        Ok(())
    }

    pub fn init(&mut self, hinstance: HINSTANCE) -> Result<()> {
        unsafe {
            // Make the window border transparent
            let pos: i32 = -GetSystemMetrics(SM_CXVIRTUALSCREEN) - 8;
            let hrgn = CreateRectRgn(pos, 0, pos + 1, 1);
            let mut bh: DWM_BLURBEHIND = Default::default();
            if !hrgn.is_invalid() {
                bh = DWM_BLURBEHIND {
                    dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
                    fEnable: TRUE,
                    hRgnBlur: hrgn,
                    fTransitionOnMaximized: FALSE,
                };
            }

            let _ = DwmEnableBlurBehindWindow(self.border_window, &bh);
            if SetLayeredWindowAttributes(self.border_window, COLORREF(0x00000000), 0, LWA_COLORKEY)
                .is_err()
            {
                println!("Error Setting Layered Window Attributes!");
            }
            if SetLayeredWindowAttributes(self.border_window, COLORREF(0x00000000), 255, LWA_ALPHA)
                .is_err()
            {
                println!("Error Setting Layered Window Attributes!");
            }

            let _ = self.create_render_targets();
            if has_native_border(self.tracking_window) {
                let _ = self.update_position(Some(SWP_SHOWWINDOW));
                let _ = self.render();

                // Sometimes, it doesn't show the window at first, so we wait 5ms and update it.
                // This is very hacky and needs to be looked into. It may be related to the issue
                // detailed in update_window_rect. TODO
                /*std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = self.update_position(Some(SWP_SHOWWINDOW));
                let _ = self.render();*/
            }

            let mut message = MSG::default();
            while GetMessageW(&mut message, HWND::default(), 0, 0).into() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        return Ok(());
    }

    pub fn create_render_targets(&mut self) -> Result<()> {
        self.render_target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        self.active_gradient_angle = 0.0;
        self.inactive_gradient_angle = 0.0;
        self.last_render_time_active = Some(std::time::Instant::now());
        self.last_render_time_inactive = Some(std::time::Instant::now());
        self.hwnd_render_target_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.border_window,
            pixelSize: Default::default(),
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        self.brush_properties = D2D1_BRUSH_PROPERTIES {
            opacity: 1.0 as f32,
            transform: Matrix3x2::identity(),
        };

        // Create a rounded_rect with radius depending on the force_border_radius variable
        let mut border_radius = 0.0;
        let mut corner_preference = DWM_WINDOW_CORNER_PREFERENCE::default();
        let dpi = unsafe { GetDpiForWindow(self.tracking_window) } as f32;
        if self.border_radius == -1.0 {
            let result = unsafe {
                DwmGetWindowAttribute(
                    self.tracking_window,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    std::ptr::addr_of_mut!(corner_preference) as *mut _,
                    size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                )
            };
            if result.is_err() {
                Logger::log("error", "Error getting window corner preference!");
            }
            match corner_preference {
                DWMWCP_DEFAULT => {
                    border_radius = 8.0 * dpi / 96.0 + ((self.border_size / 2) as f32)
                }
                DWMWCP_DONOTROUND => border_radius = 0.0,
                DWMWCP_ROUND => border_radius = 8.0 * dpi / 96.0 + ((self.border_size / 2) as f32),
                DWMWCP_ROUNDSMALL => {
                    border_radius = 4.0 * dpi / 96.0 + ((self.border_size / 2) as f32)
                }
                _ => {}
            }
        } else {
            border_radius = self.border_radius * dpi / 96.0;
        }

        self.rounded_rect = D2D1_ROUNDED_RECT {
            rect: Default::default(),
            radiusX: border_radius,
            radiusY: border_radius,
        };

        // Initialize the actual border color assuming it is in focus
        self.current_color = self.active_color.clone();

        unsafe {
            let factory = &*RENDER_FACTORY;
            let _ = self.render_target.set(
                factory
                    .CreateHwndRenderTarget(
                        &self.render_target_properties,
                        &self.hwnd_render_target_properties,
                    )
                    .expect("creating self.render_target failed"),
            );
            let render_target = self.render_target.get().unwrap();
            render_target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }

        let _ = self.update_color();
        let _ = self.update_window_rect();
        let _ = self.update_position(None);
        let _ = self.create_animation_thread();

        return Ok(());
    }

    pub fn create_animation_thread(&self) -> Result<()> {
        let active = unsafe { GetForegroundWindow() } == self.tracking_window;

        if self.use_active_animation || self.use_inactive_animation {
            let window_sent: SendHWND = SendHWND(self.border_window);
            std::thread::spawn(move || loop {
                let window = window_sent.clone().0;
                if is_window_visible(window) {
                    unsafe {
                        let _ = PostMessageW(window, WM_PAINT, WPARAM(0), LPARAM(0));
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            });
        }

        return Ok(());
    }

    pub fn update_window_rect(&mut self) -> Result<()> {
        let result = unsafe {
            DwmGetWindowAttribute(
                self.tracking_window,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                std::ptr::addr_of_mut!(self.window_rect) as *mut _,
                size_of::<RECT>() as u32,
            )
        };
        if result.is_err() {
            Logger::log("error", "Error getting frame rect!");
            unsafe {
                let _ = ShowWindow(self.border_window, SW_HIDE);
            }
        }

        self.window_rect.top -= self.border_size;
        self.window_rect.left -= self.border_size;
        self.window_rect.right += self.border_size;
        self.window_rect.bottom += self.border_size;

        return Ok(());
    }

    pub fn update_position(&mut self, c_flags: Option<SET_WINDOW_POS_FLAGS>) -> Result<()> {
        unsafe {
            // Place the window border above the tracking window
            let mut hwnd_above_tracking = GetWindow(self.tracking_window, GW_HWNDPREV);
            let custom_flags = match c_flags {
                Some(flags) => flags,
                None => SET_WINDOW_POS_FLAGS::default(),
            };
            let mut u_flags = SWP_NOSENDCHANGING | SWP_NOACTIVATE | SWP_NOREDRAW | custom_flags;

            // If hwnd_above_tracking is the window border itself, we have what we want and there's
            //  no need to change the z-order (plus it results in an error if we try it).
            // If hwnd_above_tracking returns an error, it's likely that tracking_window is already
            //  the highest in z-order, so we use HWND_TOP to place the window border above.
            if hwnd_above_tracking == Ok(self.border_window) {
                u_flags = u_flags | SWP_NOZORDER;
            } else if hwnd_above_tracking.is_err() {
                hwnd_above_tracking = Ok(HWND_TOP);
            }

            let result = SetWindowPos(
                self.border_window,
                hwnd_above_tracking.unwrap(),
                self.window_rect.left,
                self.window_rect.top,
                self.window_rect.right - self.window_rect.left,
                self.window_rect.bottom - self.window_rect.top,
                u_flags,
            );
            if result.is_err() {
                println!("Error setting window pos!");
                let _ = ShowWindow(self.border_window, SW_HIDE);
            }
        }
        return Ok(());
    }

    pub fn update_color(&mut self) -> Result<()> {
        if unsafe { GetForegroundWindow() } == self.tracking_window {
            self.current_color = self.active_color.clone();
        } else {
            self.current_color = self.inactive_color.clone();
        }

        return Ok(());
    }

    pub fn create_brush(&self, render_target: &ID2D1RenderTarget) -> Result<ID2D1Brush> {
        match &self.current_color {
            Color::Solid(color) => {
                let solid_brush = unsafe {
                    render_target.CreateSolidColorBrush(color, Some(&self.brush_properties))?
                };

                Ok(solid_brush.into())
            }
            Color::Gradient(color) => {
                let gradient_stops = color.gradient_stops.clone();
                let gradient_stop_collection: ID2D1GradientStopCollection = unsafe {
                    render_target.CreateGradientStopCollection(
                        &gradient_stops,
                        D2D1_GAMMA_2_2,
                        D2D1_EXTEND_MODE_CLAMP,
                    )?
                };

                let width = get_rect_width(self.window_rect) as f32;
                let height = get_rect_width(self.window_rect) as f32;

                let mut start_point = D2D_POINT_2F::default();
                let mut end_point = D2D_POINT_2F::default();

                let active = unsafe { GetForegroundWindow() } == self.tracking_window;

                let condition = if active {
                    self.use_active_animation
                } else {
                    self.use_inactive_animation
                };

                if condition {
                    let gradient_angle = if active {
                        self.active_gradient_angle
                    } else {
                        self.inactive_gradient_angle
                    };
                    let center_x = width / 2.0;
                    let center_y = height / 2.0;
                    let radius = (center_x.powi(2) + center_y.powi(2)).sqrt();

                    let angle_rad = gradient_angle.to_radians();
                    let (sin, cos) = angle_rad.sin_cos();
                    start_point = D2D_POINT_2F {
                        x: center_x - radius * cos,
                        y: center_y - radius * sin,
                    };
                    end_point = D2D_POINT_2F {
                        x: center_x + radius * cos,
                        y: center_y + radius * sin,
                    };
                } else {
                    let (start_x, start_y, end_x, end_y) = match color.direction.clone() {
                        Some(coords) => (
                            coords[0] * width,
                            coords[1] * height,
                            coords[2] * width,
                            coords[3] * height,
                        ), // Use coordinates if they exist
                        None => (0.0 * width, 0.0 * height, 1.0 * width, 1.0 * height),
                    };

                    start_point = D2D_POINT_2F {
                        x: start_x,
                        y: start_y,
                    };

                    end_point = D2D_POINT_2F { x: end_x, y: end_y };
                }

                let gradient_properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: start_point,
                    endPoint: end_point,
                };

                let gradient_brush = unsafe {
                    render_target.CreateLinearGradientBrush(
                        &gradient_properties,
                        Some(&self.brush_properties),
                        Some(&gradient_stop_collection),
                    )?
                };

                Ok(gradient_brush.into())
            }
        }
    }

    pub fn render(&mut self) -> Result<()> {
        // Get the render target
        let render_target = match self.render_target.get() {
            Some(rt) => rt,
            None => return Ok(()), // Return early if there is no render target
        };

        self.hwnd_render_target_properties.pixelSize = D2D_SIZE_U {
            width: (self.window_rect.right - self.window_rect.left) as u32,
            height: (self.window_rect.bottom - self.window_rect.top) as u32,
        };

        self.rounded_rect.rect = D2D_RECT_F {
            left: (self.border_size / 2 - self.border_offset) as f32,
            top: (self.border_size / 2 - self.border_offset) as f32,
            right: (self.window_rect.right - self.window_rect.left - self.border_size / 2
                + self.border_offset) as f32,
            bottom: (self.window_rect.bottom - self.window_rect.top - self.border_size / 2
                + self.border_offset) as f32,
        };

        unsafe {
            render_target.Resize(&self.hwnd_render_target_properties.pixelSize as *const _);

            let now = std::time::Instant::now();
            let active = unsafe { GetForegroundWindow() } == self.tracking_window;
            let last_render_time = if active { self.last_render_time_active } else { self.last_render_time_inactive };
            let elapsed = now
                .duration_since(last_render_time.unwrap_or(now))
                .as_secs_f32();
            if self.use_active_animation && active {
                self.last_render_time_active = Some(now);
                self.active_gradient_angle += 360.0 * elapsed;
                if self.active_gradient_angle > 360.0 {
                    self.active_gradient_angle -= 360.0;
                }
            } else if self.use_inactive_animation && !active {
                self.last_render_time_inactive = Some(now);
                self.inactive_gradient_angle += 360.0 * elapsed;
                if self.inactive_gradient_angle > 360.0 {
                    self.inactive_gradient_angle -= 360.0;
                }
            }

            // render_target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let brush = self.create_brush(render_target)?;

            render_target.BeginDraw();
            render_target.Clear(None);
            render_target.DrawRoundedRectangle(
                &self.rounded_rect,
                &brush,
                self.border_size as f32,
                None,
            );
            render_target.EndDraw(None, None);
            let _ = InvalidateRect(self.border_window, None, false);
        }

        Ok(())
    }

    // When CreateWindowExW is called, we can optionally pass a value to its LPARAM field which will
    // get sent to the window process on creation. In our code, we've passed a pointer to the
    // WindowBorder structure during the window creation process, and here we are getting that pointer
    // and attaching it to the window using SetWindowLongPtrW.
    pub unsafe extern "system" fn s_wnd_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let mut border_pointer: *mut WindowBorder = GetWindowLongPtrW(window, GWLP_USERDATA) as _;

        if border_pointer == std::ptr::null_mut() && message == WM_CREATE {
            //println!("ref is null, assigning new ref");
            let create_struct: *mut CREATESTRUCTW = lparam.0 as *mut _;
            border_pointer = (*create_struct).lpCreateParams as *mut _;
            SetWindowLongPtrW(window, GWLP_USERDATA, border_pointer as _);
        }
        match border_pointer != std::ptr::null_mut() {
            true => return Self::wnd_proc(&mut *border_pointer, window, message, wparam, lparam),
            false => return DefWindowProcW(window, message, wparam, lparam),
        }
    }

    pub unsafe fn wnd_proc(
        &mut self,
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            5000 => {
                if self.pause
                    || is_cloaked(self.tracking_window)
                    || !is_window_visible(self.tracking_window)
                {
                    return LRESULT(0);
                }

                if !has_native_border(self.tracking_window) {
                    let _ = self.update_position(Some(SWP_HIDEWINDOW));
                    return LRESULT(0);
                } else if !is_window_visible(self.border_window) {
                    let _ = self.update_position(Some(SWP_SHOWWINDOW));
                }

                let old_rect = self.window_rect.clone();
                let _ = self.update_window_rect();
                let _ = self.update_position(None);

                // When a window is minimized, all four of these points go way below 0 and we end
                // up with a weird rect that we don't want. So, we just swap out with old_rect.
                if self.window_rect.top <= 0
                    && self.window_rect.left <= 0
                    && self.window_rect.right <= 0
                    && self.window_rect.bottom <= 0
                {
                    self.window_rect = old_rect;
                    return LRESULT(0);
                }

                // Only re-render the border when its size changes
                if get_rect_width(self.window_rect) != get_rect_width(old_rect)
                    || get_rect_height(self.window_rect) != get_rect_height(old_rect)
                {
                    let _ = self.render();
                }
            }
            // EVENT_OBJECT_REORDER
            5001 => {
                if self.pause
                    || is_cloaked(self.tracking_window)
                    || !is_window_visible(self.tracking_window)
                {
                    return LRESULT(0);
                }

                let _ = self.update_color();
                let _ = self.update_position(None);
                let _ = self.render();
            }
            // EVENT_OBJECT_SHOW / EVENT_OBJECT_UNCLOAKED
            5002 => {
                if has_native_border(self.tracking_window) {
                    let _ = self.update_window_rect();
                    let _ = self.update_position(Some(SWP_SHOWWINDOW));
                    let _ = self.render();
                }
                self.pause = false;
            }
            // EVENT_OBJECT_HIDE / EVENT_OBJECT_CLOAKED
            5003 => {
                let _ = self.update_position(Some(SWP_HIDEWINDOW));
                self.pause = true;
            }
            // EVENT_OBJECT_MINIMIZESTART
            5004 => {
                let _ = self.update_position(Some(SWP_HIDEWINDOW));
                self.pause = true;
            }
            // EVENT_SYSTEM_MINIMIZEEND
            // When a window is about to be unminimized, hide the border and let the thread sleep
            // for 200ms to wait for the window animation to finish, then show the border.
            5005 => {
                std::thread::sleep(std::time::Duration::from_millis(200));

                if has_native_border(self.tracking_window) {
                    let _ = self.update_window_rect();
                    let _ = self.update_position(Some(SWP_SHOWWINDOW));
                    let _ = self.render();
                }
                self.pause = false;
            }
            WM_PAINT => {
                let _ = self.render();
                ValidateRect(window, None);
                // Schedule next frame
                let _ = SetTimer(window, 1, 16, None); // ~60 FPS
            }
            WM_DESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                PostQuitMessage(0);
            }
            // Ignore these window position messages
            WM_WINDOWPOSCHANGING => {}
            WM_WINDOWPOSCHANGED => {}
            _ => {
                return DefWindowProcW(window, message, wparam, lparam);
            }
        }
        LRESULT(0)
    }
}
