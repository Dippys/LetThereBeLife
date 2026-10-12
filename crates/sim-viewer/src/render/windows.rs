//! Floating windows: a title bar to drag them by, a close button, and their
//! lines or charts. What they show is prepared by the app each frame.

use super::{
    LineStyle, UiAction, WindowBody, WindowKind, WindowView,
    colors::{self, rgba},
    text::{text_width, wrap},
    ui::{Hit, Painter},
};

/// Characters a line is indented by, per level.
const INDENT_CHARS: usize = 2;

/// Draws the open windows, back to front.
pub(super) fn draw_windows(painter: &mut Painter, windows: &[WindowView], width: f32, height: f32) {
    for (index, window) in windows.iter().enumerate() {
        draw_window(painter, index, window, width, height);
    }
}

/// A window's frame in screen pixels, kept on screen.
pub(super) fn frame(
    painter: &Painter,
    kind: WindowKind,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> (f32, f32, f32, f32) {
    let (chars, lines) = kind.size();
    let w = (painter.chars(chars) + 2.0 * painter.pad()).min(width);
    let h = ((lines as f32 + 1.5) * painter.line() + 2.0 * painter.pad()).min(height);
    (
        x.clamp(0.0, (width - w).max(0.0)),
        y.clamp(0.0, (height - h).max(0.0)),
        w,
        h,
    )
}

fn draw_window(painter: &mut Painter, index: usize, window: &WindowView, width: f32, height: f32) {
    let (x, y, w, h) = frame(painter, window.kind, window.x, window.y, width, height);
    let pad = painter.pad();
    let line = painter.line();
    painter.panel(x, y, w, h);
    painter.hits.push(Hit {
        x,
        y,
        width: w,
        height: h,
        action: Some(UiAction::WindowBody(index)),
    });
    // Title bar: drag it to move the window.
    let bar = line + pad;
    painter.rect(x, y, w, bar, colors::UI_WINDOW_BAR);
    painter.hits.push(Hit {
        x,
        y,
        width: w,
        height: bar,
        action: Some(UiAction::WindowTitle(index)),
    });
    painter.text(&window.title, x + pad, y + pad / 2.0, colors::UI_TITLE);
    let close = "×";
    let close_w = painter.button_width(close);
    painter.button(
        close,
        x + w - close_w - painter.px,
        y + painter.px,
        UiAction::CloseWindow(index),
        false,
    );
    let body_y = y + bar + pad / 2.0;
    let body_h = h - bar - pad;
    match &window.body {
        WindowBody::Lines {
            lines,
            first,
            total,
        } => {
            let (chars, _) = window.kind.size();
            let mut row_y = body_y;
            for entry in lines {
                if row_y + line > body_y + body_h {
                    break;
                }
                let indent = usize::from(entry.indent) * INDENT_CHARS;
                let text_x = x + pad + painter.chars(indent);
                let text = wrap(&entry.text, chars.saturating_sub(indent))
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                if let Some(action) = entry.action {
                    if painter.hovered(x, row_y, w, line) {
                        painter.rect(
                            x + painter.px,
                            row_y,
                            w - 2.0 * painter.px,
                            line,
                            colors::UI_BUTTON_HOVER,
                        );
                    }
                    painter.hits.push(Hit {
                        x,
                        y: row_y,
                        width: w,
                        height: line,
                        action: Some(action),
                    });
                }
                painter.text(&text, text_x, row_y, style_color(entry.style));
                row_y += line;
            }
            // Where in the list the view is, when it doesn't all fit.
            let shown = lines.len().min((body_h / line) as usize);
            if *total > shown && *total > 0 {
                let track = body_h;
                let thumb = (track * shown as f32 / *total as f32).max(2.0 * painter.px);
                let top =
                    body_y + (track - thumb) * (*first as f32 / (*total - shown).max(1) as f32);
                painter.rect(
                    x + w - 2.0 * painter.px,
                    top,
                    painter.px,
                    thumb,
                    colors::UI_DIM,
                );
            }
        }
        WindowBody::Charts { series, years } => {
            draw_charts(
                painter,
                series,
                *years,
                x + pad,
                body_y,
                w - 2.0 * pad,
                body_h,
            );
        }
    }
}

/// Small line charts stacked down the window, each with its name and latest value.
fn draw_charts(
    painter: &mut Painter,
    series: &[super::Series],
    years: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    if series.is_empty() {
        painter.text("Nothing to chart yet", x, y, colors::UI_DIM);
        return;
    }
    let line = painter.line();
    let note = format!("Over {years} year{}", if years == 1 { "" } else { "s" });
    let each = (h - line) / series.len() as f32;
    for (index, chart) in series.iter().enumerate() {
        let top = y + index as f32 * each;
        let latest = chart.values.last().copied().unwrap_or(0);
        let heading = match chart.total {
            Some(total) => format!("{}: {total} in all", chart.label),
            None => format!("{} {latest}", chart.label),
        };
        painter.text(&heading, x, top, style_color(chart.style));
        let plot_y = top + line;
        let plot_h = (each - line - painter.px).max(painter.px);
        painter.rect(x, plot_y, w, plot_h, rgba(255, 255, 255, 10));
        let top_value = chart.values.iter().copied().max().unwrap_or(0).max(1);
        let count = chart.values.len().max(1);
        let column = w / count as f32;
        let color = style_color(chart.style);
        for (step, &value) in chart.values.iter().enumerate() {
            let bar = plot_h * value as f32 / top_value as f32;
            painter.rect(
                x + step as f32 * column,
                plot_y + plot_h - bar,
                column.max(painter.px / 2.0),
                bar.max(if value > 0 { painter.px / 2.0 } else { 0.0 }),
                color,
            );
        }
        painter.text(
            &top_value.to_string(),
            x + w - text_width(&top_value.to_string(), painter.px),
            plot_y,
            colors::UI_DIM,
        );
    }
    painter.text(&note, x, y + h - line, colors::UI_DIM);
}

const fn style_color(style: LineStyle) -> u32 {
    match style {
        LineStyle::Normal => colors::UI_TEXT,
        LineStyle::Dim => colors::UI_DIM,
        LineStyle::Heading => colors::UI_ACCENT,
        LineStyle::Good => colors::UI_GOOD,
        LineStyle::Bad => colors::UI_BAD,
        LineStyle::Talk => colors::UI_TALK,
    }
}
