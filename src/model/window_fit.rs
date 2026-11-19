//! The window size that shows a video without bars.

pub(crate) const MIN_WIDTH: f32 = 480.0;

const MONITOR_SHARE: f32 = 0.9;

// Preserve the longer side unless the monitor's 90% bound requires a smaller window.
pub(crate) fn fitted_window(
    window: (f32, f32),
    video: (u32, u32),
    monitor: Option<(f32, f32)>,
) -> Option<(f32, f32)> {
    let (video_width, video_height) = (video.0 as f32, video.1 as f32);
    if video_width <= 0.0 || video_height <= 0.0 || window.0 <= 0.0 || window.1 <= 0.0 {
        return None;
    }
    let aspect = video_width / video_height;
    let longer = window.0.max(window.1);
    let (mut width, mut height) = if aspect >= 1.0 {
        (longer, longer / aspect)
    } else {
        (longer * aspect, longer)
    };
    if width < MIN_WIDTH {
        height *= MIN_WIDTH / width;
        width = MIN_WIDTH;
    }
    // Staying on screen wins over the controls' width.
    if let Some((monitor_width, monitor_height)) = monitor {
        let scale = (monitor_width * MONITOR_SHARE / width)
            .min(monitor_height * MONITOR_SHARE / height)
            .min(1.0);
        width *= scale;
        height *= scale;
    }
    let (width, height) = (width.round(), height.round());
    ((width - window.0).abs() >= 1.0 || (height - window.1).abs() >= 1.0).then_some((width, height))
}
