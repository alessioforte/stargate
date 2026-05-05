use std::{
    collections::HashMap,
    ffi::OsString,
    io,
    sync::{Mutex, MutexGuard, OnceLock},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(crate) struct ScopedEnv {
    key: &'static str,
    previous: Option<OsString>,
}

impl ScopedEnv {
    pub(crate) fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        // Tests serialize env access through `env_lock`, so this is safe here.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => {
                // Tests serialize env access through `env_lock`, so this is safe here.
                unsafe { std::env::set_var(self.key, value) };
            }
            None => {
                // Tests serialize env access through `env_lock`, so this is safe here.
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct TestRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Vec<u8>,
}

impl TestRequest {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

pub(crate) async fn spawn_json_server(
    path: &'static str,
    response_body: &'static str,
) -> io::Result<(String, oneshot::Receiver<TestRequest>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (request_tx, request_rx) = oneshot::channel();

    tokio::spawn(async move {
        let result = async {
            let (mut stream, _) = listener.accept().await?;
            let request = read_request(&mut stream).await?;
            let body = response_body.as_bytes();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            let _ = request_tx.send(request);
            Ok::<(), io::Error>(())
        }
        .await;

        if let Err(error) = result {
            tracing::error!(%error, "oauth test server failed");
        }
    });

    Ok((format!("http://{}{}", addr, path), request_rx))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> io::Result<TestRequest> {
    let mut buffer = Vec::new();
    let headers_end = loop {
        let mut chunk = [0_u8; 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request headers were complete",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);

        if let Some(position) = find_headers_end(&buffer) {
            break position;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..headers_end]);
    let mut lines = headers.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut request_line_parts = request_line.split_whitespace();
    let method = request_line_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request method"))?;
    let path = request_line_parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request path"))?;

    let mut header_map = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        header_map.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    let expected_len = header_map
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[(headers_end + 4)..].to_vec();

    while body.len() < expected_len {
        let mut chunk = vec![0_u8; expected_len - body.len()];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request body was complete",
            ));
        }
        body.extend_from_slice(&chunk[..read]);
    }

    Ok(TestRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers: header_map,
        body,
    })
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
