//! Tokio integration tests for typed readiness (issue #534). Real short-lived children
//! (`sleep`) and a tiny local HTTP responder on a tokio TcpListener.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use spark_supervisor::config::{ServiceConfig, SupervisorConfig};
use spark_supervisor::proc_telemetry::get_process_stats;
use spark_supervisor::supervisor::{
    ManagedServiceState, ServiceState, Supervisor, SupervisorOptions,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const WAIT: Duration = Duration::from_secs(5);

/// Status code the responder answers with; 0 means hold the connection open, never answer.
struct Responder {
    url: String,
    status: Arc<AtomicU16>,
    hits: Arc<AtomicUsize>,
}

async fn responder(status: u16, delay: Duration) -> Responder {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/health", listener.local_addr().unwrap());
    let status = Arc::new(AtomicU16::new(status));
    let hits = Arc::new(AtomicUsize::new(0));
    let (s, h) = (status.clone(), hits.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let (s, h) = (s.clone(), h.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                h.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                let code = s.load(Ordering::SeqCst);
                if code == 0 {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    return;
                }
                let resp =
                    format!("HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Responder { url, status, hits }
}

/// A URL whose port has nothing listening (bound, then released).
async fn refused_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}/health")
}

fn service(args: &[&str], restart: &str, health_url: Option<&str>) -> ServiceConfig {
    ServiceConfig {
        command: "sleep".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: HashMap::new(),
        restart: restart.into(),
        health_url: health_url.map(String::from),
    }
}

fn fast_options() -> SupervisorOptions {
    SupervisorOptions {
        probe_interval: Duration::from_millis(30),
        probe_timeout: Duration::from_millis(500),
        restart_base_secs: 0,
        restart_max_secs: 0,
        spawn_retry_delay: Duration::from_millis(50),
        shutdown_grace: Duration::from_secs(5),
    }
}

struct Running {
    sup: Arc<Supervisor>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

fn start(services: Vec<(&str, ServiceConfig)>, options: SupervisorOptions) -> Running {
    let config = SupervisorConfig {
        services: services
            .into_iter()
            .map(|(n, c)| (n.to_string(), c))
            .collect(),
    };
    let sup = Arc::new(Supervisor::with_options(config, options));
    let (tx, rx) = oneshot::channel::<()>();
    let s = sup.clone();
    let task = tokio::spawn(async move {
        s.run_until(async {
            let _ = rx.await;
        })
        .await
        .expect("run_until");
    });
    Running {
        sup,
        stop: Some(tx),
        task,
    }
}

impl Running {
    async fn state(&self, name: &str) -> ManagedServiceState {
        self.sup.snapshot().await[name].clone()
    }

    /// Polls until `pred` holds; returns every state observed on the way (in order).
    async fn wait_for(
        &self,
        name: &str,
        pred: impl Fn(&ManagedServiceState) -> bool,
    ) -> Vec<ServiceState> {
        let deadline = Instant::now() + WAIT;
        let mut seen = Vec::new();
        loop {
            let st = self.state(name).await;
            if seen.last() != Some(&st.state) {
                seen.push(st.state);
            }
            if pred(&st) {
                return seen;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting on {name}; seen {seen:?}; last {st:?}"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    async fn shutdown(mut self) -> Arc<Supervisor> {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(WAIT, &mut self.task)
            .await
            .expect("shutdown within bound")
            .unwrap();
        self.sup
    }
}

#[tokio::test]
async fn healthy_probe_reaches_ready() {
    let r = responder(200, Duration::ZERO).await;
    let run = start(
        vec![("svc", service(&["30"], "always", Some(&r.url)))],
        fast_options(),
    );
    let seen = run
        .wait_for("svc", |s| s.state == ServiceState::Ready)
        .await;
    let st = run.state("svc").await;
    assert_eq!(st.generation, 1);
    assert!(st.pid.is_some());
    assert!(r.hits.load(Ordering::SeqCst) >= 1);
    assert!(!seen.contains(&ServiceState::Unhealthy), "seen {seen:?}");
    run.shutdown().await;
}

#[tokio::test]
async fn no_health_url_is_ready_by_liveness() {
    let run = start(
        vec![("svc", service(&["30"], "always", None))],
        fast_options(),
    );
    run.wait_for("svc", |s| s.state == ServiceState::Ready)
        .await;
    run.shutdown().await;
}

#[tokio::test]
async fn http_500_is_unhealthy_never_ready() {
    let r = responder(500, Duration::ZERO).await;
    let run = start(
        vec![("svc", service(&["30"], "always", Some(&r.url)))],
        fast_options(),
    );
    let mut seen = run
        .wait_for("svc", |s| s.state == ServiceState::Unhealthy)
        .await;
    // Keep watching across several more probe rounds.
    let hits = r.hits.load(Ordering::SeqCst);
    seen.extend(
        run.wait_for("svc", |_| r.hits.load(Ordering::SeqCst) >= hits + 3)
            .await,
    );
    assert!(!seen.contains(&ServiceState::Ready), "seen {seen:?}");
    assert_eq!(run.state("svc").await.state, ServiceState::Unhealthy);
    run.shutdown().await;
}

#[tokio::test]
async fn connection_refused_is_unhealthy() {
    let url = refused_url().await;
    let run = start(
        vec![("svc", service(&["30"], "always", Some(&url)))],
        fast_options(),
    );
    let seen = run
        .wait_for("svc", |s| s.state == ServiceState::Unhealthy)
        .await;
    assert!(!seen.contains(&ServiceState::Ready), "seen {seen:?}");
    run.shutdown().await;
}

#[tokio::test]
async fn delayed_readiness_not_ready_until_responder_answers_200() {
    let r = responder(503, Duration::ZERO).await;
    let run = start(
        vec![("svc", service(&["30"], "always", Some(&r.url)))],
        fast_options(),
    );
    let before = run
        .wait_for("svc", |_| r.hits.load(Ordering::SeqCst) >= 3)
        .await;
    assert!(!before.contains(&ServiceState::Ready), "seen {before:?}");
    assert_ne!(run.state("svc").await.state, ServiceState::Ready);

    r.status.store(200, Ordering::SeqCst);
    run.wait_for("svc", |s| s.state == ServiceState::Ready)
        .await;
    assert_eq!(run.state("svc").await.generation, 1);
    run.shutdown().await;
}

#[tokio::test]
async fn spawned_process_not_ready_while_probe_in_flight_and_shutdown_mid_probe() {
    // Responder accepts but never answers; the probe stays in flight.
    let r = responder(0, Duration::ZERO).await;
    let mut opts = fast_options();
    opts.probe_timeout = Duration::from_secs(30);
    let run = start(
        vec![("svc", service(&["30"], "always", Some(&r.url)))],
        opts,
    );
    let seen = run
        .wait_for("svc", |_| r.hits.load(Ordering::SeqCst) >= 1)
        .await;
    let st = run.state("svc").await;
    assert_eq!(st.state, ServiceState::Probing, "seen {seen:?}");
    assert!(!seen.contains(&ServiceState::Ready));
    let pid = st.pid.unwrap();

    let t0 = Instant::now();
    let sup = run.shutdown().await;
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    let st = &sup.snapshot().await["svc"];
    assert_eq!(st.state, ServiceState::Stopped);
    assert_eq!(st.pid, None);
    assert!(!get_process_stats(pid).is_alive, "child {pid} still alive");
}

#[tokio::test]
async fn stale_probe_after_rapid_restart_never_marks_ready() {
    // Each child lives 150 ms; the health endpoint answers 200 only after 400 ms.
    // Every successful answer therefore belongs to a generation that already exited,
    // so the only way to report Ready would be to apply a stale result.
    let r = responder(200, Duration::from_millis(400)).await;
    let run = start(
        vec![("svc", service(&["0.15"], "always", Some(&r.url)))],
        fast_options(),
    );
    // At least two answered probes (each answer takes 400 ms) across four generations.
    let seen = run
        .wait_for("svc", |s| {
            s.generation >= 4 && r.hits.load(Ordering::SeqCst) >= 3
        })
        .await;
    assert!(!seen.contains(&ServiceState::Ready), "seen {seen:?}");
    let st = run.state("svc").await;
    assert!(st.restart_count >= 3);
    run.shutdown().await;
}

#[tokio::test]
async fn shutdown_stops_all_children_and_marks_stopped() {
    let r = responder(200, Duration::ZERO).await;
    let run = start(
        vec![
            ("web", service(&["30"], "always", Some(&r.url))),
            ("bare", service(&["30"], "always", None)),
        ],
        fast_options(),
    );
    run.wait_for("web", |s| s.state == ServiceState::Ready)
        .await;
    run.wait_for("bare", |s| s.state == ServiceState::Ready)
        .await;
    let pids: Vec<u32> = run
        .sup
        .snapshot()
        .await
        .values()
        .map(|s| s.pid.unwrap())
        .collect();

    let sup = run.shutdown().await;
    for st in sup.snapshot().await.values() {
        assert_eq!(st.state, ServiceState::Stopped, "{st:?}");
        assert_eq!(st.pid, None);
    }
    for pid in pids {
        assert!(!get_process_stats(pid).is_alive, "child {pid} still alive");
    }
}

#[tokio::test]
async fn exit_with_restart_no_is_stopped() {
    let run = start(
        vec![("once", service(&["0.05"], "no", None))],
        fast_options(),
    );
    run.wait_for("once", |s| s.generation == 1).await;
    run.wait_for("once", |s| s.state == ServiceState::Stopped)
        .await;
    let st = run.state("once").await;
    assert_eq!((st.generation, st.restart_count, st.pid), (1, 1, None));
    run.shutdown().await;
}

#[tokio::test]
async fn crashed_child_is_restarted_with_new_generation() {
    let run = start(
        vec![("flaky", service(&["0.05"], "always", None))],
        fast_options(),
    );
    run.wait_for("flaky", |s| s.generation >= 3).await;
    assert!(run.state("flaky").await.restart_count >= 2);
    run.shutdown().await;
}
