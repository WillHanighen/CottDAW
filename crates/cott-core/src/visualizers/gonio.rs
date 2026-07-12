//! Stereo goniometer (vectorscope) frame renderer.
//!
//! Mapping matches common web goniometers (e.g. SamNZito/Gonio-Visualizer):
//! - X = (L − R) × scale  (side / width of stereo image)
//! - Y = −(L + R) × scale (mid / vertical; up = in-phase energy)
//!
//! Line mode connects consecutive samples into a continuous stroke; dots mode
//! plots soft points. Persistence fades trails toward a black background.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GonioDrawMode {
    /// Discrete sample points (soft bloom).
    Dots,
    /// Continuous polyline through successive samples (default).
    #[default]
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GonioColorMode {
    /// Fixed foreground color.
    #[default]
    Static,
    /// Hue slowly shifts across the export.
    Gradient,
    /// Hue from distance to center (spectrum).
    Spectrum,
}

/// Classic L/R → canvas coordinates (no mid/side √2 scaling).
#[inline]
pub fn stereo_xy(l: f32, r: f32) -> (f32, f32) {
    (l - r, l + r)
}

#[derive(Debug, Clone)]
pub struct GonioOptions {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// How much of the previous frame survives each frame (0 = clear, ~1 = long trails).
    pub persistence: f32,
    /// Extra gain applied after auto-normalization (when enabled).
    pub intensity: f32,
    pub show_guides: bool,
    /// ffmpeg x264 CRF (lower = higher quality).
    pub crf: u8,
    pub draw_mode: GonioDrawMode,
    pub color_mode: GonioColorMode,
    /// Auto-scale so peaks fill most of the display (SamNZito-style).
    pub auto_normalize: bool,
    /// Line stroke width in pixels (line mode).
    pub line_width: u8,
    /// Dot radius in pixels (dots mode).
    pub point_size: u8,
    /// Static RGB color (0–255). Used for Static mode; base for Gradient.
    pub color_rgb: [u8; 3],
    /// Subsample stride for plotting (1 = every sample; higher = faster / thinner lines).
    pub sample_stride: u32,
}

impl Default for GonioOptions {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1080,
            fps: 30,
            persistence: 0.82,
            intensity: 1.0,
            show_guides: false,
            crf: 18,
            draw_mode: GonioDrawMode::Line,
            color_mode: GonioColorMode::Static,
            auto_normalize: true,
            line_width: 2,
            point_size: 2,
            color_rgb: [0, 255, 80],
            sample_stride: 4,
        }
    }
}

impl GonioOptions {
    pub fn clamp(mut self) -> Self {
        self.width = self.width.clamp(256, 2160);
        self.height = self.height.clamp(256, 2160);
        // Even dimensions for yuv420p.
        self.width &= !1;
        self.height &= !1;
        self.fps = self.fps.clamp(1, 60);
        self.persistence = self.persistence.clamp(0.0, 0.99);
        self.intensity = self.intensity.clamp(0.05, 8.0);
        self.crf = self.crf.clamp(0, 51);
        self.line_width = self.line_width.clamp(1, 8);
        self.point_size = self.point_size.clamp(1, 8);
        self.sample_stride = self.sample_stride.clamp(1, 64);
        self
    }
}

/// Persistent RGB24 framebuffer for animated goniometer export.
pub struct GonioRenderer {
    opts: GonioOptions,
    /// Interleaved RGB, length = width * height * 3.
    pixels: Vec<u8>,
    bg: [u8; 3],
    guide: [u8; 3],
    /// Smoothed amplitude normalization (SamNZito-style).
    normalization: f32,
    /// Frame counter for gradient hue.
    frame_index: u64,
    /// Scale in pixels for |L±R| ≈ 1 after normalization.
    display_scale: f32,
}

impl GonioRenderer {
    pub fn new(opts: GonioOptions) -> Self {
        let opts = opts.clamp();
        let len = (opts.width as usize) * (opts.height as usize) * 3;
        let bg = [0, 0, 0];
        let pixels = vec![0u8; len];
        let display_scale = (opts.width.min(opts.height) as f32) * 0.4;
        Self {
            opts,
            pixels,
            bg,
            guide: [40, 44, 52],
            normalization: 1.0,
            frame_index: 0,
            display_scale,
        }
    }

    pub fn options(&self) -> &GonioOptions {
        &self.opts
    }

    pub fn width(&self) -> u32 {
        self.opts.width
    }

    pub fn height(&self) -> u32 {
        self.opts.height
    }

    /// RGB24 bytes for the current frame (row-major).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Fade trails, plot L/R samples for this slice, optionally redraw guides.
    pub fn render_frame(&mut self, left: &[f32], right: &[f32]) {
        self.fade();
        self.update_normalization(left, right);

        let stride = self.opts.sample_stride as usize;
        let n = left.len().min(right.len());
        let gain = self.normalization * self.opts.intensity;

        match self.opts.draw_mode {
            GonioDrawMode::Dots => {
                let mut i = 0;
                while i < n {
                    let l = left[i] * gain;
                    let r = right[i] * gain;
                    let (sx, sy) = stereo_xy(l, r);
                    let (px, py) = self.to_pixel(sx, sy);
                    let rgb = self.color_at(sx, sy);
                    self.plot_dot(px, py, rgb);
                    i += stride;
                }
            }
            GonioDrawMode::Line => {
                let mut points: Vec<(i32, i32, [u8; 3])> = Vec::with_capacity(n / stride + 1);
                let mut i = 0;
                while i < n {
                    let l = left[i] * gain;
                    let r = right[i] * gain;
                    let (sx, sy) = stereo_xy(l, r);
                    let (px, py) = self.to_pixel(sx, sy);
                    let rgb = self.color_at(sx, sy);
                    points.push((px, py, rgb));
                    i += stride;
                }
                if points.len() >= 2 {
                    for w in points.windows(2) {
                        let (x0, y0, c0) = w[0];
                        let (x1, y1, _) = w[1];
                        self.stroke_line(x0, y0, x1, y1, c0);
                    }
                } else if let Some(&(x, y, c)) = points.first() {
                    self.plot_dot(x, y, c);
                }
            }
        }

        if self.opts.show_guides {
            self.draw_guides();
        }
        self.frame_index = self.frame_index.saturating_add(1);
    }

    fn update_normalization(&mut self, left: &[f32], right: &[f32]) {
        if !self.opts.auto_normalize {
            self.normalization = 1.0;
            return;
        }
        let n = left.len().min(right.len());
        let mut max_amp = 0.01_f32;
        for i in 0..n {
            max_amp = max_amp.max(left[i].abs()).max(right[i].abs());
        }
        let target = if max_amp > 0.01 {
            0.7 / max_amp
        } else {
            1.0
        };
        self.normalization = self.normalization * 0.95 + target * 0.05;
    }

    fn to_pixel(&self, side: f32, mid: f32) -> (i32, i32) {
        let cx = self.opts.width as f32 * 0.5;
        let cy = self.opts.height as f32 * 0.5;
        let s = self.display_scale;
        let px = cx + side * s;
        let py = cy - mid * s;
        (px.round() as i32, py.round() as i32)
    }

    fn color_at(&self, side: f32, mid: f32) -> [u8; 3] {
        match self.opts.color_mode {
            GonioColorMode::Static => self.opts.color_rgb,
            GonioColorMode::Gradient => {
                let hue = (self.frame_index as f32 * 0.35) % 360.0;
                hsl_to_rgb(hue, 1.0, 0.5)
            }
            GonioColorMode::Spectrum => {
                let dist = (side * side + mid * mid).sqrt().clamp(0.0, 1.5);
                let hue = (dist / 1.5 * 360.0) % 360.0;
                hsl_to_rgb(hue, 1.0, 0.5)
            }
        }
    }

    fn fade(&mut self) {
        let keep = self.opts.persistence;
        let lose = 1.0 - keep;
        let bg = self.bg;
        for chunk in self.pixels.chunks_exact_mut(3) {
            for c in 0..3 {
                let v = chunk[c] as f32;
                let next = v * keep + bg[c] as f32 * lose;
                chunk[c] = next.round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    fn plot_dot(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        let radius = self.opts.point_size as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let dist2 = dx * dx + dy * dy;
                if dist2 > radius * radius {
                    continue;
                }
                let weight = if dist2 == 0 {
                    1.0
                } else if dist2 <= 1 {
                    0.55
                } else {
                    0.25
                };
                self.add_pixel(x + dx, y + dy, rgb, weight);
            }
        }
    }

    fn stroke_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: [u8; 3]) {
        let thickness = self.opts.line_width as i32;
        // Bresenham with thickness via neighbor stamps.
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.stamp_thick(x, y, thickness, rgb);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    fn stamp_thick(&mut self, x: i32, y: i32, thickness: i32, rgb: [u8; 3]) {
        let r = (thickness / 2).max(0);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + r {
                    self.add_pixel(x + dx, y + dy, rgb, 1.0);
                }
            }
        }
    }

    fn add_pixel(&mut self, x: i32, y: i32, rgb: [u8; 3], weight: f32) {
        let w = self.opts.width as i32;
        let h = self.opts.height as i32;
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let idx = ((y as usize) * (w as usize) + (x as usize)) * 3;
        for c in 0..3 {
            let add = rgb[c] as f32 * weight;
            let v = self.pixels[idx + c] as f32 + add;
            self.pixels[idx + c] = v.clamp(0.0, 255.0) as u8;
        }
    }

    fn draw_guides(&mut self) {
        let w = self.opts.width as i32;
        let h = self.opts.height as i32;
        let cx = w / 2;
        let cy = h / 2;
        for x in 0..w {
            self.set_guide(x, cy);
        }
        for y in 0..h {
            self.set_guide(cx, y);
        }
        let rx = (w as f32 * 0.4) as i32;
        let ry = (h as f32 * 0.4) as i32;
        self.draw_ellipse(cx, cy, rx, ry);
    }

    fn set_guide(&mut self, x: i32, y: i32) {
        let w = self.opts.width as i32;
        let h = self.opts.height as i32;
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let idx = ((y as usize) * (w as usize) + (x as usize)) * 3;
        for c in 0..3 {
            let cur = self.pixels[idx + c];
            if cur <= 24 {
                self.pixels[idx + c] = self.guide[c];
            }
        }
    }

    fn draw_ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        let steps = ((rx + ry) as usize * 2).max(64);
        for i in 0..steps {
            let t = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let x = cx as f32 + rx as f32 * t.cos();
            let y = cy as f32 + ry as f32 * t.sin();
            self.set_guide(x.round() as i32, y.round() as i32);
        }
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match h_prime as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_maps_to_vertical() {
        let (side, mid) = stereo_xy(0.5, 0.5);
        assert!(side.abs() < 1e-6, "mono should have near-zero side, got {side}");
        assert!(mid > 0.0, "mono positive should have positive mid, got {mid}");
    }

    #[test]
    fn antiphase_maps_to_horizontal() {
        let (side, mid) = stereo_xy(0.5, -0.5);
        assert!(mid.abs() < 1e-6, "antiphase should have near-zero mid, got {mid}");
        assert!(side > 0.0, "L>R antiphase should have positive side, got {side}");
    }

    #[test]
    fn line_mode_smoke() {
        let mut g = GonioRenderer::new(GonioOptions {
            width: 256,
            height: 256,
            fps: 30,
            persistence: 0.5,
            intensity: 1.0,
            show_guides: false,
            crf: 18,
            draw_mode: GonioDrawMode::Line,
            color_mode: GonioColorMode::Static,
            auto_normalize: false,
            line_width: 2,
            point_size: 2,
            color_rgb: [0, 255, 0],
            sample_stride: 1,
        });
        // Sweep that draws a visible path off center.
        let left: Vec<f32> = (0..64).map(|i| (i as f32 / 64.0) * 0.8).collect();
        let right: Vec<f32> = (0..64).map(|i| ((64 - i) as f32 / 64.0) * 0.8).collect();
        g.render_frame(&left, &right);
        assert_eq!(g.pixels().len(), 256 * 256 * 3);
        let lit = g.pixels().chunks_exact(3).filter(|c| c[1] > 20).count();
        assert!(lit > 10, "expected lit green pixels from line stroke, got {lit}");
    }

    #[test]
    fn mono_line_lights_vertical_center() {
        let mut g = GonioRenderer::new(GonioOptions {
            width: 256,
            height: 256,
            draw_mode: GonioDrawMode::Line,
            auto_normalize: false,
            sample_stride: 1,
            show_guides: false,
            ..Default::default()
        });
        let left = vec![0.0_f32, 0.3, 0.6, 0.3, 0.0];
        let right = left.clone();
        g.render_frame(&left, &right);
        let w = 256usize;
        let mut lit = 0usize;
        for y in 0..256 {
            let idx = (y * w + w / 2) * 3;
            if g.pixels()[idx + 1] > 20 {
                lit += 1;
            }
        }
        assert!(lit > 0, "expected lit pixels on vertical center for mono");
    }
}
