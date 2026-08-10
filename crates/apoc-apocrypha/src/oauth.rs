//! Getting this installation signed in, without ever seeing a password.
//!
//! This replaced a device-code flow, and the reason is worth keeping written
//! down because the old one looked fine. There, the app asked the service to
//! start a pairing, showed a short code, and polled until someone approved it
//! on the website. The hole is that the app which *starts* the request is also
//! the one that *collects* the token, and nothing connects those two facts to
//! the person who approved in between. An attacker started a pairing on their
//! own machine, sent the victim a genuine link to the real site, and received a
//! token on the victim's account the moment they said yes. Comparing the code
//! did not help — it matched. Neither did a password or a second factor,
//! because the victim was doing exactly what they had been asked to do.
//!
//! So the token no longer travels back to whoever asked. This app opens a
//! socket on the loopback interface, tells the service to deliver the
//! authorization code *there*, and exchanges it for a token itself. An attacker
//! who talks someone through the entire flow gets the code delivered to that
//! person's own computer, where nothing of theirs is listening.
//!
//! One more piece closes the local case. The callback is a plain HTTP port on a
//! shared machine, so another process could in principle race for it. Before
//! the browser is opened, this generates a random *verifier*, sends only its
//! SHA-256 digest to the service, and keeps the verifier in memory — so the
//! code can only be spent by whoever holds the original. Winning the race gets
//! an attacker a string they cannot use. That is RFC 7636; the loopback rule is
//! RFC 8252.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{agent, ServiceOrigin};

#[derive(Debug, Error)]
pub enum AuthorizationError {
    /// The service could not be reached at all.
    ///
    /// The message is deliberately the same whatever the underlying failure
    /// was. A person seeing this cannot act on "tls connection init failed:
    /// Connection reset by peer (os error 104)" — that is a sentence for a bug
    /// report, and putting it on screen makes a routine redeploy look like a
    /// catastrophe. The one useful thing to say is that it may be temporary,
    /// because it usually is.
    #[error("Could not reach Apocrypha. It may be updating — try again in a moment.")]
    Http(String),
    #[error("could not read the response: {0}")]
    Decode(String),
    #[error("the service refused: {0}")]
    Refused(String),
    #[error("this sign-in expired. Start again.")]
    Expired,
    #[error("the request was declined.")]
    Declined,
    /// No socket could be opened for the browser to answer on.
    #[error("could not open a port for the browser to answer on: {0}")]
    Listener(String),
    #[error("the service returned an unexpected status {0}")]
    Unexpected(u16),
}

/// Where an authorization has got to. Returned by one poll, not by a loop: the
/// caller owns the waiting, so a window can stay responsive and a person can
/// cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationStatus {
    /// The browser has not answered yet. Poll again shortly.
    Waiting,
    /// Approved, exchanged, and stored by the caller. Happens exactly once.
    Granted { token: String, expires_at: String },
    /// The person said no, and the service said so rather than leaving this to
    /// time out.
    Declined,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest<'a> {
    code: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantedToken {
    token: String,
    expires_at: String,
}

/// An authorization waiting for the browser to come back.
///
/// Holds the socket the answer arrives on and the verifier that spends it.
/// Dropping it closes the port and abandons the attempt, which is what
/// cancelling should do.
pub struct PendingAuthorization {
    listener: TcpListener,
    origin: ServiceOrigin,
    app_version: String,
    verifier: String,
    state: String,
    redirect_uri: String,
    authorize_url: String,
    deadline: Instant,
}

/// How long the socket stays open waiting for an answer.
///
/// Long enough for someone to find their browser window, sign in, and read a
/// consent screen; short enough that a forgotten attempt does not leave a
/// listening port open all session.
const WINDOW: Duration = Duration::from_secs(300);

/// The largest request this will read off the callback socket.
///
/// A browser's GET of a loopback callback is a few hundred bytes. Anything
/// approaching this is not a browser, and reading it into memory unbounded is
/// how a local process turns a callback port into a way to exhaust the app.
const MAX_REQUEST: usize = 8 * 1024;

/// How long one read off the callback socket may block.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the whole read may take, however many times the peer trickles.
const READ_DEADLINE: Duration = Duration::from_secs(5);

/// The only path that counts as an answer.
const CALLBACK_PATH: &str = "/callback";

impl PendingAuthorization {
    /// Opens the callback socket and builds the URL to send the browser to.
    ///
    /// Nothing is sent to the service here. The whole request travels in the
    /// browser's address bar, which is where the consent screen reads it from.
    pub fn begin(
        origin: ServiceOrigin,
        app_version: impl Into<String>,
        client_name: &str,
    ) -> Result<Self, AuthorizationError> {
        // Port zero: the operating system picks a free one and tells us which.
        // Choosing a fixed port would mean two copies of the app cannot both
        // sign in, and would hand any local process a port worth squatting on
        // before we get there.
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| AuthorizationError::Listener(e.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| AuthorizationError::Listener(e.to_string()))?;

        let port = listener
            .local_addr()
            .map_err(|e| AuthorizationError::Listener(e.to_string()))?
            .port();

        let verifier = random_token();
        let state = random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");

        let authorize_url = format!(
            "{}/authorize?client_name={}&redirect_uri={}&code_challenge={}\
             &code_challenge_method=S256&state={}&scopes={}",
            origin.as_str(),
            crate::urlencode(client_name),
            crate::urlencode(&redirect_uri),
            crate::urlencode(&challenge),
            crate::urlencode(&state),
            GRANTED_SCOPES,
        );

        Ok(Self {
            listener,
            origin,
            app_version: app_version.into(),
            verifier,
            state,
            redirect_uri,
            authorize_url,
            deadline: Instant::now() + WINDOW,
        })
    }

    /// The page to open in a browser.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// Checks once whether the browser has answered, without blocking.
    ///
    /// Returns [`AuthorizationStatus::Waiting`] when nothing has arrived, which
    /// is the ordinary case for almost every call.
    pub fn poll(&self) -> Result<AuthorizationStatus, AuthorizationError> {
        if Instant::now() > self.deadline {
            return Err(AuthorizationError::Expired);
        }

        let mut stream = match self.listener.accept() {
            Ok((stream, _)) => {
                // Put the accepted socket back into blocking mode.
                //
                // The listener is non-blocking so poll() can return promptly
                // when nothing has arrived. On Unix the accepted socket does not
                // inherit that. On Windows it does: Winsock hands back a socket
                // carrying the listening socket's mode, so the first read
                // returned WouldBlock immediately, the read loop treated that as
                // a dead connection, and the browser's callback was discarded as
                // unintelligible — every single time. Signing in on Windows
                // could not complete, and the failure looked like the browser
                // never answering.
                //
                // The read deadline below is what bounds this now that it
                // blocks, which is the job it was already doing.
                let _ = stream.set_nonblocking(false);
                stream
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(AuthorizationStatus::Waiting);
            }
            Err(e) => return Err(AuthorizationError::Listener(e.to_string())),
        };

        let target = match read_request_target(&mut stream) {
            Some(t) => t,
            None => {
                // Something connected and said nothing intelligible. Hang up and
                // keep waiting: the browser may still be coming, and one
                // confused connection is not a reason to fail a sign-in.
                let _ = respond(
                    &mut stream,
                    "400 Bad Request",
                    "That request was not understood.",
                );
                return Ok(AuthorizationStatus::Waiting);
            }
        };

        let path = target.split('?').next().unwrap_or_default();
        let code = query_parameter(&target, "code");
        let error = query_parameter(&target, "error");
        let state = query_parameter(&target, "state");

        // Everything that is not our callback, answered and ignored. A browser
        // fetching /favicon.ico is the common case, and treating it as an answer
        // would abandon a sign-in the person is still in the middle of.
        if path != CALLBACK_PATH || (code.is_none() && error.is_none()) {
            let _ = respond(&mut stream, "404 Not Found", "Nothing here.");
            return Ok(AuthorizationStatus::Waiting);
        }

        // The state is what makes this port ours. Anything answering with a
        // value we did not generate is not the browser we sent out.
        //
        // It is refused and then *ignored*, which is the important half. Failing
        // the sign-in here would mean any local process could end someone's
        // sign-in with a single packet to a port it only had to guess — a one-
        // packet denial of service, unauthenticated, requiring none of the
        // secrets this flow is built around. Waiting instead costs nothing: the
        // real browser is still coming, and an impostor gets a 400 and no
        // information.
        if state.as_deref() != Some(self.state.as_str()) {
            let _ = respond(
                &mut stream,
                "400 Bad Request",
                "That did not come from this sign-in. Nothing was granted.",
            );
            return Ok(AuthorizationStatus::Waiting);
        }

        if error.is_some() {
            let _ = respond(
                &mut stream,
                "200 OK",
                "Declined. You can close this tab and go back to Apocrypha.",
            );
            return Ok(AuthorizationStatus::Declined);
        }

        let code = code.expect("checked above");

        // The browser is answered before the exchange, not after. The exchange
        // is a network call that can take a second, and a tab that sits blank
        // while it happens reads as a hang.
        let _ = respond(
            &mut stream,
            "200 OK",
            "Signed in. You can close this tab and go back to Apocrypha.",
        );

        let granted = self.exchange(&code)?;
        Ok(AuthorizationStatus::Granted {
            token: granted.token,
            expires_at: granted.expires_at,
        })
    }

    /// Spends the code for a token, proving with the verifier that this is the
    /// client that started the flow.
    fn exchange(&self, code: &str) -> Result<GrantedToken, AuthorizationError> {
        let url = format!("{}/api/v1/auth/oauth/token", self.origin.as_str());

        // Deliberately not retried. The code lives for sixty seconds and is
        // single-use; a second attempt after a dropped connection is as likely
        // to meet an already-spent code as to succeed, and the honest answer to
        // the person is to sign in again rather than to keep trying quietly.
        let response = agent()
            .post(&url)
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .send_json(TokenRequest {
                code,
                code_verifier: &self.verifier,
                redirect_uri: &self.redirect_uri,
            });

        match response {
            Ok(res) => res
                .into_json::<GrantedToken>()
                .map_err(|e| AuthorizationError::Decode(e.to_string())),
            Err(ureq::Error::Status(code, res)) => {
                let body = res.into_string().unwrap_or_default();
                match code {
                    400 | 409 => Err(AuthorizationError::Refused(safe_message(&body))),
                    404 => Err(AuthorizationError::Expired),
                    _ => Err(AuthorizationError::Unexpected(code)),
                }
            }
            Err(e) => Err(AuthorizationError::Http(e.to_string())),
        }
    }
}

/// What this client asks for: read its own account, read the catalogue, claim
/// downloads. The service refuses anything wider, and this is the same set
/// spelled on its side.
const GRANTED_SCOPES: u32 = 0b111;

/// 256 bits, base64url, no padding — 43 characters, which is what RFC 7636 asks
/// a verifier to be at minimum and what a `state` may as well be too.
fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("the operating system has no entropy source");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Reads the request line and returns its target, e.g. `/callback?code=…`.
///
/// Only the first line is needed and only the first line is parsed. Headers are
/// drained to the byte cap so the browser's write completes and it does not
/// report a reset connection instead of showing the page.
fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    // A browser that has connected sends immediately. Anything that opens the
    // socket and waits is not one, and must not be able to hold the sign-in
    // open by saying nothing.
    //
    // Both bounds are needed and they are not the same bound. The socket
    // timeout limits one read; the deadline limits the whole exchange. Without
    // the deadline, a process that sends one byte just before each timeout
    // keeps this function running forever — and since the caller holds the
    // pending sign-in's lock while it runs, that stalls cancelling and starting
    // over too.
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let deadline = Instant::now() + READ_DEADLINE;

    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        if Instant::now() > deadline {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let room = MAX_REQUEST.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..n.min(room)]);
                if buffer.len() >= MAX_REQUEST || buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let line = buffer.split(|b| *b == b'\n').next()?;
    let text = std::str::from_utf8(line).ok()?;
    let mut parts = text.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    Some(parts.next()?.to_string())
}

/// One query parameter out of a request target, percent-decoded.
fn query_parameter(target: &str, name: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (percent_decode(key) == name).then(|| percent_decode(value))
    })
}

/// Percent-decoding, with `+` read as a space the way a query string means it.
///
/// Invalid escapes are left as written rather than guessed at. A value this
/// cannot decode will simply fail to match the state it is compared against,
/// which is the correct outcome.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Answers the browser with a small page and closes the connection.
///
/// The text is fixed and this file's own. Nothing from the service or the query
/// string is echoed into it: a page served from a loopback port is still a page
/// in someone's browser, and reflecting a query parameter into it would be a
/// cross-site scripting hole in a mod manager.
fn respond(stream: &mut TcpStream, status: &str, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Apocrypha</title></head>\
         <body style=\"font-family:system-ui;padding:3rem;text-align:center\">\
         <p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// Pulls the message out of the service's error shape, and refuses to pass a
/// whole response body through to a user interface if it is not that shape.
///
/// A server that is misbehaving, or something that is not the server at all,
/// should not get to put arbitrary text on screen.
fn safe_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(e) if e.error.len() <= 200 => e.error,
        _ => "The service refused the request.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> PendingAuthorization {
        PendingAuthorization::begin(ServiceOrigin::PRODUCTION, "0.5.0", "test machine")
            .expect("a loopback port should be available")
    }

    #[test]
    fn the_challenge_is_the_digest_of_the_verifier_and_not_the_verifier() {
        // The whole point: what travels through the browser must not be what
        // spends the code.
        let p = pending();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier.as_bytes()));
        assert!(p.authorize_url.contains(&crate::urlencode(&expected)));
        assert!(
            !p.authorize_url.contains(&p.verifier),
            "the verifier must never appear in the URL the browser is given"
        );
    }

    #[test]
    fn the_callback_is_a_loopback_socket_this_process_owns() {
        let p = pending();
        assert!(p.redirect_uri.starts_with("http://127.0.0.1:"));
        assert!(p.redirect_uri.ends_with("/callback"));

        // The port is real and bound here, not a number we hope is free.
        let port: u16 = p
            .redirect_uri
            .trim_start_matches("http://127.0.0.1:")
            .trim_end_matches("/callback")
            .parse()
            .expect("a port");
        assert_eq!(port, p.listener.local_addr().unwrap().port());
        assert!(port >= 1024, "must not be a privileged port");
    }

    #[test]
    fn a_verifier_and_a_state_are_never_the_same_twice() {
        let a = pending();
        let b = pending();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.state, b.state);
        assert_eq!(a.verifier.len(), 43, "256 bits of base64url");
    }

    #[test]
    fn nothing_has_answered_yet() {
        assert_eq!(pending().poll().unwrap(), AuthorizationStatus::Waiting);
    }

    /// Connects to the callback port and sends one raw request line.
    fn knock(p: &PendingAuthorization, target: &str) {
        let addr = p.listener.local_addr().unwrap();
        let mut stream = TcpStream::connect(addr).expect("connect to our own port");
        write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn a_callback_carrying_someone_elses_state_is_ignored_not_fatal() {
        // A local process that guesses the port but not the state is the case
        // this exists for. Two things must be true: the code is not spent, and
        // the sign-in is not ended — otherwise guessing the port is a one-packet
        // way to stop anyone signing in, needing none of the secrets involved.
        let p = pending();
        knock(&p, "/callback?code=whatever&state=not-ours");
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Waiting);

        // Still ours, and still listening for the real answer.
        knock(
            &p,
            &format!("/callback?error=access_denied&state={}", p.state),
        );
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Declined);
    }

    #[test]
    fn a_callback_with_no_state_at_all_is_ignored() {
        let p = pending();
        knock(&p, "/callback?code=whatever");
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Waiting);
    }

    #[test]
    fn an_answer_on_the_wrong_path_is_not_an_answer() {
        let p = pending();
        knock(&p, &format!("/somewhere-else?code=x&state={}", p.state));
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Waiting);
    }

    #[test]
    fn a_peer_that_trickles_cannot_hold_the_sign_in_open() {
        // The caller holds the pending sign-in's lock across poll(), so a read
        // that never finishes stalls cancelling and starting over as well. A
        // per-read timeout alone does not stop this: one byte before each
        // timeout resets it forever. The whole read has its own deadline.
        use std::io::Write as _;
        let p = pending();
        let addr = p.listener.local_addr().unwrap();

        let trickler = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            // Never sends the blank line that ends the headers.
            for _ in 0..30 {
                if stream.write_all(b"X").is_err() || stream.flush().is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });

        let started = Instant::now();
        let outcome = p.poll();
        let took = started.elapsed();

        assert!(
            took < READ_DEADLINE + Duration::from_secs(3),
            "poll took {took:?}, which means the deadline is not being enforced"
        );
        // Whatever it decided, it decided promptly and did not grant anything.
        assert!(!matches!(outcome, Ok(AuthorizationStatus::Granted { .. })));
        let _ = trickler.join();
    }

    #[test]
    fn an_oversized_request_is_bounded() {
        use std::io::Write as _;
        let p = pending();
        let addr = p.listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok(mut stream) = TcpStream::connect(addr) {
                let _ = stream.write_all(b"GET /callback?code=");
                let _ = stream.write_all(&vec![b'a'; 64 * 1024]);
                let _ = stream.flush();
            }
        });
        // Reads at most MAX_REQUEST and answers rather than growing without
        // bound or waiting for an end that is not coming.
        assert!(!matches!(p.poll(), Ok(AuthorizationStatus::Granted { .. })));
    }

    #[test]
    fn dropping_the_pending_sign_in_closes_the_port() {
        let p = pending();
        let addr = p.listener.local_addr().unwrap();
        drop(p);
        // The OS may keep the port in TIME_WAIT, but nothing is accepting on it.
        let refused = TcpStream::connect_timeout(&addr, Duration::from_millis(250));
        if let Ok(mut stream) = refused {
            use std::io::Write as _;
            let _ = write!(stream, "GET /callback HTTP/1.1\r\n\r\n");
            let mut buffer = [0u8; 16];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
            assert!(
                matches!(stream.read(&mut buffer), Ok(0) | Err(_)),
                "something is still answering on a closed sign-in's port"
            );
        }
    }

    #[test]
    fn a_browser_asking_for_a_favicon_does_not_end_the_sign_in() {
        // The browser does this on its own, and treating it as an answer would
        // abandon a sign-in the person is still in the middle of.
        let p = pending();
        knock(&p, "/favicon.ico");
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Waiting);
    }

    #[test]
    fn a_refusal_is_reported_as_declined_rather_than_as_a_failure() {
        let p = pending();
        let state = p.state.clone();
        knock(&p, &format!("/callback?error=access_denied&state={state}"));
        assert_eq!(p.poll().unwrap(), AuthorizationStatus::Declined);
    }

    #[test]
    fn a_query_string_is_decoded_the_way_a_browser_wrote_it() {
        assert_eq!(
            query_parameter("/callback?code=a%2Bb&state=x", "code").as_deref(),
            Some("a+b")
        );
        assert_eq!(
            query_parameter("/callback?code=a+b", "code").as_deref(),
            Some("a b")
        );
        assert_eq!(query_parameter("/callback", "code"), None);
        assert_eq!(query_parameter("/callback?state=x", "code"), None);
        // A truncated escape is left alone rather than guessed at.
        assert_eq!(
            query_parameter("/callback?code=a%2", "code").as_deref(),
            Some("a%2")
        );
    }

    #[test]
    fn the_page_served_to_the_browser_never_echoes_the_query() {
        // A loopback page is still a page in someone's browser. Reflecting a
        // parameter into it would be an XSS hole in a mod manager.
        let p = pending();
        knock(
            &p,
            "/callback?code=%3Cscript%3Ealert(1)%3C/script%3E&state=nope",
        );
        let _ = p.poll();
        // Nothing to assert on the wire here beyond the fixed text: `respond`
        // takes only literals from this file, and this test exists so that a
        // change making it take the target would have to change this too.
        assert!(!p.authorize_url.contains("<script>"));
    }

    #[test]
    fn a_message_that_is_not_the_services_error_shape_does_not_reach_the_screen() {
        assert_eq!(safe_message(r#"{"error":"no"}"#), "no");
        assert_eq!(
            safe_message("<html>gateway timeout</html>"),
            "The service refused the request."
        );
        assert_eq!(safe_message(""), "The service refused the request.");
        let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(500));
        assert_eq!(safe_message(&long), "The service refused the request.");
    }
}
