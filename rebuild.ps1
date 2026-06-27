Set-Location "C:\Users\uttam\development\turnix"
$env:RUSTFLAGS="-C link-arg=-Tkernel/linker.ld -C link-arg=-z -C link-arg=max-page-size=0x1000 -C relocation-model=static"
cargo -Zbuild-std=core,alloc build -p turnix-kernel --bin turnix-kernel-image --features build-image --target x86_64-unknown-none 2>&1 | Select-Object -Last 10
