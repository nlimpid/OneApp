use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// 缓存的 HTML 标签正则表达式
static HTML_TAG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]+>").expect("Invalid regex pattern"));

/// 格式化相对时间
pub fn format_relative_time(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - timestamp;

    if diff < 0 {
        "just now".to_string()
    } else if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

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
    #[must_use]
    pub fn formatted_time(&self) -> String {
        format_relative_time(self.time)
    }

    #[must_use]
    pub fn domain(&self) -> Option<String> {
        self.url.as_ref().and_then(|url| {
            url::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.replace("www.", "")))
        })
    }

    #[must_use]
    pub fn comment_count(&self) -> i32 {
        self.descendants.unwrap_or(0)
    }
}

/// 原始评论数据（从 API 获取）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawComment {
    pub id: i64,
    pub by: Option<String>,
    pub text: Option<String>,
    pub time: i64,
    pub kids: Option<Vec<i64>>,
    pub parent: i64,
    #[serde(rename = "type")]
    pub comment_type: String,
}

/// 带层级的评论（用于显示）
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub id: i64,
    pub by: Option<String>,
    pub text: Option<String>,
    pub time: i64,
    pub kids: Option<Vec<i64>>,
    pub parent: i64,
    pub depth: usize,
    /// 子评论数量（包括嵌套的）
    pub reply_count: usize,
}

impl From<RawComment> for Comment {
    fn from(raw: RawComment) -> Self {
        Self {
            id: raw.id,
            by: raw.by,
            text: raw.text,
            time: raw.time,
            kids: raw.kids,
            parent: raw.parent,
            depth: 0,
            reply_count: 0,
        }
    }
}

impl Comment {
    #[must_use]
    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    #[must_use]
    pub fn formatted_time(&self) -> String {
        format_relative_time(self.time)
    }

    #[must_use]
    pub fn author(&self) -> &str {
        self.by.as_deref().unwrap_or("[deleted]")
    }

    #[must_use]
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

                HTML_TAG_RE.replace_all(&cleaned, "").trim().to_string()
            },
        )
    }

    #[must_use]
    pub fn has_replies(&self) -> bool {
        self.kids.as_ref().is_some_and(|k| !k.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsChannel {
    HackerNews,
}

impl NewsChannel {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            NewsChannel::HackerNews => "Hacker News",
        }
    }

    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            NewsChannel::HackerNews => "Y",
        }
    }
}
