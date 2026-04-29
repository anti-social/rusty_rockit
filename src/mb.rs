use core::slice;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

use crate::{Error, PixelFormat, RockitSys, ffi, rk_log_err};

struct MemBufferPoolInner {
    id: u32,
}

impl Drop for MemBufferPoolInner {
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

impl MemBufferPoolInner {
    pub fn get_buffer(&self, size: u32) -> Result<MemBufferInner, Error> {
        let buf_ptr = unsafe {
            ffi::RK_MPI_MB_GetMB(self.id, size as u64, true as u32)
        };
        if buf_ptr.is_null() {
            return Err(Error::GetBuffer);
        }
        Ok(MemBufferInner { buf_ptr, size: size as _, pool_id: self.id })
    }
}

pub struct MemBufferPool<'a> {
    inner: MemBufferPoolInner,
    _mpi: &'a RockitSys,
}

impl<'a> MemBufferPool<'a> {
    const MB_INVALID_POOLID: u32 = ffi::MB_INVALID_POOLID as u32;

    pub(crate) fn new(
        mpi: &'a RockitSys, buf_size: u32,
    ) -> Result<MemBufferPool<'a>, Error> {
        let mut pool_config = ffi::rkMB_POOL_CONFIG_S {
            bNotDelete: false as u32,
            bPreAlloc: true as u32,
            enAllocType: ffi::rkMB_ALLOC_TYPE_MB_ALLOC_TYPE_DMA,
            enRemapMode: ffi::rkMB_REMAP_MODE_E_MB_REMAP_MODE_NONE,
            enDmaType: ffi::rkMB_DMA_TYPE_E_MB_DMA_TYPE_NONE,
            u32MBCnt: 1,
            u64MBSize: buf_size as u64,
        };

        let pool_id = unsafe {
            ffi::RK_MPI_MB_CreatePool(&mut pool_config as *mut _)
        };
        if pool_id == Self::MB_INVALID_POOLID {
            return Err(Error::CreatePool);
        }

        Ok(MemBufferPool {
            _mpi: mpi,
            inner: MemBufferPoolInner { id: pool_id },
        })
    }

    pub fn id(&self) -> u32 {
        self.inner.id
    }

    pub fn get_buffer(&self, size: u32) -> Result<MemBuffer<'_>, Error> {
        self.inner.get_buffer(size)
            .map(|inner| MemBuffer { _pool: self, inner })
    }

    pub fn into_owned(self) -> MemBufferPoolOwned {
        MemBufferPoolOwned {
            _mpi: self._mpi.clone(),
            inner: Rc::new(self.inner),
        }
    }
}

pub struct MemBufferPoolOwned {
    inner: Rc<MemBufferPoolInner>,
    _mpi: RockitSys,
}

impl MemBufferPoolOwned {
    pub fn id(&self) -> u32 {
        self.inner.id
    }

    pub fn get_buffer(&self, size: u32) -> Result<MemBufferOwned, Error> {
        self.inner.get_buffer(size)
            .map(|inner| MemBufferOwned {
                inner: inner,
                _pool: Rc::clone(&self.inner),
                _mpi: self._mpi.clone(),
            })
    }
}

pub struct MemBufferInner {
    buf_ptr: *mut c_void,
    size: usize,
    pool_id: u32,
}

impl Drop for MemBufferInner {
    fn drop(&mut self) {
        log::trace!("Releasing memory buffer from pool: {}", self.pool_id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_MB_ReleaseMB(self.buf_ptr),
                "Error releasing memory buffer"
            );
        }
    }
}

impl MemBufferInner {
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
}

pub struct MemBuffer<'a> {
    inner: MemBufferInner,
    _pool: &'a MemBufferPool<'a>,
}

impl<'a> MemBuffer<'a> {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }

    pub fn data_mut(&mut self) -> Result<&mut [u8], Error> {
        self.inner.data_mut()
    }

    pub fn new_frame(
        &self, pixel_format: PixelFormat, width: u16, height: u16
    ) -> MbFrame<'_> {
        MbFrame {
            _buf: self,
            inner: MbFrameInner::new(&self.inner, pixel_format, width, height),
        }
    }
}

pub struct MemBufferOwned {
    inner: MemBufferInner,
    _pool: Rc<MemBufferPoolInner>,
    _mpi: RockitSys,
}

impl MemBufferOwned {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }

    pub fn data_mut(&mut self) -> Result<&mut [u8], Error> {
        self.inner.data_mut()
    }

    pub fn new_frame(
        &self, pixel_format: PixelFormat, width: u16, height: u16
    ) -> MbFrameOwned<'_> {
        MbFrameOwned {
            _mpi: self._mpi.clone(),
            _pool: Rc::clone(&self._pool),
            _buf: &self.inner,
            inner: MbFrameInner::new(&self.inner, pixel_format, width, height),
        }
    }
}

pub(crate) struct MbFrameInner {
    frame: ffi::rkVIDEO_FRAME_INFO_S,
}

impl MbFrameInner {
    fn new(
        buf: &MemBufferInner, pixel_format: PixelFormat, width: u16, height: u16
    ) -> MbFrameInner {
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
                enPixelFormat: pixel_format.native_format(),
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

        Self { frame }
    }

    pub(crate) fn frame(&self) -> &ffi::rkVIDEO_FRAME_INFO_S {
        &self.frame
    }
}

pub struct MbFrame<'a> {
    pub(crate) inner: MbFrameInner,
    _buf: &'a MemBuffer<'a>,
}

pub struct MbFrameOwned<'a> {
    pub(crate) inner: MbFrameInner,
    _buf: &'a MemBufferInner,
    _pool: Rc<MemBufferPoolInner>,
    _mpi: RockitSys,
}
