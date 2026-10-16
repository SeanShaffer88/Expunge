//! The toolkit on a plain Win32 window: a DIB section as the frame buffer,
//! GDI for text (ClearType, Windows' font linking for Chinese, Japanese and
//! Korean), and Windows messages as input.
//!
//! Frames are drawn only on input, on a timer a widget asked for, or when
//! another thread calls `request_repaint`; between them the thread sleeps
//! in `GetMessageW`.

use super::context::{Context, FullOutput, ViewportCommand};
use super::geom::*;
use super::raster::Canvas;
use super::text::{FontFamily, FontKey, TextBackend};
use super::*;
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::*;
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::Ime::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Text through GDI
// ---------------------------------------------------------------------------

/// Owns a memory DC (with the frame buffer selected) and a font per size.
pub struct GdiText {
    dc: HDC,
    fonts: HashMap<FontKey, HFONT>,
    current: Option<FontKey>,
    utf16: Vec<u16>,
    dx: Vec<i32>,
    /// Scratch bitmap for text drawn semi-transparent.
    mask: Option<(HDC, HBITMAP, *mut u32, i32, i32)>,
}

// SAFETY: GDI objects are only used on the window's thread; the Context
// that holds this is shared across threads only to request repaints.
unsafe impl Send for GdiText {}

impl GdiText {
    fn new(dc: HDC) -> GdiText {
        GdiText {
            dc,
            fonts: HashMap::new(),
            current: None,
            utf16: Vec::new(),
            dx: Vec::new(),
            mask: None,
        }
    }

    fn font(&mut self, key: FontKey) -> HFONT {
        *self.fonts.entry(key).or_insert_with(|| {
            let face = wide(match key.family {
                FontFamily::Proportional => "Segoe UI",
                FontFamily::Monospace => "Consolas",
            });
            // SAFETY: plain font creation; the handle is deleted in Drop.
            unsafe {
                CreateFontW(
                    -(key.px().round() as i32),
                    0,
                    0,
                    0,
                    if key.bold {
                        FW_SEMIBOLD as i32
                    } else {
                        FW_NORMAL as i32
                    },
                    key.italic as u32,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_TT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    (DEFAULT_PITCH | FF_DONTCARE) as u32,
                    face.as_ptr(),
                )
            }
        })
    }

    fn select(&mut self, dc: HDC, key: FontKey) {
        if self.current != Some(key) || dc != self.dc {
            let f = self.font(key);
            // SAFETY: selecting a live font into a live DC.
            unsafe { SelectObject(dc, f as HGDIOBJ) };
            if dc == self.dc {
                self.current = Some(key);
            }
        }
    }

    /// A white-on-black bitmap of at least w×h for coverage masks.
    fn mask_bitmap(&mut self, w: i32, h: i32) -> Option<(HDC, *mut u32)> {
        let fits = matches!(self.mask, Some((_, _, _, mw, mh)) if mw >= w && mh >= h);
        if !fits {
            if let Some((dc, bmp, _, _, _)) = self.mask.take() {
                // SAFETY: releasing objects we created.
                unsafe {
                    DeleteDC(dc);
                    DeleteObject(bmp as HGDIOBJ);
                }
            }
            let (w, h) = (w.max(64), h.max(32));
            let (bmp, bits) = dib(w, h)?;
            // SAFETY: a memory DC for our own bitmap.
            let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
            unsafe { SelectObject(dc, bmp as HGDIOBJ) };
            self.mask = Some((dc, bmp, bits, w, h));
            self.current = None;
        }
        self.mask.map(|(dc, _, bits, _, _)| (dc, bits))
    }
}

impl Drop for GdiText {
    fn drop(&mut self) {
        // SAFETY: deleting objects we created.
        unsafe {
            for f in self.fonts.values() {
                DeleteObject(*f as HGDIOBJ);
            }
            if let Some((dc, bmp, _, _, _)) = self.mask.take() {
                DeleteDC(dc);
                DeleteObject(bmp as HGDIOBJ);
            }
        }
    }
}

impl TextBackend for GdiText {
    fn advances(&mut self, text: &str, font: FontKey, out: &mut Vec<f32>) {
        if text.is_empty() {
            return;
        }
        self.select(self.dc, font);
        self.utf16.clear();
        self.utf16.extend(text.encode_utf16());
        self.dx.clear();
        self.dx.resize(self.utf16.len(), 0);
        let mut size = SIZE { cx: 0, cy: 0 };
        // SAFETY: buffers sized to the string.
        unsafe {
            GetTextExtentExPointW(
                self.dc,
                self.utf16.as_ptr(),
                self.utf16.len() as i32,
                0,
                std::ptr::null_mut(),
                self.dx.as_mut_ptr(),
                &mut size,
            );
        }
        // One entry per char: skip the second half of surrogate pairs.
        let mut i = 0;
        for c in text.chars() {
            i += c.len_utf16();
            out.push(self.dx[i - 1] as f32);
        }
    }

    fn metrics(&mut self, font: FontKey) -> (f32, f32) {
        self.select(self.dc, font);
        let mut tm: TEXTMETRICW = unsafe { std::mem::zeroed() };
        // SAFETY: filling a local struct.
        unsafe { GetTextMetricsW(self.dc, &mut tm) };
        (
            tm.tmAscent as f32,
            (tm.tmHeight + tm.tmExternalLeading) as f32,
        )
    }

    fn draw(&mut self, canvas: &mut Canvas, text: &str, font: FontKey, pos: Pos2, color: Color32) {
        if color.a == 0 {
            return;
        }
        self.utf16.clear();
        self.utf16.extend(text.encode_utf16());
        let n = self.utf16.len() as i32;
        let clip = canvas.clip();
        if color.a == 255 {
            self.select(self.dc, font);
            let r = RECT {
                left: clip.min.x as i32,
                top: clip.min.y as i32,
                right: clip.max.x as i32,
                bottom: clip.max.y as i32,
            };
            // SAFETY: drawing into our DIB through our DC; flushed before the
            // rasteriser touches the pixels again.
            unsafe {
                SetTextColor(
                    self.dc,
                    (color.b as u32) << 16 | (color.g as u32) << 8 | color.r as u32,
                );
                SetBkMode(self.dc, TRANSPARENT as i32);
                ExtTextOutW(
                    self.dc,
                    pos.x as i32,
                    pos.y as i32,
                    ETO_CLIPPED,
                    &r,
                    self.utf16.as_ptr(),
                    n as u32,
                    std::ptr::null(),
                );
                GdiFlush();
            }
            return;
        }
        // Semi-transparent: draw white on black, use it as coverage.
        let mut adv = Vec::new();
        self.advances(text, font, &mut adv);
        let (_, lh) = self.metrics(font);
        let w = adv.last().copied().unwrap_or(0.0).ceil() as i32 + 2;
        let h = lh.ceil() as i32 + 1;
        let utf16 = std::mem::take(&mut self.utf16);
        let Some((mdc, bits)) = self.mask_bitmap(w, h) else {
            self.utf16 = utf16;
            return;
        };
        let mw = self.mask.map_or(0, |m| m.3);
        self.select(mdc, font);
        // SAFETY: our scratch DC and bitmap, sized above.
        let coverage: Vec<u8> = unsafe {
            let r = RECT {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
            let black = GetStockObject(BLACK_BRUSH);
            FillRect(mdc, &r, black as HBRUSH);
            SetTextColor(mdc, 0x00FF_FFFF);
            SetBkMode(mdc, TRANSPARENT as i32);
            ExtTextOutW(
                mdc,
                0,
                0,
                0,
                std::ptr::null(),
                utf16.as_ptr(),
                utf16.len() as u32,
                std::ptr::null(),
            );
            GdiFlush();
            let mut cov = Vec::with_capacity((w * h) as usize);
            for y in 0..h {
                for x in 0..w {
                    let v = *bits.add((y * mw + x) as usize);
                    // Average of the ClearType sub-pixels.
                    cov.push(
                        (((v >> 16) & 0xFF) + ((v >> 8) & 0xFF) + (v & 0xFF)).div_ceil(3) as u8,
                    );
                }
            }
            cov
        };
        self.utf16 = utf16;
        canvas.mask(
            &coverage,
            w as usize,
            h as usize,
            pos.x as i32,
            pos.y as i32,
            color,
        );
    }
}

/// A top-down 32-bit DIB section.
fn dib(w: i32, h: i32) -> Option<(HBITMAP, *mut u32)> {
    let mut bi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: standard DIB creation; `bits` receives the pixel memory.
    let bmp = unsafe {
        CreateDIBSection(
            std::ptr::null_mut(),
            &bi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        )
    };
    (!bmp.is_null() && !bits.is_null()).then_some((bmp, bits as *mut u32))
}

// ---------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------

/// The program's side of the loop.
pub trait App {
    /// Build one frame.
    fn update(&mut self, ctx: &Context);
    /// Look at (and filter) input before the frame sees it.
    fn raw_input_hook(&mut self, _ctx: &Context, _raw: &mut RawInput) {}
    /// Background colour behind everything.
    fn clear_color(&self) -> Color32 {
        Color32::from_rgb(20, 17, 19)
    }
}

pub struct NativeOptions {
    pub title: String,
    pub inner_size: Vec2,
    pub min_inner_size: Vec2,
    pub icon: Option<IconData>,
}

const WM_APP_WAKE: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

thread_local! {
    /// A frame is being built. Modal dialogs and some window calls run a
    /// message loop inside it; messages arriving then wait for the frame.
    static IN_FRAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static QUEUED: std::cell::RefCell<Vec<(u32, WPARAM, LPARAM)>> = const { std::cell::RefCell::new(Vec::new()) };
}
const TIMER_ID: usize = 1;

struct Window {
    hwnd: HWND,
    ctx: Context,
    app: Box<dyn App>,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    size: (i32, i32),
    events: Vec<Event>,
    start: Instant,
    min_size: Vec2,
    high_surrogate: Option<u16>,
    cursor: CursorIcon,
    tracking: bool,
    timer_at: Option<Instant>,
    closing: bool,
    ime_rect: Option<Rect>,
}

impl Window {
    fn ppp(&self) -> f32 {
        // SAFETY: valid window handle.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        if dpi == 0 {
            1.0
        } else {
            dpi as f32 / 96.0
        }
    }

    fn modifiers() -> Modifiers {
        // SAFETY: reading keyboard state of this thread.
        let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk as i32) } < 0;
        let ctrl = down(VK_CONTROL);
        Modifiers {
            alt: down(VK_MENU),
            ctrl,
            shift: down(VK_SHIFT),
            command: ctrl,
        }
    }

    fn point(&self, lparam: LPARAM) -> Pos2 {
        let x = (lparam & 0xFFFF) as i16 as f32;
        let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
        let z = self.ctx.pixels_per_point();
        pos2(x / z, y / z)
    }

    fn wake(&mut self) {
        // SAFETY: valid window handle.
        unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
    }

    fn ensure_buffer(&mut self, w: i32, h: i32) {
        if self.size == (w, h) && !self.bits.is_null() {
            return;
        }
        if let Some((bmp, bits)) = dib(w.max(1), h.max(1)) {
            // SAFETY: swapping our bitmap in the memory DC and deleting the old one.
            unsafe {
                let old = SelectObject(self.dc, bmp as HGDIOBJ);
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap as HGDIOBJ);
                } else {
                    let _ = old;
                }
            }
            self.bitmap = bmp;
            self.bits = bits;
            self.size = (w.max(1), h.max(1));
        }
    }

    fn frame(&mut self) {
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: valid window handle.
        unsafe { GetClientRect(self.hwnd, &mut rc) };
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return;
        }
        self.ensure_buffer(w, h);
        let native = self.ppp();
        let zoom = self.ctx.zoom_factor();
        let z = native * zoom;
        let mut raw = RawInput {
            events: std::mem::take(&mut self.events),
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                vec2(w as f32 / z, h as f32 / z),
            )),
            time: Some(self.start.elapsed().as_secs_f64()),
            modifiers: Self::modifiers(),
            pixels_per_point: Some(native),
        };
        self.app.raw_input_hook(&self.ctx, &mut raw);
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        IN_FRAME.with(|f| f.set(true));
        let out = ctx.run(raw, |c| app.update(c));
        IN_FRAME.with(|f| f.set(false));
        // SAFETY: `bits` covers w×h pixels of the DIB selected into `dc`.
        let px = unsafe {
            std::slice::from_raw_parts_mut(self.bits, (self.size.0 * self.size.1) as usize)
        };
        let mut canvas = Canvas::new(px, self.size.0 as usize, self.size.1 as usize);
        ctx.paint(&out, &mut canvas, self.app.clear_color());
        // SAFETY: copying our buffer to the window.
        unsafe {
            let wdc = GetDC(self.hwnd);
            BitBlt(wdc, 0, 0, w, h, self.dc, 0, 0, SRCCOPY);
            ReleaseDC(self.hwnd, wdc);
        }
        self.after_frame(out);
        // Input that arrived during the frame (inside a dialog's loop).
        let queued = QUEUED.with(|q| std::mem::take(&mut *q.borrow_mut()));
        for (msg, w, l) in queued {
            self.handle(msg, w, l);
        }
    }

    fn after_frame(&mut self, out: FullOutput) {
        if out.cursor != self.cursor {
            self.cursor = out.cursor;
            set_cursor(self.cursor);
        }
        if !out.copied_text.is_empty() {
            set_clipboard(self.hwnd, &out.copied_text);
        }
        if let Some(url) = out.open_url {
            let op = wide("open");
            let u = wide(&url);
            // SAFETY: opening a URL with the default handler.
            unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    op.as_ptr(),
                    u.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
        }
        self.ime_rect = out.ime_rect;
        for c in out.commands {
            match c {
                // SAFETY: posting to our own window.
                ViewportCommand::Close => unsafe {
                    PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                },
                ViewportCommand::Title(t) => {
                    let w = wide(&t);
                    unsafe { SetWindowTextW(self.hwnd, w.as_ptr()) };
                }
                ViewportCommand::Focus => unsafe {
                    SetForegroundWindow(self.hwnd);
                },
                ViewportCommand::Minimized(m) => unsafe {
                    ShowWindow(self.hwnd, if m { SW_MINIMIZE } else { SW_RESTORE });
                },
            }
        }
        // Schedule the next frame: now-ish, after a delay, or only on input.
        let wanted = if self.ctx.take_repaint_request() {
            Some(Duration::ZERO)
        } else {
            out.repaint_after
        };
        match wanted {
            Some(d) => {
                // At most about 60 frames a second.
                let d = d.max(Duration::from_millis(16));
                let at = Instant::now() + d;
                if self.timer_at.is_none_or(|t| at < t || t <= Instant::now()) {
                    self.timer_at = Some(at);
                    // SAFETY: (re)arming our timer.
                    unsafe { SetTimer(self.hwnd, TIMER_ID, d.as_millis().max(1) as u32, None) };
                }
            }
            None => {
                if self.timer_at.take().is_some() {
                    unsafe { KillTimer(self.hwnd, TIMER_ID) };
                }
            }
        }
    }

    fn place_ime(&self) {
        let Some(r) = self.ime_rect else {
            return;
        };
        let z = self.ctx.pixels_per_point();
        // SAFETY: IME context of our own window.
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.is_null() {
                return;
            }
            let pt = POINT {
                x: (r.min.x * z) as i32,
                y: (r.min.y * z) as i32,
            };
            let cf = COMPOSITIONFORM {
                dwStyle: CFS_POINT,
                ptCurrentPos: pt,
                rcArea: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
            };
            ImmSetCompositionWindow(himc, &cf);
            let cand = CANDIDATEFORM {
                dwIndex: 0,
                dwStyle: CFS_EXCLUDE,
                ptCurrentPos: POINT {
                    x: pt.x,
                    y: (r.max.y * z) as i32,
                },
                rcArea: RECT {
                    left: pt.x,
                    top: pt.y,
                    right: pt.x + 1,
                    bottom: (r.max.y * z) as i32,
                },
            };
            ImmSetCandidateWindow(himc, &cand);
            ImmReleaseContext(self.hwnd, himc);
        }
    }

    fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let m = Self::modifiers();
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
                // SAFETY: standard paint bracket.
                unsafe { BeginPaint(self.hwnd, &mut ps) };
                self.frame();
                unsafe { EndPaint(self.hwnd, &ps) };
                return Some(0);
            }
            WM_ERASEBKGND => return Some(1),
            WM_TIMER if wparam == TIMER_ID => {
                self.timer_at = None;
                unsafe { KillTimer(self.hwnd, TIMER_ID) };
                self.wake();
                return Some(0);
            }
            WM_APP_WAKE => {
                self.wake();
                return Some(0);
            }
            WM_SIZE => self.wake(),
            WM_GETMINMAXINFO => {
                let z = self.ppp();
                // SAFETY: lparam points to a MINMAXINFO for this message.
                let mmi = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                mmi.ptMinTrackSize.x = (self.min_size.x * z) as i32;
                mmi.ptMinTrackSize.y = (self.min_size.y * z) as i32;
                return Some(0);
            }
            WM_DPICHANGED => {
                // SAFETY: lparam points to the suggested window rectangle.
                let r = unsafe { &*(lparam as *const RECT) };
                unsafe {
                    SetWindowPos(
                        self.hwnd,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                self.wake();
                return Some(0);
            }
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: asking for WM_MOUSELEAVE.
                    unsafe { TrackMouseEvent(&mut tme) };
                    self.tracking = true;
                }
                let p = self.point(lparam);
                self.events.push(Event::PointerMoved(p));
                self.wake();
                return Some(0);
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.events.push(Event::PointerGone);
                self.wake();
            }
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
            | WM_MBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK => {
                let (button, pressed) = match msg {
                    WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => (PointerButton::Primary, true),
                    WM_LBUTTONUP => (PointerButton::Primary, false),
                    WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => (PointerButton::Secondary, true),
                    WM_RBUTTONUP => (PointerButton::Secondary, false),
                    WM_MBUTTONDOWN => (PointerButton::Middle, true),
                    _ => (PointerButton::Middle, false),
                };
                // SAFETY: capture so a drag continues outside the window.
                unsafe {
                    if pressed {
                        SetCapture(self.hwnd);
                    } else {
                        ReleaseCapture();
                    }
                }
                let pos = self.point(lparam);
                self.events.push(Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: m,
                });
                self.wake();
                return Some(0);
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let notches = ((wparam >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                // Windows' "lines per notch" setting.
                let mut lines: u32 = 3;
                unsafe {
                    SystemParametersInfoW(
                        SPI_GETWHEELSCROLLLINES,
                        0,
                        &mut lines as *mut u32 as *mut c_void,
                        0,
                    )
                };
                let step = 16.0 * lines.clamp(1, 20) as f32;
                let delta = if msg == WM_MOUSEWHEEL {
                    if m.shift {
                        vec2(notches * step, 0.0)
                    } else {
                        vec2(0.0, notches * step)
                    }
                } else {
                    vec2(-notches * step, 0.0)
                };
                if m.ctrl && msg == WM_MOUSEWHEEL {
                    let z = (self.ctx.zoom_factor() + notches.signum() * 0.1).clamp(0.75, 2.0);
                    self.ctx.set_zoom_factor(z);
                } else {
                    self.events.push(Event::MouseWheel {
                        delta,
                        modifiers: m,
                    });
                }
                self.wake();
                return Some(0);
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                let pressed = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                let vk = wparam as u16;
                if let Some(key) = map_key(vk) {
                    let repeat = pressed && (lparam >> 30) & 1 == 1;
                    // Zoom like a browser.
                    if pressed && m.ctrl && !m.alt {
                        let z = self.ctx.zoom_factor();
                        let nz = match key {
                            Key::Plus | Key::Equals => Some(z + 0.1),
                            Key::Minus => Some(z - 0.1),
                            Key::Num0 => Some(1.0),
                            _ => None,
                        };
                        if let Some(nz) = nz {
                            self.ctx.set_zoom_factor((nz * 10.0).round() / 10.0);
                            self.wake();
                            return Some(0);
                        }
                        match key {
                            Key::C => self.events.push(Event::Copy),
                            Key::X => self.events.push(Event::Cut),
                            Key::V => {
                                if let Some(t) = get_clipboard(self.hwnd) {
                                    self.events.push(Event::Paste(t));
                                }
                            }
                            _ => {}
                        }
                    }
                    self.events.push(Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat,
                        modifiers: m,
                    });
                    self.wake();
                    // F10 and Alt+key would start the system menu loop.
                    if msg == WM_SYSKEYDOWN && vk != VK_F4 {
                        return Some(0);
                    }
                }
            }
            WM_CHAR => {
                let u = wparam as u16;
                if (0xD800..0xDC00).contains(&u) {
                    self.high_surrogate = Some(u);
                    return Some(0);
                }
                let s = if (0xDC00..0xE000).contains(&u) {
                    match self.high_surrogate.take() {
                        Some(hi) => String::from_utf16_lossy(&[hi, u]),
                        None => return Some(0),
                    }
                } else {
                    String::from_utf16_lossy(&[u])
                };
                // Control characters arrive as keys.
                if s.chars().all(|c| !c.is_control()) && !(m.ctrl && !m.alt) {
                    self.events.push(Event::Text(s));
                    self.wake();
                }
                return Some(0);
            }
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION => self.place_ime(),
            WM_SETCURSOR => {
                if (lparam & 0xFFFF) as u32 == HTCLIENT {
                    set_cursor(self.cursor);
                    return Some(1);
                }
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                self.events.push(Event::WindowFocused(msg == WM_SETFOCUS));
                self.wake();
            }
            WM_CLOSE => {
                self.closing = true;
                // SAFETY: closing our own window.
                unsafe { DestroyWindow(self.hwnd) };
                return Some(0);
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                return Some(0);
            }
            _ => {}
        }
        None
    }
}

fn set_cursor(c: CursorIcon) {
    let id = match c {
        CursorIcon::Default | CursorIcon::Grab | CursorIcon::Grabbing => IDC_ARROW,
        CursorIcon::PointingHand => IDC_HAND,
        CursorIcon::Text => IDC_IBEAM,
        CursorIcon::ResizeHorizontal => IDC_SIZEWE,
        CursorIcon::ResizeVertical => IDC_SIZENS,
        CursorIcon::Wait => IDC_WAIT,
        CursorIcon::Progress => IDC_APPSTARTING,
        CursorIcon::NotAllowed => IDC_NO,
    };
    // SAFETY: system cursors need no cleanup.
    unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), id)) };
}

fn set_clipboard(hwnd: HWND, text: &str) {
    let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the standard clipboard sequence; on success the clipboard
    // owns the memory.
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
                if SetClipboardData(CF_UNICODETEXT as u32, h as HANDLE).is_null() {
                    GlobalFree(h);
                }
            } else {
                GlobalFree(h);
            }
        }
        CloseClipboard();
    }
}

fn get_clipboard(hwnd: HWND) -> Option<String> {
    // SAFETY: reading the clipboard's text while it is open.
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT as u32);
        let mut out = None;
        if !h.is_null() {
            let p = GlobalLock(h as HGLOBAL) as *const u16;
            if !p.is_null() {
                let n = (0..).take_while(|&i| *p.add(i) != 0).count();
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(h as HGLOBAL);
            }
        }
        CloseClipboard();
        out
    }
}

fn map_key(vk: u16) -> Option<Key> {
    let k = vk as u32;
    Some(match vk {
        x if x == VK_LEFT => Key::ArrowLeft,
        x if x == VK_RIGHT => Key::ArrowRight,
        x if x == VK_UP => Key::ArrowUp,
        x if x == VK_DOWN => Key::ArrowDown,
        x if x == VK_ESCAPE => Key::Escape,
        x if x == VK_TAB => Key::Tab,
        x if x == VK_BACK => Key::Backspace,
        x if x == VK_RETURN => Key::Enter,
        x if x == VK_SPACE => Key::Space,
        x if x == VK_INSERT => Key::Insert,
        x if x == VK_DELETE => Key::Delete,
        x if x == VK_HOME => Key::Home,
        x if x == VK_END => Key::End,
        x if x == VK_PRIOR => Key::PageUp,
        x if x == VK_NEXT => Key::PageDown,
        x if x == VK_OEM_PLUS || x == VK_ADD => Key::Plus,
        x if x == VK_OEM_MINUS || x == VK_SUBTRACT => Key::Minus,
        _ if (0x30..=0x39).contains(&k) => [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ][(k - 0x30) as usize],
        _ if (0x60..=0x69).contains(&k) => [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ][(k - 0x60) as usize],
        _ if (0x41..=0x5A).contains(&k) => [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
        ][(k - 0x41) as usize],
        _ if (0x70..=0x7B).contains(&k) => [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
        ][(k - 0x70) as usize],
        _ => return None,
    })
}

/// An icon from straight-alpha RGBA pixels.
fn make_icon(icon: &IconData) -> HICON {
    let (w, h) = (icon.width as i32, icon.height as i32);
    let Some((color, bits)) = dib(w, h) else {
        return std::ptr::null_mut();
    };
    // SAFETY: `bits` holds w×h pixels; BGRA with alpha for the icon.
    unsafe {
        for (i, p) in icon.rgba.chunks_exact(4).enumerate().take((w * h) as usize) {
            *bits.add(i) =
                (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
        }
        let mask = CreateBitmap(w, h, 1, 1, std::ptr::null());
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let hicon = CreateIconIndirect(&ii);
        DeleteObject(mask as HGDIOBJ);
        DeleteObject(color as HGDIOBJ);
        hicon
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Window;
    if !ptr.is_null() && IN_FRAME.with(|f| f.get()) {
        // Re-entered from inside a frame: keep input for later, repaint after.
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                BeginPaint(hwnd, &mut ps);
                EndPaint(hwnd, &ps);
                QUEUED.with(|q| q.borrow_mut().push((WM_APP_WAKE, 0, 0)));
                return 0;
            }
            WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP
            | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MOUSEWHEEL | WM_MOUSEHWHEEL | WM_KEYDOWN
            | WM_KEYUP | WM_CHAR | WM_TIMER | WM_APP_WAKE | WM_SIZE | WM_MOUSELEAVE => {
                QUEUED.with(|q| q.borrow_mut().push((msg, wparam, lparam)));
                if msg == WM_TIMER {
                    KillTimer(hwnd, TIMER_ID);
                }
                return 0;
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    if !ptr.is_null() {
        // A panic must not unwind into Windows.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (*ptr).handle(msg, wparam, lparam)
        }));
        match r {
            Ok(Some(v)) => return v,
            Ok(None) => {}
            Err(_) => {
                PostQuitMessage(1);
                return 0;
            }
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Open the window and run until it closes.
pub fn run_native(
    opts: NativeOptions,
    setup: impl FnOnce(&Context) -> Box<dyn App>,
) -> Result<(), String> {
    // SAFETY: the usual window creation sequence; every handle is either
    // owned by the window or released below.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("ExpungeWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("could not register the window class".into());
        }
        let title = wide(&opts.title);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            600,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("could not create the window".into());
        }
        // Size for this monitor's scale, centred on the work area.
        let dpi = GetDpiForWindow(hwnd).max(96);
        let z = dpi as f32 / 96.0;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: (opts.inner_size.x * z) as i32,
            bottom: (opts.inner_size.y * z) as i32,
        };
        AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut work = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut c_void, 0);
        let x = work.left + ((work.right - work.left - ww) / 2).max(0);
        let y = work.top + ((work.bottom - work.top - wh) / 2).max(0);
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, ww, wh, SWP_NOZORDER);
        let dark: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const i32 as *const c_void,
            4,
        );
        if let Some(icon) = &opts.icon {
            let hi = make_icon(icon);
            if !hi.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, hi as LPARAM);
                SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, hi as LPARAM);
            }
        }

        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let ctx = Context::new(Box::new(GdiText::new(dc)));
        let hwnd_val = hwnd as usize;
        ctx.set_waker(move || {
            PostMessageW(hwnd_val as HWND, WM_APP_WAKE, 0, 0);
        });
        let app = setup(&ctx);
        let window = Box::new(Window {
            hwnd,
            ctx,
            app,
            dc,
            bitmap: std::ptr::null_mut(),
            bits: std::ptr::null_mut(),
            size: (0, 0),
            events: Vec::new(),
            start: Instant::now(),
            min_size: opts.min_inner_size,
            high_surrogate: None,
            cursor: CursorIcon::Default,
            tracking: false,
            timer_at: None,
            closing: false,
            ime_rect: None,
        });
        let wptr = Box::into_raw(window);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, wptr as isize);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        let code = loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r <= 0 {
                break msg.wParam;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        };
        let window = Box::from_raw(wptr);
        if !window.bitmap.is_null() {
            DeleteObject(window.bitmap as HGDIOBJ);
        }
        // The context (and the GDI fonts in it) goes before its DC.
        let dc = window.dc;
        drop(window);
        DeleteDC(dc);
        if code == 1 {
            return Err("the interface stopped after an internal error".into());
        }
    }
    Ok(())
}
