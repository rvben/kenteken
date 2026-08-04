//! The HTTP client, driven against a local fake of the RDW endpoint.
//!
//! These tests exercise the real `HttpSource` over a real socket: URL shape,
//! query parameters, headers, and how each status maps to an error kind. RDW
//! itself is never contacted, so the suite is deterministic and does not spend a
//! free public service's bandwidth.

use kenteken::rdw::client::HttpSource;
use kenteken::rdw::{RdwSource, datasets};
use kenteken::{KentekenError, Plate};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How the fake endpoint answers.
#[derive(Clone)]
enum Behaviour {
    /// Answer with this status and body.
    Respond { status: u16, body: String },
    /// Accept the connection and never answer, so the client must time out.
    Hang,
}

/// A one-endpoint stand-in for opendata.rdw.nl, listening on loopback.
struct FakeRdw {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
}

/// What the client actually sent.
#[derive(Clone, Debug)]
struct Request {
    target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn query(&self, key: &str) -> Option<String> {
        let (_, query) = self.target.split_once('?')?;
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (percent_decode(k) == key).then(|| percent_decode(v))
        })
    }
}

/// Enough percent-decoding to read back a query this tool could have sent.
fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'%' => {
                let hi = bytes.next().unwrap_or(b'0');
                let lo = bytes.next().unwrap_or(b'0');
                let hex = String::from_utf8(vec![hi, lo]).unwrap_or_default();
                match u8::from_str_radix(&hex, 16) {
                    Ok(v) => out.push(v as char),
                    Err(_) => out.push('%'),
                }
            }
            b'+' => out.push(' '),
            _ => out.push(b as char),
        }
    }
    out
}

impl FakeRdw {
    fn start(behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_requests = Arc::clone(&requests);
        let thread_stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut stream) = stream else { return };
                let Some(request) = read_request(&mut stream) else {
                    continue;
                };
                thread_requests.lock().expect("lock").push(request);

                match &behaviour {
                    Behaviour::Respond { status, body } => {
                        let response = format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Behaviour::Hang => {
                        // Hold the connection open without answering. The client
                        // must give up on its own.
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        });

        FakeRdw {
            addr,
            requests,
            stop,
        }
    }

    fn ok(body: &str) -> Self {
        Self::start(Behaviour::Respond {
            status: 200,
            body: body.to_string(),
        })
    }

    fn status(status: u16, body: &str) -> Self {
        Self::start(Behaviour::Respond {
            status,
            body: body.to_string(),
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn source(&self, timeout: Duration) -> HttpSource {
        HttpSource::build(&self.base_url(), timeout, None).expect("client builds")
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("lock").clone()
    }
}

impl Drop for FakeRdw {
    fn drop(&mut self) {
        // Wake the blocked accept so the thread notices the flag and exits.
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut start = String::new();
    reader.read_line(&mut start).ok()?;
    let target = start.split_whitespace().nth(1)?.to_string();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    // Drain nothing further: every request this tool makes is a bodyless GET.
    let _ = &mut reader as &mut dyn Read;
    Some(Request { target, headers })
}

fn plate() -> Plate {
    Plate::parse("X99XXX").expect("test plate parses")
}

const SHORT: Duration = Duration::from_secs(5);

#[test]
fn a_successful_response_becomes_rows() {
    let server = FakeRdw::ok(r#"[{"kenteken":"X99XXX","merk":"IVECO"}]"#);
    let rows = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .expect("rows parse");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["merk"], "IVECO");
}

#[test]
fn the_request_targets_the_datasets_resource_and_filters_by_plate() {
    let server = FakeRdw::ok("[]");
    let _ = server
        .source(SHORT)
        .rows_for_plate(&datasets::DEFECTS, &plate());

    let requests = server.requests();
    assert_eq!(requests.len(), 1, "exactly one request per lookup");
    let request = &requests[0];
    assert!(
        request
            .target
            .starts_with(&format!("/{}.json", datasets::DEFECTS.id)),
        "target was {}",
        request.target
    );
    assert_eq!(request.query("kenteken").as_deref(), Some("X99XXX"));
}

#[test]
fn an_explicit_limit_is_always_sent_so_socratas_default_cannot_truncate() {
    // Socrata caps an unqualified query at 1000 rows silently. Asking for a
    // bound explicitly is what makes a short result mean "that is all there is".
    let server = FakeRdw::ok("[]");
    let _ = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate());
    assert_eq!(
        server.requests()[0].query("$limit"),
        Some(kenteken::rdw::client::FETCH_CAP.to_string())
    );
}

#[test]
fn the_user_agent_identifies_the_tool_and_its_version() {
    // A free public API deserves a caller that can be identified and contacted.
    let server = FakeRdw::ok("[]");
    let _ = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate());
    let agent = server.requests()[0]
        .header("user-agent")
        .expect("a user agent is sent")
        .to_string();
    assert!(agent.starts_with("kenteken/"), "user agent was {agent}");
    assert!(
        agent.contains(env!("CARGO_PKG_VERSION")),
        "user agent was {agent}"
    );
}

#[test]
fn an_app_token_is_sent_only_when_one_is_configured() {
    let server = FakeRdw::ok("[]");
    let with = HttpSource::build(&server.base_url(), SHORT, Some("secret".into())).unwrap();
    let _ = with.rows_for_plate(&datasets::VEHICLE, &plate());
    assert_eq!(server.requests()[0].header("x-app-token"), Some("secret"));

    let plain = FakeRdw::ok("[]");
    let _ = plain
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate());
    assert_eq!(plain.requests()[0].header("x-app-token"), None);
}

#[test]
fn an_empty_array_is_zero_rows_and_not_an_error() {
    // RDW answers an unknown plate with 200 and `[]`. That is a real answer; the
    // decision about what it means belongs to the caller, not the transport.
    let server = FakeRdw::ok("[]");
    let rows = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .expect("an empty result is not a failure");
    assert!(rows.is_empty());
}

#[test]
fn too_many_requests_becomes_a_retryable_rate_limit_error() {
    let server = FakeRdw::status(429, "");
    let err = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "rate_limit", "got {err:?}");
    assert!(err.retryable());
    assert_eq!(err.exit_code(), 6);
}

#[test]
fn a_missing_resource_becomes_an_unknown_dataset_error() {
    let server = FakeRdw::status(404, r#"{"message":"Cannot find resource"}"#);
    let err = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "unknown_dataset", "got {err:?}");
    assert_eq!(err.details().unwrap()["dataset"], datasets::VEHICLE.id);
}

#[test]
fn a_server_error_carries_socratas_own_message() {
    let server = FakeRdw::status(400, r#"{"error":true,"message":"Unrecognized argument"}"#);
    let err = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "api", "got {err:?}");
    assert!(
        err.to_string().contains("Unrecognized argument"),
        "message was {err}"
    );
}

#[test]
fn a_body_that_is_not_rows_is_an_api_error_and_never_zero_rows() {
    // The dangerous failure: a proxy or captive portal answering 200 with HTML.
    // Reading that as "no rows" would report a registered vehicle as unknown.
    let server = FakeRdw::ok("<html>hello</html>");
    let err = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "api", "got {err:?}");
}

#[test]
fn a_json_object_where_rows_are_expected_is_also_an_api_error() {
    let server = FakeRdw::ok(r#"{"kenteken":"X99XXX"}"#);
    let err = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "api", "got {err:?}");
}

#[test]
fn a_silent_server_becomes_a_timeout_and_not_a_network_error() {
    // One second, the shortest timeout the CLI accepts, so the message this
    // asserts is the one a user can actually see.
    let server = FakeRdw::start(Behaviour::Hang);
    let err = server
        .source(Duration::from_secs(1))
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "timeout", "got {err:?}");
    assert!(err.retryable());
    assert_eq!(err, KentekenError::Timeout { seconds: 1 });
    assert!(err.to_string().contains("within 1s"), "message was {err}");
}

#[test]
fn an_unreachable_endpoint_becomes_a_network_error() {
    // Port 1 on loopback refuses connections immediately.
    let source = HttpSource::build("http://127.0.0.1:1", Duration::from_secs(2), None).unwrap();
    let err = source
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .unwrap_err();
    assert_eq!(err.kind(), "network", "got {err:?}");
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn no_request_is_retried_automatically() {
    // RDW is free and public. One user action is one request, so a failure never
    // multiplies into a burst.
    let server = FakeRdw::status(500, "boom");
    let _ = server
        .source(SHORT)
        .rows_for_plate(&datasets::VEHICLE, &plate());
    assert_eq!(
        server.requests().len(),
        1,
        "a failed request must not be re-sent"
    );
}

#[test]
fn a_trailing_slash_in_the_base_url_does_not_double_up() {
    let server = FakeRdw::ok("[]");
    let source = HttpSource::build(&format!("{}/", server.base_url()), SHORT, None).unwrap();
    source
        .rows_for_plate(&datasets::VEHICLE, &plate())
        .expect("rows parse");
    assert!(
        !server.requests()[0].target.starts_with("//"),
        "target was {}",
        server.requests()[0].target
    );
}
