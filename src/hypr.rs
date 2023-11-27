use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use anyhow::Context;
use serde_json::Value;

/// Opens socket two for reading
pub fn open_events() -> anyhow::Result<UnixStream> {
    UnixStream::connect(get_hypr_socket("socket2")?).context("failed to open socket 2")
}

/// Reads a list of new events off of socket 2, returns an empty list if socket is closed
pub fn read_events(socket: &mut UnixStream) -> anyhow::Result<Vec<(String, Vec<String>)>> {
    // Yes, if there is data on the socket > than 256 bytes, things begin to break.
    let mut buf = [0; 256];

    let len = socket.read(&mut buf).context("failed to read from socket 2")?;

    Ok(String::from_utf8(buf[..len].to_vec()).context("socket 2 did not return valid utf-8")?
        .split('\n').filter(|s| !s.is_empty()).filter_map(|s| {

        // Parses e.g. activewindow>>alacritty,Window Title
        let splits: Vec<&str> = s.split(">>").collect();
        if splits.is_empty() { return None }

        let name = splits.first().expect("vec behaving weirdly").to_string();
        let args = if let Some(args) = splits.get(1) {
            args.split(',').map(String::from).collect()
        } else { vec![] };

        Some((name, args))
    }).collect())
}

/// Gets information from socket 1, is always executed through a batch request and returned in json
pub fn get_info(requests: Vec<String>) -> anyhow::Result<Vec<Value>>{
    // Produces request string, e.g. [[BATCH]] j/monitors ; j/workspaces
    let request = "[[BATCH]] ".to_string() + &requests.iter().map(|s| "j/".to_string() + s).collect::<Vec<String>>().join(" ; ");

    let mut socket = UnixStream::connect(get_hypr_socket("socket")?).context("failed to open socket 1")?;
    socket.write_all(request.as_bytes()).context("failed to write to socket 1")?;

    let mut response = String::new();
    socket.read_to_string(&mut response).context("failed to read from socket 1")?;

    // Response will be everything concatenated without any spaces, put DEL character between these so we can split by that
    response = response.replace("][", "]\x7f[").replace("}[", "}\x7f[").replace("]{", "]\x7f{");

    // Now split by that inserted character
    response.split('\x7f')
        .map(serde_json::from_str::<Value>)
        .collect::<serde_json::Result<Vec<Value>>>().context("socket 1 did not return valid json")
}

/// Returns the path to the hyprland config file
pub fn get_hypr_config() -> anyhow::Result<String> {
    env::var("XDG_CONFIG_HOME")
        .or_else(|_e| env::var("HOME").map(|s| s + "/.config"))
        .map(|s| s + "/hypr/hyprland.conf").context("$HOME is not set, cannot find hyprland config")
}

/// Returns the path to a socket, based on its name (without . and ending) and the instance signature
pub fn get_hypr_socket(name: &str) -> anyhow::Result<String> {
    let instance = env::var("HYPRLAND_INSTANCE_SIGNATURE").context("couldn't find instance singature, is hyprland running?")?;

    Ok(format!("/tmp/hypr/{instance}/.{name}.sock"))
}

