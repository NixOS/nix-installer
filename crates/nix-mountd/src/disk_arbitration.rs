//! Veto unmount attempts against the Nix Store volume via DiskArbitration.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use core_foundation::base::TCFType;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_foundation::string::CFString;
use core_foundation::url::{CFURL, CFURLRef};
use core_foundation_sys::base::{CFAllocatorRef, kCFAllocatorDefault};
use core_foundation_sys::runloop::CFRunLoopRef;
use core_foundation_sys::string::CFStringRef;

type DASessionRef = *const c_void;
type DADiskRef = *const c_void;
type DADissenterRef = *const c_void;
type DAReturn = i32;

/// kDAReturnBusy from <DiskArbitration/DADissenter.h>.
const DA_RETURN_BUSY: DAReturn = 0xF8DA0002u32 as DAReturn;

type DADiskUnmountApprovalCallback =
    extern "C" fn(disk: DADiskRef, context: *mut c_void) -> DADissenterRef;

#[link(name = "DiskArbitration", kind = "framework")]
unsafe extern "C" {
    fn DASessionCreate(allocator: CFAllocatorRef) -> DASessionRef;
    fn DASessionScheduleWithRunLoop(
        session: DASessionRef,
        run_loop: CFRunLoopRef,
        run_loop_mode: CFStringRef,
    );
    fn DADiskCopyDescription(disk: DADiskRef) -> CFDictionaryRef;
    fn DADissenterCreate(
        allocator: CFAllocatorRef,
        status: DAReturn,
        string: CFStringRef,
    ) -> DADissenterRef;
    fn DARegisterDiskUnmountApprovalCallback(
        session: DASessionRef,
        r#match: CFDictionaryRef,
        callback: DADiskUnmountApprovalCallback,
        context: *mut c_void,
    );
    static kDADiskDescriptionVolumePathKey: CFStringRef;
}

fn disk_volume_path(disk: DADiskRef) -> Option<PathBuf> {
    let description = unsafe { DADiskCopyDescription(disk) };
    if description.is_null() {
        return None;
    }
    // DADiskCopyDescription follows the create rule.
    let description: CFDictionary = unsafe { CFDictionary::wrap_under_create_rule(description) };

    let key = unsafe { CFString::wrap_under_get_rule(kDADiskDescriptionVolumePathKey) };
    let value = description.find(key.as_CFTypeRef() as *const c_void)?;
    let url = unsafe { CFURL::wrap_under_get_rule(*value as CFURLRef) };
    url.to_path()
}

extern "C" fn unmount_approval(disk: DADiskRef, context: *mut c_void) -> DADissenterRef {
    // Safety: context is the leaked PathBuf installed in `run`, which outlives
    // the run loop.
    let guarded = unsafe { &*(context as *const PathBuf) };

    match disk_volume_path(disk) {
        Some(path) if path == *guarded => {
            tracing::warn!(path = %path.display(), "Dissenting against unmount");
            let reason = CFString::new("In use by Nix. Stop nix-mountd first.");
            unsafe {
                DADissenterCreate(
                    kCFAllocatorDefault,
                    DA_RETURN_BUSY,
                    reason.as_concrete_TypeRef(),
                )
            }
        },
        // Null dissenter approves the unmount.
        _ => std::ptr::null(),
    }
}

/// Register the dissenter for `mount_point` and drive the run loop. Does not
/// return under normal operation.
pub fn run(mount_point: &Path) -> std::io::Result<()> {
    let session = unsafe { DASessionCreate(kCFAllocatorDefault) };
    if session.is_null() {
        return Err(std::io::Error::other("DASessionCreate returned null"));
    }

    // The run loop never returns, so leak the guarded path as callback context.
    let context = Box::into_raw(Box::new(mount_point.to_path_buf())) as *mut c_void;

    unsafe {
        DARegisterDiskUnmountApprovalCallback(session, std::ptr::null(), unmount_approval, context);
        DASessionScheduleWithRunLoop(
            session,
            CFRunLoop::get_current().as_concrete_TypeRef(),
            kCFRunLoopDefaultMode,
        );
    }

    tracing::info!(mount_point = %mount_point.display(), "Guarding against unmount");
    CFRunLoop::run_current();
    Ok(())
}
