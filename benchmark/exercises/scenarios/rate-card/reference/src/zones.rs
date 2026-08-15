//! Display names for the shipping zones operations currently sells.

/// Returns the operator-facing label for a zone code.
pub fn zone_label(zone: &str) -> &str {
    match zone {
        "domestic" => "Domestic ground",
        "international" => "International air",
        other => other,
    }
}
