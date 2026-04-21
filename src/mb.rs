use core::slice;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;

use crate::{Error, ffi, rk_log_err, RockitSys};

pub struct MemBufferPool<'a> {
    _mpi: &'a RockitSys,
    id: u32,
}

impl<'a> MemBufferPool<'a> {
    pub(crate) fn new(mpi: &'a RockitSys) -> Result<MemBufferPool<'a>, Error> {
        const MB_INVALID_POOLID: u32 = ffi::MB_INVALID_POOLID as u32;

        let buf_size = 1920 * 1080 * 3 / 2;
        let mut pool_config = ffi::rkMB_POOL_CONFIG_S {
            bNotDelete: false as u32,
            bPreAlloc: true as u32,
            enAllocType: ffi::rkMB_ALLOC_TYPE_MB_ALLOC_TYPE_DMA,
            enRemapMode: ffi::rkMB_REMAP_MODE_E_MB_REMAP_MODE_NONE,
            enDmaType: ffi::rkMB_DMA_TYPE_E_MB_DMA_TYPE_NONE,
            u32MBCnt: 1,
            u64MBSize: buf_size,
        };

        let pool_id = unsafe {
            ffi::RK_MPI_MB_CreatePool(&mut pool_config as *mut _)
        };
        if pool_id == MB_INVALID_POOLID {
            return Err(Error::CreatePool);
        }

        Ok(MemBufferPool { _mpi: mpi, id: pool_id })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn get_buffer(&self, size: u32) -> Result<MemBuffer<'_>, Error> {
        let buf_ptr = unsafe {
            ffi::RK_MPI_MB_GetMB(self.id, size as u64, true as u32)
        };
        if buf_ptr.is_null() {

        }
        Ok(MemBuffer { pool: self, buf_ptr, size: size as _ })
    }
}

impl<'a> Drop for MemBufferPool<'a> {
    fn drop(&mut self) {
        log::debug!("Destroying memory buffer pool: {}", self.id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_MB_DestroyPool(self.id),
                "Error destroying memory buffer pool"
            );
        }
    }
}

pub struct MemBuffer<'a> {
    pool: &'a MemBufferPool<'a>,
    buf_ptr: *mut c_void,
    size: usize,
}

impl<'a> MemBuffer<'a> {
    pub fn data(&self) -> Result<&[u8], Error> {
        let data = unsafe {
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(self.buf_ptr);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts(
                data_ptr as *const u8,
                self.size,
            )
        };
        Ok(data)
    }

    pub fn data_mut(&mut self) -> Result<&mut [u8], Error> {
        let data = unsafe {
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(self.buf_ptr);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts_mut(
                data_ptr as *mut u8,
                self.size,
            )
        };
        Ok(data)
    }

    pub fn new_frame(&self, width: u16, height: u16) -> MbFrame<'_> {
        MbFrame::new(self, width, height)
    }
}

impl<'a> Drop for MemBuffer<'a> {
    fn drop(&mut self) {
        log::trace!("Releasing memory buffer from pool: {}", self.pool.id());
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_MB_ReleaseMB(self.buf_ptr),
                "Error releasing memory buffer"
            );
        }
    }
}

pub struct MbFrame<'a> {
    buf: &'a MemBuffer<'a>,
    frame: ffi::rkVIDEO_FRAME_INFO_S,
}

impl<'a> MbFrame<'a> {
    fn new(buf: &'a MemBuffer<'a>, width: u16, height: u16) -> MbFrame<'a> {
        let width = width as u32;
        let height = height as u32;
        let frame = ffi::rkVIDEO_FRAME_INFO_S {
            stVFrame: ffi::rkVIDEO_FRAME_S {
                pMbBlk: buf.buf_ptr,
                u32Width: width,
                u32Height: height,
                u32VirWidth: width,
                u32VirHeight: height,
                enField: ffi::rkVIDEO_FIELD_E_VIDEO_FIELD_FRAME,
                enPixelFormat: ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
                enVideoFormat: ffi::rkVIDEO_FORMAT_E_VIDEO_FORMAT_LINEAR,
                enCompressMode: ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE,
                enDynamicRange: ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8,
                enColorGamut: ffi::rkCOLOR_GAMUT_E_COLOR_GAMUT_BT601,

                pVirAddr: [ptr::null_mut(); ffi::RK_MAX_COLOR_COMPONENT as usize],

                u32TimeRef: 0,
                u64PTS: 0,

                u64PrivateData: 0,
                u32FrameFlag: 0,
            },
        };

        Self {
            buf,
            frame,
        }
    }

    pub(crate) fn frame(&mut self) -> &mut ffi::rkVIDEO_FRAME_INFO_S {
        &mut self.frame
    }
}
