//! The values a caller hands in and gets back.

/// A parcel awaiting a price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parcel {
    /// Billable weight in grams.
    pub weight_grams: u32,
    /// Zone code as it appears in the rate card.
    pub zone: String,
}

/// What a parcel costs to ship, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    /// The matching band's price before surcharges.
    pub base_cents: u64,
    /// Everything added on top of the band price.
    pub surcharge_cents: u64,
    /// The amount to bill.
    pub total_cents: u64,
    /// The upper bound of the band the parcel fell into.
    pub band_max_grams: u32,
}
