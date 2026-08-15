//! Parsing a rate card. Text in, bands out, no I/O.

use crate::error::QuoteError;

/// One priced weight band within a zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    /// The heaviest parcel this band covers.
    pub max_grams: u32,
    /// The band price before surcharges.
    pub cents: u64,
}

/// Every zone's bands, in ascending weight order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateCard {
    entries: Vec<(String, Band)>,
}

impl RateCard {
    /// Reads a tab-separated rate card, ignoring blanks and `#` comments.
    ///
    /// # Errors
    ///
    /// Returns [`QuoteError::MalformedRateCard`] with the 1-based number of the
    /// first line that is not three tab-separated fields with numeric weight
    /// and price.
    ///
    /// ```
    /// # use ratecard::RateCard;
    /// let card = RateCard::parse("domestic\t500\t599\n")?;
    /// assert_eq!(card.bands("domestic").len(), 1);
    /// # Ok::<(), ratecard::QuoteError>(())
    /// ```
    pub fn parse(contents: &str) -> Result<Self, QuoteError> {
        let mut entries = Vec::new();
        for (index, line) in contents.lines().enumerate() {
            let number = index + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            entries.push(parse_line(line, number)?);
        }
        entries.sort_by_key(|(zone, band)| (zone.clone(), band.max_grams));
        Ok(Self { entries })
    }

    /// Returns the bands quoted for a zone, lightest first.
    pub fn bands(&self, zone: &str) -> Vec<&Band> {
        self.entries
            .iter()
            .filter(|(name, _)| name == zone)
            .map(|(_, band)| band)
            .collect()
    }
}

fn parse_line(line: &str, number: usize) -> Result<(String, Band), QuoteError> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [zone, max_grams, cents] = fields.as_slice() else {
        return Err(QuoteError::MalformedRateCard { line: number });
    };
    let max_grams = max_grams
        .trim()
        .parse::<u32>()
        .map_err(|_| QuoteError::MalformedRateCard { line: number })?;
    let cents = cents
        .trim()
        .parse::<u64>()
        .map_err(|_| QuoteError::MalformedRateCard { line: number })?;
    Ok((zone.trim().to_string(), Band { max_grams, cents }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_comments_and_blank_lines() {
        let card = RateCard::parse("# header\n\ndomestic\t500\t599\n").unwrap();

        assert_eq!(card.bands("domestic").len(), 1);
    }

    #[test]
    fn counts_every_line_when_reporting_a_bad_one() {
        let error = RateCard::parse("# header\ndomestic\t500\t599\n\nbad\tx\t1\n").unwrap_err();

        assert_eq!(error, QuoteError::MalformedRateCard { line: 4 });
    }

    #[test]
    fn rejects_a_line_without_three_fields() {
        let error = RateCard::parse("domestic\t500\n").unwrap_err();

        assert_eq!(error, QuoteError::MalformedRateCard { line: 1 });
    }

    #[test]
    fn orders_bands_by_weight() {
        let card = RateCard::parse("domestic\t2000\t899\ndomestic\t500\t599\n").unwrap();

        let weights: Vec<u32> = card.bands("domestic").iter().map(|band| band.max_grams).collect();
        assert_eq!(weights, vec![500, 2000]);
    }
}
