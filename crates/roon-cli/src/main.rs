mod commands;
mod config;
mod connect;
mod output_format;
mod resolve;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "roon", about = "CLI for controlling Roon audio system")]
struct Cli {
    /// Roon Core host (overrides default)
    #[arg(long, global = true)]
    host: Option<String>,

    /// Roon Core port (overrides default)
    #[arg(long, global = true)]
    port: Option<u16>,

    /// Connection timeout in seconds (initial pairing requires approval in Roon app)
    #[arg(long, global = true, default_value = "300")]
    timeout: u64,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Discover Roon Cores and set default server
    Discover {
        /// Discovery scan duration in seconds
        #[arg(long, default_value = "5")]
        scan: u64,
    },

    /// Clear default server
    Disconnect,

    /// Select default zone
    Zone,

    /// Select default output (used by volume/mute when --output is omitted)
    Output,

    /// Show playback status of the default (or specified) zone
    Status {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// List all zones
    Zones,

    /// List all outputs
    Outputs,

    /// Play (resume or search-and-play with -a/-A)
    Play {
        /// Zone name (overrides default)
        #[arg(long)]
        zone: Option<String>,
        /// Zone ID (overrides name resolution)
        #[arg(long)]
        zone_id: Option<String>,
        /// Search by album name
        #[arg(short = 'a', long)]
        album: Option<String>,
        /// Search by artist name
        #[arg(short = 'A', long)]
        artist: Option<String>,
        /// Shuffle instead of playing in order
        #[arg(short = 's', long)]
        shuffle: bool,
    },

    /// Pause playback
    Pause {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Stop playback
    Stop {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Skip to next track
    Next {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Skip to previous track
    Previous {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Seek within track
    Seek {
        /// Seconds to seek to (or offset if --relative)
        seconds: i64,
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
        /// Use relative seek
        #[arg(long)]
        relative: bool,
    },

    /// Change volume
    Volume {
        /// Volume value
        value: f64,
        /// Output name
        #[arg(long)]
        output: Option<String>,
        /// Output ID
        #[arg(long)]
        output_id: Option<String>,
        /// Use relative volume
        #[arg(long)]
        relative: bool,
    },

    /// Mute or unmute an output
    Mute {
        /// on or off
        action: String,
        /// Output name
        #[arg(long)]
        output: Option<String>,
        /// Output ID
        #[arg(long)]
        output_id: Option<String>,
    },

    /// Pause all zones
    PauseAll,

    /// Transfer playback between zones
    Transfer {
        /// Source zone name
        #[arg(long)]
        from: String,
        /// Destination zone name
        #[arg(long)]
        to: String,
    },

    /// Change zone settings
    Settings {
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
        /// Shuffle on/off
        #[arg(long)]
        shuffle: Option<String>,
        /// Loop mode: loop, loop_one, off
        #[arg(long, name = "loop")]
        loop_mode: Option<String>,
        /// Auto radio on/off
        #[arg(long)]
        auto_radio: Option<String>,
    },

    /// Group outputs into a single zone
    Group {
        /// Comma-separated output names
        #[arg(long, value_delimiter = ',')]
        outputs: Vec<String>,
    },

    /// Ungroup outputs
    Ungroup {
        /// Comma-separated output names
        #[arg(long, value_delimiter = ',')]
        outputs: Vec<String>,
    },

    /// Browse Roon's music library (JSON, one invocation = one browse+load pair).
    Browse {
        /// Browse session key (default: "roon-cli-browse"). Pass the same key to
        /// play-item for follow-up actions on the same cursor.
        #[arg(long, default_value = "roon-cli-browse")]
        session: String,
        /// Hierarchy to browse: browse, playlists, albums, artists, genres,
        /// composers, internet_radio, settings. Omit to stay in the current
        /// hierarchy.
        #[arg(long)]
        hierarchy: Option<String>,
        /// Drill into an item by its item_key.
        #[arg(long)]
        item_key: Option<String>,
        /// Reset the session cursor to the hierarchy root.
        #[arg(long)]
        pop_all: bool,
        /// Pop N levels from the cursor.
        #[arg(long)]
        pop_levels: Option<u32>,
        /// Refresh the current list on the Core.
        #[arg(long)]
        refresh: bool,
        /// Pagination offset.
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Number of items to return.
        #[arg(long, default_value_t = 100)]
        count: u32,
        /// Fulfill an input_prompt item.
        #[arg(long)]
        input: Option<String>,
        /// Zone name (passed as zone_or_output_id — Roon needs this for actions).
        #[arg(long)]
        zone: Option<String>,
        /// Zone ID (overrides --zone).
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Search across Roon's library (thin wrapper over browse hierarchy=search).
    Search {
        /// Search text.
        #[arg(long)]
        input: String,
        /// Session key (default: "roon-cli-search").
        #[arg(long, default_value = "roon-cli-search")]
        session: String,
        /// Hierarchy (default: "search" — cross-category).
        #[arg(long, default_value = "search")]
        hierarchy: String,
        #[arg(long, default_value_t = 0)]
        offset: u32,
        #[arg(long, default_value_t = 50)]
        count: u32,
    },

    /// Play or queue an item from a browse/search session.
    PlayItem {
        /// Item key (opaque token returned by browse/search).
        #[arg(long)]
        item_key: String,
        /// Same session key used for the preceding browse/search.
        #[arg(long)]
        session: String,
        /// Action preference: auto (default), play-now, queue, start-radio.
        #[arg(long, default_value = "auto")]
        action: String,
        /// Zone name (resolved to zone_or_output_id). Defaults to the `roon zone`
        /// selection if omitted.
        #[arg(long)]
        zone: Option<String>,
        /// Zone ID (overrides --zone).
        #[arg(long)]
        zone_id: Option<String>,
    },

    /// Stream zone/output changes as NDJSON to stdout.
    Watch {
        /// Per-zone seek throttle in Hz (0 = every tick).
        #[arg(long, default_value_t = 1.0)]
        seek_hz: f64,
        /// Suppress the initial snapshot line.
        #[arg(long)]
        no_initial: bool,
    },

    /// Fetch an image from Roon Core
    Image {
        /// Image key
        image_key: String,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        /// Scale mode: fit, fill, stretch
        #[arg(long)]
        scale: Option<String>,
        /// Format: jpeg, png
        #[arg(long)]
        format: Option<String>,
        /// Output file path (stdout if omitted)
        #[arg(short = 'o', long)]
        output: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::WARN.into())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Discover { scan } => {
            commands::discover::discover(scan, cli.timeout).await?;
        }
        Command::Disconnect => {
            commands::discover::disconnect().await?;
        }
        Command::Zone => {
            commands::discover::select_zone(cli.host.as_deref(), cli.port, cli.timeout).await?;
        }
        Command::Output => {
            commands::discover::select_output(cli.host.as_deref(), cli.port, cli.timeout).await?;
        }

        // Commands that require a connection
        cmd => {
            let conn =
                connect::connect_from_config(cli.host.as_deref(), cli.port, cli.timeout).await?;
            let core = &conn.core;

            match cmd {
                Command::Status { zone, zone_id } => {
                    commands::transport::status(
                        core,
                        zone.as_deref(),
                        zone_id.as_deref(),
                        cli.json,
                    )
                    .await?;
                }
                Command::Zones => {
                    commands::transport::zones(core, cli.json).await?;
                }
                Command::Outputs => {
                    commands::transport::outputs(core, cli.json).await?;
                }
                Command::Play {
                    zone,
                    zone_id,
                    album,
                    artist,
                    shuffle,
                } => {
                    commands::transport::play(
                        core,
                        zone.as_deref(),
                        zone_id.as_deref(),
                        album.as_deref(),
                        artist.as_deref(),
                        shuffle,
                    )
                    .await?;
                }
                Command::Pause { zone, zone_id } => {
                    commands::transport::pause(core, zone.as_deref(), zone_id.as_deref()).await?;
                }
                Command::Stop { zone, zone_id } => {
                    commands::transport::stop(core, zone.as_deref(), zone_id.as_deref()).await?;
                }
                Command::Next { zone, zone_id } => {
                    commands::transport::next(core, zone.as_deref(), zone_id.as_deref()).await?;
                }
                Command::Previous { zone, zone_id } => {
                    commands::transport::previous(core, zone.as_deref(), zone_id.as_deref())
                        .await?;
                }
                Command::Seek {
                    seconds,
                    zone,
                    zone_id,
                    relative,
                } => {
                    commands::transport::seek(
                        core,
                        seconds,
                        relative,
                        zone.as_deref(),
                        zone_id.as_deref(),
                    )
                    .await?;
                }
                Command::Volume {
                    value,
                    output,
                    output_id,
                    relative,
                } => {
                    commands::transport::volume(
                        core,
                        value,
                        relative,
                        output.as_deref(),
                        output_id.as_deref(),
                    )
                    .await?;
                }
                Command::Mute {
                    action,
                    output,
                    output_id,
                } => {
                    let on = match action.as_str() {
                        "on" => true,
                        "off" => false,
                        _ => anyhow::bail!("mute action must be 'on' or 'off'"),
                    };
                    commands::transport::mute(core, on, output.as_deref(), output_id.as_deref())
                        .await?;
                }
                Command::PauseAll => {
                    commands::transport::pause_all(core).await?;
                }
                Command::Transfer { from, to } => {
                    commands::transport::transfer(core, &from, &to).await?;
                }
                Command::Settings {
                    zone,
                    zone_id,
                    shuffle,
                    loop_mode,
                    auto_radio,
                } => {
                    let shuffle_bool = shuffle.map(|s| s == "on");
                    let loop_str = loop_mode.as_ref().map(|l| match l.as_str() {
                        "off" => "disabled",
                        other => other,
                    });
                    let auto_radio_bool = auto_radio.map(|s| s == "on");
                    commands::transport::settings(
                        core,
                        zone.as_deref(),
                        zone_id.as_deref(),
                        shuffle_bool,
                        loop_str,
                        auto_radio_bool,
                    )
                    .await?;
                }
                Command::Group { outputs } => {
                    commands::transport::group(core, &outputs).await?;
                }
                Command::Ungroup { outputs } => {
                    commands::transport::ungroup(core, &outputs).await?;
                }
                Command::Browse {
                    session,
                    hierarchy,
                    item_key,
                    pop_all,
                    pop_levels,
                    refresh,
                    offset,
                    count,
                    input,
                    zone,
                    zone_id,
                } => {
                    let zone_or_output_id =
                        resolve_zone_or_output_id(core, zone.as_deref(), zone_id.as_deref())
                            .await?;
                    commands::browse::run(
                        core,
                        commands::browse::BrowseArgs {
                            session: &session,
                            hierarchy: hierarchy.as_deref(),
                            item_key: item_key.as_deref(),
                            pop_all,
                            pop_levels,
                            refresh,
                            offset,
                            count,
                            input: input.as_deref(),
                            zone_or_output_id: zone_or_output_id.as_deref(),
                        },
                    )
                    .await?;
                }
                Command::Search {
                    input,
                    session,
                    hierarchy,
                    offset,
                    count,
                } => {
                    commands::search::run(core, &input, &session, &hierarchy, offset, count)
                        .await?;
                }
                Command::PlayItem {
                    item_key,
                    session,
                    action,
                    zone,
                    zone_id,
                } => {
                    let zone_or_output_id =
                        resolve_zone_or_output_id(core, zone.as_deref(), zone_id.as_deref())
                            .await?;
                    commands::play_item::run(
                        core,
                        &item_key,
                        &session,
                        &action,
                        zone_or_output_id.as_deref(),
                    )
                    .await?;
                }
                Command::Watch {
                    seek_hz,
                    no_initial,
                } => {
                    commands::watch::run(core, seek_hz, no_initial).await?;
                }
                Command::Image {
                    image_key,
                    width,
                    height,
                    scale,
                    format,
                    output,
                } => {
                    commands::image::image(
                        core,
                        &image_key,
                        width,
                        height,
                        scale.as_deref(),
                        format.as_deref(),
                        output.as_deref(),
                    )
                    .await?;
                }
                // Already handled above
                Command::Discover { .. }
                | Command::Disconnect
                | Command::Zone
                | Command::Output => unreachable!(),
            }
        }
    }

    Ok(())
}

/// Resolve `--zone` / `--zone-id` (or the default zone from config) into a
/// zone_or_output_id string. Returns `None` when no hint was given AND no
/// default is configured — callers can then omit `zone_or_output_id` entirely.
async fn resolve_zone_or_output_id(
    core: &roon_api::Core,
    zone: Option<&str>,
    zone_id: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if let Some(id) = zone_id {
        return Ok(Some(id.to_string()));
    }
    if zone.is_some() || config::load().ok().and_then(|c| c.zone).is_some() {
        let zones = core.transport().get_zones().await?;
        return Ok(Some(resolve::get_zone_id(&zones, zone, zone_id)?));
    }
    Ok(None)
}
