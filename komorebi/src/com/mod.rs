// This code is largely taken verbatim from this repository: https://github.com/Ciantic/AltTabAccessor
// which the author Jari Pennanen (Ciantic) has kindly made available with the MIT license, available
// in full here: https://github.com/Ciantic/AltTabAccessor/blob/main/LICENSE.txt

mod interfaces;

use interfaces::CLSID_ImmersiveShell;
use interfaces::IApplicationViewCollection;
use interfaces::IServiceProvider;

use std::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoCreateInstance;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoUninitialize;
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

struct ComInit();

impl ComInit {
    pub fn new() -> Self {
        unsafe {
            // Notice: Only COINIT_APARTMENTTHREADED works correctly!
            //
            // Not COINIT_MULTITHREADED or CoIncrementMTAUsage, they cause a seldom crashes in threading tests.
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
        }
        Self()
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

thread_local! {
    static COM_INIT: ComInit = ComInit::new();
}

fn get_iservice_provider() -> IServiceProvider {
    COM_INIT.with(|_| unsafe { CoCreateInstance(&CLSID_ImmersiveShell, None, CLSCTX_ALL).unwrap() })
}

fn get_iapplication_view_collection(provider: &IServiceProvider) -> IApplicationViewCollection {
    COM_INIT.with(|_| {
        let mut obj = std::ptr::null_mut::<c_void>();
        unsafe {
            provider
                .query_service(
                    &IApplicationViewCollection::IID,
                    &IApplicationViewCollection::IID,
                    &mut obj,
                )
                .unwrap();
        }

        assert!(!obj.is_null());

        unsafe { IApplicationViewCollection::from_raw(obj) }
    })
}

#[no_mangle]
pub extern "C" fn SetCloak(hwnd: HWND, cloak_type: u32, flags: i32) {
    COM_INIT.with(|_| {
        let provider = get_iservice_provider();
        let view_collection = get_iapplication_view_collection(&provider);
        let mut view = None;
        unsafe {
            if view_collection.get_view_for_hwnd(hwnd, &mut view).is_err() {
                tracing::error!(
                    "could not get view for hwnd {} due to os error: {}",
                    hwnd.0,
                    std::io::Error::last_os_error()
                );
            }
        };

        view.map_or_else(
            || {
                tracing::error!("no view was found for {}", hwnd.0,);
            },
            |view| {
                unsafe {
                    if view.set_cloak(cloak_type, flags).is_err() {
                        tracing::error!(
                            "could not change the cloaking status for hwnd {} due to os error: {}",
                            hwnd.0,
                            std::io::Error::last_os_error()
                        );
                    }
                };
            },
        );
    });
}
