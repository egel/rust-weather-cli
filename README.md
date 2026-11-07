# rust-weather-cli

A keyboard-first **terminal weather dashboard** built with Rust and
[ratatui](https://ratatui.rs), powered by [Open-Meteo](https://open-meteo.com) —
**no API key required**.

```
┌─ Paris, Île-de-France Region, France ── as of 08:00 · updated 08:07:40 ─┐
│ 🌫️ 3.8°C  (feels 1.7°C)   Fog                                          │
│ 💨 ↖ 4km/h   💧 97%   ☂ 0.0 mm                                         │
├─ hourly temperature (°C) — tab to switch ──────────────────────────────┤
│  ⣀⠤⠔⠒⠒⠒⠒⠒⠢⠤⣀                                                        │
├─ 7-day forecast ───────────────────────────────────────────────────────┤
│ Sat 🌫️ 11°/4° 💧0% │ Sun 🌫️ 8°/3° 💧0% │ Mon 🌧️ 11°/4° 💧57% ...      │
└─ q quit · c city · u units · r refresh · tab chart (temperature) ───────┘
```

## Features

- **Current conditions** — temperature, feels-like, condition (emoji + label),
  wind speed/direction, humidity, precipitation
- **Hourly chart** — next 24 hours of temperature / wind speed as a line chart,
  and precipitation rendered as **rain bars** (toggle with `tab`)
- **7-day forecast strip** — weekday icon, high/low, precipitation probability
- **Auto-refresh** — while open on the dashboard, data is refetched every
  10 minutes (countdown shown in the footer)
- **City search** — type-ahead geocoding via Open-Meteo; pick from a result list
- **Metric ⇄ imperial units** (`°C/°F`, `km/h/mph`, `mm/in`) — persisted
- **Last city persisted** — reopening the app shows your latest city


## Usage

```sh
cargo run --release
```

First launch opens the city-search popup. Type a name, press `enter`, pick a
result with `↑/↓`, confirm with `enter`. Afterwards the app starts on your
last city.

### Keybindings

| Key | Action |
|---|---|
| `q`, `ctrl+c` | quit |
| `c` | open city search |
| `u` | toggle units (metric ⇄ imperial) |
| `r` | refresh forecast |
| `tab` | switch chart metric (temperature → precipitation → wind) |
| `↑/↓` + `enter` | navigate/select in the search popup |
| `esc` | close search / back |

## Config

Settings live in the platform config directory:

- macOS: `~/Library/Application Support/rust-weather-cli/config.json`
- Linux: `~/.config/rust-weather-cli/config.json`

Stores the last `place` and preferred `units`.

## Development

```sh
cargo test   # unit tests (serde fixtures, WMO code mappings)
cargo run    # start the TUI in your terminal
```

Project layout:

```
src/
├── main.rs              # boot: terminal init/teardown, tokio main
├── config.rs            # persisted settings (Units, last place)
├── geo.rs               # Open-Meteo geocoding (city → lat/lon)
├── weather/
│   ├── mod.rs           # forecast types + WMO code → emoji/label maps
│   └── open_meteo.rs    # forecast client
└── tui/
    ├── mod.rs           # App state + event loop (keys & fetch results over channel)
    └── ui.rs            # ratatui rendering (dashboard, search popup, loading, error)
```

The UI never blocks on the network: fetches run as spawned tokio tasks and
deliver results over an `mpsc` channel; the event loop `select!`s between
keys, fetch results, and a tick that drives animations and the 10-minute
auto-refresh timer. Manual `r`/`u` refreshes keep the dashboard on screen with
a `⟳ refreshing…` indicator in the header.

## License

See [LICENSE](LICENSE).
