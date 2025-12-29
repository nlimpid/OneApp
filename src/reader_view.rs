use crate::{reader, theme::Theme};
use gpui::prelude::*;
use gpui::{div, img, px, rems, AnyElement, FontWeight, ObjectFit};

pub(crate) fn render_reader_block(theme: &Theme, block: &reader::ReaderBlock) -> AnyElement {
    match block {
        reader::ReaderBlock::Heading { level, text } => {
            let base = div()
                .w_full()
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(rems(1.25))
                .whitespace_normal()
                .child(text.clone());

            match level {
                1 => base.text_xl().into_any_element(),
                2 => base.text_lg().into_any_element(),
                3 => base.text_base().into_any_element(),
                _ => base
                    .text_base()
                    .text_color(theme.text_secondary)
                    .into_any_element(),
            }
        }
        reader::ReaderBlock::Paragraph(text) => div()
            .w_full()
            .text_base()
            .line_height(rems(1.75))
            .text_color(theme.text_primary)
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        reader::ReaderBlock::Quote(text) => div()
            .w_full()
            .pl_4()
            .pr_4()
            .py_3()
            .bg(theme.bg_secondary)
            .rounded_md()
            .border_l_2()
            .border_color(theme.border)
            .text_base()
            .line_height(rems(1.7))
            .text_color(theme.text_secondary)
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        reader::ReaderBlock::List { ordered, items } => div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let marker = if *ordered {
                            format!("{}.", i + 1)
                        } else {
                            "•".to_string()
                        };

                        div()
                            .w_full()
                            .flex()
                            .items_start()
                            .gap_3()
                            .child(div().w(px(28.)).text_color(theme.text_muted).child(marker))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_base()
                                    .line_height(rems(1.7))
                                    .text_color(theme.text_primary)
                                    .whitespace_normal()
                                    .child(item.clone()),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element(),
        reader::ReaderBlock::Code { text, language } => {
            let mut container = div()
                .w_full()
                .min_w(px(0.))
                .bg(theme.bg_secondary)
                .rounded_md()
                .border_1()
                .border_color(theme.border_subtle)
                .overflow_hidden();

            if let Some(language) = language.clone().filter(|l| !l.is_empty()) {
                container = container.child(
                    div()
                        .w_full()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(theme.border_subtle)
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(language),
                );
            }

            container
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.))
                        .px_4()
                        .py_3()
                        .font_family("Menlo")
                        .text_sm()
                        .line_height(rems(1.55))
                        .text_color(theme.text_primary)
                        .whitespace_normal()
                        .overflow_x_hidden()
                        .child(text.clone()),
                )
                .into_any_element()
        }
        reader::ReaderBlock::Image { url, alt, caption } => {
            let caption = caption
                .clone()
                .or_else(|| alt.clone())
                .filter(|s| !s.is_empty());

            let mut container = div().w_full().flex().flex_col().gap_2().child(
                img(url.clone())
                    .w_full()
                    .max_h(px(520.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border_subtle)
                    .object_fit(ObjectFit::Contain),
            );

            if let Some(caption) = caption {
                container = container.child(
                    div()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .whitespace_normal()
                        .child(caption),
                );
            }

            container.into_any_element()
        }
        reader::ReaderBlock::Rule => div()
            .w_full()
            .h(px(1.))
            .bg(theme.border_subtle)
            .into_any_element(),
    }
}

