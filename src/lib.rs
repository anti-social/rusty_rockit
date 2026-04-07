use core::slice;
use std::cell::OnceCell;
use std::mem::MaybeUninit;
use std::sync::{Arc, Mutex};

use rockit_sys as ffi;
use snafu::Snafu;

// static MPI_SYS_INIT: OnceLock<Result<(), Error>> = OnceLock::new();
static MPI_SYS_INIT: Mutex<OnceCell<()>> = Mutex::new(OnceCell::new());

#[derive(Clone, Debug, Snafu)]
pub enum Error {
    #[snafu(display("MPI is already initialized"))]
    MpiAlreadyInitialized,
    #[snafu(display("Invalid device id: {id}"))]
    InvalidDevId { id: u8 },
    #[snafu(display("Requested too many pipes: {num}"))]
    RequestedTooManyPipes { num: u8 },
    #[snafu(display("Invalid pipe id: {id}"))]
    InvalidPipeId { id: u8 },
    #[snafu(display("Invalid channel id: {id}"))]
    InvalidChannelId { id: u8 },
    #[snafu(display("Invalid frame pointer"))]
    InvalidFramePointer,
    #[snafu(display("Rockit error code: {code}"))]
    Rockit { code: i32 }
}

macro_rules! rk_check_err {
    ($fn:expr) => {
        let ret_code = $fn;
        if ret_code != RK_SUCCESS {
            return Err(Error::Rockit { code: ret_code });
        }
    };
}

macro_rules! rk_log_err {
    ($fn:expr, $msg:literal) => {
        let ret_code = $fn;
        if ret_code != RK_SUCCESS {
            eprintln!("{}: {}", $msg, ret_code);
        }
    };
}

#[derive(Clone)]
pub struct RockitSys {
    _inner: Arc<RockitSysInner>,
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
        Ok(Self { _inner: Arc::new(RockitSysInner) })
    }

    pub fn dev<'a>(&'a self, dev_id: u8, num_pipes: u8) -> Result<RockitDev<'a>, Error> {
        RockitDev::new(self, dev_id, num_pipes)
    }
}

struct RockitSysInner;

impl Drop for RockitSysInner {
    fn drop(&mut self) {
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

pub struct RockitDev<'a> {
    _mpi: &'a RockitSys,
    id: i32,
    dev: ffi::rkVI_DEV_ATTR_S,
    pipe: ffi::rkVI_DEV_BIND_PIPE_S,
}

impl<'a> Drop for RockitDev<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableDev(self.id),
                "Error disabling rockit device"
            );
        }
    }
}

impl<'a> RockitDev<'a> {
    fn new(
        mpi: &'a RockitSys,
        dev_id: u8,
        num_pipes: u8, 
    ) -> Result<Self, Error> {
        if dev_id as u32 > ffi::VI_MAX_DEV_NUM {
            return Err(Error::InvalidDevId { id: dev_id });
        }
        if num_pipes as u32 > ffi::VI_MAX_PIPE_NUM {
            return Err(Error::RequestedTooManyPipes { num: num_pipes });
        }
        
        let dev_id = dev_id as i32;
        let (dev, pipe) = unsafe {
            let mut dev = MaybeUninit::zeroed();
            let res = ffi::RK_MPI_VI_GetDevAttr(dev_id, dev.as_mut_ptr());
            if res == RK_ERR_VI_NOT_CONFIG {
                rk_check_err!(ffi::RK_MPI_VI_SetDevAttr(dev_id, dev.as_mut_ptr()));
            }

            if ffi::RK_MPI_VI_GetDevIsEnable(dev_id) != RK_SUCCESS {
                rk_check_err!(ffi::RK_MPI_VI_EnableDev(dev_id));
            }

            let mut pipe = ffi::rkVI_DEV_BIND_PIPE_S {
                u32Num: num_pipes as u32,
                PipeId: [0; 16usize],
                bDataOffline: 0,
                bUserStartPipe: [0; 16],
            };
            rk_check_err!(ffi::RK_MPI_VI_SetDevBindPipe(dev_id, &mut pipe as *mut _));

            (dev.assume_init(), pipe)
        };

        Ok(Self { _mpi: mpi, id: dev_id, dev, pipe })
    }

    pub fn get_pipe(&self, pipe_id: u8) -> Result<ViPipe<'_>, Error> {
        if pipe_id as u32 >= self.pipe.u32Num {
            return Err(Error::InvalidPipeId { id: pipe_id });
        }
        Ok(ViPipe::new(self, pipe_id as i32))
    }
}

pub struct ViPipe<'a> {
    _dev: &'a RockitDev<'a>,
    id: i32,
}

impl<'a> ViPipe<'a> {
    fn new(dev: &'a RockitDev<'a>, id: i32) -> Self {
        Self { _dev: dev, id }
    }

    pub fn create_channel(
        &self, channel_id: u8, width: u16, height: u16
    ) -> Result<RockitChannel<'_>, Error> {
        RockitChannel::new(self, channel_id, width, height)
    } 
}

pub struct RockitChannel<'a> {
    pipe: &'a ViPipe<'a>,
    id: i32,
    channel: ffi::VI_CHN_ATTR_S,
}

impl<'a> RockitChannel<'a> {
    fn new(
        pipe: &'a ViPipe<'a>,
        channel_id: u8,
        width: u16,
        height: u16,
    ) -> Result<Self, Error> {
        if channel_id as u32 > ffi::VI_MAX_CHN_NUM {
            return Err(Error::InvalidDevId { id: channel_id });
        }
        let channel_id = channel_id as i32;

        let channel = unsafe {
            let mut channel = ffi::VI_CHN_ATTR_S {
                stSize: ffi::SIZE_S {
                    u32Width: width as _,
                    u32Height: height as _,
                },
                enPixelFormat: ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
                enDynamicRange: ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8,
                enVideoFormat: ffi::rkVIDEO_FORMAT_E_VIDEO_FORMAT_LINEAR,
                enCompressMode: ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE,
                bMirror: 0,
                bFlip: 0,
                u32Depth: 2,
                stFrameRate: ffi::FRAME_RATE_CTRL_S {
                    s32SrcFrameRate: 30,
                    s32DstFrameRate: 30,
                },
                enAllocBufType: ffi::rkVI_ALLOC_BUF_TYPE_E_VI_ALLOC_BUF_TYPE_INTERNAL,
                stIspOpt: ffi::VI_ISP_OPT_S {
                    u32BufCount: 2,
                    u32BufSize: 0,
                    enCaptureType: 0,
                    enMemoryType: ffi::rkVI_V4L2_MEMORY_TYPE_VI_V4L2_MEMORY_TYPE_DMABUF,
                    aEntityName: [0; _],
                    bNoUseLibV4L2: 0,
                    stMaxSize: ffi::SIZE_S {
                        u32Width: 0,
                        u32Height: 0,
                    },
                    stWindow: ffi::RECT_S {
                        s32X: 0,
                        s32Y: 0,
                        u32Width: 0,
                        u32Height: 0,
                    },
                },
                stShareBufChn: ffi::MPP_CHN_S {
                    enModId: ffi::rkMOD_ID_E_RK_ID_CMPI,
                    s32DevId: 0,
                    s32ChnId: 0,
                },
            };
            rk_check_err!(
                ffi::RK_MPI_VI_SetChnAttr(pipe.id, channel_id, &mut channel as *mut _)
            );
            rk_check_err!(ffi::RK_MPI_VI_EnableChn(pipe.id, channel_id));
            channel
        };
        
        Ok(Self { pipe, id: channel_id, channel })
    }

    pub fn get_frame(&self) -> Result<ViFrame<'_>, Error> {
        let vi_frame = unsafe {
            let mut vi_frame = MaybeUninit::zeroed();
            rk_check_err!(
                ffi::RK_MPI_VI_GetChnFrame(0, self.id, vi_frame.as_mut_ptr(), 1000)
            );
            vi_frame.assume_init()
        };

        Ok(ViFrame { channel: self, frame: vi_frame })
    }
}

impl<'a> Drop for RockitChannel<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableChn(0, self.id),
                "Error disabling rockit channel"
            );
        }
    }
}

pub struct ViFrame<'a> {
    channel: &'a RockitChannel<'a>,
    frame: ffi::rkVIDEO_FRAME_INFO_S,
}

impl<'a> Drop for ViFrame<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_ReleaseChnFrame(
                    self.channel.pipe.id, self.channel.id, &self.frame as *const _
                ),
                "Error releasing channel frame"
            );
        }
    }
}

impl<'a> ViFrame<'a> {
    pub fn width(&self) -> u32 {
        self.frame.stVFrame.u32Width
    }

    pub fn height(&self) -> u32 {
        self.frame.stVFrame.u32Height
    }

    pub fn data(&self) -> Result<&[u8], Error> {
        let data = unsafe {
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(self.frame.stVFrame.pMbBlk);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts(
                data_ptr as *const u8,
                self.frame.stVFrame.u32Width as usize * self.frame.stVFrame.u32Height as usize * 3 / 2
            )
        };
        Ok(data)
    }
}

const RK_SUCCESS: i32 = ffi::RK_SUCCESS as i32;
const RK_ERR_APPID: u32 = 0x80000000 + 0x20000000;
const RK_ERR_VI_NOT_CONFIG: i32 = rk_def_err(
    ffi::rkMOD_ID_E_RK_ID_VI as i32,
    ffi::rkERR_LEVEL_E_RK_ERR_LEVEL_ERROR as i32,
    ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG as i32,
);

const fn rk_def_err(module: i32, level: i32, errid: i32) -> i32 {
    RK_ERR_APPID as i32 | ((module) << 16 ) | ((level) << 13) | (errid)
}


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
