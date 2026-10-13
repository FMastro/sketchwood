//! Parsing and formatting lengths the way SketchUp's Measurements box does. Lengths are inches.
//!
//! Accepted: `1.5`, `1.5"`, `1.5in`, `1 1/2`, `1-1/2"`, `3/4`, `3'`, `3ft`, `3'6"`, `3' 6 1/2"`,
//! `38mm`, `3.8cm`, `1.2m`, and a leading `-` to flip direction. Bare numbers are inches.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseLengthError(String);

impl fmt::Display for ParseLengthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid length: \"{}\"", self.0)
    }
}

impl std::error::Error for ParseLengthError {}

/// Parse one length, in inches.
pub fn parse_length(input: &str) -> Result<f64, ParseLengthError> {
    let error = || ParseLengthError(input.trim().to_owned());
    let s = input.trim();
    let (sign, s) = match s.strip_prefix('-') {
        Some(rest) => (-1.0, rest.trim_start()),
        None => (1.0, s.strip_prefix('+').unwrap_or(s).trim_start()),
    };
    let inches = if let Some((feet, rest)) = s.split_once('\'') {
        let rest = rest.trim();
        let rest = rest.strip_suffix('"').or_else(|| rest.strip_suffix("in")).unwrap_or(rest);
        let inches = if rest.trim().is_empty() { 0.0 } else { parse_number(rest).ok_or_else(error)? };
        parse_number(feet).ok_or_else(error)? * 12.0 + inches
    } else {
        // Longest suffixes first, so "mm" isn't read as "m".
        const UNITS: [(&str, f64); 6] =
            [("mm", 1.0 / 25.4), ("cm", 1.0 / 2.54), ("in", 1.0), ("ft", 12.0), ("\"", 1.0), ("m", 1.0 / 0.0254)];
        let (number, scale) = UNITS
            .iter()
            .find_map(|(suffix, scale)| Some((s.strip_suffix(suffix)?, *scale)))
            .unwrap_or((s, 1.0));
        parse_number(number).ok_or_else(error)? * scale
    };
    Ok(sign * inches)
}

/// Parse a comma- or semicolon-separated list, like `1.5",3.5"`. Empty entries are `None`,
/// meaning "keep the current value" (`,3.5` changes only the second dimension).
pub fn parse_lengths(input: &str) -> Result<Vec<Option<f64>>, ParseLengthError> {
    input
        .split([',', ';'])
        .map(|part| if part.trim().is_empty() { Ok(None) } else { parse_length(part).map(Some) })
        .collect()
}

/// Format inches as fractional inches to the nearest 1/16", like `3 1/2"`. Rounded values get
/// SketchUp's `~` prefix.
pub fn format_length(inches: f64) -> String {
    const DENOMINATOR: i64 = 16;
    let sixteenths = (inches.abs() * DENOMINATOR as f64).round();
    let exact = (inches.abs() * DENOMINATOR as f64 - sixteenths).abs() < 1e-6;
    let sixteenths = sixteenths as i64;
    let (whole, mut num, mut den) = (sixteenths / DENOMINATOR, sixteenths % DENOMINATOR, DENOMINATOR);
    while num > 0 && num % 2 == 0 {
        num /= 2;
        den /= 2;
    }
    let magnitude = match (whole, num) {
        (w, 0) => format!("{w}\""),
        (0, n) => format!("{n}/{den}\""),
        (w, n) => format!("{w} {n}/{den}\""),
    };
    let sign = if inches < 0.0 && sixteenths > 0 { "-" } else { "" };
    let approx = if exact { "" } else { "~ " };
    format!("{approx}{sign}{magnitude}")
}

/// A plain number: decimal (`1.5`), fraction (`3/4`) or mixed (`1 1/2`, `1-1/2`).
fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let decimal = |t: &str| -> Option<f64> {
        let t = t.trim();
        let valid = !t.is_empty()
            && t != "."
            && t.chars().all(|c| c.is_ascii_digit() || c == '.')
            && t.matches('.').count() <= 1;
        if valid { t.parse().ok() } else { None }
    };
    let Some((numerator, denominator)) = s.rsplit_once('/') else { return decimal(s) };
    let (whole, numerator) = match numerator.trim().rsplit_once([' ', '-']) {
        Some((whole, n)) => (decimal(whole)?, n),
        None => (0.0, numerator),
    };
    let denominator = decimal(denominator)?;
    if denominator == 0.0 {
        return None;
    }
    Some(whole + decimal(numerator)? / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> f64 {
        parse_length(s).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn inches() {
        assert_eq!(parse("1.5"), 1.5);
        assert_eq!(parse("1.5\""), 1.5);
        assert_eq!(parse(" 3.5in "), 3.5);
        assert_eq!(parse(".75"), 0.75);
    }

    #[test]
    fn fractions() {
        assert_eq!(parse("3/4"), 0.75);
        assert_eq!(parse("1 1/2\""), 1.5);
        assert_eq!(parse("1-1/2"), 1.5);
        assert_eq!(parse("3 1/2"), 3.5);
    }

    #[test]
    fn feet_and_inches() {
        assert_eq!(parse("8'"), 96.0);
        assert_eq!(parse("8ft"), 96.0);
        assert_eq!(parse("3'6\""), 42.0);
        assert_eq!(parse("3' 6 1/2\""), 42.5);
        assert_eq!(parse("3'6"), 42.0);
    }

    #[test]
    fn metric() {
        assert!((parse("38mm") - 1.496_062_992).abs() < 1e-9);
        assert!((parse("2.54cm") - 1.0).abs() < 1e-12);
        assert!((parse("1m") - 39.370_078_74).abs() < 1e-6);
    }

    #[test]
    fn negative_flips() {
        assert_eq!(parse("-1.5\""), -1.5);
        assert_eq!(parse("-1 1/2"), -1.5);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "abc", "1..5", "1/0", ".", "inf", "NaN", "1.5x", "--2"] {
            assert!(parse_length(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn dimension_lists() {
        assert_eq!(parse_lengths("1.5\",3.5\"").unwrap(), [Some(1.5), Some(3.5)]);
        assert_eq!(parse_lengths("1 1/2; 3 1/2").unwrap(), [Some(1.5), Some(3.5)]);
        assert_eq!(parse_lengths(",3.5").unwrap(), [None, Some(3.5)]);
        assert!(parse_lengths("1.5,x").is_err());
    }

    #[test]
    fn formatting() {
        assert_eq!(format_length(1.5), "1 1/2\"");
        assert_eq!(format_length(3.5), "3 1/2\"");
        assert_eq!(format_length(96.0), "96\"");
        assert_eq!(format_length(0.0625), "1/16\"");
        assert_eq!(format_length(0.0), "0\"");
        assert_eq!(format_length(-0.75), "-3/4\"");
        assert_eq!(format_length(1.0 / 3.0), "~ 5/16\"");
    }
}
