//! The Mac's HEIF reader held to `libheif`'s: each file decoded by both
//! [`super::macos::Heif`] and [`super::libheif::Heif`] and the two
//! [`DecodedImage`]s compared on what the rest of the program reads of
//! them — the size, the channels and depth, the alpha, every sample, the
//! color space, the gain map and the depth map — and on what each says
//! without decoding: the size from the header and the XMP packet.
//!
//! The fixtures are always compared. Real files are compared as well where
//! `GAMUT_HEIF_CORPUS` names them — folders, walked whole, or files, `:`
//! between — at most `GAMUT_HEIF_CORPUS_LIMIT` of them (20 unless said);
//! run with `--nocapture` to read the report. Where a collection keeps the
//! picture a file should decode to beside it, as AOM's Link-U files do,
//! each reader is scored against that instead of against the other.
//!
//! What the comparison found is written up in `docs/formats.md`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::image::decode::{Decoder, Overrides, ReadSeek};
use crate::image::gain_map::{Data, Lift};
use crate::image::{AlphaMode, DecodedImage, Samples};

/// A picture as both sides are compared in: every sample from 0 to 1, row
/// by row, `channels` to a pixel, alpha last and not multiplied in.
struct Plane {
    width: usize,
    height: usize,
    channels: usize,
    alpha: bool,
    data: Vec<f32>,
}

impl Plane {
    fn of(samples: &Samples, width: u32, height: u32, alpha: AlphaMode) -> Self {
        let channels = samples.channels();
        let n = channels.count();
        let mut data: Vec<f32> = match samples {
            Samples::U8 { data, .. } => data.iter().map(|&v| f32::from(v) / 255.0).collect(),
            Samples::U16 { data, .. } => data.iter().map(|&v| f32::from(v) / 65535.0).collect(),
            Samples::F32 { data, .. } => data.clone(),
        };
        if alpha == AlphaMode::Premultiplied {
            for pixel in data.chunks_exact_mut(n) {
                let (color, alpha) = pixel.split_at_mut(n - 1);
                if alpha[0] > 0.0 {
                    color.iter_mut().for_each(|v| *v /= alpha[0]);
                }
            }
        }
        Self {
            width: width as usize,
            height: height as usize,
            channels: n,
            alpha: channels.alpha_index().is_some(),
            data,
        }
    }

    fn of_image(image: &DecodedImage) -> Self {
        Self::of(&image.samples, image.width, image.height, image.alpha)
    }
}

/// How far apart two planes of one size are.
struct Difference {
    max: f32,
    mean: f32,
}

impl Difference {
    /// `None` where the two differ in size; a gray plane against an RGB one
    /// is gray against each of the three.
    fn of(a: &Plane, b: &Plane) -> Option<Self> {
        if (a.width, a.height) != (b.width, b.height) {
            return None;
        }
        let color = |p: &Plane| p.channels - usize::from(p.alpha);
        let mut pairs: Vec<(usize, usize)> = if color(a) == 1 && color(b) == 3 {
            vec![(0, 0), (0, 1), (0, 2)]
        } else if color(a) == 3 && color(b) == 1 {
            vec![(0, 0), (1, 0), (2, 0)]
        } else {
            (0..color(a).min(color(b))).map(|c| (c, c)).collect()
        };
        if a.alpha && b.alpha {
            pairs.push((a.channels - 1, b.channels - 1));
        }
        let (mut max, mut sum, mut count) = (0f32, 0f64, 0usize);
        for (pa, pb) in a
            .data
            .chunks_exact(a.channels)
            .zip(b.data.chunks_exact(b.channels))
        {
            for &(ca, cb) in &pairs {
                let d = (pa[ca] - pb[cb]).abs();
                let d = if d.is_nan() { 1.0 } else { d };
                max = max.max(d);
                sum += f64::from(d);
                count += 1;
            }
        }
        Some(Self {
            max,
            mean: (sum / count.max(1) as f64) as f32,
        })
    }

    fn say(&self) -> String {
        format!(
            "max {:.2} mean {:.3} codes",
            self.max * 255.0,
            self.mean * 255.0
        )
    }
}

/// The picture a test collection keeps beside a file to say what it should
/// decode to: `kimono.crop.png` beside `kimono.crop.avif`, or `fox.png`
/// beside `fox.profile0.8bpc.yuv420.avif`.
fn reference(path: &Path) -> Option<Plane> {
    let name = path.file_name()?.to_str()?;
    let stem = name.rsplit_once('.')?.0;
    let base = name.split('.').next()?;
    let png = [stem, base]
        .iter()
        .map(|s| path.with_file_name(format!("{s}.png")))
        .find(|p| p.exists())?;
    let decoded = ::image::open(png).ok()?;
    let (width, height) = (decoded.width() as usize, decoded.height() as usize);
    let full = |data: Vec<u16>| data.iter().map(|&v| f32::from(v) / 65535.0).collect();
    let (channels, alpha, data) = match (decoded.color().has_color(), decoded.color().has_alpha()) {
        (true, true) => (4, true, full(decoded.to_rgba16().into_raw())),
        (true, false) => (3, false, full(decoded.to_rgb16().into_raw())),
        _ => (1, false, full(decoded.to_luma16().into_raw())),
    };
    Some(Plane {
        width,
        height,
        channels,
        alpha,
        data,
    })
}

/// One file read both ways, as the report says it and the tests judge it.
struct Comparison {
    name: String,
    lines: Vec<String>,
    problems: Vec<String>,
    /// The mean difference of the pixels, in 8-bit codes, where both read
    /// the file at one size.
    pixels: Option<f32>,
    /// Whether the Mac's reader is as close as `libheif`'s to the picture
    /// the file came with, near enough, where it came with one.
    closer: Option<bool>,
    times: Option<(Duration, Duration)>,
}

fn decode(decoder: &dyn Decoder, path: &Path) -> anyhow::Result<(DecodedImage, Duration)> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    let started = Instant::now();
    let image = decoder.decode(&mut file, Overrides::default())?;
    Ok((image, started.elapsed()))
}

/// The fastest of three reads: the first of either pays for loading its
/// codec.
fn fastest(decoder: &dyn Decoder, path: &Path, first: Duration) -> Duration {
    (0..2).fold(first, |best, _| {
        decode(decoder, path).map_or(best, |(_, time)| best.min(time))
    })
}

/// What a decoder says of the file without decoding it.
fn ask<T>(
    decoder: &dyn Decoder,
    path: &Path,
    question: impl Fn(&dyn Decoder, &mut dyn ReadSeek) -> anyhow::Result<T>,
) -> Option<T> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    question(decoder, &mut file).ok()
}

fn kind(samples: &Samples) -> &'static str {
    match samples {
        Samples::U8 { .. } => "u8",
        Samples::U16 { .. } => "u16",
        Samples::F32 { .. } => "f32",
    }
}

fn compare(path: &Path) -> Comparison {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let (theirs, ours) = (&super::libheif::Heif, &super::macos::Heif);
    let mut comparison = Comparison {
        name,
        lines: Vec::new(),
        problems: Vec::new(),
        pixels: None,
        closer: None,
        times: None,
    };
    let ((t, t_time), (o, o_time)) = match (decode(theirs, path), decode(ours, path)) {
        (Ok(t), Ok(o)) => (t, o),
        (Err(t), Ok(_)) => {
            comparison.lines.push(format!("libheif refuses it: {t:#}"));
            return comparison;
        }
        (Ok(_), Err(o)) => {
            comparison.lines.push(format!("the Mac refuses it: {o:#}"));
            comparison.problems.push(format!("refused: {o:#}"));
            return comparison;
        }
        (Err(t), Err(o)) => {
            comparison
                .lines
                .push(format!("both refuse it: libheif {t:#}; the Mac {o:#}"));
            return comparison;
        }
    };
    comparison.times = Some((fastest(theirs, path, t_time), fastest(ours, path, o_time)));
    let lines = &mut comparison.lines;
    let problems = &mut comparison.problems;

    lines.push(format!(
        "size       libheif {}x{}  mac {}x{}",
        t.width, t.height, o.width, o.height
    ));
    if (t.width, t.height) != (o.width, o.height) {
        problems.push("the sizes differ".into());
    }
    let header = ask(ours, path, |d, s| d.dimensions(s)).flatten();
    if header != Some((o.width, o.height)) {
        problems.push(format!(
            "the Mac's header says {header:?} but it decodes to {}x{}",
            o.width, o.height
        ));
    }

    lines.push(format!(
        "layout     libheif {:?} {} {:?}  mac {:?} {} {:?}",
        t.samples.channels(),
        kind(&t.samples),
        t.alpha,
        o.samples.channels(),
        kind(&o.samples),
        o.alpha
    ));
    if t.samples.channels() != o.samples.channels() || kind(&t.samples) != kind(&o.samples) {
        problems.push("the layouts differ".into());
    }

    lines.push(format!(
        "color      libheif {}  mac {}",
        t.color.label(),
        o.color.label()
    ));
    if t.color != o.color {
        problems.push("the color spaces differ".into());
    }

    let (tp, op) = (Plane::of_image(&t), Plane::of_image(&o));
    match Difference::of(&tp, &op) {
        Some(d) => {
            lines.push(format!("pixels     {}", d.say()));
            comparison.pixels = Some(d.mean * 255.0);
        }
        None => lines.push("pixels     not compared".into()),
    }
    if let Some(expected) = reference(path)
        && let (Some(dt), Some(dm)) = (
            Difference::of(&expected, &tp),
            Difference::of(&expected, &op),
        )
    {
        lines.push(format!(
            "reference  libheif {:.3}  mac {:.3} codes from the file's own PNG",
            dt.mean * 255.0,
            dm.mean * 255.0
        ));
        comparison.closer = Some(dm.mean <= dt.mean * 1.05);
    }

    let lift = |l: &Lift| match l {
        Lift::Iso(_) => "ISO",
        Lift::Apple { .. } => "Apple",
        _ => "other",
    };
    match (&t.gain_map, &o.gain_map) {
        (None, None) => {}
        (Some(tm), Some(om)) => {
            let codes = |data: &Data| match data {
                Data::Codes(c) => c.clone(),
                Data::Ratios(_) => Vec::new(),
            };
            let same = (tm.width, tm.height, tm.channels) == (om.width, om.height, om.channels)
                && codes(&tm.data) == codes(&om.data)
                && lift(&tm.lift) == lift(&om.lift);
            lines.push(format!(
                "gain map   libheif {}x{}x{} {}  mac {}x{}x{} {}{}",
                tm.width,
                tm.height,
                tm.channels,
                lift(&tm.lift),
                om.width,
                om.height,
                om.channels,
                lift(&om.lift),
                if same { ", the same" } else { ", DIFFERENT" }
            ));
            if !same {
                problems.push("the gain maps differ".into());
            }
        }
        (t_map, o_map) => {
            lines.push(format!(
                "gain map   libheif {}  mac {}",
                if t_map.is_some() { "one" } else { "none" },
                if o_map.is_some() { "one" } else { "none" }
            ));
            problems.push("only one reader found the gain map".into());
        }
    }

    match (&t.depth, &o.depth) {
        (None, None) => {}
        (Some(td), Some(od)) => {
            let difference = Difference::of(
                &Plane::of(&td.samples, td.width, td.height, AlphaMode::Opaque),
                &Plane::of(&od.samples, od.width, od.height, AlphaMode::Opaque),
            );
            lines.push(format!(
                "depth      libheif {}x{} {}  mac {}x{} {}  {}, scale {}",
                td.width,
                td.height,
                kind(&td.samples),
                od.width,
                od.height,
                kind(&od.samples),
                difference
                    .as_ref()
                    .map_or("not compared".into(), Difference::say),
                if td.scale == od.scale {
                    "the same"
                } else {
                    "DIFFERENT"
                }
            ));
            // A disparity read back to codes may land a code either side.
            if difference.is_none_or(|d| d.max * 255.0 > 1.01) || td.scale != od.scale {
                problems.push("the depth maps differ".into());
            }
        }
        (t_depth, o_depth) => {
            lines.push(format!(
                "depth      libheif {}  mac {}",
                if t_depth.is_some() { "one" } else { "none" },
                if o_depth.is_some() { "one" } else { "none" }
            ));
            problems.push("only one reader found the depth map".into());
        }
    }

    let xmp = |d: &dyn Decoder| ask(d, path, |d, s| d.xmp(s)).flatten();
    let (tx, ox) = (xmp(theirs), xmp(ours));
    if tx.is_some() || ox.is_some() {
        lines.push(format!(
            "xmp        libheif {} bytes  mac {} bytes",
            tx.as_ref().map_or(0, Vec::len),
            ox.as_ref().map_or(0, Vec::len)
        ));
        if tx != ox {
            problems.push("the XMP packets differ".into());
        }
    }

    if let Some((t_time, o_time)) = comparison.times {
        lines.push(format!(
            "time       libheif {:.1} ms  mac {:.1} ms",
            t_time.as_secs_f64() * 1e3,
            o_time.as_secs_f64() * 1e3
        ));
    }
    comparison
}

impl Comparison {
    /// Everything that keeps the Mac's reader from standing in for
    /// `libheif`'s on this file. `tolerance` is the mean difference in
    /// 8-bit codes the pixels may show: none for a lossless file, and some
    /// for a lossy one, whose chroma the two reconstruct differently along
    /// hard edges. Being as close as `libheif` to the picture the file came
    /// with is agreement enough.
    fn judged(&self, tolerance: f32) -> Vec<String> {
        let mut problems = self.problems.clone();
        if let Some(mean) = self.pixels
            && mean > tolerance
            && self.closer != Some(true)
        {
            problems.push(format!("the pixels are {mean:.3} codes apart on average"));
        }
        problems
    }
}

fn report(comparisons: &[Comparison]) {
    for c in comparisons {
        println!("{}", c.name);
        for line in &c.lines {
            println!("  {line}");
        }
    }
    let timed: Vec<_> = comparisons.iter().filter_map(|c| c.times).collect();
    let (theirs, ours) = timed.iter().fold((0.0, 0.0), |(a, b), (t, o)| {
        (a + t.as_secs_f64(), b + o.as_secs_f64())
    });
    println!(
        "total      libheif {:.0} ms  mac {:.0} ms over the {} of {} files both read",
        theirs * 1e3,
        ours * 1e3,
        timed.len(),
        comparisons.len()
    );
}

fn is_heif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| super::EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Every HEIF under `directory`, however deep.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, found);
        } else if is_heif(&path) {
            found.push(path);
        }
    }
}

fn fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir("test_images")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| is_heif(path))
        .collect();
    paths.sort();
    paths
}

fn corpus() -> Vec<PathBuf> {
    let Ok(named) = std::env::var("GAMUT_HEIF_CORPUS") else {
        return Vec::new();
    };
    let limit = std::env::var("GAMUT_HEIF_CORPUS_LIMIT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    let mut paths = Vec::new();
    for part in named.split(':').map(PathBuf::from) {
        if part.is_dir() {
            let mut found = Vec::new();
            walk(&part, &mut found);
            found.sort();
            paths.extend(found);
        } else {
            paths.push(part);
        }
    }
    paths.truncate(limit);
    paths
}

/// The fixtures, read both ways. Every one agrees exactly, but for two
/// whose differences are known: `heic-icc-p3.heic`, the one fixture coded
/// lossy at 4:2:0, whose chroma the readers reconstruct differently along
/// its hard edges; and `heic-gray-alpha8.heic`, a gray picture with an
/// alpha plane, which ImageIO does not decode at all — nor does `sips`,
/// nor Preview — and which the Mac refuses. Should either change, this
/// says so.
#[test]
fn the_mac_reads_the_fixtures_as_libheif_does() {
    let comparisons: Vec<Comparison> = fixtures().iter().map(|path| compare(path)).collect();
    report(&comparisons);
    for c in &comparisons {
        let tolerance = if c.name == "heic-icc-p3.heic" {
            6.0
        } else {
            0.0
        };
        let problems = c.judged(tolerance);
        if c.name == "heic-gray-alpha8.heic" {
            assert!(
                problems.iter().any(|p| p.starts_with("refused")),
                "ImageIO now decodes a gray HEIC with alpha: {problems:?}"
            );
            continue;
        }
        assert!(problems.is_empty(), "{}: {}", c.name, problems.join("; "));
    }
}

/// Real files, where `GAMUT_HEIF_CORPUS` names some: every one compared,
/// and every one where the Mac's reader could not stand in for `libheif`'s
/// named at the end, with why. A phone's photograph is lossy at 4:2:0, so
/// its pixels may differ by up to `GAMUT_HEIF_TOLERANCE` codes on average
/// (1.5 unless said). A file `libheif` refuses is reported and not held
/// against the Mac.
#[test]
fn the_mac_reads_the_corpus_as_libheif_does() {
    let paths = corpus();
    if paths.is_empty() {
        return;
    }
    let tolerance = std::env::var("GAMUT_HEIF_TOLERANCE")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1.5);
    let comparisons: Vec<Comparison> = paths.iter().map(|path| compare(path)).collect();
    report(&comparisons);
    let failing: Vec<String> = comparisons
        .iter()
        .filter_map(|c| {
            let problems = c.judged(tolerance);
            (!problems.is_empty()).then(|| format!("{}: {}", c.name, problems.join("; ")))
        })
        .collect();
    assert!(
        failing.is_empty(),
        "{} of {} files:\n{}",
        failing.len(),
        comparisons.len(),
        failing.join("\n")
    );
}
