//! The one fake computerd the computer tests share. It reads each
//! connection's first line as computerd does, into the typed `Request`, so a
//! request computerd would not know comes back "bad request" as it would. It
//! remembers in memory what it was asked and which computers lost their
//! overlay. After a `connect` it is the computer's guest agent too, and reads
//! each line as a `GuestRequest`, answering at once as the real agent would.
//! Its `Script` makes it full, old, a guest that cannot confirm a handback
//! clean, or a relay that fails a connection.

use magnus_computerd_proto::guest::GuestRequest;
use magnus_computerd_proto::{Request, OPS};
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// What happens to one guest connection, in the order they come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Turn {
    /// The relay could not reach the guest: nothing is read past the connect.
    Unreached,
    /// The guest takes the request and answers it.
    Answers,
    /// The guest takes the request and goes away without answering.
    TakesAndCloses,
}

/// How the fake behaves; a test changes it with `Fake::set`.
#[derive(Default)]
pub(super) struct Script {
    /// The server has no room: every wake is refused for capacity.
    pub full: bool,
    /// Built before `version` and `erase_volume`, as the server's was on
    /// 2026-11-11: it refuses both as a bad request.
    pub old: bool,
    /// The guest cannot confirm a takeover left nothing running.
    pub unclean: bool,
    /// What `version` says it serves. None: every request there is.
    pub ops: Option<Vec<String>>,
    /// How the next guest connections go. Once empty, each is answered.
    pub turns: VecDeque<Turn>,
}

#[derive(Default)]
struct Memory {
    script: Script,
    /// Every line it read, computerd's and the guests', as sent.
    asked: Vec<Value>,
    connections: usize,
    /// Computers whose overlay was thrown away (a destroy, an erase): as the
    /// real one, they wake on a fresh one.
    fresh: HashSet<String>,
}

type Shared = Arc<Mutex<Memory>>;

pub(super) struct Fake {
    pub socket: PathBuf,
    memory: Shared,
    _dir: Option<tempfile::TempDir>,
}

impl Fake {
    /// A fake in a directory of its own.
    pub fn start() -> Fake {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Fake::at(&dir.path().join("computerd.sock"));
        f._dir = Some(dir);
        f
    }

    /// A fake listening at `socket`.
    pub fn at(socket: &Path) -> Fake {
        let listener = UnixListener::bind(socket).unwrap();
        let memory: Shared = Arc::default();
        let m = memory.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(connection(s, m.clone()));
            }
        });
        Fake { socket: socket.to_path_buf(), memory, _dir: None }
    }

    pub fn set(&self, change: impl FnOnce(&mut Script)) {
        change(&mut self.memory.lock().unwrap().script);
    }

    /// Every line it was sent, in order.
    pub fn asked(&self) -> Vec<Value> {
        self.memory.lock().unwrap().asked.clone()
    }

    /// The lines it was sent with this `op`, computerd's or a guest's.
    pub fn asked_op(&self, op: &str) -> Vec<Value> {
        self.asked().into_iter().filter(|r| r["op"] == op).collect()
    }

    pub fn connections(&self) -> usize {
        self.memory.lock().unwrap().connections
    }
}

async fn connection(s: UnixStream, memory: Shared) {
    let (r, mut w) = s.into_split();
    let mut lines = BufReader::new(r).lines();
    let Ok(Some(first)) = lines.next_line().await else { return };
    {
        let mut m = memory.lock().unwrap();
        m.connections += 1;
        m.asked.push(serde_json::from_str(&first).unwrap_or(Value::Null));
    }
    let req = serde_json::from_str::<Request>(&first);
    if let Ok(Request::Connect { .. }) = req {
        return guest(lines, w, memory).await;
    }
    let reply = match req {
        Ok(req) => answer(req, &mut memory.lock().unwrap()),
        // computerd's own words: `context("bad request")`, shown with `{:#}`.
        Err(e) => refused(format!("bad request: {e}")),
    };
    let _ = w.write_all(format!("{reply}\n").as_bytes()).await;
}

fn refused(error: String) -> Value {
    json!({ "ok": false, "error": error })
}

fn ok(mut v: Value) -> Value {
    v["ok"] = json!(true);
    v
}

/// What computerd answers. A request this fake does not model is refused.
fn answer(req: Request, m: &mut Memory) -> Value {
    let op = req.op();
    match req {
        Request::Version | Request::EraseVolume { .. } if m.script.old => refused(format!("bad request: unknown variant `{op}`")),
        Request::Version => {
            let ops = m.script.ops.clone().unwrap_or_else(|| OPS.iter().map(|o| o.to_string()).collect());
            ok(json!({ "version": "0.0.0", "base_id": null, "ops": ops }))
        }
        Request::Wake { .. } if m.script.full => refused("computerd already runs as many computers as it may".into()),
        Request::Wake { id } => {
            let new = m.fresh.remove(&id);
            ok(json!({ "resumed": !new, "overlay_fresh": new }))
        }
        Request::Destroy { id, .. } | Request::EraseVolume { id } => {
            m.fresh.insert(id);
            ok(json!({}))
        }
        Request::Capacity => ok(json!({ "live": 0 })),
        Request::Create { .. } | Request::Hibernate { .. } | Request::Pause { .. } | Request::Resume { .. } => ok(json!({})),
        _ => refused(format!("the fake computerd does not do {op}")),
    }
}

/// The guest agent, after computerd's relay said it reached it.
async fn guest(mut lines: tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>, mut w: tokio::net::unix::OwnedWriteHalf, memory: Shared) {
    let turn = memory.lock().unwrap().script.turns.pop_front().unwrap_or(Turn::Answers);
    if turn == Turn::Unreached {
        let _ = w.write_all(format!("{}\n", refused("Connection reset by peer (os error 104)".into())).as_bytes()).await;
        return;
    }
    if w.write_all(format!("{}\n", ok(json!({}))).as_bytes()).await.is_err() {
        return;
    }
    while let Ok(Some(line)) = lines.next_line().await {
        let reply = {
            let mut m = memory.lock().unwrap();
            m.asked.push(serde_json::from_str(&line).unwrap_or(Value::Null));
            if turn == Turn::TakesAndCloses {
                return;
            }
            guest_answer(&line, &m.script)
        };
        if w.write_all(format!("{reply}\n").as_bytes()).await.is_err() {
            return;
        }
    }
}

/// As `sandbox/guest` answers, with every desktop action's screenshot "SHOT".
fn guest_answer(line: &str, script: &Script) -> Value {
    let req = match serde_json::from_str::<GuestRequest>(line) {
        Ok(req) => req,
        Err(e) => return json!({ "error": format!("bad request: {e}") }),
    };
    match req {
        GuestRequest::Ping | GuestRequest::Sync | GuestRequest::ZeroFree | GuestRequest::Erase { .. } | GuestRequest::TakeoverBegin => json!({ "ok": true }),
        GuestRequest::Reset { .. } | GuestRequest::Freeze | GuestRequest::Thaw => json!({ "ok": true }),
        GuestRequest::Exec { .. } => json!({ "exit_code": 0 }),
        GuestRequest::Read { path, .. } => json!({ "data_b64": "SHOT", "path": path }),
        GuestRequest::Write { path, .. } => json!({ "ok": true, "path": path }),
        GuestRequest::List { path } => json!({ "path": path, "entries": [] }),
        GuestRequest::TakeoverLeftovers => json!({ "ok": true, "programs": ["gedit"] }),
        GuestRequest::TakeoverEnd if script.unclean => json!({ "error": "1 processes started during the takeover would not stop: spy (pid 9)" }),
        GuestRequest::TakeoverEnd => json!({ "ok": true, "stopped": ["spy (pid 9)"], "stopped_count": 1, "removed": [] }),
    }
}
