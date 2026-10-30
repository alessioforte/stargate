use super::*;
use futures_util::task::AtomicWaker;
use gate::cfg::RuntimeSettings;
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
};
use tokio_util::sync::CancellationToken;

type Socket = WebSocketStream<TcpStream>;

async fn socket_pair() -> (Socket, Socket) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/socket", listener.local_addr().unwrap());
    let (client, server) = tokio::join!(
        async {
            tokio_tungstenite::client_async(
                url,
                TcpStream::connect(listener.local_addr().unwrap())
                    .await
                    .unwrap(),
            )
            .await
            .unwrap()
            .0
        },
        async {
            tokio_tungstenite::accept_async(listener.accept().await.unwrap().0)
                .await
                .unwrap()
        },
    );
    (client, server)
}

struct Session {
    client: Socket,
    backend: Socket,
    resources: Arc<ProcessResources>,
    ended: oneshot::Receiver<bool>,
    available: usize,
}

impl Session {
    async fn start(idle: Duration) -> Self {
        Self::start_with(idle, None).await
    }

    /// A stream with an explicit router request deadline, kept after the handshake.
    async fn start_with_request_deadline(idle: Duration, request: Duration) -> Self {
        Self::start_with(idle, Some(request)).await
    }

    async fn start_with(idle: Duration, request: Option<Duration>) -> Self {
        let (client, mut downstream) = socket_pair().await;
        let (mut upstream, backend) = socket_pair().await;
        let resources =
            ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
        let available = resources.available().0;
        let admission = resources.admit_primary().unwrap();
        let mut execution = Execution::new(
            request.unwrap_or(Duration::from_secs(60)),
            admission.cancellation.clone(),
        );
        execution.stream = true;
        execution.preserve_stream_deadline = request.is_some();
        let execution = execution.response();
        let (done, ended) = oneshot::channel();
        let owned = resources.clone();
        assert!(resources.spawn(async move {
            let result =
                relay_sockets(&mut downstream, &mut upstream, &execution, &owned, idle).await;
            drop(admission);
            let _ = done.send(result.is_ok());
        }));
        Self {
            client,
            backend,
            resources,
            ended,
            available,
        }
    }

    async fn finish(self, success: bool) {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), self.ended)
                .await
                .unwrap()
                .unwrap(),
            success
        );
        assert_eq!(self.resources.available().0, self.available);
        self.resources.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(1), self.resources.wait())
            .await
            .unwrap();
    }
}

async fn receive_close<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
) -> CloseFrame {
    match tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    {
        Message::Close(Some(frame)) => frame,
        other => panic!("expected Close, got {other:?}"),
    }
}

async fn acknowledge<S: AsyncRead + AsyncWrite + Unpin>(socket: &mut WebSocketStream<S>) {
    // Reading Close queues tungstenite's reply; flushing sends it to the relay.
    match socket.flush().await {
        Ok(()) | Err(Error::ConnectionClosed) => {}
        Err(error) => panic!("close acknowledgement failed: {error}"),
    }
}

#[tokio::test]
async fn shutdown_sends_going_away_and_completes_both_handshakes() {
    let mut session = Session::start(Duration::from_secs(60)).await;
    session
        .client
        .send(Message::Text("ready".into()))
        .await
        .unwrap();
    assert_eq!(
        session.backend.next().await.unwrap().unwrap(),
        Message::Text("ready".into())
    );
    session.resources.begin_shutdown();
    assert_eq!(
        receive_close(&mut session.client).await,
        close_frame(1001, "gateway shutting down")
    );
    assert_eq!(
        receive_close(&mut session.backend).await,
        close_frame(1001, "gateway shutting down")
    );
    acknowledge(&mut session.client).await;
    acknowledge(&mut session.backend).await;
    // A client endpoint sees ConnectionClosed only after the server completes
    // the exchange and closes TCP, not ResetWithoutClosingHandshake.
    assert!(session.client.next().await.is_none());
    session.finish(true).await;
}

#[tokio::test]
async fn idle_timeout_sends_going_away_with_reason_on_both_sides() {
    let mut session = Session::start(Duration::from_millis(50)).await;
    assert_eq!(
        receive_close(&mut session.client).await,
        close_frame(1001, "idle timeout")
    );
    assert_eq!(
        receive_close(&mut session.backend).await,
        close_frame(1001, "idle timeout")
    );
    acknowledge(&mut session.client).await;
    acknowledge(&mut session.backend).await;
    session.finish(true).await;
}

#[tokio::test]
async fn request_deadline_closes_with_its_own_reason() {
    let mut session =
        Session::start_with_request_deadline(Duration::from_secs(60), Duration::from_millis(50))
            .await;
    assert_eq!(
        receive_close(&mut session.client).await,
        close_frame(1001, "request timeout")
    );
    assert_eq!(
        receive_close(&mut session.backend).await,
        close_frame(1001, "request timeout")
    );
    acknowledge(&mut session.client).await;
    acknowledge(&mut session.backend).await;
    session.finish(true).await;
}

#[tokio::test]
async fn lost_backend_sends_internal_error_to_client() {
    let mut session = Session::start(Duration::from_secs(60)).await;
    session.backend.get_mut().shutdown().await.unwrap();
    // A standard decoder accepts the code: no protocol-violation rewrite.
    assert_eq!(
        receive_close(&mut session.client).await,
        close_frame(1011, "upstream connection lost")
    );
    acknowledge(&mut session.client).await;
    assert!(session.client.next().await.is_none());
    session.finish(false).await;
}

#[tokio::test]
async fn lost_client_sends_going_away_to_backend() {
    let mut session = Session::start(Duration::from_secs(60)).await;
    session.client.get_mut().shutdown().await.unwrap();
    assert_eq!(
        receive_close(&mut session.backend).await.code,
        CloseCode::Away
    );
    acknowledge(&mut session.backend).await;
    session.finish(false).await;
}

#[tokio::test]
async fn silent_client_is_bounded_by_close_timeout_and_releases_admission() {
    let mut session = Session::start(Duration::from_secs(60)).await;
    let started = Instant::now();
    session.resources.begin_shutdown();
    assert_eq!(
        receive_close(&mut session.client).await.code,
        CloseCode::Away
    );
    assert_eq!(
        receive_close(&mut session.backend).await.code,
        CloseCode::Away
    );
    acknowledge(&mut session.backend).await;
    // The client deliberately never flushes its queued Close reply.
    assert_eq!(session.resources.available().0, session.available - 1);
    tokio::time::timeout(
        WEBSOCKET_CLOSE_TIMEOUT + Duration::from_secs(1),
        session.resources.wait(),
    )
    .await
    .unwrap();
    assert!(started.elapsed() >= WEBSOCKET_CLOSE_TIMEOUT);
    assert!(started.elapsed() < WEBSOCKET_CLOSE_TIMEOUT + Duration::from_secs(1));
    session.finish(true).await;
}

#[tokio::test]
async fn silent_client_respects_the_shorter_server_shutdown_deadline() {
    let mut session = Session::start(Duration::from_secs(60)).await;
    let started = Instant::now();
    session
        .resources
        .begin_shutdown_with_timeout(Duration::from_millis(100));
    assert_eq!(
        receive_close(&mut session.client).await.code,
        CloseCode::Away
    );
    assert_eq!(
        receive_close(&mut session.backend).await.code,
        CloseCode::Away
    );
    acknowledge(&mut session.backend).await;
    session.finish(true).await;
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn shutdown_shortens_an_idle_close_already_in_progress() {
    let mut session = Session::start(Duration::from_millis(50)).await;
    assert_eq!(
        receive_close(&mut session.client).await.reason,
        "idle timeout"
    );
    assert_eq!(
        receive_close(&mut session.backend).await.reason,
        "idle timeout"
    );
    acknowledge(&mut session.backend).await;
    let started = Instant::now();
    session
        .resources
        .begin_shutdown_with_timeout(Duration::from_millis(100));
    session.finish(true).await;
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn received_close_is_forwarded_unchanged_in_either_direction() {
    for from_client in [true, false] {
        let mut session = Session::start(Duration::from_secs(60)).await;
        let frame = close_frame(4001, "peer finished");
        let (sender, receiver) = if from_client {
            (&mut session.client, &mut session.backend)
        } else {
            (&mut session.backend, &mut session.client)
        };
        sender.close(Some(frame.clone())).await.unwrap();
        assert_eq!(receive_close(receiver).await, frame);
        acknowledge(receiver).await;
        assert_eq!(receive_close(sender).await, frame);
        acknowledge(sender).await;
        session.finish(true).await;
    }
}

#[derive(Default)]
struct WriteGate {
    remaining: AtomicUsize,
    blocked: AtomicUsize,
    changed: Notify,
    waker: AtomicWaker,
}

impl WriteGate {
    fn open() -> Arc<Self> {
        Arc::new(Self {
            remaining: AtomicUsize::new(usize::MAX),
            ..Self::default()
        })
    }
    async fn wait_blocked(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.blocked.load(Ordering::SeqCst) < count {
                self.changed.notified().await;
            }
        })
        .await
        .unwrap();
    }
    fn release(&self) {
        self.remaining.store(usize::MAX, Ordering::SeqCst);
        self.waker.wake();
    }
}

struct LimitedTcp {
    inner: TcpStream,
    gate: Arc<WriteGate>,
}
impl AsyncRead for LimitedTcp {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}
impl AsyncWrite for LimitedTcp {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.gate.waker.register(cx.waker());
        let remaining = self.gate.remaining.load(Ordering::SeqCst);
        if remaining == 0 {
            self.gate.blocked.fetch_add(1, Ordering::SeqCst);
            self.gate.changed.notify_one();
            return Poll::Pending;
        }
        let result =
            Pin::new(&mut self.inner).poll_write(cx, &buffer[..buffer.len().min(remaining)]);
        if let Poll::Ready(Ok(count)) = result
            && remaining != usize::MAX
        {
            self.gate.remaining.fetch_sub(count, Ordering::SeqCst);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn limited_pair(
    server_is_limited: bool,
    gate: Arc<WriteGate>,
) -> (Socket, WebSocketStream<LimitedTcp>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (client, server) = tokio::join!(TcpStream::connect(address), async {
        listener.accept().await.unwrap().0
    },);
    let client = client.unwrap();
    if server_is_limited {
        let (client, server) = tokio::join!(
            tokio_tungstenite::client_async(format!("ws://{address}"), client),
            tokio_tungstenite::accept_async(LimitedTcp {
                inner: server,
                gate
            })
        );
        (client.unwrap().0, server.unwrap())
    } else {
        let (client, server) = tokio::join!(
            tokio_tungstenite::client_async(
                format!("ws://{address}"),
                LimitedTcp {
                    inner: client,
                    gate
                }
            ),
            tokio_tungstenite::accept_async(server)
        );
        (server.unwrap(), client.unwrap().0)
    }
}

#[tokio::test]
async fn cancelled_send_finishes_the_frame_before_close() {
    for (to_client, bytes) in [
        (true, 4096),
        (false, 4096),
        (true, 256 * 1024),
        (false, 256 * 1024),
    ] {
        let blocked = WriteGate::open();
        let open = WriteGate::open();
        let (mut client, mut downstream) = limited_pair(
            true,
            if to_client {
                blocked.clone()
            } else {
                open.clone()
            },
        )
        .await;
        let (mut backend, mut upstream) =
            limited_pair(false, if to_client { open } else { blocked.clone() }).await;
        blocked.remaining.store(7, Ordering::SeqCst);
        let resources =
            ProcessResources::new(RuntimeSettings::default().compile().unwrap().budgets);
        let cancellation = CancellationToken::new();
        let execution = Execution::new(Duration::from_secs(60), cancellation.clone());
        let relay = tokio::spawn(async move {
            relay_sockets(
                &mut downstream,
                &mut upstream,
                &execution,
                &resources,
                Duration::from_secs(30),
            )
            .await
            .unwrap()
        });
        let payload = vec![0x5a; bytes];
        let (sender, receiver) = if to_client {
            (&mut backend, &mut client)
        } else {
            (&mut client, &mut backend)
        };
        sender
            .send(Message::Binary(payload.clone().into()))
            .await
            .unwrap();
        blocked.wait_blocked(1).await;
        cancellation.cancel();
        blocked.wait_blocked(2).await;
        // The other leg closes while the partial write is still blocked.
        assert_eq!(
            receive_close(sender).await,
            close_frame(1001, "gateway shutting down")
        );
        acknowledge(sender).await;
        blocked.release();
        assert_eq!(
            receiver.next().await.unwrap().unwrap(),
            Message::Binary(payload.into())
        );
        assert_eq!(
            receive_close(receiver).await,
            close_frame(1001, "gateway shutting down")
        );
        acknowledge(receiver).await;
        tokio::time::timeout(Duration::from_secs(1), relay)
            .await
            .unwrap()
            .unwrap();
    }
}
