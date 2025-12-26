use crate::{reader, reader_view, theme::Theme};
use gpui::prelude::*;
use gpui::{div, point, px, size, ScrollDelta, ScrollHandle, ScrollWheelEvent, TestAppContext};

#[gpui::test]
fn code_block_does_not_trap_vertical_scroll(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();

    let theme = Theme::default();
    let outer_scroll = ScrollHandle::new();

    let code_text = (0..120)
        .map(|i| format!("fn line_{i}() {{ println!(\"{i}\"); }}"))
        .collect::<Vec<_>>()
        .join("\n");

    let blocks = {
        let mut blocks = Vec::new();
        blocks.push(reader::ReaderBlock::Code {
            text: code_text,
            language: Some("rust".into()),
        });
        blocks.extend((0..40).map(|i| {
            reader::ReaderBlock::Paragraph(format!(
                "Paragraph {i}: This is filler text to force vertical scrolling."
            ))
        }));
        blocks
    };

    cx.draw(point(px(0.), px(0.)), size(px(420.), px(320.)), |_| {
        div()
            .id("outer-scroll")
            .w_full()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&outer_scroll)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(
                        blocks
                            .iter()
                            .map(|block| reader_view::render_reader_block(&theme, block))
                            .collect::<Vec<_>>(),
                    ),
            )
    });

    assert_eq!(outer_scroll.offset().y, px(0.));

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(12.), px(12.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
        ..Default::default()
    });

    assert!(
        outer_scroll.offset().y < px(0.),
        "expected outer container to scroll when the cursor is over a code block"
    );
}

#[gpui::test]
fn reader_nested_flex_layout_allows_scrolling(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();

    let theme = Theme::default();
    let scroll = ScrollHandle::new();

    let blocks = (0..80)
        .map(|i| {
            reader::ReaderBlock::Paragraph(format!(
                "Paragraph {i}: Long content to exceed viewport height and verify scrolling."
            ))
        })
        .collect::<Vec<_>>();

    cx.draw(point(px(0.), px(0.)), size(px(520.), px(420.)), |_| {
        // Approximate the real app layout:
        // detail panel (flex col, overflow hidden) -> reader page (flex col, overflow hidden)
        // -> header (fixed) + article scroll (flex_1, overflow_y_scroll)
        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(div().h(px(60.)).w_full())
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(div().h(px(56.)).w_full())
                    .child(
                        div()
                            .id("article-scroll")
                            .flex_1()
                            .w_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
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
                                        .children(
                                            blocks
                                                .iter()
                                                .map(|b| reader_view::render_reader_block(&theme, b))
                                                .collect::<Vec<_>>(),
                                        ),
                                ),
                            ),
                    ),
            )
    });

    assert_eq!(scroll.offset().y, px(0.));

    cx.simulate_event(ScrollWheelEvent {
        position: point(px(16.), px(120.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-320.))),
        ..Default::default()
    });

    assert!(
        scroll.offset().y < px(0.),
        "expected nested flex scroll container to scroll"
    );
}
