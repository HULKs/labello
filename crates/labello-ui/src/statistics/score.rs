pub(crate) fn exact(hundredths: i64) -> String {
    let magnitude = hundredths.unsigned_abs();
    let sign = if hundredths < 0 { "-" } else { "" };
    let text = format!("{sign}{}.{:02}", magnitude / 100, magnitude % 100);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

pub(crate) fn compact(hundredths: i64) -> String {
    let points = labello_domain::displayed_score(hundredths);
    let (scaled, suffix) = if points.abs() >= 999_500.0 {
        (points / 1_000_000.0, "m")
    } else if points.abs() >= 1_000.0 {
        (points / 1_000.0, "k")
    } else {
        return exact(hundredths);
    };
    let number = if scaled.abs() < 10.0 {
        let text = format!("{scaled:.1}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        format!("{scaled:.0}")
    };
    format!("{number}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_scores_are_compact_without_losing_exact_tooltip_values() {
        for (hundredths, expected) in [
            (0, "0"),
            (2_200, "22"),
            (2_025, "20.25"),
            (-1_050, "-10.5"),
            (150_000, "1.5k"),
            (4_507_129, "45k"),
            (29_203_216, "292k"),
            (99_999_999, "1m"),
            (100_000_000, "1m"),
            (150_000_000, "1.5m"),
            (-4_507_129, "-45k"),
        ] {
            assert_eq!(compact(hundredths), expected);
        }
        assert_eq!(exact(4_507_129), "45071.29");
        assert_eq!(exact(i64::MIN), "-92233720368547758.08");
    }
}
