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

    /// Connection timeout in seconds
    #[arg(long, global = true, default_value = "10")]
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
    Discover,

    /// Clear default server
    Disconnect,

    /// Select default zone
    Zone,

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

    /// Browse Roon's music library
    Browse {
        #[arg(long)]
        hierarchy: Option<String>,
        #[arg(long)]
        zone: Option<String>,
        #[arg(long)]
        zone_id: Option<String>,
        #[arg(long)]
        item_key: Option<String>,
        #[arg(long)]
        input: Option<String>,
        #[arg(long)]
        pop_all: bool,
    },

    /// Load items from current browse list
    Load {
        #[arg(long)]
        hierarchy: Option<String>,
        #[arg(long)]
        offset: Option<u32>,
        #[arg(long)]
        count: Option<u32>,
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
        Command::Discover => {
            commands::discover::discover(cli.timeout).await?;
        }
        Command::Disconnect => {
            commands::discover::disconnect().await?;
        }
        Command::Zone => {
            commands::discover::select_zone(
                cli.host.as_deref(),
                cli.port,
                cli.timeout,
            )
            .await?;
        }

        // Commands that require a connection
        cmd => {
            let conn = connect::connect_from_config(
                cli.host.as_deref(),
                cli.port,
                cli.timeout,
            )
            .await?;
            let core = &conn.core;

            match cmd {
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
                } => {
                    commands::transport::play(
                        core,
                        zone.as_deref(),
                        zone_id.as_deref(),
                        album.as_deref(),
                        artist.as_deref(),
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
                    hierarchy,
                    zone,
                    zone_id,
                    item_key,
                    input,
                    pop_all,
                } => {
                    commands::browse::browse(
                        core,
                        commands::browse::BrowseArgs {
                            hierarchy: hierarchy.as_deref(),
                            zone_name: zone.as_deref(),
                            zone_id: zone_id.as_deref(),
                            item_key: item_key.as_deref(),
                            input: input.as_deref(),
                            pop_all,
                            json: cli.json,
                        },
                    )
                    .await?;
                }
                Command::Load {
                    hierarchy,
                    offset,
                    count,
                } => {
                    commands::browse::load(
                        core,
                        hierarchy.as_deref(),
                        offset,
                        count,
                        cli.json,
                    )
                    .await?;
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
                Command::Discover | Command::Disconnect | Command::Zone => unreachable!(),
            }
        }
    }

    Ok(())
}
