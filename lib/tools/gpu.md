# Gpu.Tool — data-parallel compute
The same Latte kernel (lib/gpu.lat) runs element-wise over a vector; the host
auto-detects a GPU and falls back to the CPU path, reporting which ran and the
per-element timing. Output embeds below.

dim [field: dim=4096]
[Run](run: gpu dim=$dim) [Small](run: gpu dim=512) [Large](run: gpu dim=65536)

Pair it with the vector jets: `fast %tag` arms in your own packages get the
same treatment — see [adding libraries](run: System.Edit adding-libraries).
