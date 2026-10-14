use std::os::raw::c_void;

type CGDirectDisplayID = u32;
type CGColorSpaceRef = *mut c_void;
type CFDataRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> CGDirectDisplayID;
    fn CGDisplayCopyColorSpace(display: CGDirectDisplayID) -> CGColorSpaceRef;
    fn CGColorSpaceCopyICCData(space: CGColorSpaceRef) -> CFDataRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetLength(data: CFDataRef) -> i64;
    fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    fn CFRelease(cf: *const c_void);
}

pub fn display_p3() -> bool {
    // SAFETY: plain C functions with no preconditions beyond non-null
    unsafe {
        let space = CGDisplayCopyColorSpace(CGMainDisplayID());
        if space.is_null() {
            return false;
        }
        let data = CGColorSpaceCopyICCData(space);
        CFRelease(space);
        if data.is_null() {
            return false;
        }
        let ok = {
            let len = CFDataGetLength(data);
            let ptr = CFDataGetBytePtr(data);
            if len > 0 && !ptr.is_null() {
                let bytes = std::slice::from_raw_parts(ptr, len as usize);
                super::primaries_from_icc(bytes)
                    .is_some_and(|[r, g, _]| super::is_p3_primaries(r, g))
            } else {
                false
            }
        };
        CFRelease(data);
        ok
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn detection_is_a_bool() {
        let _ = super::display_p3();
    }
}
