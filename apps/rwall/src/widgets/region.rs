//! What the system's regional settings say, for the widgets' first values:
//! 24-hour clock or not, °C or °F, the first day of the week, the country
//! (for the holidays) and the time zone's city (for the weather).
//!
//! Windows: the user's regional format (`GetLocaleInfoEx`), home location
//! (`GetUserDefaultGeoName`) and ICU's time zone (`icu.dll`, Windows 10
//! 1903 and later). Linux: `LC_TIME`/`LANG` and `/etc/localtime`.

use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// The time is written with 24 hours.
    pub h24: bool,
    /// Temperatures in °F (the US system of measurement).
    pub fahrenheit: bool,
    /// The first day of the week, 0 = Sunday; None: as the language says.
    pub first_day: Option<usize>,
    /// ISO 3166 code, upper case ("PT"); empty when unknown.
    pub country: String,
    /// IANA time zone ("Europe/Lisbon"); empty when unknown.
    pub zone: String,
}

impl Region {
    /// The settings of a country, without asking the system.
    pub fn of_country(country: &str) -> Region {
        let c = country.to_ascii_uppercase();
        let us = matches!(c.as_str(), "US" | "LR" | "MM" | "PR" | "GU" | "VI" | "AS" | "MP" | "UM" | "BS" | "BZ" | "KY" | "PW" | "FM" | "MH");
        Region { h24: !matches!(c.as_str(), "US" | "CA" | "AU" | "NZ" | "IN" | "PH" | "EG" | "SA" | "PK"), fahrenheit: us, first_day: None, country: c, zone: String::new() }
    }

    /// The city of the time zone with the country after a comma, ready to
    /// look for: "Lisbon, PT". Empty when unknown.
    pub fn city(&self) -> String {
        zone_city(&self.zone, &self.country)
    }
}

/// "Europe/Lisbon" → "Lisbon, PT"; "America/Argentina/Buenos_Aires" →
/// "Buenos Aires, AR". Zones that are not cities ("UTC", "Etc/GMT+1") give
/// nothing.
pub fn zone_city(zone: &str, country: &str) -> String {
    let Some((area, city)) = zone.rsplit_once('/') else { return String::new() };
    if area.starts_with("Etc") || city.is_empty() || city.starts_with("GMT") || city.chars().any(|c| c.is_ascii_digit()) {
        return String::new();
    }
    let city = city.replace('_', " ");
    if country.is_empty() {
        city
    } else {
        format!("{city}, {country}")
    }
}

/// The system's regional settings (read once).
pub fn region() -> &'static Region {
    static R: OnceLock<Region> = OnceLock::new();
    R.get_or_init(read)
}

#[cfg(windows)]
fn read() -> Region {
    use windows_sys::Win32::Globalization::{GetLocaleInfoEx, GetUserDefaultGeoName, LOCALE_IFIRSTDAYOFWEEK, LOCALE_IMEASURE, LOCALE_STIMEFORMAT};
    let info = |what: u32| -> String {
        let mut buf = [0u16; 128];
        // SAFETY: the buffer and its length; a null name is the user's locale.
        let n = unsafe { GetLocaleInfoEx(std::ptr::null(), what, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
    };
    let country = {
        let mut buf = [0u16; 16];
        // SAFETY: the buffer and its length.
        let n = unsafe { GetUserDefaultGeoName(buf.as_mut_ptr(), buf.len() as i32) };
        let s = String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize]);
        // Only two-letter codes (not the numeric regions such as "001").
        if s.len() == 2 && s.chars().all(|c| c.is_ascii_alphabetic()) {
            s.to_ascii_uppercase()
        } else {
            String::new()
        }
    };
    let time = info(LOCALE_STIMEFORMAT);
    let mut r = Region::of_country(&country);
    if !time.is_empty() {
        r.h24 = time.contains('H');
    }
    match info(LOCALE_IMEASURE).as_str() {
        "1" => r.fahrenheit = true,
        "0" => r.fahrenheit = false,
        _ => {}
    }
    // Windows counts from Monday (0) to Sunday (6). Only the days weeks
    // start on somewhere (Saturday, Sunday, Monday) are taken: another one
    // is more likely a slip in the settings than a wish.
    r.first_day = info(LOCALE_IFIRSTDAYOFWEEK).parse::<usize>().ok().map(|d| (d + 1) % 7).filter(|d| matches!(d, 6 | 0 | 1));
    r.zone = icu_zone();
    r
}

/// The time zone as ICU (shipped with Windows) names it.
#[cfg(windows)]
fn icu_zone() -> String {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
    type GetZone = unsafe extern "C" fn(*mut u16, i32, *mut i32) -> i32;
    let name: Vec<u16> = "icu.dll\0".encode_utf16().collect();
    // SAFETY: loads a system library from System32 only; the function has
    // ICU's documented signature; the buffer and its length.
    unsafe {
        let lib = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if lib.is_null() {
            return String::new();
        }
        let Some(f) = GetProcAddress(lib, c"ucal_getDefaultTimeZone".as_ptr().cast()) else { return String::new() };
        let f: GetZone = std::mem::transmute(f);
        let mut buf = [0u16; 128];
        let mut err = 0i32;
        let n = f(buf.as_mut_ptr(), buf.len() as i32, &mut err);
        if err > 0 || n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())])
    }
}

#[cfg(not(windows))]
fn read() -> Region {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let locale = var("LC_ALL").or_else(|| var("LC_TIME")).or_else(|| var("LANG")).unwrap_or_default();
    // "pt_PT.UTF-8" → "PT".
    let country = locale.split(['.', '@']).next().and_then(|l| l.split_once('_')).map(|(_, c)| c.to_string()).filter(|c| c.len() == 2).unwrap_or_default();
    let mut r = Region::of_country(&country);
    r.zone = var("TZ")
        .map(|z| z.trim_start_matches(':').to_string())
        .or_else(|| std::fs::read_link("/etc/localtime").ok().and_then(|p| p.to_string_lossy().split("zoneinfo/").nth(1).map(str::to_string)))
        .or_else(|| std::fs::read_to_string("/etc/timezone").ok().map(|s| s.trim().to_string()))
        .unwrap_or_default();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cities_and_countries() {
        assert_eq!(zone_city("Europe/Lisbon", "PT"), "Lisbon, PT");
        assert_eq!(zone_city("America/Argentina/Buenos_Aires", "AR"), "Buenos Aires, AR");
        assert_eq!(zone_city("America/Sao_Paulo", ""), "Sao Paulo");
        assert_eq!(zone_city("UTC", "PT"), "");
        assert_eq!(zone_city("Etc/GMT+1", "PT"), "");
        let pt = Region::of_country("pt");
        assert!(pt.h24 && !pt.fahrenheit && pt.country == "PT");
        let us = Region::of_country("US");
        assert!(!us.h24 && us.fahrenheit);
        // Whatever the system says, it says it the same way each time.
        assert_eq!(region(), region());
        println!("{:?} → {:?}", region(), region().city());
    }
}
