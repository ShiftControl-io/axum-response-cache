//! This library provides [Axum middleware](`axum#middleware`) that caches HTTP responses to the
//! incoming requests based on their HTTP method and path.
//!
//! The main struct is [`CacheLayer`]. It can be created with any cache that implements two traits
//! from the [`cached`] crate: [`cached::Cached`] and [`cached::CloneCached`].
//!
//! The *current* version of [`CacheLayer`] is compatible only with services accepting
//! Axum’s [`Request<Body>`](`http::Request<axum::body::Body>`) and returning
//! [`axum::response::Response`], thus it is not compatible with non-Axum [`tower`] services.
//!
//! It’s possible to configure the layer to re-use an old expired response in case the wrapped
//! service fails to produce a new successful response.
//!
//! Only successful responses are cached (responses with status codes outside of the `[200-299]`
//! range are passed-through or ignored).
//!
//! The cache limits maximum size of the response’s body (128 MB by default).
//!
//! ## Examples
//!
//! To cache a response over a specific route, just wrap it in a [`CacheLayer`]:
//!
//! ```rust,no_run
//! use axum::{Router, extract::Path, routing::get};
//! use axum_response_cache::CacheLayer;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut router = Router::new()
//!         .route(
//!             "/hello/:name",
//!             get(|Path(name): Path<String>| async move { format!("Hello, {name}!") })
//!                 // this will cache responses with each `:name` for 60 seconds.
//!                 .layer(CacheLayer::with_lifespan(60)),
//!         );
//!
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
//!     axum::serve(listener, router).await.unwrap();
//! }
//! ```
//!
//! ### Reusing last successful response
//! ```rust
//! # use std::sync::atomic::{AtomicBool, Ordering};
//! use axum::{
//!     body::Body,
//!     extract::Path,
//!     http::status::StatusCode,
//!     http::Request,
//!     Router,
//!     routing::get,
//! };
//! use axum_response_cache::CacheLayer;
//! use tower::Service as _;
//!
//! // a handler that returns 200 OK only the first time it’s called
//! async fn handler(Path(name): Path<String>) -> (StatusCode, String) {
//!     static FIRST_RUN: AtomicBool = AtomicBool::new(true);
//!     let first_run = FIRST_RUN.swap(false, Ordering::AcqRel);
//!
//!     if first_run {
//!         (StatusCode::OK, format!("Hello, {name}"))
//!     } else {
//!         (StatusCode::INTERNAL_SERVER_ERROR, String::from("Error!"))
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut router = Router::new()
//!     .route("/hello/:name", get(handler))
//!     .layer(CacheLayer::with_lifespan(60).use_stale_on_failure());
//!
//! // first request will fire handler and get the response
//! let status1 = router.call(Request::get("/hello/foo").body(Body::empty()).unwrap())
//!     .await
//!     .unwrap()
//!     .status();
//! assert_eq!(StatusCode::OK, status1);
//!
//! // second request will reuse the last response since the handler now returns ISE
//! let status2 = router.call(Request::get("/hello/foo").body(Body::empty()).unwrap())
//!     .await
//!     .unwrap()
//!     .status();
//! assert_eq!(StatusCode::OK, status2);
//! # }
//! ```
//!
//! ### Serving static files
//! This middleware can be used to cache files served in memory to limit hard drive load on the
//! server. To serve files you can use [`tower-http::services::ServeDir`](https://docs.rs/tower-http/latest/tower_http/services/struct.ServeDir.html) layer.
//! ```rust,ignore
//! let router = Router::new().nest_service("/", ServeDir::new("static/"));
//! ```
//!
//! ### Limiting the body size
//! ```rust
//! use axum::{
//!     body::Body,
//!     extract::Path,
//!     http::status::StatusCode,
//!     http::Request,
//!     Router,
//!     routing::get,
//! };
//! use axum_response_cache::CacheLayer;
//! use tower::Service as _;
//!
//! // returns a short string, well below the limit
//! async fn ok_handler() -> &'static str {
//!     "ok"
//! }
//!
//! async fn too_long_handler() -> &'static str {
//!     "a response that is well beyond the limit of the cache!"
//! }
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut router = Router::new()
//!     .route("/ok", get(ok_handler))
//!     .route("/too_long", get(too_long_handler))
//!     // limit max cached body to only 16 bytes
//!     .layer(CacheLayer::with_lifespan(60).body_limit(16));
//!
//! let status_ok = router.call(Request::get("/ok").body(Body::empty()).unwrap())
//!     .await
//!     .unwrap()
//!     .status();
//! assert_eq!(StatusCode::OK, status_ok);
//!
//! let status_too_long = router.call(Request::get("/too_long").body(Body::empty()).unwrap())
//!     .await
//!     .unwrap()
//!     .status();
//! assert_eq!(StatusCode::INTERNAL_SERVER_ERROR, status_too_long);
//! # }
//! ```
//!
//! ## Using custom cache
//!
//! ```rust
//! use axum::{Router, routing::get};
//! use axum_response_cache::CacheLayer;
//! // let’s use TimedSizedCache here
//! use cached::stores::TimedSizedCache;
//! # use axum::{body::Body, http::Request};
//! # use tower::ServiceExt;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let router: Router = Router::new()
//!     .route("/hello", get(|| async { "Hello, world!" }))
//!     // cache maximum value of 50 responses for one minute
//!     .layer(CacheLayer::with(TimedSizedCache::with_size_and_lifespan(50, 60)));
//! # // force type inference to resolve the exact type of router
//! #     let _ = router.oneshot(Request::get("/hello").body(Body::empty()).unwrap()).await;
//! # }
//! ```
//!
//! ## Use cases
//! Caching responses in memory (eg. using [`cached::TimedCache`]) might be useful when the
//! underlying service produces the responses by:
//! 1. doing heavy computation,
//! 2. requesting external service(s) that might not be fully reliable or performant,
//! 3. serving static files from disk.
//!
//! In those cases, if the response to identical requests does not change often over time, it might
//! be desirable to re-use the same responses from memory without re-calculating them – skipping requests to data
//! bases, external services, reading from disk.

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tracing_futures::Instrument as _;

use axum::{
    body::{Body, Bytes},
    http::{response::Parts, Request, StatusCode},
    response::{IntoResponse, Response},
};
use cached::{Cached, CloneCached, TimedCache};
use tower::{Layer, Service};
use tracing::{debug, instrument};

/// The caching key for the responses.
///
/// The responses are cached according to the HTTP method [`axum::http::Method`]) and path
/// ([`axum::http::Uri`]) of the request they responded to.
type Key = (axum::http::Method, axum::http::Uri);

/// The struct preserving all the headers and body of the cached response.
#[derive(Clone, Debug)]
pub struct CachedResponse {
    parts: Parts,
    body: Bytes,
}

impl IntoResponse for CachedResponse {
    fn into_response(self) -> Response {
        Response::from_parts(self.parts, Body::from(self.body))
    }
}

/// The main struct of the library. The layer providing caching to the wrapped service.
#[derive(Clone)]
pub struct CacheLayer<C> {
    cache: Arc<Mutex<C>>,
    use_stale: bool,
    limit: usize,
}

impl<C> CacheLayer<C>
where
    C: Cached<Key, CachedResponse> + CloneCached<Key, CachedResponse>,
{
    /// Create a new cache layer with a given cache and the default body size limit of 128 MB.
    pub fn with(cache: C) -> Self {
        Self {
            cache: Arc::new(Mutex::new(cache)),
            use_stale: false,
            limit: 128 * 1024 * 1024,
        }
    }

    /// Switch the layer’s settings to preserve the last successful response even when it’s evicted
    /// from the cache but the service failed to provide a new successful response (ie. eg. when
    /// the underlying service responds with `404 NOT FOUND`, the cache will keep providing the last stale `200 OK`
    /// response produced).
    pub fn use_stale_on_failure(self) -> Self {
        Self {
            use_stale: true,
            ..self
        }
    }

    /// Change the maximum body size limit. If you want unlimited size, use [`usize::MAX`].
    pub fn body_limit(self, new_limit: usize) -> Self {
        Self {
            limit: new_limit,
            ..self
        }
    }
}

impl CacheLayer<TimedCache<Key, CachedResponse>> {
    /// Create a new cache layer with the desired TTL in seconds
    pub fn with_lifespan(ttl_sec: u64) -> CacheLayer<TimedCache<Key, CachedResponse>> {
        CacheLayer::with(TimedCache::with_lifespan(ttl_sec))
    }
}

impl<S, C> Layer<S> for CacheLayer<C> {
    type Service = CacheService<S, C>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service {
            inner,
            cache: Arc::clone(&self.cache),
            use_stale: self.use_stale,
            limit: self.limit,
        }
    }
}

#[derive(Clone)]
pub struct CacheService<S, C> {
    inner: S,
    cache: Arc<Mutex<C>>,
    use_stale: bool,
    limit: usize,
}

impl<S, C> Service<Request<Body>> for CacheService<S, C>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send,
    S::Future: Send + 'static,
    C: Cached<Key, CachedResponse> + CloneCached<Key, CachedResponse> + Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send + 'static>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    #[instrument(skip(self, request))]
    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let use_stale = self.use_stale;
        let limit = self.limit;
        let cache = Arc::clone(&self.cache);
        let key = (request.method().clone(), request.uri().clone());
        let inner_fut = inner
            .call(request)
            .instrument(tracing::info_span!("inner_service"));
        let (cached, evicted) = {
            let mut guard = cache.lock().unwrap();
            let (cached, evicted) = guard.cache_get_expired(&key);
            if let (Some(stale), true) = (cached.as_ref(), evicted) {
                // reinsert stale value immediately so that others don’t schedule their updating
                debug!("Found stale value in cache, reinsterting and attempting refresh");
                guard.cache_set(key.clone(), stale.clone());
            }
            (cached, evicted)
        };

        Box::pin(async move {
            match (cached, evicted) {
                (Some(value), false) => Ok(value.into_response()),
                (Some(stale_value), true) => {
                    let response = inner_fut.await.unwrap();
                    if response.status().is_success() {
                        Ok(update_cache(&cache, key, response, limit).await)
                    } else if use_stale {
                        debug!("Returning stale value.");
                        Ok(stale_value.into_response())
                    } else {
                        debug!("Stale value in cache, evicting and returning failed response.");
                        cache.lock().unwrap().cache_remove(&key);
                        Ok(response)
                    }
                }
                (None, _) => {
                    let response = inner_fut.await.unwrap();
                    if response.status().is_success() {
                        Ok(update_cache(&cache, key, response, limit).await)
                    } else {
                        Ok(response)
                    }
                }
            }
        })
    }
}

#[instrument(skip(cache, response))]
async fn update_cache<C: Cached<Key, CachedResponse> + CloneCached<Key, CachedResponse>>(
    cache: &Arc<Mutex<C>>,
    key: Key,
    response: Response,
    limit: usize,
) -> Response {
    let (parts, body) = response.into_parts();
    let Ok(body) = axum::body::to_bytes(body, limit).await else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("File too big, over {limit} bytes"),
        )
            .into_response();
    };
    let value = CachedResponse { parts, body };
    {
        cache.lock().unwrap().cache_set(key, value.clone());
    }
    value.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use axum::{
        extract::State,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use tower::Service;

    #[derive(Clone, Debug)]
    struct Counter {
        value: Arc<AtomicIsize>,
    }

    impl Counter {
        fn new(init: isize) -> Self {
            Self {
                value: AtomicIsize::from(init).into(),
            }
        }

        fn increment(&self) {
            self.value.fetch_add(1, Ordering::Release);
        }

        fn read(&self) -> isize {
            self.value.load(Ordering::Acquire)
        }
    }

    #[tokio::test]
    async fn should_use_cached_value() {
        let handler = |State(cnt): State<Counter>| async move {
            cnt.increment();
            StatusCode::OK
        };

        let counter = Counter::new(0);
        let cache = CacheLayer::with_lifespan(60).use_stale_on_failure();
        let mut router = Router::new()
            .route("/", get(handler).layer(cache))
            .with_state(counter.clone());

        for _ in 0..10 {
            let status = router
                .call(Request::get("/").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status();
            assert!(status.is_success(), "handler should return success");
        }

        assert_eq!(1, counter.read(), "handler should’ve been called only once");
    }

    #[tokio::test]
    async fn should_not_cache_unsuccessful_responses() {
        let handler = |State(cnt): State<Counter>| async move {
            cnt.increment();
            let responses = [
                StatusCode::BAD_REQUEST,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::NOT_FOUND,
            ];
            let mut rng = rand::thread_rng();
            responses[rng.gen_range(0..responses.len())]
        };

        let counter = Counter::new(0);
        let cache = CacheLayer::with_lifespan(60).use_stale_on_failure();
        let mut router = Router::new()
            .route("/", get(handler).layer(cache))
            .with_state(counter.clone());

        for _ in 0..10 {
            let status = router
                .call(Request::get("/").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status();
            assert!(!status.is_success(), "handler should never return success");
        }

        assert_eq!(
            10,
            counter.read(),
            "handler should’ve been called for all requests"
        );
    }

    #[tokio::test]
    async fn should_use_last_correct_stale_value() {
        let handler = |State(cnt): State<Counter>| async move {
            let prev = cnt.value.fetch_add(1, Ordering::AcqRel);
            let responses = [
                StatusCode::BAD_REQUEST,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::NOT_FOUND,
            ];
            let mut rng = rand::thread_rng();

            // first response successful, later failed
            if prev == 0 {
                StatusCode::OK
            } else {
                responses[rng.gen_range(0..responses.len())]
            }
        };

        let counter = Counter::new(0);
        let cache = CacheLayer::with_lifespan(1).use_stale_on_failure();
        let mut router = Router::new()
            .route("/", get(handler).layer(cache))
            .with_state(counter);

        // feed the cache
        let status = router
            .call(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status();
        assert!(status.is_success(), "handler should return success");

        // wait over 1s for cache eviction
        tokio::time::sleep(tokio::time::Duration::from_millis(1050)).await;

        for _ in 1..10 {
            let status = router
                .call(Request::get("/").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status();
            assert!(
                status.is_success(),
                "cache should return stale successful value"
            );
        }
    }

    #[tokio::test]
    async fn should_not_use_stale_values() {
        let handler = |State(cnt): State<Counter>| async move {
            let prev = cnt.value.fetch_add(1, Ordering::AcqRel);
            let responses = [
                StatusCode::BAD_REQUEST,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::NOT_FOUND,
            ];
            let mut rng = rand::thread_rng();

            // first response successful, later failed
            if prev == 0 {
                StatusCode::OK
            } else {
                responses[rng.gen_range(0..responses.len())]
            }
        };

        let counter = Counter::new(0);
        let cache = CacheLayer::with_lifespan(1);
        let mut router = Router::new()
            .route("/", get(handler).layer(cache))
            .with_state(counter.clone());

        // feed the cache
        let status = router
            .call(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status();
        assert!(status.is_success(), "handler should return success");

        // wait over 1s for cache eviction
        tokio::time::sleep(tokio::time::Duration::from_millis(1050)).await;

        for _ in 1..10 {
            let status = router
                .call(Request::get("/").body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status();
            assert!(
                !status.is_success(),
                "cache should forward unsuccessful values"
            );
        }

        assert_eq!(
            10,
            counter.read(),
            "handler should’ve been called for all requests"
        );
    }
}