# Static Opti

I often want to serve static files and I needed a solution that is secure and
efficient. Static opti take your statics files, compress them at their
maximum, precalculate their ETag and dump them in a single file. You can then mmap this file and serve optimized static files with minimal computation
and syscalls.

