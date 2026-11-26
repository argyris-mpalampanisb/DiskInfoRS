//! Disk enumeration and S.M.A.R.T. reading on macOS through IOKit.
//!
//! Whole disks are found as `IOMedia` objects (`Whole = true`) sitting directly on an
//! `IOBlockStorageDriver`, which filters out APFS containers, RAM disks and disk images.
//! S.M.A.R.T. comes from the IOKit user-client plug-ins that smartmontools also uses:
//! `IOATASMARTInterface` on services with `SMART Capable = true` (SATA), and
//! `IONVMeSMARTInterface` on services with `NVMe SMART Capable = true` (NVMe, Apple SSD).
//! Neither needs root. USB drives have no pass-through on macOS without third-party kexts.

#![allow(non_upper_case_globals, non_snake_case)]

use std::collections::BTreeMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use crate::assemble;
use crate::model::{Disk, DiskKind};

// ---------------------------------------------------------------------------------------
// Minimal CoreFoundation / IOKit FFI
// ---------------------------------------------------------------------------------------

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type CFUUIDRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFTypeID = usize;
type CFIndex = isize;
type Boolean = u8;

type IOReturn = i32;
type KernReturn = i32;
type MachPort = u32;
type IOObject = MachPort;
type IOIterator = IOObject;
type IORegistryEntry = IOObject;
type IOService = IOObject;
type IOOptionBits = u32;

const kIOReturnSuccess: IOReturn = 0;
const kIOMainPortDefault: MachPort = 0;
const kCFStringEncodingUTF8: u32 = 0x0800_0100;
const kCFNumberSInt64Type: CFIndex = 4;
const kIORegistryIterateRecursively: IOOptionBits = 1;
const kIORegistryIterateParents: IOOptionBits = 2;
const kIOServicePlane: &CStr = c"IOService";

#[repr(C)]
#[derive(Clone, Copy)]
struct CFUUIDBytes([u8; 16]);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFDictionaryGetTypeID() -> CFTypeID;
    fn CFStringCreateWithCString(alloc: CFAllocatorRef, s: *const c_char, enc: u32) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, enc: u32) -> Boolean;
    fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> Boolean;
    fn CFBooleanGetValue(b: CFTypeRef) -> Boolean;
    fn CFDictionaryGetValue(d: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFUUIDCreateFromUUIDBytes(alloc: CFAllocatorRef, bytes: CFUUIDBytes) -> CFUUIDRef;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(port: MachPort, matching: CFDictionaryRef, it: *mut IOIterator) -> KernReturn;
    fn IOIteratorNext(it: IOIterator) -> IOObject;
    fn IOObjectRelease(o: IOObject) -> KernReturn;
    fn IOObjectRetain(o: IOObject) -> KernReturn;
    fn IOObjectConformsTo(o: IOObject, class: *const c_char) -> Boolean;
    fn IORegistryEntryGetName(e: IORegistryEntry, name: *mut c_char) -> KernReturn;
    fn IORegistryEntryGetParentEntry(e: IORegistryEntry, plane: *const c_char, parent: *mut IORegistryEntry) -> KernReturn;
    fn IORegistryEntryCreateCFProperty(e: IORegistryEntry, key: CFStringRef, alloc: CFAllocatorRef, opts: IOOptionBits) -> CFTypeRef;
    fn IORegistryEntrySearchCFProperty(
        e: IORegistryEntry,
        plane: *const c_char,
        key: CFStringRef,
        alloc: CFAllocatorRef,
        opts: IOOptionBits,
    ) -> CFTypeRef;
    fn IOCreatePlugInInterfaceForService(
        service: IOService,
        plugin_type: CFUUIDRef,
        interface_type: CFUUIDRef,
        the_interface: *mut *mut *mut IOCFPlugInInterface,
        score: *mut i32,
    ) -> KernReturn;
}

/// `IUNKNOWN_C_GUTS` from CFPlugInCOM.h.
#[repr(C)]
struct IUnknownGuts {
    _reserved: *mut c_void,
    query_interface: unsafe extern "C" fn(this: *mut c_void, iid: CFUUIDBytes, ppv: *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(this: *mut c_void) -> u32,
    release: unsafe extern "C" fn(this: *mut c_void) -> u32,
}

/// `IOCFPlugInInterface` from IOCFPlugIn.h (only the IUnknown part is used).
#[repr(C)]
struct IOCFPlugInInterface {
    unknown: IUnknownGuts,
}

/// `IOATASMARTInterface` from IOKit/storage/ata/ATASMARTLib.h.
#[repr(C)]
struct IOATASMARTInterface {
    unknown: IUnknownGuts,
    version: u16,
    revision: u16,
    smart_enable_disable_operations: unsafe extern "C" fn(*mut c_void, Boolean) -> IOReturn,
    smart_enable_disable_autosave: unsafe extern "C" fn(*mut c_void, Boolean) -> IOReturn,
    smart_return_status: unsafe extern "C" fn(*mut c_void, *mut Boolean) -> IOReturn,
    smart_execute_offline_immediate: unsafe extern "C" fn(*mut c_void, Boolean) -> IOReturn,
    smart_read_data: unsafe extern "C" fn(*mut c_void, *mut u8) -> IOReturn,
    smart_validate_read_data: unsafe extern "C" fn(*mut c_void, *const u8) -> IOReturn,
    smart_read_data_thresholds: unsafe extern "C" fn(*mut c_void, *mut u8) -> IOReturn,
    smart_read_log_directory: unsafe extern "C" fn(*mut c_void, *mut u8) -> IOReturn,
    smart_read_log_at_address: unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32) -> IOReturn,
    smart_write_log_at_address: unsafe extern "C" fn(*mut c_void, u32, *const c_void, u32) -> IOReturn,
    get_ata_identify_data: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut u32) -> IOReturn,
}

/// `IONVMeSMARTInterface` (private header, layout as used by smartmontools' os_darwin.h).
#[repr(C)]
struct IONVMeSMARTInterface {
    unknown: IUnknownGuts,
    version: u16,
    revision: u16,
    smart_read_data: unsafe extern "C" fn(*mut c_void, *mut u8) -> IOReturn,
    get_identify_data: unsafe extern "C" fn(*mut c_void, *mut u8, u32) -> IOReturn,
}

// C244E858-109C-11D4-91D4-0050E4C6426F
const kIOCFPlugInInterfaceID: [u8; 16] =
    [0xC2, 0x44, 0xE8, 0x58, 0x10, 0x9C, 0x11, 0xD4, 0x91, 0xD4, 0x00, 0x50, 0xE4, 0xC6, 0x42, 0x6F];
// 24514B7A-2804-11D6-8A02-003065704866
const kIOATASMARTUserClientTypeID: [u8; 16] =
    [0x24, 0x51, 0x4B, 0x7A, 0x28, 0x04, 0x11, 0xD6, 0x8A, 0x02, 0x00, 0x30, 0x65, 0x70, 0x48, 0x66];
// 08ABE21C-20D4-11D6-8DF6-0003935A76B2
const kIOATASMARTInterfaceID: [u8; 16] =
    [0x08, 0xAB, 0xE2, 0x1C, 0x20, 0xD4, 0x11, 0xD6, 0x8D, 0xF6, 0x00, 0x03, 0x93, 0x5A, 0x76, 0xB2];
// AA0FA6F9-C2D6-457F-B10B-59A13253292F
const kIONVMeSMARTUserClientTypeID: [u8; 16] =
    [0xAA, 0x0F, 0xA6, 0xF9, 0xC2, 0xD6, 0x45, 0x7F, 0xB1, 0x0B, 0x59, 0xA1, 0x32, 0x53, 0x29, 0x2F];
// CCD1DB19-FD9A-4DAF-BF95-12454B230AB6
const kIONVMeSMARTInterfaceID: [u8; 16] =
    [0xCC, 0xD1, 0xDB, 0x19, 0xFD, 0x9A, 0x4D, 0xAF, 0xBF, 0x95, 0x12, 0x45, 0x4B, 0x23, 0x0A, 0xB6];

// ---------------------------------------------------------------------------------------
// Safe-ish wrappers
// ---------------------------------------------------------------------------------------

/// Owned IOKit object reference.
struct Obj(IOObject);

impl Obj {
    fn conforms(&self, class: &CStr) -> bool {
        // SAFETY: valid object and NUL-terminated class name.
        unsafe { IOObjectConformsTo(self.0, class.as_ptr()) != 0 }
    }

    fn parent(&self) -> Option<Obj> {
        let mut p: IORegistryEntry = 0;
        // SAFETY: out pointer is valid; the returned parent is retained by the call.
        let r = unsafe { IORegistryEntryGetParentEntry(self.0, kIOServicePlane.as_ptr(), &mut p) };
        (r == kIOReturnSuccess && p != 0).then(|| Obj(p))
    }

    fn prop(&self, key: &str) -> Option<Cf> {
        let k = CfStr::new(key)?;
        // SAFETY: valid entry and key; result follows the Create rule.
        Cf::wrap(unsafe { IORegistryEntryCreateCFProperty(self.0, k.0, ptr::null(), 0) })
    }

    /// Looks the property up on this entry and then on each of its ancestors.
    fn search_up(&self, key: &str) -> Option<Cf> {
        let k = CfStr::new(key)?;
        // SAFETY: valid entry, plane and key; result follows the Create rule.
        Cf::wrap(unsafe {
            IORegistryEntrySearchCFProperty(
                self.0,
                kIOServicePlane.as_ptr(),
                k.0,
                ptr::null(),
                kIORegistryIterateRecursively | kIORegistryIterateParents,
            )
        })
    }

    /// Registry name, e.g. "APPLE SSD AP0512Q Media".
    fn name(&self) -> Option<String> {
        let mut buf = [0 as c_char; 128]; // io_name_t
        // SAFETY: buf is the io_name_t size the call requires; it NUL-terminates.
        if unsafe { IORegistryEntryGetName(self.0, buf.as_mut_ptr()) } != kIOReturnSuccess {
            return None;
        }
        // SAFETY: NUL-terminated by the call above.
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    fn try_clone(&self) -> Obj {
        // SAFETY: valid object; the extra reference is released by the new Obj's Drop.
        unsafe { IOObjectRetain(self.0) };
        Obj(self.0)
    }
}

impl Drop for Obj {
    fn drop(&mut self) {
        // SAFETY: we own one reference.
        unsafe { IOObjectRelease(self.0) };
    }
}

/// Owned CF object (Create rule).
struct Cf(CFTypeRef);

impl Cf {
    fn wrap(p: CFTypeRef) -> Option<Cf> {
        (!p.is_null()).then(|| Cf(p))
    }

    fn type_is(&self, id: CFTypeID) -> bool {
        // SAFETY: valid CF object.
        unsafe { CFGetTypeID(self.0) == id }
    }

    fn string(&self) -> Option<String> {
        cf_string(self.0)
    }

    fn u64(&self) -> Option<u64> {
        cf_u64(self.0)
    }

    fn bool(&self) -> Option<bool> {
        cf_bool(self.0)
    }

    /// Borrowed value for `key` if this is a dictionary.
    fn get(&self, key: &str) -> Option<CFTypeRef> {
        // SAFETY: type checked before use as a dictionary.
        if !self.type_is(unsafe { CFDictionaryGetTypeID() }) {
            return None;
        }
        let k = CfStr::new(key)?;
        // SAFETY: valid dictionary and key; result is borrowed (Get rule).
        let v = unsafe { CFDictionaryGetValue(self.0, k.0) };
        (!v.is_null()).then_some(v)
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: we own one reference.
        unsafe { CFRelease(self.0) };
    }
}

struct CfStr(CFStringRef);

impl CfStr {
    fn new(s: &str) -> Option<CfStr> {
        let c = CString::new(s).ok()?;
        // SAFETY: valid C string.
        let r = unsafe { CFStringCreateWithCString(ptr::null(), c.as_ptr(), kCFStringEncodingUTF8) };
        (!r.is_null()).then(|| CfStr(r))
    }
}

impl Drop for CfStr {
    fn drop(&mut self) {
        // SAFETY: created above.
        unsafe { CFRelease(self.0) };
    }
}

fn cf_string(p: CFTypeRef) -> Option<String> {
    // SAFETY: p is a valid CF object; type checked before reading it as a string.
    unsafe {
        if p.is_null() || CFGetTypeID(p) != CFStringGetTypeID() {
            return None;
        }
        let mut buf = vec![0 as c_char; 1024];
        if CFStringGetCString(p, buf.as_mut_ptr(), buf.len() as CFIndex, kCFStringEncodingUTF8) == 0 {
            return None;
        }
        let s = CStr::from_ptr(buf.as_ptr()).to_string_lossy().trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn cf_u64(p: CFTypeRef) -> Option<u64> {
    // SAFETY: as above.
    unsafe {
        if p.is_null() || CFGetTypeID(p) != CFNumberGetTypeID() {
            return None;
        }
        let mut v: i64 = 0;
        (CFNumberGetValue(p, kCFNumberSInt64Type, &mut v as *mut i64 as *mut c_void) != 0).then_some(v as u64)
    }
}

fn cf_bool(p: CFTypeRef) -> Option<bool> {
    // SAFETY: as above.
    unsafe {
        if p.is_null() {
            return None;
        }
        if CFGetTypeID(p) == CFBooleanGetTypeID() {
            return Some(CFBooleanGetValue(p) != 0);
        }
    }
    cf_u64(p).map(|n| n != 0)
}

fn services(class: &CStr) -> Vec<Obj> {
    let mut out = Vec::new();
    let mut it: IOIterator = 0;
    // SAFETY: IOServiceGetMatchingServices consumes the matching dictionary.
    unsafe {
        let m = IOServiceMatching(class.as_ptr());
        if m.is_null() || IOServiceGetMatchingServices(kIOMainPortDefault, m, &mut it) != kIOReturnSuccess {
            return out;
        }
        loop {
            let o = IOIteratorNext(it);
            if o == 0 {
                break;
            }
            out.push(Obj(o));
        }
        IOObjectRelease(it);
    }
    out
}

/// Creates the user-client plug-in for `service` and returns the requested COM interface.
/// The returned guard releases both the interface and the plug-in.
struct PlugIn {
    plugin: *mut *mut IOCFPlugInInterface,
    iface: *mut *mut c_void,
}

impl PlugIn {
    fn open(service: &Obj, client_type: [u8; 16], iface_id: [u8; 16]) -> Result<PlugIn, String> {
        // SAFETY: standard IOCFPlugIn dance; every pointer is checked before use and the
        // UUID objects are released after the call.
        unsafe {
            let ct = CFUUIDCreateFromUUIDBytes(ptr::null(), CFUUIDBytes(client_type));
            let pi = CFUUIDCreateFromUUIDBytes(ptr::null(), CFUUIDBytes(kIOCFPlugInInterfaceID));
            if ct.is_null() || pi.is_null() {
                return Err("CFUUID allocation failed".into());
            }
            let mut plugin: *mut *mut IOCFPlugInInterface = ptr::null_mut();
            let mut score = 0i32;
            let r = IOCreatePlugInInterfaceForService(service.0, ct, pi, &mut plugin, &mut score);
            CFRelease(ct);
            CFRelease(pi);
            if r != kIOReturnSuccess || plugin.is_null() || (*plugin).is_null() {
                return Err(format!("cannot open S.M.A.R.T. user client (IOKit 0x{:08X})", r as u32));
            }
            let mut iface: *mut c_void = ptr::null_mut();
            let hr = ((**plugin).unknown.query_interface)(plugin as *mut c_void, CFUUIDBytes(iface_id), &mut iface);
            if hr != 0 || iface.is_null() {
                ((**plugin).unknown.release)(plugin as *mut c_void);
                return Err(format!("S.M.A.R.T. interface not available (0x{:08X})", hr as u32));
            }
            Ok(PlugIn { plugin, iface: iface as *mut *mut c_void })
        }
    }

    /// The interface vtable, typed.
    ///
    /// # Safety
    /// `T` must match the interface ID passed to `open`.
    unsafe fn vtbl<T>(&self) -> &T {
        &*(*self.iface as *const T)
    }

    fn this(&self) -> *mut c_void {
        self.iface as *mut c_void
    }
}

impl Drop for PlugIn {
    fn drop(&mut self) {
        // SAFETY: both pointers were obtained in `open` and are released exactly once.
        unsafe {
            let unk = &*(*self.iface as *const IUnknownGuts);
            (unk.release)(self.iface as *mut c_void);
            ((**self.plugin).unknown.release)(self.plugin as *mut c_void);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------------------

/// A physical whole disk: its `IOMedia` and the `IOBlockStorageDevice` underneath.
struct Physical {
    bsd: String,
    media: Obj,
    device: Obj,
}

/// `IOMedia` objects that are whole disks of a real block storage device.
fn physical_disks() -> Vec<Physical> {
    let mut out = Vec::new();
    for media in services(c"IOMedia") {
        if media.prop("Whole").and_then(|v| v.bool()) != Some(true) {
            continue;
        }
        let Some(driver) = media.parent() else { continue };
        if !driver.conforms(c"IOBlockStorageDriver") {
            continue;
        }
        let Some(device) = driver.parent() else { continue };
        if !device.conforms(c"IOBlockStorageDevice") {
            continue;
        }
        let Some(bsd) = media.prop("BSD Name").and_then(|v| v.string()) else { continue };
        if let Some(pc) = device.prop("Protocol Characteristics") {
            let ic = pc.get("Physical Interconnect").and_then(cf_string).unwrap_or_default();
            let loc = pc.get("Physical Interconnect Location").and_then(cf_string).unwrap_or_default();
            // Disk images (hdiutil) and RAM disks.
            if loc == "File" || ic == "Virtual Interface" {
                continue;
            }
        }
        out.push(Physical { bsd, media, device });
    }
    out.sort_by_key(|p| natural_key(&p.bsd));
    out
}

fn natural_key(s: &str) -> (String, u64) {
    let prefix: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let num: String = s[prefix.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    (prefix, num.parse().unwrap_or(0))
}

/// Finds the physical whole disk under any `IOMedia` (partition, APFS volume, ...) by
/// walking up the service plane (APFS: volume -> container -> physical store -> disk).
fn physical_of(media: &Obj) -> Option<String> {
    let mut e = media.try_clone();
    for _ in 0..32 {
        let parent = e.parent()?;
        if e.conforms(c"IOMedia") && parent.conforms(c"IOBlockStorageDriver") && e.prop("Whole").and_then(|v| v.bool()) == Some(true) {
            return e.prop("BSD Name").and_then(|v| v.string());
        }
        e = parent;
    }
    None
}

/// Maps whole-disk BSD names to the user-visible mount points on them.
fn mount_map() -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut buf: *mut libc::statfs = ptr::null_mut();
    // SAFETY: getmntinfo returns a pointer to an internal buffer valid until the next call.
    let n = unsafe { libc::getmntinfo(&mut buf, libc::MNT_NOWAIT) };
    if n <= 0 || buf.is_null() {
        return map;
    }
    // SAFETY: n entries are valid.
    let entries = unsafe { std::slice::from_raw_parts(buf, n as usize) };
    let media: BTreeMap<String, Obj> = services(c"IOMedia")
        .into_iter()
        .filter_map(|m| Some((m.prop("BSD Name")?.string()?, m)))
        .collect();
    let mut owner: BTreeMap<String, Option<String>> = BTreeMap::new();
    for fs in entries {
        // Hidden system volumes (Preboot, VM, Update, ...) are flagged "don't browse".
        if fs.f_flags & libc::MNT_DONTBROWSE as u32 != 0 && fs.f_flags & libc::MNT_ROOTFS as u32 == 0 {
            continue;
        }
        // SAFETY: NUL-terminated fixed-size arrays filled by the kernel.
        let from = unsafe { CStr::from_ptr(fs.f_mntfromname.as_ptr()) }.to_string_lossy().into_owned();
        let on = unsafe { CStr::from_ptr(fs.f_mntonname.as_ptr()) }.to_string_lossy().into_owned();
        let Some(dev) = from.strip_prefix("/dev/") else { continue };
        let whole = owner.entry(dev.to_string()).or_insert_with(|| media.get(dev).and_then(physical_of)).clone();
        if let Some(w) = whole {
            let v = map.entry(w).or_default();
            if !v.contains(&on) {
                v.push(on);
            }
        }
    }
    map
}

fn pcie_mode(dev: &Obj) -> Option<String> {
    // PCIe Link Status / Link Capabilities registers: bits 3:0 speed (gen), 9:4 width.
    let fmt = |v: u64| {
        let (g, w) = (v & 0xF, (v >> 4) & 0x3F);
        (g > 0 && w > 0).then(|| format!("PCIe {g}.0 x{w}"))
    };
    let cur = dev.search_up("IOPCIExpressLinkStatus").and_then(|v| v.u64()).and_then(fmt);
    let max = dev.search_up("IOPCIExpressLinkCapabilities").and_then(|v| v.u64()).and_then(fmt);
    match (cur, max) {
        (Some(c), Some(m)) => Some(format!("{c} | {m}")),
        (Some(c), None) => Some(c),
        (None, Some(m)) => Some(format!("---- | {m}")),
        _ => None,
    }
}

fn describe(p: &Physical, mounts: &BTreeMap<String, Vec<String>>) -> Disk {
    let mut d = Disk {
        id: p.bsd.clone(),
        dev_path: format!("/dev/{}", p.bsd),
        size_bytes: p.media.prop("Size").and_then(|v| v.u64()).unwrap_or(0),
        removable: p.media.prop("Removable").and_then(|v| v.bool()).unwrap_or(false)
            || p.media.prop("Ejectable").and_then(|v| v.bool()).unwrap_or(false),
        mount_points: mounts.get(&p.bsd).cloned().unwrap_or_default(),
        ..Default::default()
    };
    if let Some(dc) = p.device.prop("Device Characteristics") {
        let vendor = dc.get("Vendor Name").and_then(cf_string).unwrap_or_default();
        let product = dc.get("Product Name").and_then(cf_string).unwrap_or_default();
        d.model = if vendor.is_empty() || product.starts_with(&vendor) { product } else { format!("{vendor} {product}") };
        d.serial = dc.get("Serial Number").and_then(cf_string).unwrap_or_default();
        d.firmware = dc.get("Product Revision Level").and_then(cf_string).unwrap_or_default();
        match dc.get("Medium Type").and_then(cf_string).as_deref() {
            Some("Solid State") => d.rotation_rate = Some(1),
            Some("Rotational") => d.rotation_rate = None,
            _ => {}
        }
    }
    if d.model.is_empty() {
        // No product name (virtual disks, some bridges): use the media's registry name.
        if let Some(n) = p.media.name() {
            d.model = n.strip_suffix(" Media").unwrap_or(&n).to_string();
        }
    }
    let ic = p
        .device
        .prop("Protocol Characteristics")
        .and_then(|pc| pc.get("Physical Interconnect").and_then(cf_string))
        .unwrap_or_default();
    let nvme = p.device.search_up("NVMe SMART Capable").and_then(|v| v.bool()) == Some(true)
        || p.device.conforms(c"IONVMeBlockStorageDevice");
    d.interface = match ic.as_str() {
        "SATA" => "Serial ATA".into(),
        "USB" => "USB".into(),
        "Thunderbolt" => "Thunderbolt".into(),
        "SD" | "Secure Digital" => "SD/MMC".into(),
        _ if nvme => "NVM Express".into(),
        "" => "Unknown".into(),
        other => other.to_string(),
    };
    d.kind = Some(if nvme {
        DiskKind::Nvme
    } else if d.interface == "Serial ATA" {
        DiskKind::Ata
    } else {
        DiskKind::Other
    });
    if nvme {
        d.rotation_rate = Some(1);
        d.transfer_mode = pcie_mode(&p.device);
    }
    d
}

/// The registry entry (the device or one of its ancestors) carrying `key = true`.
fn smart_service(dev: &Obj, key: &str) -> Option<Obj> {
    let mut e = dev.try_clone();
    for _ in 0..16 {
        if e.prop(key).and_then(|v| v.bool()) == Some(true) {
            return Some(e);
        }
        e = e.parent()?;
    }
    None
}

fn read_nvme(service: &Obj, d: &mut Disk) -> Result<(), String> {
    let pi = PlugIn::open(service, kIONVMeSMARTUserClientTypeID, kIONVMeSMARTInterfaceID)?;
    let mut id = vec![0u8; 4096];
    let mut log = vec![0u8; 512];
    // SAFETY: interface type matches the ID; buffers are the sizes the API expects
    // (struct nvme_id_ctrl = 4096 bytes, struct nvme_smart_log = 512 bytes).
    unsafe {
        let v = pi.vtbl::<IONVMeSMARTInterface>();
        let id_ok = (v.get_identify_data)(pi.this(), id.as_mut_ptr(), 0) == kIOReturnSuccess;
        let r = (v.smart_read_data)(pi.this(), log.as_mut_ptr());
        if r != kIOReturnSuccess {
            return Err(format!("NVMe SMART read failed (IOKit 0x{:08X})", r as u32));
        }
        // Apple's internal SSDs may leave strings empty; keep the registry ones then.
        let (model, serial, firmware) = (d.model.clone(), d.serial.clone(), d.firmware.clone());
        assemble::apply_nvme(d, id_ok.then_some(id.as_slice()), &log)?;
        if d.model.is_empty() {
            d.model = model;
        }
        if d.serial.is_empty() {
            d.serial = serial;
        }
        if d.firmware.is_empty() {
            d.firmware = firmware;
        }
    }
    Ok(())
}

fn read_ata(service: &Obj, d: &mut Disk) -> Result<(), String> {
    let pi = PlugIn::open(service, kIOATASMARTUserClientTypeID, kIOATASMARTInterfaceID)?;
    let mut identify = vec![0u8; 512];
    let mut vals = vec![0u8; 512];
    let mut thr = vec![0u8; 512];
    // SAFETY: interface type matches the ID; ATASMARTData, ATASMARTDataThresholds and the
    // IDENTIFY page are all 512 bytes.
    unsafe {
        let v = pi.vtbl::<IOATASMARTInterface>();
        let mut got = 0u32;
        let r = (v.get_ata_identify_data)(pi.this(), identify.as_mut_ptr() as *mut c_void, 512, &mut got);
        if r != kIOReturnSuccess {
            return Err(format!("ATA IDENTIFY failed (IOKit 0x{:08X})", r as u32));
        }
        // IOKit returns IDENTIFY words in host order, which is little-endian on every
        // Mac that runs this (Intel and Apple silicon), so the page parses as-is.
        let smart = match (
            (v.smart_read_data)(pi.this(), vals.as_mut_ptr()),
            (v.smart_read_data_thresholds)(pi.this(), thr.as_mut_ptr()),
        ) {
            (kIOReturnSuccess, kIOReturnSuccess) => Some((vals.as_slice(), thr.as_slice())),
            (kIOReturnSuccess, _) => {
                thr.fill(0);
                Some((vals.as_slice(), thr.as_slice()))
            }
            (e, _) => {
                d.smart_error = Some(format!("SMART READ DATA failed (IOKit 0x{:08X})", e as u32));
                None
            }
        };
        let mut exceeded: Boolean = 0;
        let status = (smart.is_some() && (v.smart_return_status)(pi.this(), &mut exceeded) == kIOReturnSuccess)
            .then_some(exceeded != 0);
        assemble::apply_ata(d, &identify, smart, status)
    }
}

/// Lists whole physical disks; reads S.M.A.R.T. if `with_smart`.
pub fn scan(with_smart: bool) -> Vec<Disk> {
    let mounts = mount_map();
    let mut out = Vec::new();
    for p in physical_disks() {
        let mut d = describe(&p, &mounts);
        if with_smart {
            let res = if let Some(svc) = smart_service(&p.device, "NVMe SMART Capable") {
                read_nvme(&svc, &mut d)
            } else if let Some(svc) = smart_service(&p.device, "SMART Capable") {
                let was_usb = d.interface == "USB";
                let r = read_ata(&svc, &mut d);
                if was_usb && r.is_ok() {
                    d.interface = "USB (Serial ATA)".into();
                }
                r
            } else {
                Err(match d.interface.as_str() {
                    "USB" | "Thunderbolt" => format!("{} drives do not expose S.M.A.R.T. to macOS", d.interface),
                    _ => "S.M.A.R.T. not available for this device".into(),
                })
            };
            if let Err(e) = res {
                d.smart_error.get_or_insert(e);
            }
        } else {
            d.smart_error = Some("S.M.A.R.T. not read".into());
        }
        out.push(d);
    }
    out
}
