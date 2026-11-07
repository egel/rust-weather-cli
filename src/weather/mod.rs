pub mod open_meteo;

use serde::Deserialize;

/// Full weather bundle for one place.
#[derive(Debug, Clone)]
pub struct Forecast {
    pub current: Current,
    pub hourly: Hourly,
    pub daily: Vec<Day>,
}

/// Current conditions reported by Open-Meteo.
#[derive(Debug, Clone, Deserialize)]
pub struct Current {
    pub time: String,
    #[serde(rename = "temperature_2m")]
    pub temperature: f64,
    #[serde(rename = "apparent_temperature")]
    pub apparent_temperature: f64,
    #[serde(rename = "relative_humidity_2m")]
    pub humidity: i64,
    #[serde(rename = "weather_code")]
    pub code: i64,
    #[serde(rename = "wind_speed_10m")]
    pub wind_speed: f64,
    #[serde(rename = "wind_direction_10m", default)]
    pub wind_direction: i64,
    pub precipitation: f64,
}

/// Hourly time series (3 days requested, sliced by the UI as needed).
#[derive(Debug, Clone, Deserialize)]
pub struct Hourly {
    pub time: Vec<String>,
    #[serde(rename = "temperature_2m")]
    pub temperature: Vec<f64>,
    pub precipitation: Vec<f64>,
    #[serde(rename = "wind_speed_10m")]
    pub wind_speed: Vec<f64>,
}

/// One forecast day, assembled from the API's parallel arrays.
#[derive(Debug, Clone)]
pub struct Day {
    pub date: String,
    pub code: i64,
    pub temp_max: f64,
    pub temp_min: f64,
    pub precip_prob: i64,
}

/// Map a WMO weather interpretation code to an emoji.
/// Isolated here so the icon style (emoji / ASCII / nerd font) can be swapped easily.
pub fn wmo_emoji(code: i64) -> &'static str {
    match code {
        0 => "☀️",
        1 => "🌤️",
        2 => "⛅",
        3 => "☁️",
        45 | 48 => "🌫️",
        51..=57 => "🌦️",
        61..=67 => "🌧️",
        71..=77 => "🌨️",
        80..=82 => "🌧️",
        85 | 86 => "❄️",
        95..=99 => "⛈️",
        _ => "🌡️",
    }
}

/// Map a WMO weather interpretation code to a human-readable label.
pub fn wmo_label(code: i64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Dense drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Light showers",
        81 => "Showers",
        82 => "Violent showers",
        85 => "Snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

/// Arrow pointing *where the wind blows to* (the API direction is where it comes from).
pub fn wind_arrow(degrees: i64) -> &'static str {
    let arrows = ["↑", "↗", "→", "↘", "↓", "↙", "←", "↖"];
    let to = (degrees.rem_euclid(360) + 180) % 360;
    arrows[((to / 45) % 8) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmo_codes_map_to_icons() {
        assert_eq!(wmo_emoji(0), "☀️");
        assert_eq!(wmo_emoji(2), "⛅");
        assert_eq!(wmo_emoji(61), "🌧️");
        assert_eq!(wmo_emoji(95), "⛈️");
        assert_eq!(wmo_emoji(999), "🌡️");
    }

    #[test]
    fn wmo_codes_map_to_labels() {
        assert_eq!(wmo_label(0), "Clear sky");
        assert_eq!(wmo_label(63), "Rain");
        assert_eq!(wmo_label(12345), "Unknown");
    }

    #[test]
    fn wind_arrow_points_downwind() {
        // Wind from the north blows south.
        assert_eq!(wind_arrow(0), "↓");
        // Wind from the east blows west.
        assert_eq!(wind_arrow(90), "←");
        // 360 and negative angles wrap correctly.
        assert_eq!(wind_arrow(360), "↓");
        assert_eq!(wind_arrow(-90), "→");
    }
}
