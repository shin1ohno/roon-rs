//! Test Browse API against a real Roon Core.
//!
//! Usage:
//!   cargo run -p roon-api --example browse_test -- --host 192.168.1.20 --port 9330

use roon_api::{BrowseOptions, FileTokenStore, LoadOptions, RoonClientBuilder};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let host = args.iter().position(|a| a == "--host").map(|i| &args[i + 1]);
    let port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args[i + 1].parse().ok());

    let token_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("roon-rs")
        .join("tokens.json");

    let client = RoonClientBuilder::new(
        "com.roon-rs.browse_test",
        "roon-rs Browse Test",
        "0.1.0",
        "roon-rs",
        "dev@example.com",
    )
    .token_store(FileTokenStore::new(&token_path))
    .require_browse()
    .require_transport()
    .build()?;

    let core = match (host, port) {
        (Some(h), Some(p)) => client.connect(h, p).await?,
        _ => {
            println!("Usage: --host <ip> --port <port>");
            return Ok(());
        }
    };

    let browse = core.browse();

    // Step 1: Open the root browse hierarchy
    println!("=== Browse root ===\n");
    let result = browse
        .browse(BrowseOptions {
            hierarchy: Some("browse".into()),
            pop_all: Some(true),
            ..Default::default()
        })
        .await?;

    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items, level {})", list.title, list.count, list.level);
    }

    // Load items from root
    let items = browse
        .load(LoadOptions {
            hierarchy: Some("browse".into()),
            ..Default::default()
        })
        .await?;

    println!("\nRoot items ({}):", items.items.len());
    for item in &items.items {
        println!(
            "  {} {}{}",
            item.title,
            item.subtitle.as_deref().map(|s| format!("— {}", s)).unwrap_or_default(),
            item.hint.as_deref().map(|h| format!(" [{}]", h)).unwrap_or_default(),
        );
    }

    // Step 2: Navigate into the first item that looks like a list
    if let Some(first) = items.items.iter().find(|i| i.item_key.is_some()) {
        let key = first.item_key.as_ref().unwrap();
        println!("\n=== Browsing into: {} ===\n", first.title);

        let result = browse
            .browse(BrowseOptions {
                hierarchy: Some("browse".into()),
                item_key: Some(key.clone()),
                ..Default::default()
            })
            .await?;

        println!("Action: {}", result.action);
        if let Some(list) = &result.list {
            println!("List: {} ({} items, level {})", list.title, list.count, list.level);
        }

        let items = browse
            .load(LoadOptions {
                hierarchy: Some("browse".into()),
                count: Some(10),
                ..Default::default()
            })
            .await?;

        println!("\nItems (first {}):", items.items.len());
        for item in &items.items {
            println!(
                "  {} {}",
                item.title,
                item.subtitle.as_deref().map(|s| format!("— {}", s)).unwrap_or_default(),
            );
        }
    }

    // Step 3: Test search
    println!("\n=== Search: \"bach\" ===\n");
    let result = browse
        .browse(BrowseOptions {
            hierarchy: Some("search".into()),
            pop_all: Some(true),
            input: Some("bach".into()),
            ..Default::default()
        })
        .await?;

    println!("Action: {}", result.action);
    if let Some(list) = &result.list {
        println!("List: {} ({} items)", list.title, list.count);
    }

    let items = browse
        .load(LoadOptions {
            hierarchy: Some("search".into()),
            count: Some(10),
            ..Default::default()
        })
        .await?;

    println!("\nSearch results ({}):", items.items.len());
    for item in &items.items {
        println!(
            "  {} {}{}",
            item.title,
            item.subtitle.as_deref().map(|s| format!("— {}", s)).unwrap_or_default(),
            item.hint.as_deref().map(|h| format!(" [{}]", h)).unwrap_or_default(),
        );
    }

    println!("\nDone! Browse API working.");
    Ok(())
}
