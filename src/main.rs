mod api;
mod models;
mod theme;

use api::HackerNewsClient;
use gpui::prelude::*;
use gpui::{
    div, hsla, point, px, rems, size, App, AppContext, AsyncWindowContext, Bounds, Div,
    ElementId, FocusHandle, FontWeight, Hsla, IntoElement, Render, Stateful, TitlebarOptions,
    ViewContext, WeakView, WindowBounds, WindowOptions,
};
use models::{Comment, NewsChannel, Story};
use reqwest_client::ReqwestClient;
use std::collections::HashSet;
use std::sync::Arc;
use theme::Theme;

/// macOS traffic light 按钮区域的高度
const TITLEBAR_HEIGHT: f32 = 38.0;

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
    client: Arc<HackerNewsClient>,
    focus_handle: FocusHandle,
}

impl AppState {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let http_client = cx.app().http_client();
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
            client: Arc::new(HackerNewsClient::new(http_client)),
            focus_handle,
        }
    }

    fn selected_story(&self) -> Option<&Story> {
        self.selected_story_id
            .and_then(|id| self.stories.iter().find(|s| s.id == id))
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

    /// 检查评论是否应该被隐藏（因为其父评论被折叠）
    fn is_hidden_by_parent(&self, comment: &Comment) -> bool {
        // 找到所有深度小于当前评论的评论，检查它们是否被折叠
        for c in &self.comments {
            if c.depth < comment.depth && self.collapsed_comments.contains(&c.id) {
                // 检查 comment 是否是 c 的后代
                // 简单实现：如果 c 在 comments 列表中出现在 comment 之前，
                // 且 c 的深度小于 comment，则认为 comment 是 c 的后代
                let c_idx = self.comments.iter().position(|x| x.id == c.id);
                let comment_idx = self.comments.iter().position(|x| x.id == comment.id);
                if let (Some(ci), Some(coi)) = (c_idx, comment_idx) {
                    if ci < coi {
                        // 检查在 c 和 comment 之间是否所有评论的深度都大于 c
                        let is_descendant = self.comments[ci + 1..coi]
                            .iter()
                            .all(|x| x.depth > c.depth);
                        if is_descendant {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn load_stories(&mut self, cx: &mut ViewContext<Self>) {
        self.is_loading = true;
        self.error_message = None;
        cx.notify();

        let client = self.client.clone();

        cx.spawn(|this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
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
        })
        .detach();
    }

    fn select_story(&mut self, story_id: i64, cx: &mut ViewContext<Self>) {
        let story = self.stories.iter().find(|s| s.id == story_id).cloned();

        if let Some(story) = story {
            self.selected_story_id = Some(story_id);
            self.comments.clear();
            self.collapsed_comments.clear();
            self.is_loading_comments = true;
            cx.notify();

            let client = self.client.clone();

            cx.spawn(|this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
                let result = client.fetch_comments(&story).await;
                let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                    match result {
                        Ok(comments) => {
                            this.comments = comments;
                        }
                        Err(e) => {
                            this.error_message = Some(format!("Failed to load comments: {}", e));
                        }
                    }
                    this.is_loading_comments = false;
                    cx.notify();
                });
            })
            .detach();
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
            // Sidebar
            .child(self.render_sidebar())
            // Story List
            .child(self.render_story_list(cx))
            // Detail Panel
            .child(self.render_detail_panel(cx))
    }
}

impl AppState {
    fn render_sidebar(&self) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w(px(56.))
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
            .w(px(360.))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.bg_secondary)
            .border_r_1()
            .border_color(theme.border_subtle)
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
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .px_4()
                            .child(
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

    fn render_loading_indicator(&self) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.text_muted)
            .child("Loading...")
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
            .flex()
            .flex_row()
            .items_center()
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
            .child(if let Some(story) = self.selected_story() {
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
                        .child(clean_text),
                )
            })
            // Comments section
            .child(self.render_comments_section(cx))
    }

    fn render_story_header(&self, story: &Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let url = story.url.clone();
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
                            .child(story.title.clone()),
                    )
                    // Meta
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
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
                                this.child(
                                    div()
                                        .id("open-link-btn")
                                        .cursor_pointer()
                                        .text_color(accent)
                                        .hover(move |s| s.text_color(accent_hover))
                                        .on_click(cx.listener(move |_this, _event, _cx| {
                                            let _ = open::that(&url);
                                        }))
                                        .child("Open Link ↗"),
                                )
                            }),
                    ),
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
            .overflow_hidden()
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
                div()
                    .w_full()
                    .py_8()
                    .flex()
                    .justify_center()
                    .text_color(theme.text_muted)
                    .child("Loading comments...")
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
                    .gap_1()
                    .overflow_hidden()
                    .children(
                        self.comments
                            .iter()
                            .filter(|c| !self.is_hidden_by_parent(c))
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
        let text_secondary = theme.text_secondary;
        let text_muted = theme.text_muted;
        let text_primary = theme.text_primary;
        let bg_secondary = theme.bg_secondary;

        div()
            .id(ElementId::Name(format!("comment-{}", comment_id).into()))
            .w_full()
            .min_w(px(0.))
            .pl(px(indent))
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .py_2()
                    .px_3()
                    .bg(bg_secondary)
                    .border_l_2()
                    .border_color(border_color)
                    .overflow_hidden()
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .overflow_hidden()
                            // Author, time, and collapse button
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    // Collapse/Expand button
                                    .when(has_replies, |this| {
                                        this.child(
                                            div()
                                                .id(ElementId::Name(
                                                    format!("collapse-{}", comment_id).into(),
                                                ))
                                                .cursor_pointer()
                                                .px_1()
                                                .rounded(px(2.))
                                                .text_color(text_muted)
                                                .hover(|s| s.bg(hsla(0., 0., 0.5, 0.1)))
                                                .on_click(cx.listener(
                                                    move |this, _event, cx| {
                                                        this.toggle_collapse(comment_id, cx);
                                                    },
                                                ))
                                                .child(if is_collapsed {
                                                    format!("[+{}]", reply_count)
                                                } else {
                                                    "[-]".to_string()
                                                }),
                                        )
                                    })
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(text_secondary)
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
                                        .overflow_hidden()
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
