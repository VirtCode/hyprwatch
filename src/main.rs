mod hypr;

use std::collections::HashMap;
use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use serde_json::Value;
use crate::hypr::{get_info, open_events, read_events};

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
        persistent: bool
    },
    /// Watch changes in clients (windows)
    Clients {
        /// Only watch clients on monitor
        #[clap(short, long)]
        monitor: Option<String>,
        /// Only watch clients on workspace
        #[clap(short, long)]
        workspace: Option<String>
    }
}

fn main() {
    let command = Command::parse();

    // One-shot
    if command.once {
        let result = match command.what {
            Type::Monitors => { Err(anyhow!("not yet implemented")) }
            Type::Workspaces { persistent, monitor} => { prepare_workspaces(monitor) }
            Type::Clients { .. } => { Err(anyhow!("not yet implemented")) }
        };

        match result {
            Ok(s) => { println!("{s}") }
            Err(e) => { eprintln!("{e}")}
        }

        return;
    }

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
                Type::Workspaces { monitor, persistent } => {
                    if WORKSPACE_EVENTS.contains(&name.as_str()) { Some(prepare_workspaces(monitor.clone())) }
                    else { None }
                }
                Type::Clients { .. } => { None }
            };

            if result.is_some() { break; }
        }

        if let Some(result) = result {
            match result {
                Ok(s) => { println!("{s}") }
                Err(e) => { eprintln!("{e}")}
            }
        }
    }
}

/// Prepares the workspace data
/// In addition to the workspaces, it will also query the monitor data to see whether the workspace is displayed and focussed
fn prepare_workspaces(on_monitor: Option<String>) -> anyhow::Result<String> {
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
        if let Some(monitor) = on_monitor {
            workspaces.retain(|w| {
                w.get("monitor").and_then(Value::as_str).unwrap() == monitor
            })
        }

        // Add custom attributes
        for workspace in workspaces {
            if let Value::Object(map) = workspace {
                let id = &map.get("id").and_then(Value::as_u64).unwrap();

                map.insert("shown".into(), Value::Bool(shown_map.get(id).is_some()));
                map.insert("active".into(), Value::Bool(*shown_map.get(id).unwrap_or(&false)));
            }
        }
    }

    serde_json::to_string(&body).context("failed to re-serialize to json")
}

