mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::config::{Config, Units};
use crate::geo::{self, Place};
use crate::weather::{open_meteo, Forecast};

/// Which metric the hourly chart is rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartMetric {
    Temperature,
    Precipitation,
    Wind,
}

/// How often the dashboard refetches data while it is open.
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);

impl ChartMetric {
    fn next(self) -> Self {
        match self {
            Self::Temperature => Self::Precipitation,
            Self::Precipitation => Self::Wind,
            Self::Wind => Self::Temperature,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Precipitation => "precipitation",
            Self::Wind => "wind speed",
        }
    }

    pub fn unit(self, units: Units) -> &'static str {
        match self {
            Self::Temperature => units.temp(),
            Self::Precipitation => units.precip(),
            Self::Wind => units.wind(),
        }
    }
}

#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<Place>,
    pub selected: usize,
    pub searching: bool,
    pub error: Option<String>,
}

enum Mode {
    Loading,
    Dashboard,
    Search(SearchState),
    Error(String),
}

/// Messages flowing into the single-threaded event loop.
enum Msg {
    Key(KeyEvent),
    ForecastReady(Result<Forecast>),
    SearchReady(Result<Vec<Place>>),
}

pub struct App {
    tx: mpsc::UnboundedSender<Msg>,
    mode: Mode,
    place: Option<Place>,
    forecast: Option<Forecast>,
    pub units: Units,
    config: Config,
    chart_metric: ChartMetric,
    hourly_start: usize,
    spinner: usize,
    /// A fetch is in flight while the dashboard keeps showing the old data.
    refreshing: bool,
    /// When due, the next tick triggers an automatic refresh (Dashboard only).
    next_auto_refresh: Option<std::time::Instant>,
    last_updated: Option<chrono::DateTime<chrono::Local>>,
    should_quit: bool,
}

impl App {
    fn new(tx: mpsc::UnboundedSender<Msg>, config: Config, units: Units) -> Self {
        Self {
            tx,
            mode: Mode::Loading,
            place: None,
            forecast: None,
            units,
            config,
            chart_metric: ChartMetric::Temperature,
            hourly_start: 0,
            spinner: 0,
            refreshing: false,
            next_auto_refresh: None,
            last_updated: None,
            should_quit: false,
        }
    }

    /// Kick off an async forecast fetch *while keeping the current view*; a
    /// spinner indicator shows that a refresh is in flight.
    fn start_fetch(&mut self, place: Place) {
        self.place = Some(place.clone());
        self.refreshing = true;
        let tx = self.tx.clone();
        let units = self.units;
        tokio::spawn(async move {
            let result = open_meteo::fetch_forecast(&place, units).await;
            let _ = tx.send(Msg::ForecastReady(result));
        });
    }

    /// Kick off a fetch and switch to the loading screen (startup / city choice).
    fn request_forecast(&mut self, place: Place) {
        self.mode = Mode::Loading;
        self.start_fetch(place);
    }

    fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Key(key) => self.on_key(key),
            Msg::SearchReady(result) => {
                let Mode::Search(state) = &mut self.mode else {
                    return;
                };
                state.searching = false;
                match result {
                    Ok(results) if results.is_empty() => {
                        state.error = Some("No matching places found".to_string());
                    }
                    Ok(results) => {
                        state.results = results;
                        state.selected = 0;
                        state.error = None;
                    }
                    Err(e) => state.error = Some(format!("Search failed: {e}")),
                }
            }
            Msg::ForecastReady(result) => match result {
                Ok(forecast) => {
                    self.forecast = Some(forecast);
                    self.hourly_start = self.compute_hourly_start();
                    self.last_updated = Some(chrono::Local::now());
                    self.refreshing = false;
                    self.next_auto_refresh = Some(std::time::Instant::now() + AUTO_REFRESH_INTERVAL);
                    self.mode = Mode::Dashboard;
                }
                Err(e) => {
                    self.refreshing = false;
                    let city = self
                        .place
                        .as_ref()
                        .map(|p| p.display_name())
                        .unwrap_or_else(|| "selected place".to_string());
                    self.mode = Mode::Error(format!("Could not load weather for {city}: {e}"));
                }
            },
        }
    }

    /// Index of the first hourly slot not yet in the past (string-compare works because
    /// both the API and chrono use `YYYY-MM-DDTHH:00` local time).
    fn compute_hourly_start(&self) -> usize {
        let Some(forecast) = &self.forecast else {
            return 0;
        };
        let now_key = chrono::Local::now().format("%Y-%m-%dT%H:00").to_string();
        forecast
            .hourly
            .time
            .iter()
            .position(|t| t.as_str() >= now_key.as_str())
            .unwrap_or(0)
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Ctrl+C quits from anywhere.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        match &self.mode {
            Mode::Loading => {
                if key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                }
            }
            Mode::Dashboard => self.on_dashboard_key(key.code),
            Mode::Search(_) => self.on_search_key(key),
            Mode::Error(_) => self.on_error_key(key.code),
        }
    }

    fn on_dashboard_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') => self.mode = Mode::Search(SearchState::default()),
            KeyCode::Tab => self.chart_metric = self.chart_metric.next(),
            KeyCode::Char('r') => {
                if let (Some(place), false) = (self.place.clone(), self.refreshing) {
                    self.start_fetch(place);
                }
            }
            KeyCode::Char('u') => {
                if self.refreshing {
                    return;
                }
                self.units = self.units.toggle();
                self.config.units = Some(self.units);
                let _ = self.config.save();
                if let Some(place) = self.place.clone() {
                    self.start_fetch(place);
                }
            }
            _ => {}
        }
    }

    fn on_error_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc if self.forecast.is_some() => self.mode = Mode::Dashboard,
            KeyCode::Char('r') => {
                if let (Some(place), false) = (self.place.clone(), self.refreshing) {
                    self.start_fetch(place);
                }
            }
            _ => {}
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        enum Action {
            None,
            Back,
            Quit,
            Backspace,
            Type(char),
            Up,
            Down,
            RunQuery(String),
            Choose(usize),
        }

        let action = {
            let Mode::Search(state) = &mut self.mode else {
                return;
            };
            match key.code {
                KeyCode::Esc => {
                    if self.forecast.is_some() {
                        Action::Back
                    } else {
                        // Nothing to go back to (first launch) — Esc exits the app.
                        Action::Quit
                    }
                }
                KeyCode::Backspace => Action::Backspace,
                KeyCode::Up => Action::Up,
                KeyCode::Down => Action::Down,
                KeyCode::Enter => {
                    if !state.results.is_empty() {
                        Action::Choose(state.selected)
                    } else {
                        let query = state.query.trim().to_string();
                        if query.is_empty() || state.searching {
                            Action::None
                        } else {
                            Action::RunQuery(query)
                        }
                    }
                }
                KeyCode::Char(c) => Action::Type(c),
                _ => Action::None,
            }
        };

        match action {
            Action::None => {}
            Action::Quit => self.should_quit = true,
            Action::Back => self.mode = Mode::Dashboard,
            Action::Backspace => {
                if let Mode::Search(state) = &mut self.mode {
                    state.query.pop();
                }
            }
            Action::Type(c) => {
                if let Mode::Search(state) = &mut self.mode {
                    state.query.push(c);
                    state.results.clear();
                    state.selected = 0;
                    state.error = None;
                }
            }
            Action::Up => {
                if let Mode::Search(state) = &mut self.mode {
                    state.selected = state.selected.saturating_sub(1);
                }
            }
            Action::Down => {
                if let Mode::Search(state) = &mut self.mode {
                    if state.selected + 1 < state.results.len() {
                        state.selected += 1;
                    }
                }
            }
            Action::RunQuery(query) => {
                if let Mode::Search(state) = &mut self.mode {
                    state.searching = true;
                }
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = geo::search(&query).await;
                    let _ = tx.send(Msg::SearchReady(result));
                });
            }
            Action::Choose(index) => {
                let place = (|| {
                    let Mode::Search(state) = &self.mode else {
                        return None;
                    };
                    state.results.get(index).cloned()
                })();
                if let Some(place) = place {
                    self.config.place = Some(place.clone());
                    let _ = self.config.save();
                    self.request_forecast(place);
                }
            }
        }
    }
}

/// Terminal entry point: boot the app, spawn the key-reading thread, and run the
/// single-threaded event loop. Network results arrive over a channel so rendering
/// never blocks on HTTP.
pub async fn run(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let config = Config::load();
    let units = config.units.unwrap_or_default();
    let mut app = App::new(tx, config, units);

    match app.config.place.clone() {
        Some(place) => app.request_forecast(place),
        None => app.mode = Mode::Search(SearchState::default()),
    }

    // Dedicated OS thread translating terminal events to `Msg::Key`.
    // The thread lives until process exit; main returning tears everything down.
    {
        let tx = app.tx.clone();
        std::thread::spawn(move || loop {
            match crossterm::event::read() {
                Ok(Event::Key(key)) => {
                    if tx.send(Msg::Key(key)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        });
    }

    let mut tick = tokio::time::interval(Duration::from_millis(300));
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(msg) => app.handle_msg(msg),
                    None => break,
                }
            }
            _ = tick.tick() => app.on_tick(),
        }
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

impl App {
    /// Called every event-loop tick (300ms): animates the spinner and
    /// triggers an automatic refresh when the timer is due.
    fn on_tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
        self.maybe_auto_refresh();
    }

    /// Pure check used by tests too: should we fire an automatic refresh?
    fn auto_refresh_due(&self) -> bool {
        let Mode::Dashboard = &self.mode else {
            return false;
        };
        self.place.is_some()
            && !self.refreshing
            && self
                .next_auto_refresh
                .is_some_and(|next| std::time::Instant::now() >= next)
    }

    fn maybe_auto_refresh(&mut self) {
        if self.auto_refresh_due() {
            if let Some(place) = self.place.clone() {
                self.start_fetch(place);
            }
        }
    }

    /// Time left until the next automatic refresh, if one is scheduled.
    fn next_refresh_in(&self) -> Option<Duration> {
        self.next_auto_refresh
            .map(|next| next.saturating_duration_since(std::time::Instant::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::{Current, Forecast, Hourly};
    use ratatui::{backend::TestBackend, Terminal};
    use std::time::Instant;

    fn test_place() -> Place {
        Place {
            name: "Paris".to_string(),
            country: "France".to_string(),
            admin1: None,
            latitude: 48.85,
            longitude: 2.35,
        }
    }

    fn dashboard_app(next_auto_refresh: Option<Instant>) -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(tx, Config::default(), Units::Metric);
        app.mode = Mode::Dashboard;
        app.place = Some(test_place());
        app.next_auto_refresh = next_auto_refresh;
        app
    }

    #[test]
    fn auto_refresh_fires_when_due_on_dashboard() {
        let due = Some(Instant::now() - AUTO_REFRESH_INTERVAL);
        assert!(dashboard_app(due).auto_refresh_due());
        // Just scheduled: not due yet.
        let soon = Some(Instant::now() + Duration::from_secs(60));
        assert!(!dashboard_app(soon).auto_refresh_due());
    }

    #[test]
    fn auto_refresh_requires_place_idle_and_dashboard() {
        let due = Some(Instant::now() - AUTO_REFRESH_INTERVAL);

        // No place set -> never refresh.
        let mut app = dashboard_app(due);
        app.place = None;
        assert!(!app.auto_refresh_due(), "place required");

        // Refresh already in flight -> no overlapping refresh.
        let mut app = dashboard_app(due);
        app.refreshing = true;
        assert!(!app.auto_refresh_due(), "no overlapping refresh");

        // Not on the dashboard (e.g. search popup open) -> wait until the user returns.
        let mut app = dashboard_app(due);
        app.mode = Mode::Search(SearchState::default());
        assert!(!app.auto_refresh_due(), "dashboard only");
    }

    #[test]
    fn next_refresh_in_counts_down_only_when_scheduled() {
        let remaining = Duration::from_secs(125);
        let scheduled = dashboard_app(Some(Instant::now() + remaining));
        // Allow a tiny scheduling skew between `now` in the app and the test.
        let next = scheduled.next_refresh_in().expect("scheduled");
        assert!(next <= remaining && next >= remaining - Duration::from_secs(1));
        assert_eq!(dashboard_app(None).next_refresh_in(), None);
    }

    // --- rendering tests (deterministic, via ratatui TestBackend) ---

    fn sample_forecast() -> Forecast {
        let time: Vec<String> = (0..24).map(|i| format!("2026-11-07T{i:02}:00")).collect();
        Forecast {
            current: Current {
                time: "2026-11-07T08:00".to_string(),
                temperature: 8.3,
                apparent_temperature: 6.1,
                humidity: 82,
                code: 61,
                wind_speed: 12.4,
                wind_direction: 250,
                precipitation: 1.2,
            },
            hourly: Hourly {
                time,
                temperature: (0..24).map(|i| 10.0 + i as f64 / 2.0).collect(),
                precipitation: (0..24).map(|i| if i % 2 == 0 { 0.0 } else { 1.5 }).collect(),
                wind_speed: (0..24).map(|i| 8.0 + i as f64 / 3.0).collect(),
            },
            daily: vec![],
        }
    }

    fn render(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 34)).expect("terminal");
        terminal
            .draw(|frame| super::ui::draw(frame, app))
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect()
    }

    fn dashboard_with_metric(metric: ChartMetric, scheduled: Option<Instant>) -> App {
        let mut app = dashboard_app(scheduled);
        app.forecast = Some(sample_forecast());
        app.chart_metric = metric;
        app
    }

    #[test]
    fn dashboard_renders_header_chart_footer() {
        let app = dashboard_with_metric(
            ChartMetric::Temperature,
            Some(Instant::now() + Duration::from_secs(599)),
        );
        let screen = render(&app);
        assert!(screen.contains("Paris"), "header shows the place name");
        assert!(
            screen.contains("hourly temperature (°C)"),
            "chart title with metric and unit"
        );
        assert!(screen.contains("auto-refresh in"), "footer shows countdown");
        assert!(screen.contains("7-day forecast"), "daily strip title");
    }

    #[test]
    fn precipitation_renders_rain_bars_with_zero_baseline() {
        let app = dashboard_with_metric(ChartMetric::Precipitation, None);
        let screen = render(&app);
        assert!(
            screen.contains("hourly precipitation (mm)"),
            "precipitation chart title"
        );
        assert!(screen.contains("0.0"), "y axis starts at zero for bars");
        assert!(screen.contains("1.5mm"), "y axis max labelled with unit");
    }

    #[test]
    fn wind_speed_stays_a_line_chart() {
        let app = dashboard_with_metric(ChartMetric::Wind, None);
        let screen = render(&app);
        assert!(screen.contains("hourly wind speed (km/h)"));
        assert!(screen.contains("auto-refresh: —"), "no timer scheduled");
    }
}

