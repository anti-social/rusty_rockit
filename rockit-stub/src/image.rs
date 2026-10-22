//! Packed images in the pixel formats supported by the fake: NV12, YUYV and RGB888.

use crate::ffi;

pub(crate) const NV12: ffi::rkPIXEL_FORMAT_E = ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP;
pub(crate) const YUYV: ffi::rkPIXEL_FORMAT_E = ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV422_YUYV;
pub(crate) const RGB888: ffi::rkPIXEL_FORMAT_E = ffi::rkPIXEL_FORMAT_E_RK_FMT_RGB888;

pub(crate) fn is_supported(format: ffi::rkPIXEL_FORMAT_E) -> bool {
    matches!(format, NV12 | YUYV | RGB888)
}

/// Size in bytes of a packed image, or `None` for unsupported formats.
pub(crate) fn frame_size(format: ffi::rkPIXEL_FORMAT_E, width: u32, height: u32) -> Option<usize> {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    match format {
        NV12 => Some(w * h + cw * ch * 2),
        YUYV => Some(cw * 2 * h * 2),
        RGB888 => Some(w * h * 3),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Image {
    pub format: ffi::rkPIXEL_FORMAT_E,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub pts: u64,
}

impl Image {
    /// Moving diagonal gradient with a frame counter encoded in the top-left block.
    pub fn test_pattern(
        format: ffi::rkPIXEL_FORMAT_E, width: u32, height: u32, seq: u32, pts: u64,
    ) -> Self {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut data = vec![0u8; frame_size(NV12, width, height).unwrap()];
        let shift = seq as usize * 4;
        let (luma, chroma) = data.split_at_mut(w * h);
        for y in 0..h {
            for x in 0..w {
                luma[y * w + x] = (16 + (x + y + shift) * 219 / (w + h).max(1) % 220) as u8;
            }
        }
        for y in 0..ch {
            for x in 0..cw {
                chroma[(y * cw + x) * 2] = (64 + x * 128 / cw.max(1)) as u8;
                chroma[(y * cw + x) * 2 + 1] = (64 + y * 128 / ch.max(1)) as u8;
            }
        }
        for (i, byte) in seq.to_le_bytes().iter().enumerate() {
            if i < w * h {
                luma[i] = *byte;
            }
        }
        Image { format: NV12, width, height, data, pts }.convert(format)
    }

    /// Copies a possibly strided image out of a buffer.
    pub fn import(
        format: ffi::rkPIXEL_FORMAT_E,
        width: u32,
        height: u32,
        vir_width: u32,
        vir_height: u32,
        buf: &[u8],
        pts: u64,
    ) -> Option<Self> {
        let (w, h) = (width as usize, height as usize);
        let (vw, vh) = (vir_width as usize, vir_height as usize);
        let mut data = Vec::with_capacity(frame_size(format, width, height)?);
        let mut copy_rows = |offset: usize, row_len: usize, stride: usize, rows: usize| {
            for row in 0..rows {
                let start = offset + row * stride;
                data.extend_from_slice(buf.get(start..start + row_len)?);
            }
            Some(())
        };
        match format {
            NV12 => {
                copy_rows(0, w, vw, h)?;
                copy_rows(vw * vh, w.div_ceil(2) * 2, vw.div_ceil(2) * 2, h.div_ceil(2))?;
            }
            YUYV => copy_rows(0, w.div_ceil(2) * 4, vw.div_ceil(2) * 4, h)?,
            RGB888 => copy_rows(0, w * 3, vw * 3, h)?,
            _ => return None,
        }
        Some(Image { format, width, height, data, pts })
    }

    pub fn convert(self, format: ffi::rkPIXEL_FORMAT_E) -> Self {
        if self.format == format {
            return self;
        }
        self.to_nv12().from_nv12(format)
    }

    /// Nearest-neighbour scaling with optional mirror/flip.
    pub fn scale(self, width: u32, height: u32, mirror: bool, flip: bool) -> Self {
        if self.width == width && self.height == height && !mirror && !flip {
            return self;
        }
        let format = self.format;
        let src = self.to_nv12();
        let (sw, sh) = (src.width as usize, src.height as usize);
        let (dw, dh) = (width as usize, height as usize);
        let (scw, dcw, dch) = (sw.div_ceil(2), dw.div_ceil(2), dh.div_ceil(2));
        let map = |d: usize, dn: usize, sn: usize, rev: bool| {
            let s = (d * sn / dn.max(1)).min(sn.saturating_sub(1));
            if rev { sn.saturating_sub(1) - s } else { s }
        };
        let mut data = vec![0u8; frame_size(NV12, width, height).unwrap()];
        let (luma, chroma) = data.split_at_mut(dw * dh);
        let src_chroma = &src.data[sw * sh..];
        for y in 0..dh {
            let sy = map(y, dh, sh, flip);
            for x in 0..dw {
                luma[y * dw + x] = src.data[sy * sw + map(x, dw, sw, mirror)];
            }
        }
        for y in 0..dch {
            let sy = map(y, dch, sh.div_ceil(2), flip);
            for x in 0..dcw {
                let sx = map(x, dcw, scw, mirror);
                chroma[(y * dcw + x) * 2] = src_chroma[(sy * scw + sx) * 2];
                chroma[(y * dcw + x) * 2 + 1] = src_chroma[(sy * scw + sx) * 2 + 1];
            }
        }
        Image { format: NV12, width, height, data, pts: src.pts }.convert(format)
    }

    fn to_nv12(self) -> Self {
        let (w, h) = (self.width as usize, self.height as usize);
        let cw = w.div_ceil(2);
        let mut out = vec![0u8; frame_size(NV12, self.width, self.height).unwrap()];
        let (luma, chroma) = out.split_at_mut(w * h);
        match self.format {
            NV12 => return self,
            YUYV => {
                let row_len = cw * 4;
                for y in 0..h {
                    let row = &self.data[y * row_len..];
                    for x in 0..w {
                        luma[y * w + x] = row[x * 2];
                    }
                    if y % 2 == 0 {
                        for cx in 0..cw {
                            chroma[((y / 2) * cw + cx) * 2] = row[cx * 4 + 1];
                            chroma[((y / 2) * cw + cx) * 2 + 1] = row[cx * 4 + 3];
                        }
                    }
                }
            }
            RGB888 => {
                for y in 0..h {
                    for x in 0..w {
                        let p = (y * w + x) * 3;
                        let (yy, u, v) = rgb_to_yuv(self.data[p], self.data[p + 1], self.data[p + 2]);
                        luma[y * w + x] = yy;
                        if y % 2 == 0 && x % 2 == 0 {
                            chroma[((y / 2) * cw + x / 2) * 2] = u;
                            chroma[((y / 2) * cw + x / 2) * 2 + 1] = v;
                        }
                    }
                }
            }
            _ => unreachable!("unsupported pixel format {}", self.format),
        }
        Image { format: NV12, data: out, ..self }
    }

    fn from_nv12(self, format: ffi::rkPIXEL_FORMAT_E) -> Self {
        let (w, h) = (self.width as usize, self.height as usize);
        let cw = w.div_ceil(2);
        let (luma, chroma) = self.data.split_at(w * h);
        let uv = |x: usize, y: usize| {
            let i = ((y / 2) * cw + x / 2) * 2;
            (chroma[i], chroma[i + 1])
        };
        let data = match format {
            NV12 => return self,
            YUYV => {
                let row_len = cw * 4;
                let mut out = vec![0u8; row_len * h];
                for y in 0..h {
                    for x in 0..w {
                        out[y * row_len + x * 2] = luma[y * w + x];
                    }
                    for cx in 0..cw {
                        let (u, v) = uv(cx * 2, y);
                        out[y * row_len + cx * 4 + 1] = u;
                        out[y * row_len + cx * 4 + 3] = v;
                    }
                }
                out
            }
            RGB888 => {
                let mut out = vec![0u8; w * h * 3];
                for y in 0..h {
                    for x in 0..w {
                        let (u, v) = uv(x, y);
                        let (r, g, b) = yuv_to_rgb(luma[y * w + x], u, v);
                        out[(y * w + x) * 3..][..3].copy_from_slice(&[r, g, b]);
                    }
                }
                out
            }
            _ => unreachable!("unsupported pixel format {format}"),
        };
        Image { format, data, ..self }
    }

    pub fn luma_sample(&self, i: usize, n: usize) -> u8 {
        let len = self.data.len().max(1);
        self.data[(i * len / n.max(1)).min(len - 1)]
    }
}

// BT.601 limited range
fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (clamp(y), clamp(u), clamp(v))
}

fn yuv_to_rgb(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let (c, d, e) = (y as i32 - 16, u as i32 - 128, v as i32 - 128);
    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;
    (clamp(r), clamp(g), clamp(b))
}

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes() {
        assert_eq!(frame_size(NV12, 4, 2), Some(12));
        assert_eq!(frame_size(YUYV, 4, 2), Some(16));
        assert_eq!(frame_size(RGB888, 4, 2), Some(24));
        assert_eq!(frame_size(ffi::rkPIXEL_FORMAT_E_RK_FMT_BGR888, 4, 2), None);
    }

    #[test]
    fn conversions_keep_size_and_round_trip() {
        // Gray gradient: in gamut for RGB, so the round trip only suffers from rounding
        let (w, h) = (16, 8);
        let mut data: Vec<u8> = (0..w * h).map(|i| (16 + i * 219 / (w * h)) as u8).collect();
        data.resize(frame_size(NV12, w, h).unwrap(), 128);
        let src = Image { format: NV12, width: w, height: h, data, pts: 0 };
        for (format, tolerance) in [(YUYV, 0), (RGB888, 2)] {
            let converted = src.clone().convert(format);
            assert_eq!(converted.data.len(), frame_size(format, w, h).unwrap());
            let back = converted.convert(NV12);
            for (a, b) in src.data.iter().zip(&back.data) {
                assert!(a.abs_diff(*b) <= tolerance, "{format}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn scale_changes_dimensions() {
        let src = Image::test_pattern(RGB888, 64, 32, 0, 0);
        let scaled = src.scale(32, 16, true, true);
        assert_eq!((scaled.width, scaled.height, scaled.format), (32, 16, RGB888));
        assert_eq!(scaled.data.len(), 32 * 16 * 3);
    }

    #[test]
    fn import_strided() {
        // 2x2 NV12 frame stored with stride 4 and vir height 2
        let buf = [1, 2, 0, 0, 3, 4, 0, 0, 5, 6, 0, 0];
        let image = Image::import(NV12, 2, 2, 4, 2, &buf, 0).unwrap();
        assert_eq!(image.data, [1, 2, 3, 4, 5, 6]);
        assert!(Image::import(NV12, 2, 2, 4, 2, &buf[..9], 0).is_none());
    }
}
