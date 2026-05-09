mod icon;

use std::env;
use std::ops::Deref;
use std::path::PathBuf;
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use niri_ipc::{Event, Reply, Request, Response, Window, Workspace};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use serde::{Deserialize, Serialize};
use crate::icon::get_app_icon_path;

/// Connect to the niri socket and return a buffered async reader/writer pair.
async fn connect() -> std::io::Result<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let socket_path = env::var_os("NIRI_SOCKET").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NIRI_SOCKET is not set — are you running inside niri?",
        )
    })?;

    let stream = UnixStream::connect(socket_path).await?;
    let (read_half, write_half) = stream.into_split();
    Ok((BufReader::new(read_half), write_half))
}

/// Send one JSON-encoded request and read the reply.
async fn send_request(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    request: &Request,
) -> std::io::Result<Reply> {
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;

    let mut buf = String::new();
    reader.read_line(&mut buf).await?;
    let reply: Reply = serde_json::from_str(&buf)?;
    Ok(reply)
}

/// Read one event from the open event stream.
async fn read_event(
    reader: &mut BufReader<OwnedReadHalf>,
) -> std::io::Result<Event> {
    let mut buf = String::new();
    let n = reader.read_line(&mut buf).await?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "niri closed the event stream",
        ));
    }
    let event: Event = serde_json::from_str(&buf)?;
    Ok(event)
}

#[derive(Serialize, Deserialize, Debug)]
struct NiriWindowInfo {
    icon: PathBuf,
    // #[serde(skip_serializing_if = "Option::is_none")]
    window: Window,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut reader, mut writer) = connect().await?;

    // ── 1. Request the event stream ───────────────────────────────────────────
    let reply = send_request(&mut reader, &mut writer, &Request::EventStream).await?;
    match reply {
        Ok(Response::Handled) => {}
        // Ok(Response::Version(whatever)) => {}
        Ok(other) => {
            eprintln!("Unexpected response: {other:?}");
            return Ok(());
        }
        Err(msg) => {
            eprintln!("niri returned an error: {msg}");
            return Ok(());
        }
    }

    // After switching to event-stream mode the write half is no longer used.
    drop(writer);

    // ── 2. Accumulate state ───────────────────────────────────────────────────
    // EventStreamState tracks all compositor state by applying incoming events.
    // It provides six sub-states:
    //   .workspaces  – HashMap<u64, Workspace>
    //   .windows     – HashMap<u64, Window>
    //   .keyboard_layouts – Option<KeyboardLayouts>
    //   .overview    – bool (is_open)
    //   .config      – bool (failed)
    //   .casts       – HashMap<u64, Cast>
    let mut state = EventStreamState::default();
    let mut last: Option<String> = None;

    loop {
        let event = read_event(&mut reader).await?;
        let should_emit = matches!(
            event,
            Event::WorkspacesChanged { .. } | Event::WorkspaceActivated { .. } |
            Event::WindowOpenedOrChanged { .. } | Event::WindowFocusChanged { .. }
        );

        state.apply(event);

        if !should_emit {
            continue;
        }

        let focused_workspace_opt =
            state.workspaces.workspaces.iter().find(
                |&it| it.1.is_focused
            );

        let Some(focused_workspace) = focused_workspace_opt else {
            // emit empty
            continue
        };

        let windows: Vec<_> = state.windows.windows.values()
            .filter(|w| w.workspace_id == Some(*focused_workspace.0))
            .collect();

        let Ok(result) = serde_json::to_string(&windows)
            .inspect_err(|err| println!("failed serializing windows: {}", err))
        else {
            continue;
        };

        // pos_in_scrolling_layout
        if last.as_ref().is_some_and(|it| *it != result) || last.is_none() {
            // println!("{}", result);
            windows.iter()
                .filter_map(|it| it.app_id.as_ref())
                .filter_map(|it| get_app_icon_path(it, 16))
                .for_each(|it| println!("{}", it.display()));
            last = Some(result);
        }
    }
}


