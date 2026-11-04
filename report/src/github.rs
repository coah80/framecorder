use anyhow::{anyhow, bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ring::{
    rand::SystemRandom,
    signature::{RsaKeyPair, RSA_PKCS1_SHA256},
};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub repo: String,
    pub state: PathBuf,
    pub api: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let env = |key, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
        let state = match std::env::var_os("STATE_DIR") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(
                std::env::var_os("HOME").context("Set HOME or STATE_DIR to find the bot files.")?,
            )
            .join(".local/state/framecorder-report"),
        };
        let c = Self {
            port: env("PORT", "4791")
                .parse()
                .context("PORT must be a port number")?,
            repo: env("REPO", "coah80/framecorder"),
            state,
            api: env("GITHUB_API", "https://api.github.com")
                .trim_end_matches('/')
                .to_owned(),
        };
        if c.repo.split('/').count() != 2
            || !c
                .repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-/.".contains(c))
        {
            bail!("REPO must be owner/repository");
        }
        {
            let u = url::Url::parse(&c.api).context("invalid service URL")?;
            if !["https", "http"].contains(&u.scheme())
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
            {
                bail!(
                    "service URLs must be HTTP or HTTPS URLs without credentials or query strings"
                );
            }
        }
        Ok(c)
    }
}

struct Credentials {
    id: u64,
    slug: String,
    key: RsaKeyPair,
}

impl Credentials {
    fn load(state: &Path) -> Result<Self> {
        let env = fs::read_to_string(state.join("app.env")).context(
            "Cannot read STATE_DIR/app.env. Add APP_ID and APP_SLUG for the GitHub App.",
        )?;
        let mut values = std::collections::HashMap::new();
        for line in env
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
        {
            let (name, value) = line
                .split_once('=')
                .context("app.env must contain NAME=value lines.")?;
            if values
                .insert(name.trim(), value.trim().trim_matches(['\'', '"']))
                .is_some()
            {
                bail!("app.env has a repeated setting. Keep one APP_ID and one APP_SLUG.");
            }
        }
        let id = values
            .get("APP_ID")
            .context("Add APP_ID to app.env.")?
            .parse::<u64>()
            .context("APP_ID must be a number.")?;
        let slug = values
            .get("APP_SLUG")
            .context("Add APP_SLUG to app.env.")?
            .to_string();
        if id == 0
            || slug.is_empty()
            || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("Set a valid APP_ID and APP_SLUG in app.env.");
        }
        let path = state.join("github-app.pem");
        let metadata = fs::metadata(&path).context("Cannot read STATE_DIR/github-app.pem. Add the GitHub App's PKCS#1 RSA key with mode 0600.")?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o777 != 0o600 {
            bail!(
                "github-app.pem must be a regular file with mode 0600. Run chmod 600 on that file."
            );
        }
        let pem = fs::read_to_string(path)
            .context("Cannot read github-app.pem. Check its owner and permissions.")?;
        let data: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = STANDARD
            .decode(data)
            .context("Cannot read github-app.pem. Supply a PKCS#1 RSA PEM key.")?;
        let key = RsaKeyPair::from_der(&der).map_err(|_| anyhow!("github-app.pem is not a valid PKCS#1 RSA key. Replace it with the GitHub App's key."))?;
        Ok(Self { id, slug, key })
    }
}

pub struct GitHub {
    pub config: Config,
    credentials: Credentials,
    agent: ureq::Agent,
    token: Option<(String, u64)>,
    label_ready: bool,
}

impl GitHub {
    pub fn new(config: Config) -> Result<Self> {
        let credentials = Credentials::load(&config.state)?;
        Ok(Self {
            credentials,
            config,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(20))
                .redirects(0)
                .build(),
            token: None,
            label_ready: false,
        })
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut req = self
            .agent
            .request(method, &format!("{}{path}", self.config.api))
            .set("User-Agent", "framecorder-report/0.1")
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = bearer {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        let response = if let Some(body) = body {
            req.send_json(body)
        } else {
            req.call()
        }?;
        serde_json::from_reader(response.into_reader().take(1024 * 1024))
            .context("reading GitHub's answer")
    }

    pub fn health(&mut self) -> Result<String> {
        self.token()?;
        Ok(self.credentials.slug.clone())
    }

    fn token(&mut self) -> Result<String> {
        if let Some((token, expiry)) = &self.token {
            if *expiry > now() + 60 {
                return Ok(token.clone());
            }
        }
        let jwt = jwt(&self.credentials)?;
        let install = self.request(
            "GET",
            &format!("/repos/{}/installation", self.config.repo),
            Some(&jwt),
            None,
        )?;
        let id = install["id"]
            .as_u64()
            .context("GitHub did not find this bot's installation")?;
        let reply = self.request(
            "POST",
            &format!("/app/installations/{id}/access_tokens"),
            Some(&jwt),
            Some(json!({})),
        )?;
        let token = reply["token"]
            .as_str()
            .context("GitHub did not return a bot token")?
            .to_owned();
        let expires = chrono::DateTime::parse_from_rfc3339(
            reply["expires_at"]
                .as_str()
                .context("missing token expiry")?,
        )?
        .timestamp();
        self.token = Some((token.clone(), expires.max(0) as u64));
        Ok(token)
    }

    pub fn issue(&mut self, title: String, body: String) -> Result<Value> {
        let token = self.token()?;
        let repo = format!("/repos/{}", self.config.repo);
        if !self.label_ready {
            match self.request(
                "GET",
                &format!("{repo}/labels/from%20the%20app"),
                Some(&token),
                None,
            ) {
                Ok(_) => {}
                Err(e)
                    if e.downcast_ref::<ureq::Error>()
                        .is_some_and(|e| matches!(e, ureq::Error::Status(404, _))) =>
                {
                    self.request(
                        "POST",
                        &format!("{repo}/labels"),
                        Some(&token),
                        Some(json!({"name":"from the app","color":"cba6f7"})),
                    )?;
                }
                Err(e) => return Err(e),
            }
            self.label_ready = true;
        }
        let reply = self.request(
            "POST",
            &format!("{repo}/issues"),
            Some(&token),
            Some(json!({"title":title,"body":body,"labels":["from the app"]})),
        );
        match reply {
            Ok(v) => Ok(
                json!({"ok":true,"number": v["number"].as_u64().context("missing issue number")?, "url": v["html_url"].as_str().context("missing issue URL")?}),
            ),
            Err(e) => {
                self.token = None;
                Err(e)
            }
        }
    }
}

use std::io::Read;
fn jwt(c: &Credentials) -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &json!({"iat":now().saturating_sub(60), "exp":now()+540,"iss":c.id}),
    )?);
    let payload = format!("{header}.{claims}");
    let mut signature = vec![0; c.key.public().modulus_len()];
    c.key
        .sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            payload.as_bytes(),
            &mut signature,
        )
        .map_err(|_| anyhow!("could not sign the bot request"))?;
    Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
