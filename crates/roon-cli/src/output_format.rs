use roon_api::{Output, Zone};
use serde::Serialize;

pub fn print_zones(zones: &[Zone], json: bool) {
    if json {
        print_json(zones);
        return;
    }
    if zones.is_empty() {
        println!("No zones found.");
        return;
    }
    for zone in zones {
        let state = format!("{:?}", zone.state);
        let now_playing = zone
            .now_playing
            .as_ref()
            .map(|np| np.one_line.line1.clone())
            .unwrap_or_default();

        if now_playing.is_empty() {
            println!("  {} ({})", zone.display_name, state);
        } else {
            println!("  {} ({}: {})", zone.display_name, state, now_playing);
        }
    }
}

pub fn print_outputs(outputs: &[Output], json: bool) {
    if json {
        print_json(outputs);
        return;
    }
    if outputs.is_empty() {
        println!("No outputs found.");
        return;
    }
    for output in outputs {
        let vol = output
            .volume
            .as_ref()
            .map(|v| {
                let muted = if v.is_muted == Some(true) {
                    " [muted]"
                } else {
                    ""
                };
                format!(" vol:{:.0}{}", v.value, muted)
            })
            .unwrap_or_default();
        println!(
            "  {} (zone: {}{})",
            output.display_name, output.zone_id, vol
        );
    }
}

fn print_json<T: Serialize + ?Sized>(value: &T) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}
