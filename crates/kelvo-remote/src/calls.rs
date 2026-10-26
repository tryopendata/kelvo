//! Remote queries on a session (D-117): `Message::Request { id, query }` answered by one
//! `Message::Reply { id, answer }`. The host answers each on the blocking pool, so a slow
//! answer never holds back live frames; the controller matches replies to callers by id.
use crate::server::ServerProvider;
use crate::{RemoteError, Result, codec};
use kelvo_proto::{Answer, ErrorCode, Message, Query};
use kelvo_schema::lock::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Queries one host session answers at once; more are refused as busy.
pub(crate) const MAX_IN_FLIGHT: usize = 8;

/// Host side: the session's queries being answered. Each task holds `keep`, so the
/// session's end reaches the provider only after its last answer is done: a scan started
/// by a query that finishes after the connection dropped is still released.
pub(crate) struct Answering {
    provider: Arc<dyn ServerProvider>,
    session: u64,
    keep: Arc<dyn Send + Sync>,
    done: mpsc::Sender<Vec<u8>>,
    /// Queries that change state (start, cancel, clear a scan), answered one at a time in
    /// the order they came, so a clear sent before a start never cancels it.
    ordered: mpsc::UnboundedSender<(u32, Query)>,
    in_flight: usize,
}

impl Answering {
    /// The answering side and where its encoded replies arrive, for the session loop to
    /// write. `keep` lives until every answer started here is done.
    pub(crate) fn new(
        provider: Arc<dyn ServerProvider>,
        session: u64,
        keep: Arc<dyn Send + Sync>,
    ) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (done, replies) = mpsc::channel(MAX_IN_FLIGHT);
        let (ordered, mut queue) = mpsc::unbounded_channel::<(u32, Query)>();
        let (lane, lane_done, lane_keep) = (provider.clone(), done.clone(), keep.clone());
        tokio::spawn(async move {
            let _keep = lane_keep;
            while let Some((id, query)) = queue.recv().await {
                let reply = answer(lane.clone(), session, id, query).await;
                let _ = lane_done.send(reply).await;
            }
        });
        let answering = Self {
            provider,
            session,
            keep,
            done,
            ordered,
            in_flight: 0,
        };
        (answering, replies)
    }

    /// Starts answering `query`, or returns the busy reply to send at once.
    pub(crate) fn start(&mut self, id: u32, query: Query) -> Option<Message> {
        if self.in_flight >= MAX_IN_FLIGHT {
            return Some(Message::Reply {
                id,
                answer: Answer::Error {
                    code: ErrorCode::Busy,
                    message: "The host is busy with other requests".into(),
                },
            });
        }
        self.in_flight += 1;
        if matches!(
            query,
            Query::SpaceStart { .. } | Query::SpaceCancel | Query::SpaceClear
        ) {
            // The lane task lives as long as `self`, which holds its sender.
            let _ = self.ordered.send((id, query));
            return None;
        }
        let (provider, done, keep) = (self.provider.clone(), self.done.clone(), self.keep.clone());
        let session = self.session;
        tokio::spawn(async move {
            let _keep = keep;
            let reply = answer(provider, session, id, query).await;
            let _ = done.send(reply).await;
        });
        None
    }

    /// A reply arrived and is being written.
    pub(crate) fn finished(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }
}

/// Answers on the blocking pool and encodes the reply frame there, with an error in its
/// place when the answer would not fit in a frame.
async fn answer(provider: Arc<dyn ServerProvider>, session: u64, id: u32, query: Query) -> Vec<u8> {
    tokio::task::spawn_blocking(move || {
        let answer = provider.answer(session, query);
        let frame = kelvo_proto::encode_frame(&Message::Reply { id, answer })
            .ok()
            .filter(|bytes| bytes.len() <= codec::MAX_WIRE_BYTES + 4);
        frame.unwrap_or_else(|| failed(id, "The answer exceeds the transport limit"))
    })
    .await
    .unwrap_or_else(|e| failed(id, &e.to_string()))
}

/// The encoded error reply for `id`.
fn failed(id: u32, message: &str) -> Vec<u8> {
    let reply = Message::Reply {
        id,
        answer: error(message),
    };
    // A short error reply always encodes.
    kelvo_proto::encode_frame(&reply).unwrap_or_default()
}

fn error(message: &str) -> Answer {
    Answer::Error {
        code: ErrorCode::Internal,
        message: message.into(),
    }
}

/// A query from the controller, and where its answer goes.
pub(crate) struct Call {
    query: Query,
    answer: oneshot::Sender<Answer>,
}

/// The controller's way to its host's current session.
#[derive(Default)]
pub(crate) enum Line {
    #[default]
    Down,
    /// Connected to a host that does not answer queries (no `query.v1`).
    Old,
    Up {
        calls: mpsc::Sender<Call>,
        usage_history: bool,
    },
}

/// Calls a session takes at once before the caller is told to back off.
const QUEUED: usize = 16;

/// Asks the host on `line` and waits up to `timeout` for its answer. An
/// `Answer::Error` is the host's own refusal; connection failures and a host too busy or
/// slow to answer are errors.
pub(crate) async fn call(line: &Mutex<Line>, query: Query, timeout: Duration) -> Result<Answer> {
    let durable = matches!(&query, Query::HistoricalUsage(_) | Query::UsageProcesses(_));
    let calls = match &*line.lock_ok() {
        Line::Up {
            usage_history: false,
            ..
        }
        | Line::Old
            if durable =>
        {
            return Err(RemoteError::Unsupported(
                "This host does not support usage history; update Kelvo on that host".into(),
            ));
        }
        Line::Up { calls, .. } => calls.clone(),
        Line::Old => {
            return Err(RemoteError::Incompatible(
                "this host's Kelvo is too old to answer; update it".into(),
            ));
        }
        Line::Down => return Err(RemoteError::Transport("the host is not connected".into())),
    };
    let (answer, answered) = oneshot::channel();
    calls
        .try_send(Call { query, answer })
        .map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                RemoteError::Busy("too many requests to the host".into())
            }
            mpsc::error::TrySendError::Closed(_) => disconnected(),
        })?;
    match tokio::time::timeout(timeout, answered).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(_)) => Err(disconnected()),
        Err(_) => Err(RemoteError::Busy("the host did not answer in time".into())),
    }
}

fn disconnected() -> RemoteError {
    RemoteError::Transport("the host disconnected".into())
}

/// Opens `line` for one session, which takes calls from the returned receiver. Dropping
/// the guard puts the line down; calls still queued or pending then fail as disconnected.
pub(crate) fn open(
    line: &Mutex<Line>,
    answers: bool,
    usage_history: bool,
) -> (LineGuard<'_>, mpsc::Receiver<Call>) {
    let (calls, queued) = mpsc::channel(QUEUED);
    *line.lock_ok() = if answers {
        Line::Up {
            calls,
            usage_history,
        }
    } else {
        Line::Old
    };
    (LineGuard(line), queued)
}

pub(crate) struct LineGuard<'a>(&'a Mutex<Line>);

impl Drop for LineGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock_ok() = Line::Down;
    }
}

/// Controller side: one session's calls awaiting a reply.
#[derive(Default)]
pub(crate) struct Pending {
    next_id: u32,
    waiting: HashMap<u32, oneshot::Sender<Answer>>,
}

impl Pending {
    /// The request to send for `call`.
    pub(crate) fn request(&mut self, call: Call) -> Message {
        // A caller that gave up still gets its reply from the host; forget it now.
        self.waiting.retain(|_, answer| !answer.is_closed());
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.waiting.insert(id, call.answer);
        Message::Request {
            id,
            query: call.query,
        }
    }

    /// Hands a reply to its caller; a reply nobody waits for is dropped.
    pub(crate) fn reply(&mut self, id: u32, answer: Answer) {
        if let Some(waiting) = self.waiting.remove(&id) {
            let _ = waiting.send(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(limit: u32) -> Query {
        Query::UsageByApp {
            from_ms: 0,
            to_ms: 1,
            by: kelvo_schema::usage::UsageKey::Cpu,
            limit,
        }
    }

    const WAIT: Duration = Duration::from_secs(5);

    fn path(answer: Result<Answer>) -> String {
        match answer {
            Ok(Answer::SpacePath(text)) => text,
            other => panic!("expected a path, got {other:?}"),
        }
    }

    fn historical() -> Query {
        Query::HistoricalUsage(kelvo_schema::durable_usage::HistoricalUsageRequest {
            host: kelvo_schema::HostId(uuid::Uuid::nil()),
            from_ms: 0,
            to_ms: 60000,
            by: kelvo_schema::durable_usage::HistoricalUsageMetric::Cpu,
            app_names: Vec::new(),
        })
    }

    #[tokio::test]
    async fn durable_queries_require_negotiated_support_and_never_fall_back_offline() {
        let line = Mutex::new(Line::Down);
        assert!(matches!(
            call(&line, historical(), WAIT).await,
            Err(RemoteError::Transport(_))
        ));
        let (old, queued) = open(&line, false, false);
        assert!(matches!(
            call(&line, historical(), WAIT).await,
            Err(RemoteError::Unsupported(_))
        ));
        drop((old, queued));
        let (guard, mut queued) = open(&line, true, false);
        assert!(matches!(
            call(&line, historical(), WAIT).await,
            Err(RemoteError::Unsupported(_))
        ));
        assert!(queued.try_recv().is_err());
        let (ordinary, ()) = tokio::join!(call(&line, usage(1), WAIT), async {
            let call = queued.recv().await.unwrap();
            call.answer.send(Answer::Done).unwrap();
        });
        assert_eq!(ordinary.unwrap(), Answer::Done);
        drop((guard, queued));
        let (guard, mut queued) = open(&line, true, true);
        let (durable, ()) = tokio::join!(call(&line, historical(), WAIT), async {
            let call = queued.recv().await.unwrap();
            assert_eq!(call.query, historical());
            call.answer.send(Answer::Done).unwrap();
        });
        assert_eq!(durable.unwrap(), Answer::Done);
        drop((guard, queued));
        assert!(matches!(
            call(&line, historical(), WAIT).await,
            Err(RemoteError::Transport(_))
        ));
    }

    /// Replies reach their own callers in any order; a reply for nobody is dropped.
    #[tokio::test]
    async fn replies_reach_their_callers_out_of_order() {
        let line = Mutex::new(Line::Down);
        let (_guard, mut queued) = open(&line, true, true);
        let session = async {
            let mut pending = Pending::default();
            let mut ids = Vec::new();
            for _ in 0..2 {
                let Message::Request { id, query } = pending.request(queued.recv().await.unwrap())
                else {
                    panic!("expected a request")
                };
                ids.push((id, query));
            }
            assert!(matches!(ids[0].1, Query::UsageByApp { limit: 1, .. }));
            assert_ne!(ids[0].0, ids[1].0);
            pending.reply(ids[0].0.wrapping_add(100), Answer::Done);
            pending.reply(ids[1].0, Answer::SpacePath("second".into()));
            pending.reply(ids[0].0, Answer::SpacePath("first".into()));
        };
        let (first, second, ()) = tokio::join!(
            call(&line, usage(1), WAIT),
            call(&line, usage(2), WAIT),
            session
        );
        assert_eq!(path(first), "first");
        assert_eq!(path(second), "second");
    }

    /// Calls fail at once when no session is up or the host is too old, and every call
    /// still waiting fails when its session ends, sent or not.
    #[tokio::test]
    async fn calls_fail_when_the_session_is_down_old_or_ends() {
        let line = Mutex::new(Line::Down);
        assert!(matches!(
            call(&line, usage(1), WAIT).await,
            Err(RemoteError::Transport(_))
        ));
        drop(open(&line, false, false));
        assert!(matches!(&*line.lock_ok(), Line::Down));
        let (guard, queued) = open(&line, false, false);
        assert!(matches!(
            call(&line, usage(1), WAIT).await,
            Err(RemoteError::Incompatible(_))
        ));
        drop((guard, queued));

        let (guard, mut queued) = open(&line, true, true);
        let session = async {
            let mut pending = Pending::default();
            let _sent = pending.request(queued.recv().await.unwrap());
            tokio::task::yield_now().await;
            drop((pending, queued, guard));
        };
        let (sent, unsent, ()) = tokio::join!(
            call(&line, usage(1), WAIT),
            call(&line, usage(2), WAIT),
            session
        );
        for failed in [sent, unsent] {
            assert!(
                matches!(&failed, Err(RemoteError::Transport(m)) if m.contains("disconnected")),
                "{failed:?}"
            );
        }
        assert!(matches!(&*line.lock_ok(), Line::Down));
    }

    /// A host that never answers times the caller out, and the abandoned call is
    /// forgotten when the next one is sent.
    #[tokio::test(start_paused = true)]
    async fn an_unanswered_call_times_out_and_is_forgotten() {
        let line = Mutex::new(Line::Down);
        let (_guard, mut queued) = open(&line, true, true);
        let mut pending = Pending::default();
        let (late, ()) = tokio::join!(call(&line, usage(1), Duration::from_secs(10)), async {
            let _ = pending.request(queued.recv().await.unwrap());
        });
        assert!(
            matches!(&late, Err(RemoteError::Busy(m)) if m.contains("in time")),
            "{late:?}"
        );
        let (answered, ()) = tokio::join!(call(&line, usage(2), WAIT), async {
            let Message::Request { id, .. } = pending.request(queued.recv().await.unwrap()) else {
                panic!("expected a request")
            };
            assert_eq!(pending.waiting.len(), 1);
            pending.reply(id, Answer::SpacePath("on time".into()));
        });
        assert_eq!(path(answered), "on time");
    }
}
