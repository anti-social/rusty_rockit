use std::time::Duration;

use crate::mb::MemBufferOwned;
use crate::{Error, RockitSys};
use crate::venc::{self, StreamFrame, VencChannelOwned, VencChannelBindOwned, VencConfig, VencStreamOwned};


pub struct SimpleEncoder {
    enc: VencChannelOwned<venc::state::Started>,
    enc_frame: StreamFrame,
    mem_buf: MemBufferOwned,
    width: u16,
    height: u16,
    frame_buf_size: usize,
}

impl SimpleEncoder {
    pub fn new(
        mpi: &RockitSys, encoder_id: u8, config: &VencConfig
    ) -> Result<Self, Error> {
        let enc_channel = mpi.encoder(encoder_id, config)?.into_owned();
        let enc_channel = enc_channel.start()?;
        let buffer_pool = mpi.pool()?.into_owned();
        let frame_buf_size = config.width as u32 * config.height as u32 * 3 / 2;
        let mem_buf = buffer_pool.get_buffer(frame_buf_size)?;

        Ok(Self {
            enc: enc_channel,
            enc_frame: StreamFrame::new(),
            mem_buf,
            width: config.width,
            height: config.height,
            frame_buf_size: frame_buf_size as usize,
        })
    }

    pub fn frame_buf_size(&self) -> usize {
        self.frame_buf_size
    }
    
    pub fn encode_frame(
        &mut self, frame_buf: &[u8], timeout: Duration
    ) -> Result<VencStreamOwned<'_>, Error> {
        let data = self.mem_buf.data_mut()?;
        data.copy_from_slice(frame_buf);
        let mut frame = self.mem_buf.new_frame(self.width, self.height);
        self.enc.send_frame(&mut frame, timeout)?;

        self.enc.get_stream(&mut self.enc_frame, timeout)
    }
}

pub struct CameraEncoder {
    enc: VencChannelBindOwned,
    frame: StreamFrame,
}

impl CameraEncoder {
    pub fn new(
        mpi: &RockitSys,
        camera_id: u8,
        encoder_id: u8,
        config: &VencConfig,
    ) -> Result<Self, Error> {
        let pipe_id = 0;
        let camera_channel_id = 0;

        let cam = mpi.camera(camera_id, camera_id)?.into_owned();

        let pipe = cam.get_pipe(pipe_id)
            .ok_or(Error::InvalidPipeId { id: pipe_id })?;
        let channel = pipe.create_channel(
            camera_channel_id, config.width, config.height
        )?;

        let enc_channel = mpi.encoder(encoder_id, &config)?.into_owned();
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
