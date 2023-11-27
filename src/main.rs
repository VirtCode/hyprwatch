#![feature(try_blocks)]

mod hypr;

use std::collections::HashMap;
use std::process::exit;
use std::str::FromStr;
use anyhow::{Context};
use clap::{Parser, Subcommand};
use serde_json::{to_string, to_string_pretty, Value};
use crate::hypr::{get_info, open_events, read_events};

const MONITOR_EVENTS: [&str; 3] = ["focusedmon", "monitorremoved", "monitoradded"];
const WORKSPACE_EVENTS: [&str; 11] = ["focusedmon", "monitorremoved", "monitoradded", "workspace", "createworkspace", "destroyworkspace", "moveworkspace", "openwindow", "closewindow", "movewindow", "activespecial"];
const CLIENT_EVENTS: [&str; 7] = ["openwindow", "closewindow", "movewindow", "changefloatingmode", "fullscreen", "windowtitle", "activewindowv2"];

#[derive(Parser)]
#[clap(version, about)]
#[command(disable_help_subcommand = true)]
struct Command {
    /// What to watch
    #[clap(subcommand)]
    pub what: Type,
    /// Query only once, don't listen for events
    #[clap(short, long)]
    pub once: bool,
    /// Pretty print result (uses multiple lines per event)
    #[clap(short, long)]
    pub pretty: bool,
}

#[derive(Subcommand)]
enum Type {
    /// Watch changes in monitors
    Monitors,
    /// Watch changes in workspaces
    Workspaces {
        /// Only watch workspaces on monitor
        #[clap(short, long)]
        monitor: Option<String>,

        /// Only watch for workspaces with special status
        #[clap(short, long)]
        special: Option<bool>,
    },
    /// Watch changes in clients (windows)
    Clients {
        /// Only watch clients on monitor
        #[clap(short, long)]
        monitor: Option<String>,
        /// Only watch clients on workspace
        #[clap(short, long)]
        workspace: Option<String>,
    },
}

fn main() {
    let command = Command::parse();

    // Eventless
    print_data(&command);

    if command.once { return; }

    // Listen
    let mut socket = match open_events() {
        Ok(s) => { s }
        Err(e) => {
            eprintln!("{e}");
            exit(-1);
        }
    };

    let keywords = match &command.what {
        Type::Monitors => { MONITOR_EVENTS.as_slice() }
        Type::Workspaces { .. } => { WORKSPACE_EVENTS.as_slice() }
        Type::Clients { .. } => { CLIENT_EVENTS.as_slice() }
    };

    loop {
        let events = match read_events(&mut socket) {
            Ok(e) => {
                if e.is_empty() {
                    eprintln!("hyprland event socket has closed");
                    exit(-1);
                } else { e }
            }
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        for (s, _) in events {
            if keywords.contains(&s.as_str()) {
                print_data(&command);
                break;
            }
        }
    }
}

fn print_data(command: &Command) {
    // retrieve data
    let result = match &command.what {
        Type::Monitors => { prepare_monitors() }
        Type::Workspaces { monitor, special, .. } => { prepare_workspaces(monitor, special) }
        Type::Clients { monitor, workspace } => { prepare_clients(monitor, workspace) }
    };

    // turn to string
    let result = result.and_then(|v| {
        if command.pretty { to_string_pretty(&v) } else { to_string(&v) }
            .context("failed to serialize json")
    });

    // print
    match result {
        Ok(s) => { println!("{s}") }
        Err(e) => { eprintln!("{e}") }
    }
}

/// Prepares the monitor data
fn prepare_monitors() -> anyhow::Result<Value> {
    let mut data = get_info(vec!("monitors".into()))?;

    Ok(data.pop().expect("socket one seems broken"))
}

/// Prepares the workspace data
/// In addition to the workspaces, it will also query the monitor data to see whether the workspace is displayed and focussed
fn prepare_workspaces(on_monitor: &Option<String>, special_status: &Option<bool>) -> anyhow::Result<Value> {
    let mut data = get_info(vec!("workspaces".into(), "monitors".into()))?;

    // Process monitors to retrieve shown and active workspaces
    let shown_map: Option<HashMap<i64, bool>> = try {
        let mut map = HashMap::new();

        let monitors = data.get(1)?;
        for monitor in monitors.as_array()? {

            // Special workspace
            let special = monitor.get("specialWorkspace").and_then(|p| p.get("id")).and_then(Value::as_i64)?;
            if special != 0 {
                map.insert(special, true);
            }

            // Normal workspace
            map.insert(
                monitor.get("activeWorkspace").and_then(|p| p.get("id")).and_then(Value::as_i64)?,
                special == 0 && monitor.get("focused").and_then(Value::as_bool)?);
        }

        map
    };
    let shown_map = shown_map.context("failure whilst reading monitors to find shown workspaces")?;

    let mut body = data.remove(0);
    if let Value::Array(workspaces) = &mut body {
        // Remove workspaces not on monitor
        if let Some(monitor) = &on_monitor {
            workspaces.retain(|w| {
                w.get("monitor").and_then(Value::as_str) == Some(monitor)
            })
        }

        // Remove workspaces with special status
        if let Some(special) = &special_status {
            workspaces.retain(|w| {
                w.get("id").and_then(Value::as_i64).map(|i| i < 0) == Some(*special)
            })
        }

        // Add custom attributes
        for workspace in workspaces.iter_mut() {
            if let Value::Object(map) = workspace {
                let info: Option<(i64, String)> = try {
                    (map.get("id").and_then(Value::as_i64)?, map.get("name").and_then(Value::as_str)?.to_owned())
                };
                let (id, name) = info.context("failure whilst reading id and name of workspace")?;

                map.insert("shown".into(), Value::Bool(shown_map.get(&id).is_some()));
                map.insert("active".into(), Value::Bool(*shown_map.get(&id).unwrap_or(&false)));
            }
        }

        workspaces.sort_by(|w1, w2| {
            w1.get("id").and_then(|v| v.as_i64()).unwrap_or(i64::MAX)
                .cmp(&w2.get("id").and_then(|v| v.as_i64()).unwrap_or(i64::MAX))
        })
    }

    Ok(body)
}

/// Prepares the client data
fn prepare_clients(monitor: &Option<String>, workspace: &Option<String>) -> anyhow::Result<Value> {
    let mut data = get_info(vec!("clients".into(), "monitors".into()))?;

    // map monitor ids to names
    let monitor_names: Option<HashMap<u64, String>> = try {
        let mut map = HashMap::new();

        let monitors = data.get(1).expect("socket one seems broken");
        for monitor in monitors.as_array()? {
            map.insert(monitor.get("id").and_then(Value::as_u64)?,
                       monitor.get("name").and_then(Value::as_str)?.to_string());
        }

        map
    };
    let monitor_names = monitor_names.context("failure whilst reading monitor names for filtering")?;

    let mut body = data.remove(0);
    if let Value::Array(clients) = &mut body {
        // filter by workspace
        if let Some(workspace) = workspace {
            clients.retain(|c| {
                let ws: Option<(&str, i64)> = try {
                    let ws = c.get("workspace")?;
                    (ws.get("name").and_then(Value::as_str)?, ws.get("id").and_then(Value::as_i64)?)
                };

                if let Some((name, id)) = ws {
                    if let Some(desired_name) = workspace.strip_prefix("name:") {
                        desired_name == name
                    } else {
                        i64::from_str(workspace).map(|desired_id| desired_id == id).unwrap_or_default()
                    }
                } else { false }
            });
        }

        // associate and filter with monitors
        clients.retain_mut(|c| {
            if let Value::Object(map) = c {
                if let Some(m) = map.get("monitor").and_then(Value::as_i64) {
                    if let Some(name) = monitor_names.get(&(m as u64)) {
                        map.insert("monitorName".to_string(), Value::String(name.clone()));

                        return if let Some(filter) = monitor {
                            filter == name
                        } else { true }
                    }
                }

                // still keep fails when no monitor filter is active
                monitor.is_none()
            } else { true }
        })
    }

    Ok(body)
}

