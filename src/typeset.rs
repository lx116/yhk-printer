//! Pure document layout and rasterization using embedded fonts.

use crate::{
    document::{Block, Document, Span},
    imaging::{GrayImage, RasterImage},
};

pub const DOCUMENT_SIZE_PT_RANGE: std::ops::RangeInclusive<f32> = 4.0..=48.0;

pub fn validate_document_size(size: f32) -> Result<(), &'static str> {
    DOCUMENT_SIZE_PT_RANGE
        .contains(&size)
        .then_some(())
        .ok_or("size must be between 4.0 and 48.0 points")
}
use fontdue::{Font as Face, FontSettings};
use serde::{Deserialize, Serialize};

const DPI: f32 = 203.0;
const MAX_STRIP_HEIGHT: usize = 1600;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    #[default]
    Sans,
    Hand,
    Mono,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypesetOptions {
    pub font: Font,
    pub size_pt: f32,
    pub line_height: f32,
    pub align: Align,
    pub margin_dots: u16,
    pub paragraph_gap_rows: u16,
    pub width_dots: u16,
    pub heading_scale: [f32; 3],
    pub bullet: String,
    pub indent_dots_per_level: u16,
}

impl Default for TypesetOptions {
    fn default() -> Self {
        Self {
            font: Font::Sans,
            size_pt: 10.0,
            line_height: 1.25,
            align: Align::Left,
            margin_dots: 12,
            paragraph_gap_rows: 10,
            width_dots: crate::imaging::PRINTER_WIDTH_DOTS,
            heading_scale: [1.6, 1.3, 1.1],
            bullet: "•".into(),
            indent_dots_per_level: 16,
        }
    }
}

pub const fn points_to_dots(points: f32) -> f32 {
    points * DPI / 72.0
}

struct Faces {
    regular: Face,
    bold: Face,
    italic: Face,
    bold_italic: Face,
    mono: Face,
    hand: Face,
}
impl Faces {
    fn load() -> Self {
        fn face(bytes: &'static [u8]) -> Face {
            Face::from_bytes(bytes, FontSettings::default()).expect("embedded font is valid")
        }
        Self {
            regular: face(include_bytes!(
                "../assets/fonts/atkinsonhyperlegible-AtkinsonHyperlegible-Regular.ttf"
            )),
            bold: face(include_bytes!(
                "../assets/fonts/atkinsonhyperlegible-AtkinsonHyperlegible-Bold.ttf"
            )),
            italic: face(include_bytes!(
                "../assets/fonts/atkinsonhyperlegible-AtkinsonHyperlegible-Italic.ttf"
            )),
            bold_italic: face(include_bytes!(
                "../assets/fonts/atkinsonhyperlegible-AtkinsonHyperlegible-BoldItalic.ttf"
            )),
            mono: face(include_bytes!(
                "../assets/fonts/ibmplexmono-IBMPlexMono-Regular.ttf"
            )),
            hand: face(include_bytes!(
                "../assets/fonts/patrickhand-PatrickHand-Regular.ttf"
            )),
        }
    }
    fn select(&self, base: Font, span: &Span) -> (&Face, bool) {
        if span.mono {
            (&self.mono, false)
        } else if base == Font::Hand {
            (&self.hand, span.bold)
        } else if span.bold && span.italic {
            (&self.bold_italic, false)
        } else if span.bold {
            (&self.bold, false)
        } else if span.italic {
            (&self.italic, false)
        } else {
            (&self.regular, false)
        }
    }
}

#[derive(Clone)]
struct Run {
    text: String,
    span: Span,
}
#[derive(Clone)]
struct Line {
    runs: Vec<Run>,
    width: f32,
    indent: u16,
    size: f32,
    align: Align,
    justify: bool,
    quote: bool,
}

pub fn typeset(document: &Document, options: &TypesetOptions) -> Vec<GrayImage> {
    let faces = Faces::load();
    let mut lines = Vec::new();
    let mut gaps = Vec::new();
    for (index, block) in document.blocks.iter().enumerate() {
        let start = lines.len();
        match block {
            Block::Heading(level, spans) => {
                if start > 0 {
                    gaps.push((
                        start,
                        (f32::from(options.paragraph_gap_rows) * 1.5).ceil() as u16,
                    ));
                }
                lines.extend(wrap(
                    spans,
                    options,
                    &faces,
                    options.size_pt * options.heading_scale[usize::from((*level).clamp(1, 3) - 1)],
                    0,
                    options.align,
                    false,
                ));
            }
            Block::Paragraph(spans) => lines.extend(wrap(
                spans,
                options,
                &faces,
                options.size_pt,
                0,
                options.align,
                false,
            )),
            Block::ListItem(ordered, depth, spans) => {
                if start > 0 && !matches!(document.blocks.get(index - 1), Some(Block::ListItem(..)))
                {
                    gaps.push((start, options.paragraph_gap_rows));
                }
                let prefix =
                    ordered.map_or_else(|| format!("{} ", options.bullet), |n| format!("{n}. "));
                let mut spans = spans.clone();
                spans.insert(
                    0,
                    Span {
                        text: prefix,
                        ..Span::default()
                    },
                );
                lines.extend(wrap(
                    &spans,
                    options,
                    &faces,
                    options.size_pt,
                    options.indent_dots_per_level * u16::from(*depth),
                    options.align,
                    false,
                ));
            }
            Block::Quote(spans) => lines.extend(wrap(
                spans,
                options,
                &faces,
                options.size_pt,
                options.indent_dots_per_level,
                options.align,
                true,
            )),
            Block::Rule => lines.push(Line {
                runs: Vec::new(),
                width: 0.0,
                indent: 0,
                size: 2.0,
                align: Align::Left,
                justify: false,
                quote: false,
            }),
            Block::Code(text) => {
                for text in text.lines() {
                    lines.push(Line {
                        runs: vec![Run {
                            text: text.into(),
                            span: Span {
                                mono: true,
                                ..Span::default()
                            },
                        }],
                        width: measure(
                            text,
                            &Span {
                                mono: true,
                                ..Span::default()
                            },
                            options.size_pt,
                            options.font,
                            &faces,
                        ),
                        indent: 0,
                        size: options.size_pt,
                        align: Align::Left,
                        justify: false,
                        quote: false,
                    });
                }
            }
            Block::PageBreakGap(rows) => gaps.push((lines.len(), *rows)),
        }
        if lines.len() > start {
            let next_is_list = matches!(document.blocks.get(index + 1), Some(Block::ListItem(..)));
            let gap = match block {
                Block::Heading(..) => 3,
                Block::ListItem(..) if next_is_list => 3,
                _ => options.paragraph_gap_rows,
            };
            gaps.push((lines.len(), gap));
        }
    }
    render(lines, gaps, options, &faces)
}

#[cfg(test)]
fn laid_out_line_texts(document: &Document, options: &TypesetOptions) -> Vec<String> {
    let faces = Faces::load();
    document
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Heading(level, spans) => wrap(
                spans,
                options,
                &faces,
                options.size_pt * options.heading_scale[usize::from((*level).clamp(1, 3) - 1)],
                0,
                options.align,
                false,
            ),
            Block::Paragraph(spans) | Block::Quote(spans) => wrap(
                spans,
                options,
                &faces,
                options.size_pt,
                0,
                options.align,
                false,
            ),
            Block::ListItem(_, _, spans) => wrap(
                spans,
                options,
                &faces,
                options.size_pt,
                0,
                options.align,
                false,
            ),
            Block::Rule | Block::Code(_) | Block::PageBreakGap(_) => Vec::new(),
        })
        .map(|line| line.runs.into_iter().map(|run| run.text).collect())
        .collect()
}

fn wrap(
    spans: &[Span],
    options: &TypesetOptions,
    faces: &Faces,
    size: f32,
    indent: u16,
    align: Align,
    quote: bool,
) -> Vec<Line> {
    let width = f32::from(
        options
            .width_dots
            .saturating_sub(options.margin_dots * 2 + indent),
    );
    let mut words = Vec::new();
    let mut has_space_before = false;
    for span in spans {
        for part in span.text.split_inclusive(char::is_whitespace) {
            let word = part.trim_end_matches(char::is_whitespace);
            if !word.is_empty() {
                words.push((
                    Run {
                        text: word.into(),
                        span: span.clone(),
                    },
                    has_space_before,
                ));
                has_space_before = false;
            }
            if part.chars().last().is_some_and(char::is_whitespace) {
                has_space_before = true;
            }
        }
    }
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut used = 0.0;
    for (word, has_space_before) in words {
        let space = if current.is_empty() || !has_space_before {
            0.0
        } else {
            measure(" ", &word.span, size, options.font, faces)
        };
        let word_width = measure(&word.text, &word.span, size, options.font, faces);
        if !current.is_empty() && used + space + word_width > width {
            lines.push(Line {
                runs: std::mem::take(&mut current),
                width: used,
                indent,
                size,
                align,
                justify: align == Align::Justify,
                quote,
            });
            used = 0.0;
        }
        if word_width > width {
            for character in word.text.chars() {
                let run = Run {
                    text: character.to_string(),
                    span: word.span.clone(),
                };
                let char_width = measure(&run.text, &run.span, size, options.font, faces);
                if !current.is_empty() && used + char_width > width {
                    lines.push(Line {
                        runs: std::mem::take(&mut current),
                        width: used,
                        indent,
                        size,
                        align,
                        justify: align == Align::Justify,
                        quote,
                    });
                    used = 0.0;
                }
                current.push(run);
                used += char_width;
            }
        } else {
            if !current.is_empty() && has_space_before {
                current.push(Run {
                    text: " ".into(),
                    span: word.span.clone(),
                });
                used += space;
            }
            used += word_width;
            current.push(word);
        }
    }
    if !current.is_empty() {
        lines.push(Line {
            runs: current,
            width: used,
            indent,
            size,
            align,
            justify: false,
            quote,
        });
    }
    lines
}

fn measure(text: &str, span: &Span, size: f32, base: Font, faces: &Faces) -> f32 {
    // `size` is in points; fontdue works in pixels (dots).
    let size = points_to_dots(size);
    let (face, _) = faces.select(base, span);
    let mut width = 0.0;
    let mut previous = None;
    for character in text.chars() {
        if let Some(left) = previous {
            width += face.horizontal_kern(left, character, size).unwrap_or(0.0);
        }
        width += face.metrics(character, size).advance_width;
        previous = Some(character);
    }
    width
}

fn render(
    lines: Vec<Line>,
    gaps: Vec<(usize, u16)>,
    options: &TypesetOptions,
    faces: &Faces,
) -> Vec<GrayImage> {
    let mut rows = Vec::new();
    let mut line_rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        for (_, gap) in gaps.iter().filter(|(line_index, _)| *line_index == index) {
            rows.extend(std::iter::repeat_n(
                255,
                usize::from(*gap) * usize::from(options.width_dots),
            ));
        }
        let height = (points_to_dots(line.size) * options.line_height)
            .ceil()
            .max(1.0) as usize;
        // Row index of the line start, not a byte offset into the flat bitmap.
        line_rows.push((rows.len() / usize::from(options.width_dots), height));
        rows.extend(std::iter::repeat_n(
            255,
            height * usize::from(options.width_dots),
        ));
        if line.runs.is_empty() {
            let y = line_rows[index].0 + height / 2;
            let start = usize::from(options.width_dots) / 10;
            let end = usize::from(options.width_dots) - start;
            for yy in y..(y + 2).min(line_rows[index].0 + height) {
                rows[yy * usize::from(options.width_dots) + start
                    ..yy * usize::from(options.width_dots) + end]
                    .fill(0);
            }
            continue;
        }
        let available = f32::from(
            options
                .width_dots
                .saturating_sub(options.margin_dots * 2 + line.indent),
        );
        let extra = if line.justify {
            ((available - line.width).max(0.0) + 1.0)
                / line
                    .runs
                    .iter()
                    .filter(|run| run.text == " ")
                    .count()
                    .max(1) as f32
        } else {
            0.0
        };
        let offset = match line.align {
            Align::Center => (available - line.width).max(0.0) / 2.0,
            Align::Right => (available - line.width).max(0.0),
            _ => 0.0,
        };
        let mut x = f32::from(options.margin_dots + line.indent) + offset;
        let baseline = line_rows[index].0 as i32 + (points_to_dots(line.size) * 0.9).ceil() as i32;
        let pixel_size = points_to_dots(line.size);
        if line.quote {
            let bar_start = usize::from(options.margin_dots);
            let bar_end = bar_start + 3;
            for row in line_rows[index].0..line_rows[index].0 + height {
                rows[row * usize::from(options.width_dots) + bar_start
                    ..row * usize::from(options.width_dots) + bar_end]
                    .fill(0);
            }
        }
        for run in &line.runs {
            let (face, fake_bold) = faces.select(options.font, &run.span);
            let mut previous = None;
            for character in run.text.chars() {
                if let Some(left) = previous {
                    x += face
                        .horizontal_kern(left, character, pixel_size)
                        .unwrap_or(0.0);
                }
                let (metrics, bitmap) = face.rasterize(character, pixel_size);
                let gx = x.round() as i32 + metrics.xmin;
                let gy = baseline - metrics.ymin - metrics.height as i32;
                draw(
                    &mut rows,
                    options.width_dots,
                    gx,
                    gy,
                    metrics.width,
                    metrics.height,
                    &bitmap,
                );
                if fake_bold {
                    draw(
                        &mut rows,
                        options.width_dots,
                        gx + 1,
                        gy,
                        metrics.width,
                        metrics.height,
                        &bitmap,
                    );
                }
                x += metrics.advance_width + if run.text == " " { extra } else { 0.0 };
                previous = Some(character);
            }
        }
    }
    for (_, gap) in gaps
        .iter()
        .filter(|(line_index, _)| *line_index == lines.len())
    {
        rows.extend(std::iter::repeat_n(
            255,
            usize::from(*gap) * usize::from(options.width_dots),
        ));
    }
    split(rows, &line_rows, options.width_dots)
}

fn draw(
    data: &mut [u8],
    width: u16,
    x: i32,
    y: i32,
    glyph_width: usize,
    glyph_height: usize,
    glyph: &[u8],
) {
    for yy in 0..glyph_height {
        for xx in 0..glyph_width {
            let px = x + xx as i32;
            let py = y + yy as i32;
            if px >= 0 && py >= 0 && px < i32::from(width) {
                let index = py as usize * usize::from(width) + px as usize;
                if index < data.len() {
                    data[index] = data[index].min(255 - glyph[yy * glyph_width + xx]);
                }
            }
        }
    }
}
fn split(data: Vec<u8>, line_rows: &[(usize, usize)], width: u16) -> Vec<GrayImage> {
    let total = data.len() / usize::from(width);
    let mut result = Vec::new();
    let mut start = 0;
    while start < total {
        let mut end = (start + MAX_STRIP_HEIGHT).min(total);
        if end < total {
            if let Some((line, _)) = line_rows
                .iter()
                .find(|(line, height)| *line >= start && *line < end && *line + *height > end)
            {
                end = *line;
            }
        }
        if end == start {
            end = (start + MAX_STRIP_HEIGHT).min(total);
        }
        result.push(GrayImage {
            width,
            height: u16::try_from(end - start).unwrap_or(MAX_STRIP_HEIGHT as u16),
            data: data[start * usize::from(width)..end * usize::from(width)].to_vec(),
        });
        start = end;
    }
    result
}

pub fn typeset_to_bitmap(document: &Document, options: &TypesetOptions) -> Vec<GrayImage> {
    typeset(document, options)
        .into_iter()
        .map(|mut image| {
            for pixel in &mut image.data {
                *pixel = if *pixel < 128 { 0 } else { 255 };
            }
            image
        })
        .collect()
}
pub fn prepared(image: GrayImage) -> crate::imaging::PreparedImage {
    let mut raster = vec![0; usize::from(image.width / 8) * usize::from(image.height)];
    for (index, pixel) in image.data.iter().enumerate() {
        if *pixel < 128 {
            raster[index / usize::from(image.width) * usize::from(image.width / 8)
                + (index % usize::from(image.width)) / 8] |= 0x80 >> (index % 8);
        }
    }
    crate::imaging::PreparedImage {
        raster: RasterImage {
            width: image.width,
            height: image.height,
            data: raster,
        },
        gray: image,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{parse_markdown, parse_text};

    fn last_ink_row(image: &GrayImage) -> Option<usize> {
        let width = usize::from(image.width);
        image
            .data
            .chunks(width)
            .rposition(|row| row.iter().any(|&pixel| pixel < 128))
    }

    fn ink_rows(image: &GrayImage) -> Vec<usize> {
        let width = usize::from(image.width);
        image
            .data
            .chunks(width)
            .enumerate()
            .filter(|(_, row)| row.iter().any(|&pixel| pixel < 128))
            .map(|(row, _)| row)
            .collect()
    }

    fn blank_rows_between(image: &GrayImage) -> Vec<usize> {
        let rows = ink_rows(image);
        rows.windows(2)
            .filter_map(|rows| (rows[1] > rows[0] + 1).then_some(rows[1] - rows[0] - 1))
            .collect()
    }

    #[test]
    fn glyphs_are_rendered_at_the_requested_point_size() {
        // A 10 pt capital H at 203 dpi has an em of about 28 dots and a cap height
        // of about 20 dots; rendering points as pixels would give roughly 7.
        let document = parse_text("HHHH");
        let strips = typeset(&document, &TypesetOptions::default());
        let strip = &strips[0];
        let width = usize::from(strip.width);
        let ink_rows: Vec<usize> = strip
            .data
            .chunks(width)
            .enumerate()
            .filter(|(_, row)| row.iter().any(|&pixel| pixel < 128))
            .map(|(index, _)| index)
            .collect();
        let cap_height = ink_rows.last().unwrap() - ink_rows.first().unwrap() + 1;
        assert!(
            (17..=24).contains(&cap_height),
            "cap height {cap_height} dots is not about 20 for 10 pt"
        );
    }

    #[test]
    fn every_paragraph_is_drawn_not_only_the_first() {
        let document = parse_text("one\n\ntwo\n\nthree\n\nfour");
        let strips = typeset(&document, &TypesetOptions::default());
        let strip = &strips[0];
        let last = last_ink_row(strip).expect("some ink");
        assert!(
            last * 100 / usize::from(strip.height) > 60,
            "last ink row {last} of {} means later lines were dropped",
            strip.height
        );
    }

    #[test]
    fn horizontal_rule_is_drawn_and_does_not_panic() {
        let document = parse_markdown("above\n\n---\n\nbelow");
        let strips = typeset(&document, &TypesetOptions::default());
        let strip = &strips[0];
        let width = usize::from(strip.width);
        let has_rule = strip
            .data
            .chunks(width)
            .any(|row| row.iter().filter(|&&pixel| pixel < 128).count() > width * 7 / 10);
        assert!(has_rule, "no row with a long black run");
        assert!(last_ink_row(strip).unwrap() * 100 / usize::from(strip.height) > 60);
    }

    #[test]
    fn points_are_converted_at_203_dpi() {
        assert_eq!(points_to_dots(72.0), 203.0);
    }

    #[test]
    fn document_size_validator_accepts_only_finite_sizes_in_the_supported_range() {
        for size in [4.0, 10.0, 48.0] {
            assert_eq!(validate_document_size(size), Ok(()));
        }
        for size in [0.0, 3.99, 48.01, f32::NAN, f32::INFINITY] {
            assert_eq!(
                validate_document_size(size),
                Err("size must be between 4.0 and 48.0 points")
            );
        }
    }
    #[test]
    fn long_words_wrap_and_output_is_deterministic() {
        let doc = parse_text("supercalifragilisticexpialidocious");
        let options = TypesetOptions {
            width_dots: 80,
            ..TypesetOptions::default()
        };
        let first = typeset(&doc, &options);
        let second = typeset(&doc, &options);
        assert!(first[0].height > 10);
        assert_eq!(first[0].data, second[0].data);
    }
    #[test]
    fn missing_glyphs_do_not_panic_and_strips_are_bounded() {
        let doc = parse_text(&"emoji 😀 ".repeat(1000));
        assert!(typeset(&doc, &TypesetOptions::default())
            .iter()
            .all(|image| image.height <= 1600));
    }

    #[test]
    fn larger_text_uses_more_rows() {
        let document = parse_text("one two three four five six seven eight");
        let small = typeset(
            &document,
            &TypesetOptions {
                size_pt: 8.0,
                ..TypesetOptions::default()
            },
        );
        let large = typeset(
            &document,
            &TypesetOptions {
                size_pt: 16.0,
                ..TypesetOptions::default()
            },
        );
        assert!(
            large.iter().map(|page| page.height).sum::<u16>()
                > small.iter().map(|page| page.height).sum::<u16>()
        );
    }

    #[test]
    fn paragraph_gaps_and_list_item_gaps_follow_the_options() {
        let options = TypesetOptions {
            paragraph_gap_rows: 11,
            ..TypesetOptions::default()
        };
        let paragraphs = typeset(&parse_text("one\n\ntwo"), &options);
        let paragraph_gap = blank_rows_between(&paragraphs[0])
            .into_iter()
            .max()
            .expect("paragraphs have a blank run");
        assert!(
            paragraph_gap
                == blank_rows_between(
                    &typeset(
                        &parse_text("one\n\ntwo"),
                        &TypesetOptions {
                            paragraph_gap_rows: 0,
                            ..options.clone()
                        },
                    )[0]
                )
                .into_iter()
                .max()
                .unwrap()
                    + usize::from(options.paragraph_gap_rows),
            "paragraph gap must be line leading plus {} rows",
            options.paragraph_gap_rows
        );

        let list = typeset(&parse_markdown("- one\n- two"), &options);
        let list_gap = blank_rows_between(&list[0])
            .into_iter()
            .min()
            .expect("list items have a blank run");
        assert!(
            list_gap < paragraph_gap,
            "list gap {list_gap} should be smaller than paragraph gap {paragraph_gap}"
        );
    }

    #[test]
    fn adjacent_style_runs_do_not_insert_spaces() {
        let options = TypesetOptions::default();
        for source in ["**mano**, pero", "*mano*, pero", "`mano`, pero"] {
            assert_eq!(
                laid_out_line_texts(&parse_markdown(source), &options),
                ["mano, pero"]
            );
        }
    }

    #[test]
    fn quotes_have_a_bar_and_indented_text() {
        let options = TypesetOptions::default();
        let normal = typeset(&parse_text("quote"), &options);
        let quote = typeset(&parse_markdown("> quote"), &options);
        let width = usize::from(quote[0].width);
        let bar_start = usize::from(options.margin_dots);
        let bar_end = bar_start + 3;
        for row in quote[0]
            .data
            .chunks(width)
            .take(usize::from(quote[0].height) - usize::from(options.paragraph_gap_rows))
        {
            assert!(row[bar_start..bar_end].iter().any(|&pixel| pixel < 128));
        }
        let first_text_ink = |image: &GrayImage, start: usize| {
            image
                .data
                .chunks(usize::from(image.width))
                .flat_map(|row| row.iter().enumerate())
                .filter(|(_, pixel)| **pixel < 128)
                .map(|(column, _)| column)
                .filter(|column| *column >= start)
                .min()
                .unwrap()
        };
        assert!(
            first_text_ink(&quote[0], bar_end)
                >= first_text_ink(&normal[0], bar_start)
                    + usize::from(options.indent_dots_per_level)
                    - 3
        );
    }

    #[test]
    fn justify_reaches_both_edges_on_full_lines() {
        let options = TypesetOptions {
            align: Align::Justify,
            width_dots: 240,
            ..TypesetOptions::default()
        };
        let image = &typeset(&parse_text(&"word ".repeat(80)), &options)[0];
        let width = usize::from(image.width);
        let rows = ink_rows(image);
        let line_starts: Vec<_> = rows
            .iter()
            .copied()
            .filter(|row| *row == 0 || !rows.contains(&(row - 1)))
            .collect();
        for start in line_starts
            .into_iter()
            .take_while(|start| *start + 40 < usize::from(image.height))
        {
            let line =
                &image.data[start * width..(start + 35).min(usize::from(image.height)) * width];
            let rightmost = line.iter().rposition(|&pixel| pixel < 128).unwrap() % width;
            assert!(
                rightmost + 2 >= width - usize::from(options.margin_dots),
                "rightmost ink was {rightmost}, expected at least {}",
                width - usize::from(options.margin_dots) - 2
            );
        }
    }

    #[test]
    fn left_aligned_text_stays_inside_margins() {
        for size_pt in [8.0, 10.0, 14.0] {
            for width_dots in [240, 384] {
                let options = TypesetOptions {
                    size_pt,
                    width_dots,
                    ..TypesetOptions::default()
                };
                let image = &typeset(&parse_text(&"left aligned text ".repeat(30)), &options)[0];
                let width = usize::from(image.width);
                for row in image.data.chunks(width) {
                    assert!(row[..usize::from(options.margin_dots)]
                        .iter()
                        .all(|&pixel| pixel >= 128));
                    assert!(row[width - usize::from(options.margin_dots)..]
                        .iter()
                        .all(|&pixel| pixel >= 128));
                }
            }
        }
    }

    #[test]
    fn every_font_renders_the_same_sample() {
        for font in [Font::Sans, Font::Hand, Font::Mono] {
            let image = &typeset(
                &parse_text("Font sample"),
                &TypesetOptions {
                    font,
                    ..TypesetOptions::default()
                },
            )[0];
            assert!(image.data.iter().any(|&pixel| pixel < 128));
        }
    }
}
