//! Small pictures of where each recent video was left, for the start page.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Thumbnail {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) const WIDTH: u32 = 192;

// A 2 × 2 sample grid bounds downsampling cost per output pixel.
pub(crate) fn shrink(width: u32, height: u32, pixels: &[u8]) -> Option<Thumbnail> {
    let required = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if width == 0 || height == 0 || pixels.len() < required {
        return None;
    }
    let longest = width.max(height);
    let limit = WIDTH.min(longest);
    let out_w = ((u64::from(width) * u64::from(limit)) / u64::from(longest)).max(1) as u32;
    let out_h = ((u64::from(height) * u64::from(limit)) / u64::from(longest)).max(1) as u32;
    let mut out = Vec::with_capacity((out_w * out_h * 4) as usize);
    for y in 0..out_h {
        for x in 0..out_w {
            let mut sum = [0_u32; 4];
            for (dx, dy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                let sx = ((u64::from(x) * 4 + dx) * u64::from(width) / (u64::from(out_w) * 4))
                    .min(u64::from(width) - 1);
                let sy = ((u64::from(y) * 4 + dy) * u64::from(height) / (u64::from(out_h) * 4))
                    .min(u64::from(height) - 1);
                let at = ((sy * u64::from(width) + sx) * 4) as usize;
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += u32::from(pixels[at + channel]);
                }
            }
            out.extend(sum.map(|total| (total / 4) as u8));
        }
    }
    Some(Thumbnail {
        width: out_w,
        height: out_h,
        pixels: out,
    })
}
