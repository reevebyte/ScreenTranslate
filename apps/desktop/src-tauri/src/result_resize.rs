use screentrans_core::Rect;

#[derive(Clone, Copy, serde::Deserialize)]
pub enum ResizeDirection {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}
#[derive(Clone)]
pub struct ResizeGesture {
    #[cfg(windows)]
    pub outline: std::sync::Arc<crate::resize_outline::ResizeOutline>,
    pub frame_id: String,
    pub bounds: Rect,
    pub current: Rect,
    pub cursor: (f64, f64),
    pub direction: ResizeDirection,
    pub scale: f64,
    pub layout: crate::windows::ResultLayout,
}

/// Each edge changes only its axis. The opposite edge remains anchored.
pub fn resize(bounds: Rect, direction: ResizeDirection, dx: f64, dy: f64, scale: f64) -> Rect {
    use ResizeDirection::*;
    let (horizontal, vertical) = match direction {
        NorthWest => (-1.0, -1.0),
        North => (0.0, -1.0),
        NorthEast => (1.0, -1.0),
        East => (1.0, 0.0),
        SouthEast => (1.0, 1.0),
        South => (0.0, 1.0),
        SouthWest => (-1.0, 1.0),
        West => (-1.0, 0.0),
    };
    let width = if horizontal == 0.0 {
        bounds.width
    } else {
        (bounds.width + horizontal * dx).clamp((32.0 * scale).min(bounds.width), 16000.0)
    };
    let height = if vertical == 0.0 {
        bounds.height
    } else {
        (bounds.height + vertical * dy).clamp((12.0 * scale).min(bounds.height), 16000.0)
    };
    Rect {
        x: if horizontal < 0.0 {
            bounds.right() - width
        } else {
            bounds.x
        },
        y: if vertical < 0.0 {
            bounds.bottom() - height
        } else {
            bounds.y
        },
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> Rect {
        Rect {
            x: -600.0,
            y: 100.0,
            width: 400.0,
            height: 200.0,
        }
    }
    #[test]
    fn side_handles_leave_the_other_axis_unchanged() {
        use ResizeDirection::*;
        for direction in [East, West] {
            let result = resize(image(), direction, 80.0, 99.0, 1.5);
            assert_eq!((result.y, result.height), (100.0, 200.0));
        }
        for direction in [North, South] {
            let result = resize(image(), direction, 99.0, 40.0, 1.5);
            assert_eq!((result.x, result.width), (-600.0, 400.0));
        }
    }
    #[test]
    fn corners_change_both_axes_independently_and_keep_the_opposite_corner() {
        let result = resize(image(), ResizeDirection::NorthWest, -80.0, -10.0, 1.0);
        assert_eq!((result.width, result.height), (480.0, 210.0));
        assert_eq!(
            (result.right(), result.bottom()),
            (image().right(), image().bottom())
        );
    }
    #[test]
    fn grabbing_a_thin_capture_does_not_enlarge_it() {
        let image = Rect {
            x: -200.0,
            y: 100.0,
            width: 8.0,
            height: 3.0,
        };
        let result = resize(image, ResizeDirection::SouthEast, 0.0, 0.0, 2.0);
        assert_eq!(
            (result.width, result.height, result.x, result.y),
            (8.0, 3.0, -200.0, 100.0)
        );
    }
}
