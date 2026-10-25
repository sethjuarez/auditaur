//! Snapshot baselines for `toMatchAriaSnapshot` and `toHaveScreenshot`, as in Playwright.
//!
//! Baselines live beside the suite: `<dir>/__snapshots__/<suite file stem>/`. ARIA snapshots are
//! text and shared across platforms (`<name>.aria.yml`); screenshots differ by OS, so each OS gets
//! its own (`<name>-<os>.png`). Unnamed snapshots are numbered per test: `<test-slug>-<n>`.
//!
//! [`decide`] maps a comparison and the update mode to an outcome. A missing baseline is written
//! (unless `--update-snapshots none`) and still fails, so CI never silently accepts one.

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};

use crate::commands::drills::DRILL_FILE_SUFFIX;

pub const SNAPSHOTS_DIR: &str = "__snapshots__";

/// When baselines are (re)written, like Playwright's `--update-snapshots`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum UpdateMode {
    /// Never write baselines.
    None,
    /// Write missing baselines (the test still fails).
    #[default]
    Missing,
    /// Write missing and mismatched baselines.
    Changed,
    /// Rewrite every baseline the run checks.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    Aria,
    Screenshot,
}

/// A resolved snapshot: which baseline a step checks and how it may update it.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotTarget {
    pub name: String,
    pub kind: SnapshotKind,
    pub baseline: PathBuf,
    /// The baseline path as reported (relative to the workspace, forward slashes).
    pub display: String,
    pub update: UpdateMode,
}

impl SnapshotTarget {
    pub fn new(suite_path: &Path, kind: SnapshotKind, name: &str, update: UpdateMode) -> Self {
        let baseline = suite_dir(suite_path).join(baseline_file(kind, name));
        let display = crate::commands::drills::display_path(&baseline);
        Self {
            name: name.to_string(),
            kind,
            baseline,
            display,
            update,
        }
    }

    pub fn read(&self) -> Result<Option<Vec<u8>>> {
        match fs::read(&self.baseline) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("failed to read snapshot `{}`", self.display))
            }
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = self.baseline.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::commands::drills::run::write_atomic(&self.baseline, bytes)
            .with_context(|| format!("failed to write snapshot `{}`", self.display))
    }
}

/// `<suite dir>/__snapshots__/<suite file stem>`.
pub fn suite_dir(suite_path: &Path) -> PathBuf {
    let file = suite_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = file.strip_suffix(DRILL_FILE_SUFFIX).unwrap_or(&file);
    suite_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(SNAPSHOTS_DIR)
        .join(stem)
}

pub fn baseline_file(kind: SnapshotKind, name: &str) -> String {
    match kind {
        SnapshotKind::Aria => format!("{name}.aria.yml"),
        SnapshotKind::Screenshot => format!("{name}-{}.png", std::env::consts::OS),
    }
}

/// `<slug>-<n>` for the `n`th unnamed snapshot of a test (or hook phase).
pub fn auto_name(base: &str, n: u32) -> String {
    format!("{}-{n}", slug(base))
}

/// Lowercase letters and digits, everything else collapsed to single dashes.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "snapshot".to_string()
    } else {
        out.to_string()
    }
}

/// How the captured value compares with the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Match,
    Mismatch,
    Missing,
}

/// What to do once a comparison is final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Pass,
    /// Write the actual value as the baseline, then pass.
    Update,
    /// Write the actual value as the new baseline, but fail.
    WriteMissing,
    Fail,
}

pub fn decide(comparison: Comparison, update: UpdateMode) -> Decision {
    match (comparison, update) {
        (Comparison::Match, UpdateMode::All) => Decision::Update,
        (Comparison::Match, _) => Decision::Pass,
        (Comparison::Missing, UpdateMode::None) => Decision::Fail,
        (Comparison::Missing, UpdateMode::Missing) => Decision::WriteMissing,
        (Comparison::Mismatch | Comparison::Missing, UpdateMode::Changed | UpdateMode::All) => {
            Decision::Update
        }
        (Comparison::Mismatch, _) => Decision::Fail,
    }
}

/// Line endings and trailing whitespace never matter in ARIA snapshots.
pub fn normalize_aria(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut out = lines.join("\n").trim_matches('\n').to_string();
    out.push('\n');
    out
}

/// A compact line diff (`- expected`, `+ actual`, `  same`), trimmed to the changed region.
pub fn line_diff(expected: &str, actual: &str) -> String {
    const CONTEXT: usize = 2;
    const MAX_CELLS: usize = 4_000_000;
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        return format!(
            "(snapshots too large to diff: {} vs {} lines)",
            a.len(),
            b.len()
        );
    }
    // Longest common subsequence table, from the end.
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut lines: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            lines.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] > lcs[i + 1][j]) {
            lines.push(('+', b[j]));
            j += 1;
        } else {
            lines.push(('-', a[i]));
            i += 1;
        }
    }
    let changed: Vec<usize> = (0..lines.len()).filter(|&k| lines[k].0 != ' ').collect();
    let mut out = Vec::new();
    let mut last_shown: Option<usize> = None;
    for k in 0..lines.len() {
        let near = changed
            .iter()
            .any(|&c| k + CONTEXT >= c && k <= c + CONTEXT);
        if !near {
            continue;
        }
        if last_shown.is_some_and(|last| k > last + 1) || (last_shown.is_none() && k > 0) {
            out.push("  ...".to_string());
        }
        let (mark, text) = lines[k];
        out.push(format!("{mark} {text}"));
        last_shown = Some(k);
    }
    if last_shown.is_some_and(|last| last + 1 < lines.len()) {
        out.push("  ...".to_string());
    }
    out.join("\n")
}

/// An 8-bit RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().context("invalid PNG")?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| anyhow!("PNG is too large"))?;
        let mut buffer = vec![0; size];
        let info = reader.next_frame(&mut buffer).context("invalid PNG")?;
        buffer.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buffer,
            png::ColorType::Rgb => buffer
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => buffer
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return Err(anyhow!("unexpected indexed PNG output")),
        };
        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(out)
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// Pixel tolerances, as in Playwright's `toHaveScreenshot`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Per-pixel YIQ color distance, 0 (exact) to 1.
    pub threshold: f64,
    pub max_diff_pixels: Option<u64>,
    pub max_diff_pixel_ratio: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ImageDiff {
    pub diff_pixels: u64,
    pub total_pixels: u64,
    /// `(expected, actual)` sizes when they differ.
    pub size_mismatch: Option<((u32, u32), (u32, u32))>,
    /// Red where pixels differ over a faded copy of the expected image.
    pub diff: Image,
    pub passed: bool,
}

impl ImageDiff {
    pub fn summary(&self) -> String {
        if let Some(((ew, eh), (aw, ah))) = self.size_mismatch {
            return format!("expected an image {ew}px by {eh}px, received {aw}px by {ah}px");
        }
        let ratio = self.diff_pixels as f64 / self.total_pixels.max(1) as f64;
        format!(
            "{} pixels (ratio {:.4} of all image pixels) are different",
            self.diff_pixels, ratio
        )
    }
}

/// Compare like pixelmatch (without anti-aliasing detection): pixels whose YIQ distance exceeds
/// the threshold differ; the image passes if no more than the allowed pixels differ.
pub fn compare_images(expected: &Image, actual: &Image, tolerance: Tolerance) -> ImageDiff {
    let width = expected.width.max(actual.width);
    let height = expected.height.max(actual.height);
    let total = u64::from(width) * u64::from(height);
    let size_mismatch = (expected.width != actual.width || expected.height != actual.height)
        .then_some((
            (expected.width, expected.height),
            (actual.width, actual.height),
        ));
    let max_delta = 35215.0 * tolerance.threshold * tolerance.threshold;
    let mut rgba = Vec::with_capacity((total * 4) as usize);
    let mut diff_pixels = 0u64;
    for y in 0..height {
        for x in 0..width {
            let inside = |image: &Image| x < image.width && y < image.height;
            let different = match (inside(expected), inside(actual)) {
                (true, true) => color_delta(expected.pixel(x, y), actual.pixel(x, y)) > max_delta,
                _ => true,
            };
            if different {
                diff_pixels += 1;
                rgba.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                let [r, g, b, a] = expected.pixel(x, y);
                let gray = blend(luma(r, g, b), a);
                let faded = (255.0 + (gray - 255.0) * 0.1) as u8;
                rgba.extend_from_slice(&[faded, faded, faded, 255]);
            }
        }
    }
    let allowed_pixels = tolerance.max_diff_pixels;
    let allowed_ratio = tolerance
        .max_diff_pixel_ratio
        .map(|ratio| (ratio * total as f64).floor() as u64);
    let allowed = match (allowed_pixels, allowed_ratio) {
        (Some(a), Some(b)) => a.min(b),
        (a, b) => a.or(b).unwrap_or(0),
    };
    ImageDiff {
        diff_pixels,
        total_pixels: total,
        size_mismatch,
        diff: Image {
            width,
            height,
            rgba,
        },
        passed: size_mismatch.is_none() && diff_pixels <= allowed,
    }
}

fn blend(channel: f64, alpha: u8) -> f64 {
    255.0 + (channel - 255.0) * f64::from(alpha) / 255.0
}

fn luma(r: u8, g: u8, b: u8) -> f64 {
    f64::from(r) * 0.298_895_31 + f64::from(g) * 0.586_622_47 + f64::from(b) * 0.114_482_23
}

/// Squared YIQ distance of two pixels blended over white (pixelmatch's `colorDelta`).
fn color_delta(a: [u8; 4], b: [u8; 4]) -> f64 {
    if a == b {
        return 0.0;
    }
    let mix = |p: [u8; 4]| {
        (
            blend(f64::from(p[0]), p[3]),
            blend(f64::from(p[1]), p[3]),
            blend(f64::from(p[2]), p[3]),
        )
    };
    let (r1, g1, b1) = mix(a);
    let (r2, g2, b2) = mix(b);
    let y = |r: f64, g: f64, b: f64| r * 0.298_895_31 + g * 0.586_622_47 + b * 0.114_482_23;
    let i = |r: f64, g: f64, b: f64| r * 0.595_977_99 - g * 0.274_176_10 - b * 0.321_801_89;
    let q = |r: f64, g: f64, b: f64| r * 0.211_470_17 - g * 0.522_617_11 + b * 0.311_146_94;
    let dy = y(r1, g1, b1) - y(r2, g2, b2);
    let di = i(r1, g1, b1) - i(r2, g2, b2);
    let dq = q(r1, g1, b1) - q(r2, g2, b2);
    0.5053 * dy * dy + 0.299 * di * di + 0.1957 * dq * dq
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, color: [u8; 4]) -> Image {
        Image {
            width,
            height,
            rgba: (0..width * height).flat_map(|_| color).collect(),
        }
    }

    fn exact() -> Tolerance {
        Tolerance {
            threshold: 0.2,
            max_diff_pixels: None,
            max_diff_pixel_ratio: None,
        }
    }

    #[test]
    fn decisions_follow_playwright_update_modes() {
        use Comparison::*;
        use Decision::*;
        let cases = [
            (Match, UpdateMode::None, Pass),
            (Match, UpdateMode::Missing, Pass),
            (Match, UpdateMode::Changed, Pass),
            (Match, UpdateMode::All, Update),
            (Missing, UpdateMode::None, Fail),
            (Missing, UpdateMode::Missing, WriteMissing),
            (Missing, UpdateMode::Changed, Update),
            (Missing, UpdateMode::All, Update),
            (Mismatch, UpdateMode::None, Fail),
            (Mismatch, UpdateMode::Missing, Fail),
            (Mismatch, UpdateMode::Changed, Update),
            (Mismatch, UpdateMode::All, Update),
        ];
        for (comparison, mode, want) in cases {
            assert_eq!(decide(comparison, mode), want, "{comparison:?} {mode:?}");
        }
    }

    #[test]
    fn baselines_live_beside_the_suite() {
        let suite = Path::new("drills").join("app").join("smoke.drill.json");
        let target = SnapshotTarget::new(&suite, SnapshotKind::Aria, "home", UpdateMode::Missing);
        assert_eq!(
            target.baseline,
            Path::new("drills")
                .join("app")
                .join("__snapshots__")
                .join("smoke")
                .join("home.aria.yml")
        );
        assert_eq!(
            baseline_file(SnapshotKind::Screenshot, "home"),
            format!("home-{}.png", std::env::consts::OS)
        );
    }

    #[test]
    fn auto_names_are_slugged_and_numbered() {
        assert_eq!(
            auto_name("Renders the  Generator!", 1),
            "renders-the-generator-1"
        );
        assert_eq!(auto_name("--", 2), "snapshot-2");
        assert_eq!(slug("a_b.c"), "a-b-c");
    }

    #[test]
    fn baselines_round_trip_and_missing_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        let suite = dir.path().join("s.drill.json");
        let target = SnapshotTarget::new(&suite, SnapshotKind::Aria, "x", UpdateMode::Missing);
        assert_eq!(target.read().unwrap(), None);
        target.write(b"- text: hi\n").unwrap();
        assert_eq!(
            target.read().unwrap().as_deref(),
            Some(&b"- text: hi\n"[..])
        );
    }

    #[test]
    fn aria_normalization_ignores_line_endings_and_trailing_space() {
        assert_eq!(
            normalize_aria("\r\n- heading \"A\"  \r\n- text: b\r\n\r\n"),
            "- heading \"A\"\n- text: b\n"
        );
    }

    #[test]
    fn line_diff_marks_changes_with_context() {
        let expected = "a\nb\nc\nd\ne\nf\ng\n";
        let actual = "a\nb\nc\nD\ne\nf\ng\n";
        assert_eq!(
            line_diff(expected, actual),
            "  ...\n  b\n  c\n- d\n+ D\n  e\n  f\n  ..."
        );
        assert_eq!(line_diff("x\n", "x\ny\n"), "  x\n+ y");
    }

    #[test]
    fn png_round_trips_and_decodes_rgb() {
        let original = image(3, 2, [10, 20, 30, 255]);
        assert_eq!(
            Image::decode(&original.encode().unwrap()).unwrap(),
            original
        );

        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[1, 2, 3]).unwrap();
        writer.finish().unwrap();
        assert_eq!(Image::decode(&out).unwrap().rgba, vec![1, 2, 3, 255]);
        assert!(Image::decode(b"not a png").is_err());
    }

    #[test]
    fn identical_and_near_identical_images_pass() {
        let a = image(4, 4, [100, 100, 100, 255]);
        assert!(compare_images(&a, &a, exact()).passed);
        let b = image(4, 4, [101, 100, 100, 255]);
        let diff = compare_images(&a, &b, exact());
        assert!(
            diff.passed,
            "a 1-level change is under the default threshold"
        );
        assert_eq!(diff.diff_pixels, 0);
    }

    #[test]
    fn differing_pixels_fail_unless_allowed() {
        let a = image(10, 10, [255, 255, 255, 255]);
        let mut b = a.clone();
        for x in 0..5 {
            let i = (x * 4) as usize;
            b.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        let diff = compare_images(&a, &b, exact());
        assert!(!diff.passed);
        assert_eq!(diff.diff_pixels, 5);
        assert_eq!(diff.diff.pixel(0, 0), [255, 0, 0, 255]);
        assert_ne!(diff.diff.pixel(9, 9), [255, 0, 0, 255]);
        assert!(diff.summary().starts_with("5 pixels (ratio 0.0500"));

        let pixels = |n| Tolerance {
            max_diff_pixels: Some(n),
            ..exact()
        };
        assert!(compare_images(&a, &b, pixels(5)).passed);
        assert!(!compare_images(&a, &b, pixels(4)).passed);
        let ratio = |r| Tolerance {
            max_diff_pixel_ratio: Some(r),
            ..exact()
        };
        assert!(compare_images(&a, &b, ratio(0.05)).passed);
        assert!(!compare_images(&a, &b, ratio(0.04)).passed);
        // Both set: the stricter wins.
        let both = Tolerance {
            max_diff_pixels: Some(100),
            max_diff_pixel_ratio: Some(0.04),
            ..exact()
        };
        assert!(!compare_images(&a, &b, both).passed);
        // A loose threshold ignores the change.
        let loose = Tolerance {
            threshold: 1.0,
            ..exact()
        };
        assert!(compare_images(&a, &b, loose).passed);
    }

    #[test]
    fn size_changes_always_fail() {
        let a = image(4, 4, [0, 0, 0, 255]);
        let b = image(4, 5, [0, 0, 0, 255]);
        let diff = compare_images(
            &a,
            &b,
            Tolerance {
                max_diff_pixels: Some(1000),
                ..exact()
            },
        );
        assert!(!diff.passed);
        assert_eq!(diff.diff_pixels, 4);
        assert_eq!(
            diff.summary(),
            "expected an image 4px by 4px, received 4px by 5px"
        );
    }

    #[test]
    fn transparency_blends_over_white() {
        let clear = image(1, 1, [0, 0, 0, 0]);
        let white = image(1, 1, [255, 255, 255, 255]);
        assert!(compare_images(&clear, &white, exact()).passed);
    }
}
