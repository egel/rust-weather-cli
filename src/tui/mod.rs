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
            last_updated: None,
            should_quit: false,
        }
    }

    /// Kick off an async forecast fetch; the UI shows a spinner meanwhile.
    fn request_forecast(&mut self, place: Place) {
        self.place = Some(place.clone());
        self.mode = Mode::Loading;
        let tx = self.tx.clone();
        let units = self.units;
        tokio::spawn(async move {
            let result = open_meteo::fetch_forecast(&place, units).await;
            let _ = tx.send(Msg::ForecastReady(result));
        });
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
                    self.mode = Mode::Dashboard;
                }
                Err(e) => {
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
                if let Some(place) = self.place.clone() {
                    self.request_forecast(place);
                }
            }
            KeyCode::Char('u') => {
                self.units = self.units.toggle();
                self.config.units = Some(self.units);
                let _ = self.config.save();
                if let Some(place) = self.place.clone() {
                    self.request_forecast(place);
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
                if let Some(place) = self.place.clone() {
                    self.request_forecast(place);
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
            _ = tick.tick() => app.spinner = app.spinner.wrapping_add(1),
        }
        if app.should_quit {
            break;
        }
    }

    Ok(())
}
