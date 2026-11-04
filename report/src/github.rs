use anyhow::{anyhow, bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ring::{
    rand::SystemRandom,
    signature::{RsaKeyPair, RSA_PKCS1_SHA256},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
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
    pub public_url: String,
    pub repo: String,
    pub state: PathBuf,
    pub api: String,
    pub app_name: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let env = |key, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_owned());
        let c = Self {
            port: env("PORT", "4791")
                .parse()
                .context("PORT must be a port number")?,
            public_url: env("PUBLIC_URL", "https://framecorder-report.coah80.com")
                .trim_end_matches('/')
                .to_owned(),
            repo: env("REPO", "coah80/framecorder"),
            state: PathBuf::from(env("STATE_DIR", "state")),
            api: env("GITHUB_API", "https://api.github.com")
                .trim_end_matches('/')
                .to_owned(),
            app_name: env("APP_NAME", "framecorder-bot"),
        };
        if c.repo.split('/').count() != 2
            || !c
                .repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-/.".contains(c))
        {
            bail!("REPO must be owner/repository");
        }
        for value in [&c.public_url, &c.api] {
            let u = url::Url::parse(value).context("invalid service URL")?;
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
        fs::create_dir_all(&c.state).context("creating the bot state folder")?;
        fs::set_permissions(&c.state, fs::Permissions::from_mode(0o700))?;
        Ok(c)
    }
}

pub fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .context("saving bot setup")?;
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    f.write_all(&serde_json::to_vec(value)?)?;
    f.sync_all()?;
    fs::rename(tmp, path).context("finishing bot setup")
}

#[derive(Serialize, Deserialize)]
pub struct Credentials {
    pub id: u64,
    pub slug: String,
    pub pem: String,
}

pub struct GitHub {
    pub config: Config,
    agent: ureq::Agent,
    token: Option<(String, u64)>,
    label_ready: bool,
}

impl GitHub {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(20))
                .redirects(0)
                .build(),
            token: None,
            label_ready: false,
        }
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

    pub fn convert(&self, code: &str) -> Result<Credentials> {
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            bail!("invalid setup code");
        }
        let v = self.request(
            "POST",
            &format!("/app-manifests/{code}/conversions"),
            None,
            Some(json!({})),
        )?;
        let c: Credentials =
            serde_json::from_value(v).context("reading the new bot's credentials")?;
        if c.slug.is_empty()
            || !c
                .slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("invalid bot name");
        }
        jwt(&c)?;
        save(&self.config.state.join("app.json"), &c)?;
        Ok(c)
    }

    fn token(&mut self) -> Result<String> {
        if let Some((token, expiry)) = &self.token {
            if *expiry > now() + 60 {
                return Ok(token.clone());
            }
        }
        let c: Credentials = serde_json::from_slice(
            &fs::read(self.config.state.join("app.json")).context("the bot has not been set up")?,
        )?;
        let jwt = jwt(&c)?;
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
    let data: String = c.pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = STANDARD
        .decode(data)
        .context("reading the bot's private key")?;
    let key = RsaKeyPair::from_der(&der)
        .map_err(|_| anyhow!("the bot's private key is not a PKCS#1 RSA key"))?;
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &json!({"iat":now().saturating_sub(60), "exp":now()+540,"iss":c.id}),
    )?);
    let payload = format!("{header}.{claims}");
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        payload.as_bytes(),
        &mut signature,
    )
    .map_err(|_| anyhow!("could not sign the bot request"))?;
    Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
