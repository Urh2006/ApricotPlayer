//! Read-only capability probes. Evidence records status codes, timings and
//! counts only: no tokens, usernames, e-mails, names or titles.

use std::time::Instant;

use anyhow::Result;
use librespot_core::session::Session;
use protobuf::Message;
use reqwest::Method;
use serde_json::{Value, json};

use crate::pb;

const WEB: &str = "https://api.spotify.com";
const FALLBACK_TRACK: &str = "4u7EnebtmKWzUH433cf5Qv";
const FALLBACK_ARTIST: &str = "1dfeR4HaWDbWqFHLkxsg1d";
const FALLBACK_SPOTIFY_PLAYLIST: &str = "37i9dQZF1DXcBWIGoYBM5M";

#[derive(Clone, Copy, PartialEq)]
enum Auth {
    Login5,
    Keymaster,
}

struct Probe {
    http: reqwest::Client,
    login5: String,
    keymaster: Option<String>,
    client_token: String,
    spclient: String,
    user: String,
    evidence: Vec<Value>,
}

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Shape of a JSON response without personal strings: array lengths, totals,
/// and a few non-personal enum fields.
fn summarize(value: &Value, depth: usize) -> Value {
    match value {
        Value::Array(items) => json!({ "len": items.len() }),
        Value::Object(map) if depth < 3 => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                let keep_scalar = matches!(
                    key.as_str(),
                    "total"
                        | "limit"
                        | "product"
                        | "country"
                        | "type"
                        | "is_active"
                        | "is_restricted"
                        | "supports_volume"
                        | "is_playable"
                        | "explicit"
                        | "code"
                        | "status"
                );
                match child {
                    Value::Array(_) | Value::Object(_) => {
                        out.insert(key.clone(), summarize(child, depth + 1));
                    }
                    _ if keep_scalar => {
                        out.insert(key.clone(), child.clone());
                    }
                    _ => {}
                }
            }
            Value::Object(out)
        }
        Value::Object(map) => json!({ "keys": map.len() }),
        _ => Value::Null,
    }
}

impl Probe {
    fn redact(&self, target: &str) -> String {
        if self.user.is_empty() {
            target.to_string()
        } else {
            target.replace(&self.user, "{user}")
        }
    }

    async fn call(
        &mut self,
        id: &str,
        auth: Auth,
        method: Method,
        url: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Option<Reply> {
        let token = match auth {
            Auth::Login5 => Some(self.login5.clone()),
            Auth::Keymaster => self.keymaster.clone(),
        };
        let Some(token) = token else {
            self.evidence
                .push(json!({"id": id, "auth": "keymaster", "result": "no token"}));
            return None;
        };
        let mut request = self
            .http
            .request(method.clone(), url)
            .bearer_auth(token)
            .header("client-token", &self.client_token)
            .header("accept", "application/json");
        if let Some((content_type, bytes)) = body {
            request = request.header("content-type", content_type).body(bytes);
        }
        let started = Instant::now();
        let result = request.send().await;
        let ms = started.elapsed().as_millis();
        let target = self.redact(url);
        let auth_name = if auth == Auth::Login5 {
            "login5"
        } else {
            "keymaster"
        };
        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let body = response
                    .bytes()
                    .await
                    .map(|b| b.to_vec())
                    .unwrap_or_default();
                let reply = Reply { status, body };
                let summary = if reply.body.first() == Some(&b'{') {
                    summarize(&reply.json(), 0)
                } else {
                    Value::Null
                };
                self.evidence.push(json!({
                    "id": id, "auth": auth_name, "method": method.as_str(), "target": target,
                    "status": status, "ms": ms, "bytes": reply.body.len(),
                    "retry_after": retry_after, "summary": summary,
                }));
                println!(
                    "{id:<28} {auth_name:<9} {status} {ms:>5} ms {}",
                    reply.body.len()
                );
                Some(reply)
            }
            Err(error) => {
                self.evidence.push(json!({
                    "id": id, "auth": auth_name, "method": method.as_str(), "target": target,
                    "error": error.to_string(), "ms": ms,
                }));
                println!("{id:<28} {auth_name:<9} ERROR {error}");
                None
            }
        }
    }

    async fn web_both(&mut self, id: &str, path: &str) -> Option<Reply> {
        let url = format!("{WEB}{path}");
        let first = self.call(id, Auth::Login5, Method::GET, &url, None).await;
        let _ = self
            .call(id, Auth::Keymaster, Method::GET, &url, None)
            .await;
        first
    }

    fn note(&mut self, id: &str, value: Value) {
        println!("{id:<28} {value}");
        self.evidence.push(json!({ "id": id, "note": value }));
    }

    async fn collection_count(&mut self, set: &str) {
        let mut total = 0usize;
        let mut pages = 0usize;
        let mut token = String::new();
        let mut last_status = 0u16;
        loop {
            let mut body = Vec::new();
            pb::put_str(&mut body, 1, &self.user.clone());
            pb::put_str(&mut body, 2, set);
            if !token.is_empty() {
                pb::put_str(&mut body, 3, &token);
            }
            pb::put_int(&mut body, 4, 300);
            let url = format!("{}/collection/v2/paging", self.spclient);
            let reply = self
                .call(
                    &format!("collection.paging.{set}"),
                    Auth::Login5,
                    Method::POST,
                    &url,
                    Some(("application/vnd.collection-v2.spotify.proto", body)),
                )
                .await;
            let Some(reply) = reply else { break };
            last_status = reply.status;
            if !reply.ok() {
                break;
            }
            pages += 1;
            let mut next = String::new();
            for (field, value) in pb::fields(&reply.body).unwrap_or_default() {
                match (field, value) {
                    (1, pb::Value::Bytes(_)) => total += 1,
                    (2, pb::Value::Bytes(bytes)) => {
                        next = String::from_utf8_lossy(bytes).into_owned();
                    }
                    _ => {}
                }
            }
            if next.is_empty() || pages >= 20 {
                break;
            }
            token = next;
        }
        self.note(
            &format!("collection.count.{set}"),
            json!({ "status": last_status, "pages": pages, "items": total }),
        );
    }
}

fn first_id(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub async fn run(session: &Session, out: &str) -> Result<()> {
    let login5 = session.login5().auth_token().await?;
    let keymaster = session
        .token_provider()
        .get_token(&crate::OAUTH_SCOPES.join(","))
        .await;
    let client_token = session.spclient().client_token().await?;
    let spclient = session.spclient().base_url().await?;
    let mut probe = Probe {
        http: reqwest::Client::builder()
            .user_agent("Spotify/127700358 Win32_x86_64/Windows 10 (10.0.19045; x64)")
            .build()?,
        login5: login5.access_token.clone(),
        keymaster: keymaster.as_ref().ok().map(|t| t.access_token.clone()),
        client_token,
        spclient: spclient.clone(),
        user: session.username(),
        evidence: Vec::new(),
    };
    probe.note(
        "token.login5",
        json!({ "type": login5.token_type, "scopes": login5.scopes.len(), "expires_s": login5.expires_in.as_secs() }),
    );
    match &keymaster {
        Ok(token) => probe.note(
            "token.keymaster",
            json!({ "ok": true, "scopes": token.scopes, "expires_s": token.expires_in.as_secs() }),
        ),
        Err(error) => probe.note(
            "token.keymaster",
            json!({ "ok": false, "error": error.to_string() }),
        ),
    }
    probe.note(
        "account",
        json!({
            "type": session.get_user_attribute("type"),
            "country": session.country(),
            "filter_explicit": session.filter_explicit_content(),
        }),
    );

    // Identifiers for follow-up probes come from public catalogue search.
    let search = probe
        .web_both(
            "web.search.track",
            "/v1/search?q=bohemian%20rhapsody&type=track&limit=5",
        )
        .await;
    let search_json = search.as_ref().map(Reply::json).unwrap_or(Value::Null);
    let track = first_id(&search_json, "/tracks/items/0/id").unwrap_or(FALLBACK_TRACK.into());
    let artist =
        first_id(&search_json, "/tracks/items/0/artists/0/id").unwrap_or(FALLBACK_ARTIST.into());
    let album = first_id(&search_json, "/tracks/items/0/album/id");
    let pl_search = probe
        .web_both(
            "web.search.playlist",
            "/v1/search?q=today%27s%20top%20hits&type=playlist&limit=10",
        )
        .await;
    let spotify_playlist = pl_search
        .as_ref()
        .map(Reply::json)
        .and_then(|v| {
            v.pointer("/playlists/items")?
                .as_array()?
                .iter()
                .find_map(|item| {
                    (item.pointer("/owner/id")?.as_str()? == "spotify")
                        .then(|| item.get("id")?.as_str().map(str::to_string))?
                })
        })
        .unwrap_or(FALLBACK_SPOTIFY_PLAYLIST.into());

    for (id, path) in [
        ("web.me", "/v1/me".to_string()),
        (
            "web.search.all.limit10",
            "/v1/search?q=abba&type=track,album,artist,playlist,show,episode,audiobook&limit=10"
                .into(),
        ),
        (
            "web.search.track.limit50",
            "/v1/search?q=abba&type=track&limit=50".into(),
        ),
        ("web.me.tracks", "/v1/me/tracks?limit=1".into()),
        (
            "web.me.tracks.contains",
            format!("/v1/me/tracks/contains?ids={track}"),
        ),
        (
            "web.me.library.contains",
            format!("/v1/me/library/contains?uris=spotify:track:{track}"),
        ),
        ("web.me.playlists", "/v1/me/playlists?limit=50".into()),
        ("web.me.albums", "/v1/me/albums?limit=5".into()),
        ("web.me.shows", "/v1/me/shows?limit=5".into()),
        ("web.me.episodes", "/v1/me/episodes?limit=5".into()),
        ("web.me.audiobooks", "/v1/me/audiobooks?limit=5".into()),
        (
            "web.me.following",
            "/v1/me/following?type=artist&limit=5".into(),
        ),
        (
            "web.recently_played",
            "/v1/me/player/recently-played?limit=5".into(),
        ),
        (
            "web.top.tracks",
            "/v1/me/top/tracks?limit=5&time_range=short_term".into(),
        ),
        ("web.top.artists", "/v1/me/top/artists?limit=5".into()),
        ("web.player", "/v1/me/player".into()),
        ("web.player.devices", "/v1/me/player/devices".into()),
        ("web.player.queue", "/v1/me/player/queue".into()),
        (
            "web.browse.categories",
            "/v1/browse/categories?limit=5".into(),
        ),
        (
            "web.browse.made_for_you",
            "/v1/browse/categories/0JQ5DAt0tbjZptfcdMSKl3/playlists?limit=20".into(),
        ),
        (
            "web.browse.new_releases",
            "/v1/browse/new-releases?limit=5".into(),
        ),
        ("web.track", format!("/v1/tracks/{track}")),
        ("web.audio_features", format!("/v1/audio-features/{track}")),
        (
            "web.artist.top_tracks",
            format!("/v1/artists/{artist}/top-tracks?market=from_token"),
        ),
        (
            "web.artist.related",
            format!("/v1/artists/{artist}/related-artists"),
        ),
        (
            "web.artist.albums",
            format!("/v1/artists/{artist}/albums?limit=5"),
        ),
        (
            "web.recommendations",
            format!("/v1/recommendations?seed_tracks={track}&limit=5"),
        ),
        (
            "web.spotify_playlist",
            format!(
                "/v1/playlists/{spotify_playlist}?fields=id,owner(id),items(total),tracks(total)"
            ),
        ),
        (
            "web.spotify_playlist.items",
            format!("/v1/playlists/{spotify_playlist}/items?limit=5"),
        ),
        (
            "web.spotify_playlist.tracks",
            format!("/v1/playlists/{spotify_playlist}/tracks?limit=5"),
        ),
    ] {
        probe.web_both(id, &path).await;
    }

    // Internal client endpoints (spclient), the same ones LibreSpot uses.
    let rootlist_url = format!(
        "{spclient}/playlist/v2/user/{}/rootlist?decorate=revision,attributes,length,owner,capabilities,status_code&from=0&length=500",
        probe.user
    );
    let rootlist = probe
        .call(
            "sp.rootlist",
            Auth::Login5,
            Method::GET,
            &rootlist_url,
            None,
        )
        .await;
    let mut first_playlist = None;
    if let Some(reply) = rootlist.filter(Reply::ok) {
        match librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(
            &reply.body,
        ) {
            Ok(content) => {
                let items = &content.contents.items;
                let folders = items
                    .iter()
                    .filter(|i| i.uri().starts_with("spotify:start-group"))
                    .count();
                let playlists = items
                    .iter()
                    .filter(|i| i.uri().starts_with("spotify:playlist:"))
                    .count();
                first_playlist = items
                    .iter()
                    .find(|i| i.uri().starts_with("spotify:playlist:"))
                    .map(|i| i.uri().trim_start_matches("spotify:playlist:").to_string());
                probe.note(
                    "sp.rootlist.parsed",
                    json!({ "items": items.len(), "playlists": playlists, "folder_starts": folders,
                            "length": content.length(), "has_revision": content.has_revision() }),
                );
            }
            Err(error) => probe.note("sp.rootlist.parsed", json!({ "error": error.to_string() })),
        }
    }
    for (id, playlist) in [
        ("sp.playlist.first_rootlist", first_playlist.clone()),
        ("sp.playlist.spotify_owned", Some(spotify_playlist.clone())),
    ] {
        let Some(playlist) = playlist else { continue };
        let url = format!("{spclient}/playlist/v2/playlist/{playlist}");
        if let Some(reply) = probe
            .call(id, Auth::Login5, Method::GET, &url, None)
            .await
            .filter(Reply::ok)
        {
            match librespot_protocol::playlist4_external::SelectedListContent::parse_from_bytes(&reply.body) {
                Ok(content) => probe.note(
                    &format!("{id}.parsed"),
                    json!({
                        "length": content.length(),
                        "items_in_first_page": content.contents.items.len(),
                        "truncated": content.contents.truncated(),
                        "has_revision": content.has_revision(),
                        "item_uids": content.contents.items.iter().filter(|i| i.attributes.has_item_id()).count(),
                        "owned_by_me": content.owner_username() == probe.user,
                        "capabilities": format!("{:?}", content.capabilities),
                    }),
                ),
                Err(error) => probe.note(&format!("{id}.parsed"), json!({ "error": error.to_string() })),
            }
        }
    }

    let liked = format!("spotify:user:{}:collection", probe.user);
    let started = Instant::now();
    match session.spclient().get_context(&liked).await {
        Ok(context) => probe.note(
            "sp.context.liked_songs",
            json!({ "ms": started.elapsed().as_millis(), "pages": context.pages.len(),
                    "first_page_tracks": context.pages.first().map(|p| p.tracks.len()),
                    "restrictions": context.restrictions.is_some() }),
        ),
        Err(error) => probe.note(
            "sp.context.liked_songs",
            json!({ "error": error.to_string() }),
        ),
    }
    let spotify_ctx = format!("spotify:playlist:{spotify_playlist}");
    match session.spclient().get_context(&spotify_ctx).await {
        Ok(context) => probe.note(
            "sp.context.spotify_owned",
            json!({ "pages": context.pages.len(), "first_page_tracks": context.pages.first().map(|p| p.tracks.len()),
                    "uids": context.pages.first().map(|p| p.tracks.iter().filter(|t| t.uid.is_some()).count()) }),
        ),
        Err(error) => probe.note("sp.context.spotify_owned", json!({ "error": error.to_string() })),
    }

    for set in [
        "collection",
        "ban",
        "artistban",
        "listenlater",
        "show",
        "artist",
        "album",
    ] {
        probe.collection_count(set).await;
    }

    let track_uri = format!("spotify:track:{track}");
    probe
        .call(
            "sp.radio.inspiredby",
            Auth::Login5,
            Method::GET,
            &format!(
                "{spclient}/inspiredby-mix/v2/seed_to_playlist/{track_uri}?response-format=json"
            ),
            None,
        )
        .await;
    probe
        .call(
            "sp.radio.apollo",
            Auth::Login5,
            Method::GET,
            &format!("{spclient}/radio-apollo/v3/stations/{track_uri}?autoplay=false&count=20"),
            None,
        )
        .await;
    probe
        .call(
            "sp.lyrics",
            Auth::Login5,
            Method::GET,
            &format!("{spclient}/color-lyrics/v2/track/{track}?format=json&vocalRemoval=false&market=from_token"),
            None,
        )
        .await;
    if let Some(album) = &album {
        let mut request =
            librespot_protocol::autoplay_context_request::AutoplayContextRequest::new();
        request.set_context_uri(format!("spotify:album:{album}"));
        match session.spclient().get_autoplay_context(&request).await {
            Ok(context) => probe.note(
                "sp.autoplay.album",
                json!({ "pages": context.pages.len(), "first_page_tracks": context.pages.first().map(|p| p.tracks.len()) }),
            ),
            Err(error) => probe.note("sp.autoplay.album", json!({ "error": error.to_string() })),
        }
    }
    probe
        .call(
            "sp.connect.devices",
            Auth::Login5,
            Method::GET,
            &format!(
                "{spclient}/connect-state/v1/devices/hobs_{}",
                session.device_id()
            ),
            None,
        )
        .await;

    // Personalised home (Daily Mixes etc.): the official clients use the
    // pathfinder GraphQL "home" persisted query. The query hash is public
    // web-player code, not a credential.
    let home_hash = scrape_home_hash(&probe.http).await;
    probe.note("pathfinder.home.hash_found", json!(home_hash.is_some()));
    if let Some(hash) = home_hash {
        for (id, auth) in [
            ("pathfinder.home", Auth::Login5),
            ("pathfinder.home", Auth::Keymaster),
        ] {
            let body = json!({
                "variables": { "timeZone": "Europe/Ljubljana", "sp_t": "", "facet": "", "sectionItemsLimit": 20 },
                "operationName": "home",
                "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } }
            });
            if let Some(reply) = probe
                .call(
                    id,
                    auth,
                    Method::POST,
                    "https://api-partner.spotify.com/pathfinder/v2/query",
                    Some((
                        "application/json;charset=UTF-8",
                        body.to_string().into_bytes(),
                    )),
                )
                .await
                .filter(Reply::ok)
            {
                let text = String::from_utf8_lossy(&reply.body);
                let playlists = count_matches(&text, "\"uri\":\"spotify:playlist:");
                let personal_mix = count_matches(&text, "spotify:playlist:37i9dQZF1E");
                let sections = count_matches(&text, "\"uri\":\"spotify:section:");
                let errors = reply.json().get("errors").is_some();
                let label = if auth == Auth::Login5 {
                    "login5"
                } else {
                    "keymaster"
                };
                probe.note(
                    &format!("pathfinder.home.parsed.{label}"),
                    json!({ "sections": sections, "playlist_uris": playlists,
                            "personal_mix_prefix_37i9dQZF1E": personal_mix, "graphql_errors": errors }),
                );
            }
        }
    }

    std::fs::write(out, serde_json::to_string_pretty(&probe.evidence)?)?;
    println!("evidence written to {out}");
    Ok(())
}

fn count_matches(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

async fn scrape_home_hash(http: &reqwest::Client) -> Option<String> {
    let html = http
        .get("https://open.spotify.com/")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let scripts: Vec<String> = html
        .split("src=\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .filter(|src| src.ends_with(".js") && src.contains("web-player"))
        .map(str::to_string)
        .collect();
    for script in scripts {
        let js = http.get(&script).send().await.ok()?.text().await.ok()?;
        if let Some(hash) = find_hash(&js, "\"home\",\"query\",\"") {
            return Some(hash);
        }
        // Chunked builds reference the query from a lazily loaded chunk; the
        // persisted-query map may live in any chunk listed in the main bundle.
        for chunk in js
            .split("\"")
            .filter(|s| s.ends_with(".js") && s.len() < 120)
        {
            let url = if chunk.starts_with("http") {
                chunk.to_string()
            } else {
                format!("https://open.spotifycdn.com/cdn/build/web-player/{chunk}")
            };
            if let Ok(response) = http.get(&url).send().await
                && let Ok(text) = response.text().await
                && let Some(hash) = find_hash(&text, "\"home\",\"query\",\"")
            {
                return Some(hash);
            }
        }
    }
    None
}

fn find_hash(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let hash: String = text[start..].chars().take(64).collect();
    (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some(hash)
}

/// Single raw request for investigation. The body goes to a scratch file
/// under the P0 data dir (never into the repository); stdout gets only the
/// status and the non-personal summary.
pub async fn raw(session: &Session, kind: &str, a: &str, b: &str, c: &str) -> Result<()> {
    let login5 = session.login5().auth_token().await?;
    let client_token = session.spclient().client_token().await?;
    let http = reqwest::Client::new();
    let (method, url, body, accept): (Method, String, Option<String>, &str) = match kind {
        "pf" => (
            Method::POST,
            "https://api-partner.spotify.com/pathfinder/v2/query".into(),
            Some(
                json!({
                    "variables": serde_json::from_str::<Value>(c)?,
                    "operationName": a,
                    "extensions": { "persistedQuery": { "version": 1, "sha256Hash": b } }
                })
                .to_string(),
            ),
            "application/json",
        ),
        "web" => (Method::GET, format!("{WEB}{a}"), None, "application/json"),
        "sp" => {
            let base = session.spclient().base_url().await?;
            let path = a.replace("{user}", &session.username());
            let method = if b.is_empty() {
                Method::GET
            } else {
                Method::from_bytes(b.as_bytes())?
            };
            (
                method,
                format!("{base}{path}"),
                (!c.is_empty()).then(|| c.replace("{user}", &session.username())),
                "application/json",
            )
        }
        _ => anyhow::bail!("raw pf|web|sp"),
    };
    let mut request = http
        .request(method, &url)
        .bearer_auth(&login5.access_token)
        .header("client-token", client_token)
        .header("accept", accept)
        .header("app-platform", "Win32_x86_64");
    if let Some(body) = body {
        request = request
            .header(
                "content-type",
                if kind == "pf" {
                    "application/json;charset=UTF-8"
                } else {
                    "application/json"
                },
            )
            .body(body);
    }
    let started = Instant::now();
    let response = request.send().await?;
    let status = response.status();
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response.bytes().await?;
    let dir = crate::data_dir().join("raw");
    std::fs::create_dir_all(&dir)?;
    let name: String = format!("{kind}-{a}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(60)
        .collect();
    std::fs::write(dir.join(format!("{name}.json")), &bytes)?;
    let summary = serde_json::from_slice::<Value>(&bytes)
        .map(|v| summarize(&v, 0))
        .unwrap_or(Value::Null);
    println!(
        "{status} {} ms {} bytes retry_after={retry:?} {summary}",
        started.elapsed().as_millis(),
        bytes.len()
    );
    Ok(())
}

/// Collection v2 protobuf write: `set` with one item and optional context
/// (contextual ban = Spotify "Hide song" in a personalised playlist).
pub async fn collection_write(
    session: &Session,
    set: &str,
    uri: &str,
    context: &str,
    removed: bool,
) -> Result<()> {
    let login5 = session.login5().auth_token().await?;
    let client_token = session.spclient().client_token().await?;
    let base = session.spclient().base_url().await?;
    let mut item = Vec::new();
    pb::put_str(&mut item, 1, uri);
    if removed {
        pb::put_int(&mut item, 3, 1);
    }
    if !context.is_empty() {
        pb::put_str(&mut item, 4, context);
    }
    let mut body = Vec::new();
    pb::put_str(&mut body, 1, &session.username());
    pb::put_str(&mut body, 2, set);
    pb::put_bytes(&mut body, 3, &item);
    let response = reqwest::Client::new()
        .post(format!("{base}/collection/v2/write"))
        .bearer_auth(&login5.access_token)
        .header("client-token", client_token)
        .header(
            "content-type",
            "application/vnd.collection-v2.spotify.proto",
        )
        .body(body)
        .send()
        .await?;
    println!(
        "collection write set={set} removed={removed}: {}",
        response.status()
    );
    Ok(())
}

/// Items of a collection set as (uri, context_uri, is_removed), JSON paging.
pub async fn collection_items(session: &Session, set: &str) -> Result<()> {
    let login5 = session.login5().auth_token().await?;
    let client_token = session.spclient().client_token().await?;
    let base = session.spclient().base_url().await?;
    let mut body = Vec::new();
    pb::put_str(&mut body, 1, &session.username());
    pb::put_str(&mut body, 2, set);
    pb::put_int(&mut body, 4, 300);
    let response = reqwest::Client::new()
        .post(format!("{base}/collection/v2/paging"))
        .bearer_auth(&login5.access_token)
        .header("client-token", client_token)
        .header(
            "content-type",
            "application/vnd.collection-v2.spotify.proto",
        )
        .header("accept", "application/json")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let json: Value = response.json().await.unwrap_or(Value::Null);
    let items: Vec<String> = json
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|i| {
                    format!(
                        "{}|ctx={}|removed={}",
                        i.get("uri").and_then(Value::as_str).unwrap_or(""),
                        i.get("contextUri").and_then(Value::as_str).unwrap_or("-"),
                        i.get("isRemoved").and_then(Value::as_bool).unwrap_or(false)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    println!(
        "collection {set}: {status} {} items {:?}",
        items.len(),
        items
    );
    if let Some(first) = json
        .get("items")
        .and_then(Value::as_array)
        .and_then(|i| i.first())
    {
        println!("first item raw: {first}");
    }
    Ok(())
}
