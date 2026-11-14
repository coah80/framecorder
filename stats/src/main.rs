//! framecorder's stats service, three hosts on one port behind a tunnel:
//! the discord link (counts the visit, sends it on to the invite), the ping
//! endpoint the apps call once a day, and the locked dashboard.
//!
//! `framecorder-stats setup <dir>` makes a new password and authenticator
//! secret for the dashboard: what to type goes in `<dir>/login.txt`, what the
//! service needs in `<dir>/auth.env`, and neither is printed.

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

/// `__Host-` keeps any other coah80.com site from setting or overwriting it.
const COOKIE: &str = "__Host-id";
/// The days of counts the dashboard gets.
const WINDOW_DAYS: u64 = 90;
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
    /// The dashboard's built files, see `stats/ui`.
    ui: PathBuf,
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
    /// `Sec-Fetch-Site`, what the browser says about where a request came from.
    fetch_site: String,
    body: Vec<u8>,
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

fn text(status: u16, body: &str) -> Reply {
    Reply { status, headers: vec![("Content-Type", "text/plain; charset=utf-8".into())], body: body.into() }
}

/// Everything the dashboard's host sends goes out with these: only its own
/// files load and run, nothing may frame it or open it as a popup's parent,
/// no other site may read it, browsers stick to https, and it stays out of
/// search engines.
fn locked_down(mut reply: Reply) -> Reply {
    let csp = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";
    reply.headers.extend([
        ("Content-Security-Policy", csp.to_owned()),
        ("X-Frame-Options", "DENY".to_owned()),
        ("Strict-Transport-Security", "max-age=63072000; includeSubDomains".to_owned()),
        ("Cross-Origin-Opener-Policy", "same-origin".to_owned()),
        ("Cross-Origin-Resource-Policy", "same-origin".to_owned()),
        ("Permissions-Policy", "geolocation=(), camera=(), microphone=(), payment=(), usb=(), interest-cohort=()".to_owned()),
        ("X-Robots-Tag", "noindex, nofollow".to_owned()),
    ]);
    reply
}

fn html(status: u16, body: String) -> Reply {
    Reply { status, headers: vec![("Content-Type", "text/html; charset=utf-8".into())], body: body.into_bytes() }
}

fn redirect(to: &str, cookie: Option<String>) -> Reply {
    let mut headers = vec![("Location", to.to_owned())];
    if let Some(cookie) = cookie {
        headers.push(("Set-Cookie", cookie));
    }
    Reply { status: 303, headers, body: Vec::new() }
}

/// One of the dashboard's built files by name. Names are letters, digits,
/// dots, dashes and underscores, so nothing outside the folder is reachable.
fn file(dir: &std::path::Path, name: &str) -> Option<Reply> {
    let plain = !name.is_empty() && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    let mime = match name.rsplit_once('.')?.1 {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        _ => return None,
    };
    let body = plain.then(|| std::fs::read(dir.join(name)).ok()).flatten()?;
    Some(Reply { status: 200, headers: vec![("Content-Type", mime.into())], body })
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
        Reply { status: 302, headers: vec![("Location", self.config.invite.clone())], body: Vec::new() }
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
                Reply { status: 204, headers: Vec::new(), body: Vec::new() }
            }
            _ => text(404, "not found"),
        }
    }

    fn dash(&self, r: &Request) -> Reply {
        locked_down(self.dash_route(r))
    }

    fn dash_route(&self, r: &Request) -> Reply {
        let token = r.cookie.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == COOKIE).map_or("", |(_, v)| v);
        let time = now();
        let signed_in = self.auth.lock().expect("auth lock").valid(token, &r.agent, time);
        // a form post has to come from this page: its own origin, and not a cross-site request
        let ours = r.origin == format!("https://{}", self.config.dash_host) && matches!(r.fetch_site.as_str(), "" | "same-origin");
        let cookie = |value: &str, age: u64| format!("{COOKIE}={value}; Max-Age={age}; Path=/; HttpOnly; Secure; SameSite=Strict");
        match (r.method.as_str(), r.path.as_str()) {
            // what the login page itself needs, and nothing more, is open
            ("GET", "/login.css") => file(&self.ui, "login.css").unwrap_or_else(|| text(404, "not found")),
            ("GET", path) if path.starts_with("/fonts/") => file(&self.ui, &path[7..]).filter(|_| path.ends_with(".woff2")).unwrap_or_else(|| text(404, "not found")),
            ("GET", "/robots.txt") => text(200, "User-agent: *\nDisallow: /\n"),
            ("GET", "/") if signed_in => file(&self.ui, "index.html").unwrap_or_else(|| text(503, "the dashboard isn't installed")),
            ("GET", "/") => html(200, dash::login("")),
            ("GET", "/api/stats") if signed_in => {
                let today = store::day(time);
                let from = store::day(time.saturating_sub(WINDOW_DAYS * 86400));
                let store = self.store.lock().expect("store lock");
                let days: serde_json::Map<String, serde_json::Value> = store.days.range(from..).map(|(day, counts)| (day.clone(), serde_json::json!(counts))).collect();
                let body = serde_json::json!({"today": today, "days": days}).to_string();
                Reply { status: 200, headers: vec![("Content-Type", "application/json".into())], body: body.into_bytes() }
            }
            ("GET", path) if signed_in && path.starts_with("/assets/") => file(&self.ui, &path[8..]).unwrap_or_else(|| text(404, "not found")),
            ("POST", "/login") if ours => {
                let form = String::from_utf8_lossy(&r.body);
                let (password, code) = (field(&form, "password").unwrap_or_default(), field(&form, "code").unwrap_or_default());
                let outcome = self.auth.lock().expect("auth lock").login(r.ip, &password, &code, &r.agent, time);
                let (counter, reply) = match outcome {
                    Login::In(token) => ("auth:in", redirect("/", Some(cookie(&token, auth::SESSION_SECS)))),
                    Login::Wrong => ("auth:wrong", html(401, dash::login("That's not it."))),
                    Login::Locked => ("auth:locked", html(429, dash::login("Too many tries. Wait fifteen minutes."))),
                };
                // how often the door's been tried shows on the dashboard itself
                self.store.lock().expect("store lock").add(&store::day(time), counter, 1);
                eprintln!("dashboard login: {counter}");
                reply
            }
            ("POST", "/logout") if ours => {
                self.auth.lock().expect("auth lock").logout(token);
                let mut reply = redirect("/", Some(cookie("", 0)));
                reply.headers.push(("Clear-Site-Data", "\"cache\", \"cookies\", \"storage\"".to_owned()));
                reply
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
                    fetch_site: header("Sec-Fetch-Site"),
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
        let _ = stream.write_all(&reply.body);
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

/// Makes a new password and authenticator secret. Nothing secret is printed.
fn setup(dir: &std::path::Path) -> Result<()> {
    use ring::rand::SecureRandom;
    use std::os::unix::fs::OpenOptionsExt;
    let rng = ring::rand::SystemRandom::new();
    let (mut password, mut secret) = ([0; 20], [0; 20]);
    rng.fill(&mut password).and_then(|_| rng.fill(&mut secret)).map_err(|_| anyhow::anyhow!("no random numbers"))?;
    // 160 random bits each; the password is spelled in base32 so it's typable
    let (password, secret) = (auth::base32(&password), auth::base32(&secret));
    let uri = format!("otpauth://totp/framecorder:stats?secret={secret}&issuer=framecorder&algorithm=SHA1&digits=6&period=30");
    let write = |name: &str, body: String| -> Result<()> {
        let path = dir.join(name);
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).with_context(|| format!("writing {}", path.display()))?;
        file.write_all(body.as_bytes())?;
        Ok(())
    };
    std::fs::create_dir_all(dir)?;
    write("login.txt", format!("password: {password}\nauthenticator: {uri}\n"))?;
    write("auth.env", format!("STATS_PASSWORD_HASH={}\nSTATS_TOTP_SECRET={secret}\n", auth::hash_password(&password)?))?;
    println!("wrote login.txt and auth.env in {}", dir.display());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => {}
        ["setup", dir] => return setup(std::path::Path::new(dir)),
        _ => anyhow::bail!("usage: framecorder-stats [setup <dir>]"),
    }
    let port: u16 = env("STATS_PORT")?.parse().context("STATS_PORT isn't a port")?;
    let dir = PathBuf::from(env("STATS_DIR")?);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let service = Arc::new(Service {
        config: Config { discord_host: env("STATS_DISCORD_HOST")?, ping_host: env("STATS_PING_HOST")?, dash_host: env("STATS_DASH_HOST")?, invite: env("STATS_DISCORD_INVITE")? },
        ui: PathBuf::from(env("STATS_UI_DIR")?),
        store: Mutex::new(Store::load(dir.join("days.json"))?),
        auth: Mutex::new(Auth::new(&env("STATS_PASSWORD_HASH")?, &env("STATS_TOTP_SECRET")?)?),
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

    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn service() -> Service {
        let dir = std::env::temp_dir().join(format!("fc-stats-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/index.html"), "<title>the dashboard</title>").unwrap();
        std::fs::write(dir.join("ui/app.js"), "let dashboard").unwrap();
        std::fs::write(dir.join("ui/login.css"), "body{}").unwrap();
        std::fs::write(dir.join("secret.js"), "outside the folder").unwrap();
        Service {
            config: Config { discord_host: "discord.test".into(), ping_host: "stats.test".into(), dash_host: "db.test".into(), invite: "https://discord.gg/x".into() },
            ui: dir.join("ui"),
            store: Mutex::new(Store::load(dir.join("days.json")).unwrap()),
            auth: Mutex::new(Auth::new(&auth::hash_password("open sesame").unwrap(), SECRET).unwrap()),
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
            fetch_site: String::new(),
            body: Vec::new(),
        }
    }

    fn header<'a>(reply: &'a Reply, name: &str) -> &'a str {
        reply.headers.iter().find(|(n, _)| *n == name).map_or("", |(_, v)| v)
    }

    fn body(reply: &Reply) -> String {
        String::from_utf8_lossy(&reply.body).into_owned()
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
    fn the_dashboard_and_its_numbers_only_open_for_the_password_and_code_from_its_own_page() {
        let s = service();
        let code = || {
            // the authenticator's code for now, worked out the way a phone does
            let mac = ring::hmac::sign(&ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, b"12345678901234567890"), &(now() / 30).to_be_bytes());
            let (mac, at) = (mac.as_ref(), (mac.as_ref()[19] & 15) as usize);
            format!("{:06}", u32::from_be_bytes([mac[at] & 0x7f, mac[at + 1], mac[at + 2], mac[at + 3]]) % 1_000_000)
        };
        let login = |password: &str, code: &str, origin: &str, site: &str| {
            s.route(&Request { origin: origin.into(), fetch_site: site.into(), body: format!("password={password}&code={code}").into_bytes(), ..request("POST", "db.test", "/login") })
        };
        // shut: the page is the login, and neither the app nor the counts are served
        let shut = s.route(&request("GET", "db.test", "/"));
        assert!(body(&shut).contains("name=\"password\"") && body(&shut).contains("name=\"code\"") && !body(&shut).contains("the dashboard"));
        for path in ["/api/stats", "/assets/app.js", "/assets/index.html", "/days.json", "/assets/..%2Fsecret.js", "/assets/../secret.js"] {
            assert_eq!(s.route(&request("GET", "db.test", path)).status, 404, "{path}");
        }
        assert_eq!(s.route(&request("GET", "stats.test", "/")).status, 404);
        // every answer from this host carries the lockdown headers
        assert!(header(&shut, "Content-Security-Policy").contains("default-src 'none'") && !header(&shut, "Content-Security-Policy").contains("unsafe"));
        assert_eq!((header(&shut, "X-Frame-Options"), header(&shut, "Cross-Origin-Opener-Policy")), ("DENY", "same-origin"));
        assert_eq!(login("guess", &code(), "https://db.test", "same-origin").status, 401);
        assert_eq!(login("open+sesame", "000000", "https://db.test", "same-origin").status, 401);
        // the right pair posted from another site goes nowhere
        assert_eq!(login("open+sesame", &code(), "https://evil.test", "cross-site").status, 404);
        assert_eq!(login("open+sesame", &code(), "https://db.test", "cross-site").status, 404);
        let reply = login("open+sesame", &code(), "https://db.test", "same-origin");
        assert_eq!(reply.status, 303);
        let cookie = header(&reply, "Set-Cookie");
        assert!(cookie.starts_with("__Host-id=") && cookie.contains("Path=/") && cookie.contains("HttpOnly") && cookie.contains("Secure") && cookie.contains("SameSite=Strict") && !cookie.contains("Domain"));
        let with = |target: &str| s.route(&Request { cookie: cookie.split(';').next().unwrap().into(), ..request("GET", "db.test", target) });
        assert!(body(&with("/")).contains("the dashboard"));
        assert!(body(&with("/api/stats")).contains("\"auth:wrong\":2"));
        assert_eq!(body(&with("/assets/app.js")), "let dashboard");
        // signed in or not, nothing outside the folder
        assert_eq!(with("/assets/../secret.js").status, 404);
        // a made-up cookie doesn't open it
        let forged = s.route(&Request { cookie: format!("{COOKIE}={}", "a".repeat(64)), ..request("GET", "db.test", "/api/stats") });
        assert_eq!(forged.status, 404);
        // locking clears the browser and ends the session
        let out = s.route(&Request { cookie: cookie.split(';').next().unwrap().into(), origin: "https://db.test".into(), ..request("POST", "db.test", "/logout") });
        assert!(header(&out, "Clear-Site-Data").contains("cookies"));
        assert_eq!(with("/api/stats").status, 404);
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
