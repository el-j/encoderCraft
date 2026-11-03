//! JSON-RPC IPC server listening over Unix domain sockets and TCP streams.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

        Ok(Self { is_running, worker_thread: Some(handle), socket_path: None })
    }

    /// Binds and starts an IPC server listening on a Unix domain socket.
    #[cfg(unix)]
    pub fn bind_unix(path: &Path, handler: Arc<dyn IpcHandler>) -> Result<Self, EncoderError> {
        use std::os::unix::net::UnixListener;

        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        let listener = UnixListener::bind(path).map_err(|e| EncoderError::Io(format!("failed to bind Unix socket {}: {e}", path.display())))?;

        listener.set_nonblocking(true).map_err(|e| EncoderError::Io(format!("failed to set nonblocking: {e}")))?;

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

        Ok(Self { is_running, worker_thread: Some(handle), socket_path: Some(sock_path) })
    }

    /// Stops the server and joins the listener thread.
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_thread.take() {
            let _ = handle.join();
        }
        if let Some(ref path) = self.socket_path
            && path.exists()
        {
            let _ = std::fs::remove_file(path);
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
