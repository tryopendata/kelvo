//! Draws a [`TrayFrame`] into an RGBA template image (design-system.md, Tray icon spec).
//!
//! Shapes go through `tiny-skia`, text through `ab_glyph` in the bundled JetBrains Mono.
//! Everything is drawn in black with coverage in alpha: macOS uses only the alpha of a
//! template image and tints it for light, dark and highlighted menu bars.
//!
//! tray-icon shows the image 18 pt tall and scales its width to match, so the image is
//! `18 * scale` pixels tall and every coordinate below is in points times `scale`.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

use super::model::{
    BAR_HEIGHT_PT, CORE_HEIGHT_PT, Graph, RATE_MIN_CHARS, TEMP_MIN_CHARS, TrayFrame, min_chars,
};

/// Image height in points (tray-icon's fixed status item image height).
pub const HEIGHT_PT: f32 = 18.0;
const BAR_TOP_PT: f32 = (HEIGHT_PT - BAR_HEIGHT_PT as f32) / 2.0;
const BAR_WIDTH_PT: f32 = 3.0;
const BAR_GAP_PT: f32 = 4.0;
const BAR_RADIUS_PT: f32 = 1.0;
const TRACK_ALPHA: f32 = 0.3;
/// Between the bars and the temperature, and between a stacked label and its value.
const INNER_GAP_PT: f32 = 4.0;
/// Between one element and the next.
const GROUP_GAP_PT: f32 = 10.0;
/// Values at the menu bar text size.
const VALUE_EM_PT: f32 = 12.0;
/// Stacked labels: 6.5 pt letters on a 6 pt pitch, in a 6 pt wide column.
const LABEL_EM_PT: f32 = 6.5;
const LABEL_PITCH_PT: f32 = 6.0;
const LABEL_COLUMN_PT: f32 = 6.0;

// Graphs in own items ("Graphs" and "Cores + histogram"), in points.
/// The sparkline and history boxes: 32 x 16 pt, 1 pt outline at 35%, radius 2.
const BOX_WIDTH_PT: f32 = 32.0;
const BOX_HEIGHT_PT: f32 = 16.0;
const BOX_RADIUS_PT: f32 = 2.0;
const BOX_ALPHA: f32 = 0.35;
/// Sparkline: 1.2 pt line, first sample 2 pt in, 1.47 pt per sample, bottom 14 pt down.
const SPARK_WIDTH_PT: f32 = 1.2;
const SPARK_LEFT_PT: f32 = 2.0;
const SPARK_STEP_PT: f32 = 1.47;
const SPARK_BOTTOM_PT: f32 = 14.0;
/// History bars: 2 pt wide on a 3 pt pitch, the first 3 pt in.
const HIST_LEFT_PT: f32 = 3.0;
const HIST_PITCH_PT: f32 = 3.0;
const HIST_WIDTH_PT: f32 = 2.0;
/// Memory gauge: 6 x 16 pt, outline at 45%, radius 1.5, a 3 pt fill inside.
const GAUGE_WIDTH_PT: f32 = 6.0;
const GAUGE_RADIUS_PT: f32 = 1.5;
const GAUGE_ALPHA: f32 = 0.45;
/// Per-core strip: 2 pt bars on a 3 pt pitch, 3 pt more between core kinds, track 25%.
const CORE_WIDTH_PT: f32 = 2.0;
const CORE_PITCH_PT: f32 = 3.0;
const CLUSTER_GAP_PT: f32 = 3.0;
const CORE_TRACK_ALPHA: f32 = 0.25;
/// Network rates: two lines of 8 pt text (design-system.md) on a 9 pt pitch. 9 px text
/// on a 10 px line does not fit the 18 pt image once "/" reaches past the caps.
const RATE_EM_PT: f32 = 8.0;
const RATE_PITCH_PT: f32 = 9.0;

struct Fonts {
    regular: FontRef<'static>,
    medium: FontRef<'static>,
}

static REGULAR: &[u8] = include_bytes!("../../fonts/JetBrainsMonoNL-Regular.ttf");
static MEDIUM: &[u8] = include_bytes!("../../fonts/JetBrainsMonoNL-Medium.ttf");

fn fonts() -> Option<&'static Fonts> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Some(Fonts {
                regular: FontRef::try_from_slice(REGULAR).ok()?,
                medium: FontRef::try_from_slice(MEDIUM).ok()?,
            })
        })
        .as_ref()
}

/// A rendered icon: straight-alpha RGBA, `width * height * 4` bytes.
#[derive(Debug)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the bundled tray font could not be parsed")]
    Font,
    #[error("could not allocate a {0}x{1} tray image")]
    Pixmap(u32, u32),
}

/// `ab_glyph` scales by the font's line height; this converts a CSS-style em size.
fn px_scale(font: &FontRef<'_>, em_px: f32) -> PxScale {
    let upem = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(em_px * font.height_unscaled() / upem)
}

fn text_width(font: &FontRef<'_>, em_px: f32, text: &str) -> f32 {
    let scaled = font.as_scaled(px_scale(font, em_px));
    text.chars()
        .map(|c| scaled.h_advance(font.glyph_id(c)))
        .sum()
}

/// Cap height in px for `em_px`, used to centre digits and capitals vertically.
fn cap_height(font: &FontRef<'_>, em_px: f32) -> f32 {
    let upem = font.units_per_em().unwrap_or(1000.0);
    let cap = font
        .outline(font.glyph_id('H'))
        // ab_glyph reports outline bounds with min.y at the top (730) and max.y at the
        // baseline (0), so the height is the absolute difference.
        .map_or(730.0, |o| (o.bounds.max.y - o.bounds.min.y).abs());
    cap / upem * em_px
}

/// How far `c`'s outline reaches above the baseline, in px at `em_px`.
fn ink_above(font: &FontRef<'_>, em_px: f32, c: char) -> f32 {
    let upem = font.units_per_em().unwrap_or(1000.0);
    // As in `cap_height`: min.y and max.y bound the outline, either way up.
    font.outline(font.glyph_id(c)).map_or(0.73 * em_px, |o| {
        o.bounds.min.y.max(o.bounds.max.y) / upem * em_px
    })
}

/// How far above the baseline `c`'s outline ends at the bottom (negative below it).
fn ink_above_bottom(font: &FontRef<'_>, em_px: f32, c: char) -> f32 {
    let upem = font.units_per_em().unwrap_or(1000.0);
    font.outline(font.glyph_id(c))
        .map_or(0.0, |o| o.bounds.min.y.min(o.bounds.max.y) / upem * em_px)
}

/// Composites glyph coverage over the pixmap (black, so only alpha changes).
fn draw_text(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    em_px: f32,
    x: f32,
    baseline: f32,
    text: &str,
) {
    let scale = px_scale(font, em_px);
    let scaled = font.as_scaled(scale);
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let data = pixmap.data_mut();
    let mut caret = x;
    for c in text.chars() {
        let id = font.glyph_id(c);
        let glyph = id.with_scale_and_position(scale, point(caret, baseline));
        caret += scaled.h_advance(id);
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let b = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let px = b.min.x as i32 + gx as i32;
            let py = b.min.y as i32 + gy as i32;
            if px < 0 || py < 0 || px >= w || py >= h {
                return;
            }
            let i = ((py * w + px) * 4 + 3) as usize;
            if let Some(a) = data.get_mut(i) {
                let src = coverage.clamp(0.0, 1.0);
                let dst = f32::from(*a) / 255.0;
                *a = ((src + dst * (1.0 - src)) * 255.0).round() as u8;
            }
        });
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r == 0.0 {
        return Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill(pixmap: &mut Pixmap, path: Option<tiny_skia::Path>, alpha: f32) {
    let Some(path) = path else { return };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba(0.0, 0.0, 0.0, alpha).unwrap_or(Color::BLACK));
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn stroke(pixmap: &mut Pixmap, path: Option<tiny_skia::Path>, alpha: f32, width: f32) {
    let Some(path) = path else { return };
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba(0.0, 0.0, 0.0, alpha).unwrap_or(Color::BLACK));
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// One thing to draw, with its width in px, laid out left to right.
enum Item<'a> {
    Bars(&'a [Option<u16>]),
    /// Text and the characters of width it reserves.
    Text(&'a str, usize),
    Labeled(&'a str, &'a str),
    Graph(&'a Graph),
}

/// Width in points of a per-core strip.
fn cores_width_pt(clusters: &[Vec<Option<u16>>]) -> f32 {
    let bars: usize = clusters.iter().map(Vec::len).sum();
    let groups = clusters.iter().filter(|c| !c.is_empty()).count();
    if bars == 0 {
        return 0.0;
    }
    bars as f32 * CORE_PITCH_PT - (CORE_PITCH_PT - CORE_WIDTH_PT)
        + groups.saturating_sub(1) as f32 * CLUSTER_GAP_PT
}

/// Draws a stacked three-letter label in the label column at `x`.
fn draw_label(pixmap: &mut Pixmap, font: &FontRef<'_>, s: f32, x: f32, label: &str) {
    let label_em = LABEL_EM_PT * s;
    let label_cap = cap_height(font, label_em);
    let label_top = (HEIGHT_PT * s - (2.0 * LABEL_PITCH_PT * s + label_cap)) / 2.0;
    for (k, c) in label.chars().take(3).enumerate() {
        let mut buf = [0u8; 4];
        let ch = c.encode_utf8(&mut buf);
        let cw = text_width(font, label_em, ch);
        let cx = x + (LABEL_COLUMN_PT * s - cw) / 2.0;
        let base = label_top + label_cap + k as f32 * LABEL_PITCH_PT * s;
        draw_text(pixmap, font, label_em, cx, base, ch);
    }
}

pub fn render(frame: &TrayFrame) -> Result<Rendered, RenderError> {
    let fonts = fonts().ok_or(RenderError::Font)?;
    let s = frame.scale.max(1) as f32;
    let value_em = VALUE_EM_PT * s;
    let rate_em = RATE_EM_PT * s;

    let mut items = Vec::new();
    if !frame.bars.is_empty() {
        items.push(Item::Bars(&frame.bars));
    }
    if let Some(t) = &frame.combined_text {
        items.push(Item::Text(t, TEMP_MIN_CHARS));
    }
    for v in &frame.values {
        items.push(Item::Labeled(v.label, &v.text));
    }
    for g in &frame.graphs {
        items.push(Item::Graph(g));
    }

    // Values reserve a minimum width and sit right-aligned in it, so the item does not
    // change width as digits come and go.
    let digit = text_width(&fonts.regular, value_em, "0");
    let value_width =
        |t: &str, min: usize| text_width(&fonts.regular, value_em, t).max(min as f32 * digit);
    let rate_digit = text_width(&fonts.regular, rate_em, "0");
    let rate_width =
        |t: &str| text_width(&fonts.regular, rate_em, t).max(RATE_MIN_CHARS as f32 * rate_digit);
    // The stacked label and the gap after it.
    let labelled = (LABEL_COLUMN_PT + INNER_GAP_PT) * s;
    let width_of = |item: &Item<'_>| -> f32 {
        match item {
            Item::Bars(b) => {
                let n = b.len() as f32;
                (n * BAR_WIDTH_PT + (n - 1.0).max(0.0) * BAR_GAP_PT) * s
            }
            Item::Text(t, min) => value_width(t, *min),
            Item::Labeled(l, t) => labelled + value_width(t, min_chars(l)),
            Item::Graph(g) => match g {
                Graph::Spark { .. } | Graph::Hist { .. } => labelled + BOX_WIDTH_PT * s,
                Graph::Gauge { text, .. } => {
                    labelled + (GAUGE_WIDTH_PT + INNER_GAP_PT) * s + value_width(text, 3)
                }
                Graph::Rates { up, down } => rate_width(up).max(rate_width(down)),
                Graph::Cores { clusters, .. } => labelled + cores_width_pt(clusters) * s,
            },
        }
    };
    let gap_before = |prev: Option<&Item<'_>>, item: &Item<'_>| -> f32 {
        match (prev, item) {
            (None, _) => 0.0,
            (Some(Item::Bars(_)), Item::Text(..)) => INNER_GAP_PT * s,
            _ => GROUP_GAP_PT * s,
        }
    };
    let mut total = 0.0;
    for (i, item) in items.iter().enumerate() {
        total += gap_before(i.checked_sub(1).and_then(|p| items.get(p)), item) + width_of(item);
    }
    let width = (total.ceil() as u32).max(1);
    let height = (HEIGHT_PT * s).round() as u32;
    let mut pixmap = Pixmap::new(width, height).ok_or(RenderError::Pixmap(width, height))?;

    let value_baseline = ((HEIGHT_PT * s + cap_height(&fonts.regular, value_em)) / 2.0).round();
    // Graph boxes are 16 pt tall, centred.
    let box_top = (HEIGHT_PT - BOX_HEIGHT_PT) / 2.0 * s;

    let mut x = 0.0;
    for (i, item) in items.iter().enumerate() {
        x += gap_before(i.checked_sub(1).and_then(|p| items.get(p)), item);
        match item {
            Item::Bars(bars) => {
                let top = BAR_TOP_PT * s;
                let bar_h = BAR_HEIGHT_PT as f32 * s;
                let (bw, r) = (BAR_WIDTH_PT * s, BAR_RADIUS_PT * s);
                for (j, fill_px) in bars.iter().enumerate() {
                    let bx = x + j as f32 * (BAR_WIDTH_PT + BAR_GAP_PT) * s;
                    fill(
                        &mut pixmap,
                        rounded_rect(bx, top, bw, bar_h, r),
                        TRACK_ALPHA,
                    );
                    if let Some(px) = fill_px.filter(|&p| p > 0) {
                        let h = f32::from(px);
                        fill(
                            &mut pixmap,
                            rounded_rect(bx, top + bar_h - h, bw, h, r),
                            1.0,
                        );
                    }
                }
            }
            Item::Text(t, min) => {
                let tx = x + value_width(t, *min) - text_width(&fonts.regular, value_em, t);
                draw_text(&mut pixmap, &fonts.regular, value_em, tx, value_baseline, t);
            }
            Item::Labeled(label, t) => {
                draw_label(&mut pixmap, &fonts.medium, s, x, label);
                let vx = x + labelled + value_width(t, min_chars(label))
                    - text_width(&fonts.regular, value_em, t);
                draw_text(&mut pixmap, &fonts.regular, value_em, vx, value_baseline, t);
            }
            Item::Graph(g) => {
                let gx = x + labelled;
                // The box outline: 1 pt stroke on the half point.
                let outline = |pixmap: &mut Pixmap, w_pt: f32, r_pt: f32, alpha: f32| {
                    stroke(
                        pixmap,
                        rounded_rect(
                            gx + 0.5 * s,
                            box_top + 0.5 * s,
                            (w_pt - 1.0) * s,
                            (BOX_HEIGHT_PT - 1.0) * s,
                            r_pt * s,
                        ),
                        alpha,
                        s,
                    );
                };
                // Bottom edge of the box's fill area (y 14.5 of 16).
                let fill_bottom = box_top + (BOX_HEIGHT_PT - 1.5) * s;
                match g {
                    Graph::Spark { label, points } => {
                        draw_label(&mut pixmap, &fonts.medium, s, x, label);
                        outline(&mut pixmap, BOX_WIDTH_PT, BOX_RADIUS_PT, BOX_ALPHA);
                        let base = box_top + SPARK_BOTTOM_PT * s;
                        let step = SPARK_STEP_PT * s;
                        let at = |j: usize, p: u16| {
                            (
                                gx + SPARK_LEFT_PT * s + j as f32 * step,
                                base - f32::from(p),
                            )
                        };
                        // One path per run of samples; a gap breaks the line.
                        let mut pb = PathBuilder::new();
                        let mut run = 0usize;
                        for (j, p) in points.iter().enumerate() {
                            match p {
                                Some(p) => {
                                    let (px, py) = at(j, *p);
                                    if run == 0 {
                                        pb.move_to(px, py);
                                    } else {
                                        pb.line_to(px, py);
                                    }
                                    run += 1;
                                    // A lone sample between gaps is a dot, not nothing.
                                    let alone = points.get(j + 1).is_none_or(Option::is_none);
                                    if run == 1 && alone {
                                        pb.push_circle(px, py, SPARK_WIDTH_PT * s / 2.0);
                                    }
                                }
                                None => run = 0,
                            }
                        }
                        stroke(&mut pixmap, pb.finish(), 1.0, SPARK_WIDTH_PT * s);
                    }
                    Graph::Hist { label, bars } => {
                        draw_label(&mut pixmap, &fonts.medium, s, x, label);
                        outline(&mut pixmap, BOX_WIDTH_PT, BOX_RADIUS_PT, BOX_ALPHA);
                        for (j, h) in bars.iter().enumerate() {
                            let Some(h) = h.filter(|&h| h > 0) else {
                                continue;
                            };
                            let bx = gx + (HIST_LEFT_PT + j as f32 * HIST_PITCH_PT) * s;
                            let h = f32::from(h);
                            let rect = Rect::from_xywh(bx, fill_bottom - h, HIST_WIDTH_PT * s, h);
                            fill(&mut pixmap, rect.map(PathBuilder::from_rect), 1.0);
                        }
                    }
                    Graph::Gauge {
                        label,
                        fill: f,
                        text,
                    } => {
                        draw_label(&mut pixmap, &fonts.medium, s, x, label);
                        outline(&mut pixmap, GAUGE_WIDTH_PT, GAUGE_RADIUS_PT, GAUGE_ALPHA);
                        if let Some(h) = f.filter(|&h| h > 0) {
                            let h = f32::from(h);
                            let path = rounded_rect(
                                gx + 1.5 * s,
                                fill_bottom - h,
                                (GAUGE_WIDTH_PT - 3.0) * s,
                                h,
                                0.5 * s,
                            );
                            fill(&mut pixmap, path, 1.0);
                        }
                        let tx = gx + (GAUGE_WIDTH_PT + INNER_GAP_PT) * s + value_width(text, 3)
                            - text_width(&fonts.regular, value_em, text);
                        draw_text(
                            &mut pixmap,
                            &fonts.regular,
                            value_em,
                            tx,
                            value_baseline,
                            text,
                        );
                    }
                    Graph::Rates { up, down } => {
                        let w = rate_width(up).max(rate_width(down));
                        // Centre the ink, from the top of the first line to the bottom of
                        // the second; "/" and the arrows reach past the caps and baseline.
                        // Fixed glyphs, so the lines do not move as the digits change.
                        let above = "0/\u{2191}"
                            .chars()
                            .map(|c| ink_above(&fonts.regular, rate_em, c))
                            .fold(0.0, f32::max);
                        let below = "0/\u{2193}"
                            .chars()
                            .map(|c| -ink_above_bottom(&fonts.regular, rate_em, c))
                            .fold(0.0, f32::max);
                        let pitch = RATE_PITCH_PT * s;
                        let first =
                            ((HEIGHT_PT * s - (above + pitch + below)) / 2.0 + above).round();
                        for (k, t) in [up, down].into_iter().enumerate() {
                            let tx = x + w - text_width(&fonts.regular, rate_em, t);
                            let base = first + k as f32 * pitch;
                            draw_text(&mut pixmap, &fonts.regular, rate_em, tx, base, t);
                        }
                    }
                    Graph::Cores { label, clusters } => {
                        draw_label(&mut pixmap, &fonts.medium, s, x, label);
                        let bar_h = CORE_HEIGHT_PT as f32 * s;
                        let mut bx = gx;
                        for (c, cluster) in clusters.iter().filter(|c| !c.is_empty()).enumerate() {
                            if c > 0 {
                                bx += CLUSTER_GAP_PT * s;
                            }
                            for h in cluster {
                                let track = Rect::from_xywh(bx, box_top, CORE_WIDTH_PT * s, bar_h);
                                fill(
                                    &mut pixmap,
                                    track.map(PathBuilder::from_rect),
                                    CORE_TRACK_ALPHA,
                                );
                                if let Some(h) = h.filter(|&h| h > 0) {
                                    let h = f32::from(h);
                                    let bar = Rect::from_xywh(
                                        bx,
                                        box_top + bar_h - h,
                                        CORE_WIDTH_PT * s,
                                        h,
                                    );
                                    fill(&mut pixmap, bar.map(PathBuilder::from_rect), 1.0);
                                }
                                bx += CORE_PITCH_PT * s;
                            }
                        }
                    }
                }
            }
        }
        x += width_of(item);
    }

    Ok(Rendered {
        width,
        height,
        // Black premultiplied by alpha is still black: already straight alpha.
        rgba: pixmap.take(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::model::Labeled;

    fn alpha(r: &Rendered, x: u32, y: u32) -> u8 {
        r.rgba[((y * r.width + x) * 4 + 3) as usize]
    }

    fn combined(scale: u32, bars: Vec<Option<u16>>, text: Option<&str>) -> TrayFrame {
        TrayFrame {
            scale,
            bars,
            combined_text: text.map(str::to_owned),
            values: Vec::new(),
            graphs: Vec::new(),
        }
    }

    #[test]
    fn combined_glyph_geometry_matches_the_spec() {
        // Bars only: 3 * 3 pt + 2 * 4 pt = 17 pt wide, 18 pt tall.
        let r = render(&combined(2, vec![Some(5), Some(10), Some(28)], None)).unwrap();
        assert_eq!((r.width, r.height), (34, 36));
        let r1 = render(&combined(1, vec![Some(3), Some(5), Some(14)], None)).unwrap();
        assert_eq!((r1.width, r1.height), (17, 18));

        // Inside the first bar (x 0..6 at 2x), track rows sit at 30% alpha, fill rows at
        // full. Bars span y 4..32; a 5 px fill covers 27..32.
        let track = alpha(&r, 3, 12);
        assert!((70..=84).contains(&track), "track alpha {track}");
        assert_eq!(alpha(&r, 3, 29), 255);
        // The gap between bars is empty.
        assert_eq!(alpha(&r, 9, 20), 0);
        // The third bar is full to the top (inside the rounded corner).
        assert_eq!(alpha(&r, 31, 10), 255);
        // Nothing above or below the 14 pt bars.
        assert_eq!(alpha(&r, 3, 1), 0);
        assert_eq!(alpha(&r, 3, 34), 0);
    }

    #[test]
    fn temperature_text_follows_the_bars() {
        let bars = render(&combined(2, vec![Some(5); 3], None)).unwrap();
        let r = render(&combined(2, vec![Some(5); 3], Some("61°"))).unwrap();
        // 4 pt gap plus three mono advances at 12 pt (0.6 em each, about 43 px at 2x).
        assert!(
            r.width > bars.width + 8 + 36,
            "{} vs {}",
            r.width,
            bars.width
        );
        let ink: u32 = (bars.width + 8..r.width)
            .flat_map(|x| (0..r.height).map(move |y| (x, y)))
            .map(|(x, y)| u32::from(alpha(&r, x, y)))
            .sum();
        assert!(ink > 0, "the temperature drew nothing");
        // Digits are centred like the bars: no ink in the 2 pt above or below them.
        let rows_ink = |y0: u32, y1: u32| -> u32 {
            (bars.width + 8..r.width)
                .flat_map(|x| (y0..y1).map(move |y| (x, y)))
                .map(|(x, y)| u32::from(alpha(&r, x, y)))
                .sum()
        };
        assert_eq!(rows_ink(0, 4), 0, "text above the bars");
        assert_eq!(rows_ink(32, 36), 0, "text below the bars");
    }

    #[test]
    fn values_layout_draws_labels_and_values() {
        let frame = TrayFrame {
            scale: 2,
            bars: Vec::new(),
            combined_text: None,
            graphs: Vec::new(),
            values: vec![
                Labeled {
                    label: "CPU",
                    text: "18%".into(),
                },
                Labeled {
                    label: "PWR",
                    text: "14.8W".into(),
                },
            ],
        };
        let r = render(&frame).unwrap();
        assert_eq!(r.height, 36);
        // The label column (12 px at 2x) has ink in its top and bottom thirds.
        let col_ink = |y0: u32, y1: u32| -> u32 {
            (0..12)
                .flat_map(|x| (y0..y1).map(move |y| (x, y)))
                .map(|(x, y)| u32::from(alpha(&r, x, y)))
                .sum()
        };
        assert!(col_ink(0, 12) > 0 && col_ink(24, 36) > 0);
    }

    #[test]
    fn width_does_not_change_with_the_values() {
        let w = |text: &str| {
            render(&combined(2, vec![None; 3], Some(text)))
                .unwrap()
                .width
        };
        assert_eq!(w("61°"), w("\u{2013}"), "paused keeps the width");
        assert_eq!(w("61°"), w("9°"));
        let v = |text: &str| {
            render(&TrayFrame {
                scale: 2,
                bars: Vec::new(),
                combined_text: None,
                graphs: Vec::new(),
                values: vec![Labeled {
                    label: "CPU",
                    text: text.into(),
                }],
            })
            .unwrap()
            .width
        };
        assert_eq!(v("9%"), v("18%"));
    }

    #[test]
    fn same_frame_renders_the_same_image() {
        let f = combined(2, vec![Some(5), None, Some(12)], Some("–"));
        assert_eq!(render(&f).unwrap().rgba, render(&f).unwrap().rgba);
    }

    fn graph(scale: u32, g: Graph) -> Rendered {
        let mut f = TrayFrame::empty(scale);
        f.graphs.push(g);
        render(&f).unwrap()
    }

    fn ink(r: &Rendered, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> u32 {
        xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
            .map(|(x, y)| u32::from(alpha(r, x, y)))
            .sum()
    }

    // At 2x the stacked label and its gap take 20 px, and the 16 pt boxes start 2 px down.

    #[test]
    fn sparkline_geometry_matches_the_spec() {
        let r = graph(
            2,
            Graph::Spark {
                label: "CPU",
                points: vec![Some(12); 20],
            },
        );
        // Label column, 4 pt gap, 32 pt box; 18 pt tall.
        assert_eq!((r.width, r.height), (84, 36));
        assert!(ink(&r, 0..12, 0..36) > 0, "the CPU label");
        // The 1 pt outline at 35%, on the box's top edge (y 2..4).
        let edge = alpha(&r, 50, 2);
        assert!((80..=95).contains(&edge), "outline alpha {edge}");
        // 50% of the 12 pt travel: a flat line 12 px above the bottom (y 30), at y 18,
        // from the first sample (x 24) to the last (x 80).
        assert_eq!(alpha(&r, 30, 18), 255);
        assert_eq!(alpha(&r, 78, 18), 255);
        assert_eq!(alpha(&r, 50, 10), 0);
        assert_eq!(alpha(&r, 50, 26), 0);
    }

    #[test]
    fn sparkline_breaks_at_gaps_and_dots_a_lone_sample() {
        let mut points = vec![Some(12); 10];
        points.extend(vec![None; 10]);
        points[15] = Some(24);
        let r = graph(
            2,
            Graph::Spark {
                label: "CPU",
                points,
            },
        );
        assert_eq!(alpha(&r, 30, 18), 255, "the run before the gap");
        // No line across the gap (sample 9 ends near x 51; sample 15 is at x 68).
        assert_eq!(ink(&r, 54..64, 5..31), 0, "interpolated across a gap");
        // Sample 15 alone, at the top of the travel (y 6).
        assert!(ink(&r, 66..71, 4..9) > 0, "a lone sample drew nothing");
    }

    #[test]
    fn history_bars_geometry_matches_the_spec() {
        let mut bars = vec![None; 9];
        bars[0] = Some(26);
        bars[1] = Some(13);
        bars[8] = Some(4);
        let r = graph(2, Graph::Hist { label: "GPU", bars });
        assert_eq!((r.width, r.height), (84, 36));
        // Bars 2 pt wide on a 3 pt pitch from 3 pt in (x 26, 32, ... 74), bottom at
        // 14.5 pt into the box (y 31), 13 pt at most (26 px).
        assert_eq!(alpha(&r, 27, 6), 255, "a full bar reaches 26 px up");
        assert_eq!(alpha(&r, 27, 4), 0);
        assert_eq!(alpha(&r, 33, 20), 255);
        assert_eq!(alpha(&r, 33, 16), 0, "half a bar stops at 13 px");
        assert_eq!(ink(&r, 30..32, 5..31), 0, "1 pt between bars");
        assert_eq!(ink(&r, 38..42, 5..31), 0, "a gap is no bar");
        assert_eq!(alpha(&r, 75, 29), 255, "the newest sample at the right");
        assert_eq!(alpha(&r, 27, 31), 0, "nothing below the bars");
    }

    #[test]
    fn memory_gauge_fills_from_the_bottom_with_its_value() {
        let r = graph(
            2,
            Graph::Gauge {
                label: "MEM",
                fill: Some(13),
                text: "42%".into(),
            },
        );
        // Label, gauge 6 pt, 4 pt gap, three digits of value.
        assert!(r.width > 20 + 20 + 36, "{}", r.width);
        // The 3 pt fill (x 23..29) from y 31 up 13 px.
        assert_eq!(alpha(&r, 25, 25), 255);
        assert_eq!(alpha(&r, 25, 15), 0, "above the fill");
        // The outline at 45% on the gauge's left edge (x 20..22).
        let edge = alpha(&r, 20, 18);
        assert!((100..=125).contains(&edge), "gauge outline alpha {edge}");
        assert!(ink(&r, 40..r.width, 0..36) > 0, "the value");
    }

    #[test]
    fn core_strip_has_a_cluster_gap_and_tracks() {
        let r = graph(
            2,
            Graph::Cores {
                label: "CPU",
                clusters: vec![vec![Some(32), Some(2)], vec![Some(16)]],
            },
        );
        // Three 2 pt bars on a 3 pt pitch plus the 3 pt cluster gap: 11 pt after the label.
        assert_eq!((r.width, r.height), (42, 36));
        // P0 full height (y 2..34).
        assert_eq!(alpha(&r, 21, 3), 255);
        // P1: 25% track above a 2 px fill.
        let track = alpha(&r, 27, 3);
        assert!((58..=70).contains(&track), "core track alpha {track}");
        assert_eq!(alpha(&r, 27, 33), 255);
        // 1 pt between cores, 4 pt between P and E (x 30..38).
        assert_eq!(ink(&r, 24..26, 2..34), 0);
        assert_eq!(ink(&r, 30..38, 0..36), 0, "the cluster gap");
        // E0 half full.
        assert_eq!(alpha(&r, 39, 25), 255);
        assert!((58..=70).contains(&alpha(&r, 39, 10)));
    }

    #[test]
    fn network_rates_stack_in_two_lines_at_a_steady_width() {
        let fonts = fonts().unwrap();
        for c in ['\u{2191}', '\u{2193}'] {
            assert_ne!(fonts.regular.glyph_id(c).0, 0, "the font has no {c}");
        }
        let rates = |up: &str, down: &str| {
            graph(
                2,
                Graph::Rates {
                    up: up.into(),
                    down: down.into(),
                },
            )
        };
        let r = rates("1.2 MB/s \u{2191}", "38.4 MB/s \u{2193}");
        assert!(ink(&r, 0..r.width, 0..17) > 0, "the up line");
        assert!(ink(&r, 0..r.width, 19..36) > 0, "the down line");
        // The arrows reach above the caps and below the baseline, but nothing is clipped.
        assert_eq!(ink(&r, 0..r.width, 0..1), 0, "clipped at the top");
        assert_eq!(ink(&r, 0..r.width, 35..36), 0, "clipped at the bottom");
        let quiet = rates("0 KB/s \u{2191}", "\u{2013} \u{2193}");
        assert_eq!(r.width, quiet.width, "the item keeps its width");
    }

    /// Writes the sample menu bar rows as PNGs for visual review:
    /// `KELVO_TRAY_PNG_DIR=<dir> cargo test -p kelvo --lib dump_tray_rows -- --ignored`.
    #[test]
    #[ignore = "writes files; run by hand for visual review"]
    fn dump_tray_rows() {
        use crate::tray::model::{ItemKey, Reading, Readings, TrayHistory, build};
        use kelvo_schema::settings::MenuBarMode;
        use kelvo_schema::{Module, Settings};

        let dir = std::env::var("KELVO_TRAY_PNG_DIR").unwrap_or_else(|_| ".".into());
        // Sample values for each graph.
        let spark = [
            6, 8, 5, 9, 12, 7, 6, 10, 14, 9, 7, 8, 11, 6, 5, 8, 7, 9, 6, 7,
        ];
        let hist = [20, 28, 35, 31, 44, 38, 30, 41, 36];
        let loads = [34, 22, 41, 18, 12, 9, 27, 15, 8, 6, 52, 38, 44, 29];
        let mut history = TrayHistory::default();
        for (i, v) in spark.iter().enumerate() {
            // Sample v sits at y = 14 - v * 0.75 over a 12 pt travel.
            let cpu = *v as f32 * 0.75 / 12.0 * 100.0;
            let gpu = i.checked_sub(11).and_then(|j| hist.get(j)).copied();
            history.record(&Readings {
                cpu: Reading::Value(cpu),
                gpu: gpu.map_or(Reading::Gap, |g| Reading::Value(g as f32)),
                ..Readings::default()
            });
        }
        let readings = Readings {
            cpu: Reading::Value(18.0),
            gpu: Reading::Value(36.0),
            mem: Reading::Value(42.0),
            temp_c: Reading::Value(61.0),
            watts: Reading::Value(14.84),
            net_bps: Reading::Value(39_600_000.0),
            net_rx_bps: Reading::Value(38_400_000.0),
            net_tx_bps: Reading::Value(1_200_000.0),
            cores: vec![
                loads[..10]
                    .iter()
                    .map(|&v| Reading::Value(v as f32))
                    .collect(),
                loads[10..]
                    .iter()
                    .map(|&v| Reading::Value(v as f32))
                    .collect(),
            ],
            ..Readings::default()
        };
        let rows: [(&str, [(Module, MenuBarMode); 7]); 4] = [
            (
                "combined",
                [
                    (Module::Cpu, MenuBarMode::InCombined),
                    (Module::Gpu, MenuBarMode::InCombined),
                    (Module::Memory, MenuBarMode::InCombined),
                    (Module::Power, MenuBarMode::TempInCombined),
                    (Module::Network, MenuBarMode::Hidden),
                    (Module::Disk, MenuBarMode::Hidden),
                    (Module::Battery, MenuBarMode::Hidden),
                ],
            ),
            (
                "graphs",
                [
                    (Module::Cpu, MenuBarMode::OwnGraph),
                    (Module::Gpu, MenuBarMode::Hidden),
                    (Module::Memory, MenuBarMode::OwnGraph),
                    (Module::Power, MenuBarMode::Hidden),
                    (Module::Network, MenuBarMode::OwnGraph),
                    (Module::Disk, MenuBarMode::Hidden),
                    (Module::Battery, MenuBarMode::Hidden),
                ],
            ),
            (
                "cores-histogram",
                [
                    (Module::Cpu, MenuBarMode::OwnCores),
                    (Module::Gpu, MenuBarMode::OwnGraph),
                    (Module::Memory, MenuBarMode::Hidden),
                    (Module::Power, MenuBarMode::Hidden),
                    (Module::Network, MenuBarMode::Hidden),
                    (Module::Disk, MenuBarMode::Hidden),
                    (Module::Battery, MenuBarMode::Hidden),
                ],
            ),
            (
                "values-own",
                [
                    (Module::Cpu, MenuBarMode::OwnValue),
                    (Module::Gpu, MenuBarMode::OwnValue),
                    (Module::Memory, MenuBarMode::OwnValue),
                    (Module::Power, MenuBarMode::OwnValue),
                    (Module::Network, MenuBarMode::Hidden),
                    (Module::Disk, MenuBarMode::Hidden),
                    (Module::Battery, MenuBarMode::Hidden),
                ],
            ),
        ];
        for (name, modes) in rows {
            let mut s = Settings::default();
            for (m, mode) in modes {
                let e = s.modules.get_mut(&m).unwrap();
                e.enabled = true;
                e.menu_bar = mode;
            }
            let items = build(&readings, &history, &s, false, 2);
            let images: Vec<Rendered> = items
                .iter()
                .inspect(|i| assert!(i.key == ItemKey::Combined || name != "combined"))
                .map(|i| render(&i.content.frame).unwrap())
                .collect();
            // Items 14 pt apart, 8 px margin, on dark (ink #f5f5f7 on #18181b) and
            // light (#1d1d1f on #f4f4f5) bars.
            for (theme, ink_rgb, bg) in [
                ("dark", [0xf5u8, 0xf5, 0xf7], [0x18u8, 0x18, 0x1b]),
                ("light", [0x1d, 0x1d, 0x1f], [0xf4, 0xf4, 0xf5]),
            ] {
                let gap = 28;
                let w = images.iter().map(|r| r.width).sum::<u32>()
                    + gap * (images.len() as u32 - 1)
                    + 16;
                let h = 36 + 16;
                let mut out = Vec::with_capacity((w * h * 4) as usize);
                for _ in 0..w * h {
                    out.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
                }
                let mut x0 = 8;
                for r in &images {
                    for y in 0..r.height {
                        for x in 0..r.width {
                            let a = f32::from(alpha(r, x, y)) / 255.0;
                            let i = (((y + 8) * w + x0 + x) * 4) as usize;
                            for c in 0..3 {
                                out[i + c] = (f32::from(ink_rgb[c]) * a
                                    + f32::from(bg[c]) * (1.0 - a))
                                    .round() as u8;
                            }
                        }
                    }
                    x0 += r.width + gap;
                }
                let path = format!("{dir}/tray-{name}-{theme}.png");
                let file = std::fs::File::create(&path).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&out).unwrap();
                println!("{path}");
            }
        }
    }
}
