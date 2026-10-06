use super::RuntimeSnapshot;
use std::sync::Arc;

pub fn retain_runtime(body: axum::body::Body, runtime: Arc<RuntimeSnapshot>) -> axum::body::Body {
    axum::body::Body::new(RuntimeBody {
        body,
        _runtime: runtime,
    })
}

struct RuntimeBody {
    body: axum::body::Body,
    _runtime: Arc<RuntimeSnapshot>,
}

impl hyper::body::Body for RuntimeBody {
    type Data = hyper::body::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.get_mut().body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.body.size_hint()
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests;
