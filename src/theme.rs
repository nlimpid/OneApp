use gpui::{hsla, Hsla};

pub struct Theme {
    pub bg_primary: Hsla,
    pub bg_secondary: Hsla,
    pub bg_tertiary: Hsla,
    pub bg_hover: Hsla,
    pub bg_selected: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub border: Hsla,
    pub border_subtle: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub error: Hsla,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            bg_primary: hsla(0., 0., 0.99, 1.0),
            bg_secondary: hsla(0., 0., 0.96, 1.0),
            bg_tertiary: hsla(0., 0., 0.93, 1.0),
            bg_hover: hsla(0., 0., 0.94, 1.0),
            bg_selected: hsla(32., 1.0, 0.95, 1.0),
            text_primary: hsla(0., 0., 0.1, 1.0),
            text_secondary: hsla(0., 0., 0.35, 1.0),
            text_muted: hsla(0., 0., 0.55, 1.0),
            accent: hsla(24., 1.0, 0.50, 1.0), // HN Orange
            accent_hover: hsla(24., 1.0, 0.45, 1.0),
            border: hsla(0., 0., 0.85, 1.0),
            border_subtle: hsla(0., 0., 0.90, 1.0),
            success: hsla(142., 0.71, 0.45, 1.0),
            warning: hsla(38., 0.92, 0.50, 1.0),
            error: hsla(0., 0.72, 0.51, 1.0),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}
