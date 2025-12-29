mod api;
mod models;
mod reader;
mod reader_view;
mod theme;

#[cfg(test)]
mod scroll_tests;

use api::HackerNewsClient;
use gpui::http_client::HttpClient;
use gpui::prelude::*;
use gpui::{
    div, hsla, point, px, rems, size, AnyElement, App, AppContext, AsyncWindowContext, Bounds,
    Div, ElementId, FocusHandle, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Render, Stateful, TitlebarOptions,
    ViewContext, WeakView, WindowBounds, WindowOptions, ScrollHandle,
};
use models::{Comment, NewsChannel, Story};
use reader::{ReaderLoadState, ReaderSession};
use reqwest_client::ReqwestClient;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use theme::Theme;

/// macOS traffic light 按钮区域的高度
const TITLEBAR_HEIGHT: f32 = 38.0;
const SIDEBAR_WIDTH: f32 = 56.0;
const STORY_LIST_DEFAULT_WIDTH: f32 = 360.0;
const STORY_LIST_MIN_WIDTH: f32 = 240.0;
const STORY_LIST_MIN_DETAIL_WIDTH: f32 = 360.0;
const SPLITTER_WIDTH: f32 = 8.0;
const READER_CACHE_MAX_ENTRIES: usize = 32;

// Application State
struct AppState {
    theme: Theme,
    stories: Vec<Story>,
    selected_story_id: Option<i64>,
    comments: Vec<Comment>,
    collapsed_comments: HashSet<i64>,
    is_loading: bool,
    is_loading_comments: bool,
    error_message: Option<String>,
    selected_channel: NewsChannel,
    http_client: Arc<dyn HttpClient>,
    client: Arc<HackerNewsClient>,
    reader: Option<ReaderSession>,
    reader_cache: HashMap<String, reader::ReaderArticle>,
    reader_cache_order: VecDeque<String>,
    reader_scroll_handle: ScrollHandle,
    debug_reader_scroll: bool,
    focus_handle: FocusHandle,
    story_list_width: f32,
    is_resizing_story_list: bool,
    resize_start_x: f32,
    resize_start_width: f32,
}

impl AppState {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let http_client = cx.app().http_client();
        let debug_reader_scroll = std::env::var_os("ONEAPP_DEBUG_READER_SCROLL").is_some();
        Self {
            theme: Theme::default(),
            stories: Vec::new(),
            selected_story_id: None,
            comments: Vec::new(),
            collapsed_comments: HashSet::new(),
            is_loading: true,
            is_loading_comments: false,
            error_message: None,
            selected_channel: NewsChannel::HackerNews,
            http_client: http_client.clone(),
            client: Arc::new(HackerNewsClient::new(http_client)),
            reader: None,
            reader_cache: HashMap::new(),
            reader_cache_order: VecDeque::new(),
            reader_scroll_handle: ScrollHandle::new(),
            debug_reader_scroll,
            focus_handle,
            story_list_width: STORY_LIST_DEFAULT_WIDTH,
            is_resizing_story_list: false,
            resize_start_x: 0.0,
            resize_start_width: STORY_LIST_DEFAULT_WIDTH,
        }
    }

    fn selected_story(&self) -> Option<&Story> {
        self.selected_story_id
            .and_then(|id| self.stories.iter().find(|s| s.id == id))
    }

    fn cached_reader_article(&mut self, url: &str) -> Option<reader::ReaderArticle> {
        let article = self.reader_cache.get(url).cloned()?;
        self.touch_reader_cache(url);
        Some(article)
    }

    fn cache_reader_article(&mut self, url: String, article: reader::ReaderArticle) {
        self.reader_cache.insert(url.clone(), article);
        self.touch_reader_cache(&url);

        while self.reader_cache_order.len() > READER_CACHE_MAX_ENTRIES {
            if let Some(evicted) = self.reader_cache_order.pop_front() {
                self.reader_cache.remove(&evicted);
            }
        }
    }

    fn touch_reader_cache(&mut self, url: &str) {
        self.reader_cache_order.retain(|u| u != url);
        self.reader_cache_order.push_back(url.to_string());
    }

    fn toggle_collapse(&mut self, comment_id: i64, cx: &mut ViewContext<Self>) {
        if self.collapsed_comments.contains(&comment_id) {
            self.collapsed_comments.remove(&comment_id);
        } else {
            self.collapsed_comments.insert(comment_id);
        }
        cx.notify();
    }

    fn is_collapsed(&self, comment_id: i64) -> bool {
        self.collapsed_comments.contains(&comment_id)
    }

    fn visible_comments(&self) -> Vec<&Comment> {
        let mut visible = Vec::new();
        let mut skip_until_depth: Option<usize> = None;

        for comment in &self.comments {
            if let Some(depth) = skip_until_depth {
                if comment.depth > depth {
                    continue;
                }
                skip_until_depth = None;
            }

            visible.push(comment);

            if self.is_collapsed(comment.id) {
                skip_until_depth = Some(comment.depth);
            }
        }

        visible
    }

    fn load_stories(&mut self, cx: &mut ViewContext<Self>) {
        self.is_loading = true;
        self.error_message = None;
        cx.notify();

        let client = self.client.clone();

        cx.spawn(
            |this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
                let result = client.fetch_top_stories(30).await;
                let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                    match result {
                        Ok(stories) => {
                            this.stories = stories;
                            this.error_message = None;
                        }
                        Err(e) => {
                            this.error_message = Some(format!("Failed to load stories: {}", e));
                        }
                    }
                    this.is_loading = false;
                    cx.notify();
                });
            },
        )
        .detach();
    }

    fn select_story(&mut self, story_id: i64, cx: &mut ViewContext<Self>) {
        self.reader = None;
        let story = self.stories.iter().find(|s| s.id == story_id).cloned();

        if let Some(story) = story {
            self.selected_story_id = Some(story_id);
            self.comments.clear();
            self.collapsed_comments.clear();
            self.is_loading_comments = true;
            cx.notify();

            let client = self.client.clone();

            cx.spawn(
                |this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
                    let result = client.fetch_comments(&story).await;
                    let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                        match result {
                            Ok(comments) => {
                                this.comments = comments;
                            }
                            Err(e) => {
                                this.error_message =
                                    Some(format!("Failed to load comments: {}", e));
                            }
                        }
                        this.is_loading_comments = false;
                        cx.notify();
                    });
                },
            )
            .detach();
        }
    }

    fn start_story_list_resize(&mut self, event: &MouseDownEvent, cx: &mut ViewContext<Self>) {
        if event.click_count >= 2 {
            self.story_list_width = STORY_LIST_DEFAULT_WIDTH;
            self.is_resizing_story_list = false;
            cx.notify();
            return;
        }

        self.is_resizing_story_list = true;
        self.resize_start_x = event.position.x.0;
        self.resize_start_width = self.story_list_width;
        cx.notify();
    }

    fn update_story_list_resize(&mut self, event: &MouseMoveEvent, cx: &mut ViewContext<Self>) {
        if !self.is_resizing_story_list {
            return;
        }

        let delta = event.position.x.0 - self.resize_start_x;
        let viewport_width = cx.window_context().viewport_size().width.0;
        let max_by_window =
            (viewport_width - SIDEBAR_WIDTH - SPLITTER_WIDTH - STORY_LIST_MIN_DETAIL_WIDTH)
                .max(STORY_LIST_MIN_WIDTH);

        self.story_list_width =
            (self.resize_start_width + delta).clamp(STORY_LIST_MIN_WIDTH, max_by_window);
        cx.notify();
    }

    fn stop_story_list_resize(&mut self, _: &MouseUpEvent, cx: &mut ViewContext<Self>) {
        if self.is_resizing_story_list {
            self.is_resizing_story_list = false;
            cx.notify();
        }
    }
}

impl Render for AppState {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(theme.bg_primary)
            .text_color(theme.text_primary)
            .font_family(".SystemUIFont")
            .track_focus(&self.focus_handle)
            .on_mouse_move(cx.listener(Self::update_story_list_resize))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_story_list_resize))
            // Sidebar
            .child(self.render_sidebar())
            // Story List
            .child(self.render_story_list(cx))
            // Splitter
            .child(self.render_story_splitter(cx))
            // Detail Panel
            .child(self.render_detail_panel(cx))
    }
}

impl AppState {
    fn render_sidebar(&self) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(theme.bg_secondary)
            .border_r_1()
            .border_color(theme.border_subtle)
            // 顶部留空给 traffic lights
            .child(div().h(px(TITLEBAR_HEIGHT)).w_full().flex_shrink_0())
            // Channel icon
            .child(
                div()
                    .mt_2()
                    .w(px(40.))
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .bg(theme.accent)
                    .text_color(hsla(0., 0., 1., 1.0))
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child(self.selected_channel.icon()),
            )
    }

    fn render_story_list(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w(px(self.story_list_width))
            .flex_shrink()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.bg_secondary)
            // Header with titlebar spacing
            .child(
                div()
                    .w_full()
                    .h(px(TITLEBAR_HEIGHT + 52.))
                    .flex()
                    .flex_col()
                    .border_b_1()
                    .border_color(theme.border_subtle)
                    // Titlebar spacer
                    .child(div().h(px(TITLEBAR_HEIGHT)).w_full().flex_shrink_0())
                    // Title
                    .child(
                        div().flex_1().flex().items_center().px_4().child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.selected_channel.name()),
                        ),
                    ),
            )
            // Error message
            .when_some(self.error_message.clone(), |this, msg| {
                this.child(
                    div()
                        .w_full()
                        .px_4()
                        .py_2()
                        .bg(theme.error)
                        .text_color(hsla(0., 0., 1., 1.0))
                        .text_sm()
                        .child(msg),
                )
            })
            // Stories
            .child(
                div()
                    .id("story-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(if self.is_loading {
                        vec![self.render_loading_indicator().into_any_element()]
                    } else {
                        self.stories
                            .iter()
                            .map(|story| self.render_story_row(story, cx).into_any_element())
                            .collect()
                    }),
            )
    }

    fn render_story_splitter(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let is_resizing = self.is_resizing_story_list;
        let divider_color = if is_resizing {
            theme.border
        } else {
            theme.border_subtle
        };

        div()
            .id("story-splitter")
            .w(px(SPLITTER_WIDTH))
            .h_full()
            .flex()
            .flex_row()
            .cursor_col_resize()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::start_story_list_resize),
            )
            // Left half blends with story list background; right half blends with detail background.
            .child(div().flex_1().h_full().bg(theme.bg_secondary))
            .child(div().w(px(1.)).h_full().bg(divider_color))
            .child(div().flex_1().h_full().bg(theme.bg_primary))
    }

    fn render_loading_indicator(&self) -> impl IntoElement {
        let theme = &self.theme;

        let skeleton_bar = |max_w: f32, h: f32| {
            div()
                .h(px(h))
                .w_full()
                .max_w(px(max_w))
                .rounded(px(3.))
                .bg(theme.bg_tertiary)
        };

        let placeholders: Vec<_> = (0..10)
            .map(|i| {
                let title_max_w = match i % 3 {
                    0 => 280.0,
                    1 => 240.0,
                    _ => 200.0,
                };

                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border_subtle)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(skeleton_bar(title_max_w, 14.0))
                            .child(div().w_full().flex().gap_2().children(vec![
                                skeleton_bar(96.0, 10.0).into_any_element(),
                                skeleton_bar(72.0, 10.0).into_any_element(),
                                skeleton_bar(56.0, 10.0).into_any_element(),
                            ])),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .px_4()
                    .py_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(theme.text_muted)
                    .child("⏳")
                    .child("Loading stories…"),
            )
            .children(placeholders)
    }

    fn render_story_row(&self, story: &Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let is_selected = self.selected_story_id == Some(story.id);

        let bg_color = if is_selected {
            theme.bg_selected
        } else {
            theme.bg_secondary
        };

        let story_id = story.id;
        let title = story.title.clone();
        let score = story.score;
        let by = story.by.clone();
        let domain = story.domain();
        let formatted_time = story.formatted_time();
        let comment_count = story.comment_count();
        let hover_bg = theme.bg_hover;
        let accent = theme.accent;
        let text_muted = theme.text_muted;
        let text_secondary = theme.text_secondary;
        let border_subtle = theme.border_subtle;

        div()
            .id(ElementId::Name(format!("story-{}", story_id).into()))
            .w_full()
            .px_4()
            .py_3()
            .cursor_pointer()
            .bg(bg_color)
            .hover(move |s| s.bg(hover_bg))
            .border_b_1()
            .border_color(border_subtle)
            .on_click(cx.listener(move |this, _event, cx| {
                this.select_story(story_id, cx);
            }))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .gap_1()
                    // Title
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(rems(1.4))
                            .whitespace_normal()
                            .child(title),
                    )
                    // Meta row
                    .child(self.render_story_meta(
                        score,
                        domain,
                        &by,
                        &formatted_time,
                        comment_count,
                        accent,
                        text_muted,
                        text_secondary,
                    )),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_story_meta(
        &self,
        score: i32,
        domain: Option<String>,
        by: &str,
        formatted_time: &str,
        comment_count: i32,
        accent: Hsla,
        text_muted: Hsla,
        text_secondary: Hsla,
    ) -> impl IntoElement {
        div()
            .min_w(px(0.))
            .flex()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap_3()
            .text_xs()
            .text_color(text_muted)
            // Score
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(accent)
                    .child("▲")
                    .child(score.to_string()),
            )
            // Domain
            .when_some(domain, |this, domain| {
                this.child(div().text_color(text_secondary).child(domain))
            })
            // Author
            .child(format!("by {}", by))
            // Time
            .child(formatted_time.to_string())
            // Comments
            .when(comment_count > 0, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child("💬")
                        .child(comment_count.to_string()),
                )
            })
    }

    fn render_detail_panel(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.bg_primary)
            .overflow_hidden()
            // Titlebar spacer
            .child(div().h(px(TITLEBAR_HEIGHT)).w_full().flex_shrink_0())
            .child(if let Some(reader) = self.reader.as_ref() {
                self.render_reader_page(reader, cx).into_any_element()
            } else if let Some(story) = self.selected_story() {
                self.render_story_detail(story, cx).into_any_element()
            } else {
                self.render_empty_state().into_any_element()
            })
    }

    fn render_empty_state(&self) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Select a story to read")
    }

    fn open_reader(&mut self, url: String, title_hint: Option<String>, cx: &mut ViewContext<Self>) {
        self.reader_scroll_handle.set_offset(point(px(0.), px(0.)));

        if let Some(article) = self.cached_reader_article(&url) {
            self.reader = Some(ReaderSession {
                url,
                title_hint,
                state: ReaderLoadState::Ready(article),
            });
            cx.notify();
            return;
        }

        self.reader = Some(ReaderSession {
            url: url.clone(),
            title_hint: title_hint.clone(),
            state: ReaderLoadState::Loading,
        });
        cx.notify();

        let http_client = self.http_client.clone();

        cx.spawn(
            |this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
                let result = reader::load_article(http_client, &url, title_hint.as_deref()).await;
                let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                    let Some(session) = this.reader.as_mut() else {
                        return;
                    };
                    if session.url != url {
                        return;
                    }

                    match result {
                        Ok(article) => {
                            session.state = ReaderLoadState::Ready(article.clone());
                            this.cache_reader_article(url.clone(), article);
                            // Reset scroll position when article finishes loading
                            this.reader_scroll_handle.set_offset(point(px(0.), px(0.)));
                        }
                        Err(message) => session.state = ReaderLoadState::Error(message),
                    }
                    cx.notify();
                });
            },
        )
        .detach();
    }

    fn close_reader(&mut self, cx: &mut ViewContext<Self>) {
        self.reader = None;
        cx.notify();
    }

    fn render_reader_page(
        &self,
        reader: &ReaderSession,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let text_secondary = theme.text_secondary;
        let text_primary = theme.text_primary;
        let accent = theme.accent;
        let accent_hover = theme.accent_hover;
        let url = reader.url.clone();
        let debug_reader_scroll = self.debug_reader_scroll;
        let scroll_debug = debug_reader_scroll.then(|| {
            let offset_y = self.reader_scroll_handle.offset().y;
            let viewport_h = self.reader_scroll_handle.bounds().size.height;
            let content_h = self
                .reader_scroll_handle
                .bounds_for_item(0)
                .map(|b| b.size.height)
                .unwrap_or_else(|| px(0.));
            let max_scroll = (content_h - viewport_h).max(px(0.));
            format!(
                "y:{:.0} max:{:.0} children:{}",
                offset_y.0,
                max_scroll.0,
                self.reader_scroll_handle.children_count()
            )
        });

        let title = match &reader.state {
            ReaderLoadState::Ready(article) if !article.title.is_empty() => article.title.clone(),
            _ => reader.title_hint.clone().unwrap_or_else(|| url.clone()),
        };

        let content = match &reader.state {
            ReaderLoadState::Loading => self.render_reader_loading().into_any_element(),
            ReaderLoadState::Error(message) => self
                .render_reader_error(message, reader, cx)
                .into_any_element(),
            ReaderLoadState::Ready(article) => {
                self.render_reader_article(article).into_any_element()
            }
        };

        div()
            .id("reader-page")
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .p_6()
                    .bg(theme.bg_secondary)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("reader-back")
                                            .cursor_pointer()
                                            .text_color(text_secondary)
                                            .hover(move |s| s.text_color(text_primary))
                                            .on_click(cx.listener(|this, _event, cx| {
                                                this.close_reader(cx);
                                            }))
                                            .child("← Back"),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .text_sm()
                                            .text_color(theme.text_muted)
                                            .overflow_hidden()
                                            .child(title),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .when_some(scroll_debug, |this, debug| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.text_muted)
                                                .child(debug),
                                        )
                                    })
                                    .child(
                                        div()
                                            .id("reader-open-external")
                                            .cursor_pointer()
                                            .text_color(accent)
                                            .hover(move |s| s.text_color(accent_hover))
                                            .on_click(cx.listener(move |_this, _event, _cx| {
                                                let _ = open::that(&url);
                                            }))
                                            .child("Open in Browser ↗"),
                                    ),
                            ),
                    ),
            )
            .child(content)
    }

    fn render_reader_loading(&self) -> impl IntoElement {
        let theme = &self.theme;

        let skeleton_bar = |max_w: f32, h: f32| {
            div()
                .h(px(h))
                .w_full()
                .max_w(px(max_w))
                .rounded(px(3.))
                .bg(theme.bg_tertiary)
        };

        let placeholders: Vec<_> = (0..10)
            .map(|i| {
                let line_w = match i % 4 {
                    0 => 640.0,
                    1 => 720.0,
                    2 => 680.0,
                    _ => 560.0,
                };
                skeleton_bar(line_w, 12.0).into_any_element()
            })
            .collect();

        div()
            .id("reader-loading-scroll")
            .flex_1()
            .w_full()
            .overflow_y_scroll()
            .child(
                div().w_full().flex().justify_center().child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .px_8()
                        .py_10()
                        .flex()
                        .flex_col()
                        .gap_6()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_color(theme.text_muted)
                                .child("⏳")
                                .child("Loading article…"),
                        )
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .children(placeholders),
                        ),
                ),
            )
    }

    fn render_reader_error(
        &self,
        message: &str,
        reader: &ReaderSession,
        cx: &mut ViewContext<Self>,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let accent = theme.accent;
        let accent_hover = theme.accent_hover;
        let url = reader.url.clone();
        let url_for_open = reader.url.clone();
        let title_hint = reader.title_hint.clone();

        // Convert technical error messages to user-friendly descriptions
        let (friendly_title, friendly_message, suggestion) = Self::parse_error_message(message);

        div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(480.))
                    .p_8()
                    .bg(theme.bg_secondary)
                    .rounded_xl()
                    .border_1()
                    .border_color(theme.border_subtle)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    // Error icon
                    .child(
                        div()
                            .w(px(64.))
                            .h(px(64.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(hsla(0., 0.8, 0.95, 1.0))
                            .text_2xl()
                            .child("⚠️"),
                    )
                    // Title
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(friendly_title),
                    )
                    // Description
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text_secondary)
                                    .whitespace_normal()
                                    .child(friendly_message),
                            )
                            .when_some(suggestion, |this, suggestion| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.text_muted)
                                        .whitespace_normal()
                                        .child(suggestion),
                                )
                            }),
                    )
                    // URL display
                    .child(
                        div()
                            .w_full()
                            .px_3()
                            .py_2()
                            .bg(theme.bg_tertiary)
                            .rounded_md()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(reader.url.clone()),
                    )
                    // Action buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .id("reader-retry")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .bg(theme.accent)
                                    .text_color(hsla(0., 0., 1., 1.0))
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .hover(move |s| s.bg(accent_hover))
                                    .on_click(cx.listener(move |this, _event, cx| {
                                        this.open_reader(url.clone(), title_hint.clone(), cx);
                                    }))
                                    .child("Try Again"),
                            )
                            .child(
                                div()
                                    .id("reader-open-browser")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .px_4()
                                    .py_2()
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_color(accent)
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .hover(move |s| s.bg(theme.bg_hover))
                                    .on_click(cx.listener(move |_this, _event, _cx| {
                                        let _ = open::that(&url_for_open);
                                    }))
                                    .child("Open in Browser"),
                            ),
                    ),
            )
    }

    fn parse_error_message(message: &str) -> (String, String, Option<String>) {
        let msg_lower = message.to_lowercase();

        if msg_lower.contains("error sending request") || msg_lower.contains("connection") {
            (
                "Unable to connect".to_string(),
                "The page couldn't be reached. This might be a network issue or the website may be unavailable.".to_string(),
                Some("Check your internet connection and try again.".to_string()),
            )
        } else if msg_lower.contains("timeout") {
            (
                "Request timed out".to_string(),
                "The server took too long to respond.".to_string(),
                Some("The website might be experiencing high traffic. Try again later.".to_string()),
            )
        } else if msg_lower.contains("http 404") {
            (
                "Page not found".to_string(),
                "The requested page doesn't exist or has been moved.".to_string(),
                None,
            )
        } else if msg_lower.contains("http 403") {
            (
                "Access denied".to_string(),
                "You don't have permission to view this page.".to_string(),
                Some("Try opening it in your browser instead.".to_string()),
            )
        } else if msg_lower.contains("http 5") {
            (
                "Server error".to_string(),
                "The website is experiencing technical difficulties.".to_string(),
                Some("Try again later or open in browser.".to_string()),
            )
        } else if msg_lower.contains("unsupported content type") {
            (
                "Unsupported content".to_string(),
                "This type of content can't be displayed in reader mode.".to_string(),
                Some("Try opening it in your browser instead.".to_string()),
            )
        } else if msg_lower.contains("invalid url") {
            (
                "Invalid URL".to_string(),
                "The link appears to be malformed or invalid.".to_string(),
                None,
            )
        } else if msg_lower.contains("too large") {
            (
                "Page too large".to_string(),
                "This page is too large to load in reader mode.".to_string(),
                Some("Try opening it in your browser instead.".to_string()),
            )
        } else {
            (
                "Couldn't load this page".to_string(),
                message.to_string(),
                Some("Try opening it in your browser instead.".to_string()),
            )
        }
    }

    fn render_reader_block(&self, block: &reader::ReaderBlock) -> AnyElement {
        reader_view::render_reader_block(&self.theme, block)
    }

    fn render_reader_article(&self, article: &reader::ReaderArticle) -> impl IntoElement {
        let theme = &self.theme;

        let meta = [
            article.site_name.clone().unwrap_or_default(),
            article.byline.clone().unwrap_or_default(),
            article.reading_time.clone().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

        div()
            .id("reader-article-scroll")
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .min_w(px(0.))
            .overflow_y_scroll()
            .overflow_x_hidden()
            .track_scroll(&self.reader_scroll_handle)
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .justify_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .max_w(px(760.))
                            .px_8()
                            .py_10()
                            .flex()
                            .flex_col()
                            .gap_6()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .line_height(rems(1.3))
                                            .whitespace_normal()
                                            .child(article.title.clone()),
                                    )
                                    .when(!meta.is_empty(), |this| {
                                        this.child(
                                            div().text_sm().text_color(theme.text_muted).child(meta),
                                        )
                                    }),
                            )
                            .children(
                                article
                                    .blocks
                                    .iter()
                                    .map(|block| self.render_reader_block(block))
                                    .collect::<Vec<_>>(),
                            ),
                    ),
            )
    }

    fn render_story_detail(&self, story: &Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        // Clone values needed for closures
        let story_text = story.text.clone();
        let text_primary = theme.text_primary;

        div()
            .id("story-detail")
            .flex_1()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            // Header
            .child(self.render_story_header(story, cx))
            // Story text if available
            .when_some(story_text, move |this: Stateful<Div>, text: String| {
                let clean_text = html_escape::decode_html_entities(&text).to_string();
                this.child(
                    div()
                        .w_full()
                        .p_6()
                        .text_sm()
                        .line_height(rems(1.6))
                        .text_color(text_primary)
                        .whitespace_normal()
                        .child(clean_text),
                )
            })
            // Comments section
            .child(self.render_comments_section(cx))
    }

    fn render_story_header(&self, story: &Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let url = story.url.clone();
        let title_hint = story.title.clone();
        let accent = theme.accent;
        let accent_hover = theme.accent_hover;

        div()
            .w_full()
            .p_6()
            .bg(theme.bg_secondary)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .overflow_hidden()
                    // Title
                    .child(
                        div()
                            .w_full()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(rems(1.4))
                            .whitespace_normal()
                            .child(story.title.clone()),
                    )
                    // Meta
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_wrap()
                            .gap_4()
                            .text_sm()
                            // Score
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_color(theme.accent)
                                    .child("▲")
                                    .child(format!("{} points", story.score)),
                            )
                            // Author
                            .child(
                                div()
                                    .text_color(theme.text_secondary)
                                    .child(format!("by {}", story.by)),
                            )
                            // Time
                            .child(
                                div()
                                    .text_color(theme.text_muted)
                                    .child(story.formatted_time()),
                            )
                            // Link
                            .when_some(url, |this: Div, url: String| {
                                let title_hint = title_hint.clone();
                                this.child(
                                    div()
                                        .id("open-link-btn")
                                        .cursor_pointer()
                                        .text_color(accent)
                                        .hover(move |s| s.text_color(accent_hover))
                                        .on_click(cx.listener(move |this, _event, cx| {
                                            this.open_reader(
                                                url.clone(),
                                                Some(title_hint.clone()),
                                                cx,
                                            );
                                        }))
                                        .child("Read"),
                                )
                            }),
                    ),
            )
    }

    fn render_comments_loading_indicator(&self) -> Div {
        let theme = &self.theme;

        let skeleton_bar = |max_w: f32, h: f32| {
            div()
                .h(px(h))
                .w_full()
                .max_w(px(max_w))
                .rounded(px(3.))
                .bg(theme.bg_tertiary)
        };

        let placeholders: Vec<_> = (0..6)
            .map(|i| {
                let indent = (i.min(2) * 16) as f32;
                let line_1 = match i % 3 {
                    0 => 360.0,
                    1 => 300.0,
                    _ => 260.0,
                };

                div()
                    .w_full()
                    .min_w(px(0.))
                    .pl(px(indent))
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .py_2()
                            .px_3()
                            .bg(theme.bg_secondary)
                            .border_l_2()
                            .border_color(theme.border_subtle)
                            .child(
                                div()
                                    .w_full()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(skeleton_bar(140.0, 10.0))
                                    .child(skeleton_bar(line_1, 10.0))
                                    .child(skeleton_bar(240.0, 10.0)),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .w_full()
            .py_8()
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .text_color(theme.text_muted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child("💬")
                    .child("Loading comments…"),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(placeholders),
            )
    }

    fn render_comments_section(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .p_6()
            .overflow_x_hidden()
            // Comments header
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mb_4()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Comments")
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .child(format!("({})", self.comments.len())),
                    ),
            )
            // Comments list or loading
            .child(if self.is_loading_comments {
                self.render_comments_loading_indicator()
            } else if self.comments.is_empty() {
                div()
                    .w_full()
                    .py_8()
                    .flex()
                    .justify_center()
                    .text_color(theme.text_muted)
                    .child("No comments yet")
            } else {
                div()
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .bg(theme.bg_secondary)
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_subtle)
                    .children(
                        self.visible_comments()
                            .into_iter()
                            .map(|c| self.render_comment(c, cx)),
                    )
            })
    }

    fn render_comment(&self, comment: &Comment, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let depth = comment.depth;
        let comment_id = comment.id;
        let is_collapsed = self.is_collapsed(comment_id);
        let has_replies = comment.has_replies();
        let reply_count = comment.reply_count;

        // 计算缩进，每层 16px，最大 5 层
        let indent = (depth.min(5) * 16) as f32;

        // 根据层级使用不同的左边框颜色
        let border_colors = [
            theme.accent,
            hsla(200., 0.7, 0.5, 1.0), // 蓝色
            hsla(280., 0.7, 0.5, 1.0), // 紫色
            hsla(160., 0.7, 0.5, 1.0), // 绿色
            hsla(40., 0.7, 0.5, 1.0),  // 黄色
            hsla(340., 0.7, 0.5, 1.0), // 粉色
        ];
        let border_color = border_colors[depth.min(border_colors.len() - 1)];

        let author = comment.author().to_string();
        let time = comment.formatted_time();
        let text = comment.clean_text();
        let text_muted = theme.text_muted;
        let text_primary = theme.text_primary;
        let header_hover_bg = hsla(0., 0., 0.5, 0.06);
        let collapse_label = if is_collapsed {
            format!("▸ {}", reply_count)
        } else {
            format!("▾ {}", reply_count)
        };

        div()
            .id(ElementId::Name(format!("comment-{}", comment_id).into()))
            .w_full()
            .min_w(px(0.))
            .flex_shrink_0()
            .pl(px(indent))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .relative()
                    .bg(theme.bg_primary)
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_subtle)
                    .shadow_sm()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.))
                            .bg(border_color)
                            .rounded_l_md(),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .py_2()
                            .px_3()
                            .flex()
                            .flex_col()
                            .gap_1()
                            // Author, time, and collapse button
                            .child(
                                div()
                                    .id(ElementId::Name(
                                        format!("comment-header-{}", comment_id).into(),
                                    ))
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_2()
                                    .text_xs()
                                    .when(has_replies, |this| {
                                        this.cursor_pointer()
                                            .rounded(px(3.))
                                            .px_1()
                                            .hover(move |s| s.bg(header_hover_bg))
                                            .on_click(cx.listener(move |this, _event, cx| {
                                                this.toggle_collapse(comment_id, cx);
                                            }))
                                    })
                                    .when(has_replies, move |this| {
                                        this.child(
                                            div()
                                                .id(ElementId::Name(
                                                    format!("collapse-{}", comment_id).into(),
                                                ))
                                                .text_color(text_muted)
                                                .child(collapse_label),
                                        )
                                    })
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(text_primary)
                                            .child(author),
                                    )
                                    .child(div().text_color(text_muted).child(time)),
                            )
                            // Comment text
                            .when(!is_collapsed, |this| {
                                this.child(
                                    div()
                                        .w_full()
                                        .min_w(px(0.))
                                        .text_sm()
                                        .line_height(rems(1.5))
                                        .text_color(text_primary)
                                        .whitespace_normal()
                                        .overflow_x_hidden()
                                        .child(text),
                                )
                            }),
                    ),
            )
    }
}

fn main() {
    App::new()
        .with_http_client(Arc::new(ReqwestClient::new()))
        .run(|cx: &mut AppContext| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(800.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("OneRss".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(12.))),
                }),
                ..Default::default()
            };

            cx.open_window(options, |cx| {
                cx.new_view(|cx| {
                    let mut state = AppState::new(cx);
                    state.load_stories(cx);
                    state
                })
            })
            .unwrap();
        });
}
