//! Unit and currency conversion.
//!
//! Each unit maps to a category base unit by `base = (value + offset) * factor`
//! (the offset is only used for temperatures). Factors are exact definitions
//! where one exists, written as expressions (`5/18`, `180/pi`) and evaluated
//! with the calculator's own decimal engine.

use crate::expr::{eval, parse_str, LexMode};
use crate::num::{CalcError, Dec, PREC};

pub struct Unit {
    pub name: String,
    pub abbr: String,
    factor: Dec,
    offset: Dec,
}

pub struct Category {
    pub name: &'static str,
    pub units: Vec<Unit>,
    /// Default (from, to) unit indexes.
    pub defaults: (usize, usize),
}

impl Category {
    pub fn convert(&self, v: &Dec, from: usize, to: usize) -> Result<Dec, CalcError> {
        let (f, t) = (&self.units[from], &self.units[to]);
        if from == to {
            return Ok(v.clone());
        }
        let base = v.add(&f.offset).mul(&f.factor);
        let out = base.div(&t.factor)?.sub(&t.offset);
        // Remove representation noise beyond what the display can show.
        Ok(out.round_sig(PREC - 30))
    }

    pub fn find(&self, abbr: &str) -> Option<usize> {
        self.units.iter().position(|u| u.abbr == abbr)
    }
}

fn num(expr: &str) -> Dec {
    let node = parse_str(expr, LexMode::Decimal).unwrap_or_else(|_| panic!("bad factor {expr}"));
    eval::eval(&node, &eval::Ctx::default()).unwrap_or_else(|_| panic!("bad factor {expr}"))
}

fn units(list: &[(&str, &str, &str)]) -> Vec<Unit> {
    list.iter()
        .map(|(name, abbr, factor)| Unit {
            name: name.to_string(),
            abbr: abbr.to_string(),
            factor: num(factor),
            offset: Dec::zero(),
        })
        .collect()
}

fn category(name: &'static str, list: &[(&str, &str, &str)], from: &str, to: &str) -> Category {
    let units = units(list);
    let pos = |a: &str| units.iter().position(|u| u.abbr == a).expect("default unit");
    let defaults = (pos(from), pos(to));
    Category { name, units, defaults }
}

/// Builds the unit categories (everything except currency).
pub fn unit_categories() -> Vec<Category> {
    let mut cats = vec![
        category(
            "Volume",
            &[
                ("Milliliters", "mL", "1"),
                ("Cubic centimeters", "cm³", "1"),
                ("Liters", "L", "1000"),
                ("Cubic meters", "m³", "1e6"),
                ("Teaspoons (US)", "tsp", "4.92892159375"),
                ("Tablespoons (US)", "tbsp", "14.78676478125"),
                ("Fluid ounces (US)", "fl oz", "29.5735295625"),
                ("Cups (US)", "cup", "236.5882365"),
                ("Pints (US)", "pt", "473.176473"),
                ("Quarts (US)", "qt", "946.352946"),
                ("Gallons (US)", "gal", "3785.411784"),
                ("Cubic inches", "in³", "16.387064"),
                ("Cubic feet", "ft³", "28316.846592"),
                ("Cubic yards", "yd³", "764554.857984"),
                ("Teaspoons (UK)", "tsp (UK)", "5.91938802083333333333333333333"),
                ("Tablespoons (UK)", "tbsp (UK)", "17.758164062500"),
                ("Fluid ounces (UK)", "fl oz (UK)", "28.4130625"),
                ("Pints (UK)", "pt (UK)", "568.26125"),
                ("Quarts (UK)", "qt (UK)", "1136.5225"),
                ("Gallons (UK)", "gal (UK)", "4546.09"),
            ],
            "mL",
            "cup",
        ),
        category(
            "Length",
            &[
                ("Nanometers", "nm", "1e-9"),
                ("Microns", "µm", "1e-6"),
                ("Millimeters", "mm", "0.001"),
                ("Centimeters", "cm", "0.01"),
                ("Meters", "m", "1"),
                ("Kilometers", "km", "1000"),
                ("Inches", "in", "0.0254"),
                ("Feet", "ft", "0.3048"),
                ("Yards", "yd", "0.9144"),
                ("Miles", "mi", "1609.344"),
                ("Nautical miles", "NM", "1852"),
            ],
            "cm",
            "in",
        ),
        category(
            "Weight and mass",
            &[
                ("Carats", "ct", "0.0002"),
                ("Milligrams", "mg", "1e-6"),
                ("Centigrams", "cg", "1e-5"),
                ("Decigrams", "dg", "1e-4"),
                ("Grams", "g", "0.001"),
                ("Dekagrams", "dag", "0.01"),
                ("Hectograms", "hg", "0.1"),
                ("Kilograms", "kg", "1"),
                ("Metric tonnes", "t", "1000"),
                ("Ounces", "oz", "0.028349523125"),
                ("Pounds", "lb", "0.45359237"),
                ("Stone", "st", "6.35029318"),
                ("Short tons (US)", "ton (US)", "907.18474"),
                ("Long tons (UK)", "ton (UK)", "1016.0469088"),
            ],
            "kg",
            "lb",
        ),
        temperature(),
        category(
            "Energy",
            &[
                ("Electron volts", "eV", "1.602176634e-19"),
                ("Joules", "J", "1"),
                ("Kilojoules", "kJ", "1000"),
                ("Thermal calories", "cal", "4.184"),
                ("Food calories", "kcal", "4184"),
                ("Foot-pounds", "ft•lb", "1.3558179483314004"),
                ("British thermal units", "BTU", "1055.05585262"),
                ("Kilowatt-hours", "kWh", "3600000"),
            ],
            "J",
            "kcal",
        ),
        category(
            "Area",
            &[
                ("Square millimeters", "mm²", "1e-6"),
                ("Square centimeters", "cm²", "1e-4"),
                ("Square meters", "m²", "1"),
                ("Hectares", "ha", "10000"),
                ("Square kilometers", "km²", "1e6"),
                ("Square inches", "in²", "0.00064516"),
                ("Square feet", "ft²", "0.09290304"),
                ("Square yards", "yd²", "0.83612736"),
                ("Acres", "ac", "4046.8564224"),
                ("Square miles", "mi²", "2589988.110336"),
            ],
            "m²",
            "ft²",
        ),
        category(
            "Speed",
            &[
                ("Centimeters per second", "cm/s", "0.01"),
                ("Meters per second", "m/s", "1"),
                ("Kilometers per hour", "km/h", "5/18"),
                ("Feet per second", "ft/s", "0.3048"),
                ("Miles per hour", "mph", "0.44704"),
                ("Knots", "kn", "1852/3600"),
                ("Mach (sea level)", "M", "340.3"),
            ],
            "km/h",
            "mph",
        ),
        category(
            "Time",
            &[
                ("Microseconds", "µs", "1e-6"),
                ("Milliseconds", "ms", "0.001"),
                ("Seconds", "s", "1"),
                ("Minutes", "min", "60"),
                ("Hours", "h", "3600"),
                ("Days", "d", "86400"),
                ("Weeks", "wk", "604800"),
                ("Years (365 days)", "yr", "31536000"),
            ],
            "h",
            "min",
        ),
        category(
            "Power",
            &[
                ("Watts", "W", "1"),
                ("Kilowatts", "kW", "1000"),
                ("Horsepower (US)", "hp", "745.69987158227022"),
                ("Foot-pounds/minute", "ft•lb/min", "1.3558179483314004/60"),
                ("BTUs/minute", "BTU/min", "1055.05585262/60"),
            ],
            "kW",
            "hp",
        ),
        category(
            "Data",
            &[
                ("Bits", "b", "1"),
                ("Nibbles", "nibble", "4"),
                ("Bytes", "B", "8"),
                ("Kilobits", "Kb", "1e3"),
                ("Kibibits", "Kib", "2^10"),
                ("Kilobytes", "KB", "8e3"),
                ("Kibibytes", "KiB", "8*2^10"),
                ("Megabits", "Mb", "1e6"),
                ("Mebibits", "Mib", "2^20"),
                ("Megabytes", "MB", "8e6"),
                ("Mebibytes", "MiB", "8*2^20"),
                ("Gigabits", "Gb", "1e9"),
                ("Gibibits", "Gib", "2^30"),
                ("Gigabytes", "GB", "8e9"),
                ("Gibibytes", "GiB", "8*2^30"),
                ("Terabits", "Tb", "1e12"),
                ("Tebibits", "Tib", "2^40"),
                ("Terabytes", "TB", "8e12"),
                ("Tebibytes", "TiB", "8*2^40"),
                ("Petabits", "Pb", "1e15"),
                ("Petabytes", "PB", "8e15"),
                ("Pebibytes", "PiB", "8*2^50"),
            ],
            "GB",
            "MB",
        ),
        category(
            "Pressure",
            &[
                ("Atmospheres", "atm", "101325"),
                ("Bars", "bar", "100000"),
                ("Kilopascals", "kPa", "1000"),
                ("Millimeters of mercury", "mmHg", "133.322387415"),
                ("Pascals", "Pa", "1"),
                ("Pounds per square inch", "psi", "6894.757293168361"),
            ],
            "atm",
            "bar",
        ),
        category(
            "Angle",
            &[("Degrees", "deg", "1"), ("Radians", "rad", "180/pi"), ("Gradians", "grad", "0.9")],
            "deg",
            "rad",
        ),
    ];
    cats.shrink_to_fit();
    cats
}

fn temperature() -> Category {
    // Base unit: kelvin.
    let unit = |name: &str, abbr: &str, offset: &str, factor: &str| Unit {
        name: name.into(),
        abbr: abbr.into(),
        factor: num(factor),
        offset: num(offset),
    };
    Category {
        name: "Temperature",
        units: vec![
            unit("Celsius", "°C", "273.15", "1"),
            unit("Fahrenheit", "°F", "459.67", "5/9"),
            unit("Kelvin", "K", "0", "1"),
        ],
        defaults: (0, 1),
    }
}

// ---------------------------------------------------------------- currency

/// Bundled reference rates (units per 1 US dollar). These are an
/// approximate offline snapshot, not live market data.
pub const CURRENCY_SNAPSHOT: &str = "approximate 2025 reference rates";

const RATES: &[(&str, &str, &str)] = &[
    ("USD", "United States - Dollar", "1"),
    ("EUR", "Europe - Euro", "0.86"),
    ("GBP", "United Kingdom - Pound", "0.74"),
    ("JPY", "Japan - Yen", "147"),
    ("CNY", "China - Yuan", "7.18"),
    ("INR", "India - Rupee", "86"),
    ("CAD", "Canada - Dollar", "1.37"),
    ("AUD", "Australia - Dollar", "1.53"),
    ("NZD", "New Zealand - Dollar", "1.67"),
    ("CHF", "Switzerland - Franc", "0.80"),
    ("SEK", "Sweden - Krona", "9.55"),
    ("NOK", "Norway - Krone", "10.1"),
    ("DKK", "Denmark - Krone", "6.42"),
    ("PLN", "Poland - Zloty", "3.65"),
    ("CZK", "Czechia - Koruna", "21.2"),
    ("HUF", "Hungary - Forint", "342"),
    ("RON", "Romania - Leu", "4.35"),
    ("TRY", "Türkiye - Lira", "40.5"),
    ("BRL", "Brazil - Real", "5.45"),
    ("MXN", "Mexico - Peso", "18.7"),
    ("ZAR", "South Africa - Rand", "17.8"),
    ("KRW", "Korea - Won", "1385"),
    ("SGD", "Singapore - Dollar", "1.28"),
    ("HKD", "Hong Kong SAR - Dollar", "7.85"),
    ("TWD", "Taiwan - Dollar", "29.5"),
    ("THB", "Thailand - Baht", "32.4"),
    ("MYR", "Malaysia - Ringgit", "4.22"),
    ("IDR", "Indonesia - Rupiah", "16300"),
    ("PHP", "Philippines - Peso", "56.5"),
    ("AED", "United Arab Emirates - Dirham", "3.6725"),
    ("SAR", "Saudi Arabia - Riyal", "3.75"),
    ("ILS", "Israel - New Shekel", "3.37"),
];

pub struct Currency {
    pub category: Category,
    /// Where the rates came from (bundled snapshot or the user's file).
    pub source: String,
}

/// Builds the currency category from the bundled rates, overridden by a
/// user rates file if present. File format, one per line:
/// `CODE = units per USD [= optional name]`; `#` starts a comment.
pub fn currency(user_file: Option<&str>) -> Currency {
    let mut list: Vec<(String, String, Dec)> = RATES.iter().map(|(c, n, r)| (c.to_string(), n.to_string(), num(r))).collect();
    let mut source = format!("Offline: {CURRENCY_SNAPSHOT}");
    if let Some(text) = user_file {
        let mut applied = 0;
        let mut date = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(c) = line.strip_prefix('#') {
                if let Some(d) = c.trim().strip_prefix("date:") {
                    date = Some(d.trim().to_string());
                }
                continue;
            }
            let mut parts = line.splitn(3, '=').map(str::trim);
            let (Some(code), Some(rate)) = (parts.next(), parts.next()) else {
                continue;
            };
            let name = parts.next();
            let Ok(rate) = rate.parse::<Dec>() else { continue };
            if code.is_empty() || rate.is_zero() || rate.is_neg() {
                continue;
            }
            let code = code.to_ascii_uppercase();
            match list.iter_mut().find(|(c, _, _)| *c == code) {
                Some(e) => {
                    e.2 = rate;
                    if let Some(n) = name {
                        e.1 = n.to_string();
                    }
                }
                None => list.push((code.clone(), name.unwrap_or(&code).to_string(), rate)),
            }
            applied += 1;
        }
        if applied > 0 {
            source = match date {
                Some(d) => format!("Rates from your currency file ({d})"),
                None => "Rates from your currency file".into(),
            };
        }
    }
    let units: Vec<Unit> = list
        .into_iter()
        .map(|(code, name, rate)| Unit {
            name,
            abbr: code,
            // base (USD) = value / rate
            factor: Dec::one().div_p(&rate, PREC),
            offset: Dec::zero(),
        })
        .collect();
    let pos = |a: &str| units.iter().position(|u| u.abbr == a).unwrap_or(0);
    let defaults = (pos("USD"), pos("EUR"));
    Currency {
        category: Category {
            name: "Currency",
            units,
            defaults,
        },
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(cat: &str, v: &str, from: &str, to: &str) -> String {
        let cats = unit_categories();
        let c = cats.iter().find(|c| c.name == cat).unwrap();
        let r = c.convert(&v.parse().unwrap(), c.find(from).unwrap(), c.find(to).unwrap()).unwrap();
        r.format(16, false)
    }

    #[test]
    fn length_and_mass() {
        assert_eq!(conv("Length", "1", "in", "cm"), "2.54");
        assert_eq!(conv("Length", "1", "mi", "km"), "1.609344");
        assert_eq!(conv("Length", "100", "cm", "in"), "39.37007874015748");
        assert_eq!(conv("Weight and mass", "1", "lb", "kg"), "0.45359237");
        assert_eq!(conv("Weight and mass", "1", "kg", "lb"), "2.204622621848776");
    }

    #[test]
    fn temperature() {
        assert_eq!(conv("Temperature", "100", "°C", "°F"), "212");
        assert_eq!(conv("Temperature", "-40", "°F", "°C"), "-40");
        assert_eq!(conv("Temperature", "0", "K", "°C"), "-273.15");
        assert_eq!(conv("Temperature", "98.6", "°F", "°C"), "37");
    }

    #[test]
    fn other_categories() {
        assert_eq!(conv("Speed", "100", "km/h", "m/s"), "27.77777777777778");
        assert_eq!(conv("Data", "1", "GiB", "MiB"), "1024");
        assert_eq!(conv("Data", "1", "GB", "MB"), "1000");
        assert_eq!(conv("Angle", "180", "deg", "rad"), "3.141592653589793");
        assert_eq!(conv("Time", "1", "d", "min"), "1440");
        assert_eq!(conv("Pressure", "1", "atm", "bar"), "1.01325");
        assert_eq!(conv("Volume", "1", "gal", "L"), "3.785411784");
        assert_eq!(conv("Area", "1", "ac", "m²"), "4046.8564224");
        assert_eq!(conv("Energy", "1", "kWh", "J"), "3600000");
    }

    #[test]
    fn same_unit_is_identity() {
        assert_eq!(conv("Length", "123.456", "m", "m"), "123.456");
    }

    #[test]
    fn currency_override() {
        let c = currency(Some("# date: 2026-10-01\nEUR = 0.5\nXYZ = 4 = Test - Thing\nbad line\n"));
        assert!(c.source.contains("2026-10-01"));
        let cat = &c.category;
        let usd = cat.find("USD").unwrap();
        let r = cat.convert(&Dec::from_i64(10), usd, cat.find("EUR").unwrap()).unwrap();
        assert_eq!(r.format(16, false), "5");
        let xyz = cat.find("XYZ").unwrap();
        assert_eq!(cat.units[xyz].name, "Test - Thing");
        let r = cat.convert(&Dec::from_i64(8), xyz, cat.find("EUR").unwrap()).unwrap();
        assert_eq!(r.format(16, false), "1");
        let bundled = currency(None);
        assert!(bundled.source.starts_with("Offline"));
    }
}
