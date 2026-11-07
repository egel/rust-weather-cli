use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, Cell, Chart, Clear, Dataset, GraphType, List, ListItem, ListState,
        Paragraph, Row, Table,
    },
    Frame,
};

use super::{App, ChartMetric, Mode, SearchState};
use crate::weather;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn draw(frame: &mut Frame, app: &App) {
    match &app.mode {
        Mode::Loading => draw_loading(frame, app),
        Mode::Dashboard => draw_dashboard(frame, app),
        Mode::Search(state) => {
            draw_base(frame, app);
            draw_search_popup(frame, app, state);
        }
        Mode::Error(message) => draw_error(frame, message),
    }
}

/// What shows underneath the search popup: the old dashboard or a hint.
fn draw_base(frame: &mut Frame, app: &App) {
    if app.forecast.is_some() {
        draw_dashboard(frame, app);
    } else {
        let area = Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .split(frame.area())[0];
        frame.render_widget(
            Paragraph::new("No forecast loaded yet — pick a city to get started.")
                .style(Style::new().dim())
                .alignment(Alignment::Center),
            area,
        );
    }
}

fn draw_dashboard(frame: &mut Frame, app: &App) {
    let Some(forecast) = &app.forecast else {
        return;
    };
    let Some(place) = &app.place else {
        return;
    };

    let layout = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(6),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(frame.area());

    draw_header(frame, layout[0], app, place, forecast);
    draw_chart(frame, layout[1], app);
    draw_daily(frame, layout[2], forecast);
    draw_footer(frame, layout[3], app);
}

fn draw_header(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    place: &crate::geo::Place,
    forecast: &crate::weather::Forecast,
) {
    let current = &forecast.current;
    let units = app.units;
    let emoji = weather::wmo_emoji(current.code);
    let label = weather::wmo_label(current.code);
    let arrow = weather::wind_arrow(current.wind_direction);

    let updated = app
        .last_updated
        .map(|time| format!("updated {}", time.format("%H:%M:%S")))
        .unwrap_or_default();
    let as_of = current
        .time
        .get(11..16)
        .map(|time| format!(" as of {time} · {updated}"))
        .unwrap_or_else(|| format!(" {updated}"));

    let line_one = Line::from(vec![
        Span::raw(format!("{emoji} ")),
        Span::styled(
            format!("{:.1}", current.temperature),
            Style::new().bold().cyan(),
        ),
        Span::raw(units.temp().to_string()),
        Span::raw(format!(
            "  (feels {:.1}{})   ",
            current.apparent_temperature,
            units.temp()
        )),
        Span::styled(label, Style::new().italic()),
    ]);

    let line_two = Line::from(vec![
        Span::raw(format!("💨 {arrow} {:.0}{}   ", current.wind_speed, units.wind())),
        Span::raw(format!("💧 {:.0}%   ", current.humidity)),
        Span::raw(format!(
            "☂ {:.1} {}",
            current.precipitation,
            units.precip()
        )),
    ]);

    let block = Block::bordered()
        .title(Span::styled(
            format!(" {} ", place.display_name()),
            Style::new().bold(),
        ))
        .title(
            Line::from(format!("{as_of}    u:units  r:refresh"))
                .right_aligned()
                .fg(Color::DarkGray),
        );
    frame.render_widget(Paragraph::new(vec![line_one, line_two]).block(block), area);
}

fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let Some(forecast) = &app.forecast else {
        return;
    };
    let hourly = &forecast.hourly;
    if hourly.time.is_empty() {
        return;
    }
    let start = app.hourly_start.min(hourly.time.len() - 1);
    let end = (start + 24).min(hourly.time.len());
    let values: Vec<f64> = match app.chart_metric {
        ChartMetric::Temperature => hourly.temperature[start..end].to_vec(),
        ChartMetric::Precipitation => hourly.precipitation[start..end].to_vec(),
        ChartMetric::Wind => hourly.wind_speed[start..end].to_vec(),
    };
    if values.is_empty() {
        return;
    }

    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(index, value)| (index as f64, *value))
        .collect();
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let pad = (max - min).max(1.0) * 0.15;
    let unit = app.chart_metric.unit(app.units);

    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::new().cyan())
        .data(&points);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::bordered().title(format!(
                " hourly {} ({}) — tab to switch ",
                app.chart_metric.title(),
                unit
            )),
        )
        .x_axis(
            Axis::default()
                .bounds([0.0, (end - start) as f64])
                .labels(["now", "+12h", "+24h"]),
        )
        .y_axis(
            Axis::default()
                .bounds([min - pad, max + pad])
                .labels([format!("{min:.1}"), format!("{max:.1}{unit}")]),
        );
    frame.render_widget(chart, area);
}

fn draw_daily(frame: &mut Frame, area: Rect, forecast: &crate::weather::Forecast) {
    let count = forecast.daily.len() as u16;

    let row = Row::new(forecast.daily.iter().map(|day| {
        let weekday = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
            .map(|date| date.format("%a").to_string())
            .unwrap_or_else(|_| day.date.clone());
        Cell::from(format!(
            " {} {} {:.0}°/{:.0}° 💧{}% ",
            weekday,
            weather::wmo_emoji(day.code),
            day.temp_max,
            day.temp_min,
            day.precip_prob
        ))
    }));

    let widths: Vec<Constraint> = (0..count.max(1))
        .map(|_| Constraint::Ratio(1, count.max(1) as u32))
        .collect();

    let table = Table::new(vec![row], widths)
        .block(Block::bordered().title(" 7-day forecast "))
        .column_spacing(0)
        .style(Style::new().white());
    frame.render_widget(table, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(format!(
            " q quit · c city · u units · r refresh · tab chart ({}) ",
            app.chart_metric.title()
        ))
        .style(Style::new().dim()),
        area,
    );
}

fn draw_loading(frame: &mut Frame, app: &App) {
    let spinner = SPINNER[app.spinner % SPINNER.len()];
    let text = match &app.place {
        Some(place) => format!("{spinner} Loading forecast for {}…", place.display_name()),
        None => format!("{spinner} Loading…"),
    };
    let area = Layout::vertical([Constraint::Length(3)])
        .flex(Flex::Center)
        .split(frame.area())[0];
    frame.render_widget(
        Paragraph::new(text).alignment(Alignment::Center),
        area,
    );
}

fn draw_error(frame: &mut Frame, message: &str) {
    let area = centered_rect(70, 40, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Error ")
        .border_style(Style::new().red().bold());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    frame.render_widget(
        Paragraph::new(message.to_string()).style(Style::new().red()),
        layout[0],
    );
    frame.render_widget(
        Paragraph::new(" r retry · esc back · q quit")
            .style(Style::new().dim()),
        layout[1],
    );
}

fn draw_search_popup(frame: &mut Frame, app: &App, state: &SearchState) {
    let area = centered_rect(60, 40, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Search city ")
        .border_style(Style::new().yellow());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let input = Paragraph::new(format!("> {}", state.query))
        .style(Style::new().white())
        .block(Block::bordered().title("type a name · enter to search"));
    frame.render_widget(input, layout[0]);

    if state.searching {
        let spinner = SPINNER[app.spinner % SPINNER.len()];
        frame.render_widget(
            Paragraph::new(format!("{spinner} searching…"))
                .style(Style::new().dim())
                .block(Block::bordered()),
            layout[1],
        );
    } else if state.results.is_empty() {
        let hint = state
            .error
            .clone()
            .unwrap_or_else(|| "results will appear here".to_string());
        frame.render_widget(
            Paragraph::new(hint)
                .style(if state.error.is_some() {
                    Style::new().red()
                } else {
                    Style::new().dim()
                })
                .block(Block::bordered()),
            layout[1],
        );
    } else {
        let items: Vec<ListItem> = state
            .results
            .iter()
            .map(|place| ListItem::new(place.display_name()))
            .collect();
        let list = List::new(items)
            .block(Block::bordered().title("↑/↓ select · enter choose · esc back"))
            .highlight_style(Style::new().bg(Color::Blue).white().bold());
        let mut list_state = ListState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(list, layout[1], &mut list_state);
    }

    frame.render_widget(
        Paragraph::new("enter: search · esc: back · ↑/↓: pick · enter: choose")
            .style(Style::new().dim()),
        layout[2],
    );
}

/// Rectangle centered within `space`, sized as percentages (max 100).
fn centered_rect(percent_x: u16, percent_y: u16, space: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y).div_ceil(2)),
    ])
    .split(space);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x).div_ceil(2)),
    ])
    .split(vertical[1]);
    horizontal[1]
}

