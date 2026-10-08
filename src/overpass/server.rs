use std::{borrow::Cow, sync::LazyLock, time::Duration};
use crate::{Overpass, OverpassError, OverpassQL, OverpassResult, Query};
use reqwest::{
    Client, Response,
    header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
};
use serde::de::DeserializeOwned;

/// The `User-Agent` of the shared static HTTP client. The official API server refuses requests
/// that do not identify themselves (`406 Not Acceptable`).
pub const DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder().user_agent(DEFAULT_USER_AGENT).build().unwrap_or_default()
});

/// The URL of the official API server.
pub const DEFAULT_URL: &str = "https://overpass-api.de/api/interpreter";

/// Makes an HTTP request to an Overpass API server to evaluate queries.
///
/// This implements [Default] with the official API server URL and a shared static HTTP client.
/// This means you will usually not need to construct your own instance, you can use
/// `OverpassServer::default()` directly.
///
/// To talk to another server, or to control how the requests are sent, start from
/// [OverpassServer::new] and the `with_*` methods:
///
/// ```
/// # use std::time::Duration;
/// # use overpass_lib::*;
/// use reqwest::header::HeaderValue;
///
/// let http = reqwest::Client::builder()
///     .connect_timeout(Duration::from_secs(5))
///     .build()
///     .unwrap();
///
/// let server = OverpassServer::new("http://localhost:8081/api/interpreter")
///     .with_client(http)
///     .with_timeout(Duration::from_secs(60))
///     .with_user_agent(HeaderValue::from_static("my-app/1.0"));
/// ```
#[derive(Debug, Clone)]
pub struct OverpassServer {
    /// The HTTP client used to connect to the API server.
    pub client: Cow<'static, Client>,

    /// The URL of the API server's `interpreter` endpoint.
    pub url: String,

    /// Headers added to every request. They take precedence over the default headers of the
    /// [client](Self::client).
    pub headers: HeaderMap,

    /// The timeout of each request, from sending it to having read the whole answer. `None`
    /// leaves it to the [client](Self::client).
    pub timeout: Option<Duration>,
}

impl Default for OverpassServer {
    fn default() -> Self {
        Self::new(DEFAULT_URL)
    }
}

impl OverpassServer {
    /// A server at the given `interpreter` URL, with the shared static HTTP client (which
    /// identifies itself as [DEFAULT_USER_AGENT]).
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            client: Cow::Borrowed(&CLIENT),
            url: url.into(),
            headers: HeaderMap::new(),
            timeout: None,
        }
    }

    /// Send the requests with this HTTP client instead of the shared one: its connection pool,
    /// its TLS and proxy settings, its timeouts and its default headers apply.
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = Cow::Owned(client);
        self
    }

    /// Set the timeout of each request.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Identify the requests with this `User-Agent`.
    pub fn with_user_agent(self, user_agent: HeaderValue) -> Self {
        self.with_header(USER_AGENT, user_agent)
    }

    /// Add a header to every request, replacing a previous value of the same header.
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Send an OverpassQL string to the server as it is, and return the HTTP response untouched.
    ///
    /// The string is the body of a `POST` request. Nothing is added to it and nothing in it is
    /// checked: the output format, in particular, is the one it asks for.
    pub async fn send_oql(&self, oql: impl Into<String>) -> Result<Response, OverpassError> {
        let mut req = self.client.post(&self.url).headers(self.headers.clone()).body(oql.into());
        if let Some(timeout) = self.timeout {
            req = req.timeout(timeout);
        }
        req.send().await.map_err(|e| OverpassError::Request(e))
    }

    /// Evaluate an OverpassQL string written by hand, for whatever the query types of this crate
    /// cannot express, and return the resulting [Element](crate::Element)s.
    ///
    /// The query must ask for JSON (`[out:json]`).
    ///
    /// ```no_run
    /// # use overpass_lib::*;
    /// # tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
    /// let res = OverpassServer::default()
    ///     .evaluate_oql(r#"[out:json];relation["name"="Seattle"]["boundary"="administrative"];out center tags;"#)
    ///     .await
    ///     .unwrap();
    ///
    /// for element in &res.elements {
    ///     println!("{:?} is at {:?}", element.tag("name"), element.position());
    /// }
    /// # });
    /// ```
    pub async fn evaluate_oql(&self, oql: impl Into<String>) -> Result<OverpassResult, OverpassError> {
        self.evaluate_oql_as(oql).await
    }

    /// Evaluate an OverpassQL string written by hand and return the server's JSON document as it
    /// came, whatever its elements look like.
    pub async fn evaluate_oql_json(&self, oql: impl Into<String>) -> Result<serde_json::Value, OverpassError> {
        self.evaluate_oql_as(oql).await
    }

    /// Evaluate an OverpassQL string written by hand and read the server's JSON document as any
    /// deserializable type.
    pub async fn evaluate_oql_as<T: DeserializeOwned>(&self, oql: impl Into<String>) -> Result<T, OverpassError> {
        let res = self.send_oql(oql).await?;
        let status = res.status();

        match res.bytes().await {
            Err(e) => Err(OverpassError::Request(e)),
            // The status is not looked at first: a JSON answer is an answer. Without one, an
            // error status says more than the position where the parser gave up.
            Ok(b) => serde_json::from_slice(&b).map_err(|e| match status.is_success() {
                true => OverpassError::Parse(e),
                false => OverpassError::Other(format!("HTTP {status}")),
            }),
        }
    }
}

impl Overpass for OverpassServer {
    async fn evaluate(&self, query: &Query<'_>) -> Result<OverpassResult, OverpassError> {
        let mut body = String::new();
        query.fmt_oql(&mut body).map_err(|e| OverpassError::Query(e))?;

        self.evaluate_oql(body).await
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use reqwest::header::CONTENT_TYPE;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::JoinHandle,
    };
    use crate::{ElementId, FilterSet, FilterType, Set};

    use super::*;

    const ANSWER: &str = r#"{
        "version": 0.6,
        "generator": "Overpass API 0.7.62.1 084b4234",
        "elements": [
            {"type": "node", "id": 1, "lat": 47.6, "lon": -122.3, "tags": {"name": "A"}},
            {"type": "relation", "id": 2, "center": {"lat": 47.5, "lon": -122.2}, "tags": {"name": "B"}}
        ]
    }"#;

    /// Answers the first request received on a loopback port with `body`, and hands back that
    /// request (head and body) as text.
    async fn serve_once(status: u16, body: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/interpreter", listener.local_addr().unwrap());

        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();

            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let text = String::from_utf8_lossy(&received).to_string();
                if let Some((head, sent)) = text.split_once("\r\n\r\n") {
                    let length = head.lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|v| v.to_string()))
                        .map(|v| v.parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if sent.len() >= length {
                        break;
                    }
                }
                let n = socket.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..n]);
            }

            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len(),
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();

            String::from_utf8(received).unwrap()
        });

        (url, handle)
    }

    fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        request.split("\r\n\r\n").next().unwrap().lines().skip(1)
            .filter_map(|l| l.split_once(": "))
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    #[test]
    fn default_server() {
        let server = OverpassServer::default();
        assert_eq!(server.url, DEFAULT_URL);
        assert!(server.headers.is_empty());
        assert_eq!(server.timeout, None);
    }

    #[tokio::test]
    async fn raw_oql_is_the_request_body() {
        let (url, request) = serve_once(200, ANSWER).await;
        let oql = r#"[bbox:47.5,-122.4,47.7,-122.2][timeout:25][out:json];(node["name"];relation["name"];);out center tags;"#;

        let res = OverpassServer::new(url).evaluate_oql(oql).await.unwrap();
        assert_eq!(
            res.elements.iter().map(|e| e.id()).collect::<Vec<_>>(),
            vec![ElementId::Node(1), ElementId::Relation(2)],
        );
        assert_eq!(res.other_fields["version"], 0.6);

        let request = request.await.unwrap();
        assert!(request.starts_with("POST /api/interpreter HTTP/1.1\r\n"), "{request}");
        assert_eq!(request.split_once("\r\n\r\n").unwrap().1, oql);
        assert_eq!(header(&request, "content-length"), Some(oql.len().to_string().as_str()));
        assert_eq!(header(&request, "content-type"), None);
        assert_eq!(header(&request, "user-agent"), Some(DEFAULT_USER_AGENT));
    }

    #[tokio::test]
    async fn a_given_client_sends_only_its_own_headers() {
        let (url, request) = serve_once(200, ANSWER).await;

        OverpassServer::new(url).with_client(Client::new())
            .evaluate_oql("[out:json];node(1);out;").await.unwrap();

        let request = request.await.unwrap();
        assert_eq!(header(&request, "user-agent"), None);
        assert_eq!(header(&request, "content-type"), None);
    }

    #[tokio::test]
    async fn headers_and_client_are_the_callers() {
        let (url, request) = serve_once(200, ANSWER).await;

        let mut defaults = HeaderMap::new();
        defaults.insert("x-from-the-client", HeaderValue::from_static("yes"));
        defaults.insert(USER_AGENT, HeaderValue::from_static("the client's"));
        let client = Client::builder().default_headers(defaults).build().unwrap();

        let server = OverpassServer::new(url)
            .with_client(client)
            .with_user_agent(HeaderValue::from_static("my-app/1.0"))
            .with_header(CONTENT_TYPE, HeaderValue::from_static("application/x-www-form-urlencoded"));
        server.evaluate_oql("[out:json];node(1);out;").await.unwrap();

        let request = request.await.unwrap();
        assert_eq!(header(&request, "user-agent"), Some("my-app/1.0"));
        assert_eq!(header(&request, "content-type"), Some("application/x-www-form-urlencoded"));
        assert_eq!(header(&request, "x-from-the-client"), Some("yes"));
    }

    #[tokio::test]
    async fn query_is_sent_as_its_oql() {
        let (url, request) = serve_once(200, ANSWER).await;

        let query = Query::from(Set::Filter(FilterSet {
            filter_type: FilterType::Node,
            id_filters: HashSet::from([3359850618]),
            ..Default::default()
        }));
        OverpassServer::new(url).evaluate(&query).await.unwrap();

        let request = request.await.unwrap();
        assert_eq!(request.split_once("\r\n\r\n").unwrap().1, query.to_oql());
    }

    #[tokio::test]
    async fn json_is_returned_as_it_came() {
        let (url, _request) = serve_once(
            200,
            r#"{"elements": [{"type": "count", "id": 0, "tags": {"total": "3"}}], "remark": "runtime error"}"#,
        ).await;
        let server = OverpassServer::new(url);

        let json = server.evaluate_oql_json("[out:json];node(1);out count;").await.unwrap();
        assert_eq!(json["elements"][0]["type"], "count");
        assert_eq!(json["remark"], "runtime error");
    }

    #[tokio::test]
    async fn the_status_does_not_hide_a_json_answer() {
        let (url, _request) = serve_once(429, ANSWER).await;

        let res = OverpassServer::new(url).send_oql("[out:json];node(1);out;").await.unwrap();
        assert_eq!(res.status(), 429);
    }

    #[tokio::test]
    async fn an_answer_that_is_not_json_is_an_error() {
        let (url, _request) = serve_once(200, "<html>not what was asked</html>").await;
        let err = OverpassServer::new(url).evaluate_oql("[out:json];node(1);out;").await.unwrap_err();
        assert!(matches!(err, OverpassError::Parse(_)), "{err:?}");

        let (url, _request) = serve_once(504, "<html>Gateway Timeout</html>").await;
        let err = OverpassServer::new(url).evaluate_oql("[out:json];node(1);out;").await.unwrap_err();
        assert!(matches!(&err, OverpassError::Other(e) if e.starts_with("HTTP 504")), "{err:?}");
    }

    #[tokio::test]
    async fn a_json_answer_is_read_whatever_the_status() {
        let (url, _request) = serve_once(429, ANSWER).await;

        let res = OverpassServer::new(url).evaluate_oql("[out:json];node(1);out;").await.unwrap();
        assert_eq!(res.elements.len(), 2);
    }

    #[tokio::test]
    async fn timeout_applies_to_the_request() {
        // Accepts connections and never answers.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/interpreter", listener.local_addr().unwrap());

        let err = OverpassServer::new(url)
            .with_timeout(Duration::from_millis(200))
            .evaluate_oql("[out:json];node(1);out;")
            .await
            .unwrap_err();
        assert!(matches!(&err, OverpassError::Request(e) if e.is_timeout()), "{err:?}");
        drop(listener);
    }

    #[tokio::test]
    #[ignore]
    async fn server() {
        let q = OverpassServer::default().evaluate(
            &Query::from(Set::Filter(FilterSet {
                filter_type: FilterType::Node,
                id_filters: HashSet::from([3359850618]),
                ..Default::default()
            })),
        ).await;

        let ids = q.unwrap().elements.into_iter().map(|e| e.id()).collect::<HashSet<ElementId>>();
        assert_eq!(ids, HashSet::from([ElementId::Node(3359850618)]));
    }
}
