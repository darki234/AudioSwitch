//! Floating "island" notice in the Rust Browser style: pill shape, blurred shadow,
//! comet gradient, outline icons with round caps, spring animation.
//! Drawn with tiny-skia into a per-pixel-alpha layered window.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use std::cell::RefCell;
use std::time::Instant;
use tiny_skia::*;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::*;

const HOLD: f64 = 2.0; // default seconds on screen before leaving

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Headphones,
    Speaker,
    Keyboard,
}

impl Icon {
    pub fn for_device(headphones: bool) -> Icon {
        if headphones { Icon::Headphones } else { Icon::Speaker }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Osd>> = const { RefCell::new(None) };
    static QUIT_WHEN_DONE: RefCell<bool> = const { RefCell::new(false) };
}

pub fn quit_when_done() {
    QUIT_WHEN_DONE.with(|q| *q.borrow_mut() = true);
}

/// Show the island (replaces one that is still on screen).
pub fn show(caption: &str, title: &str, icon: Icon) {
    show_for(caption, title, icon, HOLD);
}

/// Same, staying on screen for `hold` seconds.
pub fn show_for(caption: &str, title: &str, icon: Icon, hold: f64) {
    let old = CURRENT.with(|c| c.borrow_mut().take());
    drop(old);
    if let Some(osd) = Osd::new(caption, title, icon, hold) {
        CURRENT.with(|c| *c.borrow_mut() = Some(osd));
    }
}

fn tick() {
    let done = CURRENT.with(|c| c.borrow_mut().as_mut().map(|o| o.step()).unwrap_or(true));
    if done {
        let old = CURRENT.with(|c| c.borrow_mut().take());
        drop(old);
        if QUIT_WHEN_DONE.with(|q| *q.borrow()) {
            unsafe { PostQuitMessage(0) };
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_TIMER {
        tick();
        return LRESULT(0);
    }
    DefWindowProcW(h, msg, w, l)
}

// ---------- theme (Rust Browser palette) ----------

#[derive(Clone, Copy)]
struct Theme {
    dark: bool,
    surface: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    accent2: Color,
    accent3: Color,
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

fn with_alpha(c: Color, a: f32) -> Color {
    let mut c = c;
    c.set_alpha((c.alpha() * a).clamp(0.0, 1.0));
    c
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.red() + (b.red() - a.red()) * t,
        a.green() + (b.green() - a.green()) * t,
        a.blue() + (b.blue() - a.blue()) * t,
        1.0,
    )
    .unwrap()
}

fn apps_light_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_err() || value != 0
}

impl Theme {
    fn system() -> Theme {
        if apps_light_theme() {
            Theme {
                dark: false,
                surface: rgb(255, 255, 255),
                border: Color::from_rgba8(20, 24, 48, 23),
                text: rgb(27, 28, 34),
                muted: rgb(105, 112, 132),
                accent: rgb(79, 91, 245),
                accent2: rgb(224, 74, 154),
                accent3: rgb(24, 182, 201),
            }
        } else {
            Theme {
                dark: true,
                surface: rgb(35, 36, 44),
                border: Color::from_rgba8(255, 255, 255, 20),
                text: rgb(236, 238, 245),
                muted: rgb(154, 159, 177),
                accent: rgb(130, 136, 255),
                accent2: rgb(244, 114, 182),
                accent3: rgb(56, 211, 230),
            }
        }
    }
}

// ---------- geometry helpers ----------

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

/// Arc as a polyline; angles in degrees, clockwise on screen, 0 = +x.
fn arc(pb: &mut PathBuilder, cx: f32, cy: f32, r: f32, start: f32, sweep: f32, move_first: bool) {
    let n = 28;
    for i in 0..=n {
        let a = (start + sweep * i as f32 / n as f32).to_radians();
        let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
        if i == 0 && move_first {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
}

// ---------- text ----------

struct Fonts {
    regular: Vec<u8>,
    semibold: Vec<u8>,
}

impl Fonts {
    fn load() -> Option<Fonts> {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()) + "\\Fonts\\";
        let regular = std::fs::read(dir.clone() + "segoeui.ttf").ok()?;
        let semibold = std::fs::read(dir.clone() + "seguisb.ttf").unwrap_or_else(|_| regular.clone());
        Some(Fonts { regular, semibold })
    }
}

fn text_width(font: &FontRef, px: f32, text: &str) -> f32 {
    let sf = font.as_scaled(PxScale::from(px));
    let mut w = 0.0;
    let mut prev = None;
    for ch in text.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

fn ellipsize(font: &FontRef, px: f32, text: &str, max_w: f32) -> String {
    if text_width(font, px, text) <= max_w {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let s: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if text_width(font, px, &s) <= max_w {
            return s;
        }
    }
    "…".into()
}

/// Draws text with its baseline at `baseline`, blending into premultiplied RGBA.
fn draw_text(pm: &mut Pixmap, font: &FontRef, px: f32, x: f32, baseline: f32, color: Color, text: &str) {
    let sf = font.as_scaled(PxScale::from(px));
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let (cr, cg, cb, ca) = (color.red() * 255.0, color.green() * 255.0, color.blue() * 255.0, color.alpha());
    let mut caret = x;
    let mut prev = None;
    for ch in text.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            caret += sf.kern(p, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(px), point(caret, baseline));
        caret += sf.h_advance(id);
        prev = Some(id);
        let Some(og) = font.outline_glyph(glyph) else { continue };
        let b = og.px_bounds();
        og.draw(|gx, gy, cov| {
            let px_ = b.min.x as i32 + gx as i32;
            let py_ = b.min.y as i32 + gy as i32;
            if px_ < 0 || py_ < 0 || px_ >= w || py_ >= h {
                return;
            }
            // light gamma lift so thin strokes don't look washed out
            let a = cov.clamp(0.0, 1.0).powf(0.85) * ca;
            if a <= 0.0 {
                return;
            }
            let i = ((py_ * w + px_) * 4) as usize;
            let inv = 1.0 - a;
            data[i] = (cr * a + data[i] as f32 * inv).min(255.0) as u8;
            data[i + 1] = (cg * a + data[i + 1] as f32 * inv).min(255.0) as u8;
            data[i + 2] = (cb * a + data[i + 2] as f32 * inv).min(255.0) as u8;
            data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv).min(255.0) as u8;
        });
    }
}

// ---------- shadow ----------

fn box_blur(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut tmp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];
    let div = (2 * r + 1) as u32;
    for y in 0..h {
        let mut acc: u32 = 0;
        for x in 0..w {
            let lo = x.saturating_sub(r);
            let hi = (x + r).min(w - 1);
            if x == 0 {
                acc = (lo..=hi).map(|i| src[y * w + i] as u32).sum();
            } else {
                if x + r < w {
                    acc += src[y * w + x + r] as u32;
                }
                if x > r {
                    acc -= src[y * w + x - r - 1] as u32;
                }
            }
            tmp[y * w + x] = (acc / div) as u8;
        }
    }
    for x in 0..w {
        let mut acc: u32 = 0;
        for y in 0..h {
            if y == 0 {
                acc = (0..=r.min(h - 1)).map(|i| tmp[i * w + x] as u32).sum();
            } else {
                if y + r < h {
                    acc += tmp[(y + r) * w + x] as u32;
                }
                if y > r {
                    acc -= tmp[(y - r - 1) * w + x] as u32;
                }
            }
            out[y * w + x] = (acc / div) as u8;
        }
    }
    out
}

// ---------- the island ----------

struct Osd {
    hwnd: HWND,
    memdc: HDC,
    dib: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u8,
    base: Pixmap,
    frame: Pixmap,
    pill_mask: Mask,
    pill: Rect,
    theme: Theme,
    s: f32,
    w: u32,
    h: u32,
    base_x: i32,
    base_y: i32,
    start: Instant,
    last: Instant,
    offset: f64,
    velocity: f64,
    alpha: f64,
    leaving: Option<Instant>,
    hold: f64,
}

impl Osd {
    fn new(caption: &str, title: &str, icon: Icon, hold: f64) -> Option<Osd> {
        let s = unsafe { GetDpiForSystem() } as f32 / 96.0;
        let theme = Theme::system();
        let fonts = Fonts::load()?;
        let regular = FontRef::try_from_slice(&fonts.regular).ok()?;
        let semibold = FontRef::try_from_slice(&fonts.semibold).ok()?;
        let (cap_px, title_px) = (12.0 * s, 14.5 * s);

        let text_w = text_width(&regular, cap_px, caption).max(text_width(&semibold, title_px, title));
        let pill_h = (60.0 * s).round();
        let pill_w = (12.0 * s + 38.0 * s + 12.0 * s + text_w + 26.0 * s).clamp(220.0 * s, 380.0 * s).round();
        let margin = (30.0 * s).round();
        let (w, h) = ((pill_w + margin * 2.0) as u32, (pill_h + margin * 2.0) as u32);
        let pill = Rect::from_xywh(margin, margin, pill_w, pill_h)?;
        let pill_path = rounded_rect(pill.x(), pill.y(), pill.width(), pill.height(), pill_h / 2.0);

        let mut base = Pixmap::new(w, h)?;

        // blurred shadow, offset downwards
        {
            let mut m = Mask::new(w, h)?;
            let shadow_path = rounded_rect(pill.x(), pill.y() + 7.0 * s, pill.width(), pill.height(), pill_h / 2.0);
            m.fill_path(&shadow_path, FillRule::Winding, true, Transform::identity());
            let r = (5.0 * s).round().max(1.0) as usize;
            let mut a = m.data().to_vec();
            for _ in 0..3 {
                a = box_blur(&a, w as usize, h as usize, r);
            }
            let (sr, sg, sb, strength) = if theme.dark { (0.0, 0.0, 0.0, 0.55) } else { (24.0, 28.0, 70.0, 0.20) };
            let data = base.data_mut();
            for (i, &v) in a.iter().enumerate() {
                let al = v as f32 / 255.0 * strength;
                data[i * 4] = (sr * al) as u8;
                data[i * 4 + 1] = (sg * al) as u8;
                data[i * 4 + 2] = (sb * al) as u8;
                data[i * 4 + 3] = (255.0 * al) as u8;
            }
        }

        // the pill itself
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color(theme.surface);
        // clear the shadow under the pill so a translucent edge doesn't darken
        base.fill_path(&pill_path, &paint, FillRule::Winding, Transform::identity(), None);

        let mut pill_mask = Mask::new(w, h)?;
        pill_mask.fill_path(&pill_path, FillRule::Winding, true, Transform::identity());

        // badge with comet gradient
        let r = 19.0 * s;
        let (cx, cy) = (pill.x() + 11.0 * s + r, pill.y() + pill_h / 2.0);
        let mut badge = Paint::default();
        badge.anti_alias = true;
        badge.shader = LinearGradient::new(
            Point::from_xy(cx - r, cy - r),
            Point::from_xy(cx + r, cy + r),
            vec![
                GradientStop::new(0.0, theme.accent),
                GradientStop::new(0.6, mix(theme.accent, theme.accent2, 0.45)),
                GradientStop::new(1.0, mix(theme.accent2, theme.accent3, 0.25)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )?;
        let circle = PathBuilder::from_circle(cx, cy, r)?;
        base.fill_path(&circle, &badge, FillRule::Winding, Transform::identity(), None);

        draw_icon(&mut base, cx, cy, s, icon);

        // texts
        let tx = cx + r + 12.0 * s;
        let max_w = pill.right() - tx - 22.0 * s;
        let cap = ellipsize(&regular, cap_px, caption, max_w);
        let tit = ellipsize(&semibold, title_px, title, max_w);
        draw_text(&mut base, &regular, cap_px, tx, pill.y() + 25.0 * s, theme.muted, &cap);
        draw_text(&mut base, &semibold, title_px, tx, pill.y() + 44.5 * s, theme.text, &tit);

        // hairline border
        let mut bp = Paint::default();
        bp.anti_alias = true;
        bp.set_color(theme.border);
        let inset = 0.5 * s.max(1.0);
        let border = rounded_rect(pill.x() + inset, pill.y() + inset, pill_w - inset * 2.0, pill_h - inset * 2.0, pill_h / 2.0 - inset);
        base.stroke_path(&border, &bp, &Stroke { width: s.max(1.0), ..Stroke::default() }, Transform::identity(), None);

        // window placement: bottom-right, above the taskbar
        let mut wa = RECT::default();
        unsafe {
            let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut RECT as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        }
        let base_x = wa.right - (16.0 * s) as i32 - pill_w as i32 - margin as i32;
        let base_y = wa.bottom - (16.0 * s) as i32 - pill_h as i32 - margin as i32;

        let (hwnd, memdc, dib, old_obj, bits) = unsafe { create_window(base_x, base_y, w, h)? };

        let frame = base.clone();
        let now = Instant::now();
        let mut osd = Osd {
            hwnd, memdc, dib, old_obj, bits, base, frame, pill_mask, pill, theme, s, w, h, base_x, base_y,
            start: now, last: now, offset: 26.0, velocity: 0.0, alpha: 0.0, leaving: None, hold,
        };
        osd.render();
        unsafe {
            let _ = ShowWindow(osd.hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(osd.hwnd), 1, 15, None);
        }
        Some(osd)
    }

    /// Advances the animation; returns true when the island has left.
    fn step(&mut self) -> bool {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.05);
        self.last = now;
        let life = (now - self.start).as_secs_f64();

        match self.leaving {
            None => {
                // underdamped spring towards rest: a soft little bounce
                let mut t = dt;
                while t > 0.0 {
                    let h = t.min(0.004);
                    let a = -260.0 * self.offset - 20.0 * self.velocity;
                    self.velocity += a * h;
                    self.offset += self.velocity * h;
                    t -= h;
                }
                self.alpha = (life / 0.16).min(1.0);
                if life >= self.hold {
                    self.leaving = Some(now);
                }
            }
            Some(t0) => {
                let k = ((now - t0).as_secs_f64() / 0.24).min(1.0);
                let ease = k * k;
                self.alpha = 1.0 - ease;
                self.offset = 12.0 * ease;
                if k >= 1.0 {
                    return true;
                }
            }
        }
        self.render();
        false
    }

    fn render(&mut self) {
        let s = self.s;
        self.frame.data_mut().copy_from_slice(self.base.data());

        // comet sweeping along the bottom edge once, its tail then fades
        let life = (Instant::now() - self.start).as_secs_f64() as f32;
        let k = (life / 1.1).min(1.0);
        let head = 1.0 - (1.0 - k).powi(3);
        let fade = if life < 1.1 { 1.0 } else { (1.0 - (life - 1.1) / 0.6).max(0.0) };
        if fade > 0.0 {
            let t = self.theme;
            let line_h = 2.5 * s;
            let y = self.pill.bottom() - line_h;
            let hx = self.pill.x() + head * self.pill.width();
            let tail = self.pill.width() * 0.55;
            if let Some(rect) = Rect::from_xywh(hx - tail, y, tail, line_h) {
                let mut p = Paint::default();
                p.anti_alias = true;
                if let Some(sh) = LinearGradient::new(
                    Point::from_xy(hx - tail, y),
                    Point::from_xy(hx, y),
                    vec![
                        GradientStop::new(0.0, with_alpha(t.accent3, 0.0)),
                        GradientStop::new(0.35, with_alpha(t.accent3, 0.6 * fade)),
                        GradientStop::new(0.7, with_alpha(t.accent2, 0.85 * fade)),
                        GradientStop::new(1.0, with_alpha(t.accent, fade)),
                    ],
                    SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    p.shader = sh;
                    self.frame.fill_rect(rect, &p, Transform::identity(), Some(&self.pill_mask));
                }
                // glowing head
                let gr = 6.0 * s;
                if let Some(sh) = RadialGradient::new(
                    Point::from_xy(hx, y + line_h / 2.0),
                    Point::from_xy(hx, y + line_h / 2.0),
                    gr,
                    vec![GradientStop::new(0.0, with_alpha(t.accent, 0.7 * fade)), GradientStop::new(1.0, with_alpha(t.accent, 0.0))],
                    SpreadMode::Pad,
                    Transform::identity(),
                ) {
                    p.shader = sh;
                    if let Some(c) = PathBuilder::from_circle(hx, y + line_h / 2.0, gr) {
                        self.frame.fill_path(&c, &p, FillRule::Winding, Transform::identity(), Some(&self.pill_mask));
                    }
                }
            }
        }

        // RGBA (premultiplied) -> BGRA for GDI
        let n = (self.w * self.h) as usize;
        let src = self.frame.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, n * 4) };
        for i in 0..n {
            let j = i * 4;
            dst[j] = src[j + 2];
            dst[j + 1] = src[j + 1];
            dst[j + 2] = src[j];
            dst[j + 3] = src[j + 3];
        }

        let pos = POINT { x: self.base_x, y: self.base_y + (self.offset * s as f64).round() as i32 };
        let size = SIZE { cx: self.w as i32, cy: self.h as i32 };
        let src_pt = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (255.0 * self.alpha.clamp(0.0, 1.0)) as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let screen = GetDC(None);
            let _ = UpdateLayeredWindow(
                self.hwnd, Some(screen), Some(&pos), Some(&size), Some(self.memdc), Some(&src_pt),
                COLORREF(0), Some(&blend), ULW_ALPHA,
            );
            ReleaseDC(None, screen);
        }
    }
}

impl Drop for Osd {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), 1);
            let _ = DestroyWindow(self.hwnd);
            SelectObject(self.memdc, self.old_obj);
            let _ = DeleteObject(self.dib.into());
            let _ = DeleteDC(self.memdc);
        }
    }
}

unsafe fn create_window(x: i32, y: i32, w: u32, h: u32) -> Option<(HWND, HDC, HBITMAP, HGDIOBJ, *mut u8)> {
    let hinst = GetModuleHandleW(None).ok()?;
    let class = w!("AudioSwitchIsland");
    let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
    RegisterClassW(&wc); // fails harmlessly if already registered

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
        class, w!(""), WS_POPUP, x, y, w as i32, h as i32, None, None, Some(hinst.into()), None,
    )
    .ok()?;

    let screen = GetDC(None);
    let memdc = CreateCompatibleDC(Some(screen));
    ReleaseDC(None, screen);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(Some(memdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    let old = SelectObject(memdc, dib.into());
    Some((hwnd, memdc, dib, old, bits as *mut u8))
}

/// Outline icons with round caps, white on the badge.
fn draw_icon(pm: &mut Pixmap, cx: f32, cy: f32, s: f32, icon: Icon) {
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color(Color::WHITE);
    let stroke = Stroke { width: 1.8 * s, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
    let mut pb = PathBuilder::new();
    if icon == Icon::Keyboard {
        let (w, h) = (17.0 * s, 12.0 * s);
        let body = rounded_rect(cx - w / 2.0, cy - h / 2.0, w, h, 2.6 * s);
        pm.stroke_path(&body, &paint, &stroke, Transform::identity(), None);
        // two rows of keys as dots, then the space bar
        for (row, n) in [(-2.6f32, 4), (0.4, 4)] {
            for i in 0..n {
                let x = cx - 4.8 * s + i as f32 * 3.2 * s;
                if let Some(dot) = PathBuilder::from_circle(x, cy + row * s, 0.95 * s) {
                    pm.fill_path(&dot, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
        }
        pb.move_to(cx - 3.2 * s, cy + 3.3 * s);
        pb.line_to(cx + 3.2 * s, cy + 3.3 * s);
    } else if icon == Icon::Headphones {
        let r = 7.5 * s;
        let top = cy + 0.5 * s;
        // headband: from the left side over the top to the right side
        pb.move_to(cx - r, top + 2.5 * s);
        arc(&mut pb, cx, top, r, 180.0, 180.0, false);
        pb.line_to(cx + r, top + 2.5 * s);
        let cup_w = 4.2 * s;
        let cup_h = 6.5 * s;
        let p1 = rounded_rect(cx - r - 1.2 * s, cy + 1.5 * s, cup_w, cup_h, 1.6 * s);
        let p2 = rounded_rect(cx + r - 3.0 * s, cy + 1.5 * s, cup_w, cup_h, 1.6 * s);
        pm.stroke_path(&p1, &paint, &stroke, Transform::identity(), None);
        pm.stroke_path(&p2, &paint, &stroke, Transform::identity(), None);
    } else {
        let ox = cx - 2.5 * s;
        pb.move_to(ox - 6.0 * s, cy - 2.6 * s);
        pb.line_to(ox - 3.0 * s, cy - 2.6 * s);
        pb.line_to(ox + 1.5 * s, cy - 6.5 * s);
        pb.line_to(ox + 1.5 * s, cy + 6.5 * s);
        pb.line_to(ox - 3.0 * s, cy + 2.6 * s);
        pb.line_to(ox - 6.0 * s, cy + 2.6 * s);
        pb.close();
        let wx = ox + 1.5 * s;
        arc(&mut pb, wx, cy, 3.8 * s, -42.0, 84.0, true);
        arc(&mut pb, wx, cy, 7.2 * s, -42.0, 84.0, true);
    }
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}
