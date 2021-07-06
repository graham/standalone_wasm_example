use std::io::Read;

use wasmer_runtime::{func, instantiate, memory, Array, Ctx, ImportObject, Value, WasmPtr};
use wasmer_runtime_core::import::Namespace;

use bincode;
use reqwest;

fn main() {
    let debug_result = run_wasm("./client/target/wasm32-unknown-unknown/debug/client.wasm");
    println!("Debug result: {}", debug_result);

    let release_result = run_wasm("./client/target/wasm32-unknown-unknown/release/client.wasm");
    println!("Release result: {}", release_result);
}

fn run_wasm(filename: &str) -> i32 {
    let mut import_object = ImportObject::new();
    let mut ns = Namespace::new();

    let get_url = |ctx: &mut Ctx, alloc_fn_ptr: u32, ptr: WasmPtr<u8, Array>, len: u32| -> i32 {
        let slice = read_argument_payload(ctx, ptr, len);

        let url: String = bincode::deserialize(&slice).unwrap();
        let body = reqwest::blocking::get(url).unwrap().text().unwrap();

        let payload = bincode::serialize(&body).unwrap();
        let ptr = write_response_to_memory(ctx, alloc_fn_ptr, payload);

        ptr
    };

    ns.insert("unsafe_get_url", func!(get_url));
    import_object.register("env", ns);

    let mut f =
        std::fs::File::open(&filename).expect(format!("File not found: {}", filename).as_str());
    let metadata = std::fs::metadata(&filename).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");
    let b: &[u8] = &buffer;

    let instance = instantiate(b, &import_object).unwrap();

    let result = instance.call("doit", &[]).unwrap();
    match result.to_vec()[0] {
        Value::I32(i) => i,
        _ => -1,
    }
}

fn read_argument_payload(ctx: &mut Ctx, ptr: WasmPtr<u8, Array>, len: u32) -> Vec<u8> {
    let memory = ctx.memory(0);
    let view: memory::MemoryView<u8> = memory.view();
    let slice: Vec<_> = view
        .get((ptr.offset() as usize)..(ptr.offset() as usize + len as usize))
        .unwrap()
        .iter()
        .map(|i| i.get())
        .collect();

    drop(view);
    drop(memory);

    slice
}

fn write_response_to_memory(ctx: &mut Ctx, alloc_fn_ptr: u32, payload: Vec<u8>) -> i32 {
    let alloc_fn = unsafe { std::mem::transmute(alloc_fn_ptr) };
    let memory_required = payload.len();
    let arguments = [Value::I32(memory_required as i32)];
    let result = ctx.call_with_table_index(alloc_fn, &arguments);

    match result {
        Ok(v) => {
            if let Value::I32(ptr) = v.to_vec()[0] {
                let memory = ctx.memory(0);
                let view: memory::MemoryView<u8> = memory.view();

                let mut target_index = ptr as usize;
                for byte in payload.iter() {
                    view[target_index].set(*byte);
                    target_index += 1;
                }
                return ptr;
            }
        }
        Err(e) => {
            println!("{:?}", e);
        }
    }

    return 0;
}
