use screentrans_core::config::Theme;

/// A stale autosave must never repaint over the user's newer theme preview.
#[derive(Default)]
pub struct ThemePreview {
    desired: Option<Theme>,
}
impl ThemePreview {
    pub fn effective(&self, saved: Theme) -> Theme {
        self.desired.unwrap_or(saved)
    }
    pub fn preview(&mut self, saved: Theme, requested: Theme) -> bool {
        let changed = self.desired.unwrap_or(saved) != requested;
        self.desired = Some(requested);
        changed
    }
    pub fn commit(&mut self, previous: Theme, saved: Theme) -> bool {
        match self.desired {
            Some(preview) if preview == saved => {
                self.desired = None;
                false
            }
            Some(_) => false,
            None => previous != saved,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_older_save_does_not_overwrite_a_newer_preview() {
        let mut preview = ThemePreview::default();
        assert!(preview.preview(Theme::Light, Theme::Dark));
        assert!(preview.preview(Theme::Light, Theme::Light));
        assert!(!preview.commit(Theme::Light, Theme::Dark));
        assert_eq!(preview.effective(Theme::Dark), Theme::Light);
        assert!(!preview.commit(Theme::Dark, Theme::Light));
        assert!(preview.commit(Theme::Light, Theme::Dark));
    }
    #[test]
    fn initialization_and_matching_save_do_not_repeat_the_native_theme_change() {
        let mut preview = ThemePreview::default();
        assert!(!preview.preview(Theme::System, Theme::System));
        assert!(!preview.commit(Theme::System, Theme::System));
    }
}
