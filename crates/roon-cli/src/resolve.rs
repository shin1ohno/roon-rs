use anyhow::{bail, Result};
use roon_api::{Output, Zone};

use crate::config;

/// Resolve a zone by name from the zone list.
/// Uses case-insensitive substring matching.
pub fn resolve_zone<'a>(zones: &'a [Zone], name: &str) -> Result<&'a Zone> {
    let lower = name.to_lowercase();
    let matches: Vec<&Zone> = zones
        .iter()
        .filter(|z| z.display_name.to_lowercase().contains(&lower))
        .collect();

    match matches.len() {
        0 => bail!("No zone matching '{}'. Available: {}", name, zone_names(zones)),
        1 => Ok(matches[0]),
        _ => bail!(
            "Ambiguous zone '{}'. Matches: {}",
            name,
            matches.iter().map(|z| z.display_name.as_str()).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Resolve an output by name from the output list.
pub fn resolve_output<'a>(outputs: &'a [Output], name: &str) -> Result<&'a Output> {
    let lower = name.to_lowercase();
    let matches: Vec<&Output> = outputs
        .iter()
        .filter(|o| o.display_name.to_lowercase().contains(&lower))
        .collect();

    match matches.len() {
        0 => bail!(
            "No output matching '{}'. Available: {}",
            name,
            outputs.iter().map(|o| o.display_name.as_str()).collect::<Vec<_>>().join(", ")
        ),
        1 => Ok(matches[0]),
        _ => bail!(
            "Ambiguous output '{}'. Matches: {}",
            name,
            matches.iter().map(|o| o.display_name.as_str()).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Get zone_id from --zone/--zone-id flags or the default zone config.
pub fn get_zone_id(
    zones: &[Zone],
    zone_name: Option<&str>,
    zone_id: Option<&str>,
) -> Result<String> {
    if let Some(id) = zone_id {
        return Ok(id.to_string());
    }
    if let Some(name) = zone_name {
        return Ok(resolve_zone(zones, name)?.zone_id.clone());
    }
    // Try default zone from config
    let cfg = config::load()?;
    if let Some(zc) = &cfg.zone {
        return Ok(resolve_zone(zones, &zc.name)?.zone_id.clone());
    }
    bail!("No default zone. Run `roon zone` first, or use --zone.")
}

/// Get output_id from --output/--output-id flags.
pub fn get_output_id(
    outputs: &[Output],
    output_name: Option<&str>,
    output_id: Option<&str>,
) -> Result<String> {
    if let Some(id) = output_id {
        return Ok(id.to_string());
    }
    if let Some(name) = output_name {
        return Ok(resolve_output(outputs, name)?.output_id.clone());
    }
    bail!("--output or --output-id is required.")
}

fn zone_names(zones: &[Zone]) -> String {
    zones
        .iter()
        .map(|z| z.display_name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
