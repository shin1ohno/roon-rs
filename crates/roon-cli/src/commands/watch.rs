use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use roon_api::{Core, Output, OutputEvent, Zone, ZoneEvent, ZoneSeek};
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::signal;

/// Envelope written to stdout, one JSON object per line.
#[derive(Debug, Serialize)]
struct Envelope<'a> {
    schema: u32,
    ts: String,
    #[serde(flatten)]
    event: &'a OutEvent,
}

/// NDJSON event variants. The `event` tag matches the wire contract.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum OutEvent {
    Initial {
        zones: Vec<Zone>,
        outputs: Vec<Output>,
    },
    ZoneAdded {
        zone: Zone,
    },
    ZoneChanged {
        zone: Zone,
    },
    ZoneRemoved {
        zone_id: String,
    },
    ZoneSeeked {
        zone_id: String,
        seek_position: Option<f64>,
        queue_time_remaining: Option<f64>,
    },
    OutputAdded {
        output: Output,
    },
    OutputChanged {
        output: Output,
    },
    OutputRemoved {
        output_id: String,
    },
    #[allow(dead_code)] // reserved for in-band non-fatal errors
    Error {
        message: String,
    },
}

/// Per-zone seek throttle. `None` min_gap ⇒ never throttle.
struct SeekThrottle {
    min_gap: Option<Duration>,
    last: HashMap<String, Instant>,
}

impl SeekThrottle {
    fn new(rate_hz: f64) -> Self {
        let min_gap = if rate_hz > 0.0 {
            Some(Duration::from_secs_f64(1.0 / rate_hz))
        } else {
            None
        };
        Self {
            min_gap,
            last: HashMap::new(),
        }
    }

    fn admit(&mut self, zone_id: &str, now: Instant) -> bool {
        let Some(gap) = self.min_gap else {
            return true;
        };
        let ok = self
            .last
            .get(zone_id)
            .is_none_or(|prev| now.duration_since(*prev) >= gap);
        if ok {
            self.last.insert(zone_id.to_string(), now);
        }
        ok
    }
}

async fn emit<W: AsyncWrite + Unpin>(w: &mut W, ev: &OutEvent) -> io::Result<()> {
    let ts = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into());
    let env = Envelope {
        schema: 1,
        ts,
        event: ev,
    };
    let mut buf = serde_json::to_vec(&env).map_err(io::Error::other)?;
    buf.push(b'\n');
    w.write_all(&buf).await?;
    w.flush().await
}

fn seek_event(seek: ZoneSeek) -> OutEvent {
    OutEvent::ZoneSeeked {
        zone_id: seek.zone_id,
        seek_position: seek.seek_position,
        queue_time_remaining: seek.queue_time_remaining,
    }
}

pub async fn run(core: &Core, seek_hz: f64, no_initial: bool) -> Result<()> {
    let transport = core.transport();
    let mut zone_rx = transport.subscribe_zones().await?;
    let mut output_rx = transport.subscribe_outputs().await?;

    let mut stdout = tokio::io::stdout();
    let mut throttle = SeekThrottle::new(seek_hz);

    let mut initial_zones: Option<Vec<Zone>> = None;
    let mut initial_outputs: Option<Vec<Output>> = None;
    let mut did_initial = false;
    let mut buffered: VecDeque<OutEvent> = VecDeque::new();

    loop {
        let step = tokio::select! {
            _ = signal::ctrl_c() => return Ok(()),
            maybe_z = zone_rx.recv() => {
                let Some(ev) = maybe_z else { return Ok(()); };
                handle_zone_event(
                    ev,
                    &mut initial_zones,
                    &mut initial_outputs,
                    &mut did_initial,
                    &mut buffered,
                    &mut throttle,
                    no_initial,
                    &mut stdout,
                ).await
            }
            maybe_o = output_rx.recv() => {
                let Some(ev) = maybe_o else { return Ok(()); };
                handle_output_event(
                    ev,
                    &mut initial_zones,
                    &mut initial_outputs,
                    &mut did_initial,
                    &mut buffered,
                    no_initial,
                    &mut stdout,
                ).await
            }
        };
        if let Err(err) = step {
            if is_broken_pipe(&err) {
                return Ok(());
            }
            return Err(err);
        }
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
}

#[allow(clippy::too_many_arguments)]
async fn handle_zone_event<W: AsyncWrite + Unpin>(
    ev: ZoneEvent,
    initial_zones: &mut Option<Vec<Zone>>,
    initial_outputs: &mut Option<Vec<Output>>,
    did_initial: &mut bool,
    buffered: &mut VecDeque<OutEvent>,
    throttle: &mut SeekThrottle,
    no_initial: bool,
    stdout: &mut W,
) -> Result<()> {
    match ev {
        ZoneEvent::Initial(zs) => {
            *initial_zones = Some(zs);
            maybe_flush_initial(
                initial_zones,
                initial_outputs,
                did_initial,
                buffered,
                no_initial,
                stdout,
            )
            .await?;
        }
        ZoneEvent::Added(zs) => {
            for z in zs {
                push_or_emit(
                    OutEvent::ZoneAdded { zone: z },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
        ZoneEvent::Changed(zs) => {
            for z in zs {
                push_or_emit(
                    OutEvent::ZoneChanged { zone: z },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
        ZoneEvent::Removed(ids) => {
            for id in ids {
                push_or_emit(
                    OutEvent::ZoneRemoved { zone_id: id },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
        ZoneEvent::Seeked(seeks) => {
            let now = Instant::now();
            for s in seeks {
                if !throttle.admit(&s.zone_id, now) {
                    continue;
                }
                push_or_emit(seek_event(s), *did_initial, buffered, stdout).await?;
            }
        }
    }
    Ok(())
}

async fn handle_output_event<W: AsyncWrite + Unpin>(
    ev: OutputEvent,
    initial_zones: &mut Option<Vec<Zone>>,
    initial_outputs: &mut Option<Vec<Output>>,
    did_initial: &mut bool,
    buffered: &mut VecDeque<OutEvent>,
    no_initial: bool,
    stdout: &mut W,
) -> Result<()> {
    match ev {
        OutputEvent::Initial(os) => {
            *initial_outputs = Some(os);
            maybe_flush_initial(
                initial_zones,
                initial_outputs,
                did_initial,
                buffered,
                no_initial,
                stdout,
            )
            .await?;
        }
        OutputEvent::Added(os) => {
            for o in os {
                push_or_emit(
                    OutEvent::OutputAdded { output: o },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
        OutputEvent::Changed(os) => {
            for o in os {
                push_or_emit(
                    OutEvent::OutputChanged { output: o },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
        OutputEvent::Removed(ids) => {
            for id in ids {
                push_or_emit(
                    OutEvent::OutputRemoved { output_id: id },
                    *did_initial,
                    buffered,
                    stdout,
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn push_or_emit<W: AsyncWrite + Unpin>(
    ev: OutEvent,
    did_initial: bool,
    buffered: &mut VecDeque<OutEvent>,
    stdout: &mut W,
) -> Result<()> {
    if did_initial {
        emit(stdout, &ev).await?;
    } else {
        buffered.push_back(ev);
    }
    Ok(())
}

async fn maybe_flush_initial<W: AsyncWrite + Unpin>(
    initial_zones: &mut Option<Vec<Zone>>,
    initial_outputs: &mut Option<Vec<Output>>,
    did_initial: &mut bool,
    buffered: &mut VecDeque<OutEvent>,
    no_initial: bool,
    stdout: &mut W,
) -> Result<()> {
    if *did_initial {
        return Ok(());
    }
    let (Some(zs), Some(os)) = (initial_zones.as_ref(), initial_outputs.as_ref()) else {
        return Ok(());
    };
    if !no_initial {
        let ev = OutEvent::Initial {
            zones: zs.clone(),
            outputs: os.clone(),
        };
        emit(stdout, &ev).await?;
    }
    *did_initial = true;
    while let Some(ev) = buffered.pop_front() {
        emit(stdout, &ev).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn throttle_rate_zero_admits_always() {
        let mut t = SeekThrottle::new(0.0);
        let now = Instant::now();
        assert!(t.admit("z1", now));
        assert!(t.admit("z1", now));
        assert!(t.admit("z1", now + Duration::from_millis(1)));
    }

    #[test]
    fn throttle_rate_two_hz_respects_gap() {
        let mut t = SeekThrottle::new(2.0); // min_gap = 500 ms
        let t0 = Instant::now();
        assert!(t.admit("z1", t0)); // first sample always admitted
        assert!(!t.admit("z1", t0 + Duration::from_millis(400)));
        assert!(t.admit("z1", t0 + Duration::from_millis(600)));
    }

    #[test]
    fn throttle_is_per_zone() {
        let mut t = SeekThrottle::new(2.0);
        let t0 = Instant::now();
        assert!(t.admit("z1", t0));
        assert!(t.admit("z2", t0 + Duration::from_millis(100)));
        assert!(!t.admit("z1", t0 + Duration::from_millis(200)));
    }

    fn envelope_of(ev: &OutEvent) -> Value {
        let env = Envelope {
            schema: 1,
            ts: "2026-01-01T00:00:00Z".to_string(),
            event: ev,
        };
        let s = serde_json::to_string(&env).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    #[test]
    fn zone_seeked_serializes_with_flat_fields() {
        let ev = OutEvent::ZoneSeeked {
            zone_id: "abc".into(),
            seek_position: Some(37.5),
            queue_time_remaining: Some(152.0),
        };
        let v = envelope_of(&ev);
        assert_eq!(v["schema"], 1);
        assert_eq!(v["event"], "zone_seeked");
        assert_eq!(v["zone_id"], "abc");
        assert_eq!(v["seek_position"], 37.5);
        assert_eq!(v["queue_time_remaining"], 152.0);
        assert_eq!(v["ts"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn zone_removed_has_zone_id_only() {
        let ev = OutEvent::ZoneRemoved {
            zone_id: "xyz".into(),
        };
        let v = envelope_of(&ev);
        assert_eq!(v["event"], "zone_removed");
        assert_eq!(v["zone_id"], "xyz");
        assert!(v.get("zone").is_none());
    }

    #[test]
    fn error_variant_serializes() {
        let ev = OutEvent::Error {
            message: "oops".into(),
        };
        let v = envelope_of(&ev);
        assert_eq!(v["event"], "error");
        assert_eq!(v["message"], "oops");
    }

    /// Fixture: drive the buffering helpers with a scripted sequence of events
    /// and assert the emitted line order.
    #[tokio::test]
    async fn initial_buffer_drains_in_fifo_after_both_snapshots() {
        let mut buf: Vec<u8> = Vec::new();
        let mut initial_outputs: Option<Vec<Output>> = None;
        let mut did_initial = false;
        let mut buffered: VecDeque<OutEvent> = VecDeque::new();

        // 1. Zone Initial arrives (buffered side: snapshot stored)
        let mut initial_zones: Option<Vec<Zone>> = Some(vec![]);
        maybe_flush_initial(
            &mut initial_zones,
            &mut initial_outputs,
            &mut did_initial,
            &mut buffered,
            true, // no_initial
            &mut buf,
        )
        .await
        .unwrap();
        assert!(!did_initial, "output snapshot not yet received");
        assert!(buf.is_empty());

        // 2. A diff arrives before output Initial — must buffer.
        push_or_emit(
            OutEvent::ZoneRemoved {
                zone_id: "z-early".into(),
            },
            did_initial,
            &mut buffered,
            &mut buf,
        )
        .await
        .unwrap();
        assert_eq!(buffered.len(), 1);
        assert!(buf.is_empty());

        // 3. Output Initial arrives → flush triggers.
        initial_outputs = Some(vec![]);
        maybe_flush_initial(
            &mut initial_zones,
            &mut initial_outputs,
            &mut did_initial,
            &mut buffered,
            true,
            &mut buf,
        )
        .await
        .unwrap();
        assert!(did_initial);

        // no_initial=true → no `initial` line. Only the buffered diff should be emitted.
        let s = String::from_utf8(buf.clone()).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 1);
        let v: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v["event"], "zone_removed");
        assert_eq!(v["zone_id"], "z-early");

        // 4. Further diffs emit immediately.
        buf.clear();
        push_or_emit(
            OutEvent::ZoneRemoved {
                zone_id: "z-late".into(),
            },
            did_initial,
            &mut buffered,
            &mut buf,
        )
        .await
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        let v: Value = serde_json::from_str(s.trim()).unwrap();
        assert_eq!(v["zone_id"], "z-late");
    }
}
