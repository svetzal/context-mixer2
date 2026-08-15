pub fn zone_label(zone: &str) -> &str {
    match zone {
        "domestic" => "Domestic ground",
        "international" => "International air",
        other => other,
    }
}
