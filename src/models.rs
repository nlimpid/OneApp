use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Story {
    pub id: i64,
    pub title: String,
    pub url: Option<String>,
    pub score: i32,
    pub by: String,
    pub time: i64,
    pub descendants: Option<i32>,
    pub kids: Option<Vec<i64>>,
    pub text: Option<String>,
    #[serde(rename = "type")]
    pub story_type: String,
}

impl Story {
    pub fn formatted_time(&self) -> String {
        let now = chrono::Utc::now().timestamp();
        let diff = now - self.time;

        if diff < 60 {
            format!("{}s ago", diff)
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }

    pub fn domain(&self) -> Option<String> {
        self.url.as_ref().and_then(|url| {
            url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.replace("www.", "")))
        })
    }

    pub fn comment_count(&self) -> i32 {
        self.descendants.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Comment {
    pub id: i64,
    pub by: Option<String>,
    pub text: Option<String>,
    pub time: i64,
    pub kids: Option<Vec<i64>>,
    pub parent: i64,
    #[serde(rename = "type")]
    pub comment_type: String,
}

impl Comment {
    pub fn formatted_time(&self) -> String {
        let now = chrono::Utc::now().timestamp();
        let diff = now - self.time;

        if diff < 60 {
            format!("{}s ago", diff)
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }

    pub fn author(&self) -> &str {
        self.by.as_deref().unwrap_or("[deleted]")
    }

    pub fn clean_text(&self) -> String {
        self.text.as_ref().map_or_else(
            || "[deleted]".to_string(),
            |text| {
                let cleaned = html_escape::decode_html_entities(text);
                let cleaned = cleaned
                    .replace("<p>", "\n\n")
                    .replace("</p>", "")
                    .replace("<br>", "\n")
                    .replace("<br/>", "\n")
                    .replace("<br />", "\n");

                let re = regex::Regex::new(r"<[^>]+>").unwrap();
                re.replace_all(&cleaned, "").trim().to_string()
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsChannel {
    HackerNews,
}

impl NewsChannel {
    pub fn name(&self) -> &'static str {
        match self {
            NewsChannel::HackerNews => "Hacker News",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            NewsChannel::HackerNews => "Y",
        }
    }
}
