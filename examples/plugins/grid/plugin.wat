;; Grid: draws a 4x3 grid of rounded cards in the current page.
;; A complete Deesainer plugin in WebAssembly text. See docs/plugins.md.
(module
  (import "deesainer" "call" (func $call (param i32 i32 i32 i32) (result i32)))
  (import "deesainer" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "node.create.rectangle")
  (data (i32.const 32) "Grid plugin done")
  ;; JSON params are written at offset 1024 by $params.

  ;; Writes the decimal digits of $n at $at; returns the number of bytes.
  (func $itoa (param $n i32) (param $at i32) (result i32)
    (local $len i32) (local $tmp i32) (local $i i32)
    (if (i32.eqz (local.get $n))
      (then (i32.store8 (local.get $at) (i32.const 48)) (return (i32.const 1))))
    (local.set $tmp (local.get $n))
    (block $done (loop $count
      (br_if $done (i32.eqz (local.get $tmp)))
      (local.set $len (i32.add (local.get $len) (i32.const 1)))
      (local.set $tmp (i32.div_u (local.get $tmp) (i32.const 10)))
      (br $count)))
    (local.set $i (local.get $len))
    (block $out (loop $write
      (br_if $out (i32.eqz (local.get $i)))
      (local.set $i (i32.sub (local.get $i) (i32.const 1)))
      (i32.store8 (i32.add (local.get $at) (local.get $i))
        (i32.add (i32.const 48) (i32.rem_u (local.get $n) (i32.const 10))))
      (local.set $n (i32.div_u (local.get $n) (i32.const 10)))
      (br $write)))
    (local.get $len))

  ;; Copies a static ASCII string; returns the new cursor.
  (func $put (param $at i32) (param $s i32) (param $len i32) (result i32)
    (memory.copy (local.get $at) (local.get $s) (local.get $len))
    (i32.add (local.get $at) (local.get $len)))

  (data (i32.const 64) "{\"x\":")
  (data (i32.const 80) ",\"y\":")
  (data (i32.const 96) ",\"width\":120,\"height\":80,\"fill\":\"#FF7A59\"}")

  (func (export "run")
    (local $row i32) (local $col i32) (local $p i32)
    (loop $rows
      (local.set $col (i32.const 0))
      (loop $cols
        (local.set $p (call $put (i32.const 1024) (i32.const 64) (i32.const 5)))
        (local.set $p (i32.add (local.get $p) (call $itoa (i32.mul (local.get $col) (i32.const 140)) (local.get $p))))
        (local.set $p (call $put (local.get $p) (i32.const 80) (i32.const 5)))
        (local.set $p (i32.add (local.get $p) (call $itoa (i32.mul (local.get $row) (i32.const 100)) (local.get $p))))
        (local.set $p (call $put (local.get $p) (i32.const 96) (i32.const 42)))
        (drop (call $call (i32.const 0) (i32.const 21) (i32.const 1024) (i32.sub (local.get $p) (i32.const 1024))))
        (local.set $col (i32.add (local.get $col) (i32.const 1)))
        (br_if $cols (i32.lt_u (local.get $col) (i32.const 4))))
      (local.set $row (i32.add (local.get $row) (i32.const 1)))
      (br_if $rows (i32.lt_u (local.get $row) (i32.const 3))))
    (call $log (i32.const 32) (i32.const 16))))
