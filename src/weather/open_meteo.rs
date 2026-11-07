use anyhow::{Context, Result};
use serde::Deserialize;

use super::{Current, Day, Forecast, Hourly};
use crate::config::Units;
use crate::geo::Place;

/// Daily block of the API response: parallel arrays instead of objects.
#[derive(Deserialize, Debug)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Vec<i64>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    precipitation_probability_max: Vec<i64>,
}

/// Raw response shape; `daily` is converted into a proper `Vec<Day>` afterwards.
#[derive(Deserialize, Debug)]
struct ApiResponse {
    current: Current,
    hourly: Hourly,
    daily: RawDaily,
}

impl From<ApiResponse> for Forecast {
    fn from(raw: ApiResponse) -> Self {
        let daily_len = raw.daily.time.len();
        let daily = (0..daily_len)
            .map(|i| Day {
                date: raw.daily.time[i].clone(),
                code: raw.daily.weather_code.get(i).copied().unwrap_or(0),
                temp_max: raw.daily.temperature_2m_max.get(i).copied().unwrap_or(0.0),
                temp_min: raw.daily.temperature_2m_min.get(i).copied().unwrap_or(0.0),
                precip_prob: raw
                    .daily
                    .precipitation_probability_max
                    .get(i)
                    .copied()
                    .unwrap_or(0),
            })
            .collect();
        Self {
            current: raw.current,
            hourly: raw.hourly,
            daily,
        }
    }
}

/// Fetch current conditions + hourly + daily forecast for a place.
pub async fn fetch_forecast(place: &Place, units: Units) -> Result<Forecast> {
    let (temp_unit, wind_unit, precip_unit) = match units {
        Units::Metric => ("celsius", "kmh", "mm"),
        Units::Imperial => ("fahrenheit", "mph", "inch"),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let response = client
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&[
            ("latitude", place.latitude.to_string()),
            ("longitude", place.longitude.to_string()),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,wind_direction_10m,precipitation".to_string(),
            ),
            ("hourly", "temperature_2m,precipitation,wind_speed_10m".to_string()),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max"
                    .to_string(),
            ),
            ("timezone", "auto".to_string()),
            ("forecast_days", "3".to_string()),
            ("temperature_unit", temp_unit.to_string()),
            ("wind_speed_unit", wind_unit.to_string()),
            ("precipitation_unit", precip_unit.to_string()),
        ])
        .send()
        .await
        .context("forecast request failed")?
        .error_for_status()
        .context("forecast API returned an error")?
        .json::<ApiResponse>()
        .await
        .context("could not parse forecast response")?;
    Ok(response.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "latitude": 51.5,
        "longitude": -0.12,
        "current": {
            "time": "2026-11-07T14:00",
            "temperature_2m": 8.3,
            "apparent_temperature": 6.1,
            "relative_humidity_2m": 82,
            "weather_code": 61,
            "wind_speed_10m": 12.4,
            "wind_direction_10m": 250,
            "precipitation": 1.2
        },
        "hourly": {
            "time": ["2026-11-07T00:00", "2026-11-07T01:00", "2026-11-07T02:00"],
            "temperature_2m": [7.0, 6.5, 6.2],
            "precipitation": [0.0, 0.1, 0.4],
            "wind_speed_10m": [10.0, 11.0, 12.0]
        },
        "daily": {
            "time": ["2026-11-07", "2026-11-08"],
            "weather_code": [61, 3],
            "temperature_2m_max": [9.0, 10.0],
            "temperature_2m_min": [4.0, 3.0],
            "precipitation_probability_max": [70, 10]
        }
    }"#;

    #[test]
    fn parses_full_response() {
        let raw: ApiResponse = serde_json::from_str(FIXTURE).expect("valid forecast JSON");
        let forecast: Forecast = raw.into();

        assert_eq!(forecast.current.temperature, 8.3);
        assert_eq!(forecast.current.apparent_temperature, 6.1);
        assert_eq!(forecast.current.humidity, 82);
        assert_eq!(forecast.current.code, 61);
        assert_eq!(forecast.current.wind_direction, 250);

        assert_eq!(forecast.hourly.time.len(), 3);
        assert_eq!(forecast.hourly.temperature[1], 6.5);
        assert_eq!(forecast.hourly.precipitation[2], 0.4);

        assert_eq!(forecast.daily.len(), 2);
        assert_eq!(forecast.daily[0].date, "2026-11-07");
        assert_eq!(forecast.daily[0].code, 61);
        assert_eq!(forecast.daily[0].temp_max, 9.0);
        assert_eq!(forecast.daily[0].temp_min, 4.0);
        assert_eq!(forecast.daily[0].precip_prob, 70);
        assert_eq!(forecast.daily[1].precip_prob, 10);
    }

    #[test]
    fn daily_defaults_when_arrays_short() {
        let truncated = r#"{
            "latitude": 0.0,
            "longitude": 0.0,
            "current": {
                "time": "2026-11-07T14:00",
                "temperature_2m": 0.0,
                "apparent_temperature": 0.0,
                "relative_humidity_2m": 50,
                "weather_code": 0,
                "wind_speed_10m": 0.0,
                "wind_direction_10m": 0,
                "precipitation": 0.0
            },
            "hourly": {
                "time": ["2026-11-07T00:00"],
                "temperature_2m": [0.0],
                "precipitation": [0.0],
                "wind_speed_10m": [0.0]
            },
            "daily": {
                "time": ["2026-11-07"],
                "weather_code": [],
                "temperature_2m_max": [],
                "temperature_2m_min": [],
                "precipitation_probability_max": []
            }
        }"#;
        let raw: ApiResponse = serde_json::from_str(truncated).expect("valid JSON");
        let forecast: Forecast = raw.into();
        assert_eq!(forecast.daily.len(), 1);
        assert_eq!(forecast.daily[0].code, 0);
        assert_eq!(forecast.daily[0].temp_max, 0.0);
        assert_eq!(forecast.daily[0].precip_prob, 0);
    }
}
