mod icon;

use crate::icon::get_app_icon_path;
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use niri_ipc::{Event, Reply, Request, Response, Window};
use serde::Serialize;
use std::env;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;

async fn connect() -> std::io::Result<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let socket_path = env::var_os("NIRI_SOCKET").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NIRI_SOCKET is not set - are you running inside niri?",
        )
    })?;

    let stream = UnixStream::connect(socket_path).await?;
    let (read_half, write_half) = stream.into_split();
    Ok((BufReader::new(read_half), write_half))
}

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

#[derive(Serialize, Debug)]
struct NiriWindowInfo<'a> {
    // #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<PathBuf>,
    window: &'a Window,
}

#[derive(Serialize, Debug)]
struct NiriInfo<'a> {
    focused_window: Option<u64>,
    windows: &'a[NiriWindowInfo<'a>],
}

fn hash<T>(items: &[&T]) -> u64
where T: Serialize + ?Sized {
    let mut hasher = DefaultHasher::new();

    if let Ok(bytes) = postcard::to_allocvec(&items) {
        bytes.hash(&mut hasher);
    }

    hasher.finish()
}

fn print_empty() -> u64 {
    println!("{}", r#"{"focused_window":null,"windows":[]}"#);
    0
}

fn emit(state: &EventStreamState, last: u64) -> u64 {
    let focused_workspace_opt =
        state.workspaces.workspaces.iter().find(
            |&it| it.1.is_focused
        );

    let Some(focused_workspace) = focused_workspace_opt else {
        return print_empty();
    };

    let windows: Vec<_> = state.windows.windows.values()
        .filter(|w| w.workspace_id == Some(*focused_workspace.0))
        .collect();

    let check = hash(&windows);

    if check == last {
        return check;
    }

    if windows.len() == 0 {
        return print_empty();
    }

    let mut filtered_windows = windows
        .iter()
        .map(|&it| {
            NiriWindowInfo {
                window: it,
                icon: it.app_id.as_ref().and_then(|it| get_app_icon_path(&it, 32)),
            }
        })
        .collect::<Vec<_>>();

    filtered_windows.sort_by_key(|w| w.window.layout.pos_in_scrolling_layout);

    let info = NiriInfo {
        focused_window: filtered_windows
            .iter()
            .find(|it| it.window.is_focused)
            .and_then(|it| Some(it.window.id)),
        windows: &filtered_windows,
    };

    let result = serde_json::to_string(&info);

    let Ok(result) = result
        .inspect_err(|err| eprintln!("failed serializing windows: {}", err))
    else {
        return check;
    };

    println!("{}", result);

    check
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let (mut reader, mut writer) = connect().await?;

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

    drop(writer);

    let mut state = EventStreamState::default();
    let mut last: u64 = emit(&state, 0);

    loop {
        let event = read_event(&mut reader).await?;
        // println!("{:?}", event);

        state.apply(event);

        last = emit(&state, last);
    }
}


