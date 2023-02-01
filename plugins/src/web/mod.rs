// SPDX-License-Identifier: GPL-3.0-only
// Copyright © 2021 System76

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client;
use url::Url;

use pop_launcher::*;

pub use config::{load, Config, Definition};
use regex::Regex;

mod config;
pub async fn main() {
    let mut app = App::default();

    let mut requests = json_input_stream(async_stdin());

    while let Some(result) = requests.next().await {
        match result {
            Ok(request) => match request {
                Request::Activate(id) => app.activate(id).await,
                Request::Search(query) => app.search(query).await,
                Request::Exit => break,
                _ => (),
            },
            Err(why) => tracing::error!("malformed JSON input: {}", why),
        }
    }
}

pub struct App {
    config: Config,
    queries: Vec<String>,
    out: tokio::io::Stdout,
    client: Client,
    cache: PathBuf,
}

const ALLOWED_FAVICON_MIME: [&str; 5] = [
    "image/vnd.microsoft.icon",
    "image/png",
    "image/gif",
    "image/svg+xml",
    "image/x-icon",
];

impl Default for App {
    fn default() -> Self {
        let cache = dirs::home_dir()
            .map(|cache| cache.join(".cache/pop-launcher"))
            .expect("no home dir");

        if !cache.exists() {
            std::fs::create_dir(&cache).expect("unable to create $HOME/.cache/pop-launcher")
        }

        Self {
            config: config::load(),
            queries: Vec::new(),
            out: async_stdout(),
            client: Client::builder()
                .timeout(Duration::from_secs(1))
                .build()
                .expect("failed to create http client"),
            cache,
        }
    }
}

impl App {
    pub async fn activate(&mut self, id: u32) {
        if let Some(query) = self.queries.get(id as usize) {
            crate::xdg_open(query);
        }

        crate::send(&mut self.out, PluginResponse::Close).await;
    }

    pub async fn search(&mut self, query: String) {
        self.queries.clear();
        if let Some(word) = query.split_ascii_whitespace().next() {
            if let Some(defs) = self.config.get(word) {
                for (id, def) in defs.iter().enumerate() {
                    let (_, mut query) = query.split_at(word.len());
                    query = query.trim();
                    let encoded = build_query(def, query);
                    let icon = self.get_favicon(def).await;

                    crate::send(
                        &mut self.out,
                        PluginResponse::Append(PluginSearchResult {
                            id: id as u32,
                            name: [&def.name, ": ", query].concat(),
                            description: encoded.clone(),
                            icon,
                            ..Default::default()
                        }),
                    )
                    .await;

                    self.queries.push(encoded);
                }
            }
        }

        crate::send(&mut self.out, PluginResponse::Finished).await;
    }
}

impl App {
    async fn get_favicon(&self, def: &Definition) -> Option<IconSource> {
        let favicon_path = self.cache.join(format!("{}.ico", def.name));

        if favicon_path.exists() {
            let favicon_path = favicon_path.to_string_lossy().into_owned();
            Some(IconSource::Name(Cow::Owned(favicon_path)))
        } else {
            self.fetch_icon_in_background(def, &favicon_path).await;
            None
        }
    }

    async fn fetch_icon_in_background(&self, def: &Definition, favicon_path: &Path) {
        let client = self.client.clone();

        let url = build_query(def, "");
        let url = Url::parse(&url).expect("invalid url");
        let icon_source = def.icon.clone();

        let domain = url
            .domain()
            .map(|domain| domain.to_string())
            .expect("url have no domain");

        let favicon_path = favicon_path.to_path_buf();

        tokio::spawn(async move {
            let client = &client;
            let favicon_path = &favicon_path;

            // Attempts to fetch the favicon from the given URL.
            let fetch =
                |url: String| async move { fetch_favicon(&url, favicon_path, client).await };

            // Generate List of Icon sources in order of priority
            let mut icon_sources = vec![
                // First use the defined icon source, if it is defined
                Some(icon_source)
                    .filter(|s| !s.is_empty())
                    .map(|url| fetch(url)),
                // Searches for the favicon if it's not defined at the root of the domain.
                favicon_from_page(&domain, client)
                    .await
                    .map(|url| fetch(url)),
                // If not found, fetch from root domain.
                Some(fetch(["https://", &domain, "/favicon.ico"].concat())),
                // If all else fails, try Google.
                Some(fetch(format!(
                    "https://www.google.com/s2/favicons?domain={}&sz=32",
                    domain
                ))),
            ];

            // await every single source and take the first one, which does not return None
            let mut result = None;
            for f in icon_sources.drain(..).flatten() {
                if let res @ Some(_) = f.await {
                    result = res;
                    break;
                }
            }

            match result {
                Some(icon) => {
                    // Ensure we recreate the pop-launcher cache dir if it was removed at runtime
                    let cache_dir = favicon_path.parent().unwrap();
                    if !cache_dir.exists() {
                        std::fs::create_dir_all(cache_dir).expect("error creating cache directory");
                    }

                    let copy = tokio::fs::write(&favicon_path, icon).await;
                    if let Err(err) = copy {
                        tracing::error!("error writing favicon to {:?}: {}", &favicon_path, err);
                    }
                }
                None => tracing::error!("no icon found for {}", domain),
            }
        });
    }
}

fn build_query(definition: &Definition, query: &str) -> String {
    let q = definition.query.as_str();

    let scheme_regex = Regex::new(r"^([a-zA-Z]+[a-zA-Z0-9\+\-\.]*):").unwrap();

    let prefix = if scheme_regex.is_match(q) {
        ""
    } else {
        "https://"
    };

    [prefix, &*definition.query, &*urlencoding::encode(query)].concat()
}

async fn fetch_favicon(url: &str, favicon_path: &Path, client: &Client) -> Option<Bytes> {
    let response = client.get(url).send().await;
    match response {
        Err(err) => {
            tracing::error!("error fetching favicon {}: {}", url, err);
            None
        }
        Ok(response) => {
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|header| header.to_str().ok())?;

            if !ALLOWED_FAVICON_MIME.contains(&content_type) {
                tracing::error!(
                    "Got unexpected content-type '{}' type for {:?} favicon",
                    content_type,
                    favicon_path
                );
                return None;
            };

            match response.bytes().await {
                Ok(icon) => Some(icon),
                Err(why) => {
                    tracing::error!("error reading favicon response body: {}", why);
                    None
                }
            }
        }
    }
}

// Try to extract a favicon url from html the icon path
// returned can be either absolute or relative to the page domain
async fn favicon_from_page(domain: &str, client: &Client) -> Option<String> {
    let url = format!("https://{}", domain);
    match client.get(&url).send().await {
        Ok(html) => html
            .text()
            .await
            .ok()
            .and_then(|html| parse_favicon(&html))
            .map(|icon_url| {
                if !icon_url.starts_with("https://") {
                    format!("https://{}{}", domain, icon_url)
                } else {
                    icon_url
                }
            }),
        Err(_err) => None,
    }
}

fn parse_favicon(html: &str) -> Option<String> {
    let regex = Regex::new(r"<!--(.+)-->").unwrap();
    let html = regex.replace_all(html, "").to_string();

    let idx = html
        .find("rel=\"shortcut icon")
        .or_else(|| html.find("rel=\"alternate icon"))
        .or_else(|| html.find("rel=\"icon"));

    if let Some(idx) = idx {
        let html = &html[idx..];
        let idx = html.find("href=\"");

        if let Some(idx) = idx {
            let start = idx + 6;
            let html = &html[start..];
            let end = html.find('"');

            if let Some(end) = end {
                let icon_uri = &html[..end];
                let icon_uri = if icon_uri.starts_with("//") {
                    format!("https:{}", icon_uri)
                } else {
                    icon_uri.to_string()
                };

                return Some(icon_uri);
            }
        }
    }

    None
}

#[cfg(test)]
mod test {
    use crate::web::parse_favicon;

    async fn fetch(url: &str) -> String {
        reqwest::get(url).await.unwrap().text().await.unwrap()
    }

    #[tokio::test]
    async fn should_parse_favicon_url_github() {
        let html = fetch("https://github.com").await;

        let icon_url = parse_favicon(&html);
        assert_eq!(
            Some("https://github.githubassets.com/favicons/favicon.png".to_string()),
            icon_url
        );
    }

    #[tokio::test]
    async fn should_parse_favicon_url_ddg() {
        // Ddg returns a relative path to its favicon
        let html = fetch("https://duckduckgo.com").await;

        let icon_url = parse_favicon(&html);
        assert_eq!(Some("/favicon.ico".to_string()), icon_url);
    }

    #[tokio::test]
    async fn parse_favicon_url_google_returns_none() {
        // Google seems to set its favicon via javascript
        // hence there is no way to get the favicon from the page
        // source
        let html = fetch("https://google.com").await;

        let icon_url = parse_favicon(&html);
        assert!(icon_url.is_none());
    }

    #[tokio::test]
    async fn should_parse_favicon_url_flathub() {
        // Ensure we don't match the commented icon in flathub page
        // <!-- <link rel="icon" type="image/x-icon" href="favicon.ico"> -->
        // <link rel="icon" type="image/png" href="/assets/themes/flathub/favicon-32x32.png">
        let html = fetch("https://flathub.org").await;

        let icon_url = parse_favicon(&html);
        assert_eq!(
            Some("/assets/themes/flathub/favicon-32x32.png".to_string()),
            icon_url
        );
    }

    #[tokio::test]
    async fn should_parse_favicon_url_aliexpress() {
        // Aliexpress icon href start with two slash :`href="//ae01.alicdn.com/images/eng/wholesale/icon/aliexpress.ico"`

        let client = reqwest::Client::new();

        let html = client
            .get("https://aliexpress.com")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        let icon_url = parse_favicon(&html);
        assert_eq!(
            Some("https://ae01.alicdn.com/images/eng/wholesale/icon/aliexpress.ico".to_string()),
            icon_url
        );
    }
}
