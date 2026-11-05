//! JSON-RPC IPC server listening over Unix domain sockets and TCP streams.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use encodercraft_core::EncoderError;
use encodercraft_dispatch::{IpcRequest, IpcResponse};

/// Trait implemented by command dispatchers and engine hosts to process incoming requests.
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, req: &IpcRequest) -> IpcResponse;

    /// Streaming methods (such as `queue.subscribe`) push notifications until `emit` returns `false`
    /// (the client went away). Returns `true` when the request was a streaming one.
    fn stream(&self, _req: &IpcRequest, _emit: &mut dyn FnMut(IpcResponse) -> bool) -> bool {
        false
    }
}

/// Headless IPC daemon server.
pub struct IpcServer {
    is_running: Arc<AtomicBool>,
    worker_thread: Option<JoinHandle<()>>,
    socket_path: Option<PathBuf>,
    #[cfg(unix)]
    socket_identity: Option<(u64, u64)>,
}

impl IpcServer {
    /// Binds and starts an IPC server listening on a TCP address (e.g. "127.0.0.1:8998").
    pub fn bind_tcp(addr: &str, handler: Arc<dyn IpcHandler>) -> Result<Self, EncoderError> {
        let listener = TcpListener::bind(addr).map_err(|e| EncoderError::Io(format!("failed to bind TCP server on {addr}: {e}")))?;

        listener.set_nonblocking(true).map_err(|e| EncoderError::Io(format!("failed to set nonblocking: {e}")))?;

        let is_running = Arc::new(AtomicBool::new(true));
        let running_flag = is_running.clone();

        let handle = thread::spawn(move || {
            while running_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let h = handler.clone();
                        thread::spawn(move || {
                            let _ = handle_tcp_client(stream, h);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            is_running,
            worker_thread: Some(handle),
            socket_path: None,
            #[cfg(unix)]
            socket_identity: None,
        })
    }

    /// Binds and starts an IPC server listening on a Unix domain socket.
    #[cfg(unix)]
    pub fn bind_unix(path: &Path, handler: Arc<dyn IpcHandler>) -> Result<Self, EncoderError> {
        use std::os::unix::net::{UnixListener, UnixStream};

        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

        match std::fs::symlink_metadata(path) {
            Ok(meta) => {
                if !meta.file_type().is_socket() {
                    return Err(EncoderError::Io(format!("refusing to replace non-socket path {}", path.display())));
                }
                match UnixStream::connect(path) {
                    Ok(_) => return Err(EncoderError::Io(format!("Unix socket is already active at {}", path.display()))),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                        let current = std::fs::symlink_metadata(path)
                            .map_err(|e| EncoderError::Io(format!("failed to inspect Unix socket {}: {e}", path.display())))?;
                        if current.dev() != meta.dev() || current.ino() != meta.ino() || !current.file_type().is_socket() {
                            return Err(EncoderError::Io(format!("Unix socket path changed while binding {}", path.display())));
                        }
                        std::fs::remove_file(path)
                            .map_err(|e| EncoderError::Io(format!("failed to remove stale Unix socket {}: {e}", path.display())))?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(EncoderError::Io(format!("failed to check Unix socket {}: {error}", path.display())));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(EncoderError::Io(format!("failed to inspect Unix socket {}: {error}", path.display()))),
        }

        // Bind under a private name, restrict to the owner (0600), then atomically rename into place
        // so the public path is never reachable with looser permissions.
        static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);
        let mut staging = path.as_os_str().to_owned();
        staging.push(format!(".{}.{}.tmp", std::process::id(), NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed)));
        let staging = PathBuf::from(staging);

        let listener =
            UnixListener::bind(&staging).map_err(|e| EncoderError::Io(format!("failed to bind Unix socket {}: {e}", staging.display())))?;
        if let Err(e) = listener.set_nonblocking(true) {
            let _ = std::fs::remove_file(&staging);
            return Err(EncoderError::Io(format!("failed to set nonblocking: {e}")));
        }
        let secured = std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600)).and_then(|()| std::fs::rename(&staging, path));
        if let Err(e) = secured {
            let _ = std::fs::remove_file(&staging);
            return Err(EncoderError::Io(format!("failed to secure Unix socket {}: {e}", path.display())));
        }

        let socket_meta =
            std::fs::symlink_metadata(path).map_err(|e| EncoderError::Io(format!("failed to inspect bound Unix socket {}: {e}", path.display())))?;
        let socket_identity = (socket_meta.dev(), socket_meta.ino());

        let is_running = Arc::new(AtomicBool::new(true));
        let running_flag = is_running.clone();
        let sock_path = path.to_path_buf();

        let handle = thread::spawn(move || {
            while running_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let h = handler.clone();
                        thread::spawn(move || {
                            let _ = handle_unix_client(stream, h);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { is_running, worker_thread: Some(handle), socket_path: Some(sock_path), socket_identity: Some(socket_identity) })
    }

    /// Stops the server and joins the listener thread.
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
        #[cfg(unix)]
        if let (Some(path), Some(identity)) = (&self.socket_path, self.socket_identity) {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};

            if let Ok(meta) = std::fs::symlink_metadata(path)
                && meta.file_type().is_socket()
                && (meta.dev(), meta.ino()) == identity
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_tcp_client(stream: TcpStream, handler: Arc<dyn IpcHandler>) -> Result<(), EncoderError> {
    let reader = stream.try_clone().map_err(|e| EncoderError::Io(e.to_string()))?;
    serve_client(reader, stream, handler.as_ref())
}

#[cfg(unix)]
fn handle_unix_client(stream: std::os::unix::net::UnixStream, handler: Arc<dyn IpcHandler>) -> Result<(), EncoderError> {
    let reader = stream.try_clone().map_err(|e| EncoderError::Io(e.to_string()))?;
    serve_client(reader, stream, handler.as_ref())
}

fn write_response(writer: &mut impl Write, resp: &IpcResponse) -> bool {
    let Ok(mut json) = serde_json::to_string(resp) else { return true };
    json.push('\n');
    writer.write_all(json.as_bytes()).and_then(|()| writer.flush()).is_ok()
}

/// Reads JSON lines and answers each one until the peer disconnects.
fn serve_client(reader: impl std::io::Read, mut writer: impl Write, handler: &dyn IpcHandler) -> Result<(), EncoderError> {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 {
            break;
        }

        match serde_json::from_str::<IpcRequest>(&line) {
            Ok(req) => {
                let mut alive = true;
                if !handler.stream(&req, &mut |resp| {
                    alive = write_response(&mut writer, &resp);
                    alive
                }) {
                    alive = write_response(&mut writer, &handler.handle(&req));
                }
                if !alive {
                    break;
                }
            }
            Err(e) => {
                if !write_response(&mut writer, &IpcResponse::error(0, format!("malformed request: {e}"))) {
                    break;
                }
            }
        }

        line.clear();
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Noop;
    impl IpcHandler for Noop {
        fn handle(&self, req: &IpcRequest) -> IpcResponse {
            IpcResponse::success(req.id, serde_json::json!({}))
        }
    }

    #[test]
    fn unix_socket_is_owner_only_and_staging_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ec.sock");
        let server = IpcServer::bind_unix(&path, Arc::new(Noop)).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1);
        drop(server);
        assert!(!path.exists());
    }

    #[test]
    fn stale_socket_is_replaced_but_regular_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ec.sock");
        let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
        drop(stale);
        assert!(path.exists());
        let server = IpcServer::bind_unix(&path, Arc::new(Noop)).unwrap();
        drop(server);

        std::fs::write(&path, b"precious").unwrap();
        assert!(IpcServer::bind_unix(&path, Arc::new(Noop)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"precious");
    }

    #[test]
    fn active_socket_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ec.sock");
        let server = IpcServer::bind_unix(&path, Arc::new(Noop)).unwrap();
        let identity = {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::symlink_metadata(&path).unwrap();
            (meta.dev(), meta.ino())
        };

        assert!(IpcServer::bind_unix(&path, Arc::new(Noop)).is_err());
        let meta = std::fs::symlink_metadata(&path).unwrap();
        use std::os::unix::fs::MetadataExt;
        assert_eq!((meta.dev(), meta.ino()), identity);
        drop(server);
    }

    #[test]
    fn stopping_server_does_not_remove_replacement_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ec.sock");
        let server = IpcServer::bind_unix(&path, Arc::new(Noop)).unwrap();
        std::fs::remove_file(&path).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();

        drop(server);
        assert!(path.exists());
        drop(replacement);
    }
}
