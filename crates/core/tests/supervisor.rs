use blockhost_core::supervisor::{supervise, Outcome, Policy};
use std::{
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::channel,
        Arc,
    },
    time::Duration,
};

fn fast() -> Policy {
    Policy {
        initial_backoff: Duration::from_millis(20),
        max_backoff: Duration::from_millis(80),
        max_crashes: 3,
        crash_window: Duration::from_secs(60),
        healthy_after: Duration::from_secs(60),
        stop_timeout: Duration::from_secs(3),
        max_uptime: None,
    }
}

#[cfg(unix)]
fn exit_cmd(msg: &str, code: i32) -> Command {
    let mut c = Command::new("sh");
    if msg.is_empty() {
        c.args(["-c", &format!("exit {code}")]);
    } else {
        c.args(["-c", &format!("echo {msg}; exit {code}")]);
    }
    c
}

#[cfg(windows)]
fn exit_cmd(msg: &str, code: i32) -> Command {
    let mut c = Command::new("cmd");
    if msg.is_empty() {
        c.args(["/C", &format!("exit /b {code}")]);
    } else {
        c.args(["/C", &format!("echo {msg}& exit /b {code}")]);
    }
    c
}

#[cfg(unix)]
fn wait_for_stop_cmd(exit_code: i32) -> Command {
    let mut c = Command::new("sh");
    c.args([
        "-c",
        &format!("while read l; do [ \"$l\" = stop ] && echo stopping && exit {exit_code}; done"),
    ]);
    c
}

#[cfg(windows)]
fn wait_for_stop_cmd(exit_code: i32) -> Command {
    let mut c = Command::new("powershell");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "while ($null -ne ($l = [Console]::In.ReadLine())) {{ if ($l.Trim() -eq 'stop') {{ Write-Output 'stopping'; exit {exit_code} }} }}"
        ),
    ]);
    c
}

#[cfg(unix)]
fn echo_one_line_cmd() -> Command {
    let mut c = Command::new("sh");
    c.args(["-c", "read l; echo got:$l; exit 0"]);
    c
}

#[cfg(windows)]
fn echo_one_line_cmd() -> Command {
    let mut c = Command::new("powershell");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$l = [Console]::In.ReadLine(); Write-Output ('got:' + $l); exit 0",
    ]);
    c
}

#[test]
fn crash_loop_gives_up_after_limit() {
    let (_tx, rx) = channel();
    let starts = Arc::new(AtomicUsize::new(0));
    let s2 = starts.clone();
    let out = supervise(
        move || {
            s2.fetch_add(1, Ordering::SeqCst);
            Ok(exit_cmd("boom", 1))
        },
        &fast(),
        &AtomicBool::new(false),
        &rx,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, Outcome::GaveUp { crashes: 4 });
    assert_eq!(starts.load(Ordering::SeqCst), 4);
}

#[test]
fn clean_exit_is_not_restarted_and_output_is_captured() {
    let (_tx, rx) = channel();
    let mut seen = Vec::new();
    let out = supervise(
        || Ok(exit_cmd("hello", 0)),
        &fast(),
        &AtomicBool::new(false),
        &rx,
        &mut |l| seen.push(l.trim().to_string()),
    )
    .unwrap();
    assert_eq!(out, Outcome::CleanExit);
    assert!(seen.iter().any(|l| l == "hello"), "{seen:?}");
}

#[test]
fn recovers_after_one_crash() {
    let (_tx, rx) = channel();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let out = supervise(
        move || {
            Ok(if n2.fetch_add(1, Ordering::SeqCst) == 0 {
                exit_cmd("", 1)
            } else {
                exit_cmd("up", 0)
            })
        },
        &fast(),
        &AtomicBool::new(false),
        &rx,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, Outcome::CleanExit);
    assert_eq!(n.load(Ordering::SeqCst), 2);
}

#[test]
fn shutdown_sends_stop_command() {
    let (_tx, rx) = channel();
    let flag = Arc::new(AtomicBool::new(false));
    let f2 = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        f2.store(true, Ordering::SeqCst);
    });
    let out = supervise(|| Ok(wait_for_stop_cmd(0)), &fast(), &flag, &rx, &mut |_| {}).unwrap();
    assert_eq!(out, Outcome::Shutdown);
}

#[test]
fn commands_reach_the_console() {
    let (tx, rx) = channel();
    tx.send("say hi".to_string()).unwrap();
    let mut seen = Vec::new();
    let out = supervise(
        || Ok(echo_one_line_cmd()),
        &fast(),
        &AtomicBool::new(false),
        &rx,
        &mut |l| seen.push(l.trim().to_string()),
    )
    .unwrap();
    assert_eq!(out, Outcome::CleanExit);
    assert!(seen.iter().any(|l| l == "got:say hi"), "{seen:?}");
}

#[test]
fn scheduled_restart_does_not_count_as_crash() {
    let (_tx, rx) = channel();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    let mut p = fast();
    p.max_uptime = Some(Duration::from_millis(500));
    p.max_crashes = 0; // any counted crash would end the run immediately
    let flag = Arc::new(AtomicBool::new(false));
    let f2 = flag.clone();
    let n3 = n.clone();
    std::thread::spawn(move || {
        while n3.load(Ordering::SeqCst) < 2 {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
        f2.store(true, Ordering::SeqCst);
    });
    let out = supervise(
        move || {
            n2.fetch_add(1, Ordering::SeqCst);
            Ok(wait_for_stop_cmd(143))
        },
        &p,
        &flag,
        &rx,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, Outcome::Shutdown);
    assert!(n.load(Ordering::SeqCst) >= 2);
}
