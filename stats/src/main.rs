//! framecorder's stats service, three hosts on one port behind a tunnel:
//! the discord link (counts the visit, sends it on to the invite), the ping
//! endpoint the apps call once a day, and the locked dashboard.

mod auth;
mod dash;
mod ping;
mod store;

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use auth::{Auth, Login};
use store::Store;

const COOKIE: &str = "fc_stats";
/// Pings one address may send a day: a household with a few installs, with room.
const PINGS_A_DAY: usize = 24;
/// Where a discord link click can say it came from.
const SOURCES: [&str; 5] = ["desktop", "android", "headset", "site", "readme"];

struct Config {
    discord_host: String,
    ping_host: String,
    dash_host: String,
    invite: String,
}

struct Service {
    config: Config,
    store: Mutex<Store>,
    auth: Mutex<Auth>,
    pings: Mutex<HashMap<IpAddr, VecDeque<u64>>>,
}

struct Request {
    method: String,
    path: String,
    query: String,
    host: String,
    ip: IpAddr,
    country: String,
    agent: String,
    cookie: String,
    origin: String,
    body: Vec<u8>,
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: String,
}

fn text(status: u16, body: &str) -> Reply {
    Reply { status, headers: vec![("Content-Type", "text/plain; charset=utf-8".into())], body: body.into() }
}

fn html(status: u16, body: String) -> Reply {
    // nothing on the page comes from anywhere else, and nothing may frame it
    let csp = "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";
    Reply {
        status,
        headers: vec![
            ("Content-Type", "text/html; charset=utf-8".into()),
            ("Content-Security-Policy", csp.into()),
            ("X-Frame-Options", "DENY".into()),
            ("Strict-Transport-Security", "max-age=31536000".into()),
        ],
        body,
    }
}

fn redirect(to: &str, cookie: Option<String>) -> Reply {
    let mut headers = vec![("Location", to.to_owned())];
    if let Some(cookie) = cookie {
        headers.push(("Set-Cookie", cookie));
    }
    Reply { status: 303, headers, body: String::new() }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// One `key=value` out of a query string or a form body, percent-decoded.
fn field(pairs: &str, key: &str) -> Option<String> {
    pairs.split('&').find_map(|pair| pair.split_once('=').filter(|(k, _)| *k == key)).map(|(_, v)| {
        let (mut out, bytes, mut i) = (Vec::new(), v.as_bytes(), 0);
        while i < bytes.len() {
            match bytes[i] {
                b'+' => out.push(b' '),
                b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&v[i + 1..i + 3], 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                },
                b => out.push(b),
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    })
}

impl Service {
    fn route(&self, r: &Request) -> Reply {
        let c = &self.config;
        if r.host == c.discord_host {
            self.discord(r)
        } else if r.host == c.ping_host {
            self.ping(r)
        } else if r.host == c.dash_host {
            self.dash(r)
        } else {
            text(404, "not found")
        }
    }

    /// The discord link: counts where the click came from and which country,
    /// then sends it on. Link previews and crawlers aren't people.
    fn discord(&self, r: &Request) -> Reply {
        if r.method != "GET" && r.method != "HEAD" {
            return text(405, "method not allowed");
        }
        let agent = r.agent.to_ascii_lowercase();
        let person = !agent.is_empty() && !["bot", "crawl", "spider", "preview", "curl", "wget", "python", "http"].iter().any(|w| agent.contains(w));
        if r.method == "GET" && person {
            let from = field(&r.query, "from").filter(|f| SOURCES.contains(&f.as_str())).unwrap_or_else(|| "elsewhere".into());
            let country = if r.country.len() == 2 && r.country.bytes().all(|b| b.is_ascii_uppercase()) { r.country.as_str() } else { "??" };
            let day = store::day(now());
            let mut store = self.store.lock().expect("store lock");
            store.add(&day, &format!("discord:from={from}"), 1);
            store.add(&day, &format!("discord:country={country}"), 1);
        }
        Reply { status: 302, headers: vec![("Location", self.config.invite.clone())], body: String::new() }
    }

    fn ping(&self, r: &Request) -> Reply {
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/health") => text(200, "ok"),
            ("POST", "/v1/ping") => {
                let counters = match ping::counters(&r.body) {
                    Ok(c) => c,
                    Err(why) => return text(400, why),
                };
                let time = now();
                {
                    // the address is only held here, in memory, to cap how many pings it can send
                    let mut pings = self.pings.lock().expect("ping limiter lock");
                    pings.retain(|_, times| {
                        times.retain(|t| time.saturating_sub(*t) < 86400);
                        !times.is_empty()
                    });
                    let times = pings.entry(r.ip).or_default();
                    if times.len() >= PINGS_A_DAY {
                        return text(429, "that's enough for today");
                    }
                    times.push_back(time);
                }
                let day = store::day(time);
                let mut store = self.store.lock().expect("store lock");
                for (key, n) in counters {
                    store.add(&day, &key, n);
                }
                Reply { status: 204, headers: Vec::new(), body: String::new() }
            }
            _ => text(404, "not found"),
        }
    }

    fn dash(&self, r: &Request) -> Reply {
        let token = r.cookie.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == COOKIE).map_or("", |(_, v)| v);
        let time = now();
        let signed_in = self.auth.lock().expect("auth lock").valid(token, time);
        // a form post has to come from this page
        let ours = r.origin == format!("https://{}", self.config.dash_host);
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/") if signed_in => {
                let window: Vec<String> = (0..30).rev().map(|back| store::day(time.saturating_sub(back * 86400))).collect();
                html(200, dash::dashboard(&self.store.lock().expect("store lock").days, &window))
            }
            ("GET", "/") => html(200, dash::login("")),
            ("POST", "/login") if ours => {
                let password = field(&String::from_utf8_lossy(&r.body), "password").unwrap_or_default();
                match self.auth.lock().expect("auth lock").login(r.ip, &password, time) {
                    Login::In(token) => redirect("/", Some(format!("{COOKIE}={token}; Max-Age={}; Path=/; HttpOnly; Secure; SameSite=Strict", auth::SESSION_SECS))),
                    Login::Wrong => html(401, dash::login("That's not it.")),
                    Login::Locked => html(429, dash::login("Too many tries. Wait fifteen minutes.")),
                }
            }
            ("POST", "/logout") if ours => {
                self.auth.lock().expect("auth lock").logout(token);
                redirect("/", Some(format!("{COOKIE}=; Max-Age=0; Path=/; HttpOnly; Secure; SameSite=Strict")))
            }
            _ => text(404, "not found"),
        }
    }
}

fn receive(stream: &mut TcpStream) -> Option<Request> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return None;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
        let mut headers = [httparse::EMPTY_HEADER; 48];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&bytes) {
            Ok(httparse::Status::Complete(end)) => {
                let header = |name: &str| req.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).and_then(|h| std::str::from_utf8(h.value).ok()).unwrap_or_default().to_owned();
                if req.headers.iter().any(|h| h.name.eq_ignore_ascii_case("Transfer-Encoding")) {
                    return None;
                }
                let length: usize = match header("Content-Length").as_str() {
                    "" => 0,
                    n => n.parse().ok()?,
                };
                // a ping or a password, never more
                if length > ping::MAX_BODY {
                    return None;
                }
                let target = req.path.unwrap_or_default();
                let (path, query) = target.split_once('?').unwrap_or((target, ""));
                let host = header("Host").to_ascii_lowercase();
                let request = Request {
                    method: req.method.unwrap_or_default().to_owned(),
                    path: path.to_owned(),
                    query: query.to_owned(),
                    host: host.split(':').next().unwrap_or_default().to_owned(),
                    ip: header("CF-Connecting-IP").parse().ok().or_else(|| stream.peer_addr().ok().map(|a| a.ip()))?,
                    country: header("CF-IPCountry"),
                    agent: header("User-Agent"),
                    cookie: header("Cookie"),
                    origin: header("Origin"),
                    body: Vec::new(),
                };
                let mut body = bytes[end..].to_vec();
                while body.len() < length {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
                        return None;
                    }
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => return None,
                        Ok(n) => body.extend_from_slice(&buf[..n]),
                    }
                }
                body.truncate(length);
                return Some(Request { body, ..request });
            }
            Ok(httparse::Status::Partial) if bytes.len() <= 16384 => {}
            _ => return None,
        }
    }
}

fn respond(stream: &mut TcpStream, reply: Reply, head_only: bool) {
    let reason = match reply.status {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    let mut head = format!("HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n", reply.status, reply.body.len());
    for (name, value) in &reply.headers {
        head += &format!("{name}: {value}\r\n");
    }
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(b"\r\n");
    if !head_only {
        let _ = stream.write_all(reply.body.as_bytes());
    }
}

fn handle(mut stream: TcpStream, service: &Service) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    match receive(&mut stream) {
        Some(request) => {
            let reply = service.route(&request);
            respond(&mut stream, reply, request.method == "HEAD");
        }
        None => respond(&mut stream, text(400, "bad request"), false),
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} isn't set"))
}

fn main() -> Result<()> {
    let port: u16 = env("STATS_PORT")?.parse().context("STATS_PORT isn't a port")?;
    let dir = PathBuf::from(env("STATS_DIR")?);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let service = Arc::new(Service {
        config: Config { discord_host: env("STATS_DISCORD_HOST")?, ping_host: env("STATS_PING_HOST")?, dash_host: env("STATS_DASH_HOST")?, invite: env("STATS_DISCORD_INVITE")? },
        store: Mutex::new(Store::load(dir.join("days.json"))?),
        auth: Mutex::new(Auth::new(&env("STATS_PASSWORD_SHA256")?)?),
        pings: Mutex::new(HashMap::new()),
    });
    // only the tunnel on this machine can reach it
    let listener = TcpListener::bind(("127.0.0.1", port)).context("opening the stats port")?;
    eprintln!("framecorder-stats listening on {}", listener.local_addr()?);
    {
        let service = service.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            if let Err(e) = service.store.lock().expect("store lock").save() {
                eprintln!("couldn't save the counts: {e:#}");
            }
        });
    }
    let (send, queue) = mpsc::sync_channel::<TcpStream>(32);
    let queue = Arc::new(Mutex::new(queue));
    for _ in 0..4 {
        let (queue, service) = (queue.clone(), service.clone());
        std::thread::spawn(move || loop {
            let stream = queue.lock().expect("connection queue lock").recv();
            match stream {
                Ok(stream) => handle(stream, &service),
                Err(_) => break,
            }
        });
    }
    for stream in listener.incoming().flatten() {
        if let Err(mpsc::TrySendError::Full(mut stream)) = send.try_send(stream) {
            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
            respond(&mut stream, text(503, "busy"), false);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Service {
        let dir = std::env::temp_dir().join(format!("fc-stats-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // sha256 of "open sesame"
        let password = "41ef4bb0b23661e66301aac36066912dac037827b4ae63a7b1165a5aa93ed4eb";
        Service {
            config: Config { discord_host: "discord.test".into(), ping_host: "stats.test".into(), dash_host: "db.test".into(), invite: "https://discord.gg/x".into() },
            store: Mutex::new(Store::load(dir.join("days.json")).unwrap()),
            auth: Mutex::new(Auth::new(password).unwrap()),
            pings: Mutex::new(HashMap::new()),
        }
    }

    fn request(method: &str, host: &str, target: &str) -> Request {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            host: host.into(),
            ip: "10.0.0.1".parse().unwrap(),
            country: "US".into(),
            agent: "Mozilla/5.0".into(),
            cookie: String::new(),
            origin: String::new(),
            body: Vec::new(),
        }
    }

    fn header<'a>(reply: &'a Reply, name: &str) -> &'a str {
        reply.headers.iter().find(|(n, _)| *n == name).map_or("", |(_, v)| v)
    }

    #[test]
    fn the_discord_link_counts_people_and_always_sends_them_on() {
        let s = service();
        let reply = s.route(&request("GET", "discord.test", "/?from=desktop"));
        assert_eq!((reply.status, header(&reply, "Location")), (302, "https://discord.gg/x"));
        // a made-up source, and a link preview, still get the invite
        assert_eq!(s.route(&request("GET", "discord.test", "/?from=%3Cscript%3E")).status, 302);
        let preview = Request { agent: "Mozilla/5.0 (compatible; Discordbot/2.0)".into(), ..request("GET", "discord.test", "/") };
        assert_eq!(s.route(&preview).status, 302);
        let days = &s.store.lock().unwrap().days;
        let today: Vec<(&str, u64)> = days.values().next().unwrap().iter().map(|(k, n)| (k.as_str(), *n)).collect();
        assert_eq!(today, [("discord:country=US", 2), ("discord:from=desktop", 1), ("discord:from=elsewhere", 1)]);
    }

    #[test]
    fn the_dashboard_only_opens_for_the_password_from_its_own_page() {
        let s = service();
        let login = |password: &str, origin: &str| {
            s.route(&Request { origin: origin.into(), body: format!("password={password}").into_bytes(), ..request("POST", "db.test", "/login") })
        };
        // shut: the page is the login, and the counts aren't anywhere else
        let shut = s.route(&request("GET", "db.test", "/"));
        assert!(shut.body.contains("name=\"password\"") && !shut.body.contains("Installs"));
        assert_eq!(s.route(&request("GET", "stats.test", "/")).status, 404);
        assert_eq!(s.route(&request("GET", "db.test", "/days.json")).status, 404);
        assert_eq!(login("guess", "https://db.test").status, 401);
        // the right password posted from another site goes nowhere
        assert_eq!(login("open+sesame", "https://evil.test").status, 404);
        let reply = login("open+sesame", "https://db.test");
        assert_eq!(reply.status, 303);
        let cookie = header(&reply, "Set-Cookie");
        assert!(cookie.contains("HttpOnly") && cookie.contains("Secure") && cookie.contains("SameSite=Strict"));
        let open = s.route(&Request { cookie: cookie.split(';').next().unwrap().into(), ..request("GET", "db.test", "/") });
        assert!(open.status == 200 && open.body.contains("Installs that ran each day"));
        // a made-up cookie doesn't
        let forged = s.route(&Request { cookie: format!("{COOKIE}={}", "a".repeat(64)), ..request("GET", "db.test", "/") });
        assert!(!forged.body.contains("Installs"));
    }

    #[test]
    fn pings_count_once_each_and_an_address_runs_out() {
        let s = service();
        let ping = || s.route(&Request { body: br#"{"app":"desktop","version":"0.2.0","os":"linux","new":true}"#.to_vec(), ..request("POST", "stats.test", "/v1/ping") });
        for _ in 0..PINGS_A_DAY {
            assert_eq!(ping().status, 204);
        }
        assert_eq!(ping().status, 429);
        assert_eq!(s.route(&Request { body: b"{}".to_vec(), ..request("POST", "stats.test", "/v1/ping") }).status, 400);
        let days = &s.store.lock().unwrap().days;
        assert_eq!(days.values().next().unwrap()["active:desktop"], PINGS_A_DAY as u64);
    }
}
