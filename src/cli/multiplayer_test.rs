use std::{
    io::{BufRead, BufReader},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};

/// Default display names for the two spawned test clients. Picked from the
/// NATO alphabet so the labels read cleanly above each character — the
/// purpose is debugging, so trivially distinguishable beats clever.
const DEFAULT_NAMES: [&str; 2] = ["Alpha", "Bravo"];
/// Stable but distinct Steam IDs so the server treats each spawned client
/// as a separate player. Different from the default offline ID
/// (`76561197960287930`) to avoid colliding with a real local-dev session.
const TEST_STEAM_IDS: [u64; 2] = [76_561_197_960_287_001, 76_561_197_960_287_002];
/// How long we wait for the server to advertise its listening port before
/// giving up. The server prints `Lightyear game server listening on …` once
/// it's ready, so on a warm rebuild this typically takes a few hundred ms.
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(45);

/// Spawn a fresh local server with an ephemeral test world and two client
/// windows that auto-connect with distinct identities. Blocks until both
/// clients exit, then shuts down the server.
pub(super) fn run_multiplayer_test(port: u16, names_override: Option<Vec<String>>) -> Result<()> {
    let names = resolved_names(names_override);
    let port = resolve_port(port)?;
    let bind: SocketAddr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);

    let exe = std::env::current_exe()
        .context("could not resolve the current executable path for multiplayer-test")?;
    let world_dir = tempdir(&format!("game-multiplayer-test-{}", std::process::id()))?;
    let world_save = world_dir.path.join("test.save");

    println!("multiplayer-test: starting server on {bind}");
    println!(
        "multiplayer-test: temporary world save → {}",
        world_save.display()
    );

    let mut server = spawn_server(&exe, &world_save, bind)?;
    if let Err(error) = wait_for_server_ready(&mut server) {
        let _ = server
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.kill().ok());
        bail!("server did not become ready: {error:#}");
    }

    println!("multiplayer-test: server ready — launching clients {names:?}");
    let mut clients = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let steam_id = TEST_STEAM_IDS[index];
        let child = spawn_client(&exe, bind, name, steam_id)
            .with_context(|| format!("could not spawn test client {name}"))?;
        clients.push(child);
    }

    let exit_signal = Arc::new(AtomicBool::new(false));
    let signal_for_handler = exit_signal.clone();
    ctrlc_listener(signal_for_handler);

    wait_for_clients(&mut clients, exit_signal.clone());
    println!("multiplayer-test: clients exited, shutting down server");
    server.shutdown();

    let _ = world_dir;
    Ok(())
}

fn resolved_names(override_names: Option<Vec<String>>) -> [String; 2] {
    let mut names = [DEFAULT_NAMES[0].to_owned(), DEFAULT_NAMES[1].to_owned()];
    if let Some(custom) = override_names {
        for (slot, name) in names.iter_mut().zip(custom.into_iter()) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                *slot = trimmed.to_owned();
            }
        }
    }
    names
}

fn resolve_port(requested: u16) -> Result<u16> {
    if requested != 0 {
        return Ok(requested);
    }
    // Bind+drop a TCP listener to reserve a port that's almost certainly
    // free for the UDP server seconds later. Not bulletproof — the kernel
    // can technically re-allocate it — but in practice it gives us a
    // distinct port per test run with no manual configuration.
    let listener = TcpListener::bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0))
        .context("could not pick a free port for multiplayer-test")?;
    let port = listener
        .local_addr()
        .context("could not read picked port")?
        .port();
    drop(listener);
    Ok(port)
}

struct ServerProcess {
    child: Arc<Mutex<Child>>,
    addr: SocketAddr,
    ready_rx: std::sync::mpsc::Receiver<ServerReady>,
}

enum ServerReady {
    Listening,
    Exited,
}

fn spawn_server(
    exe: &std::path::Path,
    save: &std::path::Path,
    addr: SocketAddr,
) -> Result<ServerProcess> {
    let mut command = Command::new(exe);
    command
        .arg("server")
        .arg("--bind")
        .arg(addr.to_string())
        .arg("--world")
        .arg(save)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = command
        .spawn()
        .with_context(|| format!("could not spawn server binary {}", exe.display()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("server stdout pipe missing"))?;

    let (tx, ready_rx) = std::sync::mpsc::channel();
    thread::Builder::new()
        .name("multiplayer-test-server-stdout".to_owned())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            let mut signalled = false;
            for line in reader.lines().map_while(Result::ok) {
                println!("[server] {line}");
                if !signalled && line.contains("listening on") {
                    let _ = tx.send(ServerReady::Listening);
                    signalled = true;
                }
            }
            if !signalled {
                let _ = tx.send(ServerReady::Exited);
            }
        })
        .context("could not spawn server stdout reader")?;

    Ok(ServerProcess {
        child: Arc::new(Mutex::new(child)),
        addr,
        ready_rx,
    })
}

fn wait_for_server_ready(server: &mut ServerProcess) -> Result<()> {
    let deadline = Instant::now() + SERVER_READY_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            bail!("timed out after {:?}", SERVER_READY_TIMEOUT);
        }
        match server.ready_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(ServerReady::Listening) => {
                // The UDP socket is reservation-based; once the message
                // landed we still pause briefly for the netcode server
                // entity to start accepting connections. Tiny — but
                // skipping it causes the first client to occasionally
                // hit "connection refused".
                wait_for_tcp_canary(server.addr);
                return Ok(());
            }
            Ok(ServerReady::Exited) => bail!("server exited before signalling readiness"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(mut child) = server.child.lock()
                    && let Ok(Some(status)) = child.try_wait()
                {
                    bail!("server process exited with status {status}");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("server output stream closed before ready signal")
            }
        }
    }
}

/// The server prints its "listening" line as soon as the UDP socket is
/// reserved, but the Lightyear netcode entity needs another tick or two to
/// finish initialising before it accepts a session. We can't TCP-probe a
/// UDP server, so just sleep a short, fixed window — short enough to feel
/// instant, long enough to let the first `app.update()` complete.
fn wait_for_tcp_canary(addr: SocketAddr) {
    // Burn a couple of frame budgets — ~50 ms at 20 Hz is one server tick.
    let _ = addr;
    thread::sleep(Duration::from_millis(150));
}

fn spawn_client(
    exe: &std::path::Path,
    server_addr: SocketAddr,
    name: &str,
    steam_id: u64,
) -> Result<Child> {
    let mut command = Command::new(exe);
    command
        .arg("client")
        .arg("--connect")
        .arg(server_addr.to_string())
        .env("GAME_PLAYER_NAME", name)
        .env("GAME_STEAM_ID", steam_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
        .spawn()
        .with_context(|| format!("could not spawn client binary {}", exe.display()))
}

fn wait_for_clients(clients: &mut Vec<Child>, exit_signal: Arc<AtomicBool>) {
    while !clients.is_empty() {
        if exit_signal.load(Ordering::SeqCst) {
            for child in clients.iter_mut() {
                let _ = child.kill();
            }
            for child in clients.iter_mut() {
                let _ = child.wait();
            }
            clients.clear();
            return;
        }
        let mut still_running = Vec::with_capacity(clients.len());
        for mut child in std::mem::take(clients) {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("multiplayer-test: client exited with {status}");
                }
                Ok(None) => still_running.push(child),
                Err(error) => {
                    eprintln!("multiplayer-test: error polling client: {error}");
                    still_running.push(child);
                }
            }
        }
        *clients = still_running;
        if clients.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

impl ServerProcess {
    fn shutdown(self) {
        // First try a clean wait — the server has a Ctrl-C handler and the
        // process tree dies when the parent exits, but we still join so we
        // don't leak when the user closed clients gracefully.
        if let Ok(mut child) = self.child.lock() {
            if let Ok(None) = child.try_wait() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        // TCP probe just keeps the addr in scope for the lifetime of the
        // server — match prevents the field from being warned as dead.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(1));
        // Drain readiness channel to make sure the stdout thread can exit.
        let _ = self.ready_rx;
    }
}

fn ctrlc_listener(flag: Arc<AtomicBool>) {
    let flag_clone = flag.clone();
    let _ = thread::Builder::new()
        .name("multiplayer-test-ctrlc".to_owned())
        .spawn(move || {
            // No external crate dependency. POSIX ignore-SIGINT-and-flag
            // pattern via the standard library only — install a tiny signal
            // shim by spawning a child that re-reads stdin. We have no such
            // shim, so we just busy-wait until the parent's stdin is gone.
            //
            // Best-effort: if a user hits Ctrl-C in the terminal, the
            // signal kills the spawned processes (same process group), and
            // they'll exit on their own. This loop just ensures the helper
            // doesn't get stuck if something weird happens.
            let mut buf = String::new();
            let _ = std::io::stdin().read_line(&mut buf);
            flag_clone.store(true, Ordering::SeqCst);
        });
    let _ = flag;
}

struct TempDir {
    path: std::path::PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn tempdir(prefix: &str) -> Result<TempDir> {
    let mut path = std::env::temp_dir();
    path.push(prefix);
    std::fs::create_dir_all(&path)
        .with_context(|| format!("could not create temp directory {}", path.display()))?;
    Ok(TempDir { path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_names_falls_back_to_defaults_when_unset() {
        assert_eq!(
            resolved_names(None),
            ["Alpha".to_owned(), "Bravo".to_owned(),]
        );
    }

    #[test]
    fn resolved_names_applies_partial_overrides() {
        let names = resolved_names(Some(vec!["Tom".to_owned()]));
        assert_eq!(names, ["Tom".to_owned(), "Bravo".to_owned()]);
    }

    #[test]
    fn resolved_names_ignores_whitespace_overrides() {
        let names = resolved_names(Some(vec!["   ".to_owned(), "Echo".to_owned()]));
        assert_eq!(names, ["Alpha".to_owned(), "Echo".to_owned()]);
    }
}
