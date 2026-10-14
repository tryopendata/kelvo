//! Shared on-demand log pipelines. Quiet Follow streams have no read-idle timeout.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::task::AbortHandle;

pub struct LogSubscription {
    pub id: u32,
    pub initial: Option<DockerLogBatch>,
    pub receiver: LogReceiver,
}
#[derive(Clone)]
struct LogDelivery {
    batch: DockerLogBatch,
    tail_complete: bool,
}
pub struct LogReceiver {
    receiver: broadcast::Receiver<LogDelivery>,
    follow: bool,
    cancellation: watch::Receiver<Option<&'static str>>,
    generation: u32,
    ended: bool,
}
impl LogReceiver {
    pub async fn recv(&mut self) -> Result<DockerLogBatch, broadcast::error::RecvError> {
        loop {
            if self.ended {
                return Err(broadcast::error::RecvError::Closed);
            }
            let mut delivery = tokio::select! { biased;
                changed = self.cancellation.changed() => {
                    self.ended = true;
                    if changed.is_err() { return Err(broadcast::error::RecvError::Closed); }
                    return Ok(DockerLogBatch { generation: self.generation, lines: Vec::new(), truncated: false, complete: true, error: self.cancellation.borrow().map(str::to_owned) });
                }
                delivery = self.receiver.recv() => delivery?,
            };
            if delivery.tail_complete {
                if self.follow {
                    continue;
                }
                delivery.batch.complete = true;
            }
            if delivery.batch.lines.is_empty()
                && !delivery.batch.complete
                && !delivery.batch.truncated
                && delivery.batch.error.is_none()
            {
                continue;
            }
            return Ok(delivery.batch);
        }
    }
}
struct Pipeline {
    sender: broadcast::Sender<LogDelivery>,
    abort: AbortHandle,
    users: usize,
    followers: watch::Sender<bool>,
    buffer: Arc<Mutex<LogBuffer>>,
}
#[derive(Default)]
pub(super) struct LogRegistry {
    next: u32,
    subscriptions: HashMap<u32, (String, watch::Sender<Option<&'static str>>)>,
    pipelines: HashMap<String, Pipeline>,
}
impl LogRegistry {
    pub(super) fn cancel_all(&mut self) {
        for (_, p) in self.pipelines.drain() {
            let _ = p.sender.send(LogDelivery {
                tail_complete: false,
                batch: DockerLogBatch {
                    generation: 0,
                    lines: Vec::new(),
                    truncated: false,
                    complete: true,
                    error: Some("Monitoring changed; reopen logs to continue".into()),
                },
            });
            p.abort.abort();
        }
        for (_, (_, cancellation)) in self.subscriptions.drain() {
            cancellation.send_replace(Some("Monitoring changed; reopen logs to continue"));
        }
    }
    pub(super) fn close(&mut self, id: u32) {
        self.cancel(id, "Log subscription closed; reopen logs to continue");
    }
    pub(super) fn cancel(&mut self, id: u32, reason: &'static str) {
        if let Some((key, cancellation)) = self.subscriptions.remove(&id) {
            cancellation.send_replace(Some(reason));
            let Some(p) = self.pipelines.get_mut(&key) else {
                return;
            };
            p.users = p.users.saturating_sub(1);
            if p.users == 0 {
                p.abort.abort();
                self.pipelines.remove(&key);
            }
        }
    }
}
pub(super) fn open(
    handle: &DockerHandle,
    request: DockerLogsRequest,
) -> Result<LogSubscription, DockerServiceError> {
    let _gate = handle.gate.lock_ok();
    let adapter = log_adapter(handle, &request)?;
    let generation = handle.control.borrow().generation;
    let key = format!("{generation}:{}", request.container_id);
    let mut registry = handle.logs.lock_ok();
    if registry.subscriptions.len() >= 32 {
        return Err(DockerServiceError::ResourceLimit(
            "Too many log panes are open",
        ));
    }
    let id = registry.next.wrapping_add(1);
    registry.next = id;
    let (cancel, cancellation) = watch::channel(None);
    if let Some(p) = registry.pipelines.get_mut(&key) {
        p.users += 1;
        if request.follow {
            p.followers.send_replace(true);
        }
        let buffer = p.buffer.lock_ok();
        let receiver = LogReceiver {
            receiver: p.sender.subscribe(),
            follow: request.follow,
            cancellation,
            generation,
            ended: false,
        };
        let initial = Some(DockerLogBatch {
            generation,
            lines: buffer
                .lines
                .iter()
                .skip(buffer.lines.len().saturating_sub(200))
                .cloned()
                .collect(),
            truncated: buffer.truncated,
            complete: buffer.complete || (!request.follow && buffer.tail_complete),
            error: buffer.error.clone(),
        });
        drop(buffer);
        registry.subscriptions.insert(id, (key, cancel));
        return Ok(LogSubscription {
            id,
            receiver,
            initial,
        });
    }
    if registry.pipelines.len() >= 2 {
        return Err(DockerServiceError::ResourceLimit(
            "Two log streams are open; close one to continue",
        ));
    }
    let (sender, receiver) = broadcast::channel(16);
    let receiver = LogReceiver {
        receiver,
        follow: request.follow,
        cancellation,
        generation,
        ended: false,
    };
    let (followers, follow_changes) = watch::channel(request.follow);
    let buffer = Arc::new(Mutex::new(LogBuffer::default()));
    let sink = Arc::clone(&buffer);
    let send = sender.clone();
    let control = handle.control.subscribe();
    let task = tokio::spawn(async move {
        pump(
            adapter,
            request,
            generation,
            send,
            control,
            sink,
            follow_changes,
        )
        .await;
    });
    registry.pipelines.insert(
        key.clone(),
        Pipeline {
            sender,
            abort: task.abort_handle(),
            users: 1,
            followers,
            buffer,
        },
    );
    registry.subscriptions.insert(id, (key, cancel));
    Ok(LogSubscription {
        id,
        receiver,
        initial: None,
    })
}
// The caller holds the configuration gate while selecting the live adapter.
fn log_adapter(
    handle: &DockerHandle,
    request: &DockerLogsRequest,
) -> Result<DockerAdapter, DockerServiceError> {
    if !kelvo_collect::docker::valid_id(&request.container_id) {
        return Err(DockerServiceError::InvalidRequest("Invalid container ID"));
    }
    let adapter = handle
        .inner
        .lock_ok()
        .adapter
        .clone()
        .ok_or(DockerServiceError::Unavailable("Docker is not connected"))?;
    if adapter.engine.id != request.engine_id {
        return Err(DockerServiceError::Unavailable(
            "Logs are available only for the selected live engine",
        ));
    }
    Ok(adapter)
}
async fn pump(
    adapter: DockerAdapter,
    request: DockerLogsRequest,
    generation: u32,
    sender: broadcast::Sender<LogDelivery>,
    mut control: watch::Receiver<Control>,
    buffer: Arc<Mutex<LogBuffer>>,
    mut followers: watch::Receiver<bool>,
) {
    // Fetch the latest tail without an upper timestamp bound. Follow replays from
    // the second captured before that fetch; counted overlap removal preserves the
    // handoff interval without opening concurrent HTTP streams for one container.
    let Ok(boundary) = i32::try_from(now_ms().div_euclid(1000)) else {
        let _ = send_batch(
            &sender,
            &buffer,
            DockerLogBatch {
                generation,
                lines: Vec::new(),
                truncated: false,
                complete: true,
                error: Some("Docker log timestamp is outside the supported API range".into()),
            },
        );
        return;
    };
    let mut overlap = BTreeMap::<[u8; 32], usize>::new();
    let mut follow = false;
    loop {
        let mut stream = Box::pin(adapter.logs_window(&request.container_id, follow, boundary));
        let mut parser = PlainLines::default();
        let mut rate_start = Instant::now();
        let mut bytes = 0usize;
        loop {
            tokio::select! {biased;
                _=control.changed()=>{if control.borrow().generation!=generation||control.borrow().shutdown{return;}},
                output=stream.next()=>{
                    match output{
                        Some(Ok(data))=>{
                            if rate_start.elapsed()>=Duration::from_secs(1){rate_start=Instant::now();bytes=0;}
                            bytes=bytes.saturating_add(data.len());
                            if bytes>2*1024*1024 {let _=send_batch(&sender,&buffer,DockerLogBatch{generation,lines:Vec::new(),truncated:true,complete:true,error:Some("Log rate exceeded the 2 MiB/s limit; reopen to retry".into())});return;}
                            let (mut lines,truncated)=parser.push(&data,false);
                            if follow { lines.retain(|line| { if let Some(count) = overlap.get_mut(&<[u8; 32]>::from(Sha256::digest(line.as_bytes()))) && *count > 0 { *count -= 1; false } else { true } }); }
                            if (!lines.is_empty()||truncated)&& send_batch(&sender,&buffer,DockerLogBatch{generation,lines,truncated,complete:false,error:None}).is_err(){return;}
                        },
                        other=>{
                            let (lines,truncated)=parser.push(&[],true);
                            let failed = other.is_some();
                            let _=send_batch(&sender,&buffer,DockerLogBatch{generation,lines,truncated:truncated||failed,complete:follow||failed,error:other.and_then(|e|e.err()).map(|_|"Docker logs unavailable or stream byte limit reached; reopen to retry".into())});
                            if follow || failed { return; }
                            break;
                        }
                    }
                }
            }
        }
        drop(stream);
        {
            let mut cache = buffer.lock_ok();
            cache.tail_complete = true;
            for line in &cache.lines {
                *overlap
                    .entry(Sha256::digest(line.as_bytes()).into())
                    .or_default() += 1;
            }
            let _ = sender.send(LogDelivery {
                tail_complete: true,
                batch: DockerLogBatch {
                    generation,
                    lines: Vec::new(),
                    truncated: false,
                    complete: false,
                    error: None,
                },
            });
        }
        while !*followers.borrow_and_update() {
            tokio::select! {biased;
                _=control.changed()=>{if control.borrow().generation!=generation||control.borrow().shutdown{return;}},
                changed=followers.changed()=>{if changed.is_err(){return;}}
            }
        }
        follow = true;
    }
}
/// Stateful terminal-control removal handles sequences split between HTTP chunks.
#[derive(Default)]
struct PlainLines {
    line: Vec<u8>,
    escape: u8,
    truncated: bool,
}
impl PlainLines {
    fn push(&mut self, data: &[u8], finish: bool) -> (Vec<String>, bool) {
        let mut lines = Vec::new();
        let mut truncated = false;
        for &b in data {
            match self.escape {
                1 => {
                    self.escape = match b {
                        b'[' => 2,
                        b']' => 3,
                        _ => 0,
                    };
                    continue;
                }
                2 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.escape = 0;
                    }
                    continue;
                }
                3 => {
                    if b == 7 {
                        self.escape = 0;
                    } else if b == 27 {
                        self.escape = 4;
                    }
                    continue;
                }
                4 => {
                    self.escape = if b == b'\\' { 0 } else { 3 };
                    continue;
                }
                _ => {}
            }
            if b == 27 {
                self.escape = 1;
                continue;
            }
            if b == b'\n' {
                lines.push(self.finish_line());
                truncated |= self.truncated;
                self.truncated = false;
            } else if b == b'\t' || b >= 32 {
                if self.line.len() < 65536 {
                    self.line.push(b);
                } else {
                    self.truncated = true;
                }
            }
            if lines.len() >= 2000 {
                truncated = true;
                break;
            }
        }
        if finish && !self.line.is_empty() {
            lines.push(self.finish_line());
            truncated |= self.truncated;
            self.truncated = false;
        }
        (lines, truncated)
    }
    fn finish_line(&mut self) -> String {
        let mut line = String::from_utf8_lossy(&std::mem::take(&mut self.line)).into_owned();
        if self.truncated {
            line.push_str(" [line truncated]");
        }
        line
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_split_ansi_and_osc_and_bounds_lines() {
        let mut p = PlainLines::default();
        assert!(p.push(b"hello\x1b[3", false).0.is_empty());
        assert_eq!(
            p.push(b"1m red\x1b[0m\x1b]0;bad title\x07\n", false).0,
            vec!["hello red"]
        );
        let data = vec![b'a'; 70000];
        p.push(&data, false);
        let (lines, cut) = p.push(b"\n", false);
        assert!(cut);
        assert!(lines[0].ends_with("[line truncated]"));
        assert!(lines[0].len() < 65600);
    }
}

#[derive(Default)]
struct LogBuffer {
    lines: VecDeque<String>,
    bytes: usize,
    truncated: bool,
    complete: bool,
    tail_complete: bool,
    error: Option<String>,
}
fn send_batch(
    sender: &broadcast::Sender<LogDelivery>,
    buffer: &Mutex<LogBuffer>,
    batch: DockerLogBatch,
) -> Result<(), ()> {
    let mut buffer = buffer.lock_ok();
    for line in &batch.lines {
        buffer.bytes += line.len() + std::mem::size_of::<String>();
        buffer.lines.push_back(line.clone());
    }
    while buffer.lines.len() > 2000 || buffer.bytes > 2 * 1024 * 1024 {
        if let Some(line) = buffer.lines.pop_front() {
            buffer.bytes = buffer
                .bytes
                .saturating_sub(line.len() + std::mem::size_of::<String>());
            buffer.truncated = true;
        } else {
            break;
        }
    }
    buffer.truncated |= batch.truncated;
    buffer.complete = batch.complete;
    buffer.error = batch.error.clone();
    let mut lines = Vec::new();
    let mut bytes = 0;
    for line in batch.lines {
        if bytes + line.len() > 65536 && !lines.is_empty() {
            sender
                .send(LogDelivery {
                    tail_complete: false,
                    batch: DockerLogBatch {
                        generation: batch.generation,
                        lines: std::mem::take(&mut lines),
                        truncated: batch.truncated,
                        complete: false,
                        error: None,
                    },
                })
                .map_err(|_| ())?;
            bytes = 0;
        }
        bytes += line.len();
        lines.push(line);
    }
    sender
        .send(LogDelivery {
            tail_complete: false,
            batch: DockerLogBatch {
                generation: batch.generation,
                lines,
                truncated: batch.truncated,
                complete: batch.complete,
                error: batch.error,
            },
        })
        .map_err(|_| ())?;
    Ok(())
}
