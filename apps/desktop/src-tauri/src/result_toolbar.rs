use screentrans_core::Rect;

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolbarState {
    pub above: bool,
    #[serde(skip)]
    pub preferred_above: bool,
    pub align_left: bool,
    pub detached: bool,
    #[serde(skip)]
    pub quiet: bool,
    #[serde(skip)]
    pub expected: Option<(i32, i32)>,
    #[serde(skip)]
    pub generation: u64,
    #[serde(skip)]
    pub image_moving: bool,
    #[serde(skip)]
    pub image_resizing: bool,
}

impl ToolbarState {
    pub fn suppressed(&self) -> bool {
        self.quiet || self.image_moving || self.image_resizing
    }
    pub fn finish_image_gesture(&mut self) {
        self.image_moving = false;
        self.image_resizing = false;
        self.detached = false;
    }
}

/// The toolbar is its own OS window, so this placement cannot clip at image edges.
pub fn placement(image: Rect, scale: f64, state: &ToolbarState, area: Rect) -> Rect {
    let width = 248.0 * scale;
    let height = 48.0 * scale;
    let gap = 4.0 * scale;
    let above = image.y - height - gap >= area.y;
    let below = image.bottom() + gap + height <= area.bottom();
    let y = if (state.preferred_above || !below) && above {
        image.y - height - gap
    } else if below {
        image.bottom() + gap
    } else {
        image.bottom() - height - gap
    };
    let x = if state.align_left {
        image.x
    } else {
        image.right() - width
    };
    clamp(
        Rect {
            x,
            y,
            width,
            height,
        },
        area,
    )
}
pub fn clamp(mut bounds: Rect, area: Rect) -> Rect {
    bounds.x = bounds
        .x
        .clamp(area.x, (area.right() - bounds.width).max(area.x));
    bounds.y = bounds
        .y
        .clamp(area.y, (area.bottom() - bounds.height).max(area.y));
    bounds
}
#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultDisplay {
    pub frame_id: String,
    pub original: bool,
    pub comparing: bool,
    pub holding_original: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_edge_avoidance_does_not_become_a_permanent_top_preference() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let edge = Rect {
            x: 500.0,
            y: 800.0,
            width: 600.0,
            height: 220.0,
        };
        let center = Rect { y: 300.0, ..edge };
        let mut state = ToolbarState::default();
        let top = placement(edge, 1.0, &state, area);
        assert!(top.bottom() <= edge.y);
        state.above = true;
        let bottom = placement(center, 1.0, &state, area);
        assert!(bottom.y >= center.bottom());
        state.preferred_above = true;
        assert!(placement(center, 1.0, &state, area).bottom() <= center.y);
    }
    #[test]
    fn image_gestures_suppress_freely_placed_tools_and_restore_docking_without_clearing_notices() {
        let mut state = ToolbarState {
            detached: true,
            image_moving: true,
            ..Default::default()
        };
        assert!(state.suppressed());
        state.finish_image_gesture();
        assert!(!state.suppressed() && !state.detached);
        state.image_resizing = true;
        state.quiet = true;
        state.finish_image_gesture();
        assert!(state.suppressed() && state.quiet);
    }
    #[test]
    fn toolbar_stays_on_screen_at_every_corner_and_mixed_dpi() {
        let area = Rect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        for scale in [1.0, 1.5, 2.0] {
            for (x, y) in [
                (-1920.0, 0.0),
                (-100.0, 0.0),
                (-1920.0, 1020.0),
                (-100.0, 1020.0),
            ] {
                let image = Rect {
                    x,
                    y,
                    width: 80.0,
                    height: 20.0,
                };
                let bar = placement(image, scale, &ToolbarState::default(), area);
                assert!(bar.x >= area.x && bar.y >= area.y);
                assert!(bar.right() <= area.right() && bar.bottom() <= area.bottom());
            }
        }
    }
    #[test]
    fn a_fullscreen_capture_keeps_the_toolbar_visible_inside_it() {
        let image = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let bar = placement(image, 1.0, &ToolbarState::default(), image);
        assert!(bar.bottom() <= image.bottom() && bar.y >= image.y);
    }
    #[test]
    fn detached_toolbar_can_be_clamped_without_moving_the_image() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let moved = Rect {
            x: 770.0,
            y: 590.0,
            width: 248.0,
            height: 48.0,
        };
        let result = clamp(moved, area);
        assert_eq!((result.x, result.y), (552.0, 552.0));
    }
}
