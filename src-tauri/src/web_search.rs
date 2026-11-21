use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::State;

const MAX_SEARCH_RESPONSE_BYTES: usize = 512 * 1024;

fn shared_search_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

async fn read_response_bounded(res: reqwest::Response, max_bytes: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Stream read error: {e}"))?;
        if buf.len() + chunk.len() > max_bytes {
            let remaining = max_bytes.saturating_sub(buf.len());
            buf.extend_from_slice(&chunk[..remaining]);
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub title: String,
    pub snippet: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct DdgInstantAnswer {
    #[serde(rename = "AbstractText")]
    abstract_text: Option<String>,
    #[serde(rename = "AbstractURL")]
    abstract_url: Option<String>,
    #[serde(rename = "Heading")]
    heading: Option<String>,
    #[serde(rename = "RelatedTopics")]
    related_topics: Option<Vec<DdgRelatedTopic>>,
}

#[derive(Debug, Deserialize)]
struct DdgRelatedTopic {
    #[serde(rename = "Text")]
    text: Option<String>,
    #[serde(rename = "FirstURL")]
    first_url: Option<String>,
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("<b>", "")
        .replace("</b>", "")
}

fn strip_html_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;

    for ch in input.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            output.push(ch);
        }
    }

    decode_html_entities(output.trim())
}

/// Parses results from DuckDuckGo HTML search output
pub fn parse_ddg_html(html: &str) -> Vec<WebSearchResult> {
    let mut results = Vec::new();
    let lower = html;

    let parts: Vec<&str> = lower.split("class=\"result results_links").collect();
    for part in parts.iter().skip(1).take(5) {
        // Extract Title and URL
        let mut title = String::new();
        let mut url = String::new();
        let mut snippet = String::new();

        if let Some(snippet_idx) = part.find("class=\"result__snippet") {
            let rest = &part[snippet_idx..];
            if let Some(tag_close) = rest.find('>') {
                let text_start = &rest[tag_close + 1..];
                if let Some(end_tag) = text_start.find("</a>") {
                    snippet = strip_html_tags(&text_start[..end_tag]);
                }
            }
        }

        if let Some(title_idx) = part.find("class=\"result__a\"") {
            let rest = &part[title_idx..];
            if let Some(href_idx) = rest.find("href=\"") {
                let href_rest = &rest[href_idx + 6..];
                if let Some(href_end) = href_rest.find('"') {
                    let raw_url = &href_rest[..href_end];
                    if let Some(uddg) = raw_url.find("uddg=") {
                        let enc = &raw_url[uddg + 5..];
                        let clean = enc.split('&').next().unwrap_or(enc);
                        url = urlencoding_decode(clean);
                    } else if !raw_url.starts_with('/') {
                        url = raw_url.to_string();
                    }
                }
            }

            if let Some(tag_close) = rest.find('>') {
                let text_start = &rest[tag_close + 1..];
                if let Some(end_tag) = text_start.find("</a>") {
                    title = strip_html_tags(&text_start[..end_tag]);
                }
            }
        }

        if !title.is_empty() && (!snippet.is_empty() || !url.is_empty()) {
            results.push(WebSearchResult {
                title,
                snippet,
                url,
            });
        }
    }

    results
}

fn urlencoding_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            let hex = format!("{h1}{h2}");
            if let Ok(b) = u8::from_str_radix(&hex, 16) {
                bytes.push(b);
            }
        } else if ch == '+' {
            bytes.push(b' ');
        } else {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            bytes.extend_from_slice(encoded.as_bytes());
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

/// Executes a live web search for current events and real-time knowledge
#[tauri::command]
pub async fn web_search(
    db_state: State<'_, crate::db::DbState>,
    query: String,
    workspace_id: Option<String>,
) -> Result<Vec<WebSearchResult>, String> {
    if let Some(ref ws_id) = workspace_id {
        let conn = db_state
            .conn
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let key = format!("ws_offline_{ws_id}");
        if crate::db::get_setting_internal(&conn, &key).as_deref() == Some("true") {
            return Err("Strict Offline Mode is active for this workspace. External web search is blocked.".to_string());
        }
    }

    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let client = shared_search_client();

    // 1. Try DuckDuckGo HTML endpoint
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding_encode(trimmed)
    );

    let html_res = client
        .get(&url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await;

    if let Ok(res) = html_res {
        if res.status().is_success() {
            if let Ok(html) = read_response_bounded(res, MAX_SEARCH_RESPONSE_BYTES).await {
                let parsed = parse_ddg_html(&html);
                if !parsed.is_empty() {
                    return Ok(parsed);
                }
            }
        }
    }

    // 2. Fallback to DuckDuckGo Instant Answer JSON API
    let instant_url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding_encode(trimmed)
    );

    let json_res = client
        .get(&instant_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        )
        .send()
        .await;

    if let Ok(res) = json_res {
        if res.status().is_success() {
            if let Ok(raw_json) = read_response_bounded(res, MAX_SEARCH_RESPONSE_BYTES).await {
                if let Ok(data) = serde_json::from_str::<DdgInstantAnswer>(&raw_json) {
                let mut fallback_results = Vec::new();

                if let Some(abs_text) = data.abstract_text {
                    if !abs_text.is_empty() {
                        fallback_results.push(WebSearchResult {
                            title: data.heading.unwrap_or_else(|| trimmed.to_string()),
                            snippet: abs_text,
                            url: data.abstract_url.unwrap_or_default(),
                        });
                    }
                }

                if let Some(topics) = data.related_topics {
                    for topic in topics.into_iter().take(3) {
                        if let Some(txt) = topic.text {
                            if !txt.is_empty() {
                                fallback_results.push(WebSearchResult {
                                    title: txt.chars().take(60).collect(),
                                    snippet: txt,
                                    url: topic.first_url.unwrap_or_default(),
                                });
                            }
                        }
                    }
                }

                if !fallback_results.is_empty() {
                    return Ok(fallback_results);
                }
                }
            }
        }
    }

    Ok(Vec::new())
}

fn urlencoding_encode(s: &str) -> String {
    let mut encoded = String::new();
    for byte in s.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags_and_decode() {
        let sample = "<b>Red Bull</b> wins in &#x27;Monaco&#x27; &amp; sets pole!";
        assert_eq!(
            strip_html_tags(sample),
            "Red Bull wins in 'Monaco' & sets pole!"
        );
    }

    #[test]
    fn test_parse_ddg_html_extracts_structured_results() {
        let mock_html = r#"
        <div class="result results_links results_links_deep web-result ">
            <h2 class="result__title">
                <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.formula1.com%2Fqualifying">F1 Qualifying Results Today</a>
            </h2>
            <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.formula1.com%2Fqualifying">Verstappen captures pole position in today qualifying session.</a>
        </div>
        "#;

        let results = parse_ddg_html(mock_html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "F1 Qualifying Results Today");
        assert_eq!(
            results[0].snippet,
            "Verstappen captures pole position in today qualifying session."
        );
        assert_eq!(results[0].url, "https://www.formula1.com/qualifying");
    }
}
