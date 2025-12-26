use futures::AsyncReadExt as _;
use gpui::http_client::{http, AsyncBody, HttpClient, HttpRequestExt, Method, RedirectPolicy};
use readabilityrs::{Readability, ReadabilityOptions};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
const MAX_BLOCKS: usize = 300;
const DISK_CACHE_TTL_SECS: i64 = 24 * 60 * 60;
const POSITIVE_KEYWORDS: &[&str] = &[
    "article", "body", "content", "entry", "main", "page", "post", "read", "story", "text",
];
const NEGATIVE_KEYWORDS: &[&str] = &[
    "ad",
    "ads",
    "advert",
    "banner",
    "cookie",
    "comment",
    "footer",
    "header",
    "masthead",
    "menu",
    "modal",
    "nav",
    "newsletter",
    "pagination",
    "popup",
    "promo",
    "recommend",
    "related",
    "share",
    "sidebar",
    "social",
    "sponsor",
    "subscribe",
    "toolbar",
    "widget",
];

#[derive(Debug, Clone)]
pub struct ReaderSession {
    pub url: String,
    pub title_hint: Option<String>,
    pub state: ReaderLoadState,
}

#[derive(Debug, Clone)]
pub enum ReaderLoadState {
    Loading,
    Ready(ReaderArticle),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderArticle {
    pub title: String,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub reading_time: Option<String>,
    pub blocks: Vec<ReaderBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReaderBlock {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph(String),
    Quote(String),
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Code {
        text: String,
        language: Option<String>,
    },
    Image {
        url: String,
        alt: Option<String>,
        caption: Option<String>,
    },
    Rule,
}

pub async fn load_article(
    http_client: Arc<dyn HttpClient>,
    url: &str,
    title_hint: Option<&str>,
) -> Result<ReaderArticle, String> {
    let parsed_url = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err("Only http(s) URLs are supported.".to_string());
    }

    if let Some(mut cached) = read_disk_cache(url) {
        if cached.title.is_empty() {
            if let Some(title_hint) = title_hint {
                cached.title = title_hint.to_string();
            }
        }
        return Ok(cached);
    }

    let request = http::Request::builder()
        .method(Method::GET)
        .uri(url)
        .follow_redirects(RedirectPolicy::FollowAll)
        .header("User-Agent", "OneApp/0.1 (GPUI Reader Mode)")
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .body(AsyncBody::empty())
        .map_err(|e| e.to_string())?;

    let response = http_client.send(request).await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {} for {}", response.status(), url));
    }

    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut body = response.into_body();
    let bytes = read_to_end_limited(&mut body, MAX_HTML_BYTES).await?;
    let content = String::from_utf8_lossy(&bytes).to_string();

    if content_type.contains("text/plain") {
        let article = plain_text_article(&content, &parsed_url, title_hint.map(str::to_string));
        let _ = write_disk_cache(url, &article);
        return Ok(article);
    }

    if !content_type.is_empty()
        && !(content_type.contains("text/html") || content_type.contains("application/xhtml+xml"))
    {
        return Err(format!("Unsupported content type: {content_type}"));
    }

    let article = extract_html_article(&content, &parsed_url, title_hint.map(str::to_string));
    let _ = write_disk_cache(url, &article);
    Ok(article)
}

async fn read_to_end_limited(body: &mut AsyncBody, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut total = 0usize;
    let mut buf = [0u8; 8192];
    loop {
        let n = body.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > limit {
            return Err(format!(
                "Response too large (>{} MB)",
                (limit as f32 / (1024.0 * 1024.0)).ceil() as usize
            ));
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCacheEntry {
    fetched_at: i64,
    article: ReaderArticle,
}

fn read_disk_cache(url: &str) -> Option<ReaderArticle> {
    let path = disk_cache_path(url)?;
    let bytes = std::fs::read(path).ok()?;
    let entry: DiskCacheEntry = serde_json::from_slice(&bytes).ok()?;
    if is_cache_stale(entry.fetched_at) {
        return None;
    }
    Some(entry.article)
}

fn write_disk_cache(url: &str, article: &ReaderArticle) -> Result<(), String> {
    let path = disk_cache_path(url).ok_or_else(|| "No cache directory available".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let entry = DiskCacheEntry {
        fetched_at: now_unix_secs().ok_or_else(|| "Clock unavailable".to_string())?,
        article: article.clone(),
    };
    let json = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;

    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp_path, &path).map_err(|_| error.to_string())?;
    }
    Ok(())
}

fn is_cache_stale(fetched_at: i64) -> bool {
    let Some(now) = now_unix_secs() else {
        return true;
    };
    now.saturating_sub(fetched_at) > DISK_CACHE_TTL_SECS
}

fn now_unix_secs() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn disk_cache_path(url: &str) -> Option<PathBuf> {
    let dir = reader_cache_dir()?;
    let key = url_cache_key(url);
    Some(dir.join("reader").join(format!("{key}.json")))
}

fn url_cache_key(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn reader_cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ONEAPP_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }

    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(dir).join("oneapp"));
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return Some(PathBuf::from(home).join("Library/Caches/OneApp"));
        }
    }

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return Some(PathBuf::from(home).join(".cache/oneapp"));
    }

    Some(std::env::temp_dir().join("oneapp-cache"))
}

fn extract_html_article(html: &str, url: &url::Url, title_hint: Option<String>) -> ReaderArticle {
    if let Some(article) = extract_with_readabilityrs(html, url, title_hint.clone()) {
        return article;
    }

    let doc = Html::parse_document(html);

    let title = extract_title(&doc).or(title_hint).unwrap_or_default();

    let site_name =
        extract_meta(&doc, "meta[property=\"og:site_name\"]").or_else(|| host_without_www(url));

    let byline = extract_meta(&doc, "meta[name=\"author\"]")
        .or_else(|| extract_meta(&doc, "meta[property=\"article:author\"]"));

    let root = select_best_root(&doc).unwrap_or_else(|| doc.root_element());
    let blocks = extract_blocks(&root, url);

    ReaderArticle {
        title,
        byline,
        site_name,
        reading_time: estimate_reading_time(&blocks),
        blocks,
    }
}

fn extract_with_readabilityrs(
    html: &str,
    url: &url::Url,
    title_hint: Option<String>,
) -> Option<ReaderArticle> {
    let options = ReadabilityOptions::default();
    let readability = Readability::new(html, Some(url.as_str()), Some(options)).ok()?;
    let parsed = readability.parse()?;

    let content_html = parsed.content.clone().or(parsed.raw_content.clone())?;
    if content_html.trim().is_empty() {
        return None;
    }

    let content_doc = Html::parse_fragment(&content_html);
    let root = content_doc.root_element();
    let blocks = extract_blocks(&root, url);
    if blocks.is_empty() || total_text_len(&blocks) < 200 {
        return None;
    }

    let title = parsed
        .title
        .and_then(|s| {
            let s = normalize_whitespace(&s);
            (!s.is_empty()).then_some(s)
        })
        .or(title_hint)
        .unwrap_or_default();

    let byline = parsed.byline.and_then(|s| {
        let s = normalize_whitespace(&s);
        (!s.is_empty()).then_some(s)
    });

    let site_name = parsed.site_name.and_then(|s| {
        let s = normalize_whitespace(&s);
        (!s.is_empty()).then_some(s)
    });

    Some(ReaderArticle {
        title,
        byline,
        site_name: site_name.or_else(|| host_without_www(url)),
        reading_time: estimate_reading_time(&blocks),
        blocks,
    })
}

fn plain_text_article(text: &str, url: &url::Url, title_hint: Option<String>) -> ReaderArticle {
    let title = title_hint.unwrap_or_else(|| url.to_string());
    let site_name = host_without_www(url);

    let paragraphs = split_paragraphs(text);
    let blocks = paragraphs
        .into_iter()
        .map(ReaderBlock::Paragraph)
        .collect::<Vec<_>>();
    ReaderArticle {
        title,
        byline: None,
        site_name,
        reading_time: estimate_reading_time(&blocks),
        blocks,
    }
}

fn extract_title(doc: &Html) -> Option<String> {
    extract_meta(doc, "meta[property=\"og:title\"]")
        .or_else(|| extract_meta(doc, "meta[name=\"twitter:title\"]"))
        .or_else(|| {
            let selector = Selector::parse("title").ok()?;
            let title = doc.select(&selector).next()?;
            let raw = title.text().collect::<Vec<_>>().join(" ");
            let title = normalize_whitespace(&raw);
            (!title.is_empty()).then_some(title)
        })
}

fn extract_meta(doc: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    let el = doc.select(&selector).next()?;
    let content = el.value().attr("content")?;
    let content = normalize_whitespace(content);
    (!content.is_empty()).then_some(content)
}

fn host_without_www(url: &url::Url) -> Option<String> {
    url.host_str()
        .map(|h| h.trim_start_matches("www.").to_string())
        .filter(|h| !h.is_empty())
}

fn select_best_root<'a>(doc: &'a Html) -> Option<ElementRef<'a>> {
    let selector = Selector::parse("article, main, section, div").ok()?;
    let mut best: Option<(f32, ElementRef<'a>)> = None;

    for el in doc.select(&selector) {
        if is_unlikely_candidate(&el) {
            continue;
        }

        let score = score_candidate(&el);
        if score <= 0.0 {
            continue;
        }

        match &best {
            Some((best_score, _)) if score <= *best_score => {}
            _ => best = Some((score, el)),
        }
    }

    best.map(|(_, el)| el)
}

fn score_candidate(candidate: &ElementRef<'_>) -> f32 {
    let p_selector = match Selector::parse("p") {
        Ok(s) => s,
        Err(_) => return 0.0,
    };
    let a_selector = match Selector::parse("a") {
        Ok(s) => s,
        Err(_) => return 0.0,
    };

    let mut paragraph_count = 0usize;
    let mut paragraph_text_len = 0usize;
    for p in candidate.select(&p_selector) {
        let len = element_text_len(&p);
        if len < 20 {
            continue;
        }
        paragraph_count += 1;
        paragraph_text_len = paragraph_text_len.saturating_add(len);
    }

    let text_len = element_text_len(candidate);
    if text_len < 120 {
        return 0.0;
    }

    let mut link_text_len = 0usize;
    for a in candidate.select(&a_selector) {
        link_text_len = link_text_len.saturating_add(element_text_len(&a));
    }

    let link_density = (link_text_len as f32 / text_len as f32).min(1.0);
    if link_density > 0.75 {
        return 0.0;
    }

    let tag_bonus = match candidate.value().name() {
        "article" => 800.0,
        "main" => 650.0,
        "section" => 250.0,
        _ => 0.0,
    };

    let weight = class_id_weight(candidate) as f32;
    let comma_count = count_commas(candidate) as f32;

    let mut score = tag_bonus;
    score += weight * 25.0;
    score += (paragraph_text_len as f32) * (1.0 - link_density);
    score += (paragraph_count as f32) * 120.0;
    score += comma_count * 20.0;

    if paragraph_text_len < 400 {
        score *= 0.85;
    }
    if link_density > 0.5 {
        score *= 0.6;
    }

    score
}

fn class_id_weight(element: &ElementRef<'_>) -> i32 {
    let mut weight = 0i32;
    if let Some(id) = element.value().attr("id") {
        weight += keyword_weight(id);
    }
    if let Some(class) = element.value().attr("class") {
        weight += keyword_weight(class);
    }
    if let Some(role) = element.value().attr("role") {
        weight += keyword_weight(role);
    }
    weight
}

fn keyword_weight(value: &str) -> i32 {
    let value = value.to_ascii_lowercase();
    let mut weight = 0i32;
    for keyword in POSITIVE_KEYWORDS {
        if value.contains(keyword) {
            weight += 25;
        }
    }
    for keyword in NEGATIVE_KEYWORDS {
        if value.contains(keyword) {
            weight -= 25;
        }
    }
    weight
}

fn is_unlikely_candidate(element: &ElementRef<'_>) -> bool {
    let mut combined = String::new();
    if let Some(id) = element.value().attr("id") {
        combined.push_str(id);
        combined.push(' ');
    }
    if let Some(class) = element.value().attr("class") {
        combined.push_str(class);
        combined.push(' ');
    }
    if let Some(role) = element.value().attr("role") {
        combined.push_str(role);
    }

    let combined = combined.to_ascii_lowercase();
    let has_negative = NEGATIVE_KEYWORDS.iter().any(|kw| combined.contains(kw));
    let has_positive = POSITIVE_KEYWORDS.iter().any(|kw| combined.contains(kw));
    has_negative && !has_positive
}

fn count_commas(element: &ElementRef<'_>) -> usize {
    element
        .text()
        .flat_map(|s| s.chars())
        .filter(|ch| *ch == ',' || *ch == '，')
        .count()
}

fn extract_blocks(root: &ElementRef<'_>, base_url: &url::Url) -> Vec<ReaderBlock> {
    let mut blocks = Vec::new();
    collect_blocks(root, base_url, 0, &mut blocks);
    let mut blocks = normalize_blocks(blocks);

    if blocks.is_empty() || total_text_len(&blocks) < 200 {
        let paragraphs = extract_paragraphs(root);
        blocks = paragraphs.into_iter().map(ReaderBlock::Paragraph).collect();
    }

    blocks.truncate(MAX_BLOCKS);
    blocks
}

fn collect_blocks(
    element: &ElementRef<'_>,
    base_url: &url::Url,
    depth: usize,
    out: &mut Vec<ReaderBlock>,
) {
    if out.len() >= MAX_BLOCKS || depth > 40 {
        return;
    }

    for child in element.child_elements() {
        if out.len() >= MAX_BLOCKS {
            break;
        }
        if should_skip_subtree(&child) {
            continue;
        }

        match child.value().name() {
            "p" => {
                if let Some(text) = extract_text(&child) {
                    if !is_noise_paragraph(&text) {
                        out.push(ReaderBlock::Paragraph(text));
                    }
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if let Some(text) = extract_text(&child) {
                    let level = heading_level(child.value().name());
                    out.push(ReaderBlock::Heading { level, text });
                }
            }
            "blockquote" => {
                if let Some(text) = extract_blockquote_text(&child) {
                    out.push(ReaderBlock::Quote(text));
                }
            }
            "ul" => {
                if let Some(items) = extract_list_items(&child) {
                    out.push(ReaderBlock::List {
                        ordered: false,
                        items,
                    });
                }
            }
            "ol" => {
                if let Some(items) = extract_list_items(&child) {
                    out.push(ReaderBlock::List {
                        ordered: true,
                        items,
                    });
                }
            }
            "pre" => {
                if let Some((text, language)) = extract_code_block(&child) {
                    out.push(ReaderBlock::Code { text, language });
                }
            }
            "figure" => {
                if let Some(block) = extract_figure_image(&child, base_url) {
                    out.push(block);
                } else {
                    collect_blocks(&child, base_url, depth + 1, out);
                }
            }
            "img" => {
                if let Some(block) = extract_image(&child, base_url, None) {
                    out.push(block);
                }
            }
            "hr" => out.push(ReaderBlock::Rule),
            "article" | "main" | "section" | "div" => {
                collect_blocks(&child, base_url, depth + 1, out)
            }
            _ => collect_blocks(&child, base_url, depth + 1, out),
        }
    }
}

fn should_skip_subtree(element: &ElementRef<'_>) -> bool {
    if element.value().attr("hidden").is_some() {
        return true;
    }
    if element
        .value()
        .attr("aria-hidden")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return true;
    }

    match element.value().name() {
        "script" | "style" | "noscript" | "header" | "footer" | "nav" | "aside" | "form"
        | "button" | "input" | "textarea" | "select" | "option" | "iframe" | "canvas" => true,
        _ => is_unlikely_candidate(element),
    }
}

fn extract_text(element: &ElementRef<'_>) -> Option<String> {
    let raw = element.text().collect::<Vec<_>>().join(" ");
    let text = normalize_whitespace(&raw);
    (!text.is_empty()).then_some(text)
}

fn extract_blockquote_text(element: &ElementRef<'_>) -> Option<String> {
    let p_selector = Selector::parse("p").ok()?;
    let mut paragraphs = element
        .select(&p_selector)
        .filter_map(|p| extract_text(&p))
        .collect::<Vec<_>>();

    if paragraphs.is_empty() {
        return extract_text(element);
    }

    paragraphs.truncate(20);
    Some(paragraphs.join("\n\n"))
}

fn extract_list_items(list: &ElementRef<'_>) -> Option<Vec<String>> {
    let mut items = Vec::new();
    for child in list.child_elements() {
        if child.value().name() != "li" {
            continue;
        }
        if should_skip_subtree(&child) {
            continue;
        }
        if let Some(text) = extract_text(&child) {
            if !is_noise_paragraph(&text) {
                items.push(text);
            }
        }
        if items.len() >= 50 {
            break;
        }
    }
    (!items.is_empty()).then_some(items)
}

fn extract_code_block(pre: &ElementRef<'_>) -> Option<(String, Option<String>)> {
    let code_selector = Selector::parse("code").ok()?;
    let code = pre.select(&code_selector).next();

    let raw = match code {
        Some(code) => code.text().collect::<Vec<_>>().join(""),
        None => pre.text().collect::<Vec<_>>().join(""),
    };

    let text = normalize_code_text(&raw);
    if text.is_empty() {
        return None;
    }

    let language = code.and_then(detect_code_language);
    Some((text, language))
}

fn detect_code_language(code: ElementRef<'_>) -> Option<String> {
    let class = code.value().attr("class")?;
    for token in class.split_whitespace() {
        let token = token.trim();
        if let Some(lang) = token.strip_prefix("language-") {
            let lang = lang.trim();
            if !lang.is_empty() {
                return Some(lang.to_string());
            }
        }
        if let Some(lang) = token.strip_prefix("lang-") {
            let lang = lang.trim();
            if !lang.is_empty() {
                return Some(lang.to_string());
            }
        }
    }
    None
}

fn extract_figure_image(figure: &ElementRef<'_>, base_url: &url::Url) -> Option<ReaderBlock> {
    let img_selector = Selector::parse("img").ok()?;
    let img = figure.select(&img_selector).next()?;

    let caption = {
        let caption_selector = Selector::parse("figcaption").ok()?;
        figure
            .select(&caption_selector)
            .next()
            .and_then(|c| extract_text(&c))
    };

    extract_image(&img, base_url, caption)
}

fn extract_image(
    img: &ElementRef<'_>,
    base_url: &url::Url,
    caption: Option<String>,
) -> Option<ReaderBlock> {
    let raw_src = image_src(img)?;
    let url = resolve_url(base_url, &raw_src)?;

    let alt = img
        .value()
        .attr("alt")
        .map(normalize_whitespace)
        .filter(|s| !s.is_empty());

    if is_likely_noise_image_url(&url, &alt, &caption) {
        return None;
    }

    Some(ReaderBlock::Image { url, alt, caption })
}

fn image_src(img: &ElementRef<'_>) -> Option<String> {
    let value = img.value();
    let candidates = [
        "src",
        "data-src",
        "data-original",
        "data-lazy-src",
        "data-actualsrc",
    ];

    for attr in candidates {
        if let Some(src) = value.attr(attr) {
            let src = src.trim();
            if !src.is_empty() {
                return Some(src.to_string());
            }
        }
    }

    value.attr("srcset").and_then(parse_srcset)
}

fn parse_srcset(srcset: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for item in srcset.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let url = item.split_whitespace().next().unwrap_or("").trim();
        if !url.is_empty() {
            best = Some(url.to_string());
        }
    }
    best
}

fn resolve_url(base_url: &url::Url, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("data:") {
        return None;
    }

    if raw.starts_with("//") {
        return Some(format!("{}:{}", base_url.scheme(), raw));
    }

    if let Ok(url) = url::Url::parse(raw) {
        return Some(url.to_string());
    }

    base_url.join(raw).ok().map(|u| u.to_string())
}

fn is_likely_noise_image_url(url: &str, alt: &Option<String>, caption: &Option<String>) -> bool {
    let url_lower = url.to_ascii_lowercase();

    let always_bad = [
        "sprite",
        "favicon",
        "avatar",
        "badge",
        "spinner",
        "/ads/",
        "doubleclick",
    ];
    if always_bad.iter().any(|k| url_lower.contains(k)) {
        return true;
    }

    let maybe_bad = ["logo", "icon"];
    if maybe_bad.iter().any(|k| url_lower.contains(k)) {
        let has_context = caption.as_ref().is_some_and(|c| !c.is_empty())
            || alt.as_ref().is_some_and(|a| a.len() >= 8);
        return !has_context;
    }

    false
}

fn heading_level(tag: &str) -> u8 {
    match tag {
        "h1" => 1,
        "h2" => 2,
        "h3" => 3,
        "h4" => 4,
        "h5" => 5,
        "h6" => 6,
        _ => 2,
    }
}

fn normalize_code_text(input: &str) -> String {
    let input = input.replace("\r\n", "\n").replace('\t', "    ");
    let mut lines = input.lines().collect::<Vec<_>>();

    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }

    let mut min_indent = usize::MAX;
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        min_indent = min_indent.min(indent);
    }
    if min_indent == usize::MAX {
        min_indent = 0;
    }
    let dedent_prefix = " ".repeat(min_indent);

    let mut out_lines = Vec::new();
    for line in lines {
        let line = line.strip_prefix(&dedent_prefix).unwrap_or(line);

        let mut out = String::with_capacity(line.len());
        let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
        for _ in 0..leading_spaces {
            out.push('\u{00A0}');
        }
        out.push_str(line.trim_start_matches(' '));
        out_lines.push(out);
    }

    out_lines.join("\n")
}

fn is_noise_paragraph(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.len() < 6 {
        return true;
    }
    let noise_tokens = [
        "cookie",
        "sign in",
        "log in",
        "subscribe",
        "newsletter",
        "advert",
        "sponsored",
        "privacy policy",
        "terms of service",
    ];
    noise_tokens.iter().any(|t| lower.contains(t))
}

fn normalize_blocks(blocks: Vec<ReaderBlock>) -> Vec<ReaderBlock> {
    let mut out = Vec::new();

    for block in blocks {
        let block = match block {
            ReaderBlock::Heading { level, text } => {
                let text = normalize_whitespace(&text);
                if text.is_empty() {
                    continue;
                }
                ReaderBlock::Heading { level, text }
            }
            ReaderBlock::Paragraph(text) => {
                let text = normalize_whitespace(&text);
                if text.is_empty() {
                    continue;
                }
                ReaderBlock::Paragraph(text)
            }
            ReaderBlock::Quote(text) => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                ReaderBlock::Quote(text)
            }
            ReaderBlock::List { ordered, items } => {
                let items = items
                    .into_iter()
                    .map(|s| normalize_whitespace(&s))
                    .filter(|s| !s.is_empty())
                    .take(100)
                    .collect::<Vec<_>>();
                if items.is_empty() {
                    continue;
                }
                ReaderBlock::List { ordered, items }
            }
            ReaderBlock::Code { text, language } => {
                let text = text.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                ReaderBlock::Code { text, language }
            }
            ReaderBlock::Image { url, alt, caption } => {
                if url.trim().is_empty() {
                    continue;
                }
                ReaderBlock::Image {
                    url,
                    alt: alt.and_then(|s| {
                        let s = normalize_whitespace(&s);
                        (!s.is_empty()).then_some(s)
                    }),
                    caption: caption.and_then(|s| {
                        let s = normalize_whitespace(&s);
                        (!s.is_empty()).then_some(s)
                    }),
                }
            }
            ReaderBlock::Rule => ReaderBlock::Rule,
        };

        if let Some(prev) = out.last() {
            if matches!(
                (prev, &block),
                (ReaderBlock::Paragraph(a), ReaderBlock::Paragraph(b)) if a == b
            ) {
                continue;
            }
        }

        out.push(block);
        if out.len() >= MAX_BLOCKS {
            break;
        }
    }

    out
}

fn total_text_len(blocks: &[ReaderBlock]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            ReaderBlock::Heading { text, .. } => text.len(),
            ReaderBlock::Paragraph(text) => text.len(),
            ReaderBlock::Quote(text) => text.len(),
            ReaderBlock::List { items, .. } => items.iter().map(|s| s.len()).sum(),
            ReaderBlock::Code { text, .. } => text.len(),
            ReaderBlock::Image { alt, caption, .. } => {
                alt.as_ref().map_or(0, |s| s.len()) + caption.as_ref().map_or(0, |s| s.len())
            }
            ReaderBlock::Rule => 0,
        })
        .sum()
}

fn extract_paragraphs(root: &ElementRef<'_>) -> Vec<String> {
    let selector = match Selector::parse("p") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut paragraphs = Vec::new();
    for p in root.select(&selector) {
        let raw = p.text().collect::<Vec<_>>().join(" ");
        let text = normalize_whitespace(&raw);
        if text.is_empty() {
            continue;
        }
        paragraphs.push(text);
        if paragraphs.len() >= 200 {
            break;
        }
    }

    if paragraphs.is_empty() {
        let raw = root.text().collect::<Vec<_>>().join("\n");
        paragraphs = split_paragraphs(&raw);
    }

    paragraphs
}

fn split_paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|p| normalize_whitespace(p))
        .filter(|p| !p.is_empty())
        .take(200)
        .collect()
}

fn estimate_reading_time(blocks: &[ReaderBlock]) -> Option<String> {
    let (mut words, mut chars) = (0usize, 0usize);

    let mut add_text = |text: &str| {
        words = words.saturating_add(text.split_whitespace().count());
        chars = chars.saturating_add(text.chars().count());
    };

    for block in blocks {
        match block {
            ReaderBlock::Heading { text, .. } => add_text(text),
            ReaderBlock::Paragraph(text) => add_text(text),
            ReaderBlock::Quote(text) => add_text(text),
            ReaderBlock::List { items, .. } => {
                for item in items {
                    add_text(item);
                }
            }
            ReaderBlock::Code { text, .. } => add_text(text),
            ReaderBlock::Image { alt, caption, .. } => {
                if let Some(alt) = alt {
                    add_text(alt);
                }
                if let Some(caption) = caption {
                    add_text(caption);
                }
            }
            ReaderBlock::Rule => {}
        }
    }

    if words == 0 && chars == 0 {
        return None;
    }

    let minutes_by_words = (words + 199) / 200;
    let minutes_by_chars = (chars + 999) / 1000;
    let minutes = minutes_by_words.max(minutes_by_chars).max(1);
    Some(format!("{minutes} min read"))
}

fn element_text_len(element: &ElementRef<'_>) -> usize {
    element
        .text()
        .map(|s| s.split_whitespace().map(|w| w.len()).sum::<usize>())
        .sum()
}

fn normalize_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}
