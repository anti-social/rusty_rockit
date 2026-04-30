use std::cell::OnceCell;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

pub use rockit_sys::mpi as ffi;
use snafu::Snafu;

#[cfg(feature = "aiq")]
pub mod aiq;
pub mod encoder;
pub use encoder::{CameraEncoder, SimpleEncoder};
pub mod mb;
use mb::MemBufferPool;
pub mod venc;
use venc::{VencChannel, VencChannelResourceManager, VencConfig};
pub mod vi;
use vi::{Camera, CameraId, ViCameraResourceManager};
use vpss::{VpssGroup, VpssGroupConfig, VpssGroupResourceManager};
pub mod vpss;

const RK_SUCCESS: i32 = ffi::RK_SUCCESS as i32;
const RK_ERR_APPID: u32 = 0x80000000 + 0x20000000;
const RK_ERR_VI_NOT_CONFIG: i32 = rk_def_err(
    ffi::rkMOD_ID_E_RK_ID_VI as i32,
    ffi::rkERR_LEVEL_E_RK_ERR_LEVEL_ERROR as i32,
    ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG as i32,
);

static MPI_SYS_INIT: Mutex<OnceCell<()>> = Mutex::new(OnceCell::new());

const fn rk_def_err(module: i32, level: i32, errid: i32) -> i32 {
    RK_ERR_APPID as i32 | ((module) << 16 ) | ((level) << 13) | (errid)
}

#[derive(Clone, Debug)]
pub struct RockitErr {
    pub id: ffi::rkEN_ERR_CODE_E,
    pub level: ffi::rkERR_LEVEL_E,
    pub module: ffi::rkMOD_ID_E,
}

impl RockitErr {
    pub const fn from_code(code: i32) -> Self {
        let id = (code & 0b0001_1111_1111_1111) as u32;
        let level = ((code >> 13) & 0b0111) as u32;
        let module = ((code >> 3) & 0b1111_1111) as u32;
        Self { id, level, module }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Snafu)]
pub enum Error {
    #[snafu(display("MPI is already initialized"))]
    MpiAlreadyInitialized,
    #[snafu(display("Resource unavailable {name}: {id:?}"))]
    ResourceUnavailable { name: String, id: Option<usize> },
    #[snafu(display("Invalid camera id: {id}"))]
    InvalidCameraId { id: u8 },
    #[snafu(display("Invalid device id: {id}"))]
    InvalidDevId { id: u8 },
    #[snafu(display("Invalid pipe id: {id}"))]
    InvalidPipeId { id: u8 },
    #[snafu(display("Requested too many pipes: {num}"))]
    RequestedTooManyPipes { num: u8 },
    #[snafu(display("Invalid channel id: {id}"))]
    InvalidChannelId { id: u8 },
    #[snafu(display("Invalid frame pointer"))]
    InvalidFramePointer,
    #[snafu(display("Invalid pool id"))]
    CreatePool,
    #[snafu(display("Cannot get buffer"))]
    GetBuffer,
    #[snafu(display("MPI lock is poisoned"))]
    LockPoisoned,
    #[snafu(display("Rockit error code: {err:?}"))]
    Rockit { err: RockitErr }
}

#[macro_export]
macro_rules! rk_check_err {
    ($fn:expr) => {
        let ret_code = $fn;
        if ret_code != crate::RK_SUCCESS {
            return Err(crate::Error::Rockit { err: crate::RockitErr::from_code(ret_code) });
        }
    };
}

#[macro_export]
macro_rules! rk_log_err {
    ($fn:expr, $msg:literal) => {
        let ret_code = $fn;
        if ret_code != crate::RK_SUCCESS {
            log::error!("{}: {}", $msg, ret_code);
        }
    };
}

#[derive(Clone)]
pub(crate) struct ResourceManager<const N: usize> {
    name: String,
    state: Arc<Mutex<[AtomicBool; N]>>,
}

impl<const N: usize> ResourceManager<N> {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name: name.to_string(),
            state: Arc::new(Mutex::new([const { AtomicBool::new(false) }; N])),
        }
    }

    pub(crate) fn acqure(&self) -> Result<AcquiredResource<N>, Error> {
        let slots = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        for (id, slot) in slots.iter().enumerate() {
            if slot.load(Ordering::Relaxed) {
                continue;
            }
            slot.store(true, Ordering::Relaxed);
            return Ok(AcquiredResource { id, resource: self.clone() });
        }
        Err(Error::ResourceUnavailable { name: self.name.clone(), id: None })
    }

    pub(crate) fn acqure_specific(&self, id: usize) -> Result<AcquiredResource<N>, Error> {
        let slot = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if slot[id].load(Ordering::Relaxed) {
            return Err(Error::ResourceUnavailable { name: self.name.clone(), id: Some(id) });
        }
        slot[id].store(true, Ordering::Relaxed);
        Ok(AcquiredResource { id, resource: self.clone() })
    }
    
    pub(crate) fn release(&self, id: usize) -> Result<(), Error> {
        let slots = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        slots[id].store(false, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct AcquiredResource<const N: usize> {
    id: usize,
    resource: ResourceManager<N>,
}

impl<const N: usize> Drop for AcquiredResource<N> {
    fn drop(&mut self) {
        log::debug!("Releasing resource {}: {}", self.resource.name, self.id);
        if let Err(e) = self.resource.release(self.id) {
            log::error!(
                "Error releasing resource [name {}, id = {}]: {e}",
                self.resource.name, self.id
            );
        }
    }
}

#[derive(Clone)]
pub struct RockitSys {
    _inner: Arc<RockitSysInner>,
    pub(crate) cameras: ViCameraResourceManager,
    pub(crate) venc_channels: VencChannelResourceManager,
    pub(crate) vpss_groups: VpssGroupResourceManager,
}

impl RockitSys {
    pub fn init() -> Result<Self, Error> {
        let mpi_sys_init = MPI_SYS_INIT.lock().unwrap();
        if mpi_sys_init.get().is_some() {
            return Err(Error::MpiAlreadyInitialized);
        }
        unsafe {
            rk_check_err!(ffi::RK_MPI_SYS_Init());
        }
        let _ = mpi_sys_init.set(());
        Ok(Self {
            _inner: Arc::new(RockitSysInner),
            cameras: ResourceManager::new("camera".to_string()),
            venc_channels: ResourceManager::new("venc_channel".to_string()),
            vpss_groups: ResourceManager::new("vpss_group".to_string()),
        })
    }

    pub fn camera<'a>(
        &'a self, camera_id: CameraId, num_pipes: u8
    ) -> Result<Camera<'a>, Error> {
        Camera::new(self, camera_id as u8, num_pipes)
    }

    pub fn venc_channel<'a>(
        &'a self, cfg: &VencConfig
    ) -> Result<VencChannel<'a, venc::state::Initialized>, Error> {
        let channel = self.venc_channels.acqure()?;
        VencChannel::new(self, channel.id as i32, cfg)
    }

    pub fn pool<'a>(
        &'a self, buf_size: u32
    ) -> Result<MemBufferPool<'a>, Error> {
        MemBufferPool::new(self, buf_size)
    }

    pub fn vpss_group<'a>(
        &'a self, cfg: &VpssGroupConfig
    ) -> Result<VpssGroup<'a, vpss::state::Initialized>, Error> {
        let group = self.venc_channels.acqure()?;
        VpssGroup::<vpss::state::Initialized>::new(self, group.id as i32, cfg)
    }
}

struct RockitSysInner;

impl Drop for RockitSysInner {
    fn drop(&mut self) {
        log::info!("Exiting MPI system");
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_SYS_Exit(),
                "Error exiting from rockit library"
            );
        }
        let mut mpi_sys_init = MPI_SYS_INIT.lock().unwrap();
        mpi_sys_init.take();
    }
}

pub(crate) struct ChannelBind {
    src_channel: ffi::rkMPP_CHN_S, 
    dst_channel: ffi::rkMPP_CHN_S,
}

impl Drop for ChannelBind {
    fn drop(&mut self) {
        log::debug!(
            "Unbind channels: {} -> {}",
            self.src_channel.s32ChnId,
            self.dst_channel.s32ChnId,
        );
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_SYS_UnBind(
                    &self.src_channel as *const _, &self.dst_channel as *const _
                ),
                "Error unbinding channels"
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PixelFormat {
    Nv12,
    Yuyv,
    Rgb24,
}

impl PixelFormat {
    fn native_format(&self) -> ffi::rkPIXEL_FORMAT_E {
        use PixelFormat::*;

        match self {
            Nv12 => ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
            Yuyv => ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV422_YUYV,
            Rgb24 => ffi::rkPIXEL_FORMAT_E_RK_FMT_RGB888,
        }
    }

    pub fn bytes_per_pixel(&self) -> (u8, u8) {
        use PixelFormat::*;

        match self {
            Nv12 => (3, 2),
            Yuyv => (2, 1),
            Rgb24 => (3, 1),
        }
    }
}

// TODO
// #define RK_ERR_VI_INVALID_PARA        RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_ILLEGAL_PARAM)
// #define RK_ERR_VI_INVALID_DEVID       RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_INVALID_DEVID)
// #define RK_ERR_VI_INVALID_PIPEID      RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_INVALID_PIPEID)
// #define RK_ERR_VI_INVALID_CHNID       RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_INVALID_CHNID)
// #define RK_ERR_VI_INVALID_NULL_PTR    RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NULL_PTR)
// #define RK_ERR_VI_FAILED_NOTCONFIG    RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOT_CONFIG)
// #define RK_ERR_VI_SYS_NOTREADY        RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOTREADY)
// #define RK_ERR_VI_BUF_EMPTY           RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_BUF_EMPTY)
// #define RK_ERR_VI_BUF_FULL            RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_BUF_FULL)
// #define RK_ERR_VI_NOMEM               RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOMEM)
// #define RK_ERR_VI_NOT_SUPPORT         RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOT_SUPPORT)
// #define RK_ERR_VI_BUSY                RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_BUSY)
// #define RK_ERR_VI_NOT_PERM            RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOT_PERM)
// /* try to enable or initialize system,device or pipe or channel, before configing attribute */
// #define RK_ERR_VI_NOT_CONFIG          RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_NOT_CONFIG)
// /* channel exists */
// #define RK_ERR_VI_EXIST               RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_EXIST)
// /* the channel is not existed  */
// #define RK_ERR_VI_UNEXIST             RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_UNEXIST)
// /* the dev exists */
// #define RK_ERR_VI_DEV_EXIST           RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_DEV_EXIST)
// /* the dev is not existed */
// #define RK_ERR_VI_DEV_UNEXIST         RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_DEV_UNEXIST)
// /* the pipe exists */
// #define RK_ERR_VI_PIPE_EXIST          RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_PIPE_EXIST)
// /* the pipe is not existed */
// #define RK_ERR_VI_PIPE_UNEXIST        RK_DEF_ERR(RK_ID_VI, RK_ERR_LEVEL_ERROR, RK_ERR_PIPE_UNEXIST)

// typedef enum rkMOD_ID_E {
//     RK_ID_CMPI    = 0,
//     RK_ID_MB      = 1,
//     RK_ID_SYS     = 2,
//     RK_ID_RGN     = 3,
//     RK_ID_VENC    = 4,
//     RK_ID_VDEC    = 5,
//     RK_ID_VPSS    = 6,
//     RK_ID_VGS     = 7,
//     RK_ID_VI      = 8,
//     RK_ID_VO      = 9,
//     RK_ID_AI      = 10,
//     RK_ID_AO      = 11,
//     RK_ID_AENC    = 12,
//     RK_ID_ADEC    = 13,
//     RK_ID_TDE     = 14,
//     RK_ID_ISP     = 15,
//     RK_ID_WBC     = 16,
//     RK_ID_AVS     = 17,
//     RK_ID_RGA     = 18,
//     RK_ID_AF      = 19,
//     RK_ID_IVS     = 20,
//     RK_ID_GPU     = 21,
//     RK_ID_NN      = 22,
//     RK_ID_AIISP   = 23,

//     RK_ID_BUTT,
// } MOD_ID_E;

// typedef enum rkERR_LEVEL_E {
//     RK_ERR_LEVEL_DEBUG = 0,  /* debug-level                                  */
//     RK_ERR_LEVEL_INFO,       /* informational                                */
//     RK_ERR_LEVEL_NOTICE,     /* normal but significant condition             */
//     RK_ERR_LEVEL_WARNING,    /* warning conditions                           */
//     RK_ERR_LEVEL_ERROR,      /* error conditions                             */
//     RK_ERR_LEVEL_CRIT,       /* critical conditions                          */
//     RK_ERR_LEVEL_ALERT,      /* action must be taken immediately             */
//     RK_ERR_LEVEL_FATAL,      /* just for compatibility with previous version */
//     RK_ERR_LEVEL_BUTT
// } ERR_LEVEL_E;

// #define RK_ERR_APPID  (0x80000000L + 0x20000000L)

// /******************************************************************************
// |----------------------------------------------------------------|
// | 1 |   APP_ID   |   MOD_ID    | ERR_LEVEL |   ERR_ID            |
// |----------------------------------------------------------------|
// |<--><--7bits----><----8bits---><--3bits---><------13bits------->|
// ******************************************************************************/

// #define RK_DEF_ERR(module, level, errid) \
//     ((RK_S32)((RK_ERR_APPID) | ((module) << 16 ) | ((level) << 13) | (errid)))

// /* NOTE! the following defined all common error code,
// ** all module must reserved 0~63 for their common error code
// */
// typedef enum rkEN_ERR_CODE_E {
//     // invlalid device ID
//     RK_ERR_INVALID_DEVID = 1,
//     // invlalid channel ID
//     RK_ERR_INVALID_CHNID = 2,
//     /*
//      * at lease one parameter is illagal
//      * eg, an illegal enumeration value
//      */
//     RK_ERR_ILLEGAL_PARAM = 3,
//     // resource exists
//     RK_ERR_EXIST         = 4,
//     // resource unexists
//     RK_ERR_UNEXIST       = 5,
//     // using a NULL point
//     RK_ERR_NULL_PTR      = 6,
//     /*
//      * try to enable or initialize system, device
//      * or channel, before configing attribute
//      */
//     RK_ERR_NOT_CONFIG    = 7,
//     // operation or type is not supported by NOW
//     RK_ERR_NOT_SUPPORT   = 8,
//     /*
//      * operation is not permitted
//      * eg, try to change static attribute
//      */
//     RK_ERR_NOT_PERM      = 9,
//     // invlalid pipe ID
//     RK_ERR_INVALID_PIPEID = 10,
//     // invlalid stitch group ID
//     RK_ERR_INVALID_STITCHGRPID  = 11,
//     // failure caused by malloc memory
//     RK_ERR_NOMEM         = 12,
//     // failure caused by malloc buffer
//     RK_ERR_NOBUF         = 13,
//     // no data in buffer
//     RK_ERR_BUF_EMPTY     = 14,
//     // no buffer for new data
//     RK_ERR_BUF_FULL      = 15,
//     /*
//      * System is not ready,maybe not initialed or
//      * loaded. Returning the error code when opening
//      * a device file failed.
//      */
//     RK_ERR_NOTREADY      = 16,
//     /*
//      * bad address,
//      * eg. used for copy_from_user & copy_to_user
//      */
//     RK_ERR_BADADDR       = 17,
//     /*
//      * resource is busy,
//      * eg. destroy a venc chn without unregister it
//      */
//     RK_ERR_BUSY          = 18,
//     // buffer size is smaller than the actual size required
//     RK_ERR_SIZE_NOT_ENOUGH = 19,
//     /*
//      * dev resource exists
//      */
//     RK_ERR_DEV_EXIST       = 20,
//     /*
//      * dev resource unexists
//      */
//     RK_ERR_DEV_UNEXIST     = 21,
//     /*
//      * pipe resource exists
//      */
//     RK_ERR_PIPE_EXIST      = 22,
//     /*
//      * pipe resource unexists
//      */
//     RK_ERR_PIPE_UNEXIST    = 23,
//     /*
//      * group resource exists
//      */
//     RK_ERR_GROUP_EXIST      = 24,
//     /*
//      * group resource unexists
//      */
//     RK_ERR_GROUP_UNEXIST    = 25,
//     /*
//      * maxium code, private error code of all modules
//      * must be greater than it
//      */
//     RK_ERR_BUTT          = 63,
// }RK_ERR_CODE_E;
