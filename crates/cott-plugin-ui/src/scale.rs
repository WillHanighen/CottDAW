//! Linux plugin windows often ignore the desktop scale. nih-plug-egui even
//! forces a factor of 1 on X11 so the GUI is not clipped. That leaves a 175%
//! laptop looking at 11-pixel type.
//!
//! We recover a scale from the environment or the panel's native mode, then
//! the editor sets `pixels_per_point` and opens a window in physical pixels.

use std::sync::OnceLock;

/// Desktop UI scale, 1.0..3.0.
///
/// Order: `COTT_UI_SCALE`, then GDK/Qt factors greater than 1, then a guess
/// from `/sys/class/drm/*/modes` (2560-wide panels land on 1.75).
pub fn display_scale() -> f32 {
    static SCALE: OnceLock<f32> = OnceLock::new();
    *SCALE.get_or_init(compute_scale)
}

/// Logical size mapped to the physical pixels the host actually creates.
/// nih-plug reports this size at scale 1, so we bake the scale in.
pub fn physical_size(logical_w: u32, logical_h: u32) -> (u32, u32) {
    let s = display_scale();
    (
        ((logical_w as f32) * s).round() as u32,
        ((logical_h as f32) * s).round() as u32,
    )
}

fn compute_scale() -> f32 {
    if let Some(v) = env_f32("COTT_UI_SCALE") {
        return v.clamp(1.0, 3.0);
    }
    // GNOME fractional scaling: GDK_SCALE=2 and GDK_DPI_SCALE=0.875 is 175%.
    let gdk = env_f32("GDK_SCALE").unwrap_or(1.0);
    let gdk_dpi = env_f32("GDK_DPI_SCALE").unwrap_or(1.0);
    let gdk_product = gdk * gdk_dpi;
    if gdk_product > 1.01 {
        return gdk_product.clamp(1.0, 3.0);
    }
    if let Some(v) = env_f32("QT_SCALE_FACTOR") {
        if v > 1.01 {
            return v.clamp(1.0, 3.0);
        }
    }
    guess_from_drm().clamp(1.0, 2.5)
}

fn env_f32(key: &str) -> Option<f32> {
    std::env::var(key).ok()?.parse().ok()
}

fn guess_from_drm() -> f32 {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return 1.0;
    };
    let mut max_w = 0u32;
    for entry in entries.flatten() {
        let path = entry.path().join("modes");
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(line) = text.lines().next() else {
            continue;
        };
        let Some((w, _)) = line.split_once('x') else {
            continue;
        };
        if let Ok(w) = w.trim().parse::<u32>() {
            max_w = max_w.max(w);
        }
    }
    match max_w {
        0 => 1.0,
        w if w >= 3840 => 2.0,
        w if w >= 2560 => 1.75,
        w if w >= 2048 => 1.5,
        w if w >= 1920 => 1.25,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_stays_sane() {
        let s = display_scale();
        assert!((1.0..=3.0).contains(&s));
    }

    #[test]
    fn physical_size_grows_with_scale() {
        let (w, h) = physical_size(100, 50);
        let s = display_scale();
        assert_eq!(w, (100.0 * s).round() as u32);
        assert_eq!(h, (50.0 * s).round() as u32);
    }
}
