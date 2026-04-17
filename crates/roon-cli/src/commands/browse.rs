use anyhow::Result;
use roon_api::{BrowseItem, BrowseList, BrowseOptions, Core, InputPrompt, LoadOptions};
use serde::{Deserialize, Serialize};

use crate::config::{self, SessionState};

pub struct BrowseArgs<'a> {
    pub session: &'a str,
    pub hierarchy: Option<&'a str>,
    pub item_key: Option<&'a str>,
    pub pop_all: bool,
    pub pop_levels: Option<u32>,
    pub refresh: bool,
    pub offset: u32,
    pub count: u32,
    pub input: Option<&'a str>,
    pub zone_or_output_id: Option<&'a str>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BrowseResponse {
    pub session: String,
    pub list: Option<ListView>,
    pub items: Vec<ItemView>,
    pub offset: u32,
    pub total: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListView {
    pub title: String,
    pub subtitle: Option<String>,
    pub level: u32,
    pub count: u32,
    pub hint: Option<String>,
    pub image_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ItemView {
    pub item_key: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub image_key: Option<String>,
    pub hint: Option<String>,
    pub input_prompt: Option<InputPromptView>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputPromptView {
    pub prompt: Option<String>,
    pub action: Option<String>,
    pub value: Option<String>,
    pub is_password: Option<bool>,
}

impl From<BrowseList> for ListView {
    fn from(l: BrowseList) -> Self {
        Self {
            title: l.title,
            subtitle: None,
            level: l.level,
            count: l.count,
            hint: l.hint,
            image_key: l.image_key,
        }
    }
}

impl From<BrowseItem> for ItemView {
    fn from(it: BrowseItem) -> Self {
        Self {
            item_key: it.item_key,
            title: it.title,
            subtitle: it.subtitle,
            image_key: it.image_key,
            hint: it.hint,
            input_prompt: it.input_prompt.map(Into::into),
        }
    }
}

impl From<InputPrompt> for InputPromptView {
    fn from(p: InputPrompt) -> Self {
        Self {
            prompt: p.prompt,
            action: p.action,
            value: p.value,
            is_password: p.is_password,
        }
    }
}

/// Run a single `browse` + `load` pair against the given session.
/// Returns the structured response so `search` / `play-item` can reuse it.
pub async fn browse_and_load(
    core: &Core,
    session: &str,
    opts: BrowseOptions,
    offset: u32,
    count: u32,
) -> Result<BrowseResponse> {
    let browse_svc = core.browse();

    let mut browse_opts = opts;
    browse_opts.multi_session_key = Some(session.to_string());

    // Roon requires `hierarchy` on every browse/load call, including drills.
    // Cache it per-session so subsequent CLI invocations (drill, play-item)
    // don't need to re-pass it.
    let saved = config::load_session(session);
    let hierarchy = match (browse_opts.hierarchy.as_ref(), saved.hierarchy.as_ref()) {
        (Some(h), _) => Some(h.clone()),
        (None, Some(h)) => Some(h.clone()),
        (None, None) => None,
    };
    browse_opts.hierarchy = hierarchy.clone();

    if let Some(h) = hierarchy.as_ref() {
        let _ = config::save_session(
            session,
            &SessionState {
                hierarchy: Some(h.clone()),
            },
        );
    }

    let _browse_result = browse_svc.browse(browse_opts).await?;

    let load_result = browse_svc
        .load(LoadOptions {
            multi_session_key: Some(session.to_string()),
            hierarchy,
            offset: Some(offset),
            count: Some(count),
            ..Default::default()
        })
        .await?;

    let list = load_result.list.clone();
    let total = list.as_ref().map(|l| l.count).unwrap_or(0);

    Ok(BrowseResponse {
        session: session.to_string(),
        list: list.map(Into::into),
        items: load_result.items.into_iter().map(Into::into).collect(),
        offset: load_result.offset,
        total,
    })
}

pub async fn run(core: &Core, args: BrowseArgs<'_>) -> Result<()> {
    let opts = BrowseOptions {
        hierarchy: args.hierarchy.map(str::to_string),
        item_key: args.item_key.map(str::to_string),
        pop_all: if args.pop_all { Some(true) } else { None },
        pop_levels: args.pop_levels,
        refresh_list: if args.refresh { Some(true) } else { None },
        input: args.input.map(str::to_string),
        zone_or_output_id: args.zone_or_output_id.map(str::to_string),
        ..Default::default()
    };

    let resp = browse_and_load(core, args.session, opts, args.offset, args.count).await?;
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_roundtrips_with_nulls() {
        let r = BrowseResponse {
            session: "s".into(),
            list: Some(ListView {
                title: "Albums".into(),
                subtitle: None,
                level: 1,
                count: 1287,
                hint: None,
                image_key: None,
            }),
            items: vec![ItemView {
                item_key: Some("k1".into()),
                title: "In Rainbows".into(),
                subtitle: Some("Radiohead".into()),
                image_key: None,
                hint: Some("list".into()),
                input_prompt: None,
            }],
            offset: 0,
            total: 1287,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: BrowseResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back.session, "s");
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0].title, "In Rainbows");
        assert_eq!(back.total, 1287);
    }

    #[test]
    fn input_prompt_view_roundtrips() {
        let r = BrowseResponse {
            session: "s".into(),
            list: None,
            items: vec![ItemView {
                item_key: None,
                title: "Search".into(),
                subtitle: None,
                image_key: None,
                hint: Some("action".into()),
                input_prompt: Some(InputPromptView {
                    prompt: Some("Search for...".into()),
                    action: Some("search".into()),
                    value: None,
                    is_password: Some(false),
                }),
            }],
            offset: 0,
            total: 0,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: BrowseResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(
            back.items[0]
                .input_prompt
                .as_ref()
                .unwrap()
                .prompt
                .as_deref(),
            Some("Search for...")
        );
    }
}
