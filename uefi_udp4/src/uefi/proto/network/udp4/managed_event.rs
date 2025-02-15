use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::NonNull;
use uefi::Event;
use uefi_raw::table::boot::{EventType, Tpl};

pub struct ManagedEvent {
    pub event: Event,
    boxed_closure: *mut (dyn FnMut(Event) + 'static),
}

/// Higher level modelling on top of the thin wrapper that uefi-rs provides.
/// The wrapper as-is can't be used because the wrapper can be cheaply cloned and passed around,
/// whereas we need there to be a single instance per event (so the destructor only runs once).
impl ManagedEvent {
    pub fn new<F>(event_type: EventType, callback: F) -> Self
    where
        F: FnMut(Event) + 'static,
    {
        let boxed_closure = Box::into_raw(Box::new(callback));
        unsafe {
            let event = uefi::boot::create_event(
                event_type,
                Tpl::CALLBACK,
                Some(call_closure::<F>),
                Some(NonNull::new(boxed_closure as *mut _ as *mut c_void).unwrap()),
            )
            .expect("Failed to create event");
            Self {
                event,
                boxed_closure,
            }
        }
    }

    pub fn wait(&self) -> uefi::Result<usize, Option<usize>> {
        // Safety: The event clone is discarded after being passed to the UEFI function.
        unsafe { uefi::boot::wait_for_event(&mut [self.event.unsafe_clone()]) }
    }

    pub fn wait_for_events(events: &[&Self]) -> uefi::Result<usize, Option<usize>> {
        // Safety: The event clone is discarded after being passed to the UEFI function.
        Ok(unsafe {
            uefi::boot::wait_for_event(
                &mut events
                    .iter()
                    .map(|e| e.event.unsafe_clone())
                    .collect::<Vec<Event>>(),
            )?
        })
    }
}

impl Drop for ManagedEvent {
    fn drop(&mut self) {
        unsafe {
            // Close the UEFI handle
            // Safety: We're dropping the event here and don't use the handle again after
            // passing it to the UEFI function.
            uefi::boot::close_event(self.event.unsafe_clone()).expect("Failed to close event");
            // *Drop the box* that carries the closure.
            let x = Box::from_raw(self.boxed_closure);
            drop(x);
        }
    }
}

unsafe extern "efiapi" fn call_closure<F>(event: Event, raw_context: Option<NonNull<c_void>>)
where
    F: FnMut(Event) + 'static,
{
    let unwrapped_context = cast_ctx(raw_context);
    let callback_ptr = unwrapped_context as *mut F;
    let callback = &mut *callback_ptr;
    callback(event);
    // Safety: *Don't drop the box* that carries the closure yet, because
    // the closure might be invoked again.
}

unsafe fn cast_ctx<T>(raw_val: Option<NonNull<c_void>>) -> &'static mut T {
    let val_ptr = raw_val.unwrap().as_ptr() as *mut c_void as *mut T;
    &mut *val_ptr
}
