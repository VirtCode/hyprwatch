#![feature(try_blocks)]

mod hypr;

use std::collections::HashMap;
use std::process::exit;
use std::str::FromStr;
use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use serde_json::{to_string, to_string_pretty, Value};
use crate::hypr::{get_config_workspaces, get_info, open_events, read_events, WorkspaceInformation};

const MONITOR_EVENTS: [&str; 3] = ["focusedmon", "monitorremoved", "monitoradded"];
const WORKSPACE_EVENTS: [&str; 10] = ["focusedmon", "monitorremoved", "monitoradded", "workspace", "createworkspace", "destroyworkspace", "moveworkspace", "openwindow", "closewindow", "movewindow"];
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
        /// Also watch workspaces, which are empty and only defined in the config
        #[clap(short, long)]
        config: bool,
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

    // Load persistent stuff if required
    let config_workspaces = if let Type::Workspaces { config, .. } = &command.what {
        if *config {
            match get_config_workspaces() {
                Ok(w) => { Some(w) }
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            }
        } else { None }
    } else { None };

    // Eventless
    print_data(&command, &config_workspaces);

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
                print_data(&command, &config_workspaces);
                break;
            }
        }
    }
}

fn print_data(command: &Command, config_workspaces: &Option<Vec<WorkspaceInformation>>) {
    // retrieve data
    let result = match &command.what {
        Type::Monitors => { prepare_monitors() }
        Type::Workspaces { monitor, .. } => { prepare_workspaces(monitor, config_workspaces) }
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
fn prepare_workspaces(on_monitor: &Option<String>, config_workspaces: &Option<Vec<WorkspaceInformation>>) -> anyhow::Result<Value> {
    let mut data = get_info(vec!("workspaces".into(), "monitors".into()))?;

    // Process monitors to retrieve shown and active workspaces
    let shown_map: Option<HashMap<u64, bool>> = try {
        let mut map = HashMap::new();

        let monitors = data.get(1)?;
        for monitor in monitors.as_array()? {
            map.insert(
                monitor.get("activeWorkspace").and_then(|p| p.get("id")).and_then(Value::as_u64)?,
                monitor.get("focused").and_then(Value::as_bool)?);
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

        let mut existing = vec![];

        // Add custom attributes
        for workspace in workspaces.iter_mut() {
            if let Value::Object(map) = workspace {
                let info: Option<(u64, String)> = try {
                    (map.get("id").and_then(Value::as_u64)?, map.get("name").and_then(Value::as_str)?.to_owned())
                };
                let (id, name) = info.context("failure whilst reading id and name of workspace")?;

                map.insert("shown".into(), Value::Bool(shown_map.get(&id).is_some()));
                map.insert("active".into(), Value::Bool(*shown_map.get(&id).unwrap_or(&false)));

                if let Some(infos) = &config_workspaces {
                    let configured = infos.iter().position(|info| info.id == Some(id) || info.name == Some(name.to_owned()));
                    map.insert("dynamic".into(), Value::Bool(configured.is_none()));
                    map.insert("exists".into(), Value::Bool(true));
                    if let Some(index) = configured { existing.push(index) }
                }
            }
        }

        // add configured workspaces
        if let Some(infos) = &config_workspaces {
            workspaces.append(&mut infos.iter().enumerate()
                .filter(|(i, info)| {
                    !existing.contains(i) &&
                        (!on_monitor.is_some() || on_monitor == &info.monitor)
                }).map(|(_, info)| {
                let mut map = serde_json::Map::new();

                if let Some(id) = info.id { map.insert("id".to_string(), Value::Number(id.into())); }
                if let Some(name) = &info.name { map.insert("name".to_string(), Value::String(name.to_owned())); }
                if let Some(monitor) = &info.monitor { map.insert("monitor".to_string(), Value::String(monitor.to_owned())); }

                map.insert("dynamic".to_string(), Value::Bool(false));
                map.insert("exists".to_string(), Value::Bool(false));

                Value::Object(map)
            }).collect());
        }

        workspaces.sort_by(|w1, w2| {
            w1.get("id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX)
                .cmp(&w2.get("id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX))
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

