mod api;
mod models;
mod theme;

use api::HackerNewsClient;
use gpui::prelude::*;
use gpui::{
    div, hsla, point, px, rems, size, App, AppContext, AsyncWindowContext, Bounds, Div,
    ElementId, FocusHandle, FontWeight, IntoElement, Render, Stateful, TitlebarOptions, WeakView,
    ViewContext, WindowBounds, WindowOptions,
};
use models::{Comment, NewsChannel, Story};
use std::sync::Arc;
use theme::Theme;

// Application State
struct AppState {
    theme: Theme,
    stories: Vec<Story>,
    selected_story: Option<Story>,
    comments: Vec<Comment>,
    is_loading: bool,
    is_loading_comments: bool,
    selected_channel: NewsChannel,
    client: Arc<HackerNewsClient>,
    focus_handle: FocusHandle,
}

impl AppState {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            theme: Theme::default(),
            stories: Vec::new(),
            selected_story: None,
            comments: Vec::new(),
            is_loading: true,
            is_loading_comments: false,
            selected_channel: NewsChannel::HackerNews,
            client: Arc::new(HackerNewsClient::new()),
            focus_handle,
        }
    }

    fn load_stories(&mut self, cx: &mut ViewContext<Self>) {
        self.is_loading = true;
        cx.notify();

        let client = self.client.clone();
        let task = cx.background_executor().spawn(async move {
            client.fetch_top_stories(30).unwrap_or_default()
        });

        cx.spawn(|this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
            let stories = task.await;
            let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                this.stories = stories;
                this.is_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn select_story(&mut self, story: Story, cx: &mut ViewContext<Self>) {
        self.selected_story = Some(story.clone());
        self.comments.clear();
        self.is_loading_comments = true;
        cx.notify();

        let client = self.client.clone();
        let task = cx.background_executor().spawn(async move {
            client.fetch_comments(&story).unwrap_or_default()
        });

        cx.spawn(|this: WeakView<Self>, mut cx: AsyncWindowContext| async move {
            let comments = task.await;
            let _ = this.update(&mut cx, |this: &mut Self, cx: &mut ViewContext<Self>| {
                this.comments = comments;
                this.is_loading_comments = false;
                cx.notify();
            });
        })
        .detach();
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
            .child(self.render_sidebar(cx))
            // Story List
            .child(self.render_story_list(cx))
            // Detail Panel
            .child(self.render_detail_panel(cx))
    }
}

impl AppState {
    fn render_sidebar(&self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w(px(56.))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .py_3()
            .bg(theme.bg_secondary)
            .border_r_1()
            .border_color(theme.border_subtle)
            .child(
                div()
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
            // Header
            .child(
                div()
                    .w_full()
                    .h(px(52.))
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(theme.border_subtle)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.selected_channel.name()),
                    ),
            )
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
                            .map(|story| self.render_story_row(story.clone(), cx).into_any_element())
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

    fn render_story_row(&self, story: Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let is_selected = self
            .selected_story
            .as_ref()
            .map(|s| s.id == story.id)
            .unwrap_or(false);

        let bg_color = if is_selected {
            theme.bg_selected
        } else {
            theme.bg_secondary
        };

        let story_clone = story.clone();

        div()
            .id(ElementId::Name(format!("story-{}", story.id).into()))
            .w_full()
            .px_4()
            .py_3()
            .cursor_pointer()
            .bg(bg_color)
            .hover(|s| s.bg(theme.bg_hover))
            .border_b_1()
            .border_color(theme.border_subtle)
            .on_click(cx.listener(move |this, _event, cx| {
                this.select_story(story_clone.clone(), cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    // Title
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(rems(1.4))
                            .child(story.title.clone()),
                    )
                    // Meta row
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .text_color(theme.text_muted)
                            // Score
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_color(theme.accent)
                                    .child("▲")
                                    .child(story.score.to_string()),
                            )
                            // Domain
                            .when_some(story.domain(), |this, domain| {
                                this.child(
                                    div()
                                        .text_color(theme.text_secondary)
                                        .child(domain),
                                )
                            })
                            // Author
                            .child(format!("by {}", story.by))
                            // Time
                            .child(story.formatted_time())
                            // Comments
                            .when(story.comment_count() > 0, |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child("💬")
                                        .child(story.comment_count().to_string()),
                                )
                            }),
                    ),
            )
    }

    fn render_detail_panel(&self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.bg_primary)
            .child(if let Some(story) = &self.selected_story {
                self.render_story_detail(story.clone(), cx).into_any_element()
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

    fn render_story_detail(&self, story: Story, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .id("story-detail")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            // Header
            .child(
                div()
                    .w_full()
                    .p_6()
                    .bg(theme.bg_secondary)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            // Title
                            .child(
                                div()
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
                                    .when_some(story.url.clone(), |this: Div, _url: String| {
                                        this.child(
                                            div()
                                                .text_color(theme.accent)
                                                .child("Open Link"),
                                        )
                                    }),
                            ),
                    ),
            )
            // Story text if available
            .when_some(story.text.clone(), |this: Stateful<Div>, text: String| {
                let theme = &Theme::default();
                let clean_text = html_escape::decode_html_entities(&text).to_string();
                this.child(
                    div()
                        .w_full()
                        .p_6()
                        .text_sm()
                        .line_height(rems(1.6))
                        .text_color(theme.text_primary)
                        .child(clean_text),
                )
            })
            // Comments section
            .child(self.render_comments_section(cx))
    }

    fn render_comments_section(&self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w_full()
            .flex()
            .flex_col()
            .p_6()
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
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(self.comments.iter().map(|c| self.render_comment(c)))
            })
    }

    fn render_comment(&self, comment: &Comment) -> impl IntoElement {
        let theme = &self.theme;

        div()
            .w_full()
            .p_4()
            .rounded_lg()
            .bg(theme.bg_secondary)
            .border_1()
            .border_color(theme.border_subtle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    // Author and time
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text_secondary)
                                    .child(comment.author().to_string()),
                            )
                            .child(
                                div()
                                    .text_color(theme.text_muted)
                                    .child(comment.formatted_time()),
                            ),
                    )
                    // Comment text
                    .child(
                        div()
                            .text_sm()
                            .line_height(rems(1.5))
                            .text_color(theme.text_primary)
                            .child(comment.clean_text()),
                    ),
            )
    }
}

fn main() {
    App::new().run(|cx: &mut AppContext| {
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
