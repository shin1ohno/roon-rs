use anyhow::{Result, bail};
use roon_api::{BrowseOptions, Core};
use serde_json::json;

use crate::commands::browse::{self, ItemView};

/// Which playback action the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Auto,
    PlayNow,
    Queue,
    StartRadio,
}

impl Action {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(Self::Auto),
            "play-now" | "play_now" => Ok(Self::PlayNow),
            "queue" => Ok(Self::Queue),
            "start-radio" | "start_radio" => Ok(Self::StartRadio),
            other => bail!(
                "unknown action '{}': expected auto|play-now|queue|start-radio",
                other
            ),
        }
    }
}

/// Pure function: given the action list, pick the item to invoke.
/// Returns `Err` with the list of available titles when no match is found.
pub fn select_action(items: &[ItemView], action: Action) -> Result<&ItemView, Vec<String>> {
    let available: Vec<String> = items.iter().map(|i| i.title.clone()).collect();

    let candidates: &[&str] = match action {
        Action::Auto => &["Play Now", "Start Radio", "Queue"],
        Action::PlayNow => &["Play Now"],
        Action::Queue => &["Queue"],
        Action::StartRadio => &["Start Radio"],
    };

    for wanted in candidates {
        if let Some(it) = items
            .iter()
            .find(|i| i.title.eq_ignore_ascii_case(wanted) && i.item_key.is_some())
        {
            return Ok(it);
        }
    }
    Err(available)
}

pub async fn run(
    core: &Core,
    item_key: &str,
    session: &str,
    action: &str,
    zone_or_output_id: Option<&str>,
) -> Result<()> {
    let action = Action::parse(action)?;

    // 1. Drill into the item — its children are the action list.
    let opts = BrowseOptions {
        item_key: Some(item_key.to_string()),
        zone_or_output_id: zone_or_output_id.map(str::to_string),
        ..Default::default()
    };
    let resp = browse::browse_and_load(core, session, opts, 0, 100).await?;

    // 2. Pick the action.
    let chosen = match select_action(&resp.items, action) {
        Ok(it) => it,
        Err(available) => {
            let err = json!({
                "error": "no matching action",
                "available": available,
            });
            println!("{}", err);
            std::process::exit(3);
        }
    };

    let chosen_key = chosen.item_key.clone().unwrap();
    let chosen_title = chosen.title.clone();

    // 3. Invoke by drilling into the chosen action item — this is what actually
    //    triggers playback on the target zone.
    let exec_opts = BrowseOptions {
        item_key: Some(chosen_key.clone()),
        zone_or_output_id: zone_or_output_id.map(str::to_string),
        ..Default::default()
    };
    let _ = browse::browse_and_load(core, session, exec_opts, 0, 1).await?;

    let out = json!({
        "ok": true,
        "played": { "title": chosen_title, "item_key": chosen_key },
    });
    println!("{}", out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, key: Option<&str>) -> ItemView {
        ItemView {
            item_key: key.map(str::to_string),
            title: title.into(),
            subtitle: None,
            image_key: None,
            hint: Some("action".into()),
            input_prompt: None,
        }
    }

    #[test]
    fn auto_prefers_play_now() {
        let items = vec![
            item("Queue", Some("q")),
            item("Play Now", Some("p")),
            item("Start Radio", Some("r")),
        ];
        let chosen = select_action(&items, Action::Auto).unwrap();
        assert_eq!(chosen.title, "Play Now");
    }

    #[test]
    fn auto_falls_back_to_start_radio_when_play_now_missing() {
        let items = vec![item("Queue", Some("q")), item("Start Radio", Some("r"))];
        let chosen = select_action(&items, Action::Auto).unwrap();
        assert_eq!(chosen.title, "Start Radio");
    }

    #[test]
    fn auto_falls_back_to_queue_when_others_missing() {
        let items = vec![item("Queue", Some("q"))];
        let chosen = select_action(&items, Action::Auto).unwrap();
        assert_eq!(chosen.title, "Queue");
    }

    #[test]
    fn specific_action_matches_exactly() {
        let items = vec![item("Play Now", Some("p")), item("Queue", Some("q"))];
        let chosen = select_action(&items, Action::Queue).unwrap();
        assert_eq!(chosen.title, "Queue");
    }

    #[test]
    fn specific_action_is_case_insensitive() {
        let items = vec![item("PLAY NOW", Some("p"))];
        let chosen = select_action(&items, Action::PlayNow).unwrap();
        assert_eq!(chosen.title, "PLAY NOW");
    }

    #[test]
    fn skips_items_without_key() {
        let items = vec![item("Play Now", None), item("Queue", Some("q"))];
        let chosen = select_action(&items, Action::Auto).unwrap();
        assert_eq!(chosen.title, "Queue");
    }

    #[test]
    fn no_match_returns_available_titles() {
        let items = vec![item("Go Back", Some("b"))];
        let err = select_action(&items, Action::Auto).unwrap_err();
        assert_eq!(err, vec!["Go Back".to_string()]);
    }

    #[test]
    fn action_parse_variants() {
        assert_eq!(Action::parse("auto").unwrap(), Action::Auto);
        assert_eq!(Action::parse("play-now").unwrap(), Action::PlayNow);
        assert_eq!(Action::parse("play_now").unwrap(), Action::PlayNow);
        assert_eq!(Action::parse("queue").unwrap(), Action::Queue);
        assert_eq!(Action::parse("start-radio").unwrap(), Action::StartRadio);
        assert_eq!(Action::parse("start_radio").unwrap(), Action::StartRadio);
        assert!(Action::parse("nope").is_err());
    }
}
