use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use wasmer_runtime::types::TableIndex;
use wasmer_runtime::{
    func, instantiate, memory, Array, Ctx, ImportObject, Instance, Value, WasmPtr,
};
use wasmer_runtime_core::import::Namespace;

use bincode;
use httparse;

fn main() {
    let debug_arc = Arc::new(Mutex::new(String::new()));
    let release_arc = Arc::new(Mutex::new(String::new()));
    let (debug_instance,) = run_wasm(
        "./client/target/wasm32-unknown-unknown/debug/client.wasm",
        Arc::clone(&debug_arc),
    );
    let (release_instance,) = run_wasm(
        "./client/target/wasm32-unknown-unknown/release/client.wasm",
        Arc::clone(&release_arc),
    );

    let addr = "127.0.0.1:22222";
    let server_sock = TcpListener::bind(addr).unwrap();

    println!("listening...");

    loop {
        for stream in server_sock.incoming() {
            let mut stream = stream.unwrap();
            let mut buffer = [0; 1024 * 4];

            stream.read(&mut buffer).unwrap();

            //let tok = "\r\n\r\n".as_bytes();
            //let index = buffer.windows(4).position(|w| w == tok);

            let mut headers = [httparse::EMPTY_HEADER; 20];
            let mut req = httparse::Request::new(&mut headers);
            let _phead = req.parse(&buffer);

            //println!("Header: {:?}", req.path);

            stream.write_all(b"HTTP/1.1 200 OK\r\n").unwrap();
            stream.write_all(b"Content-Type: text/plain\r\n").unwrap();
            stream.write_all(b"Connection: close\r\n").unwrap();
            stream.write_all(b"\r\n").unwrap();

            debug_instance.call("doit", &[]);
            let g: String = debug_arc.lock().unwrap().clone();
            stream.write_all(g.as_bytes()).unwrap();
            drop(g);

            stream.write_all(b"\n\n").unwrap();

            release_instance.call("doit", &[]);
            let g: String = release_arc.lock().unwrap().clone();
            stream.write_all(g.as_bytes()).unwrap();
        }
    }
}

fn run_wasm(filename: &str, arc: Arc<Mutex<String>>) -> (Instance,) {
    let mut import_object = ImportObject::new();
    let mut ns = Namespace::new();

    let data = Arc::clone(&arc);

    let set_response = move |ctx: &mut Ctx,
                             _alloc_fn_ptr: u32,
                             dealloc_fn_ptr: u32,
                             ptr: WasmPtr<u8, Array>,
                             len: u32|
          -> i32 {
        let slice = read_argument_payload(ctx, dealloc_fn_ptr, ptr, len);
        let url: String = bincode::deserialize(&slice).unwrap();
        let mut g = data.lock().unwrap();
        *g = String::from(url).clone();

        0
    };

    let unsafe_log = |ctx: &mut Ctx,
                      _alloc_fn_ptr: u32,
                      dealloc_fn_ptr: u32,
                      ptr: WasmPtr<u8, Array>,
                      len: u32|
     -> i32 {
        let slice = read_argument_payload(ctx, dealloc_fn_ptr, ptr, len);
        let msg: String = bincode::deserialize(&slice).unwrap();
        println!("LOG:\t {}", msg);

        0
    };

    let fname: String = String::from(filename);
    let get_url = move |ctx: &mut Ctx,
                        alloc_fn_ptr: u32,
                        dealloc_fn_ptr: u32,
                        ptr: WasmPtr<u8, Array>,
                        len: u32|
          -> i32 {
        let slice = read_argument_payload(ctx, dealloc_fn_ptr, ptr, len);

        let url: String = bincode::deserialize(&slice).unwrap();
        let body: String = String::from(format!("hello world: {} {}", fname, url));

        let payload = bincode::serialize(&body).unwrap();
        let ptr = write_response_to_memory(ctx, alloc_fn_ptr, payload);

        ptr
    };

    ns.insert("unsafe_log", func!(unsafe_log));
    ns.insert("set_response", func!(set_response));
    ns.insert("unsafe_get_url", func!(get_url));
    import_object.register("env", ns);

    let mut f =
        std::fs::File::open(&filename).expect(format!("File not found: {}", filename).as_str());
    let metadata = std::fs::metadata(&filename).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");
    let b: &[u8] = &buffer;

    let instance = instantiate(b, &import_object).unwrap();

    (instance,)
}

fn read_argument_payload(
    ctx: &mut Ctx,
    dealloc_fn_ptr: u32,
    ptr: WasmPtr<u8, Array>,
    len: u32,
) -> Vec<u8> {
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

    inner_dealloc(ctx, dealloc_fn_ptr, ptr, len);

    slice.clone()
}

fn write_response_to_memory(ctx: &mut Ctx, alloc_fn_ptr: u32, payload: Vec<u8>) -> i32 {
    let alloc_fn: TableIndex = unsafe { std::mem::transmute(alloc_fn_ptr) };
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
            println!("Write response error {:?}", e);
        }
    }

    return 0;
}

fn inner_dealloc(ctx: &mut Ctx, dealloc_fn_ptr: u32, ptr: WasmPtr<u8, Array>, len: u32) {
    let dealloc_fn: TableIndex = unsafe { std::mem::transmute(dealloc_fn_ptr) };
    let arguments = [Value::I32(ptr.offset() as i32), Value::I32(len as i32)];
    /*
    let result = ctx.call_with_table_index(dealloc_fn, &arguments);

    match result {
        Ok(v) => if let Value::I32(ptr) = v.to_vec()[0] {},
        Err(e) => {
            println!("inner dealloc error {:?}", e);
        }
    }
    */
}
