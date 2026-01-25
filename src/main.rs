use std::fmt::Write;
use std::process::Stdio;
use std::sync::LazyLock;
use std::{collections::BTreeMap, time::Duration};

use ::quirky_binder_capnp::discover_processes;
use ::quirky_binder_capnp::Process;
use dioxus::document::eval;
use dioxus::prelude::*;
use futures::{AsyncReadExt, AsyncWriteExt};
use quirky_binder_capnp::quirky_binder_capnp;
use regex::Regex;
use smol::process::Command;
use smol::Timer;
use teleop::{attach::unix_socket::connect, operate::capnp::client_connection};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[route("/")]
    Home {},
    #[route("/teleop/:pid")]
    Teleop { pid: u32 },
}

const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

enum AppTheme {
    #[allow(unused)]
    Light,
    #[allow(unused)]
    Dark,
    Corporate,
}

#[derive(Clone)]
struct GlobalState {
    theme: Signal<AppTheme>,
}

#[derive(Clone)]
struct HomeState {
    pid: Signal<Option<u32>>,
}

#[component]
fn App() -> Element {
    let theme = use_signal(|| AppTheme::Corporate);
    use_context_provider(|| GlobalState { theme });

    use_effect(move || {
        eval(&format!(
            r#"
            document.body.setAttribute("data-theme", "{}");
        "#,
            match *theme.read() {
                AppTheme::Light => "light",
                AppTheme::Dark => "dark",
                AppTheme::Corporate => "corporate",
            },
        ));
    });

    let pid = use_signal(|| None);
    use_context_provider(|| HomeState { pid });

    rsx! {
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
    }
}

#[component]
fn Home() -> Element {
    let nav = navigator();

    let HomeState { pid: mut pid_state } = use_context::<HomeState>();

    let mut processes = use_signal(|| discover_processes().unwrap());
    if let Some(pid) = pid_state() {
        if !processes().iter().any(|p| p.pid == pid) {
            pid_state.set(None)
        }
    }

    rsx! {
        div {
            class: "home",
            div {
                class: "pid-list",
                if processes().is_empty() {
                    div {
                        "No processes found"
                    }
                }
                ul {
                    class: "list",
                    for &Process{ pid, ref description } in processes().iter() {
                        li {
                            key: "{pid}",
                            class: "list-row process",
                            div {
                                class: "process-description",
                                "{description}"
                            }
                            button {
                                class: if pid_state() != Some(pid) { "btn" } else { "btn btn-active btn-accent" },
                                onclick: move |_| {
                                    pid_state.set(Some(pid));
                                    nav.push(Route::Teleop{ pid });
                                },
                                "{pid}"
                            }
                        }
                    }
                }
            }
            div {
                class: "pid-buttons",
                button {
                    class: "btn btn-secondary",
                    onclick: move |_| {
                        processes.set(discover_processes().unwrap());
                    },
                    "Refresh"
                }
            }
        }
    }
}

enum RpcState {
    Connecting,
    Connected,
    Disconnected,
}

static SVG_SIZE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"width="([0-9]+)pt" height="([0-9]+)pt""#).expect("Could not compile RE")
});

#[component]
pub fn Teleop(pid: u32) -> Element {
    let GlobalState { theme } = use_context::<GlobalState>();

    let svg = use_signal(|| None);
    let svg_size = use_memo(move || {
        svg().as_ref().map(|svg: &String| {
            let captures = SVG_SIZE_REGEX.captures(svg).unwrap();
            (
                captures[1].parse::<usize>().unwrap(),
                captures[2].parse::<usize>().unwrap(),
            )
        })
    });
    let mut scale_percent = use_signal(|| 100);
    let svg_scaled_size = use_memo(move || {
        svg_size().map(|(width, height)| {
            (
                width * scale_percent() / 100,
                height * scale_percent() / 100,
            )
        })
    });

    let mut rpc_state = use_signal(|| RpcState::Connecting);

    let state_span = match *rpc_state.read() {
        RpcState::Connecting => rsx! {
            div { "aria-label": "status", class: "status status-neutral" }
        },
        RpcState::Connected => rsx! {
            div { "aria-label": "success", class: "status status-success" }
        },
        RpcState::Disconnected => rsx! {
            div { "aria-label": "error", class: "status status-error" }
        },
    };

    use_future(move || async move {
        if let Err(err) = poll(pid, theme, rpc_state, svg).await {
            eprintln!("Error in poller: {err}");
            rpc_state.set(RpcState::Disconnected);
        }
    });

    let nav = navigator();

    rsx! {
        div {
            class: "teleop",

            div {
                class: "breadcrumbs",

                ul {
                    li { a {
                        onclick: move |_| { nav.push(Route::Home {}); },
                        "Home"
                    } }
                    li {
                        span { "Process {pid}" {state_span} }
                    }
                }
            }
            div {
                class: "teleop-svg",
                if let Some((width, height)) = svg_scaled_size() {
                    style {
                        r#"
                            .teleop-svg > div > svg {{ width: {width}px; height: {height}px; }}
                        "#
                    },
                }
                if let Some(svg) = svg() {
                    div {
                        dangerous_inner_html: "{svg}",
                    }
                }
            }
            div {
                class: "teleop-footer",
                input {
                    type: "range",
                    min: 10,
                    max: 200,
                    step: 10,
                    value: scale_percent(),
                    class: "range range-primary",
                    oninput: move |e| {
                        if let Ok(value) = e.value().parse() {
                            scale_percent.set(value);
                        }
                    },
                }
                /*
                input {
                    type: "checkbox",
                    value: "dark",
                    class: "toggle",
                    onchange: move |e| {
                        match e.checked() {
                            false => { theme.set(AppTheme::Light); }
                            true => { theme.set(AppTheme::Dark); }
                        }
                    }
                }
                */
            }
        }
    }
}

pub fn node_name_to_dot_id(name: &str) -> String {
    format!("\"{name}\"")
}

pub async fn dot_to_svg(dot_source: &str) -> std::io::Result<String> {
    let mut child = Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(dot_source.as_bytes()).await?;
    }

    let output = child.output().await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let error_message = String::from_utf8_lossy(&output.stderr);
        Err(std::io::Error::other(format!(
            "Erreur lors de l'exécution de la commande dot : {error_message}"
        )))
    }
}

async fn poll(
    pid: u32,
    theme: Signal<AppTheme>,
    mut rpc_state: Signal<RpcState>,
    mut svg: Signal<Option<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream = connect(pid).await?;

    rpc_state.set(RpcState::Connected);

    let (input, output) = stream.split();
    let (rpc_system, teleop) = client_connection(input, output).await;

    spawn(async move {
        if let Err(err) = rpc_system.await {
            eprintln!("Connection interrupted {err}");
        }
    });

    let mut req = teleop.service_request();
    req.get().set_name("state");
    let state = req.send().promise.await?;
    let state = state.get()?.get_service();
    let state: quirky_binder_capnp::state::Client = state.get_as()?;

    let graph = state.graph_request().send().promise.await?;
    let graph = graph.get()?.get_graph()?;

    let mut update_graph = async || -> Result<(), Box<dyn std::error::Error>> {
        let statuses = state.node_statuses_request().send().promise.await?;
        let statuses = statuses.get()?.get_statuses()?;
        let statuses = statuses
            .into_iter()
            .map(|s| Ok((s.get_node_name()?.to_str()?, s)))
            .collect::<capnp::Result<BTreeMap<&str, _>>>()?;

        let mut dot = String::new();

        writeln!(&mut dot, "digraph G {{")?;

        writeln!(&mut dot, "    graph [bgcolor=\"transparent\"];")?;

        match *theme.read() {
            AppTheme::Light | AppTheme::Corporate => {
                writeln!(&mut dot, "    node [fontcolor=\"black\", color=\"black\"];")?;
                writeln!(&mut dot, "    edge [fontcolor=\"black\", color=\"black\"];")?;
            }
            AppTheme::Dark => {
                writeln!(&mut dot, "    node [fontcolor=\"white\", color=\"white\"];")?;
                writeln!(&mut dot, "    edge [fontcolor=\"white\", color=\"white\"];")?;
            }
        }

        let nodes = graph.get_nodes()?;

        for node in nodes {
            let node_name = node.get_name()?.to_str()?;

            write!(&mut dot, "{} [", node_name_to_dot_id(node_name))?;

            let node_status = statuses[node_name];
            let state = node_status.get_state()?.which()?;
            let read_records = node_status
                .get_input_read()?
                .iter()
                .fold(None, |acc, read| {
                    acc.map_or(Some(read), |acc| Some(acc + read))
                });
            let written_records = node_status
                .get_output_written()?
                .iter()
                .fold(None, |acc, written| {
                    acc.map_or(Some(written), |acc| Some(acc + written))
                });
            let total_records = read_records.map_or(written_records, |read| {
                Some(read + written_records.unwrap_or(0))
            });

            for (i, (attr, val)) in [(
                "color",
                match state {
                    quirky_binder_capnp::node_state::Which::Waiting(()) => "#59636e",
                    quirky_binder_capnp::node_state::Which::Running(()) => match total_records {
                        None => "#59636e",
                        Some(_) => "#dbab0a",
                    },
                    quirky_binder_capnp::node_state::Which::Success(()) => "#1a7f37",
                    quirky_binder_capnp::node_state::Which::Error(_) => "#d1242f",
                },
            )]
            .into_iter()
            .enumerate()
            {
                if i > 0 {
                    write!(&mut dot, ", ")?;
                } else {
                    writeln!(&mut dot)?;
                }
                writeln!(&mut dot, "{attr} = \"{val}\"",)?;
            }

            writeln!(&mut dot, "]")?;
        }

        let edges = graph.get_edges()?;

        for edge in edges {
            let tail_name = edge.get_tail_name()?.to_str()?;
            let head_name = edge.get_head_name()?.to_str()?;

            write!(
                &mut dot,
                "{} -> {} [",
                node_name_to_dot_id(tail_name),
                node_name_to_dot_id(head_name)
            )?;

            let tail_index = edge.get_tail_index();
            let tail_counter = statuses
                .get(tail_name)
                .map(|s| capnp::Result::Ok(s.get_output_written()?.get(tail_index as _)))
                .transpose()?;

            let head_index = edge.get_head_index();
            let head_counter = statuses
                .get(head_name)
                .map(|s| capnp::Result::Ok(s.get_input_read()?.get(head_index as _)))
                .transpose()?;

            for (i, (attr, val)) in tail_counter
                .map(|n| ("taillabel", n.to_string()))
                .into_iter()
                .chain(
                    head_counter
                        .map(|n| ("headlabel", n.to_string()))
                        .into_iter(),
                )
                .enumerate()
            {
                if i > 0 {
                    write!(&mut dot, ", ")?;
                } else {
                    writeln!(&mut dot)?;
                }
                writeln!(&mut dot, "{attr} = \"{val}\"",)?;
            }

            writeln!(&mut dot, "]")?;
        }
        writeln!(&mut dot, "}}")?;

        //println!("DOT: {dot}");

        let svg_str = dot_to_svg(&dot).await?;

        svg.set(Some(svg_str));

        Timer::after(Duration::from_millis(3000)).await;

        Ok(())
    };

    loop {
        update_graph().await?;
    }
}
