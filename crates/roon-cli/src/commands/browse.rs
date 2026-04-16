use anyhow::Result;
use roon_api::{BrowseOptions, Core, LoadOptions};

use crate::output_format;
use crate::resolve;

pub struct BrowseArgs<'a> {
    pub hierarchy: Option<&'a str>,
    pub zone_name: Option<&'a str>,
    pub zone_id: Option<&'a str>,
    pub item_key: Option<&'a str>,
    pub input: Option<&'a str>,
    pub pop_all: bool,
    pub json: bool,
}

pub async fn browse(core: &Core, args: BrowseArgs<'_>) -> Result<()> {
    let browse_svc = core.browse();

    let zone_or_output_id = if args.zone_name.is_some() || args.zone_id.is_some() {
        let transport = core.transport();
        let zones = transport.get_zones().await?;
        Some(resolve::get_zone_id(&zones, args.zone_name, args.zone_id)?)
    } else {
        let cfg = crate::config::load()?;
        if let Some(zc) = &cfg.zone {
            let transport = core.transport();
            let zones = transport.get_zones().await?;
            resolve::resolve_zone(&zones, &zc.name)
                .ok()
                .map(|z| z.zone_id.clone())
        } else {
            None
        }
    };

    let result = browse_svc
        .browse(BrowseOptions {
            hierarchy: args.hierarchy.map(|s| s.to_string()),
            item_key: args.item_key.map(|s| s.to_string()),
            input: args.input.map(|s| s.to_string()),
            pop_all: if args.pop_all { Some(true) } else { None },
            zone_or_output_id,
            ..Default::default()
        })
        .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Action: {}", result.action);
        if let Some(list) = &result.list {
            println!(
                "{} ({} items, level {})",
                list.title, list.count, list.level
            );
        }
        if let Some(item) = &result.item {
            println!("Item: {}", item.title);
        }
    }

    Ok(())
}

pub async fn load(
    core: &Core,
    hierarchy: Option<&str>,
    offset: Option<u32>,
    count: Option<u32>,
    json: bool,
) -> Result<()> {
    let browse_svc = core.browse();

    let result = browse_svc
        .load(LoadOptions {
            hierarchy: hierarchy.map(|s| s.to_string()),
            offset,
            count,
            ..Default::default()
        })
        .await?;

    output_format::print_browse_items(result.list.as_ref(), &result.items, json);
    Ok(())
}
