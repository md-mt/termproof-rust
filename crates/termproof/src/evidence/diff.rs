//! Baseline comparison and visual diff.
//!
//! Mirrors `termproof/visual_diff.py`.  PNG diff uses `image` pixel
//! comparison; SVG diff falls back to side-by-side embedding with base64
//! data URIs.  Update mode copies the current screenshot to the baseline.

use crate::{AssertionResult, RunResult};
use std::path::{Path, PathBuf};

/// Apply visual diff to `result` against `baseline_root`.
///
/// When `update` is true, the baseline is refreshed and a passing
/// `visual_diff` assertion is added.  Otherwise missing / mismatched
/// baselines produce a failing assertion and (for mismatches) a
/// `visual_diff` artifact beside the run screenshot.
pub fn apply_visual_diff(mut result: RunResult, baseline_root: &Path, update: bool) -> RunResult {
    let screenshot_path = match result.artifacts.get("screenshot") {
        Some(p) => PathBuf::from(p),
        None => {
            result.assertions.push(AssertionResult {
                name: "visual_diff".into(),
                passed: false,
                detail: "no screenshot artifact to compare".into(),
            });
            result.passed = false;
            result.score = RunResult::score_from_assertions(&result.assertions);
            return result;
        }
    };

    let suffix = screenshot_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("svg");
    let baseline = baseline_path(baseline_root, &result.recipe_name, &result.renderer, suffix);
    result.artifacts.insert(
        "visual_baseline".into(),
        baseline.to_string_lossy().into_owned(),
    );

    if update {
        if let Some(parent) = baseline.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&screenshot_path, &baseline);
        result.assertions.push(AssertionResult {
            name: "visual_diff".into(),
            passed: true,
            detail: format!("updated baseline: {}", baseline.display()),
        });
        result.score = RunResult::score_from_assertions(&result.assertions);
        return result;
    }

    if !baseline.exists() {
        result.assertions.push(AssertionResult {
            name: "visual_diff".into(),
            passed: false,
            detail: format!(
                "missing baseline: {}; run with --update-baselines to create it",
                baseline.display()
            ),
        });
        result.passed = false;
        result.score = RunResult::score_from_assertions(&result.assertions);
        return result;
    }

    if screenshots_match(&baseline, &screenshot_path) {
        result.assertions.push(AssertionResult {
            name: "visual_diff".into(),
            passed: true,
            detail: format!("matches baseline: {}", baseline.display()),
        });
        result.score = RunResult::score_from_assertions(&result.assertions);
        return result;
    }

    // Mismatch → write diff.
    let diff_path =
        screenshot_path.with_file_name(format!("visual-diff.{}", suffix.trim_start_matches('.')));
    // For PNG we generate a 3-panel diff; for SVG we embed side-by-side.
    if suffix.eq_ignore_ascii_case("png") {
        let _ = write_png_diff(&baseline, &screenshot_path, &diff_path);
        result.artifacts.insert(
            "visual_diff".into(),
            diff_path.to_string_lossy().into_owned(),
        );
    } else {
        let svg_diff = diff_path.with_extension("svg");
        let _ = write_svg_side_by_side(&baseline, &screenshot_path, &svg_diff);
        result.artifacts.insert(
            "visual_diff".into(),
            svg_diff.to_string_lossy().into_owned(),
        );
    }
    let diff_display = result
        .artifacts
        .get("visual_diff")
        .cloned()
        .unwrap_or_default();
    result.assertions.push(AssertionResult {
        name: "visual_diff".into(),
        passed: false,
        detail: format!(
            "visual regression: baseline={} actual={} diff={}",
            baseline.display(),
            screenshot_path.display(),
            diff_display
        ),
    });
    result.passed = false;
    result.score = RunResult::score_from_assertions(&result.assertions);
    result
}

fn baseline_path(baseline_root: &Path, recipe: &str, renderer: &str, suffix: &str) -> PathBuf {
    let safe_recipe = sanitize(recipe);
    let safe_renderer = sanitize(renderer);
    let ext = suffix.trim_start_matches('.');
    baseline_root
        .join(safe_recipe)
        .join(safe_renderer)
        .join(format!("final.{ext}"))
}

fn sanitize(value: &str) -> String {
    let s: String = value
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "default".into()
    } else {
        s
    }
}

fn screenshots_match(a: &Path, b: &Path) -> bool {
    if a.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .eq_ignore_ascii_case("png")
        && b.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .eq_ignore_ascii_case("png")
    {
        // PNG pixel comparison via `image`.
        let a_img = image::open(a).ok();
        let b_img = image::open(b).ok();
        if let (Some(a_img), Some(b_img)) = (a_img, b_img) {
            let a_rgb = a_img.to_rgb8();
            let b_rgb = b_img.to_rgb8();
            if a_rgb.dimensions() != b_rgb.dimensions() {
                return false;
            }
            return a_rgb.pixels().zip(b_rgb.pixels()).all(|(p1, p2)| p1 == p2);
        }
        // Fallback to byte comparison on decode failure.
    }
    // Byte comparison for SVG or unreadable PNGs.
    std::fs::read(a).ok() == std::fs::read(b).ok()
}

fn write_png_diff(baseline: &Path, actual: &Path, diff_path: &Path) -> std::io::Result<()> {
    let base_img = image::open(baseline)
        .map_err(std::io::Error::other)?
        .to_rgb8();
    let actual_img = image::open(actual)
        .map_err(std::io::Error::other)?
        .to_rgb8();
    let w = base_img.width().max(actual_img.width());
    let h = base_img.height().max(actual_img.height());
    // Label strip height.
    let label_h: u32 = 28;
    let mut combined = image::RgbImage::new(w * 3, h + label_h);
    // Fill white.
    for p in combined.pixels_mut() {
        *p = image::Rgb([255, 255, 255]);
    }

    // Helper to paste with padding.
    let mut paste = |img: &image::RgbImage, x_off: u32| {
        for y in 0..img.height() {
            for x in 0..img.width() {
                combined.put_pixel(x + x_off, y + label_h, *img.get_pixel(x, y));
            }
        }
    };
    // Create padded images sized w×h.
    let pad = |img: &image::RgbImage| -> image::RgbImage {
        let mut out = image::RgbImage::new(w, h);
        for p in out.pixels_mut() {
            *p = image::Rgb([255, 255, 255]);
        }
        for y in 0..img.height() {
            for x in 0..img.width() {
                out.put_pixel(x, y, *img.get_pixel(x, y));
            }
        }
        out
    };
    let base_pad = pad(&base_img);
    let actual_pad = pad(&actual_img);
    // Diff panel: absolute difference.
    let mut diff_pad = image::RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = base_pad.get_pixel(x, y);
            let b = actual_pad.get_pixel(x, y);
            diff_pad.put_pixel(
                x,
                y,
                image::Rgb([
                    (a[0] as i16 - b[0] as i16).unsigned_abs() as u8,
                    (a[1] as i16 - b[1] as i16).unsigned_abs() as u8,
                    (a[2] as i16 - b[2] as i16).unsigned_abs() as u8,
                ]),
            );
        }
    }

    paste(&base_pad, 0);
    paste(&actual_pad, w);
    paste(&diff_pad, w * 2);

    // Simple label via rectangle (full font rendering not required for correctness).
    // Combined is already white strip at top; we keep it.

    if let Some(parent) = diff_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    combined.save(diff_path).map_err(std::io::Error::other)
}

fn write_svg_side_by_side(baseline: &Path, actual: &Path, diff_path: &Path) -> std::io::Result<()> {
    let base_data = std::fs::read(baseline)?;
    let actual_data = std::fs::read(actual)?;
    // Use base64 data URIs.
    let base_b64 = base64_encode(&base_data);
    let actual_b64 = base64_encode(&actual_data);
    let base_mime = if baseline
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .eq_ignore_ascii_case("png")
    {
        "image/png"
    } else {
        "image/svg+xml"
    };
    let actual_mime = if actual
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .eq_ignore_ascii_case("png")
    {
        "image/png"
    } else {
        "image/svg+xml"
    };
    let base_uri = format!("data:{base_mime};base64,{base_b64}");
    let actual_uri = format!("data:{actual_mime};base64,{actual_b64}");
    let width = 1200;
    let height = 620;
    let panel_w = 560;
    let panel_h = 520;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#f6f8fa\"/><style>text{{font:16px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;fill:#24292f}}</style><text x=\"24\" y=\"32\">baseline</text><text x=\"616\" y=\"32\">actual</text><image href=\"{}\" x=\"24\" y=\"56\" width=\"{panel_w}\" height=\"{panel_h}\" preserveAspectRatio=\"xMinYMin meet\"/><image href=\"{}\" x=\"616\" y=\"56\" width=\"{panel_w}\" height=\"{panel_h}\" preserveAspectRatio=\"xMinYMin meet\"/></svg>",
        html_escape(&base_uri),
        html_escape(&actual_uri)
    );
    if let Some(parent) = diff_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(diff_path, svg + "\n")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn base64_encode(data: &[u8]) -> String {
    // Minimal base64 without extra crate.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if i + 1 < data.len() {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if i + 2 < data.len() {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
        i += 3;
    }
    out
}
