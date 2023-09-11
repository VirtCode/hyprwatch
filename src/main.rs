mod hypr;

use std::collections::HashMap;
use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use serde_json::Value;
use serde_json::value::Index;
use crate::hypr::{get_config_workspaces, get_info, open_events, read_events, WorkspaceInformation};

const MONITOR_EVENTS: [&str; 3] = ["focusedmon", "monitorremoved", "monitoradded"];
const WORKSPACE_EVENTS: [&str; 7] = ["focusedmon", "monitorremoved", "monitoradded", "workspace", "createworkspace", "destroyworkspace", "moveworkspace"];

#[derive(Parser)]
#[clap(version, about)]
#[command(disable_help_subcommand = true)]
struct Command {
    /// What to watch
    #[clap(subcommand)]
    pub what: Type,
    /// Run specified command on shell on change, with data in $data
    #[clap(short, long)]
    pub run: Option<String>,
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
    {
        let result = match &command.what {
            Type::Monitors => { Err(anyhow!("not yet implemented")) }
            Type::Workspaces { monitor, .. } => { prepare_workspaces(monitor, &config_workspaces, command.pretty) }
            Type::Clients { .. } => { Err(anyhow!("not yet implemented")) }
        };

        match result {
            Ok(s) => { println!("{s}") }
            Err(e) => { eprintln!("{e}") }
        }
    }

    if command.once { return; }

    // Listen
    let mut socket = open_events().unwrap();

    loop {
        let events = match read_events(&mut socket) {
            Ok(e) => {
                if e.is_empty() {
                    eprintln!("hyprland event socket has closed");
                    return;
                } else { e }
            }
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        let mut result = None;
        for (name, _) in events {
            result = match &command.what {
                Type::Monitors => { None }
                Type::Workspaces { monitor, .. } => {
                    if WORKSPACE_EVENTS.contains(&name.as_str()) { Some(prepare_workspaces(&monitor, &config_workspaces, command.pretty)) } else { None }
                }
                Type::Clients { .. } => { None }
            };

            if result.is_some() { break; }
        }

        if let Some(result) = result {
            match result {
                Ok(s) => { println!("{s}") }
                Err(e) => { eprintln!("{e}") }
            }
        }
    }
}

/// Prepares the workspace data
/// In addition to the workspaces, it will also query the monitor data to see whether the workspace is displayed and focussed
fn prepare_workspaces(on_monitor: &Option<String>, config_workspaces: &Option<Vec<WorkspaceInformation>>, pretty: bool) -> anyhow::Result<String> {
    let mut data = get_info(vec!("workspaces".into(), "monitors".into()))?;

    // Process monitors to retrieve shown and active workspaces
    let mut shown_map = HashMap::new();
    let monitors = data.get(1).unwrap();
    for monitor in monitors.as_array().unwrap() {
        shown_map.insert(
            monitor.get("activeWorkspace").and_then(|p| p.get("id")).and_then(Value::as_u64).unwrap(),
            monitor.get("focused").and_then(Value::as_bool).unwrap());
    }

    let mut body = data.remove(0);
    if let Value::Array(workspaces) = &mut body {
        // Remove workspaces not on monitor
        if let Some(monitor) = &on_monitor {
            workspaces.retain(|w| {
                w.get("monitor").and_then(Value::as_str).unwrap() == monitor
            })
        }

        let mut existing = vec![];

        // Add custom attributes
        for workspace in workspaces.iter_mut() {
            if let Value::Object(map) = workspace {
                let id = map.get("id").and_then(Value::as_u64).unwrap();
                let name = map.get("name").and_then(Value::as_str).unwrap().to_owned();

                map.insert("shown".into(), Value::Bool(shown_map.get(&id).is_some()));
                map.insert("active".into(), Value::Bool(*shown_map.get(&id).unwrap_or(&false)));
                map.insert("exists".into(), Value::Bool(true));

                if let Some(infos) = &config_workspaces {
                    let configured = infos.iter().position(|info| info.id == Some(id) || info.name == Some(name.to_owned()));
                    map.insert("dynamic".into(), Value::Bool(!configured.is_some()));
                    if let Some(index) = configured { existing.push(index) }
                }
            }
        }

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

                    Value::Object(map.into())
                }).collect());
        }

        workspaces.sort_by(|w1, w2| {
            w1.get("id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX)
                .cmp(&w2.get("id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX))
        })
    }

    if pretty {
        serde_json::to_string_pretty(&body).context("failed to re-serialize to json")
    } else {
        serde_json::to_string(&body).context("failed to re-serialize to json")
    }

}

