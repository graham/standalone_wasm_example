use std::mem;
use std::slice;

use bincode;

use serde::{Deserialize, Serialize};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

extern "C" {
    pub fn unsafe_get_url(
        alloc_fn: extern "C" fn(usize) -> *const usize,
        ptr: *mut usize,
        len: usize,
    ) -> u32;
}

#[no_mangle]
pub extern "C" fn alloc(size: usize) -> *const usize {
    let mut buf = Vec::with_capacity(size);
    let ptr = buf.as_mut_ptr();
    mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut usize, size: usize) {
    unsafe {
        let _ = Vec::from_raw_parts(ptr, 0, size);
    }
}

pub fn read_mem_to_value<'a, T>(ptr: u32) -> T
where
    T: Deserialize<'a>,
{
    unsafe {
        let slice = slice::from_raw_parts(ptr as _, 1024 * 4096);
        bincode::deserialize(&slice[..]).unwrap()
    }
}

pub fn write_value_to_mem<T>(value: T) -> (*mut usize, usize)
where
    T: Serialize,
{
    let len = bincode::serialized_size(&value).unwrap();
    let mut buf = bincode::serialize(&value).unwrap();
    let ptr = buf.as_mut_ptr();
    mem::forget(buf);
    (ptr as *mut usize, len as usize)
}

pub fn get_url(url: &str) -> String {
    let (ptr, len) = write_value_to_mem(url);
    let resp = unsafe { unsafe_get_url(alloc, ptr, len) };
    let body: String = read_mem_to_value(resp as u32);

    body
}

#[no_mangle]
pub fn doit() -> i32 {
    let body = get_url("http://www.google.com/");
    println!("body: {}", body);

    body.len() as i32
}
