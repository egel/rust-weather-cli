use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A place resolved via the Open-Meteo geocoding API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub country: String,
    #[serde(default)]
    pub admin1: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// Human-readable display: `Name, Admin region, Country` (region dropped when equal to country).
    pub fn display_name(&self) -> String {
        match &self.admin1 {
            Some(region) if !region.is_empty() && region != &self.country => {
                format!("{}, {}, {}", self.name, region, self.country)
            }
            _ => format!("{}, {}", self.name, self.country),
        }
    }
}

#[derive(Deserialize, Debug, Default)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<Place>,
}

/// Search for places matching a free-form city name using Open-Meteo geocoding.
pub async fn search(query: &str) -> Result<Vec<Place>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let response = client
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query(&[
            ("name", query),
            ("count", "10"),
            ("language", "en"),
            ("format", "json"),
        ])
        .send()
        .await
        .context("geocoding request failed")?
        .error_for_status()
        .context("geocoding API returned an error")?
        .json::<GeoResponse>()
        .await
        .context("could not parse geocoding response")?;
    Ok(response.results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_geocoding_results() {
        let json = r#"{
            "results": [
                {
                    "name": "London",
                    "country": "United Kingdom",
                    "admin1": "England",
                    "latitude": 51.5072,
                    "longitude": -0.1276
                },
                {
                    "name": "Londonderry",
                    "country": "United Kingdom",
                    "admin1": "Northern Ireland",
                    "latitude": 54.9972,
                    "longitude": -7.3089
                }
            ]
        }"#;
        let response: GeoResponse = serde_json::from_str(json).expect("valid geocoding JSON");
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].name, "London");
        assert_eq!(response.results[0].latitude, 51.5072);
    }

    #[test]
    fn missing_results_is_empty() {
        let response: GeoResponse = serde_json::from_str("{}").expect("valid JSON");
        assert!(response.results.is_empty());
    }

    #[test]
    fn display_name_includes_admin1() {
        let place = Place {
            name: "Warsaw".into(),
            country: "Poland".into(),
            admin1: Some("Masovian".into()),
            latitude: 52.2,
            longitude: 21.0,
        };
        assert_eq!(place.display_name(), "Warsaw, Masovian, Poland");
    }

    #[test]
    fn display_name_skips_redundant_admin1() {
        let place = Place {
            name: "Singapore".into(),
            country: "Singapore".into(),
            admin1: Some("Singapore".into()),
            latitude: 1.35,
            longitude: 103.8,
        };
        assert_eq!(place.display_name(), "Singapore, Singapore");
    }
}
