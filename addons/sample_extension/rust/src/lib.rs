//! Sample Rust GDExtension.
//!
//! Optional acceleration / FFI showcase addon. The base `powertool` addon does
//! not depend on anything in this crate — if the binary is absent or the
//! plugin is disabled, the rest of the project runs unchanged. See
//! `addons/sample_extension/plugin.gd` for the defensive registration check
//! and `~/Desktop/sho/docs/plans/native-split.md` for the design pattern.

#![allow(clippy::needless_pass_by_value)] // Godot bindings require pass-by-value
#![allow(clippy::unused_self)] // #[func] methods must take &self
#![allow(clippy::missing_const_for_fn)] // Godot methods can't be const

use godot::prelude::*;

struct SampleExtension;

#[gdextension]
unsafe impl ExtensionLibrary for SampleExtension {}

/// Trivial RefCounted class that demonstrates exposing Rust to GDScript.
///
/// From GDScript, after enabling the "Sample Rust Extension" plugin:
///
/// ```gdscript
/// var greeter := SampleGreeter.new()
/// print(greeter.greet("World"))     # -> "Hello, World!"
/// print(greeter.fibonacci(40))       # -> 102334155
/// ```
#[derive(GodotClass)]
#[class(init, base = RefCounted)]
struct SampleGreeter;

#[godot_api]
impl SampleGreeter {
    /// String round-trip: GString in, GString out.
    #[func]
    fn greet(&self, name: GString) -> GString {
        GString::from(&format!("Hello, {name}!"))
    }

    /// Iterative Fibonacci with saturating arithmetic. A stand-in for the kind
    /// of numeric hot path where moving from GDScript to Rust is worth it.
    #[func]
    fn fibonacci(&self, n: i64) -> i64 {
        if n < 2 {
            return n;
        }
        let (mut a, mut b) = (0_i64, 1_i64);
        for _ in 2..=n {
            (a, b) = (b, a.saturating_add(b));
        }
        b
    }
}
