use anyhow::Result;
use roon_api::{BrowseOptions, Core};

use crate::commands::browse;

pub async fn run(
    core: &Core,
    input: &str,
    session: &str,
    hierarchy: &str,
    offset: u32,
    count: u32,
) -> Result<()> {
    let opts = BrowseOptions {
        hierarchy: Some(hierarchy.to_string()),
        input: Some(input.to_string()),
        // Always reset the cursor before searching so consecutive keystroke
        // searches never see leftover state from a previous query.
        pop_all: Some(true),
        ..Default::default()
    };
    let resp = browse::browse_and_load(core, session, opts, offset, count).await?;
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}
