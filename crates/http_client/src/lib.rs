use std::time::Duration;

use async_compat::{Compat, CompatExt};
use bytes::Bytes;
use futures::Stream;
use http::HeaderValue;
pub use http::header::AUTHORIZATION;
use http::header::HeaderName;
pub use http::{HeaderMap, StatusCode};
use reqwest::IntoUrl;
use serde::Serialize;
use serde::de::DeserializeOwned;
use warp_errors::report_error;

/// A wrapper around a `reqwest::Client` to execute requests. Returns a custom `RequestBuilder` type
/// that ensures any call to the underlying `reqwest::Client` are properly adapted so that they can
/// run outside of a Tokio context.
pub struct Client {
    wrapped: reqwest::Client,

    /// A callback that is executed before every request is sent with a cloned
    /// version of the outbound request.  If for some reason the request cannot be
    /// cloned the function is not called.
    before_request_sent: Option<RequestHookFn>,

    /// A callback that is executed on after each response is received.
    after_response_received: Option<ResponseHookFn>,
}

/// Type for 'hook' functions to be executed prior to sending a request. A reference to the
/// outbound request object is given as the first argument. The second argument the request's
/// serialized JSON payload, if any.
pub type RequestHookFn = Box<dyn Fn(&reqwest::Request, &Option<String>) + 'static + Send + Sync>;

/// Type for 'hook' functions to be executed after receiving a response. The sole argument is a
/// reference to the inbound response object.
pub type ResponseHookFn = Box<dyn Fn(&reqwest::Response) + 'static + Send + Sync>;

/// A custom request builder that is a wrapper around a `request::RequestBuilder`. Ensures any async
/// call to the underyling `reqwest::RequestBuilder` are properly adapted to run outside of a Tokio
/// context via a call to `compat`.
pub struct RequestBuilder<'a> {
    wrapped: reqwest::RequestBuilder,
    client: &'a Client,

    // The JSON payload of the request, if any, serialized to a pretty-printed String.
    serialized_payload: Option<String>,
}

pub struct Request {
    wrapped: reqwest::Request,
    serialized_payload: Option<String>,
}

/// A wrapper around a `reqwest::Response` that ensures any async calls to the underlying `Response`
/// a properly adapted to be run outside of a Tokio context.
pub struct Response(reqwest::Response);

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let mut builder = reqwest::Client::builder();

        // Set some HTTP/2-related settings.
        {
            builder = builder
                .http2_keep_alive_interval(Duration::from_secs(60))
                // If a pong is not received within 15s, consider the connection dead.
                .http2_keep_alive_timeout(Duration::from_secs(15))
                // Send these even when there aren't active streams, to ensure that we detect
                // dead connections before we attempt to use them.
                .http2_keep_alive_while_idle(true);
        }

        Self::from_client_builder(builder).expect("should not fail to create client")
    }

    #[cfg(feature = "test-util")]
    pub fn new_for_test() -> Self {
        let client_builder = reqwest::ClientBuilder::new()
            // Don't load any SSL/TLS certificates, as doing so can be slow and we should
            // never be making real requests in tests.
            .tls_certs_only([])
            // Disable proxy usage in tests, as loading system proxy configuration can be
            // slow.
            .no_proxy();
        Self::from_client_builder(client_builder).expect("should not fail to create client")
    }

    pub fn from_client_builder(client_builder: reqwest::ClientBuilder) -> reqwest::Result<Self> {
        client_builder.build().map(|client| Self {
            wrapped: client,
            before_request_sent: None,
            after_response_received: None,
        })
    }

    pub fn set_before_request_fn(&mut self, hook_fn: RequestHookFn) {
        self.before_request_sent = Some(hook_fn);
    }

    pub fn set_after_response_fn(&mut self, hook_fn: ResponseHookFn) {
        self.after_response_received = Some(hook_fn);
    }

    fn builder(&self, wrapped: reqwest::RequestBuilder) -> RequestBuilder<'_> {
        RequestBuilder {
            wrapped,
            client: self,
            serialized_payload: None,
        }
    }

    pub fn get<U: IntoUrl>(&self, url: U) -> RequestBuilder<'_> {
        self.builder(self.wrapped.get(url))
    }

    pub async fn execute(&self, request: Request) -> reqwest::Result<Response> {
        self.execute_inner(request).await
    }

    /// Core request execution logic shared by all platforms.
    async fn execute_inner(&self, request: Request) -> reqwest::Result<Response> {
        let Request {
            wrapped: request,
            serialized_payload,
        } = request;

        if let Some(before_response_send_fn) = &self.before_request_sent {
            before_response_send_fn(&request, &serialized_payload);
        }

        // Explicitly await the future before converting from tokio -> futures. This is because
        // certain calls to tokio (such as tokio::time::sleep) will panic upon creation if they
        // are not in a tokio runtime. Wrapping the call in an async block first makes sure that it
        // is lazily evaluated, ensuring that it is created within a tokio runtime.
        let result = Compat::new(async { self.wrapped.execute(request).await }).await?;

        if let Some(after_response_received_fn) = &self.after_response_received {
            after_response_received_fn(&result);
        }

        Ok(Response(result))
    }
}

impl<'a> RequestBuilder<'a> {
    pub fn build(self) -> reqwest::Result<Request> {
        self.build_split().1
    }

    pub fn build_split(self) -> (&'a Client, reqwest::Result<Request>) {
        let request = self.wrapped.build().map(|request| Request {
            wrapped: request,
            serialized_payload: self.serialized_payload,
        });
        (self.client, request)
    }

    pub async fn send(self) -> reqwest::Result<Response> {
        let (client, request) = self.build_split();
        client.execute(request?).await
    }

    pub fn json<T: Serialize + ?Sized>(self, json: &T) -> RequestBuilder<'a> {
        let serialized_payload =
            match serde_json::to_string_pretty(json).map_err(anyhow::Error::from) {
                Ok(payload) => Some(payload),
                Err(err) => {
                    report_error!(err.context("Failed to serialize JSON request payload."));
                    None
                }
            };
        Self {
            wrapped: self.wrapped.json(json),
            serialized_payload,
            ..self
        }
    }

    pub fn header<K, V>(self, key: K, value: V) -> RequestBuilder<'a>
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        Self {
            wrapped: self.wrapped.header(key, value),
            ..self
        }
    }

    pub fn body<T: Into<reqwest::Body>>(self, body: T) -> RequestBuilder<'a> {
        Self {
            wrapped: self.wrapped.body(body),
            ..self
        }
    }
}

impl Response {
    pub async fn text(self) -> reqwest::Result<String> {
        Compat::new(async { self.0.text().compat().await }).await
    }

    pub fn status(&self) -> StatusCode {
        self.0.status()
    }

    pub async fn json<T: DeserializeOwned>(self) -> reqwest::Result<T> {
        Compat::new(async { self.0.json().compat().await }).await
    }

    pub async fn bytes(self) -> reqwest::Result<Bytes> {
        self.0.bytes().await
    }

    pub fn bytes_stream(self) -> impl Stream<Item = reqwest::Result<Bytes>> {
        self.0.bytes_stream()
    }

    pub fn headers(&self) -> &http::HeaderMap {
        self.0.headers()
    }

    pub fn url(&self) -> &reqwest::Url {
        self.0.url()
    }
}
