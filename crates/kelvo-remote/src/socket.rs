use crate::{RemoteError, Result};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::watch,
};

#[derive(Clone, Debug)]
pub struct SocketPaths {
    pub control: PathBuf,
    pub metrics: PathBuf,
}
impl SocketPaths {
    pub fn new(app_data: impl AsRef<Path>) -> Self {
        let dir = app_data.as_ref().join("remote");
        Self {
            control: dir.join("control.sock"),
            metrics: dir.join("metrics.sock"),
        }
    }
    pub fn for_current_user() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| RemoteError::Protocol("HOME unavailable".into()))?;
        Ok(Self::new(
            PathBuf::from(home).join("Library/Application Support/com.tryopendata.kelvo"),
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ControlRequest {
    Status,
    Configure {
        allow_remote_monitoring: bool,
        show_menu_bar_icon: Option<bool>,
        launch_at_login: Option<bool>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
    pub host: Option<kelvo_schema::HostRecord>,
    #[serde(default)]
    pub allow_remote_monitoring: Option<bool>,
    #[serde(default)]
    pub show_menu_bar_icon: Option<bool>,
    #[serde(default)]
    pub launch_at_login: Option<bool>,
}

pub(crate) fn bind_private(path: &Path) -> Result<UnixListener> {
    let parent = path
        .parent()
        .ok_or_else(|| RemoteError::Protocol("socket needs parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let meta = std::fs::symlink_metadata(parent)?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid {
        return Err(RemoteError::Protocol(
            "socket directory must be owned by this user".into(),
        ));
    }
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        if !meta.file_type().is_socket() || meta.uid() != uid {
            return Err(RemoteError::Protocol("unsafe existing socket path".into()));
        }
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(RemoteError::Protocol("socket already serving".into()));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}
pub(crate) fn same_uid(stream: &UnixStream) -> Result<()> {
    let credentials = stream.peer_cred()?;
    // SAFETY: geteuid has no preconditions.
    if credentials.uid() != unsafe { libc::geteuid() } {
        return Err(RemoteError::Authentication(
            "socket peer UID differs".into(),
        ));
    }
    Ok(())
}
pub(crate) struct SocketCleanup(pub PathBuf);
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub async fn serve_control(
    path: PathBuf,
    handler: Arc<dyn Fn(ControlRequest) -> ControlResponse + Send + Sync>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let listener = bind_private(&path)?;
    let _cleanup = SocketCleanup(path);
    let mut sessions = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _=shutdown.changed()=>break,
            accepted=listener.accept()=>{
                let (mut socket,_)=accepted?; if same_uid(&socket).is_err(){continue;} let handler=handler.clone();
                if sessions.len()>=16 {continue;}
                sessions.spawn(async move {
                    let _=tokio::time::timeout(std::time::Duration::from_secs(10),async move {
                        let size=socket.read_u32().await? as usize;
                        if size>16*1024 {return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"control request too large"));}
                        let mut bytes=vec![0;size];socket.read_exact(&mut bytes).await?;
                        let request:ControlRequest=serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                        let reply=tokio::task::spawn_blocking(move||handler(request)).await.map_err(std::io::Error::other)?;let bytes=serde_json::to_vec(&reply).map_err(std::io::Error::other)?;
                        socket.write_u32(bytes.len() as u32).await?;socket.write_all(&bytes).await?;
                        socket.shutdown().await
                    }).await;
                });
            },
            _=sessions.join_next(),if !sessions.is_empty()=>{}
        }
    }
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
    Ok(())
}
pub async fn control_request(path: &Path, request: &ControlRequest) -> Result<ControlResponse> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut stream = UnixStream::connect(path).await?;
        same_uid(&stream)?;
        let bytes =
            serde_json::to_vec(request).map_err(|e| RemoteError::Protocol(e.to_string()))?;
        stream.write_u32(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        let len = stream.read_u32().await? as usize;
        if len > 65536 {
            return Err(RemoteError::Protocol("control response too large".into()));
        }
        let mut bytes = vec![0; len];
        stream.read_exact(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(|e| RemoteError::Protocol(e.to_string()))
    })
    .await
    .map_err(|_| RemoteError::Transport("control request timeout".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn control_is_private_and_roundtrips() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let paths = SocketPaths::new(dir.path());
        let (tx, rx) = watch::channel(false);
        let path = paths.control.clone();
        let task = tokio::spawn(serve_control(
            path,
            Arc::new(|_| ControlResponse {
                ok: true,
                message: "running".into(),
                host: None,
                allow_remote_monitoring: None,
                show_menu_bar_icon: None,
                launch_at_login: None,
            }),
            rx,
        ));
        for _ in 0..30 {
            if paths.control.exists() {
                break;
            }
            if task.is_finished() {
                panic!("server failed: {:?}", task.await);
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let reply = control_request(&paths.control, &ControlRequest::Status)
            .await
            .unwrap();
        assert!(reply.ok);
        assert_eq!(
            std::fs::metadata(&paths.control)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        tx.send(true).unwrap();
        task.await.unwrap().unwrap();
        assert!(!paths.control.exists());
    }
}
