use core::slice;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

use crate::{Error, ffi, rk_log_err, RockitSys};

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
    _mpi: &'a RockitSys,
    inner: MemBufferPoolInner,
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
    _mpi: RockitSys,
    inner: Rc<MemBufferPoolInner>,
}

impl MemBufferPoolOwned {
    pub fn id(&self) -> u32 {
        self.inner.id
    }

    pub fn get_buffer(&self, size: u32) -> Result<MemBufferOwned, Error> {
        self.inner.get_buffer(size)
            .map(|inner| MemBufferOwned {
                _mpi: self._mpi.clone(),
                _pool: Rc::clone(&self.inner),
                inner: Rc::new(inner),
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
    _pool: &'a MemBufferPool<'a>,
    inner: MemBufferInner,
}

impl<'a> MemBuffer<'a> {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }

    pub fn data_mut(&mut self) -> Result<&mut [u8], Error> {
        self.inner.data_mut()
    }

    pub fn new_frame(&self, width: u16, height: u16) -> MbFrame<'_> {
        MbFrame {
            _buf: self,
            inner: MbFrameInner::new(&self.inner, width, height),
        }
    }
}

pub struct MemBufferOwned {
    _mpi: RockitSys,
    _pool: Rc<MemBufferPoolInner>,
    inner: Rc<MemBufferInner>,
}

impl MemBufferOwned {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }

    pub fn data_mut(&mut self) -> Result<&mut [u8], Error> {
        Rc::get_mut(&mut self.inner).unwrap().data_mut()
    }

    pub fn new_frame(&self, width: u16, height: u16) -> MbFrameOwned {
        MbFrameOwned {
            _mpi: self._mpi.clone(),
            _pool: Rc::clone(&self._pool),
            _buf: Rc::clone(&self.inner),
            inner: MbFrameInner::new(&self.inner, width, height),
        }
    }
}

pub(crate) struct MbFrameInner {
    frame: ffi::rkVIDEO_FRAME_INFO_S,
}

impl MbFrameInner {
    fn new(buf: &MemBufferInner, width: u16, height: u16) -> MbFrameInner {
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

        Self { frame }
    }

    pub(crate) fn frame(&self) -> &ffi::rkVIDEO_FRAME_INFO_S {
        &self.frame
    }

    // pub(crate) fn frame_mut(&mut self) -> &mut ffi::rkVIDEO_FRAME_INFO_S {
    //     &mut self.frame
    // }
}

pub struct MbFrame<'a> {
    _buf: &'a MemBuffer<'a>,
    pub(crate) inner: MbFrameInner,
}

pub struct MbFrameOwned {
    _mpi: RockitSys,
    _pool: Rc<MemBufferPoolInner>,
    _buf: Rc<MemBufferInner>,
    pub(crate) inner: MbFrameInner,
}
