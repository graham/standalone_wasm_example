cd client;

cargo build
cargo build --release

cd ../

RUST_BACKTRACE=1 cargo run

