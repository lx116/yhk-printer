//! TUI settings mutation helpers.
use super::*;

pub(super) fn adjust_setting(app: &mut App, increase: bool) {
    match app.selected_setting {
        0 => {
            app.settings.density = match (Quality::from_density(app.settings.density), increase) {
                (Quality::Low, false) | (Quality::High, true) => app.settings.density,
                (Quality::Low, true) | (Quality::Custom(_), true) => Quality::Medium.density(),
                (Quality::Medium, false) | (Quality::Custom(_), false) => Quality::Low.density(),
                (Quality::Medium, true) | (Quality::High, false) => Quality::High.density(),
            }
        }
        1 => {
            app.settings.content = match (app.settings.content, increase) {
                (imaging::ContentKind::Auto, false) | (imaging::ContentKind::Qr, true) => {
                    app.settings.content
                }
                (imaging::ContentKind::Auto, true) => imaging::ContentKind::Photo,
                (imaging::ContentKind::Photo, true) => imaging::ContentKind::Text,
                (imaging::ContentKind::Text, true) => imaging::ContentKind::Qr,
                (imaging::ContentKind::Photo, false) => imaging::ContentKind::Auto,
                (imaging::ContentKind::Text, false) => imaging::ContentKind::Photo,
                (imaging::ContentKind::Qr, false) => imaging::ContentKind::Text,
            }
        }
        2 => {
            app.settings.definition = match (app.settings.definition, increase) {
                (imaging::Definition::Standard, true) | (imaging::Definition::Hd, false) => {
                    imaging::Definition::Hd
                }
                _ => imaging::Definition::Standard,
            }
        }
        3 => cycle_print_mode(app),
        4 => cycle_batch_mode(app),
        5 => {
            app.settings.gap_mm =
                (app.settings.gap_mm + if increase { 1.0 } else { -1.0 }).clamp(0.0, 30.0)
        }
        6 => app.settings.cut_marks = !app.settings.cut_marks,
        7 => {
            app.settings.chunk = if increase {
                app.settings.chunk.saturating_add(1)
            } else {
                app.settings.chunk.saturating_sub(1).max(1)
            }
        }
        8 => {
            app.settings.delay_ms = if increase {
                app.settings.delay_ms.saturating_add(1)
            } else {
                app.settings.delay_ms.saturating_sub(1)
            }
        }
        9 => app.settings.acked = !app.settings.acked,
        10 => {
            app.settings.doc_font = match (app.settings.doc_font, increase) {
                (Font::Sans, true) => Font::Hand,
                (Font::Hand, true) => Font::Mono,
                (Font::Mono, false) => Font::Hand,
                (Font::Hand, false) => Font::Sans,
                value => value.0,
            }
        }
        11 => {
            app.settings.doc_size_pt =
                (app.settings.doc_size_pt + if increase { 1.0 } else { -1.0 }).clamp(8.0, 16.0)
        }
        12 => {
            app.settings.doc_align = match (app.settings.doc_align, increase) {
                (Align::Left, true) => Align::Center,
                (Align::Center, true) => Align::Right,
                (Align::Right, true) => Align::Justify,
                (Align::Justify, false) => Align::Right,
                (Align::Right, false) => Align::Center,
                (Align::Center, false) => Align::Left,
                value => value.0,
            }
        }
        13 => {
            app.settings.doc_margin = if increase {
                app.settings.doc_margin.saturating_add(2).min(80)
            } else {
                app.settings.doc_margin.saturating_sub(2)
            }
        }
        14 => {
            app.settings.flip = match app.settings.flip {
                Flip::Book => Flip::Calendar,
                Flip::Calendar => Flip::Book,
            }
        }
        15 => {
            app.settings.card_margin_mm =
                (app.settings.card_margin_mm + if increase { 1.0 } else { -1.0 }).clamp(0.0, 24.0)
        }
        16 => {
            app.settings.back_offset_mm =
                (app.settings.back_offset_mm + if increase { 0.5 } else { -0.5 }).clamp(-5.0, 5.0)
        }
        17 => app.settings.frame = !app.settings.frame,
        18 => {
            app.settings.min_pt =
                (app.settings.min_pt + if increase { 0.5 } else { -0.5 }).clamp(7.0, 16.0)
        }
        _ => unreachable!(),
    }
}

pub(super) fn cycle_print_mode(app: &mut App) {
    app.settings.mode = match app.settings.mode {
        PrintMode::Rows => PrintMode::Raster,
        PrintMode::Raster => PrintMode::Gray,
        PrintMode::Gray => PrintMode::Rows,
    };
}

pub(super) fn cycle_batch_mode(app: &mut App) {
    app.settings.batch_mode = app.settings.batch_mode.clone().next();
}

pub(super) fn upsert_job(jobs: &mut Vec<JobInfo>, job: JobInfo) {
    if let Some(existing) = jobs.iter_mut().find(|existing| existing.id == job.id) {
        *existing = job;
    } else {
        jobs.push(job);
    }
}
