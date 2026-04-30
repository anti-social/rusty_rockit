use std::time::Duration;

use crate::vi::CameraId;
use crate::vpss::{FrameRateControl, VpssChannelConfig, VpssGroupConfig};
use crate::{Error, PixelFormat, RockitMpi};
use crate::mb::MemBufferPoolOwned;
use crate::venc::{
    self, StreamFrame, VencChannelBindOwned, VencChannelOwned, VencConfig, VencStreamOwned,
    VpssVencBindOwned,
};


pub struct SimpleEncoder {
    enc: VencChannelOwned<venc::state::Started>,
    vpss_venc_bind: Option<VpssVencBindOwned>,
    enc_frame: StreamFrame,
    buffer_pool: MemBufferPoolOwned,
    pixel_format: PixelFormat,
    width: u16,
    height: u16,
    frame_buf_size: usize,
}

impl SimpleEncoder {
    pub fn new(
        mpi: &RockitMpi, config: &VencConfig
    ) -> Result<Self, Error> {
        let buffer_size = config.calc_buffer_size();
        log::debug!("Input buffer size: {buffer_size}");

        let pixel_format = config.pixel_format;
        let mut config = config.clone();
        config.pixel_format = PixelFormat::Nv12;

        let enc_channel = mpi.venc_channel(&config)?.into_owned();
        let enc_channel = enc_channel.start()?;

        let buffer_pool = mpi.pool(buffer_size)?.into_owned();

        let vpss_venc_bind = if !matches!(pixel_format, PixelFormat::Nv12) {
            let vpss_config = VpssGroupConfig {
                pixel_format: pixel_format,
                max_width: config.width,
                max_height: config.height,
                frame_rate: FrameRateControl {
                    src: config.codec.framerate(),
                    dst: config.codec.framerate(),
                },
            };
            let vpss_group = mpi.vpss_group(&vpss_config)?.into_owned();
            let vpss_channel_config = VpssChannelConfig {
                pixel_format: PixelFormat::Nv12,
                width: config.width,
                height: config.height,
                frame_rate: FrameRateControl {
                    src: config.codec.framerate(),
                    dst: config.codec.framerate(),
                },
                mirror: false,
                flip: false,
                queue_size: 0,
                frame_buffer_count: 2,
            };
            let vpss_group = vpss_group.start()?;
            let vpss_channel = vpss_group.channel(&vpss_channel_config)?;
            let vpss_channel = vpss_channel.enable()?;
            Some(enc_channel.bind_vpss(&vpss_channel)?)
        } else {
            None
        };

        Ok(Self {
            pixel_format,
            enc: enc_channel,
            vpss_venc_bind,
            enc_frame: StreamFrame::new(),
            buffer_pool,
            // mem_buf,
            width: config.width,
            height: config.height,
            frame_buf_size: buffer_size as usize,
        })
    }

    pub fn frame_buf_size(&self) -> usize {
        self.frame_buf_size
    }
    
    pub fn encode_frame(
        &mut self, frame_buf: &[u8], timeout: Duration
    ) -> Result<VencStreamOwned<'_>, Error> {
        let mut mem_buf = self.buffer_pool.get_buffer(self.frame_buf_size as u32)?;
        let data = mem_buf.data_mut()?;
        data.copy_from_slice(frame_buf);
        let mut frame = mem_buf.new_frame(
            self.pixel_format, self.width, self.height
        );
        if let Some(ref vpss_venc_bind) = self.vpss_venc_bind {
            vpss_venc_bind.send_frame(0, &mut frame, timeout)?;
        } else {
            self.enc.send_frame(&mut frame, timeout)?;
        }

        self.enc.get_stream(&mut self.enc_frame, timeout)
    }
}

pub struct CameraEncoder {
    enc: VencChannelBindOwned,
    frame: StreamFrame,
}

impl CameraEncoder {
    pub fn new(
        mpi: &RockitMpi,
        camera_id: CameraId,
        config: &VencConfig,
    ) -> Result<Self, Error> {
        let pipe_id = 0;
        let camera_channel_id = 0;

        let cam = mpi.camera(camera_id, 1)?.into_owned();

        let pipe = cam.get_pipe(pipe_id)
            .ok_or(Error::InvalidPipeId { id: pipe_id })?;
        let channel = pipe.create_channel(
            camera_channel_id, config.width, config.height
        )?;

        let enc_channel = mpi.venc_channel(&config)?.into_owned();
        let enc_channel = enc_channel.start()?;
        let bind = enc_channel.bind(&channel)?;

        Ok(Self { enc: bind, frame: StreamFrame::new() })
    }

    pub fn get_frame(
        &mut self, timeout: Duration
    ) -> Result<VencStreamOwned<'_>, Error> {
        self.enc.get_stream(&mut self.frame, timeout)
    }
}
