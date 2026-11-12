//! The weather, from Open-Meteo (open-meteo.com): free, no key, no account.
//!
//! Two GET requests: the geocoding service finds the city the user typed
//! (once, then its coordinates are remembered), and the forecast service
//! gives the weather now and for the next days. The weather widget asks
//! again every half hour.

use crate::json::{self, Json};
use crate::net;

/// How often the weather is asked again.
pub const REFRESH_MINUTES: u64 = 30;

/// Days of forecast shown after today.
pub const DAYS: usize = 3;

/// What the sky is doing, from the WMO weather codes Open-Meteo gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sky {
    Clear,
    MostlyClear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Showers,
    Thunder,
}

impl Sky {
    pub fn from_code(code: u32) -> Sky {
        match code {
            0 => Sky::Clear,
            1 => Sky::MostlyClear,
            2 => Sky::PartlyCloudy,
            3 => Sky::Overcast,
            45 | 48 => Sky::Fog,
            51..=57 => Sky::Drizzle,
            61..=67 => Sky::Rain,
            71..=77 | 85 | 86 => Sky::Snow,
            80..=82 => Sky::Showers,
            95..=99 => Sky::Thunder,
            _ => Sky::Overcast,
        }
    }

    /// Text in the widget (translated by the caller).
    pub fn label(self) -> &'static str {
        match self {
            Sky::Clear => "Clear sky",
            Sky::MostlyClear => "Mostly clear",
            Sky::PartlyCloudy => "Partly cloudy",
            Sky::Overcast => "Cloudy",
            Sky::Fog => "Fog",
            Sky::Drizzle => "Drizzle",
            Sky::Rain => "Rain",
            Sky::Snow => "Snow",
            Sky::Showers => "Showers",
            Sky::Thunder => "Thunderstorm",
        }
    }

    pub const ALL: [Sky; 10] =
        [Sky::Clear, Sky::MostlyClear, Sky::PartlyCloudy, Sky::Overcast, Sky::Fog, Sky::Drizzle, Sky::Rain, Sky::Snow, Sky::Showers, Sky::Thunder];
}

/// A day of the forecast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub y: i32,
    pub m: u32,
    pub d: u32,
    pub sky: Sky,
    pub max: f64,
    pub min: f64,
}

/// The weather at a place.
#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub temp: f64,
    pub sky: Sky,
    /// Daytime there (the sun is drawn, else the moon).
    pub day: bool,
    /// Today first.
    pub days: Vec<Day>,
}

/// A place found for what the user typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// "Lisboa, Portugal".
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// What the user typed, split as "City, region or country".
pub fn split_city(typed: &str) -> (&str, &str) {
    match typed.split_once(',') {
        Some((c, q)) => (c.trim(), q.trim()),
        None => (typed.trim(), ""),
    }
}

pub fn geocode_url(city: &str, lang: &str) -> String {
    format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=10&language={}&format=json", net::encode(split_city(city).0), net::encode(lang))
}

pub fn forecast_url(lat: f64, lon: f64, fahrenheit: bool) -> String {
    let unit = if fahrenheit { "&temperature_unit=fahrenheit" } else { "" };
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,weather_code,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days={}{unit}",
        DAYS + 1
    )
}

/// The place meant among the answers of the geocoding service: the most
/// populous, among those of the region or country typed after a comma.
pub fn read_place(j: &Json, typed: &str) -> Option<Place> {
    let qualifier = split_city(typed).1.to_lowercase();
    let fits = |r: &Json| {
        qualifier.is_empty()
            || ["country", "admin1", "admin2", "country_code"].iter().any(|k| {
                let v = r.get(k).str().to_lowercase();
                !v.is_empty() && (v.starts_with(&qualifier) || qualifier.starts_with(&v))
            })
    };
    let r = j.get("results").items().iter().filter(|r| fits(r)).max_by_key(|r| r.get("population").num().unwrap_or(0.0) as u64)?;
    let (lat, lon) = (r.get("latitude").num()?, r.get("longitude").num()?);
    let name = r.get("name").str().trim();
    if name.is_empty() || !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    let country = r.get("country").str().trim();
    let name = if country.is_empty() || country == name { name.to_string() } else { format!("{name}, {country}") };
    Some(Place { name, lat, lon })
}

fn date(s: &str) -> Option<(i32, u32, u32)> {
    let mut it = s.splitn(3, '-');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.get(..2)?.parse().ok()?))
}

/// A forecast answer.
pub fn read_forecast(j: &Json) -> Option<Forecast> {
    let c = j.get("current");
    let temp = c.get("temperature_2m").num()?;
    let sky = Sky::from_code(c.get("weather_code").num()? as u32);
    let day = c.get("is_day").num().is_none_or(|v| v != 0.0);
    let d = j.get("daily");
    let days = d
        .get("time")
        .items()
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let (y, m, dd) = date(t.str())?;
            Some(Day {
                y,
                m,
                d: dd,
                sky: Sky::from_code(d.get("weather_code").at(i).num()? as u32),
                max: d.get("temperature_2m_max").at(i).num()?,
                min: d.get("temperature_2m_min").at(i).num()?,
            })
        })
        .collect();
    Some(Forecast { temp, sky, day, days })
}

fn get_json(url: &str) -> Result<Json, String> {
    let r = net::get(url, 512 * 1024)?;
    json::parse(&String::from_utf8_lossy(&r.body))
}

/// Finds `city` (in language `lang`, such as "pt").
pub fn find(city: &str, lang: &str) -> Result<Option<Place>, String> {
    Ok(read_place(&get_json(&geocode_url(city, lang))?, city))
}

/// The weather at a place.
pub fn fetch(lat: f64, lon: f64, fahrenheit: bool) -> Result<Forecast, String> {
    read_forecast(&get_json(&forecast_url(lat, lon, fahrenheit))?).ok_or_else(|| "unexpected answer".to_string())
}

/// A temperature as shown: "18°".
pub fn degrees(t: f64) -> String {
    let r = t.round();
    // No "-0°".
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r:.0}°")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers() {
        let g = json::parse(
            r#"{"results":[{"name":"Lisboa","latitude":-20.12,"longitude":33.77,"country":"Moçambique","admin1":"Manica","population":1200},
            {"id":2267057,"name":"Lisboa","latitude":38.71667,"longitude":-9.13333,"country":"Portugal","country_code":"PT","admin1":"Lisboa","population":517802}],"generationtime_ms":0.5}"#,
        )
        .unwrap();
        let lisbon = Some(Place { name: "Lisboa, Portugal".into(), lat: 38.71667, lon: -9.13333 });
        assert_eq!(read_place(&g, "Lisboa"), lisbon, "the largest");
        assert_eq!(read_place(&g, "Lisboa, PT"), lisbon);
        assert_eq!(read_place(&g, "lisboa, moç").map(|p| p.lat), Some(-20.12), "the one asked for");
        assert_eq!(read_place(&g, "Lisboa, Brasil"), None);
        assert_eq!(read_place(&json::parse(r#"{"generationtime_ms":0.5}"#).unwrap(), "x"), None);
        assert_eq!(split_city(" Porto , Portugal "), ("Porto", "Portugal"));
        let f = json::parse(
            r#"{"latitude":38.7,"current":{"time":"2026-11-12T14:00","interval":900,"temperature_2m":17.6,"weather_code":2,"is_day":1},
            "daily":{"time":["2026-11-12","2026-11-13","2026-11-14","2026-11-15"],"weather_code":[2,61,0,95],
            "temperature_2m_max":[19.1,16.0,18.2,15.5],"temperature_2m_min":[11.0,10.4,9.8,-0.3]}}"#,
        )
        .unwrap();
        let fc = read_forecast(&f).unwrap();
        assert_eq!((fc.sky, fc.day, degrees(fc.temp)), (Sky::PartlyCloudy, true, "18°".to_string()));
        assert_eq!(fc.days.len(), 4);
        assert_eq!((fc.days[1].d, fc.days[1].sky), (13, Sky::Rain));
        assert_eq!(degrees(fc.days[3].min), "0°");
        assert_eq!(Sky::from_code(99), Sky::Thunder);
        assert_eq!(Sky::from_code(86), Sky::Snow);
        assert!(geocode_url("São Paulo, Brasil", "pt").contains("name=S%C3%A3o+Paulo&count=10&language=pt"));
        assert!(forecast_url(38.71667, -9.13333, true).contains("latitude=38.7167&longitude=-9.1333&") && forecast_url(0.0, 0.0, true).ends_with("fahrenheit"));
    }

    /// Asks Open-Meteo for real (`cargo test -p rwall --lib weather_live -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn weather_live() {
        let p = find("Lisboa", "pt").unwrap().expect("found");
        println!("{p:?}");
        let f = fetch(p.lat, p.lon, false).unwrap();
        println!("{f:?}");
        assert_eq!(f.days.len(), DAYS + 1);
        // As the regional settings fill it in: the time zone's city in English.
        let p = find("Lisbon, PT", "pt").unwrap().expect("found");
        assert!(p.name.contains("Portugal") && (p.lat - 38.7).abs() < 0.5, "{p:?}");
    }
}
