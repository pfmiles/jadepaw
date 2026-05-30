(module
  ;; Import host functions from "jadepaw" namespace (D-02)
  (import "jadepaw" "log_message" (func $log_message (param i32 i32 i32 i32) (result i32)))
  (import "jadepaw" "file_read" (func $file_read (param i32 i32 i32 i32) (result i32)))
  (import "jadepaw" "file_write" (func $file_write (param i32 i32 i32 i32) (result i32)))

  ;; 1 page = 64KB of linear memory
  (memory (export "memory") 2)

  ;; Store a string at a given offset. Returns the length written.
  ;; We use hardcoded data for simplicity in WAT.

  ;; Data segments: pre-loaded strings
  ;; offset 0: "info" (4 bytes) — level for log_message
  (data (i32.const 0) "info")
  ;; offset 16: "hello from guest" (16 bytes) — message for log_message
  (data (i32.const 16) "hello from guest")
  ;; offset 64: "test_file.txt" (13 bytes) — path for file_read
  (data (i32.const 64) "test_file.txt")
  ;; offset 128: "../outside" (10 bytes) — traversal path
  (data (i32.const 128) "../outside")
  ;; offset 192: "write test data" (15 bytes) — data for file_write
  (data (i32.const 192) "write test data")
  ;; offset 256: "output.txt" (10 bytes) — path for file_write
  (data (i32.const 256) "output.txt")

  ;; Export: test_log_message — call jadepaw.log_message("info", "hello from guest")
  (func (export "test_log_message") (result i32)
    i32.const 0    ;; level_ptr
    i32.const 4    ;; level_len
    i32.const 16   ;; msg_ptr
    i32.const 16   ;; msg_len
    call $log_message
    return
  )

  ;; Export: test_file_read — call jadepaw.file_read("test_file.txt", buf, buf_len)
  (func (export "test_file_read") (result i32)
    i32.const 64   ;; path_ptr ("test_file.txt")
    i32.const 13   ;; path_len
    i32.const 320  ;; buf_ptr (at offset 320)
    i32.const 1024 ;; buf_len (1KB buffer)
    call $file_read
    return
  )

  ;; Export: test_file_read_traversal — call jadepaw.file_read("../outside", buf, buf_len)
  (func (export "test_file_read_traversal") (result i32)
    i32.const 128  ;; path_ptr ("../outside")
    i32.const 10   ;; path_len
    i32.const 320  ;; buf_ptr
    i32.const 1024 ;; buf_len
    call $file_read
    return
  )

  ;; Export: test_file_write — call jadepaw.file_write("output.txt", "write test data")
  (func (export "test_file_write") (result i32)
    i32.const 256  ;; path_ptr ("output.txt")
    i32.const 10   ;; path_len
    i32.const 192  ;; data_ptr ("write test data")
    i32.const 15   ;; data_len
    call $file_write
    return
  )

  ;; Export: test_file_write_traversal — call jadepaw.file_write("../outside", data)
  (func (export "test_file_write_traversal") (result i32)
    i32.const 128  ;; path_ptr ("../outside")
    i32.const 10   ;; path_len
    i32.const 192  ;; data_ptr
    i32.const 5    ;; data_len (short)
    call $file_write
    return
  )

  ;; Export: _start (noop, required by some instantiation paths)
  (func (export "_start"))
)